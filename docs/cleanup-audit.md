# Cleanup audit — 2026-04-29 (post-execution residual, 2026-04-30)

Original audit executed across four phases (commits `8edfd77` through
`fd38445`). Resolved + obsolete findings pruned. Remaining entries are
items that were deferred during execution — see "Why deferred" on each.

## Findings (deferred)

### Near-duplicates

#### 3. Three `handle_input` impls in `repl/textarea`
**Type:** near-duplicate
**Location:** `src/bin/repl/src/textarea/edit_area.rs handle_input` (189 LOC)
**Other locations:** `src/bin/repl/src/textarea/form.rs handle_input`, `src/bin/repl/src/textarea/form.rs handle_input_field`
**Evidence:** All three pattern-match on `IKeyCode::{Char, Enter, Backspace, ...}` with overlapping shift/ctrl/alt logic. The `edit_area` body has its own ctrl-letter handling (`'a','z','y','j'`) which the form path likely also wants for consistency.
**Recommendation:** Pull a `KeyIntent` enum + `classify(key) -> KeyIntent` helper; have each `handle_input` dispatch on intents. Keeps one source of truth for keymap.
**Risk:** medium — touches actively edited input handling.
**Why deferred:** `LAYOUT.md` work still in flight; refactor would tangle with active edits.

#### 6. `kern/mcp/tools/tool_definitions` is JSON-by-hand
**Type:** near-duplicate (data, not code)
**Location:** `src/bin/kern/src/mcp/tools.rs tool_definitions` (116 LOC)
**Evidence:** Nine `serde_json::json!` blocks, each `{name, description, inputSchema:{type,required,properties}}`. Field-name strings repeated dozens of times.
**Recommendation:** Define a `ToolSpec { name, description, schema }` const slice with helpers; or define inputs as Rust structs with `schemars`. Same fix shape as #15 (already applied to `agnt/fs_inproc::list_tools` in commit `fd38445`).
**Risk:** low.
**Why deferred:** Lower priority — not load-bearing duplication.

### Oversized

#### 11. `repl/textarea/edit_area/handle_input` (189 LOC)
**Type:** oversized
**Location:** `src/bin/repl/src/textarea/edit_area.rs handle_input`
**Recommendation:** Split into `handle_form_active`, `handle_list_active`, `handle_normal_key`. The early-return form/list branches account for ~60 LOC of nesting; pulling them out flattens the control flow and lets each helper be unit-tested.
**Risk:** medium (active edits in `git status`).
**Why deferred:** Same as #3 — `LAYOUT.md` still in flux.

#### 12. `kern/tick/do_cluster` (128 LOC), `kern/retrieval/expand` (108), `kern/gnn/gat/forward_graph` (105), `kern/commands/run_server` (107)
**Type:** oversized
**Location:** see split index.
**Recommendation:** Audit each in a follow-up — they're algorithmic, not glue, so blind extraction risks slowing them. Read `do_cluster` and `expand` first; both likely have step-functions worth naming.
**Risk:** medium.
**Why deferred:** Needs benching before/after; not pure cleanup.

### Misc

#### A2 (sub-step d). `kern::mcp::{ok,err_resp}` envelope helpers
**Type:** duplicate (vs the lifted `trnsprt::jsonrpc` envelope landed in commit `2fadc05`)
**Location:** `src/bin/kern/src/mcp.rs` (~lines 231 + 240)
**Recommendation:** Migrate kern's MCP server to the shared `trnsprt::jsonrpc::serve` + `ok_response`/`error_response`. Delete the local helpers afterwards.
**Risk:** medium-high — kern uses a typed `Response` struct with `Box<RawValue>` IDs. Migration would touch every kern MCP handler.
**Why deferred:** Out of scope for this audit pass; structural envelope change rather than dedup.

#### A6. `dev_plugin` `handle_*` table is templated
**Type:** weak signal
**Location:** `src/bin/agnt/src/dev_plugin.rs` — 7 `handle_<verb>` fns (read/write/edit/grep/glob/list/bash) at 11–20 loc each, dispatched by `handle()`.
**Recommendation:** Only refactor if the parsing surface is uniform; if signatures legitimately differ keep as-is. Inspect before extracting.
**Risk:** medium (premature abstraction risk).
**Why deferred:** Audit explicitly flagged premature-abstraction risk; not worth the read.
