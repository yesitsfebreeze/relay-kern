//! `KernRpcClient::connect_local` — convenience constructor that dials
//! the per-user `kern.sock` endpoint and wraps it in the JSON-envelope
//! codec.
//!
//! There is no port file. The endpoint is fixed per user (see
//! [`Endpoint::kern`](crate::typed::Endpoint::kern)) so the only
//! coordination kern and its clients need is to agree on the resolver.

use std::time::Duration;

use crate::typed::{
    connect_kern, AdapterError, Channel, Endpoint, JsonEnvelopeCodec,
};

use super::svc::KernRpcClient;

const RETRIES: u32 = 5;
const RETRY_DELAY_MS: u64 = 100;

impl KernRpcClient<JsonEnvelopeCodec> {
    /// Connect to a kern singleton at the per-user endpoint. Caller is
    /// expected to run on a tokio runtime.
    pub async fn connect_local() -> Result<Self, AdapterError> {
        Self::connect_endpoint(&Endpoint::kern()).await
    }

    /// Connect to a kern singleton at an explicit endpoint. Useful for
    /// tests that spawn kern at a private path/pipe name.
    pub async fn connect_endpoint(endpoint: &Endpoint) -> Result<Self, AdapterError> {
        // Short retry loop to absorb the daemon-start race: a client
        // launched alongside `kern --daemon` may dial before the listener
        // is up.
        let mut last_err: Option<AdapterError> = None;
        for _ in 0..RETRIES {
            match connect_kern(endpoint).await {
                Ok(adapter) => {
                    let channel = Channel::new(adapter, JsonEnvelopeCodec::new());
                    return Ok(KernRpcClient::new(channel));
                }
                Err(e) => last_err = Some(e),
            }
            tokio::time::sleep(Duration::from_millis(RETRY_DELAY_MS)).await;
        }
        Err(last_err.unwrap_or_else(|| AdapterError::Other("no endpoint".into())))
    }
}
