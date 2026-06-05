//! MCP transport boundary.
//!
//! Public surface is the abstraction (`McpServer`, `Transport`, `Client`,
//! `Registry`, `InProcTransport`) plus the value types they require. The
//! JSON-RPC wire format is an implementation detail hidden inside `mod server`.

mod client;
mod error;
mod http;
mod inproc;
mod registry;
mod server;
mod transport;
mod types;

pub use client::Client;
pub use error::McpError;
pub use inproc::InProcTransport;
pub use registry::{LiveServer, Registry};
pub use http::serve_http;
pub use server::{serve_rw, serve_stdio, McpServer};
pub use transport::{ChildStdio, Transport};
pub use types::{ServerId, ToolResult, ToolSchema};

pub const PROTOCOL_VERSION: &str = "2024-11-05";

// The `service!` macro emits `::trnsprt::*` paths; when the macro is
// invoked inside this crate (see `mod search`), self-aliasing makes
// those paths resolve.
extern crate self as trnsprt;

// -- Typed-RPC stack (Phase 1, additive). MCP plumbing above is untouched.
pub mod typed;
pub use trnsprt_macros::service;

// -- SearchSvc — typed-RPC search surface. Re-exports the generated
//    trait/client/serve fn through `search::*`.
pub mod search;

// -- KernRpc — typed-RPC surface exposing kern's read+write ops to
//    sub-agents and other clients. Sibling to `search::SearchSvc`;
//    DTOs are intentionally shared where the wire shape overlaps.
pub mod kern_rpc;

#[doc(hidden)]
pub mod __private {
	//! Internals re-exported for the `service!` macro. NOT a stable API.
	pub use bytes;
	pub use futures;
	pub use serde_json;
	pub use tokio;
	pub use tokio_util;
}
