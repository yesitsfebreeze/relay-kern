# Codebase audit — ticket b67b83da

> **Historical.** Kept for context on decisions that shaped the board crate
> (`planned/board/cranyum/`). Not current work.

Scope: apply KISS/DRY/YAGNI across `cranyum/` to reduce legacy complexity,
redundancy, and unused code. Primary deliverable is a set of focused
subtickets; one low-risk in-place simplification is also committed here.

Audit method: `cargo clippy --all-targets`, `wc -l`, grep for
`allow(dead_code)`, `TODO`, `legacy`, `deprecated`. Behaviour preserved —
no public API change. `cargo test` and `cargo fmt -- --check` green.

---

## Findings

Each finding names file(s), describes the issue, and points at the
subticket created to address it.

### 1. `cranyum/main.rs` — 1028 lines, mixed concerns
Entry point has grown into an event loop **plus** overlay dispatch, chat
input wiring, chat-draft wiring, form edit wiring, and terminal lifecycle.
Violates KISS (functions > 20 lines, deep nesting) and the project rule
"small files".
**Subticket:** `6d2ecbf0` — Split cranyum/main.rs into focused modules.

### 2. `cranyum/kanban/overlay.rs` — 1752 lines, largest module
Paints help, ticket-detail, confirm overlays plus their scroll/paging
helpers in one file. Single-responsibility violation.
**Subticket:** `bb85f8a1` — Split overlay.rs by overlay type.

### 3. `cranyum/agent/orchestrator/orchestrator.rs` — 1363 lines
God-module combining process supervision, event fan-out, session
lifecycle, and board syncing. Combined with `manager.rs` (865) and
`events.rs` (411), the orchestrator subsystem is ~2600 lines with
unclear boundaries.
**Subticket:** `2c4c1ebc` — Split orchestrator.rs into
session_lifecycle / process_supervisor / board_sync / event_fanout.

### 4. `cranyum/board_ops/tasks.rs` — 763 lines
All task CRUD, move, reorder, search, ready, blocked, upsert in one file;
one function carries `#[allow(clippy::too_many_arguments)]` at line 299.
Other `board_ops/*` files already follow a one-concern-per-file pattern.
**Subticket:** `2ac928b1` — Split tasks.rs into crud / move_reorder /
query, and replace too_many_arguments with a builder input struct.

### 5. `#[allow(dead_code)]` — 15+ sites
- `cranyum/agent/adapter.rs:26`
- `cranyum/agent/harness/agent_loop.rs:24`
- `cranyum/agent/harness/provider.rs:59`
- `cranyum/agent/orchestrator/manager.rs:54,56,58,73,165,176,192`
- `cranyum/buffer/color.rs:10`
- `cranyum/event_log.rs:27`
- `cranyum/kanban/mod.rs:342` (`cursor_to_row` — clearly unused helper)
- `cranyum/mcp/core.rs:17`
- `cranyum/react/mod.rs:12` (module-wide)

Speculative "public API; called from MCP tools / UI as features land"
notes are YAGNI smells — the code should be either consumed now or
deleted.
**Subticket:** `266b5f51` — Audit and remove dead_code annotations.

### 6. 79 clippy warnings — mostly mechanical
- `io_other_error` in `cranyum/session.rs:62`
- `manual_ignore_case_cmp` in `cranyum/main.rs:434`
- `items_after_test_module` in `cranyum/kanban/card.rs:79` (violates our
  "tests go at bottom" rule)
- `module_inception` in `cranyum/board_ops/tests.rs` (a module named
  `tests` inside `tests.rs`)
- `unwrap_or_default`, `unnecessary_map_or` across modules

Two of these are fixed in this commit (see In-place refactor below).
**Subticket:** `a5028a49` — Drive clippy to zero warnings.

### 7. Legacy-color reading duplication
`cranyum/db.rs:8-14` handles legacy INTEGER vs new TEXT colour columns;
`cranyum/board_ops/task_types.rs:14,48` re-implements the same tolerance
at the call site. Two sources of truth for one migration concern.
**Subticket:** `42224e1c` — Route all color reads through db::read_color
and add a one-shot INTEGER→TEXT migration.

### 8. Repeated region bookkeeping in `cranyum/kanban/mod.rs::paint_board`
`paint_board` computes `top`, `bottom`, `sidebar_total` and constructs
`Region { … }` at five call sites. KISS/DRY violation that also
obscures the layout rules.
**Subticket:** `cbbf6b16` — Introduce a `BoardLayout` struct computed
once at the top of paint_board.

### 9. `cranyum/react/mod.rs` module-wide `#![allow(dead_code)]`
574 lines gated by a module-wide allow that the file's own doc-comment
admits covers "currently-unused symmetric helpers". YAGNI: ship only
what is used.
**Subticket:** `1a7c9198` — Delete unused helpers or gate with
`#[cfg(test)]`; remove the module-wide allow.

---

## In-place refactor applied in this commit

Two mechanical, zero-risk simplifications (both flagged by clippy):

- **`cranyum/session.rs:62`** — replace
  `io::Error::new(ErrorKind::Other, e)` with `io::Error::other(e)`.
  Identical behaviour, less ceremony. Comment added.
- **`cranyum/main.rs:434`** — replace
  `text.to_ascii_lowercase() == "finish"` with
  `text.eq_ignore_ascii_case("finish")`. Avoids an intermediate
  `String` allocation on every chat-draft submit. Comment added.

Both changes are local, single-line, and verified by `cargo test`.
Broader refactors are deferred to the subtickets above.

---

## Non-findings (considered and rejected)

- **SQLite schema / event_log** — intentionally untouched; acceptance
  criteria require integrity preservation and no schema change was
  necessary for KISS/DRY/YAGNI compliance.
- **`cranyum/board/*`** — already small and focused (mod.rs, column.rs,
  state.rs, task.rs each < 240 lines). No refactor warranted.
- **`cranyum/kanban/chat_edit.rs`** — recently consolidated (see
  CLAUDE.md); leave alone.
