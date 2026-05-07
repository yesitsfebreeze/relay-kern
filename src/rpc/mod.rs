//! Typed-RPC server modules.
//!
//! Sibling to [`crate::memory_service::MemoryHandler`]. Where
//! `MemoryHandler` implements the legacy `protocol::memory::MemoryRpc`
//! trait, the modules here implement the slice-J `trnsprt::kern_rpc`
//! surface — the typed read+write API consumed by sub-agents and the
//! relay TUI.

pub mod kern_rpc_server;

pub use kern_rpc_server::{kern_rpc_listen, KernRpcHandler};
