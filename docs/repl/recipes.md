# Recipes

A **recipe** is a reusable, declarative workflow bundled with its prompts.
Each recipe lives in its own directory:

```
recipes/<name>/
  recipe.toml    # schema + steps (this document)
  pre.md         # prompt fragment prepended before steps run
  post.md        # prompt fragment appended after steps run
```

Recipes are purely declarative. Markdown files contain **no scripting and no
inline tool calls** — every side effect is expressed as a step in
`recipe.toml`.

## Schema (authoritative)

The published JSON Schema at [`recipes/recipe.schema.json`](../recipes/recipe.schema.json)
is the single source of truth for recipe shape. Every field, enum value, and
validation rule is described there, with human-readable `description`s.

Any recipe author — human or LLM — should read that file first. Tooling that
speaks JSON Schema (Taplo LSP, IDE plugins, `ajv`) will give live autocomplete
and validation from it.

Each `recipe.toml` declares its schema on line 1:

```toml
#:schema ../recipe.schema.json
```

Taplo picks this up automatically in VS Code / Neovim. The table below is a
human summary; when it drifts from the JSON Schema, the schema wins.

## `recipe.toml` schema

| Field          | Type                | Required | Notes                                                               |
| -------------- | ------------------- | -------- | ------------------------------------------------------------------- |
| `name`         | string              | yes      | Unique id within a project.                                         |
| `description`  | string              | no       | One-line human summary.                                             |
| `tools`        | array of string     | no       | Allowlist of tool ids (`"fs.read"`, `"llm.repl"`) steps may invoke. |
| `[[triggers]]` | array of tables     | no       | How the recipe is launched (see below).                             |
| `[[inputs]]`   | array of tables     | no       | User-supplied variables.                                            |
| `[[steps]]`    | array of tables     | no       | Ordered execution plan.                                             |

### Triggers

| Field   | Type                            | Required | Notes                                               |
| ------- | ------------------------------- | -------- | --------------------------------------------------- |
| `kind`  | `"slash"` / `"event"` / `"manual"` | yes   | How the recipe fires.                               |
| `value` | string                          | no       | Slash-command name or event name. Omit for manual.  |

### Inputs

| Field         | Type    | Required | Notes                                             |
| ------------- | ------- | -------- | ------------------------------------------------- |
| `name`        | string  | yes      | Referenced as `{{name}}` in markdown and args.    |
| `description` | string  | no       | Shown in prompts.                                 |
| `required`    | bool    | no       | Runtime must prompt when unset. Default `false`.  |
| `default`     | string  | no       | Fallback when the user supplies no value.         |

### Steps

| Field    | Type                   | Required         | Notes                                                                                                        |
| -------- | ---------------------- | ---------------- | ------------------------------------------------------------------------------------------------------------ |
| `kind`   | `"mcp"` / `"llm"`      | yes              | Step kind.                                                                                                   |
| `tool`   | string                 | when `kind="mcp"` | Tool id. Must appear in `tools` allowlist.                                                                   |
| `args`   | table                  | no               | Arguments; values may contain `{{var}}` placeholders.                                                        |
| `prompt` | string                 | when `kind="llm"` | `"pre"`, `"post"`, or an inline prompt string.                                                               |
| `bind`   | string                 | no               | Name to bind the step's result under (usable in later `{{…}}`).                                              |
| `when`   | string                 | no               | Render-time guard. Interpolated against bindings; step runs only if result is truthy (see below).            |

### Conditional steps (`when`)

A step may carry a `when` guard. The string is rendered with the current
bindings, then tested:

- **falsy** — empty after trim, `"false"` (case-insensitive), or `"0"`
- **truthy** — anything else

Falsy → the step is skipped; the engine emits a `TraceEvent::Skipped` for the
journal but does not call the tool or LLM.

Typical shape: a prior `llm` step binds a verdict, later steps gate on it.

```toml
[[steps]]
kind = "llm"
prompt = "pre"
bind = "verdict"           # "" = skip, anything else = run

[[steps]]
kind = "mcp"
tool = "kern.ingest"
when = "{{verdict}}"       # runs only if the classifier replied non-empty
args = { text = "{{text}}", topic = "{{verdict}}" }
```

See [`recipes/branch/`](../recipes/branch/) for a full exemplar.

## Variable interpolation

Variables are interpolated with **`{{var}}`** syntax (double-braces). This
applies to `pre.md`, `post.md`, and any string value under `args` in a step.

The name inside the braces resolves against, in order:

1. step bindings (`bind = "…"` from earlier steps)
2. user-supplied inputs
3. input defaults

Unknown names are a render-time error. Interpolation is not recursive.

Literal `{{` is unsupported today — add escaping when a concrete recipe needs
it.

## Triggers

Three trigger kinds:

- **`slash`** — invoked by typing `/<value>` in repl.
- **`event`** — fired by a runtime lifecycle event (e.g. `"session_start"`).
- **`manual`** — only reachable from the UI / command palette.

A recipe may declare multiple triggers.

## Shipped exemplars

Each illustrates a distinct pattern. Read the `recipe.toml`, `pre.md`, and
`post.md` side-by-side.

| Recipe                                      | Pattern                                                                 |
| ------------------------------------------- | ----------------------------------------------------------------------- |
| [`recipes/repl/`](../recipes/repl/)         | Baseline. No tools. One LLM call. Minimum shape a recipe can take.      |
| [`recipes/example/`](../recipes/example/)   | `fs.read` → `llm`. Single MCP step feeding prompt context.              |
| [`recipes/review/`](../recipes/review/)     | Same shape as `example` but with multi-input + default + tool allowlist.|
| [`recipes/memorize/`](../recipes/memorize/) | `llm` → `kern.ingest`. Post-LLM memory write: recipe as memory hook.    |
| [`recipes/branch/`](../recipes/branch/)     | `llm` classifier + `when`-gated MCP step. Minimal "recipe as agent".    |

### Authoring a new recipe

1. `mkdir recipes/<name>/`
2. Copy the closest exemplar's three files as a starting point.
3. Keep `#:schema ../recipe.schema.json` on line 1 of `recipe.toml` — it gives
   editors instant validation.
4. List every MCP tool the recipe uses in the top-level `tools = [...]`. The
   loader rejects any step whose `tool` is not on that list.
5. Run `cargo test -p recipe` to validate the document parses and the shipped
   exemplar tests still pass.

### Recipes as agents

A recipe with a classifier LLM step plus `when`-gated follow-ups is an agent:
the file declares the allowed moves, the model picks one by what it binds, and
the engine routes. No separate agent DSL — the existing `[[steps]]` list with
conditional guards is enough.

If a future workload genuinely needs multi-arm selection (`choose` step kind),
add it then; until then the `llm → when` pattern covers the real cases.

## Validation

`recipe::schema::Recipe::from_toml_str` parses and validates a document. It
rejects:

- empty `name`
- `kind = "mcp"` steps missing `tool`
- `kind = "mcp"` steps whose `tool` is not in the `tools` allowlist
- `kind = "llm"` steps missing `prompt`
- unknown trigger / step kinds
