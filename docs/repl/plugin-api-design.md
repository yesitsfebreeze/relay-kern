# Plugin API Design (Research)

Status: research / design note. No code changes.
Scope: research plugin API design.
Audience: Relay workspace contributors.

This document grounds the plugin API direction in the code that exists today
and proposes a concrete, minimal extension path. File references use the
worktree layout (`src/<crate>/…`).

---

## 1. Current State of the Workspace

Relevant crates (from `Cargo.toml:3`):

- `src/harness` — plugin trait, registry, runtime (agent-side plugins).
- `src/plugin_ui` — `View` trait, `PluginHost`, `TextField` (UI-side plugins).
- `src/render` — `Renderer`, `Frame`, `FrameView`, `Region`, `FramePass`,
  `Surface`.
- `src/input` — `Key`, `KeyCode`, `Mods`, `EventPump`, `Shortcut`.
- `src/textarea` — `EditArea` text editing primitives, reused by `TextField`.
- `src/demo` — example binary consuming the stack.

The workspace already has **two distinct "plugin" surfaces**:

1. **Agent / compute plugins** (`harness::Plugin`,
   `src/harness/src/plugin.rs:86`) — `info()` + synchronous
   `handle(&Request) -> Result<Response, PluginError>` with opaque UTF-8
   payloads. Routed via `Registry` (`src/harness/src/registry.rs:14`) and
   dispatched by `Runtime::dispatch`
   (`src/harness/src/runtime.rs:31`).
2. **UI / view plugins** (`plugin_ui::View`,
   `src/plugin_ui/src/view.rs:37`) — `render(&mut FrameView)` +
   `handle_key(&Key) -> EventResult` + `set_focus(bool)`. Owned by a
   `PluginHost` (`src/plugin_ui/src/view.rs:76`) that assigns stable
   `ViewId`s and dispatches keys to the focused entry.

Both surfaces are deliberately narrow — `harness/src/lib.rs:10` explicitly
calls out the "PI-mono" principle ("one small, public interface that
everything composes through"). The research below assumes this principle
holds and that the two surfaces stay orthogonal rather than merge.

---

## 2. Design Requirements

Derived from the current code and the workspace's stated constraints.

### 2.1 Functional

- **Two plugin kinds, one workspace.** Compute plugins (agents, tool
  runners) and UI plugins (widgets, overlays) must both be supported
  without forcing one into the other's shape.
- **Stable identity.** Every plugin has a stable key (string name for
  `harness::Plugin` via `PluginInfo.name`,
  `src/harness/src/plugin.rs:15`; opaque `ViewId` for `plugin_ui::View`,
  `src/plugin_ui/src/view.rs:62`).
- **Discovery.** Registries expose deterministic listings
  (`Registry::list` sorts by name, `src/harness/src/registry.rs:43`).
- **Focus + event routing for UI plugins.** Exactly one focused view at
  a time; unfocused views render but do not consume keys
  (`TextField::handle_key`, `src/plugin_ui/src/text_field.rs:133`).
- **Submit / Cancel signalling.** UI plugins surface intent back to the
  host without owning the app state (`EventResult::Submit | Cancel`,
  `src/plugin_ui/src/view.rs:21`).
- **Opaque payloads for compute plugins.** Request/Response bodies are
  `String` so each plugin defines its own schema
  (`src/harness/src/plugin.rs:28`).

### 2.2 Non-functional

- **Thread safety for compute plugins.** `Plugin: Send + Sync`
  (`src/harness/src/plugin.rs:86`) — the runtime reserves the right to
  dispatch concurrently.
- **No unsafe.** `#![forbid(unsafe_code)]` in `harness/src/lib.rs:28`;
  plugin code must not introduce it.
- **Documented public surface.** `#![warn(missing_docs)]` in harness,
  `#![deny(missing_docs)]` in `plugin_ui` and `input`. Any plugin-facing
  item needs a doc comment.
- **Zero-alloc hot paths where possible.** The render pipeline lazily
  allocates only when passes are added (`Renderer::passes: Option<Vec<…>>`,
  `src/render/src/lib.rs:44`). Plugin-side additions must respect this —
  no per-frame `Box` churn.
- **Determinism for tests.** UI plugins render into a `FrameView` that
  clips to a `Region` so tests can assert on specific cells
  (`view.rs:194`, `text_field.rs:203`).
- **Grapheme / wide-glyph correctness.** Any text-producing plugin goes
  through `Frame::put_str` / `FrameView` so clusters are interned in the
  shared `GraphemeArena` (`render/src/lib.rs:142`, tests at
  `render/src/lib.rs:362`).

### 2.3 Out of scope (YAGNI)

- Dynamic loading (dylib / WASM). The workspace compiles all plugins
  statically today; adding a dynamic loader is a separate, much larger
  design.
- Async plugin handlers. `harness::Plugin::handle` is synchronous on
  purpose (`plugin.rs:90`); an async variant can be added behind a
  feature flag later.
- Cross-plugin messaging / pub-sub. The runtime is a request/response
  dispatcher; event fan-out belongs in a layer above.
- Permission model / sandboxing. Defer until there is a non-first-party
  plugin to worry about.

---

## 3. Design Patterns in the Codebase

The existing code leans on a small, consistent set of Rust patterns. New
plugin-facing code should stay inside this vocabulary.

### 3.1 Trait-object registry (compute plugins)

`Registry` stores `Arc<dyn Plugin>` keyed by `String`
(`registry.rs:14`). Dynamic dispatch is the right call: heterogeneous
plugins are the whole point, and `handle` is not a per-cell hot path.

### 3.2 Trait-object stack with `ViewId` handles (UI plugins)

`PluginHost` stores `Box<dyn View>` in a `Vec` with monotonically
increasing `ViewId`s (`view.rs:62`, `view.rs:93`). Ids are **not reused**
after unregister — callers can hold them safely. `saturating_add`
avoids id reuse even at overflow.

### 3.3 Default-noop trait methods

Both `View::render` and `View::handle_key` default to no-ops
(`view.rs:40`, `view.rs:44`). Plugin authors only write what they need.
Apply the same pattern to any new optional hooks (e.g. `on_resize`,
`on_tick`) — never force implementers to care about capabilities they
do not use.

### 3.4 Enum-as-outcome

`EventResult` (`view.rs:21`) and `TextFieldOutcome`
(`text_field.rs:39`) encode routing decisions as small `Copy` enums.
Preferred over `bool` or `Option<()>` because the states are named and
extensible.

### 3.5 Delegation over inheritance

`TextField` wraps `EditArea` rather than re-implementing editing
(`text_field.rs:68`). New composite plugins should delegate to the
underlying primitive rather than re-deriving its behaviour.

### 3.6 Pipeline / strategy for rendering side-effects

`FramePass` + `PassCtx` (`render/src/lib.rs:76`) let extensions mutate
the frame post-paint without touching core widgets. This is the pattern
any "global" UI plugin (debug overlay, FPS counter, theme tint) should
use, not a new `View` that floats above everything.

---

## 4. Integration with Rendering and Input

### 4.1 Rendering seam

- Plugin views paint through `FrameView::new(renderer.frame(), region)`
  (`view.rs:168`). The `FrameView` clips writes to its region, so plugins
  cannot trample the rest of the screen.
- The host is **layout-agnostic**: callers compute the `Region` and pass
  it to `PluginHost::render` (`view.rs:161`). This matches the tui-design
  skill's "layout is row/column math, computed by the app" rule — we
  deliberately do not ship a layout engine.
- Wide glyphs, ZWJ emoji, and combining marks go through
  `GraphemeArena` interning already owned by `Renderer`
  (`render/src/lib.rs:142`). Plugins must use `FrameView::put_str` /
  `FrameView::set` and never synthesise cells that skip the arena.
- Global effects (e.g. a plugin-contributed "dim background"
  backdrop for an overlay) should be installed as a `FramePass` via
  `Renderer::add_pass` (`render/src/lib.rs:76`), not as another `View`
  painted last. Passes run after widgets and share `PassCtx` (frame
  counter, elapsed seconds, fps EMA) which plugins can use for
  animation without owning their own clock.

### 4.2 Input seam

- Plugin views receive `input::Key` values
  (`src/input/src/key.rs`, re-exported at `input/src/lib.rs:29`), not raw
  `crossterm` events. This keeps the plugin API backend-agnostic: swap
  `crossterm` for something else and plugins keep compiling.
- `EventPump` filters `KeyEventKind::Press` only
  (`input/src/lib.rs:14`). Plugins never see release/repeat duplicates on
  Windows.
- Routing order the embedding app should adopt:
  1. App-global shortcuts (quit, toggle help) checked against a
     `ShortcutSet` (`input/src/lib.rs:31`).
  2. `PluginHost::handle_key` for the focused view.
  3. On `EventResult::Propagate`, fall through to app navigation
     handlers.
  4. On `EventResult::Submit | Cancel`, the app reads the view's
     accessor (e.g. `TextField::text`, `text_field.rs:106`) and
     dismisses / commits.
- `set_focus` is a callback, not a pull: `PluginHost::focus`
  (`view.rs:126`) drives it, so plugins cannot silently steal focus.

### 4.3 Compute plugins vs. the TUI

`harness::Runtime` is not on the render thread. Dispatch happens from
wherever the agent loop runs (today synchronous, eventually pool-backed —
see the comment at `runtime.rs:3`). UI plugins that need to *call* a
compute plugin should do so via a message passed out of `handle_key`
(returning `Handled` or `Submit`), never inline on the render thread.

---

## 5. Proposed Implementation Strategy

The research concludes that the current API **already covers the goals
for a v1 plugin surface**. The work is to close a small set of gaps
rather than redesign. Each item below is a discrete, implementable unit.

### 5.1 Keep the two-surface split; document the seam

Add a short `docs/architecture/plugin-surfaces.md` that names the
distinction (compute vs. UI) and points at `harness::Plugin` and
`plugin_ui::View`. Prevents future drift where someone bolts rendering
onto `harness::Plugin` or routes RPC through `View`.

### 5.2 Optional lifecycle hooks on `View`

Add default-noop methods:

```rust
fn on_resize(&mut self, region: Region) { let _ = region; }
fn on_tick(&mut self, ctx: &TickCtx) { let _ = ctx; }
```

`TickCtx` mirrors `PassCtx` (`render/src/pass.rs`) but is called at the
event-loop cadence, not per flush. Default no-ops keep existing plugins
source-compatible.

### 5.3 Mouse routing for UI plugins

`input::MouseEvent` exists (`input/src/lib.rs:28`) but `View` has no
hook. Add:

```rust
fn handle_mouse(&mut self, ev: &MouseEvent, region: Region) -> EventResult {
  let _ = (ev, region); EventResult::Propagate
}
```

The host translates absolute mouse coordinates into region-relative ones
before calling. Keeps tui-design's "keyboard first, mouse additive"
constraint — the default propagates.

### 5.4 Z-order and modal stack in `PluginHost`

Today `render` order is caller-controlled (`view.rs:160`). Add an
optional "modal" flag so a pushed view:

- renders last (topmost),
- receives input exclusively until popped,
- is dismissed by `EventResult::Cancel`.

This is the standard overlay pattern described in tui-design
(§"Overlays and Modals") and matches what a repl-draft plugin would
need.

### 5.5 Plugin metadata for UI plugins

`plugin_ui::View` has no `info()`. Add a small `ViewInfo { name,
summary }` so introspection (e.g. a "list installed plugins"
command) works uniformly across both surfaces. Keep `ViewInfo` distinct
from `PluginInfo` — they describe different things.

### 5.6 Opt-in async compute plugins

Add a sibling trait `AsyncPlugin` in `harness` behind a feature flag:

```rust
#[cfg(feature = "async")]
pub trait AsyncPlugin: Send + Sync {
  fn info(&self) -> PluginInfo;
  fn handle<'a>(&'a self, req: &'a Request)
    -> Pin<Box<dyn Future<Output = Result<Response, PluginError>> + Send + 'a>>;
}
```

Keeps the default build synchronous (KISS) but unblocks LLM plugins that
need `tokio`. Deferred until an async consumer exists (YAGNI).

### 5.7 Data flow (sequence)

```text
terminal
  │  raw bytes
  ▼
input::EventPump  ───► InputEvent ──► App loop
                                       │
                                       │ 1. ShortcutSet lookup
                                       │ 2. PluginHost::handle_key(focused)
                                       │ 3. app navigation fallback
                                       ▼
                                  EventResult
                                       │
                       ┌───────────────┼───────────────┐
                       ▼               ▼               ▼
                  Handled          Submit/Cancel   Propagate
                  (loop tick)      (read state,    (fallback
                                   dispatch to      handler)
                                   harness::Runtime
                                   if needed)

render loop (per frame):
  app layout → Region per View → PluginHost::render(...)
    → FrameView writes into Frame
    → Renderer::flush → FramePass pipeline → diff → Surface::write_frame
```

### 5.8 API boundaries (summary)

| Concern                 | Crate         | Public type / trait                |
|-------------------------|---------------|------------------------------------|
| Compute plugin contract | `harness`     | `Plugin`, `Request`, `Response`    |
| Compute registry        | `harness`     | `Registry`, `Runtime`              |
| UI plugin contract      | `plugin_ui`   | `View`, `EventResult`              |
| UI host                 | `plugin_ui`   | `PluginHost`, `ViewId`             |
| Built-in UI widgets     | `plugin_ui`   | `TextField`                        |
| Rendering primitives    | `render`      | `Frame`, `FrameView`, `Region`     |
| Global effects          | `render`      | `FramePass`, `PassCtx`             |
| Input primitives        | `input`       | `Key`, `MouseEvent`, `Shortcut`    |

Plugins depend **only** on `harness`, `plugin_ui`, `render`, and
`input`. They do not touch `crossterm`, `textarea` internals (except
via `TextField::edit_area`, `text_field.rs:112`), or any app crate.

---

## 6. Challenges and Trade-offs

### 6.1 Opaque string payloads (compute plugins)

**Trade-off:** `Request.body: String` (`plugin.rs:30`) is schema-free.
Pros: zero coupling, any plugin picks its own format. Cons: no
compile-time validation, every plugin re-parses. Mitigation: ship a
small helper (`harness::json` behind a feature) that wraps
`serde_json::from_str` into `PluginError::Failed`. Do not force JSON
into the core type.

### 6.2 Dynamic dispatch cost

`Arc<dyn Plugin>` and `Box<dyn View>` mean a vtable indirection per
call. For compute plugins this is irrelevant (one call per request).
For `View::render` it could matter if we ever host hundreds of views.
Today the `PluginHost::render` is called once per view per frame and
the `find` over `Vec<Entry>` is linear (`view.rs:165`). Mitigation:
if profiling shows pressure, switch to a `HashMap<ViewId, Entry>` —
but not preemptively (YAGNI).

### 6.3 Concurrent `handle` on compute plugins

`Plugin: Send + Sync` (`plugin.rs:86`) puts the burden of internal
locking on implementors. A naive plugin that uses `RefCell` will not
compile — good. A plugin that uses `Mutex` trivially will serialise all
calls — acceptable default.

### 6.4 UI plugin focus vs. app focus

`PluginHost` owns one focused view (`view.rs:79`). The embedding app
also has its own focus model (repl input, overlays). The two must
be reconciled — e.g. when the app focuses a non-plugin region, the host
should be told to clear focus. Proposed convention: the app calls
`host.focus(None)` whenever its global focus leaves the plugin area.
Documented in §5.1's architecture note.

### 6.5 Layout responsibility

`PluginHost` is deliberately layout-agnostic (`view.rs:7`). Trade-off:
every embedding app re-implements the "give each plugin a region"
logic. Mitigation (future): a thin `plugin_ui::layout` module with a
vertical/horizontal stack. Defer until a second consumer outside
`demo` appears.

### 6.6 Dynamic loading

Not supported. Every plugin is a crate in the workspace. Trade-off:
zero ABI risk, good IDE support, no sandboxing story needed. Cost:
no third-party distribution. Accepted for the foreseeable horizon;
revisit only when external authors materialise.

### 6.7 Version skew

`PluginInfo.version: &'static str` (`plugin.rs:17`) is free-form. If we
ever need compatibility checks, add a `fn abi_version() -> u32` hook
returning a compile-time constant per crate. Not needed yet.

---

## 7. Summary

The workspace already exposes a credible plugin API across two
surfaces: `harness::Plugin` for compute, `plugin_ui::View` for UI. Both
follow the same minimalist discipline — small traits, default no-ops,
`Arc`/`Box<dyn _>` registries, `Copy` enum outcomes, explicit focus
callbacks. The integration seams with rendering
(`FrameView` + `FramePass`) and input (`Key` + `MouseEvent` +
`ShortcutSet`) are already in place and backend-agnostic.

The recommended path is to **keep the two-surface split, document it,
and add a small set of targeted extensions** (optional lifecycle hooks,
mouse routing, modal stack, view metadata, optional async). No redesign
is warranted.

---

## 8. References (file:line)

- `src/harness/src/lib.rs:1` — harness overview and layering.
- `src/harness/src/plugin.rs:86` — `Plugin` trait.
- `src/harness/src/registry.rs:14` — registry.
- `src/harness/src/runtime.rs:13` — runtime dispatch.
- `src/plugin_ui/src/lib.rs:1` — plugin_ui overview.
- `src/plugin_ui/src/view.rs:37` — `View` trait.
- `src/plugin_ui/src/view.rs:76` — `PluginHost`.
- `src/plugin_ui/src/text_field.rs:68` — `TextField`.
- `src/render/src/lib.rs:34` — `Renderer`.
- `src/render/src/lib.rs:76` — `add_pass` / `FramePass` pipeline.
- `src/render/src/lib.rs:217` — `present` (surface + sync update).
- `src/input/src/lib.rs:1` — input crate surface.
- `docs/architecture/agent-harness.md` — prior harness design note.
