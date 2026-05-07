> Rewritten 2026-04-23 against current code. Prior version described an earlier architecture.

# KB-Driven Context Architecture for Relay

How repl assembles LLM context: one repl input, a ReAct loop, MCP
tools, event hooks, and Kern as the long-term memory substrate.

---

## 1. Shape of the loop

Repl is a coding-agnt harness (see `CLAUDE.md`). The loop is:

```
repl input ─► agent_loop ─► pre_turn hooks ─► LLM call ─► tool dispatch ─► post_tool hooks ─► … ─► post_turn hooks
                                                                  ▲                         │
                                                                  └─────── journal ─────────┘
```

There is no ticket board, no organizer, no per-ticket working set. One
user, one conversation, one loop.

Core crates that participate:

- `src/harness/` — `Plugin` trait, `Registry`, event bus, hook runner,
  MCP adapter. See `src/harness/README.md`.
- `src/mcp_client/` — stdio MCP JSON-RPC transport.
- `src/agent_loop/` — drives the ReAct cycle.
- `src/journal/` — append-only event log.

---

## 2. Plugin model (MCP only)

Every tool surface is an MCP server. No WASM, no dylibs. Two flavours:

- **Stdio subprocess** — any language speaking MCP JSON-RPC 2.0.
- **In-process `impl harness::Plugin`** — Rust plugin that speaks the
  same request shape with zero serialisation overhead.

Both are reached through `harness::hooks::ToolInvoker`. See
`docs/plugin-model.md` for the full spec.

Shipped plugins (`plugins/` at repo root):

| Plugin | Role |
|---|---|
| `echo` | Reference in-process plugin; used in tests and smoke checks. |
| `llm`  | LLM provider wrapper used by the agent loop. |
| `kern` | Memory bridge. Subscribes to `context.build` (priority 100) and `turn.end`; retrieves and ingests through Kern. |

Each ships its own `README.md` with the concrete op list.

---

## 3. Event hooks — how context gets injected

The hook runner binds lifecycle events to MCP tool calls declaratively.
Event files live at `<plugin>/events/*.yaml` and
`<config_root>/events/*.yaml`. Naming convention inside the runtime:
`ev:<event>:<tool>`.

Lifecycle events (source of truth: `harness::LifecycleEvent`):

- `startup`, `shutdown`
- `pre_turn`, `post_turn`
- `pre_tool`, `post_tool`
- `on_file_change`, `on_error`, `on_slash_command`

Each hook binds one event to one MCP tool with `args` templates,
`inject_as`, `priority`, `timeout_ms`, `max_bytes`, and a `when` guard.
Safety: cycle guard (default depth 4), per-turn inject budget (32 KiB),
per-hook body cap (8 KiB, UTF-8 safe).

Full schema and examples: `docs/plugin-model.md`.

---

## 4. Kern integration

Kern is the knowledge-graph substrate. Repl reaches it through the
**`kern` MCP server** — not through a custom Rust client. Tools
exposed: `query`, `ingest`, `link`, `degrade`, `forget`, `pulse`,
`health`, `purpose`, `descriptor`.

Typical wiring:

- `pre_turn` hook → `kern.query` with the user's message → result
  injected as `system` context.
- `post_turn` hook → `kern.ingest` with the turn's new thoughts →
  `inject_as: discard` (write-only side effect).

Repl does not embed its own vector store. Kern owns retrieval; the
harness is a thin MCP client.

---

## 5. Context budgeting

Two nested budgets govern what reaches the model:

1. **Per-hook `max_bytes`** — each hook truncates its own payload before
   injection (UTF-8 safe, appends a `… [truncated N bytes]` marker).
2. **Per-turn `turn_inject_budget`** — 32 KiB default across all hooks
   in a single event firing; lowest-priority hooks drop first.

The agent loop does not re-implement retrieval ranking. Prompt shape is
whatever the LLM plugin sends plus whatever hooks inject. Determinism
comes from the journal, not from a rigid prompt skeleton.

---

## 6. Authoritative references

- Plugin model spec: `docs/plugin-model.md`
- Recipes: `docs/recipes.md`
- Harness crate: `src/harness/README.md`
- Agent loop crate: `src/agent_loop/` (see crate docs)
- Journal: `src/journal/` (see crate docs)
- Project goal: `CLAUDE.md`, `.claude/skills/goal/SKILL.md`
- Constraints: `.claude/skills/constraints/SKILL.md`
- Kern: `src/bin/kern/README.md`
