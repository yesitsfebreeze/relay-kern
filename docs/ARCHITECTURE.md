# Architecture

**Status: target layout.** The repo is mid-port. Only `kern` is partially
built; `agnt` and `repl` are not yet scaffolded. This doc describes the
end-state every port row converges toward. See `docs/OVERVIEW.md`
"Current state" for what is actually built.

Three binaries in a star: `repl -> agnt -> kern`. Each speaks tarpc to
the next over a per-cwd Unix socket (named pipe on Windows) under
`<cwd>/.relay/`. External plugins speak MCP. For the pitch and
invariants see [`README.md`](../README.md).

## kern — knowledge graph daemon

Stores **thoughts** (factual chunks and extracted claims) and **reasons**
(justified edges between thoughts). Retrieval is vector + lexical, fused
with a GNN layer for learned embeddings. Federation over gossip.

Pipeline: chunk → extract thought → propose reason edges → query by
either dimension → assemble context.

Condensation: vector clusters periodically consolidate into sub-child
databases that stay detached until a query needs them, then lazy-link.
Hot graph stays small; long tail stays cheap.

Persistence is `bincode`. HNSW, GNN, beam search, gossip, MCP — all
hand-rolled. Dependencies deliberately minimal.

Surface: tarpc `KernRpc` (internal, to `agnt`) and MCP (external
clients). One `kern` per cwd; multiple `agnt`s share it.

## agnt — harness daemon

ReAct loop: assemble context → call LLM → dispatch tool calls → observe.
LLM is one participant, not the conductor; deterministic steps dispatch
near-inline and only spend an LLM turn when reasoning is required.

**Plugins are MCP servers.** Stdio subprocess or in-proc Rust
`impl Plugin`. Agent is the MCP client. On top of MCP is an event-hook
layer: declarative TOML binds lifecycle events (`pre_turn`, `post_tool`,
`on_error`, …) to tool calls with priority, size caps, cycle guard.

**Recipes** are reusable declarative workflows (TOML + `pre.md` /
`post.md`) — the authoring entry point for new tool-surface work.

**Journal** lives in `kern`. Source of truth. No sessions; forks are
journal pointers. `/continue` and `/resume` attach to a prior fork tail.

Surface: tarpc `AgntRpc` (to `repl`) and MCP client (to plugins) +
tarpc client (to `kern`).

## repl — TUI

Terminal UI rendered with raw ANSI. Single self-contained binary. Holds
no conversation state — every authoritative read goes to `agnt` over
tarpc; every keystroke that matters dispatches an `AgntRpc` call.

**UI plugins.** `View` surface paints into a clipped `FrameView`,
consumes keys for its region, addressed by `ViewId`. Separate from
`agnt`'s compute-plugin surface; don't bolt one onto the other.

A composer input sits at the bottom. User types a task; `repl` forwards
to `agnt`, streams output back via a subscription.

## Spawn chain

```
`repl` launches
	├─ resolves <cwd>/.relay/agnt.sock
	├─ spawns agnt (detached) if absent
	└─ connects AgntRpc

`agnt` on first tarpc accept
	├─ resolves <cwd>/.relay/kern.sock
	├─ spawns kern (detached) if absent
	└─ connects KernRpc

last `repl` detaches → `agnt` warns → drains → exits → `kern` exits
```

## Data flow

```
terminal -> repl ──tarpc──► agnt ──tarpc──► kern
															│                 │
															▼                 ▼
													MCP plugins      vector + graph
													(stdio/inproc)    walk
```

Event-hook layer wraps both sides of `agnt`: `pre_turn` can pull context
from `kern` before the LLM call; `post_tool` can journal output and
enrich context for the next turn; `on_error` can attach detail the
model sees.
