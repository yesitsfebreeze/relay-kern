# Relay — Core Architecture

Status: research / draft
Scope: workspace-wide system design and public API reference.

This document describes the architecture of the `relay` Cargo workspace —
the reusable TUI + agent harness foundation. It is intentionally grounded in
what actually exists in the tree today; speculative subsystems are called out
as such.

> Companion document: `docs/repl/agent-harness.md` covers the
> `harness` crate in depth. This file provides the workspace-level view and
> cross-crate contracts.

---

## 1. System design principles

Relay is built around a small number of non-negotiable principles. Every
module is expected to respect them.

### 1.1 PI-mono

*One small Public Interface per subsystem.* A subsystem exposes the minimum
trait + value types callers need, and hides everything else. The canonical
example is `harness::Plugin` — a single trait plus four value types covers
every external integration (see §4 and `src/harness/src/plugin.rs:86`).

### 1.2 KISS / DRY / YAGNI

The workspace deliberately avoids speculative generality:

- No async runtime until a synchronous path is proven insufficient.
- No dynamic-library plugin loading until a use case demands it
  (`docs/repl/agent-harness.md:106`).
- Shared editing logic lives in *one* place (`textarea::EditArea`,
  `src/bin/repl/src/textarea/edit_area.rs`) and is reused by every text input.

### 1.3 Layered, acyclic crates

Crates depend downward only. The dependency stack, top-down:

```
  demo, downstream consumers
       │
       ▼
  plugin_ui ── harness
       │         │
       ▼         │
  render ──────  │
       │         │
       ▼         │
  textarea ── input
       │         │
       ▼         ▼
                std / crossterm
```

No cycles. Each crate's `Cargo.toml` makes the dependency direction explicit
and is the source of truth (`Cargo.toml:3`).

### 1.4 Raw ANSI, character-grid TUI

The render path is explicit row/column math writing ANSI escape sequences
into a diffed cell grid. There is no widget reflow engine, no DOM, no layout
solver. See `.claude/skills/tui-design/SKILL.md` for the project-wide
constraints this enforces.

### 1.5 Tests inline

Per `CLAUDE.md`, every module ends with `#[cfg(test)] mod tests` in the same
file. This convention is visible throughout the codebase (for example
`src/bin/repl/src/render/lib.rs:289`).

### 1.6 Errors: `thiserror`-style enums at crate boundaries

Public errors are small, exhaustive enums. `anyhow` is reserved for binary
call sites. `PluginError` (`src/harness/src/plugin.rs:61`) and
`PumpError` (`src/bin/repl/src/input/pump.rs`) are the reference shape.

---

## 2. Workspace layout

The workspace manifest at `Cargo.toml:1-4` declares six members:

| Crate                  | Path               | Role                                                 |
|------------------------|--------------------|------------------------------------------------------|
| `render`               | `src/render/`      | Cell grid, frame diffing, ANSI emission, surfaces.   |
| `textarea`             | `src/textarea/`    | Multi-line text buffer, edit operations, history.    |
| `input`                | `src/input/`       | Keyboard/mouse event model, event pump, shortcuts.   |
| `plugin_ui`            | `src/plugin_ui/`   | View trait + text-field widget for plugin UIs.       |
| `harness`              | `src/harness/`     | Plugin trait, registry, runtime (agent scaffolding). |
| `demo`                 | `src/demo/`        | Unified integration demo binary.                     |

`src/render/fuzz`, `src/input/fuzz`, and `src/textarea/fuzz` are fuzz targets
and are excluded from the workspace (`Cargo.toml:4`). Benches live under
`src/render/benches/`.

---

## 3. Rendering subsystem (`render`)

Source: `src/bin/repl/src/render/`.

### 3.1 Purpose

`render` owns the terminal presentation layer: an in-memory cell grid, a
diff engine, ANSI emission, and a `Surface` abstraction over the sink.

### 3.2 Public surface

Re-exported from `src/bin/repl/src/render/lib.rs:16-25`:

- `Renderer` — top-level frame lifecycle: `new`, `resize`, `frame`,
  `put_str`, `flush`, `present`, `snapshot`, `restore`, `add_pass`.
- `Frame`, `Cell`, `Attrs`, `Color` — the grid model.
- `Surface` trait (`StdoutSurface`, `BufferSurface`) with a
  `Capabilities` struct that advertises terminal features such as DEC
  `?2026` synchronized output.
- `FramePass` / `PassCtx` / `DebugOverlay` — a post-paint pipeline for
  HUDs, debug overlays, and future compositor passes.
- `Strategy` — diff classification (`Full`, `Lines`, `Cells`, `Noop`).
- `Snapshot` / `VtReplay` / `ReplayError` — deterministic capture and
  replay for tests.
- `Region`, `FrameView` — subrectangle painting.
- `StyleRole`, `StyleSet`, `Style`, `GraphemeArena`, `ClusterId`.

### 3.3 Frame lifecycle

1. Caller obtains the mutable "next" frame via `Renderer::frame()` or
   `put_str` and paints it.
2. `flush` (or `present` for a `Surface`) runs any `FramePass` instances,
   diffs `next` against `current` (`src/bin/repl/src/render/lib.rs:233`), picks a
   `Strategy`, and serialises the minimal update via `emit::{full,lines,
   cells}`.
3. DEC synchronized-output brackets the payload if supported
   (`src/bin/repl/src/render/lib.rs:257`).
4. Frames swap; `current` now holds what the terminal sees.

### 3.4 Grapheme handling

Cluster-aware. Wide glyphs (`漢`) occupy two cells with a continuation
marker; ZWJ emoji and combining marks collapse into one cluster stored in a
`GraphemeArena`. See tests at `src/bin/repl/src/render/lib.rs:362-423` for the
contract.

### 3.5 Performance contract

- One `write_frame` call per flush; no per-cell syscalls.
- SGR state is deduped across contiguous runs
  (`src/bin/repl/src/render/lib.rs:345`).
- Empty frame-pass pipeline is zero-allocation
  (`src/bin/repl/src/render/lib.rs:474`).
- EMA-smoothed FPS exposed to passes through `PassCtx`.

---

## 4. Agent harness (`harness`)

Source: `src/harness/src/`. Companion: `docs/repl/agent-harness.md`.
Shipped plugins (`echo`, `relay`, `llm`) live under `plugins/` at the repo
root; `relay` is the MCP server backing Relay memory.

### 4.1 Purpose

Minimum viable scaffolding for the coding-agent loop: spool input drives a
ReAct loop (assemble context → call LLM → dispatch tool calls → observe),
with LLM providers and tools plugged in through one small public surface.

### 4.2 Public surface

Re-exported from `src/harness/src/lib.rs:35-37`:

- `trait Plugin: Send + Sync` with `info(&self) -> PluginInfo` and
  `handle(&self, req: &Request) -> Result<Response, PluginError>`
  (`src/harness/src/plugin.rs:86`).
- `PluginInfo { name, version, summary }` — static metadata
  (`src/harness/src/plugin.rs:13`).
- `Request { op, body }` / `Response { body }` — opaque UTF-8 payloads so
  each plugin owns its own schema (`src/harness/src/plugin.rs:28-47`).
- `PluginError::{NotFound, UnsupportedOp, Failed}` — exhaustive
  (`src/harness/src/plugin.rs:61`).
- `Registry` — `BTreeMap<String, Arc<dyn Plugin>>` with deterministic
  ordering (`src/harness/src/registry.rs:14`).
- `Runtime` — owns an `Arc<Registry>`, exposes
  `dispatch(plugin_name, &Request)` (`src/harness/src/runtime.rs:13`).

### 4.3 Design choices

- `&self` on `handle` so `Arc<dyn Plugin>` can be shared across executor
  threads without locking.
- Synchronous today; streaming / async is an additive future change
  (`docs/repl/agent-harness.md:153`).
- Programmatic registration only in Phase 1; MCP-sourced and dyn-lib
  discovery are deferred (`docs/repl/agent-harness.md:102`).

### 4.4 Concurrency model (planned)

`Runtime::spawn_executor` launches workers that pull work items from a
shared queue and invoke the registry. See
`docs/repl/agent-harness.md:114`.

---

## 5. Text editing (`textarea`)

Source: `src/bin/repl/src/textarea/`.

### 5.1 Purpose

Every text input in the system — spool prompt, search, any text field —
shares one editing core. No duplicated cursor / selection logic.

### 5.2 Public surface

Re-exported from `src/bin/repl/src/textarea/lib.rs:21-25`:

- `Buffer`, `Pos` — gap-free string buffer plus `(row, col)` positions.
- `EditArea`, `EditOutcome`, `WrapMode` — driver type that applies key
  events and reports `Handled | Submit | Cancel | Unhandled`.
- `History`, `Group`, `Edit`, `EditKind` — undo/redo tree with
  keystroke-grouped edits.
- `hard_wrap`, `wrap_line`, `VisualRow` — soft-wrap helpers for
  render-time layout.

### 5.3 Consumption pattern

A caller borrows `&mut String`, `&mut usize` (cursor), and
`&mut Option<usize>` (anchor), wraps them in an `EditArea`, and forwards key
events. The spool input widget and any other text field reuse this so there
is one implementation (see `CLAUDE.md` "Text editing" section).

---

## 6. Input (`input`)

Source: `src/bin/repl/src/input/`.

### 6.1 Public surface

Re-exported from `src/bin/repl/src/input/lib.rs:28-31`:

- `InputEvent`, `MouseEvent`, `MouseButton`, `MouseKind` — the event
  model.
- `Key`, `KeyCode`, `Mods` — normalized keyboard representation across
  platforms.
- `EventPump`, `PumpError` — non-blocking event source over the terminal.
- `Shortcut`, `ShortcutSet` — declarative key-binding tables.

### 6.2 Role

`input` is the platform-abstraction boundary for keyboard and mouse. The
pump wraps `crossterm` today (`Cargo.toml:12`) but the public types are
independent of it, so alternative backends (xterm.js / WebSocket bridge)
can be added without touching downstream crates.

---

## 7. Plugin UI (`plugin_ui`)

Source: `src/bin/repl/src/plugin_ui/`.

### 7.1 Purpose

A minimum viable widget surface for plugins that want to render into the
host TUI without pulling in the entire `render` API.

### 7.2 Public surface

Re-exported from `src/bin/repl/src/plugin_ui/lib.rs:74-75`:

- `View`, `ViewId`, `PluginHost`, `EventResult` — the view trait and host
  contract. A plugin registers a `View`; the host assigns a `ViewId` and
  forwards `InputEvent`s, receiving `EventResult` signals back
  (`Handled`, `Unhandled`, redraw requests, dismissal).
- `TextField`, `TextFieldOutcome` — single-line input widget built on
  `textarea::EditArea`.

### 7.3 Data flow

```
 input::InputEvent ─► PluginHost ─► View::on_event
                                    │
                                    ▼
                            EventResult::Redraw
                                    │
                                    ▼
                       View::paint(Frame, Region)
                                    │
                                    ▼
                        render::Renderer::flush
```

---

## 8. Demo (`demo`)

Source: `src/bin/repl/src/demo/main.rs`. A single binary that wires `render`,
`input`, `textarea`, and `plugin_ui` together as an integration smoke test.
Not a product — it exists to validate the cross-crate seams during
development.

---

## 9. Data flow: a single frame

Putting the pieces together, one tick through the stack:

```
 ┌─────────────────────────────────────────────────────────────┐
 │  OS terminal                                                │
 └────────────▲──────────────────────────────────┬─────────────┘
              │ ANSI payload                     │ raw bytes
              │                                  ▼
 ┌────────────┴─────────┐             ┌─────────────────────┐
 │  render::Surface     │             │  input::EventPump   │
 │  (Stdout/Buffer)     │             │  (crossterm today)  │
 └────────────▲─────────┘             └──────────┬──────────┘
              │ write_frame                     │ InputEvent
              │                                 ▼
 ┌────────────┴──────────────────────────────────────────────┐
 │  render::Renderer                                         │
 │  ─ diff(current, next) → Strategy                         │
 │  ─ FramePass pipeline (DebugOverlay, HUDs)                │
 │  ─ emit::{full,lines,cells}  → buf                        │
 └────────────▲──────────────────────────────────────────────┘
              │ paints into Frame
 ┌────────────┴──────────────────────────────────────────────┐
 │  Host loop / app                                          │
 │  ─ routes InputEvent to focused View / EditArea           │
 │  ─ textarea::EditArea mutates buffer                      │
 │  ─ plugin_ui::View::paint fills Frame regions             │
 │  ─ harness::Runtime::dispatch for agent work              │
 └───────────────────────────────────────────────────────────┘
```

---

## 10. Persistence and event logs

Relay itself does **not** own persistence. SQLite and event logs live
in downstream consumers — notably Relay, which stores the shared memory
graph. The design split is intentional:

- `harness` plugins are free to open their own SQLite databases or emit
  structured events. Nothing in the harness core imports `rusqlite` or a
  logging framework.
- The opaque `Request`/`Response` bodies mean each plugin can serialise
  its own event envelopes (typically JSON) without leaking a schema
  dependency into the core PI.
- A future middleware layer may wrap `Runtime::dispatch` to tee requests
  into an event log, but that is additive and deferred
  (`docs/repl/agent-harness.md:148`).

When persistence ships, the expected shape is:

1. A single append-only event log per run (JSONL on disk or SQLite WAL).
2. Each dispatched `Request`/`Response` pair captured with wall-clock
   timestamp and plugin name.
3. Downstream state rebuildable by replaying the log against a clean
   SQLite snapshot.

This document will be updated when a concrete implementation lands.

---

## 11. Cross-cutting conventions

Enforced by `CLAUDE.md` and `rustfmt.toml`:

- Hard tabs, width 2.
- Inline `#[cfg(test)] mod tests` at the bottom of each module.
- `#![forbid(unsafe_code)]` where practical (see
  `src/harness/src/lib.rs:28`).
- `#![warn(missing_docs)]` on public library crates.
- Small files; split before a module grows past a few hundred lines.

Tooling:

- `cargo build` — full workspace build.
- `cargo test` — all inline tests.
- `cargo fmt -- --check` — formatting gate.
- `cargo clippy --all-targets --all-features --locked -- -D warnings` —
  recommended lint gate.

---

## 12. Non-goals

- No async runtime baked into core crates. Individual plugins may opt in.
- No dynamic-library loading for plugins until demand is proven.
- No built-in telemetry, retry, or rate-limiting middleware — those
  belong to plugins or an explicit middleware layer.
- No widget reflow / layout engine beyond explicit row/column math.

---

## 13. Source map

Quick reference for readers navigating the tree:

- Workspace manifest — `Cargo.toml`
- Harness design doc — `docs/repl/agent-harness.md`
- Harness PI — `src/harness/src/plugin.rs:86`
- Harness registry — `src/harness/src/registry.rs:14`
- Harness runtime — `src/harness/src/runtime.rs:13`
- Renderer core — `src/bin/repl/src/render/lib.rs:34`
- Frame pass pipeline — `src/bin/repl/src/render/pass.rs`
- Surface abstraction — `src/bin/repl/src/render/surface.rs`
- Edit area driver — `src/bin/repl/src/textarea/edit_area.rs`
- Input event model — `src/bin/repl/src/input/event.rs`
- Input key model — `src/bin/repl/src/input/key.rs`
- Shortcut tables — `src/bin/repl/src/input/shortcut.rs`
- View trait — `src/bin/repl/src/plugin_ui/view.rs`
- Text-field widget — `src/bin/repl/src/plugin_ui/text_field.rs`
- Integration demo — `src/bin/repl/src/demo/main.rs`

---

## 14. Open questions

- **Streaming `handle`.** LLM providers want token streams. A second
  `handle_stream` method with a default impl is the likely addition.
- **Typed payloads.** Every plugin re-parsing JSON is friction. An
  optional typed helper crate (never core) may be worth adding.
- **Event log location.** `harness` or a new `journal` crate? TBD once
  the first consumer needs replay.
- **xterm.js surface.** A WebSocket-backed `Surface` impl would let the
  same stack drive the browser terminal. No code today.
