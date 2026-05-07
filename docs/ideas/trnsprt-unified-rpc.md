# Unified Transport — Design Doc

Status: proposal
Date: 2026-04-30
Author: design-only round (no code)

## Goal

One communication crate (`shared/trnsprt`) shared by all three binaries
(`agnt`, `kern`, `repl`) so every RPC hop in the system uses the same
abstraction. Make wire format (codec) and byte-stream (adapter) two
orthogonal axes so:

- A service can be hosted over TCP, HTTP, UDP, in-proc, or stdio without
  rewriting the service.
- Mixed topologies are possible (HTTP-frontend, TCP-backbone, in-proc
  unit tests) by bridging adapters that share a codec.
- New transports plug in by implementing one trait.

## Today

```
┌─────────┐   tarpc/bincode/tcp           ┌─────────┐
│  repl   │  ◀──────────────────────────▶│  agnt   │
└─────────┘                               └─────────┘
                                               │
                              MCP-stdio /      │ trnsprt::Client
                              MCP-SSE /        │ (sync, JSON-RPC,
                              local REPL       │ ChildStdio only)
                                               ▼
                                          ┌─────────┐
                                          │  kern   │
                                          └─────────┘
                                               │
                                          raw TcpStream
                                          + custom frame
                                               │
                                               ▼
                                          ┌─────────┐
                                          │  kern   │  (peer gossip)
                                          └─────────┘
```

Three RPC stacks coexist:

| stack         | participants     | transport         | codec        |
|---------------|------------------|-------------------|--------------|
| `tarpc`       | repl ↔ agnt      | bincode over TCP  | bincode      |
| `trnsprt`     | agnt/repl → kern | stdio (ChildStdio)| JSON-RPC     |
| gossip-raw    | kern ↔ kern      | raw TcpStream     | custom       |

`shared/trnsprt` shape today (`src/shared/trnsprt/src/`):

- `Transport` trait: synchronous, exposes `&mut dyn Read` + `&mut dyn Write`
  + `kill()`. Single concrete adapter `ChildStdio` (spawns process, talks
  on stdin/stdout). Plus `InProcServer`/`InProcTransport` for loopback.
- `Client`: hardcodes JSON-RPC envelope and newline framing in `send`/`recv`
  — codec is **not** pluggable; one byte buffer, one `\n` delimiter.
- `jsonrpc::serve` server-loop is `#[doc(hidden)]` re-exported (TODO note
  acknowledges this leaks the wire format across the crate boundary).
- `Registry` + `LiveServer` glue MCP-tool listings to a live client.

Result: trnsprt is a *JSON-RPC-over-stdio* transport, not a generic one.

## Proposed Layering

```
       ┌──────────────────────────────────────────┐
       │ Service traits (MemoryRpc, AgntRpc, ...) │
       └──────────────────────┬───────────────────┘
                              │ async fn calls
                              ▼
       ┌──────────────────────────────────────────┐
       │ Codec  (encode/decode message frames)    │
       │   ── JsonRpcCodec                        │
       │   ── BincodeCodec                        │
       │   ── MsgPackCodec                        │
       └──────────────────────┬───────────────────┘
                              │ Vec<u8> frames
                              ▼
       ┌──────────────────────────────────────────┐
       │ Transport adapter  (deliver bytes)       │
       │   ── TcpAdapter                          │
       │   ── HttpAdapter (POST per request)      │
       │   ── UdpAdapter (best-effort framed)     │
       │   ── StdioAdapter (existing ChildStdio)  │
       │   ── InprocAdapter (channel pair)        │
       │   ── WsAdapter      (future)             │
       └──────────────────────────────────────────┘
```

Each axis is independently swappable. Codec sits between Service and
Adapter — it's the wire format. Adapter is the byte transport.

### Trait sketch

```rust
// async-first; current sync `Transport` becomes an in-proc helper only.
pub trait Adapter: Send + 'static {
    fn split(self: Box<Self>) -> (Box<dyn AsyncRead + Unpin + Send>,
                                  Box<dyn AsyncWrite + Unpin + Send>);
}

pub trait Codec: Send + Sync + 'static {
    type Frame: Send;
    fn encode(&self, frame: Self::Frame) -> Result<Vec<u8>, CodecError>;
    fn decode(&self, bytes: &mut BytesMut) -> Result<Option<Self::Frame>, CodecError>;
}

pub struct Channel<C: Codec> {
    reader: FramedRead<Box<dyn AsyncRead+Unpin+Send>, C>,
    writer: FramedWrite<Box<dyn AsyncWrite+Unpin+Send>, C>,
}
```

Service code (e.g. `MemoryRpc`) is generated from a trait once and dispatches
through `Channel<C>` — it does not see the adapter.

### Mix-and-match

- Same codec, different adapter per direction: client speaks
  `JsonRpcCodec` over `HttpAdapter`, hits a proxy that re-emits over
  `TcpAdapter` to a kern node. Possible because the codec is identical
  and the proxy is a 20-line bridge.
- Same adapter, different codec: not supported (would require codec
  detection on each frame). Not a use case we need.

## Migration Path

### Phase 1 — Memory hop (smallest end-to-end slice)

Goal: agnt's `edit_message` calls `kern.MemoryRpc.truncate_after`. See
**Phase 1 Revised Scope** below for the step list with decisions baked in.

Touchpoints: `shared/trnsprt/{src/codec.rs, src/adapter/{tcp,stdio,inproc}.rs}`,
`shared/trnsprt-macros/` (new crate),
`shared/protocol/src/memory.rs`, `bin/kern/src/commands.rs`,
`bin/agnt/src/{rpc.rs,memory_client.rs}`.

### Phase 2 — Migrate agnt↔repl off tarpc

Once Phase 1 proves the layering, replace the tarpc connection between
agnt and repl with `trnsprt + JsonRpcCodec + TcpAdapter`. Drop `tarpc`,
`tokio-serde`, `bincode` deps from agnt and repl Cargo.toml. Service
definitions for `AgentRpc` move from tarpc-style traits to whatever
codegen we adopt (manual, or a small `service!` macro).

Risk: tarpc's deadline + cancellation semantics need to be reproduced
in the new client. This is a larger task — split out separately.

### Phase 3 — Gossip on top of trnsprt

Replace `kern::gossip::node` raw `TcpStream + decode_msg` with
`Channel<GossipCodec>` over `TcpAdapter`. Gossip's frame format is
already custom binary; wrap it as a `Codec` impl with no other change.
This consolidates kern's two outbound stacks (MCP + gossip) under one
crate.

### Phase 4 — Optional adapters

`HttpAdapter`, `UdpAdapter`, `WsAdapter` added as needs arise. Not on
the critical path. HTTP useful for browser-facing nodes; UDP useful for
gossip if reliable-UDP framing is desired; WebSocket useful for the web
relay UI.

## Non-goals

- Replacing tarpc's macro-generated client surface 1:1. The new
  `service!`-style codegen can be simpler (untyped `Value` payloads
  bridged through codec traits) since we own both ends.
- Streaming RPCs for now. Phase 1–3 is unary call/response. Streams can
  be added once `Channel` is in place.
- Auth/TLS. Add as adapter wrappers later (`TlsAdapter<TcpAdapter>`).

## Decisions

1. **Async-first**. `Adapter` exposes `AsyncRead`/`AsyncWrite` (tokio).
   Sync callers (REPL's `Client`) wrap calls in
   `tokio::runtime::Handle::block_on` on the runtime they already own.
   No sync `Transport` trait survives the migration.
2. **Split error model**.
   - `AdapterError` — I/O, connect, EOF, kill.
   - `CodecError` — frame parse, encode, malformed envelope.
   - `RpcError` — method not found, application error, deadline.
   - The old `McpError` is removed; service code returns
     `Result<T, RpcError>` and adapters surface `AdapterError` upstream.
3. **Macro-first codegen**. A `service!` proc macro under
   `shared/trnsprt-macros` (new crate) generates client + server stubs
   from a service trait declaration. Phase 1 lands the macro alongside
   `MemoryRpc` so subsequent services (`AgentRpc`, `KernRpc`,
   `GossipRpc`) inherit it without per-service boilerplate.
   - Macro input: a trait with `async fn` methods returning
     `Result<T, RpcError>`.
   - Macro output: a `MemoryRpcClient` struct (holds `Channel<C>`,
     each method serialises args, awaits frame match by id), and a
     `serve<H: MemoryRpc>(channel, handler)` server loop.
   - Wire envelope: `{ id: u64, method: &str, params: bytes }` for
     requests, `{ id: u64, result | error: bytes }` for replies. Codec
     decides the outer encoding (JSON-RPC for human-readable hops,
     bincode for hot paths).
4. **No backwards compat**. v1.0.0 in-place rewrites. Drop `tarpc`,
   `tokio-serde`, `bincode` deps from agnt + repl when Phase 2 lands.
   Drop the old `Transport` sync trait when Phase 1 lands. No shim
   crate, no deprecation period.

## Phase 1 Revised Scope (with decisions baked in)

1. Carve `Adapter` (async) + `Codec` traits in `trnsprt`.
2. New crate `shared/trnsprt-macros` with `service!` proc macro.
3. Implement codecs: `JsonRpcCodec`, `BincodeCodec` (used for
   higher-throughput hops once Phase 2 lands).
4. Implement adapters: `TcpAdapter`, `StdioAdapter` (rename of
   `ChildStdio`), `InprocAdapter` (channel pair, kept for tests).
5. Split errors into `AdapterError` / `CodecError` / `RpcError`.
   Remove `McpError`.
6. Define `MemoryRpc` via the new macro under
   `shared/protocol/src/memory.rs`.
7. Kern: TCP listener mounted in `commands::run_server` parallel to
   MCP branches. Discovery file `.relay/kern_memory.port`.
8. Agnt: `MemoryRpcClient` connect on startup with brief retry; stored
   `Option<Arc<MemoryRpcClient>>` on `AgntServer`. `edit_message`
   fires `truncate_after` and swallows errors (logged once).
9. REPL: keep its existing sync `Client` but route it through the new
   `Adapter`/`Codec`/macro stack via `block_on` on the runtime it
   already spins for agnt RPC.

## Phases 2–4 unchanged

(See "Migration Path" above — agnt↔repl swap, gossip migration, optional
adapters.)
