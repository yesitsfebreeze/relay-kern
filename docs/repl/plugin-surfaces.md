# Plugin Surfaces

Relay exposes **two** plugin surfaces. They are deliberately separate. Do not
merge them, and do not bolt responsibilities from one onto the other.

## The two surfaces

### 1. Compute surface — `harness::Plugin`

- Anchor: [`src/harness/src/plugin.rs:130`](../../src/harness/src/plugin.rs)
- Shape: `pub trait Plugin: Send + Sync` — a registered, RPC-like handler
  with `info()` metadata and an opaque `handle(op, payload)` entrypoint.
- Purpose: headless compute. Called by the agent loop, validator, tools,
  or MCP shims. No terminal, no frames, no input.
- Dispatch: trait-object registry keyed by name. `ManifestPlugin` provides
  stubs for manifest-declared entries until a concrete handler is injected.

### 2. UI surface — `plugin_ui::View`

- Anchor: [`src/plugin_ui/src/view.rs:37`](../../src/plugin_ui/src/view.rs)
- Shape: `pub trait View` with default-noop `render`, `handle_key`,
  `set_focus`, `on_resize`. Views are owned by `PluginHost` and addressed
  via opaque `ViewId` handles.
- Purpose: paint cells into a clipped `FrameView` and consume keys inside
  the region the app assigns. No network, no side channels, no RPC.
- Dispatch: trait-object stack; the host keeps z-order and one focused view.

## Why the split matters

| Concern              | `harness::Plugin` | `plugin_ui::View` |
|----------------------|-------------------|-------------------|
| Renders to terminal  | no                | yes               |
| Handles key/mouse    | no                | yes               |
| RPC / opaque payload | yes               | no                |
| Send + Sync required | yes               | no                |
| Addressed by         | name              | `ViewId`          |

Bolting rendering onto `harness::Plugin` couples headless compute to the
TUI frame loop. Routing RPC through `View` drags `Send + Sync` and opaque
payload plumbing into input/render code. Keep them apart.

A plugin crate is free to ship **both** — a `Plugin` impl for its compute
entrypoints and one or more `View` impls for its UI — but each concrete
type picks exactly one surface.

## Focus-reconciliation convention

`PluginHost` tracks one focused view; the embedding app tracks its own
focus (repl input, overlays, etc.). These two focus models are
reconciled by a single rule:

> **When the app's global focus leaves the plugin area, the app calls
> `host.focus(None)`.**

This keeps the host's focused-view slot from pointing at a view that no
longer has the user's attention, and it lets views rely on `set_focus`
for caret visibility and similar state.

Source: `docs/plugin-api-design.md` §5.1, §6.4.
