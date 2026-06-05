//! `KernRpc` typed-RPC service definition.
//!
//! Driven by the `service!` proc macro from `trnsprt-macros`. Generates
//! - the `KernRpc` trait (one method per RPC),
//! - `KernRpcClient<C>` that owns a [`Channel`](crate::typed::Channel),
//! - `serve_kern_rpc(channel, handler)` server loop.
//!
//! Sub-agent recipes and the relay TUI hold a `KernRpcClient`; kern (or
//! a mock for tests) implements `KernRpc`. The service is a sibling to
//! [`SearchSvc`](crate::search::SearchSvc) and intentionally shares
//! several DTOs with it (`EntityRef`, `EntityKindLite`, `EdgeKind`,
//! `NeighborsReq`/`Res`) — see `docs/relay-orchestrator-tui.md`
//! decision #5.
//!
//! `fork_at` is **not** part of `KernRpc`. Forks are agnt's concern;
//! routing them through kern would force kern to know about agnt
//! sessions. Slice L will introduce a `fork_at` method on `AgntRpc`.

use super::dto::{
    CallToolReq, CallToolRes, DegradeReq, DegradeRes, DescriptorReq, DescriptorRes, ForgetReq,
    ForgetRes, HealthRes, IngestReq, IngestRes, LinkReq, LinkRes, NeighborsReq, NeighborsRes,
    PulseReq, PulseRes, PurposeReq, PurposeRes, QueryReq, QueryRes, TruncateAfterReq,
    TruncateAfterRes,
};

crate::service! {
    /// Typed-RPC surface exposing kern's read+write operations to
    /// sub-agents and the relay TUI. See
    /// `docs/relay-orchestrator-tui.md` (Architecture / KernRpc).
    pub trait KernRpc {
        /// Retrieval pipeline: ranked hits + optional LLM answer.
        async fn query(req: QueryReq) -> QueryRes;
        /// Ingest text/URI as an Entity. Returns the new entity id
        /// (or a doc id if the call ran async).
        async fn ingest(req: IngestReq) -> IngestRes;
        /// Create a typed Reason edge between two entities.
        async fn link(req: LinkReq) -> LinkRes;
        /// Depth-1 (clamped to 3) typed graph walk. Reuses the same
        /// `NeighborsReq`/`Res` types as `SearchSvc::neighbors`.
        async fn neighbors(req: NeighborsReq) -> NeighborsRes;
        /// Drop in-memory entries with `ts_ms > input` and any
        /// persistent rows after that timestamp. Used by Adjust mode
        /// to roll the memory store back to a chosen point before
        /// re-ingesting a rewritten turn.
        async fn truncate_after(req: TruncateAfterReq) -> TruncateAfterRes;
        /// Hard-delete an entity by id (prefix-matched).
        async fn forget(req: ForgetReq) -> ForgetRes;
        /// Decay confidence on an entity by id (prefix-matched).
        async fn degrade(req: DegradeReq) -> DegradeRes;
        /// Daemon liveness + summary counters.
        async fn health() -> HealthRes;
        /// Set the root kern's purpose text.
        async fn purpose(req: PurposeReq) -> PurposeRes;
        /// Add or remove a descriptor classifier.
        async fn descriptor(req: DescriptorReq) -> DescriptorRes;
        /// Fire a stigmergic pulse through the root kern.
        async fn pulse(req: PulseReq) -> PulseRes;
        /// Generic MCP-style tool dispatch. Forwards the named tool's
        /// arguments to the daemon's `mcp::Server::call_tool` and
        /// returns the full MCP `{ content, isError? }` envelope. Used
        /// by the `kern mcp` proxy subprocess so it can relay stdio
        /// MCP `tools/call` requests over kern.sock without
        /// enumerating each tool as a typed method.
        async fn call_tool(req: CallToolReq) -> CallToolRes;
    }
}
