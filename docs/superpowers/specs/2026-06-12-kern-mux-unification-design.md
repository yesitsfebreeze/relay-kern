# Kern Mux Unification — Design Spec (Spec A)

**Date:** 2026-06-12
**Status:** Approved
**Scope:** `src/mux/mod.rs`, `src/mux/tui.rs`, `src/mux/research.rs`, `src/mcp/` (`Server`), `src/commands.rs`, `src/commands/mcp_cmd.rs`, `src/config/mux.rs`, `.mcp.json` registration. New: `src/engine/` (or equivalent) shared bootstrap. Deletes: `src/mux/mcp.rs`, `src/mux/kern_client.rs`, `Commands::MuxMcp` / `run_mux_proxy`.

**Follow-on:** Spec B (`raise_question` blocking tool + waiting-sessions overlay) builds on this. Out of scope here.

---

## Overview

The mux is too complex: it spawns a **detached** kern daemon, runs a **second** MCP server (`:7779`) for its own `mux_*` tools, registers a `kern-mux` stdio↔TCP proxy in `.mcp.json`, and the research chat reaches the engine over a **TCP loopback** (`KernClient` → `:7778`). That is two processes, two MCP servers, and two proxies for what is conceptually one thing.

This spec collapses it to **one process**. `kern` (mux mode) builds the engine in-process and *is* the cwd's singleton daemon. The TUI and research chat call the engine through a shared in-process `Arc`. The mux communication tools become kern-native tools on the single MCP surface, reached by panes through the `kern mcp` proxy they already use. The second server and both proxies are deleted.

**Guiding fact:** panes already attach to `kern.sock` in **proxy mode** holding no graph (`src/commands/mcp_cmd.rs`). When the mux is what answers `kern.sock`, panes proxy into the mux unchanged — only the process behind the socket differs.

---

## Section 1: Process Model

`run_mux` stops spawning a detached daemon (`ensure_daemon()` removed from the mux path) and instead brings up the engine in-process, becoming the cwd singleton.

```
              BEFORE                                   AFTER
  ┌─ kern (mux) ───────────┐               ┌─ kern (mux) ───────────────────────┐
  │  :7779 MuxMcpServer     │   :7778       │  Arc<mcp::Server> {                 │
  │  KernClient ──TCP───────┼──► daemon     │     graph, worker, llm, save_fn,    │
  │  ResearchPanel(loopback)│  (engine)     │     task_q, cache,                  │
  └─ panes: kern mcp ───────┴──► :7778      │     mux: Some(registry) }  ◄── TUI  │
        + kern-mux mcp ──► :7779            │  serves kern.sock + viewer + gossip │
                                            └─ panes: kern mcp ──► this process   │
                                                       (proxy mode, unchanged)    │
```

**Singleton contention.** On launch the mux probes `kern.sock`:
- **Free** → mux opens the engine (acquires the data-dir write lock), serves `kern.sock`, viewer, gossip, capture, keepalive, watchdog — everything `run_server` does — and additionally runs the TUI. This is the common case (fresh `kern` launch).
- **Already held** (a headless `kern daemon` owns the cwd) → the mux does **not** double-open the engine (single-writer lock). It attaches to the existing daemon as a client for engine calls; comms tools remain local to the mux. Degraded path; documented, not the primary.

`kern daemon` headless mode is unchanged and remains the option for a TUI-less host.

---

## Section 2: Shared Engine Bring-up (`bootstrap`)

`run_server` (`src/commands.rs:477`) and `run_mux` require the identical engine stack. **This logic is extracted into one shared helper** — it must not be duplicated into the mux (repo law: shared).

New module (e.g. `src/engine/mod.rs`) exposing:

```rust
pub struct EngineHandle {
    pub server:  Arc<crate::mcp::Server>, // graph/worker/llm/save_fn/task_q/cache
    pub graph:   /* GraphHandle */,
    pub worker:  /* WorkerHandle */,
    pub save_fn: /* SaveFn */,
    // whatever the side-services (viewer/gossip/capture) need
}

/// Build the engine stack: Registry::open → mcp::Server, reap empty kerns,
/// spawn viewer + capture + session-mirror + file-watcher + gossip + keepalive
/// + watchdog. Does NOT block/park — caller decides what to run on the main task.
pub async fn bootstrap(cfg: &Config, cli: &Cli, mux: Option<Arc<Mutex<PaneRegistry>>>) -> EngineHandle;
```

- `run_server` = `bootstrap(cfg, cli, None)` then park (existing RPC/serve loop).
- `run_mux`    = `bootstrap(cfg, cli, Some(registry))` then run the TUI with `handle.server`.

The `mux` argument threads the registry into `mcp::Server` so the comms tools are registered and the registry is reachable from tool handlers. Extraction is a pure refactor of existing `run_server` code — behaviour for the headless daemon is unchanged.

---

## Section 3: Engine Handle Reaches the TUI

- `run_mux` retains the `Arc<mcp::Server>` from `bootstrap` and passes it into `run_tui` (new parameter), replacing the removed `kern_mcp_addr: String`.
- `ResearchPanel` stores the `Arc<mcp::Server>` instead of an address. `ResearchPanel::answer` calls the **same in-process query path the MCP `query` tool runs** with `answer=true` — a direct method call on the handle. The dedicated answer thread + `sync_channel` stays (the LLM path is 12–21 s and must not block the render loop), but the thread now calls the in-process engine, not TCP.
- **Delete `src/mux/kern_client.rs`** (`KernClient`, `TcpTransport`). Remove `KernClient::answer` usage from `research.rs`.

The exact `Server` method to call (e.g. a `query`/`answer` entry that the `query` tool dispatches to) is pinned during planning; the design requirement is "in-process, no socket."

---

## Section 4: Comms Tools Become Kern-Native

The tools currently on `MuxMcpServer` (`src/mux/mcp.rs`) move onto `crate::mcp::Server`, gated on `mux.is_some()`:

| Old name        | New name   | Behaviour change |
|-----------------|------------|------------------|
| `mux_delegate`  | `delegate` | Ingests the task via the **in-process** graph/worker (no `KernClient`); spawns a worker pane via the registry; sends the boot message. |
| `mux_collect`   | `collect`  | Queries the result key **in-process**. |
| pane control (`mux_list`/`mux_focus`/`mux_send`/`mux_close` — exact set per current `mcp.rs`) | drop `mux_` prefix | Operate on the in-process registry directly. |

- **Rename approved:** drop the `mux_` prefix on all relocated tools — they are kern tools now. This changes agent-facing tool names; no compat alias is kept (clean base).
- Tool registration: when `Server.mux` is `Some`, the comms tools are added to `tools/list` and dispatchable in `tools/call`; when `None` (headless), they are absent.
- `delegate` boot message (`src/mux/delegate.rs::boot_message`) loses its `kern_mcp_addr` parameter — workers reach the engine through their normal `mcp__kern__query` (the same in-process `Server` via the proxy). `task_key` / `result_key` / key conventions are retained.

---

## Section 5: Deletions

Remove cleanly, no shims:

- `src/mux/mcp.rs` (`MuxMcpServer`) and the `:7779` listener block in `run_mux`.
- `Commands::MuxMcp` arm + `run_mux_proxy` (`src/commands/mcp_cmd.rs`) — the `mcp-mux` bridge.
- The `kern-mux` server entry in `.mcp.json` registration (`ensure_mcp_registered` / the tuple that writes `{"command":"kern","args":["mcp-mux"]}`). Only the `kern` (stdio `kern mcp`) server stays registered.
- `MuxConfig::mcp_addr` (`:7779`) and `MuxConfig::kern_mcp_addr` (`:7778`) — both obsolete. TOML config, **not** bincode shards, so removal is data-safe. Update `MuxConfig::default()` and any tests referencing them.
- `src/mux/kern_client.rs` entirely.

`src/mux/mod.rs` re-exports updated: remove `KernClient`, `MuxMcpServer`; keep `ResearchPanel`, registry, pty, `delegate` helpers.

---

## Section 6: Pane → Engine Path (Unchanged Contract)

Panes still launch the agent (`claude`) with `kern` registered as an MCP server (stdio `kern mcp`). That bridge attaches to the mux's `kern.sock` in proxy mode. The only difference from today: the process answering `kern.sock` is the mux, and its tool surface now includes the comms tools.

**Risk to verify in planning:** the `kern mcp` proxy (`run_proxy` in `mcp_cmd.rs`) must forward **`tools/list`** (so panes *see* `delegate`/`collect`/pane tools) as well as arbitrary **`tools/call`** names. The module doc states it forwards via the typed `call_tool` escape hatch; the plan must confirm `tools/list` reflects the daemon's full list including the comms tools, and add forwarding if it does not.

---

## Section 7: Forward-Compatibility with Spec B

Spec B adds `raise_question` (blocking — the call parks until a human answers via a dedicated overlay). Because the engine `Server` and the `PaneRegistry` now share one process, B drops in as a small in-process **question broker** reachable from both the `raise_question` tool handler and the TUI overlay. Spec A enables this purely by adding the `mux` handle to `Server`; no further hook is required here.

---

## What Does Not Change

- `kern daemon` headless mode (engine bring-up is shared, behaviour identical).
- `bincode`-derived structs — none touched.
- Normal pane layout / `draw_frame` when the research panel is closed.
- Key bindings (`Ctrl+L` research panel) and the journal tailer.
- Version stays `1.0.0`.

---

## Repo-Law Warnings (surfaced)

- **Shared:** engine bring-up extracted to one `bootstrap` helper instead of duplicated into the mux (Section 2). ✓
- **Duplicates:** the relocated comms tools must not leave stale copies — `src/mux/mcp.rs` is deleted, not left alongside the new `Server` tools (Section 5). ✓
- **No compat:** renames drop `mux_` with no aliases; deletions are outright; no fallback shims (Sections 4–5). ✓
- **Pre-existing duplication (noted, out of scope):** `research.rs` duplicates journal formatting from `relay/src/journal_tail.rs` (deliberate in the prior spec). If ever shared, it belongs in `shared/journal/`. Not addressed here.
- **Version:** `1.0.0` unchanged. ✓
