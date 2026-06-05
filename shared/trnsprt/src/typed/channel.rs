//! `Channel<C>` — pairs a reader and writer half of a transport with a
//! `Codec` to provide framed `send`/`recv`.
//!
//! Backed by `tokio_util::codec::{FramedRead, FramedWrite}`. Our `Codec`
//! trait mirrors `tokio_util::codec::{Encoder, Decoder}`; concrete codec
//! types implement both sides of that bridge so the same codec value can
//! drive read and write halves. Because the read and write halves each
//! need to own their own codec instance, `Channel::new` constructs the
//! second instance via `Codec::Default` — both `JsonEnvelopeCodec` and
//! `BincodeCodec` are zero-sized so this is free.

use futures::{SinkExt, StreamExt};
use tokio_util::codec::{Decoder, Encoder, FramedRead, FramedWrite};

use super::adapter::{Adapter, DynRead, DynWrite};
use super::codec::Codec;
use super::error::{AdapterError, CodecError};

pub struct Channel<C>
where
    C: Codec
        + Default
        + Encoder<<C as Codec>::Frame, Error = CodecError>
        + Decoder<Item = <C as Codec>::Frame, Error = CodecError>,
{
    reader: FramedRead<DynRead, C>,
    writer: FramedWrite<DynWrite, C>,
}

impl<C> Channel<C>
where
    C: Codec
        + Default
        + Encoder<<C as Codec>::Frame, Error = CodecError>
        + Decoder<Item = <C as Codec>::Frame, Error = CodecError>,
{
    pub fn new<A: Adapter>(adapter: A, codec: C) -> Self {
        let (read_half, write_half) = Box::new(adapter).split();
        let reader = FramedRead::new(read_half, codec);
        // Both concrete codecs are zero-sized & default-constructible, so a
        // second instance for the writer side costs nothing.
        let writer = FramedWrite::new(write_half, C::default());
        Self { reader, writer }
    }

    pub async fn send(&mut self, frame: <C as Codec>::Frame) -> Result<(), AdapterError> {
        <FramedWrite<DynWrite, C> as SinkExt<<C as Codec>::Frame>>::send(&mut self.writer, frame)
            .await
            .map_err(adapter_err_from_codec)?;
        Ok(())
    }

    pub async fn recv(&mut self) -> Result<Option<<C as Codec>::Frame>, AdapterError> {
        match self.reader.next().await {
            Some(Ok(f)) => Ok(Some(f)),
            Some(Err(e)) => Err(adapter_err_from_codec(e)),
            None => Ok(None),
        }
    }
}

fn adapter_err_from_codec(e: CodecError) -> AdapterError {
    // `FramedRead`/`FramedWrite` surface either a CodecError or an
    // io::Error wrapped into the codec's Error type. Our codecs use
    // `CodecError` directly, so fold straight into AdapterError::Codec.
    AdapterError::Codec(e)
}

#[cfg(test)]
mod tests {
    use super::super::adapter::InprocAdapter;
    use super::super::codec::JsonEnvelopeCodec;
    use super::Channel;
    use serde_json::json;

    #[tokio::test]
    async fn channel_roundtrip_json_envelope() {
        let (a, b) = InprocAdapter::pair();
        let mut ca = Channel::new(a, JsonEnvelopeCodec::new());
        let mut cb = Channel::new(b, JsonEnvelopeCodec::new());
        ca.send(json!({"hello": "world"})).await.unwrap();
        let got = cb.recv().await.unwrap().unwrap();
        assert_eq!(got["hello"], "world");
    }
}
