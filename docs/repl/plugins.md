# Plugins

Every plugin is an **MCP server**. Two flavours, one contract:

- **stdio subprocess** — any language. The harness spawns it and speaks
  MCP JSON-RPC 2.0 over stdio. This is the default.
- **in-proc** — a native Rust type that implements `harness::Plugin`. Same
  shape as an MCP server, zero serialization overhead. Used for hot-path
  plugins such as the LLM provider.

The harness is the MCP *client*. It discovers each plugin's tools, exposes
them to the LLM, and lets [recipes](./recipes.md) call them declaratively via
`[[steps]]` with `kind = "mcp"`.

Related documents:

- [`plugin-model.md`](./plugin-model.md) — event-hook layer (declarative YAML
  bindings from lifecycle events to tool calls).
- [`plugin-api-design.md`](./plugin-api-design.md) — in-proc `Plugin` trait.

## Where plugins live

```
plugins/<name>/
  Cargo.toml           # (in-proc only) crate manifest
  src/lib.rs           # (in-proc only) impl Plugin
  manifest.toml        # plugin-level metadata: id, transport, tools advertised
  events/*.yaml        # (optional) default event-hook bindings
  README.md
```

The `tools = [...]` allowlist in a recipe's `recipe.toml` must name tool ids
(e.g. `"fs.read"`, `"kern.ingest"`) that at least one registered plugin
advertises. The loader rejects recipes that reference unknown ids.

## Tool contract

Each tool advertises, via MCP:

- `name` — the id (e.g. `fs.read`).
- `description` — one line. The LLM sees this.
- `inputSchema` — JSON Schema for the arguments object.

The recipe engine ships arguments through as a TOML table after var
interpolation. The harness translates that to MCP's JSON argument envelope
before dispatch. The tool's reply body is captured as a **string** and
bound under the step's `bind` name.

If your tool's output is naturally structured, serialize to JSON or plain
text at the MCP boundary and document the shape in the tool `description`;
the recipe will interpolate it as-is into later templates.

## Two invocation paths

Every MCP tool is reachable two ways:

1. **LLM-called.** The model picks the tool from the advertised schema during
   a turn. Normal ReAct flow. Recipes do **not** drive this path — it's the
   agent-loop default.
2. **Event-triggered.** The harness fires the tool on a lifecycle event and
   injects the result into context. Declared via event-file YAML, never in
   the LLM prompt. Full spec in [`plugin-model.md`](./plugin-model.md).

A recipe `[[steps]]` entry is a third, explicit invocation: deterministic,
declared in the recipe file, runs whether or not the LLM would have chosen
it. Recipes use the same tool surface as paths 1–2.

## Writing a stdio plugin

Any language with an MCP SDK works. Minimal shape:

1. Implement an MCP server that advertises your tool(s).
2. Read requests from stdin, write JSON-RPC responses to stdout.
3. Ship a `manifest.toml` at the plugin root:

```toml
id = "myplugin"
transport = "stdio"
command = ["./myplugin"]      # how the harness spawns it
tools = ["myplugin.greet"]    # tool ids this plugin advertises
```

4. (Optional) Drop YAML event files under `plugins/myplugin/events/` to bind
   lifecycle events to your tools.

The harness takes care of discovery, spawn, handshake, capability exchange,
and routing.

## Writing an in-proc plugin

Use this path when serialization cost matters (LLM providers, streaming tools,
high-frequency callers). The trait lives in `harness`:

```rust
use harness::Plugin;

pub struct EchoPlugin;

impl Plugin for EchoPlugin {
    fn id(&self) -> &str { "echo" }
    fn tools(&self) -> &[ToolDescriptor] { &ECHO_TOOLS }
    fn invoke(&mut self, tool: &str, args: &serde_json::Value)
        -> Result<String, PluginError> { /* … */ }
}
```

See `plugins/echo/` for a current working example and
[`plugin-api-design.md`](./plugin-api-design.md) for the trait's exact
surface.

In-proc plugins register themselves in the harness registry at startup.
They are indistinguishable from stdio plugins to the recipe engine — the
same `ToolInvoker` indirection covers both.

## How the recipe engine reaches your tool

The recipe crate is I/O-free. It defines two traits:

```rust
pub trait ToolInvoker {
    fn invoke(&mut self, name: &str, args: &toml::value::Table)
        -> Result<String, EngineError>;
}

pub trait LlmInvoker {
    fn complete(&mut self, prompt: &str) -> Result<String, EngineError>;
}
```

The harness implements both, fanning `ToolInvoker::invoke` out to the correct
MCP server (stdio or in-proc) and `LlmInvoker::complete` to whichever LLM
plugin is active. Recipe authors never touch these traits — they're the wire
between engine and harness. Plugin authors expose tools through MCP; the
harness routes.

## Safety / resource limits

Event-hook layer enforces (see [`plugin-model.md`](./plugin-model.md)):

- **Cycle guard** — default depth 4 on hook chains to prevent feedback loops.
- **Per-turn inject budget** — default 32 KiB of context injected per turn.
- **Per-hook truncation** — default 8 KiB, UTF-8 safe.

Tool-level timeouts and arg validation are the plugin's responsibility; the
harness honours MCP cancellation and surfaces errors back to the engine as
`EngineError::Tool(…)`.

## Authoring checklist

1. Pick transport: stdio (most cases) or in-proc (hot path).
2. Advertise each tool with a tight `description` — the LLM uses it.
3. Declare args via JSON Schema; keep required fields small.
4. Return a **string** body; use JSON inside if you need structure.
5. Ship default event files only when the tool has an obvious lifecycle
   binding (e.g. `on_file_change → myindex.reindex`). Otherwise leave event
   wiring to the user.
6. Add a recipe under `recipes/<name>/` that exercises the new tool — it
   doubles as a smoke test and as documentation-by-example.
