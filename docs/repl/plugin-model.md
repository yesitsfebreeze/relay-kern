# Plugin Model: Event-File Schema & Lifecycle Catalog

Status: **spec / pre-implementation**

Relay is pivoting away from WASM Component Model plugins toward a thinner **MCP + event-hooks** model. MCP servers advertise tools over the Model Context Protocol. The LLM can call those tools through normal MCP flow, and the harness can *also* fire them itself at well-defined points in the turn lifecycle. Event files are the glue: small YAML documents that declaratively bind a lifecycle event to an MCP tool call.

This document specifies:

1. The event-file file format (grammar, location, load order).
2. The full catalog of lifecycle events the harness emits, with payload schemas.
3. The templating language used inside `args`.
4. `inject_as` semantics and interaction with the context / token budget.
5. Priority ordering and tie-breaking.
6. Worked examples.

No Rust code is shipped alongside this document. This is a spec-only deliverable.

---

## 1. Rationale: why YAML

Event files are written by humans (plugin authors, power users) and read by the harness. The candidates were JSON, TOML, and YAML.

| | JSON | TOML | YAML |
| --- | --- | --- | --- |
| Comments | no | yes | yes |
| Multiline strings (for prompt templates) | painful | fair | native (`|`, `>`) |
| Nested `args` with mixed types | verbose | awkward | clean |
| Multiple documents per file (`---`) | no | no | yes |
| Widely recognised by agent / MCP tooling | yes | fair | yes |

**Choice: YAML.** Three reasons dominate:

- Event files frequently embed prompt fragments and JSON-shaped `args`. YAML block scalars handle both without escaping.
- A single file can declare several hooks separated by `---`, which matches how users think about "my `memory` plugin" (one file, many hooks).
- Comments are essential; this is configuration humans maintain, not machine-generated data.

Trade-offs accepted: YAML's whitespace sensitivity and the infamous "Norway problem" (`no` → `false`). Mitigated by requiring quoted strings for all `call` targets and by schema validation on load — unknown keys fail fast, type mismatches fail fast.

Parser: `serde_yaml` (current ecosystem default).

---

## 2. Event-file grammar

### 2.1 Location & load order

```
<config_root>/events/*.yaml
<config_root>/events/*.yml
```

plus any files contributed by installed plugins under their own `events/` directory.

Load order:

1. All files are discovered, sorted by path (stable, deterministic).
2. Each file is parsed as a stream of YAML documents (`---`-separated).
3. Each document must be a mapping and is validated against the hook schema below.
4. Hooks are collected into a per-event list, then sorted by `priority` (see §6).

### 2.2 Hook schema

Each document is one **hook**:

```yaml
# Required
on:        <event-name>          # see §3
call:      "<server>.<tool>"     # MCP fully-qualified tool id

# Optional
args:       <mapping>            # templated; see §4. Default: {}
inject_as:  system | user | tool_result | discard   # Default: tool_result
priority:   <int>                # Default: 0. Higher runs first. See §6.
when:       "<expr>"             # Guard. Hook fires only if expr is truthy. See §4.3.
id:         "<string>"           # Optional stable id for logs / dedupe.
timeout_ms: <int>                # Default: 5000. Hard cap on tool execution.
max_bytes:  <int>                # Default: 8192. Per-hook cap on injected body. See §5.
on_error:   skip | fail | inject # Default: skip. What to do if the tool errors.
```

#### BNF sketch

```
hook         ::= "on:" event SP
                 "call:" tool_ref SP
                 [ "args:" mapping ]
                 [ "inject_as:" inject_mode ]
                 [ "priority:" int ]
                 [ "when:" expr_string ]
                 [ "id:" string ]
                 [ "timeout_ms:" int ]
                 [ "max_bytes:" int ]
                 [ "on_error:" error_mode ]
event        ::= "startup" | "shutdown"
               | "pre_turn" | "post_turn"
               | "pre_tool" | "post_tool"
               | "on_file_change" | "on_error"
               | "on_slash_command"
tool_ref     ::= quoted_string      ; "<server>.<tool>"
inject_mode  ::= "system" | "user" | "tool_result" | "discard"
error_mode   ::= "skip" | "fail" | "inject"
```

Unknown keys → hard error on load. Missing required keys → hard error on load. The harness logs the offending file+document index and refuses to start when running in `strict` mode; in `lenient` mode it disables that one hook and continues.

---

## 3. Lifecycle event catalog

The harness emits exactly these events. Each fires at one well-defined point in the agent loop. Payload schema is JSON-shaped (documented here; in Rust it is a typed struct serialised once before templating).

### 3.1 `startup`

Fires once when the harness initialises, after config load, before the first prompt is shown.

```
payload: {
  version:     string,     # harness semver
  session_id:  string,     # ulid
  cwd:         string,     # absolute
  config_root: string,     # absolute
  plugins:     [string],   # loaded MCP server ids
}
```

Use: warm caches, load long-lived context (project summary, repo map), greet.

### 3.2 `shutdown`

Fires once on clean exit, after the loop has stopped and before the terminal is restored.

```
payload: {
  session_id: string,
  reason:     "user_quit" | "error" | "signal",
  turns:      int,
  duration_ms: int,
}
```

Use: flush journal, persist memory, write session summary. `inject_as` is ignored — there is no next turn.

### 3.3 `pre_turn`

Fires before each LLM call, after the user's message is appended but before the messages array is shipped.

```
payload: {
  session_id:  string,
  turn_index:  int,             # 0-based
  user_input:  string,          # latest user message (may be empty on tool-only turns)
  history_len: int,             # messages so far
  model:       string,          # e.g. "claude-opus-4-7"
  budget_left: int,             # tokens remaining in context budget
}
```

Use: retrieve memory, inject repo map, inject todo list, rewrite user input.

### 3.4 `post_turn`

Fires after the LLM returns its final assistant message for the turn (after any tool loop has settled).

```
payload: {
  session_id:   string,
  turn_index:   int,
  assistant:    string,         # final assistant text
  tool_calls:   int,            # how many tool calls this turn
  input_tokens: int,
  output_tokens: int,
}
```

Use: journal, distil memory, compact scratchpad. Output is typically discarded or written elsewhere; setting `inject_as: tool_result` would attach it to the *next* turn's context.

### 3.5 `pre_tool`

Fires immediately before the harness dispatches a tool call the LLM asked for.

```
payload: {
  session_id: string,
  turn_index: int,
  tool:       string,           # "<server>.<tool>"
  args:       object,           # the args the LLM produced
  call_id:    string,           # matches tool-use id in transcript
}
```

Use: argument validation, policy checks (block `bash.run rm -rf`), auto-approve low-risk tools, log intent.

If a `pre_tool` hook returns an error and `on_error: fail`, the tool call is cancelled and an error tool-result is synthesised back to the LLM.

### 3.6 `post_tool`

Fires after each tool call returns, whether success or error.

```
payload: {
  session_id: string,
  turn_index: int,
  tool:       string,
  args:       object,
  call_id:    string,
  ok:         bool,
  result:     string,           # UTF-8, truncated to max_bytes before templating
  duration_ms: int,
  error:      string | null,
}
```

Use: journal tool output, post-process (e.g. run `cargo fmt` after an edit), enrich result with context before the LLM sees it.

### 3.7 `on_file_change`

Fires when a watched path is created/modified/deleted while the harness is running. Debounced; coalesces bursts.

```
payload: {
  session_id: string,
  path:       string,           # absolute
  kind:       "create" | "modify" | "delete",
  size:       int | null,
  mtime:      int | null,       # unix seconds
}
```

Use: invalidate cached symbol index, re-run linters, notify the LLM on the next turn that a file moved under it.

Watch roots are configured separately (out of scope here).

### 3.8 `on_error`

Fires for any recoverable error the harness decides to surface: tool dispatch failure, MCP connection drop, template resolution failure, etc. Fatal crashes do not fire this — they exit.

```
payload: {
  session_id: string,
  turn_index: int | null,
  source:     "tool" | "mcp" | "template" | "io" | "llm",
  message:    string,
  detail:     string,           # stack or structured detail, may be multi-line
}
```

Use: append stacktrace to journal, notify the LLM ("the previous tool failed, here's why"), open a bug.

### 3.9 `on_slash_command`

Fires when the user submits a `/command` from the repl input (see existing `src/bin/repl` slash dispatch).

```
payload: {
  session_id: string,
  command:    string,           # without leading slash
  args:       string,           # rest of the line, verbatim
}
```

Use: custom slash commands backed by MCP tools without writing Rust.

---

## 4. Templating

`args` and `when` accept templates. Values inside `args` that are *not* strings are passed through unchanged. String values are expanded.

### 4.1 Syntax

- `${path.to.value}` — substitute the scalar at that payload path. Replaces the whole string if it was the only content; otherwise interpolates.
- `${path.to.value | default:"foo"}` — default if missing/null.
- `${path.to.value | json}` — JSON-encode the value (useful for nested objects).
- `${path.to.value | trim}` / `| upper` / `| lower` — minimal filter set.
- Literal `$` is written `$$`.

The grammar is intentionally narrow. No arithmetic, no loops, no conditionals inside templates. If you need more, write a real MCP tool.

### 4.2 Scope

Inside a template the root is the event payload defined in §3. Additionally:

- `${env.VAR}` — process environment (read-only, whitelist configured elsewhere).
- `${session.*}` — session-scoped values (`session.id`, `session.cwd`, `session.model`).
- `${now}` — RFC3339 timestamp of hook dispatch.

Nothing else is in scope. Cross-hook state is explicitly out: hooks communicate through MCP, not through template globals.

### 4.3 `when` expressions

The `when` guard is a single template that expands to a string, then evaluated as truthy/falsey:

- Truthy: non-empty string, `"true"`, any non-zero decimal.
- Falsey: empty string, `"false"`, `"0"`, `"null"`.

Example: `when: "${payload.ok}"` — only fire post_tool hooks when the tool succeeded.

### 4.4 Missing keys

Three behaviours, configurable per hook via `on_error`:

- `skip` (default): the hook silently does not fire.
- `fail`: the hook returns an error; an `on_error` event fires (careful — avoid loops; `on_error` hooks with template failures downgrade to `skip`).
- `inject`: the literal text `<missing:path.to.value>` is substituted so the LLM can see the gap.

### 4.5 Escaping

Quoted YAML strings are the author's friend. Inside an interpolated value, the harness escapes nothing — the result is passed byte-for-byte to MCP as a JSON argument. If the receiving tool expects JSON, use `| json`.

---

## 5. `inject_as` semantics

After the hook's tool returns, the harness takes its result and places it somewhere. `inject_as` controls where.

| Mode | Placement | Visible to LLM? | Visible in UI? | Typical use |
| --- | --- | --- | --- | --- |
| `system` | Prepended to the system prompt of the **next** LLM call as a dedicated system block. | yes | optional (dim) | Memory recall, repo map, "user prefers X". |
| `user` | Appended as a synthetic user turn (or part of one) before the next LLM call. | yes | yes | Rarely used; mostly for tests / mock inputs. |
| `tool_result` | Becomes a synthetic `tool_use` / `tool_result` pair in the next message array, marked as originating from `<server>.<tool>`. | yes | yes (collapsed) | **Default**. The LLM sees "I called X, here is the result" even though it didn't. |
| `discard` | Dropped after journaling. | no | no | Side-effect hooks: `post_turn` journaling, `shutdown` flush. |

### 5.1 Token budget

- Every injected body is truncated to `max_bytes` (default 8 KiB) **before** tokenisation.
- Truncation is byte-safe (UTF-8 boundary respected) and appends `… [truncated N bytes]`.
- If total injected bytes for a turn exceed `turn_inject_budget` (harness-level setting, default 32 KiB), injections are dropped in reverse priority order until they fit. Drops are logged and surface on the next `on_error` with `source: "template"` is **not** used — a dedicated warning channel is used instead so we don't self-trigger.
- `tool_result` injections count toward the LLM's context budget as tokens, same as any message. `system` injections are amortised by prompt-caching if the backend supports it.
- `discard` costs zero tokens regardless of body size.

### 5.2 Ordering within a turn

For a single event firing (e.g. one `pre_turn`), the harness:

1. Collects all matching hooks.
2. Sorts by priority (§6).
3. Executes them **in parallel** where safe (all `pre_turn` hooks are read-only w.r.t. the upcoming turn), collects results.
4. Concatenates injected bodies into the outgoing message in priority order (highest priority first, closest to the top / most recent).

`pre_tool` hooks are sequential because later hooks may depend on earlier ones rewriting `args` (future extension — v1 does not allow rewriting).

---

## 6. Priority & tie-breaking

- `priority` is a signed 32-bit integer. Default `0`.
- Higher priority runs **first** and is placed **earlier** in the injected block.
- Tie-breaker 1: file path (lexicographic).
- Tie-breaker 2: document index within the file.
- `id`, if set, must be unique across all loaded hooks. Duplicates → hard error on load.

Rule of thumb: leave `priority: 0` unless you specifically need to beat another hook. Reserve `priority >= 100` for harness-internal hooks (journal, safety policy). Reserve `priority <= -100` for last-resort fallbacks.

---

## 7. Examples

### 7.1 `pre_turn` — recall memory

`events/memory.recall.yaml`:

```yaml
# Before each turn, ask the kern memory plugin for anything related to the
# user's current message, and inject the top results as system context.
on: pre_turn
call: "kern.query"
args:
  text: "${user_input}"
  k: 5
  scope: "session:${session.id}"
inject_as: system
priority: 50
max_bytes: 4096
when: "${user_input}"     # skip on tool-only turns where user_input is empty
id: "memory.recall.preturn"
```

### 7.2 `post_tool` — journal every tool call

`events/journal.tools.yaml`:

```yaml
# Record every tool call outcome to the session journal.
on: post_tool
call: "journal.append"
args:
  session: "${session.id}"
  entry:
    kind: "tool"
    tool: "${tool}"
    ok: "${ok}"
    duration_ms: "${duration_ms}"
    args: "${args | json}"
    result_preview: "${result | trim}"
inject_as: discard        # side-effect only; don't waste tokens
priority: 100             # run before anything that might mutate result
timeout_ms: 1000
on_error: skip
id: "journal.post_tool"

---

# Second document in the same file: also run `cargo fmt` after any edit tool.
on: post_tool
call: "bash.run"
args:
  cmd: "cargo fmt"
  cwd: "${session.cwd}"
inject_as: tool_result    # let the LLM see the fmt output
priority: 10
when: "${tool}"           # non-empty guard; real filter lives in the tool
timeout_ms: 30000
max_bytes: 2048
id: "autofmt.post_tool"
```

(A richer `when` expression language — e.g. `tool == "fs.edit"` — is deliberately out of scope; v1 relies on server-side filtering inside the tool itself.)

### 7.3 `on_error` — attach stacktrace to the transcript

`events/errors.inject.yaml`:

```yaml
# Whenever the harness reports a recoverable error, persist the detail and
# surface a condensed version to the LLM on the next turn so it can react.
on: on_error
call: "journal.append"
args:
  session: "${session.id}"
  entry:
    kind: "error"
    source: "${source}"
    message: "${message}"
    detail: "${detail}"
    at: "${now}"
inject_as: system
priority: 90
max_bytes: 2048
on_error: skip            # never loop on_error -> on_error
id: "errors.journal"
```

---

## 8. What this spec does not cover

Out of scope:

- The Rust types that back `Hook`, `EventPayload`, `Template`.
- The MCP transport layer and server registry.
- File-watcher configuration for `on_file_change`.
- A richer expression language for `when` (deferred; users can push logic into MCP tools).
- Hot-reload of event files (deferred; v1 loads once at startup).

## 9. Change log

- v0.1: initial spec. Event catalog, YAML grammar, templating, `inject_as`, priority, examples.

---

See also: [`ui-slots.md`](./ui-slots.md) — declarative UI chrome above and below the composer, built on the same MCP plugin surface.
