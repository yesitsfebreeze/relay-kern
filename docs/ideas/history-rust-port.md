# relay: Go to Rust Conversion Plan

> **Historical.** All seven phases are complete; relay ships as Rust.
> Kept for context on why the current shape exists.

## Why Rust?

- **Memory safety without GC** -- eliminates Go's GC pauses, critical for real-time beam search and HNSW queries
- **Zero-cost abstractions** -- traits, generics, and iterators compile to bare-metal performance
- **Fearless concurrency** -- the borrow checker prevents data races at compile time (Go relies on runtime race detection)
- **Smaller binaries** -- static linking without a runtime; current Go binary ~18MB Alpine image can shrink further
- **SIMD/vectorization** -- direct control over vector math for embeddings and tensor ops (GNN, HNSW)
- **No external dependencies policy preserved** -- Rust's stdlib + minimal crates can maintain the "batteries included" philosophy

---

## Current Architecture (Go)

```
src/
  relay/          -- core engine (single module)
    commands/    -- 19 CLI commands
    core/        -- central dispatcher
    env/         -- config loading
    graph/       -- knowledge graph, HNSW, thoughts, reasons, persistence
    ingest/      -- chunking, embedding, placement
    retrieval/   -- beam search, scoring, answer synthesis
    tick/        -- background task queue
    gossip/      -- TCP federation
    gnn/         -- graph neural network (GCN, GAT, GraphSAGE)
    llm/         -- embedding/LLM client
    util/        -- test helpers
    wire/        -- protocol types
  cli/           -- interactive REPL client
  http/          -- HTTP API server
  mcp/           -- MCP JSON-RPC 2.0 server
  tests/         -- integration/e2e tests
```

Zero external Go dependencies. Everything hand-rolled.

---

## Proposed Rust Structure

```
rust/
  Cargo.toml              -- workspace root
  crates/
    base/            -- graph, thoughts, reasons, HNSW, persistence
    ingest/          -- chunking, embedding, placement pipeline
    retrieval/       -- seed, beam search, scoring, answer synthesis
    gnn/             -- GCN, GAT, GraphSAGE, tensor ops, training
    tick/            -- background task scheduler
    gossip/          -- TCP federation protocol
    llm/             -- embedding/LLM HTTP client
    wire/            -- shared protocol types & serialization
    env/             -- config/env loading
    cli/             -- CLI commands (binary)
    repl/            -- interactive REPL client (binary)
    http/            -- HTTP API server (binary)
    mcp/             -- MCP JSON-RPC server (binary)
  tests/                  -- integration/e2e tests
```

### Dependency Strategy

Minimal external crates, keeping the self-contained philosophy:

| Need | Crate | Rationale |
|------|-------|-----------|
| Async runtime | `tokio` | Industry standard, required for network I/O |
| Serialization | `serde` + `serde_json` | De facto standard, replaces `encoding/json` + `encoding/gob` |
| HTTP client | `reqwest` | For LLM/embedding API calls |
| HTTP server | `axum` | Lightweight, tokio-native |
| CLI parsing | `clap` | Derive-based arg parsing |
| Logging | `tracing` | Structured, async-aware |

Everything else (HNSW, GNN, beam search, gossip, MCP) stays hand-rolled.

---

## Conversion Phases

### Phase 1: Foundation (Weeks 1-2) ✅ COMPLETE
**Goal**: Core types compile and basic graph operations work.

- [x] Initialize Cargo workspace
- [x] `wire` -- port all shared types (Thought, Reason, Relay, Sphere, Edge, etc.)
- [x] `env` -- config loading from .env files
- [x] `base` -- graph structure, thought/reason CRUD, basic dispatcher shell
- [x] Persistence -- replace gob encoding with serde/bincode

**Key decisions**:
- Use `Arc<RwLock<>>` for shared graph state (mirrors Go's `sync.RWMutex`)
- Consider `DashMap` for concurrent map access as alternative
- Define `ThoughtId`, `ReasonId`, `RelayId` as newtypes for type safety (deferred -- IDs are bare `String` currently)

### Phase 2: Vector Engine (Weeks 3-4) ✅ COMPLETE
**Goal**: HNSW index and embedding pipeline operational.

- [x] `base/hnsw` -- port HNSW index with SIMD-accelerated cosine similarity
- [x] `llm` -- async HTTP client for Ollama/OpenAI embedding + completion APIs
- [x] `ingest` -- chunking, embedding, graph placement pipeline
- [x] Unit tests matching Go test coverage for HNSW and scoring

**Performance targets**:
- Cosine similarity: 2-5x faster via SIMD (`std::simd` or manual intrinsics)
- HNSW search: match or beat Go baseline
- Embedding pipeline: async parallelism should outperform Go goroutines for I/O-bound work

### Phase 3: Retrieval (Weeks 5-6) ✅ COMPLETE
**Goal**: Full query pipeline working end-to-end.

- [x] `retrieval` -- seed search, beam search, heap, scoring, merge, expand
- [x] Answer synthesis via LLM
- [x] Three retrieval modes (content/reason/hybrid)
- [x] Confidence system and scoring weights
- [ ] Benchmark against Go implementation

### Phase 4: GNN (Weeks 7-8) ✅ COMPLETE
**Goal**: Graph neural network framework ported.

- [x] `gnn` -- tensor operations, matrix math (Tensor: 2D dense, matmul, transpose, row ops)
- [x] GCN, GAT (GATv2 multi-head), GraphSAGE layer implementations
- [x] Backpropagation, Adam + SGD optimizers
- [x] Dropout, LayerNorm, activation functions (ReLU, Sigmoid, Tanh, LeakyReLU, Softmax, LogSoftmax)
- [x] Message passing (Sum/Mean/Max), pooling/readout layers
- [x] Loss functions (MSE, CrossEntropy, NLL, link prediction)
- [x] Model composition with residual skip connections
- [x] Training loop with gradient clipping
- [x] Weight persistence via bincode (marshal/unmarshal)
- [x] GNN propagation module (learned propagation with 2-layer GCN, link prediction self-supervision)
- [x] 56 tests passing, all hand-rolled (no ndarray), SIMD cosine in base crate

**Note**: `propagate.rs` contains pure GNN logic. Integration with live graph will be added in the `tick` crate (Phase 5).

### Phase 5: Background & Networking (Weeks 9-10) ✅
**Goal**: Tick system and gossip federation working.

- [x] `tick` -- async task scheduler (tokio tasks replace Go goroutines) -- 6 modules, 13 tests
- [x] All tick jobs: name, enrich, resolve, persist, pulse, GNN propagation, cluster/crystallise
- [x] `gossip` -- TCP federation with length-prefixed bincode -- 6 modules, 13 tests
- [x] Peer discovery (UDP multicast), sphere broadcasting, ledger with TTL, seen ring buffer
- [x] Two-node TCP communication test passing

### Phase 6: Interfaces (Weeks 11-12) ✅
**Goal**: All external interfaces operational.

- [x] `server` -- axum HTTP API (7 endpoints, metrics, AppState) -- 2 modules, 4 tests
- [x] `mcp` -- MCP JSON-RPC 2.0 server (stdio + SSE transports, 9 tools, 4 resources, 1 prompt) -- 7 modules, 6 tests
- [x] `relay` -- binary crate: 15 CLI commands via clap, REPL, `run` server mode
- [x] Dockerfile -- multi-stage build (rust:alpine -> alpine:3.20)

### Phase 7: Validation & Optimization (Weeks 13-14) ✅
**Goal**: Feature parity confirmed, performance validated.

- [x] Integration test suite -- 27 tests: full pipeline, retrieval, HTTP server, MCP protocol, persistence roundtrip
- [x] Stub embedder (deterministic 4-dim SHA-256 vectors, matches Go's `util.StubEmbedder`)
- [x] Criterion benchmarks -- 7 benchmarks: cosine (151ns), search (119µs), query (134µs), matmul, persist
- [x] `cargo fmt` clean, all 193 tests pass across 14 crates
- [ ] 3-node cluster demo (future)

---

## Rust-Specific Improvements Over Go

### Type Safety
- Newtypes for all IDs (`ThoughtId(Uuid)`, `ReasonId(Uuid)`, etc.) -- prevents mixing
- Enums with data for protocol addresses (`thought://`, `reason://`, `relay://`)
- `Result<T, E>` everywhere -- no silent error swallowing

### Concurrency Model
- `tokio` async for all I/O (HTTP, TCP gossip, LLM calls)
- `rayon` for CPU-bound parallelism (HNSW build, GNN training, batch scoring)
- `Arc<RwLock<>>` or lock-free structures for shared graph state
- Channel-based task queue for tick system (replaces Go channels directly)

### Memory
- Arena allocation for graph nodes (reduce allocator pressure)
- Stack-allocated small vectors for neighbor lists
- Zero-copy deserialization where possible with serde

### Persistence
- Replace gob with `bincode` (compact, fast, schema-aware)
- Memory-mapped file option for large graphs
- Consider `rkyv` for zero-copy deserialization of persisted state

---

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| Async complexity (lifetimes + async) | Keep async at boundaries only; base graph ops stay synchronous |
| Borrow checker friction with graph structures | Use arena allocation or `petgraph`-style indexed graphs |
| Loss of Go's simplicity | Lean on strong type system to prevent bugs; comprehensive tests |
| Timeline slip | Phases are independent enough to parallelize across contributors |
| Feature regression | Run Go and Rust side-by-side during validation; compare outputs |

---

## Getting Started

```bash
cd rust
cargo init --name workspace
# Set up workspace in Cargo.toml
# Begin with Phase 1: wire types
```

## Success Criteria

1. All 19 CLI commands functional
2. All 7 HTTP endpoints operational
3. MCP server passing existing integration tests
4. 3-node gossip cluster working
5. Performance equal or better than Go on all benchmarks
6. Binary size <= Go binary size
7. Memory usage <= Go memory usage under equivalent load
