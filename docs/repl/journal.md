# Journal schema

The journal is an append-only JSONL file describing a harness run as a stream
of typed events. One JSON object per line. Intended for downstream scripting:
replay, cost analysis, trace viewers.

Canonical serde types live in [`src/journal/src/event.rs`]. The wire format is
owned by `journal::event::SCHEMA_VERSION` (currently `1`).

## Event envelope

Every event is a tagged object discriminated by `kind` and carries:

| field    | type   | notes                                    |
| -------- | ------ | ---------------------------------------- |
| `kind`   | string | one of `turn_start` `turn_end` `tool_call` `usage` |
| `ts_ms`  | u64    | wall-clock, milliseconds since UNIX epoch |
| `turn`   | u32    | 0-indexed turn number within the run      |

Additional fields are variant-specific. Unknown fields MUST be ignored by
consumers; absent optional fields MUST be treated as `null`/unset.

## Variants

### `turn_start`

Start of an agent turn.

```json
{ "kind": "turn_start", "ts_ms": 1700000000000, "turn": 0, "prompt": "hello" }
```

- `prompt` (optional string) — user- or harness-supplied prompt that
  initiated the turn.

### `turn_end`

End of an agent turn.

```json
{ "kind": "turn_end", "ts_ms": 1700000001000, "turn": 0, "duration_ms": 1000, "output": "hi" }
```

- `duration_ms` (u64) — wall-clock duration of the turn.
- `output` (optional string) — final assistant output for the turn.

### `tool_call`

A tool invocation dispatched during a turn.

```json
{
  "kind": "tool_call",
  "ts_ms": 1700000000500,
  "turn": 0,
  "call": {
    "id": "call_1",
    "name": "fs.read",
    "args_json": "{\"path\":\"/tmp/x\"}",
    "result_json": "\"ok\"",
    "duration_ms": 12
  }
}
```

`call` fields:

- `id` (string) — caller-assigned correlation id.
- `name` (string) — fully qualified tool name.
- `args_json` (string) — serialised JSON of the arguments.
- `result_json` (optional string) — serialised JSON of the result on success.
- `error` (optional string) — stringified error on failure. Mutually
  exclusive with `result_json` in practice.
- `duration_ms` (u64) — wall-clock duration of the call.

### `usage`

Token usage (and optional cost) reported by an LLM call.

```json
{
  "kind": "usage",
  "ts_ms": 1700000000900,
  "turn": 0,
  "usage": {
    "prompt_tokens": 120,
    "completion_tokens": 42,
    "total_tokens": 162,
    "cost_usd": 0.00123,
    "model": "claude-opus-4-7"
  }
}
```

`usage` fields:

- `prompt_tokens`, `completion_tokens`, `total_tokens` (u64) — token counts.
  `total_tokens` is stored explicitly; consumers need not recompute it.
- `cost_usd` (optional f64) — cost in USD, if known.
- `model` (optional string) — provider model identifier.

## Stability

`SCHEMA_VERSION` is bumped on any non-additive change (renamed/removed
field, changed type, re-tagged variant). Additive optional fields do **not**
require a bump — consumers MUST tolerate unknown fields.

Snapshot tests in `src/journal/src/event.rs` (via `cargo insta`) guard the
on-the-wire JSON shape against accidental drift.
