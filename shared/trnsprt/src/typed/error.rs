//! Errors for the typed-RPC stack.
//!
//! Split into three orthogonal kinds (per design doc):
//! - [`AdapterError`] — byte-level transport (I/O, connect, EOF, kill).
//! - [`CodecError`] — frame parse/encode at the wire format layer.
//! - [`RpcError`] — application-level / generated-stub layer.
//!
//! These are NEW and live alongside the legacy `McpError`. Phase 2 may
//! remove `McpError`; for now both coexist.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("adapter i/o: {0}")]
    Io(#[from] std::io::Error),
    #[error("adapter eof")]
    Eof,
    #[error("adapter codec: {0}")]
    Codec(#[from] CodecError),
    #[error("adapter: {0}")]
    Other(String),
}

#[derive(Debug, Error)]
pub enum CodecError {
    #[error("codec encode: {0}")]
    Encode(String),
    #[error("codec decode: {0}")]
    Decode(String),
}

#[derive(Debug, Error)]
pub enum RpcError {
    #[error("rpc adapter: {0}")]
    Adapter(String),
    #[error("rpc codec: {0}")]
    Codec(String),
    #[error("rpc method not found: {0}")]
    MethodNotFound(String),
    #[error("rpc deadline exceeded")]
    Deadline,
    #[error("rpc application error: {0}")]
    Application(String),
}

impl From<serde_json::Error> for CodecError {
    fn from(e: serde_json::Error) -> Self {
        CodecError::Decode(e.to_string())
    }
}

// `tokio_util::codec::Framed{Read,Write}` lifts the underlying I/O errors
// into the codec's `Error` type, so we have to absorb `io::Error` here.
// Treat I/O at the Decode/Encode boundary as a decode-side failure (the
// `Channel` layer above wraps it back into `AdapterError::Codec`).
impl From<std::io::Error> for CodecError {
    fn from(e: std::io::Error) -> Self {
        CodecError::Decode(format!("io: {e}"))
    }
}
