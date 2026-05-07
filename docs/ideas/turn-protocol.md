# Turn Protocol — Phase 0 PRD

Status: design locked, not implemented.
Owner: agnt main loop.
Last updated: 2026-04-29.

## Goal

Define the per-turn contract between user, agnt, and kern. Single LLM call per turn, structured XML I/O, bounded goal carried in a flat markdown file, kern queried read-only for per-turn information slices. No separate compactor model, no kern table for goals, no `/resume` command.

## Non-goals

- Training a small specialist model for context assembly. Deferred — main LLM with kern read tools is sufficient.
- Storing goals in kern. Goals are working state, not durable knowledge.
- Per-conversation goal files, sub-goal hierarchies, status enums beyond what the LLM writes in markdown.
- Multi-day daily snapshot files. Day rollover appends a goal-snapshot event to the existing journal JSONL only.

## Glossary

| Term | Definition | Aliases to avoid |
|---|---|---|
| **goal** | Bounded markdown body capturing the user's evolving vision; ≤ ~1000 chars total across all goals | vision, plan, todo |
| **slice** | Per-turn information set assembled by the main LLM via kern read tools | context, snippet |
| **turn** | One user question + one structured LLM call + one response | iteration, exchange |
| **kern** | Knowledge daemon — read-only substrate for slices | the Current, the kern |
| **journal** | Existing JSONL append-only event log under `.relay/journal/` | history, log |

## Storage

```
.relay/journal/
  goal.md           # single source of truth, flat markdown
  YYYY-MM-DD.jsonl  # existing journal; receives goal-snapshot event at rollover
```

- `goal.md` is the only place goals live. No DB table, no per-goal files, no per-day duplicate.
- The journal JSONL receives a `kind: "goal-snapshot"` event at midnight rollover with the current goal body. Provides audit trail without duplicating files.
- kern stores nothing about goals. Read-only for slice assembly.

## Per-turn contract

### Input to main LLM

```
<goal>{contents of .relay/journal/goal.md}</goal>
<info>{empty initially; main LLM fills via tool calls}</info>
<question>{user's current message}</question>
```

### Tool surface (read-only against kern)

Exposed via the existing knids MCP server:

- `kern.query(text, k, mode, scope, kind, since, before)` — vector + lexical + reason fused retrieval.
- `kern.fetch(id)` — direct thought lookup; full text, no truncation.
- `kern.walk(from_id, edge_kind?)` — traverse reason edges from a thought.
- `kern.health()` — sanity check; counts.

No write tools (`ingest`, `link`, `forget`, `degrade`) are exposed in the turn loop. Writes occur outside this protocol.

### Output from main LLM

```
<new_goal>{updated goal body, soft ≤1000 chars}</new_goal>
<answer>{response shown to user}</answer>
```

The same call produces both. No separate compactor pass.

## Cold-start

- Turn 1, fresh conversation: `goal.md` is empty or missing; `<goal>` is empty.
- Main LLM seeds `<new_goal>` from `<question>`.
- agnt writes the seed to `goal.md`.

There is no `/resume` command. Resuming is implicit: `goal.md` carries forward across sessions.

## /goals slash command

Renders a journey view by parsing `goal.md`. Output sections:

- **Active goals.** Title + body of each goal block.
- **Where we are.** Status distilled from goal body.
- **Next steps.** Surfaced from goal body or live-derived by main LLM.
- **Hints.** Bootstrap suggestions for continuing work.

Implementation options (decide at build time):

- (A) Repl-side parser: extract sections, render chrome.
- (B) LLM-mediated: agnt main LLM reads goal.md, emits formatted markdown response.

Option B reuses existing turn machinery; option A is faster but requires a fixed goal.md schema. Default to B for v1; add A only if latency matters.

## Goal mutation

- **User clears goal**: `rm .relay/journal/goal.md` or in-app shortcut. Next turn cold-starts.
- **Delete one goal**: user types natural language ("drop the SONIC goal"); main LLM rewrites `goal.md` accordingly via `<new_goal>`.
- **Update**: implicit on every turn.

No `/create-goal`, `/delete-goal`, or schema-validated commands. Mutation is conversational.

## Daily rollover

At local midnight (or on the next agnt tick after midnight):

1. Append a journal event to today's `YYYY-MM-DD.jsonl`:
   ```json
   {"ts": "...", "kind": "goal-snapshot", "body_md": "<contents of goal.md>"}
   ```
2. `goal.md` itself is left in place.

No file copy, no kern write.

## Budget

- Soft cap: goal body ≤ ~1000 chars. Enforced by prompt instruction; no post-hoc truncation.
- Tunable per-conversation if drift becomes measurable.
- Slice (`<info>`) has no fixed cap; main LLM is responsible for keeping it tight under the small-window invariant.

## Failure modes and mitigations

- **Compaction drift.** Lossy rewrite each turn may erode nuance over long sessions. Mitigation: durable failed-attempts and decisions are ingested as kern thoughts (`kind=fact` where appropriate); the slice re-injects them on relevant questions. Goal stays for vision; kern carries the why-it-failed details.
- **Empty slice.** If no kern tool call returns useful content, main LLM proceeds with `<goal>` + `<question>` only. No fallback path required.
- **Goal file corruption / missing.** Treat as cold-start. The journal JSONL retains historical snapshots for manual recovery.
- **Concurrent agents writing goal.md.** Out of scope for v1; assume single agnt instance per `.relay/`.

## Acceptance criteria

1. agnt reads `.relay/journal/goal.md` on every turn; treats missing/empty as cold-start.
2. Main LLM call uses XML I/O exactly as specified; tool calls limited to the read-only kern surface above.
3. agnt writes `<new_goal>` payload back to `goal.md` after each turn.
4. `/goals` returns a journey view derived from `goal.md`.
5. Day rollover appends a `goal-snapshot` event to the journal JSONL.
6. No kern write occurs as part of the turn loop.

## Open items

- **Slice assembly latency budget.** Main LLM tool calls add round trips to kern. Observe in shadow mode; revisit if median turn latency exceeds target.
- **Multi-conversation goals.** v1 assumes one `goal.md` per `.relay/` directory. Per-chat goals across one agnt are not supported and should be rejected if surfaced.
- **Tool naming alignment.** `kern.query` vs `mcp__knids__query` — pick a canonical user-facing name for the prompt; current MCP names leak the `knids` legacy term.

## References

- `docs/OVERVIEW.md` — project thesis and architecture.
- `docs/ideas/mesh-reasoner.md` — long-term trained-specialist option (not v1).
- knids thoughts (load-bearing for this PRD):
  - `69f9c0643c5552b3` — single-pass turn shape.
  - `88c8fffc3486add9` — XML I/O, smart tools, soft budget, cold-start.
  - `3dac0df5c42ed46b` — goals-as-flat-markdown invariant.
  - `c61a1a64f88e8d75` — `.relay/journal/goal.md` path lock.
  - `fd2f784b7577cd4b` — `/goals` journey view.
  - `138fca660dc22790` — bounded goal compaction stream.
  - `4c2c65a3ccf658e9` — three-part per-turn injection.
  - `903f39694b4232e2` — tool-call architecture, no extra LLM.
