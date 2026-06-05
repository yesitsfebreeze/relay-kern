//! Codec trait + concrete implementations.
//!
//! A `Codec` is the wire format. Its API mirrors `tokio_util::codec`:
//! `encode(frame, &mut BytesMut)` writes one frame's bytes into `dst`,
//! `decode(&mut BytesMut)` tries to extract one frame from `src`. A
//! blanket impl below converts any `Codec` into the matching
//! `tokio_util::codec::{Encoder, Decoder}` so codecs compose directly
//! with `FramedRead`/`FramedWrite`.

use bytes::BytesMut;
use serde_json::Value;
use tokio_util::codec::{Decoder, Encoder};

use super::error::CodecError;

pub trait Codec: Send + 'static {
    type Frame: Send;
    fn encode(&mut self, frame: Self::Frame, dst: &mut BytesMut) -> Result<(), CodecError>;
    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Frame>, CodecError>;
}

// ---- Bridge to `tokio_util::codec` ------------------------------------------
//
// We can't write a generic blanket `impl<T: Codec> Encoder<T::Frame> for T`
// without orphan-rule grief, so each concrete `Codec` impl below also gets
// an explicit `Encoder` + `Decoder` impl that just delegates to the trait.

/// Line-delimited JSON envelope.
///
/// Frames are `serde_json::Value`. Each frame is encoded as the JSON text
/// of the value followed by a `\n`. Decoding scans for the next `\n` in
/// the buffer. The envelope shape (`{ id, method, params }` /
/// `{ id, result|error }`) is owned by the caller — this codec only
/// shuttles `Value`s.
#[derive(Default)]
pub struct JsonEnvelopeCodec;

impl JsonEnvelopeCodec {
    pub fn new() -> Self {
        Self
    }
}

impl Codec for JsonEnvelopeCodec {
    type Frame = Value;

    fn encode(&mut self, frame: Self::Frame, dst: &mut BytesMut) -> Result<(), CodecError> {
        let s = serde_json::to_string(&frame)
            .map_err(|e| CodecError::Encode(e.to_string()))?;
        if s.contains('\n') {
            return Err(CodecError::Encode("frame contained newline".into()));
        }
        dst.extend_from_slice(s.as_bytes());
        dst.extend_from_slice(b"\n");
        Ok(())
    }

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Frame>, CodecError> {
        if let Some(pos) = src.iter().position(|&b| b == b'\n') {
            let line = src.split_to(pos + 1);
            let slice = &line[..pos];
            let trimmed = if slice.last() == Some(&b'\r') {
                &slice[..slice.len() - 1]
            } else {
                slice
            };
            if trimmed.is_empty() {
                return <Self as Codec>::decode(self, src);
            }
            let v: Value = serde_json::from_slice(trimmed)
                .map_err(|e| CodecError::Decode(e.to_string()))?;
            Ok(Some(v))
        } else {
            Ok(None)
        }
    }
}

impl Encoder<Value> for JsonEnvelopeCodec {
    type Error = CodecError;
    fn encode(&mut self, item: Value, dst: &mut BytesMut) -> Result<(), Self::Error> {
        <Self as Codec>::encode(self, item, dst)
    }
}

impl Decoder for JsonEnvelopeCodec {
    type Item = Value;
    type Error = CodecError;
    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        <Self as Codec>::decode(self, src)
    }
}

/// Length-delimited bincode (bincode 2 standard config).
///
/// Frames are `Vec<u8>` payloads — the caller bincode-encodes their typed
/// envelope into bytes before handing it to the codec. (We could be more
/// clever and parameterise on the frame type, but Phase 1 only needs the
/// JSON codec; this is here as a placeholder for hot-path hops.)
#[derive(Default)]
pub struct BincodeCodec;

impl BincodeCodec {
    pub fn new() -> Self {
        Self
    }
}

impl Codec for BincodeCodec {
    type Frame = Vec<u8>;

    fn encode(&mut self, frame: Self::Frame, dst: &mut BytesMut) -> Result<(), CodecError> {
        let len = u32::try_from(frame.len())
            .map_err(|_| CodecError::Encode("frame too large".into()))?;
        dst.extend_from_slice(&len.to_be_bytes());
        dst.extend_from_slice(&frame);
        Ok(())
    }

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Frame>, CodecError> {
        if src.len() < 4 {
            return Ok(None);
        }
        let mut len_buf = [0u8; 4];
        len_buf.copy_from_slice(&src[..4]);
        let len = u32::from_be_bytes(len_buf) as usize;
        if src.len() < 4 + len {
            return Ok(None);
        }
        let _ = src.split_to(4);
        let payload = src.split_to(len);
        Ok(Some(payload.to_vec()))
    }
}

impl Encoder<Vec<u8>> for BincodeCodec {
    type Error = CodecError;
    fn encode(&mut self, item: Vec<u8>, dst: &mut BytesMut) -> Result<(), Self::Error> {
        <Self as Codec>::encode(self, item, dst)
    }
}

impl Decoder for BincodeCodec {
    type Item = Vec<u8>;
    type Error = CodecError;
    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        <Self as Codec>::decode(self, src)
    }
}
