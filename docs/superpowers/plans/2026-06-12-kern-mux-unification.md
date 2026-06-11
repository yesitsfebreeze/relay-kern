# Kern Mux Unification Implementation Plan (Spec A)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Environment note:** subagent fan-out is currently unavailable (kern MCP `mux_delegate` is dead — that is what this plan fixes — and the built-in Agent tool is hook-blocked). Until this lands, execute **inline** via superpowers:executing-plans.

**Goal:** Collapse the mux from "two processes + two MCP servers + two proxies" into one process: `kern` (mux mode) embeds the engine, becomes the cwd singleton daemon, exposes the comms tools on the single kern MCP surface, and runs the research chat in-process.

**Architecture:** `run_server` and `run_mux` share one `engine::bootstrap` helper that builds the `mcp::Server` stack and all side-services. `mcp::Server` gains `mux: Option<Arc<Mutex<PaneRegistry>>>`; when `Some`, it advertises + dispatches the comms tools (`delegate`/`collect`/`spawn`/`send`/`panes`/`status`) which operate on the in-process registry and reuse `self.tool_query`/`self.tool_ingest`. The `kern mcp` proxy forwards a live `list_tools` so panes discover those tools. The `:7779` mux server, the `mcp-mux` bridge, and `KernClient` are deleted.

**Tech Stack:** Rust, `trnsprt` (workspace: `McpServer`, `serve_rw`, `kern_rpc`, `typed::bind_kern_listener`), `tokio`, `serde_json`, `crossterm`.

**Spec:** `docs/superpowers/specs/2026-06-12-kern-mux-unification-design.md`

---

## File Map

| Action | Path | Responsibility |
|--------|------|----------------|
| Create | `src/engine.rs` (or `src/engine/mod.rs`) | `EngineHandle` + `bootstrap(cfg, cli, mux) -> EngineHandle`: the shared engine bring-up extracted from `run_server`. |
| Create | `src/mcp/tools_mux.rs` | `tool_schemas()` for the comms tools + the `Server`-method handlers (`tool_delegate`/`tool_collect`/`tool_spawn`/`tool_send`/`tool_panes`/`tool_status`). |
| Modify | `src/mcp.rs` | add `mux` field to `Server`; `tools_list()` appends mux schemas when `mux.is_some()`; `call_tool` dispatches mux tool names. |
| Modify | `src/commands.rs` | `run_server` calls `engine::bootstrap(cfg, cli, None)`; remove inlined bring-up. |
| Modify | `src/mux/mod.rs` | `run_mux` calls `engine::bootstrap(cfg, cli, Some(registry))`, becomes the kern.sock singleton, runs TUI with the `Arc<Server>`; delete `:7779` listener + `ensure_daemon` use. |
| Modify | `src/mux/tui.rs` | `run_tui` takes `Arc<crate::mcp::Server>` instead of `kern_mcp_addr: String`; pass to `ResearchPanel`. |
| Modify | `src/mux/research.rs` | `ResearchPanel` holds `Arc<Server>`; `answer` thread calls `server.tool_query({answer:true})` in-process. |
| Modify | `src/mux/delegate.rs` | drop `kern_mcp_addr` param from `boot_message`. |
| Modify | `src/commands/mcp_cmd.rs` | `ProxyServer::tools_list` forwards live `list_tools` over kern_rpc; delete `run_mux_proxy`. |
| Modify | `src/rpc.rs` + `trnsprt` kern_rpc | add a `list_tools` kern_rpc method mirroring `call_tool`. |
| Modify | `src/commands.rs` (Cli/Commands) + `src/main.rs` | delete `Commands::MuxMcp` arm. |
| Modify | `src/config/mux.rs` | delete `mcp_addr` + `kern_mcp_addr` fields. |
| Delete | `src/mux/mcp.rs` | `MuxMcpServer` — handlers relocated to `tools_mux.rs`. |
| Delete | `src/mux/kern_client.rs` | `KernClient` + `TcpTransport` — replaced by in-process calls. |

---

## Task 1: Add `mux` handle to `mcp::Server` (no behaviour change)

Add the optional registry handle and thread `None` through every existing construction site so the crate still compiles and the daemon behaves identically.

**Files:**
- Modify: `src/mcp.rs:42-53`
- Modify: `src/commands.rs:575-586` (run_server construction)
- Modify: `src/commands/mcp_cmd.rs:338-349` (run_standalone construction)

- [ ] **Step 1: Add the field**

In `src/mcp.rs`, add to `pub struct Server` after `cache`:

```rust
    /// Present only when this engine is hosted inside the mux TUI process.
    /// `Some` → the comms tools (`delegate`/`collect`/`spawn`/`send`/`panes`/
    /// `status`) are advertised and dispatched against this live pane registry.
    /// `None` → headless daemon; comms tools are absent.
    pub mux: Option<Arc<Mutex<crate::mux::registry::PaneRegistry>>>,
```

- [ ] **Step 2: Set `mux: None` at the two existing construction sites**

`src/commands.rs` (`run_server`, the `crate::mcp::Server { ... }` literal) — add `mux: None,`.
`src/commands/mcp_cmd.rs` (`run_standalone`, the `crate::mcp::Server { ... }` literal) — add `mux: None,`.

- [ ] **Step 3: Build**

```
cargo build -p kern 2>&1 | tail -5
```
Expected: `Finished`, no errors.

- [ ] **Step 4: Commit**

```
git add src/mcp.rs src/commands.rs src/commands/mcp_cmd.rs
git commit -m "feat(mcp): add optional mux registry handle to Server (None everywhere)"
```

---

## Task 2: Relocate comms tools onto `Server` as `tools_mux.rs`

Port the six `MuxMcpServer` handlers into a new module whose handlers are `Server` methods. `delegate`/`collect` now ingest/query **in-process** via `self.tool_ingest`/`self.tool_query` instead of `KernClient`. Rename drops the `mux_` prefix.

**Files:**
- Create: `src/mcp/tools_mux.rs`
- Modify: `src/mcp.rs` (declare module; wire `tools_list` + `call_tool`)

- [ ] **Step 1: Write `tools_mux.rs` schemas + tests**

Create `src/mcp/tools_mux.rs`. Tool names: `delegate`, `collect`, `spawn`, `send`, `panes`, `status` (was `mux_list` → `panes`; `mux_status` → `status`). Schemas mirror `src/mux/mcp.rs::tool_schemas()` with renamed `name` fields. Include a test asserting the six names and required fields (port `mux_delegate_schema_requires_label_and_task`, `mux_collect_schema_requires_session_id` with new names).

```rust
//! Mux communication tools, served by `mcp::Server` only when it hosts a live
//! pane registry (mux mode). Handlers operate on the in-process registry and
//! reuse `Server::tool_ingest` / `Server::tool_query` — no socket, no KernClient.

use serde::Deserialize;
use serde_json::json;

use crate::mcp::{tool_error, tool_result_json, Server};

pub fn tool_schemas() -> Vec<serde_json::Value> {
    vec![
        json!({"name":"delegate","description":"Store a task in kern and spawn a fresh worker pane that boots by querying kern for its assignment. Returns session_id, task_key, result_key.","inputSchema":{"type":"object","required":["label","task"],"properties":{"label":{"type":"string"},"task":{"type":"string"},"cmd":{"type":"string"}}}}),
        json!({"name":"collect","description":"Query kern for the result a worker published under mux:result:<session_id>. Empty string if not yet published.","inputSchema":{"type":"object","required":["session_id"],"properties":{"session_id":{"type":"string"}}}}),
        json!({"name":"spawn","description":"Spawn a new agent sub-pane in the mux TUI.","inputSchema":{"type":"object","required":["label"],"properties":{"label":{"type":"string"},"cmd":{"type":"string"}}}}),
        json!({"name":"send","description":"Write text to a pane's PTY stdin.","inputSchema":{"type":"object","required":["session_id","text"],"properties":{"session_id":{"type":"string"},"text":{"type":"string"}}}}),
        json!({"name":"panes","description":"List all active panes.","inputSchema":{"type":"object","properties":{}}}),
        json!({"name":"status","description":"Get the current visible screen content of a pane.","inputSchema":{"type":"object","required":["session_id"],"properties":{"session_id":{"type":"string"}}}}),
    ]
}
```

- [ ] **Step 2: Port the handlers as `Server` methods**

Add to `tools_mux.rs` an `impl Server { ... }` block. The handlers are the bodies from `src/mux/mcp.rs` with three changes: (a) `self.registry` → `self.mux.as_ref()` (return `tool_error("not running under a mux")` when `None`); (b) `self.agent_cmd` → read from `self.cfg.mux.agent_cmd`; (c) `delegate` ingests via `self.tool_ingest` and `collect` queries via `self.tool_query` (see Step 3).

```rust
#[derive(Deserialize)] struct SpawnArgs { label: String, cmd: Option<String> }
#[derive(Deserialize)] struct SendArgs { session_id: String, text: String }
#[derive(Deserialize)] struct IdArgs { session_id: String }
#[derive(Deserialize)] struct DelegateArgs { label: String, task: String, cmd: Option<String> }
#[derive(Deserialize)] struct CollectArgs { session_id: String }

impl Server {
    fn mux_reg(&self) -> Option<&std::sync::Arc<std::sync::Mutex<crate::mux::registry::PaneRegistry>>> {
        self.mux.as_ref()
    }

    pub(crate) fn tool_spawn(&self, args: &serde_json::Value) -> serde_json::Value {
        let Some(reg) = self.mux_reg() else { return tool_error("not running under a mux") };
        let p: SpawnArgs = match serde_json::from_value(args.clone()) { Ok(v)=>v, Err(e)=>return tool_error(&format!("invalid args: {e}")) };
        let cmd = p.cmd.unwrap_or_else(|| self.cfg.mux.agent_cmd.clone());
        let mut reg = match reg.lock() { Ok(g)=>g, Err(_)=>return tool_error("registry lock poisoned") };
        let (cols, rows) = (reg.cols, reg.rows);
        match reg.spawn_pane(p.label, cmd, cols, rows) {
            Ok(id) => tool_result_json(&json!({"session_id": id})),
            Err(e) => tool_error(&format!("spawn failed: {e}")),
        }
    }
    // tool_send, tool_panes, tool_status: identical ports of tool_send/tool_list/tool_status
    // from src/mux/mcp.rs, with self.mux_reg() guard and tool_result_json for the ok path.
    // (Full bodies: copy from src/mux/mcp.rs lines 180-237, swapping registry access + ok helper.)
}
```

> **Engineer note:** copy the `tool_send`, `tool_list` (→`tool_panes`), and `tool_status` bodies verbatim from `src/mux/mcp.rs:180-237`, replacing `self.registry` with the guarded `reg` from `self.mux_reg()` and `tool_ok(..)` with `tool_result_json(&..)` (the `mcp.rs` ok helper). These are mechanical and have no logic change.

- [ ] **Step 3: `delegate` + `collect` go in-process**

```rust
impl Server {
    pub(crate) fn tool_delegate(&self, args: &serde_json::Value) -> serde_json::Value {
        let Some(reg) = self.mux_reg() else { return tool_error("not running under a mux") };
        let p: DelegateArgs = match serde_json::from_value(args.clone()) { Ok(v)=>v, Err(e)=>return tool_error(&format!("invalid args: {e}")) };
        let cmd = p.cmd.unwrap_or_else(|| self.cfg.mux.agent_cmd.clone());
        let id = {
            let mut r = match reg.lock() { Ok(g)=>g, Err(_)=>return tool_error("registry lock poisoned") };
            let (cols, rows) = (r.cols, r.rows);
            match r.spawn_pane(p.label, cmd, cols, rows) { Ok(id)=>id, Err(e)=>return tool_error(&format!("spawn failed: {e}")) }
        };
        let task_key   = crate::mux::task_key(&id);
        let result_key = crate::mux::result_key(&id);
        // In-process ingest: reuse the same tool path Claude's mcp__kern__ingest uses.
        let ingest_text = format!("[KEY={task_key}]\n{}", p.task);
        let _ = self.tool_ingest(&json!({"text": ingest_text, "source": "agent", "object_id": task_key, "sync": true}));
        let boot = crate::mux::boot_message(&id); // kern_mcp_addr param removed (Task 7)
        if let Ok(mut r) = reg.lock() { let _ = r.send_to(&id, &boot); }
        tool_result_json(&json!({"session_id": id, "task_key": task_key, "result_key": result_key}))
    }

    pub(crate) fn tool_collect(&self, args: &serde_json::Value) -> serde_json::Value {
        if self.mux_reg().is_none() { return tool_error("not running under a mux") }
        let p: CollectArgs = match serde_json::from_value(args.clone()) { Ok(v)=>v, Err(e)=>return tool_error(&format!("invalid args: {e}")) };
        let result_key = crate::mux::result_key(&p.session_id);
        // In-process query for the worker's published result.
        let res = self.tool_query(&json!({"text": result_key, "k": 3}));
        tool_result_json(&json!({"session_id": p.session_id, "result_key": result_key, "result": res}))
    }
}
```

> **Engineer note:** confirm `Server::tool_ingest` and `Server::tool_query` signatures in `src/mcp/tools_mutate.rs` / `src/mcp/tools_query.rs` (both are `fn(&self, &serde_json::Value) -> serde_json::Value`, called from `call_tool`). They are private today — change to `pub(crate)` so `tools_mux.rs` can call them.

- [ ] **Step 4: Wire into `mcp.rs`**

In `src/mcp.rs`: add `pub mod tools_mux;` to the module list. In `tools_list()`:

```rust
fn tools_list(&self) -> Vec<trnsprt::ToolSchema> {
    let mut defs = tools::tool_definitions();
    if self.mux.is_some() { defs.extend(tools_mux::tool_schemas()); }
    defs.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect()
}
```

In `call_tool`, add arms before the `_` fallback:

```rust
"delegate" => self.tool_delegate(args),
"collect"  => self.tool_collect(args),
"spawn"    => self.tool_spawn(args),
"send"     => self.tool_send(args),
"panes"    => self.tool_panes(args),
"status"   => self.tool_status(args),
```

Make `tool_query`/`tool_ingest` `pub(crate)` (Step 3 note).

- [ ] **Step 5: Build + test**

```
cargo test -p kern mcp::tools_mux 2>&1 | tail -10
cargo build -p kern 2>&1 | tail -5
```
Expected: schema tests pass; crate builds. (`Server` still constructed with `mux: None` everywhere, so no live behaviour change yet.)

- [ ] **Step 6: Commit**

```
git add src/mcp.rs src/mcp/tools_mux.rs src/mcp/tools_mutate.rs src/mcp/tools_query.rs
git commit -m "feat(mcp): comms tools on Server gated by mux handle, in-process delegate/collect"
```

---

## Task 3: Extract `engine::bootstrap` shared bring-up

Pull the engine construction out of `run_server` into one helper both entry points call. Pure refactor — headless behaviour unchanged.

**Files:**
- Create: `src/engine.rs`
- Modify: `src/commands.rs` (`run_server` 519-598 → call bootstrap; keep RPC/SSE/repl/park tail)
- Modify: `src/lib.rs` (add `pub mod engine;`)

- [ ] **Step 1: Define `EngineHandle` + `bootstrap`**

Move the body of `run_server` from the `llm_client` setup (`src/commands.rs:519`) through `spawn_maintenance_tick` (`:598`) into `engine::bootstrap`, returning the handles the tail needs:

```rust
pub struct EngineHandle {
    pub server:  std::sync::Arc<crate::mcp::Server>,
    pub graph:   std::sync::Arc<std::sync::RwLock<crate::base::graph::GraphGnn>>,
    pub worker:  std::sync::Arc<crate::ingest::Worker>,
    pub task_q:  std::sync::Arc<crate::tick::queue::Queue>,
    pub save_fn: std::sync::Arc<dyn Fn() + Send + Sync>,
    pub llm:     crate::llm::Client,
}

/// Build the engine stack and spawn every background service (viewer, capture,
/// session-mirror, file-watcher, gossip, keepalive, watchdog, maintenance tick).
/// Does NOT bind kern.sock and does NOT block — the caller owns the serve/park
/// loop and decides whether to attach a TUI. `mux` is threaded into `Server`.
pub async fn bootstrap(
    cfg: &crate::config::Config,
    cli: &crate::Cli,
    mux: Option<std::sync::Arc<std::sync::Mutex<crate::mux::registry::PaneRegistry>>>,
) -> EngineHandle {
    // ... exact lines moved from run_server 519-598, with the mcp::Server
    // literal taking `mux,` instead of `mux: None`. Return EngineHandle { .. }.
}
```

> **Engineer note:** `spawn_watchdog()`/`spawn_keepalive()`/`spawn_viewer()`/`spawn_capture()`/`spawn_session_mirror()`/`spawn_file_watcher()`/`start_gossip()`/`spawn_maintenance_tick()` are free fns in `commands.rs`. Either keep them in `commands.rs` and call `crate::commands::spawn_*` from `engine.rs` (make them `pub(crate)`), or move them alongside. Prefer making them `pub(crate)` and calling from `engine.rs` to minimise churn.

- [ ] **Step 2: `run_server` calls bootstrap**

Replace `run_server`'s 519-598 block with:

```rust
let h = crate::engine::bootstrap(cfg, cli, None).await;
let (g, worker, q, save_fn, llm_client, mcp_server) =
    (h.graph.clone(), h.worker.clone(), h.task_q.clone(), h.save_fn.clone(), h.llm.clone(), h.server.clone());
```

Keep the empty-kern reap, the kern.sock bind (`:606-637`), SSE/stdio/repl/park tail (`:639-674`) exactly as-is.

- [ ] **Step 3: Build + test**

```
cargo build -p kern 2>&1 | tail -5
cargo test -p kern 2>&1 | tail -15
```
Expected: builds; all existing tests pass; `kern daemon` path byte-identical.

- [ ] **Step 4: Smoke test the daemon**

```
cargo run -p kern -- daemon --help 2>&1 | head -3
```
Expected: clap help, no panic.

- [ ] **Step 5: Commit**

```
git add src/engine.rs src/lib.rs src/commands.rs
git commit -m "refactor(engine): extract shared bootstrap() from run_server"
```

---

## Task 4: `run_mux` embeds the engine and becomes the kern.sock singleton

`run_mux` builds the registry first (it already does), calls `bootstrap(cfg, cli, Some(registry))`, binds kern.sock, and runs the TUI with `handle.server`. Deletes the `:7779` listener and the `ensure_daemon()` call.

**Files:**
- Modify: `src/mux/mod.rs` (whole `run_mux`)
- Modify: `src/main.rs` (`run_mux(&cfg)` → `run_mux(&cli, &cfg)` to pass `Cli`)

- [ ] **Step 1: Rewrite `run_mux`**

```rust
pub async fn run_mux(cli: &crate::Cli, cfg: &Config) {
    // Register kern (stdio) in .mcp.json so panes' `kern mcp` bridge attaches here.
    { let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into()); crate::commands::ensure_mcp_registered(&cwd); }

    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let registry = match PaneRegistry::new(cfg.mux.agent_cmd.clone(), cols / 2, rows.saturating_sub(1)) {
        Ok(r)  => Arc::new(Mutex::new(r)),
        Err(e) => { eprintln!("kern mux: failed to spawn main pane: {e}"); return; }
    };

    // Try to become the cwd singleton (own kern.sock). If one already runs,
    // we still launch the TUI; engine calls degrade to the existing daemon.
    let endpoint = trnsprt::typed::Endpoint::kern();
    match trnsprt::typed::bind_kern_listener(&endpoint).await {
        Ok(trnsprt::typed::BindOutcome::Bound(listener)) => {
            let h = crate::engine::bootstrap(cfg, cli, Some(registry.clone())).await;
            let mem = Arc::new(std::sync::Mutex::new(crate::memory_service::MemoryService::new()));
            let handler = crate::rpc::KernRpcHandler::new(h.server.clone(), mem);
            tokio::spawn(crate::rpc::serve_kern_rpc_loop(listener, handler));
            let server = h.server.clone();
            let keymap  = KeyMap::from_config(&cfg.mux);
            let reg_tui = registry.clone();
            let _ = tokio::task::spawn_blocking(move || run_tui(&reg_tui, &keymap, server)).await;
            let g = h.graph.clone(); let g = crate::base::locks::read_recovered(&g); crate::commands::save_graph(&g);
        }
        Ok(trnsprt::typed::BindOutcome::AlreadyRunning) => {
            // Degraded: a headless daemon owns the cwd. Run the TUI; research chat
            // attaches to that daemon over kern.sock via an in-process rpc client.
            // (Spec A §1 fallback. ResearchPanel built with a client handle — see Task 6 note.)
            eprintln!("kern mux: a daemon already owns this cwd; running TUI in attached mode");
            // Minimal path: skip the engine; ResearchPanel uses an rpc client. See Task 6.
            let keymap = KeyMap::from_config(&cfg.mux);
            let reg_tui = registry.clone();
            let _ = tokio::task::spawn_blocking(move || run_tui_attached(&reg_tui, &keymap)).await;
        }
        Err(e) => { eprintln!("kern mux: kern.sock bind failed: {e}"); }
    }
}
```

> **Engineer note:** the degraded `AlreadyRunning` branch is the rare path. To keep Task 4 shippable, implement the `Bound` branch fully and stub `AlreadyRunning` to print + run the normal `run_tui` with research chat disabled (a `None` server). Promote it to a real attached client only if needed — flag to the user before investing.

- [ ] **Step 2: Update `main.rs`**

`None => run_mux(&cli, &cfg).await,` (pass `cli`).

- [ ] **Step 3: Build**

```
cargo build -p kern 2>&1 | tail -15
```
Expected: builds once `run_tui` signature is updated (Task 5) — until then expect a signature mismatch; do Task 5 in the same change if the compiler demands it.

- [ ] **Step 4: Commit**

```
git add src/mux/mod.rs src/main.rs
git commit -m "feat(mux): run_mux embeds engine via bootstrap and owns kern.sock singleton"
```

---

## Task 5: `run_tui` + `ResearchPanel` go in-process

`run_tui` takes `Arc<Server>` instead of `kern_mcp_addr`. `ResearchPanel` stores it; the answer thread calls `server.tool_query({answer:true})` in-process. Delete `KernClient`.

**Files:**
- Modify: `src/mux/tui.rs` (`run_tui` signature + the `ResearchPanel::new` call site)
- Modify: `src/mux/research.rs` (`ResearchPanel` field + `answer` thread)
- Delete: `src/mux/kern_client.rs`
- Modify: `src/mux/mod.rs` (drop `mod kern_client;` + `KernClient` re-export)

- [ ] **Step 1: Read current wiring**

Read `src/mux/tui.rs` (the `run_tui` signature + the `Ctrl+L` toggle block that calls `ResearchPanel::new(kern_mcp_addr.clone())`) and `src/mux/research.rs` (the `kern_mcp_addr` field + the `answer` thread that calls `KernClient::new(addr).answer(&query)`). Confirm exact lines before editing.

- [ ] **Step 2: `run_tui` signature**

```rust
pub fn run_tui(registry: &SharedRegistry, keymap: &KeyMap, server: std::sync::Arc<crate::mcp::Server>) -> io::Result<()> {
```
Pass `server.clone()` into `ResearchPanel::new(server.clone())` at the `Ctrl+L` toggle.

- [ ] **Step 3: `ResearchPanel` holds the server**

In `research.rs`, replace `kern_mcp_addr: String` with `server: std::sync::Arc<crate::mcp::Server>`. The answer thread:

```rust
let server = self.server.clone();
std::thread::Builder::new().name("kern-research-answer".into()).spawn(move || {
    let v = server.tool_query(&serde_json::json!({"text": query, "k": 5, "answer": true}));
    // tool_query returns a {"content":[{"type":"text","text":...}]} value; extract the text.
    let text = v.get("content").and_then(|c| c.as_array()).and_then(|a| a.first())
        .and_then(|b| b.get("text")).and_then(|t| t.as_str())
        .map(str::to_string).ok_or_else(|| anyhow::anyhow!("empty answer"));
    let _ = tx.send(text);
}).expect("spawn kern-research-answer");
```

> Note: `tool_query` is sync and blocking (it drives the LLM path); the dedicated thread keeps it off the render loop. `pub(crate)` was set in Task 2.

- [ ] **Step 4: Delete `kern_client.rs` + re-exports**

```
git rm src/mux/kern_client.rs
```
Remove `mod kern_client;` and `pub use kern_client::KernClient;` from `src/mux/mod.rs`. Remove `KernClient::answer` test from the test module if present.

- [ ] **Step 5: Build + test**

```
cargo build -p kern 2>&1 | tail -15
cargo test -p kern mux::research 2>&1 | tail -10
```
Expected: builds; research tests pass (adjust `ResearchPanel::new` test args to a test `Arc<Server>` or gate those tests behind a constructor that takes the server).

- [ ] **Step 6: Commit**

```
git add -A src/mux
git commit -m "feat(mux): research chat calls engine in-process; delete KernClient"
```

---

## Task 6: Proxy discovers comms tools (live `list_tools` over kern_rpc)

So a pane's Claude *sees* `delegate`/`collect`/etc., `ProxyServer::tools_list` must return the daemon's real list (which includes mux tools when the daemon is a mux), not the static catalogue.

**Files:**
- Modify: `trnsprt` kern_rpc (add `list_tools` request/response)
- Modify: `src/rpc.rs` (`KernRpcHandler` serves it from `mcp_server.tools_list()`)
- Modify: `src/commands/mcp_cmd.rs` (`ProxyServer::tools_list` forwards it)

- [ ] **Step 1: Read the kern_rpc surface**

Read the `trnsprt` kern_rpc module (`KernRpcClient`, `CallToolReq`, the handler trait) and `src/rpc.rs` (`KernRpcHandler`). Confirm how `call_tool` is defined end-to-end — `list_tools` mirrors it.

- [ ] **Step 2: Add `list_tools` to kern_rpc**

Mirror `CallToolReq`/`call_tool` with a `ListToolsReq {}` → `{ tools: Vec<serde_json::Value> }`. Handler returns `self.mcp_server.tools_list()` re-serialised to `serde_json::Value`.

- [ ] **Step 3: `ProxyServer::tools_list` forwards it**

```rust
fn tools_list(&self) -> Vec<ToolSchema> {
    let client = self.client.clone();
    let tools = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async move { client.lock().await.list_tools().await })
    });
    match tools {
        Ok(v) => v.into_iter().filter_map(|t| serde_json::from_value(t).ok()).collect(),
        Err(_) => crate::mcp::tools::tool_definitions().into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect(), // fallback
    }
}
```

- [ ] **Step 4: Build + test**

```
cargo build -p kern 2>&1 | tail -10
cargo test -p kern 2>&1 | tail -15
```

- [ ] **Step 5: Commit**

```
git add -A
git commit -m "feat(rpc): kern_rpc list_tools; proxy reflects daemon's live tool list"
```

---

## Task 7: Delete the `:7779` server, `mcp-mux`, and obsolete config

**Files:**
- Delete: `src/mux/mcp.rs`
- Modify: `src/mux/mod.rs` (drop `pub mod mcp;`, `MuxMcpServer` re-export — already gone after Task 4 removed the listener)
- Modify: `src/mux/delegate.rs` (`boot_message` loses `kern_mcp_addr`)
- Modify: `src/commands/mcp_cmd.rs` (delete `run_mux_proxy` + the mux-proxy section)
- Modify: `src/commands.rs` (delete `Commands::MuxMcp` arm + the `#[command(name="mcp-mux")]` variant)
- Modify: `src/config/mux.rs` (delete `mcp_addr`, `kern_mcp_addr` fields + defaults + tests)
- Modify: `src/commands/mcp_cmd.rs` (`ensure_mcp_registered`: drop the `kern-mux` server tuple; keep only `kern`)

- [ ] **Step 1: `boot_message` drops the addr param**

In `src/mux/delegate.rs`, change `pub fn boot_message(session_id: &str) -> String` and remove the kern-addr line from the template (workers use `mcp__kern__query` with the task key). Update its tests.

- [ ] **Step 2: Remove the mcp-mux command + proxy**

Delete `Commands::MuxMcp` (the `mcp-mux` clap variant in `src/commands.rs`), its dispatch arm (`Commands::MuxMcp => mcp_cmd::run_mux_proxy(...)`), and `run_mux_proxy` + the "Mux MCP proxy" section in `mcp_cmd.rs`.

- [ ] **Step 3: De-register kern-mux**

In `ensure_mcp_registered` (`mcp_cmd.rs:~405`), remove the `("kern-mux", json!({"command":"kern","args":["mcp-mux"]}))` tuple. Update the tests at `mcp_cmd.rs:461/477/517` that assert `kern-mux`.

- [ ] **Step 4: Delete config fields + the old mux server module**

```
git rm src/mux/mcp.rs
```
Remove `mcp_addr`/`kern_mcp_addr` from `MuxConfig`, its `Default`, and the `mux_config_kern_mcp_addr_default` test in `src/config/mux.rs`. Remove `pub mod mcp;` / `MuxMcpServer` from `src/mux/mod.rs`.

- [ ] **Step 5: Build + full test**

```
cargo build -p kern 2>&1 | tail -15
cargo test -p kern 2>&1 | tail -20
```
Expected: builds; all tests pass (the `mux::mcp` tests are gone with the file; new `mcp::tools_mux` tests cover the relocated schemas).

- [ ] **Step 6: Commit**

```
git add -A
git commit -m "feat(mux): delete :7779 server, mcp-mux bridge, and obsolete mux config"
```

---

## Task 8: Final verification

- [ ] **Step 1: Clippy clean**

```
cargo clippy -p kern -- -D warnings 2>&1 | tail -20
```

- [ ] **Step 2: Full suite**

```
cargo test -p kern 2>&1 | tail -20
```

- [ ] **Step 3: Manual smoke (per superpowers:verification-before-completion)**

1. `cargo run -p kern` launches the mux; status bar shows the cwd; no second daemon spawns (check: only one `kern` process owns kern.sock).
2. In a pane, the agent's `mcp__kern__*` list includes `delegate`, `collect`, `spawn`, `send`, `panes`, `status`.
3. `Ctrl+L` opens research chat; a query returns an answer (in-process, no `:7778`/`:7779` connection).
4. `delegate` spawns a worker pane; `collect` returns its published result.
5. `kern daemon` (headless) still works and does NOT advertise the comms tools.

- [ ] **Step 4: Personas review**

Run `/personas` on the completed change (Bjorn for the lock/runtime-safety of the in-process `tool_query` from the TUI thread; Otto for the singleton/lock interplay on kern.sock).

---

## Self-Review

- **Spec §1 process model** → Tasks 3–4 (bootstrap + run_mux singleton). ✓
- **Spec §2 shared bootstrap** → Task 3. ✓
- **Spec §3 engine handle to TUI** → Task 5. ✓
- **Spec §4 comms tools kern-native + rename** → Task 2 (+ discovery Task 6). ✓
- **Spec §5 deletions** → Tasks 5,7. ✓
- **Spec §6 proxy forwards tool list** → Task 6 (verified: proxy serves a static list today; fixed via kern_rpc `list_tools`). ✓
- **Spec §7 forward-compat (mux handle on Server)** → Task 1. ✓
- **Open investigation steps (not placeholders):** Task 2.3 (confirm `tool_ingest`/`tool_query` sigs), Task 5.1 (current tui/research wiring), Task 6.1 (kern_rpc surface). Each is a read-to-confirm before editing the named file — required because those files were not fully read at plan time.
- **Headless cleanliness:** comms tools advertised only when `self.mux.is_some()`; `tool_definitions()` canonical 9-tool set + its guard test untouched. ✓
- **Version `1.0.0`, no bincode structs touched, no compat shims.** ✓
