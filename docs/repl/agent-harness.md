# Agent Harness Architecture

Status: design
Crate: `src/harness` (workspace member `harness`)

## Goal

Provide the minimum viable scaffolding for the kern coding-agent loop:
LLM providers, MCP tool plugins, and hook-runner participants all plug into
one small public surface and are orchestrated by a shared runtime.

## Guiding principle: PI-mono

**One small Public Interface, everything else composes through it.**

Concretely: the harness exposes exactly one trait — `Plugin` — plus three value
types (`PluginInfo`, `Request`, `Response`) and one error enum (`PluginError`).
Every external integration (LLM provider, tool, agent policy) implements that
trait. This keeps:

- the documentation surface flat (one trait to learn),
- the ABI/API stability story simple (one trait to version),
- composition trivial (anything that is a `Plugin` fits anywhere a `Plugin` is
  expected).

It is an explicit KISS/YAGNI choice: we resist sprawling trait hierarchies
until a concrete need for specialisation appears.

## Module layout

```
harness/
  src/
    lib.rs       re-exports and crate-level docs
    plugin.rs    Plugin trait + value types + PluginError
    registry.rs  name-keyed Registry<Arc<dyn Plugin>>
    runtime.rs   Runtime { registry } + dispatch
```

Layering is strictly downward:

```
runtime ──► registry ──► plugin trait ◄── external plugin impls
```

`plugin.rs` depends on nothing outside `std`. `registry.rs` depends only on
`plugin`. `runtime.rs` depends on both. No cycles, no framework lock-in.

## Public interfaces

### `trait Plugin: Send + Sync`

```rust
fn info(&self) -> PluginInfo;
fn handle(&self, req: &Request) -> Result<Response, PluginError>;
```

- `info` is pure and cheap; the registry relies on it being stable.
- `handle` is synchronous in the stub. The signature intentionally takes
  `&self` (not `&mut self`) so the runtime can share an `Arc<dyn Plugin>`
  across executor threads without locking.
- `Send + Sync` is non-negotiable: multiple executor agents will share the
  registry.

### Value types

- `PluginInfo { name: &'static str, version: &'static str, summary: &'static str }`
  — static metadata, registry key is `name`.
- `Request { op: String, body: String }` — plugin-defined op selector plus an
  opaque UTF-8 body. Keeping the body opaque preserves the flat PI: each plugin
  documents its own schema (typically JSON) without leaking schema types into
  the harness core.
- `Response { body: String }` — symmetric.
- `PluginError` — `NotFound | UnsupportedOp | Failed`. Small, exhaustive,
  public.

### `Registry`

`BTreeMap<String, Arc<dyn Plugin>>` with `register`, `get`, `list`, `len`,
`is_empty`. Sorted iteration is guaranteed (useful for deterministic UI
listings).

### `Runtime`

Owns an `Arc<Registry>`. `dispatch(plugin_name, &Request) -> Result<Response>`
looks up and delegates. The stub is synchronous. A later iteration will add:

- executor pool (thread or async task per in-flight work item),
- cancellation token per dispatch,
- structured logging / tracing hook.

The current `dispatch` signature is forward-compatible: adding a cancellation
token or an async variant is additive.

## Plugin registration & discovery

Phase 1 (stubs): **programmatic registration**. Callers build a
`Registry`, `register(Arc::new(MyPlugin))` each implementor, and hand the
registry to `Runtime::new`.

Phase 2 (follow-up work): **dynamic discovery**. Candidate mechanisms, all
of which end in a call to `Registry::register`:

1. **Built-in modules** — `harness_builtins::all()` returns `Vec<Arc<dyn Plugin>>`.
2. **MCP-sourced plugins** — a bridge crate queries a Kern MCP server for
   available tools and wraps each as a `Plugin`.
3. **Dynamic libraries** — last resort, behind a cargo feature, using
   `libloading` and a `#[no_mangle] extern "C" fn register(&mut Registry)`
   entry point. Deferred until demand is proven (YAGNI).

No discovery code ships in Phase 1.

## Concurrency & runtime design

Plan for Phase 2:

- `Runtime::spawn_executor(name: String) -> ExecutorHandle` launches a worker
  thread that pulls work items from a shared queue and calls
  `registry.get(plugin)?.handle(&req)`.
- Multiple executors share one `Arc<Registry>`; `Send + Sync` on `Plugin`
  makes this safe by construction.

Because `Plugin::handle` is `&self`, plugins are responsible for their own
internal synchronisation (e.g. a provider wrapping an HTTP client already has
an internal `reqwest::Client` that is `Send + Sync`).

## Error model

Small, explicit enum. No `anyhow` in the public API (we are a library, per the
rust-best-practices skill). Plugins surface detailed errors via
`PluginError::Failed(String)`; structured error codes can be added as variants
only when a caller needs to branch on them.

## Testing

Each module has `#[cfg(test)] mod tests` inline at the bottom of the same file
(per CLAUDE.md). Tests cover:

- `plugin`: an `Echo` plugin round-trips, rejects unknown op, error `Display`.
- `registry`: register/get, missing key, sorted listing, replace returns prior.
- `runtime`: dispatch to registered plugin, not-found mapping.

## Non-goals

- Async runtime — synchronous `handle` is sufficient for stubs.
- Persistence, telemetry, retry, rate limiting — all belong in individual
  plugins or a later middleware layer, not in the core PI.
- Dynamic library loading — gated on proven demand.

## Open questions

- **Streaming responses.** LLM providers want token streams. Likely addition:
  a second method `handle_stream(&self, req, sink: &mut dyn FnMut(&str))` with
  a default impl that calls `handle` and emits once. Deferred.
- **Typed payloads.** If every plugin re-parses JSON, a thin typed layer may
  be worth adding — but only as an optional helper crate, never at the PI.
