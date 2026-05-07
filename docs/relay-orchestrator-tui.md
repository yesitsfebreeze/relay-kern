# Relay Orchestrator TUI — PRD v1

Status: drafted 2026-05-06. Phase 3 of `/coder`. Builds on
`docs/relay-search-tui.md` (slices A–E + palette wiring).

## Vision

`relay` exe replaces the legacy `repl` binary as the single user-facing
TUI. Two top-level views — **Main** (orchestrator chat + rolling plan
+ agent strip + search palette) and **Editor** (file/Entity editor) —
flip with `Ctrl-E`. The Editor can replicate an agent (`Ctrl-Shift-A`)
forking with file/line/selection as opening context.

## Non-goals (v1)

- Custom EntityKind set per database (canonical 7 stays locked).
- Multi-orchestrator. One orchestrator chat, many sub-agents.
- Mouse drag to resize panes.
- Cross-kern federated orchestration.
- Edit-mode for Entities other than files (Documents/Claims preview-only).

## Decisions (locked)

| # | Choice |
|---|--------|
| 1 | Rename `src/bin/repl/` → `src/bin/relay/`; fold legacy `relay/src/{file_picker,main}.rs` into `relay/src/editor/`; legacy `relay` thin-wrapper retired |
| 2 | Plan = journal events `kind=plan_step` (append-only, replayable) |
| 3 | Main layout: chat-left, plan-right, agent-strip-bottom |
| 4 | Sessions persisted: journal `fork_*` events as truth + kern `Document` mirror w/ `source=session://fork-id` |
| 5 | Typed `KernRpc` service: `query`, `ingest`, `link`, `neighbors` (extends slice B's `SearchSvc`) |
| 6 | Plan write authority = orchestrator only; sub-agents emit `kind=plan_proposal` for review |
| 7 | View toggle `Ctrl-E` Main↔Editor |
| 8 | Editor → agent replicate `Ctrl-Shift-A`: `fork_at(parent_id, anchor)` w/ `{file, byte_range, selection}` as opening ctx |
| 9 | Editor opens files + Entity preview from palette Cards |
| 10 | Recents = in-memory MRU ring (64) backed by journal `kind=entity_touched` events |
| 11 | Recents UX = palette empty-state shows MRU + `Ctrl-O` back / `Ctrl-Shift-O` forward |
| 12 | Touch ops = open/drill/@mention/agent-rw/fs-write |
| 13 | Sub-agents do NOT auto-open Editor tabs — agent-strip drill only |

## Glossary (lock)

- **Main view** — chat (L) + plan (R) + agent-strip (B) + palette overlay.
- **Editor view** — file editor + Entity preview pane; folded from legacy relay.
- **View** — top-level enum `{ Main, Editor }`; toggled by `Ctrl-E`; per-view state preserved across toggles.
- **Plan** — ordered journal-backed step list. Step: `{id, parent: Option<id>, status: Pending|Active|Done|Blocked, body, ts}`.
- **Plan panel** — right pane in Main view rendering current plan.
- **Plan proposal** — sub-agent journal event suggesting a step; surfaces in panel as pending; orchestrator accepts → emits `plan_step`.
- **Agent strip** — bottom row, one tile per active fork: `{fork_id, state, last_msg_ts}`. Drill = open overlay w/ fork chat history.
- **Recents** — MRU ring of `EntityRef`s; oldest evicted at 64; journal-backed for replay.
- **Touch op** — discrete user/agent action that records an `entity_touched` event: `Open|Drill|Mention|AgentRead|AgentWrite|FsWrite`.
- **Anchor** — fork-attached snapshot `{entity_id, source_uri, byte_range, fork_id}` carrying caller context into a replicated fork.
- **Replicate** — `KernRpc::fork_at(parent, anchor) -> new_fork_id`.
- **KernRpc** — typed trnsprt service exposing kern's read+write surface to repl/agnt: `query`, `ingest`, `link`, `neighbors` (and `truncate_after` already there).
- **SearchSvc** — slice-B service; remains. KernRpc and SearchSvc are siblings, may share DTOs.

## Architecture

```
+--------------------- relay (TUI binary) ---------------------+
|                                                              |
|  view: Main | Editor   (toggle Ctrl-E)                       |
|                                                              |
|  Main view:                                                  |
|   ┌─ chat (orch) ───────┬─ plan ───┐                         |
|   │                     │          │                         |
|   │ ChatView messages   │ steps    │                         |
|   │                     │          │                         |
|   ├─ agent strip ───────┴──────────┤                         |
|   │ [a] [b] [c] [d]               │                         |
|   └────────────────────────────────┘                         |
|   palette overlay (Ctrl-P)                                   |
|                                                              |
|  Editor view:                                                |
|   ┌─ file tree ─┬─ buffer ─────────┐                         |
|   │             │ tree-sitter HL   │                         |
|   │ palette can │                  │                         |
|   │ open file   │ Ctrl-Shift-A     │                         |
|   │             │   replicates     │                         |
|   └─────────────┴──────────────────┘                         |
|                                                              |
|  Shared services:                                            |
|   - palette (ctrl-P, w/ recents empty state, Ctrl-O nav)     |
|   - recents (MRU ring → journal entity_touched)              |
|   - status_bar (view, fork_id, agent count)                  |
|                                                              |
|  Clients (typed trnsprt):                                    |
|   - AgntRpcClient (forks, turns, output)                     |
|   - KernRpc (query, ingest, link, neighbors)                 |
|   - SearchSvc client (palette backend adapter)               |
+--------------------------------------------------------------+

      ▲ trnsprt typed channels
      │
+-----+-------+    +-------+    +--------+
|    agnt     |    |  kern |    | watcher|
|  forks +    |◄──►| graph |◄──┤ files  |
|  recipes    |    |  + RPC│    +--------+
+-------------+    +-------+
```

## Module map (planned, on top of slices A–E)

- `src/bin/relay/` (was `src/bin/repl/`) — every existing module preserved.
- `src/bin/relay/src/view.rs` — `enum View { Main, Editor }`, toggle handler, focus restore per view.
- `src/bin/relay/src/main_view/` — chat panel + plan panel + agent strip composition.
- `src/bin/relay/src/main_view/plan_panel.rs` — rolling plan render + journal subscription.
- `src/bin/relay/src/main_view/agent_strip.rs` — fork tile row + drill overlay.
- `src/bin/relay/src/editor/` — folded from legacy `src/bin/relay/src/{file_picker,main}.rs`. Editor view: buffer, file tree, syntax highlight (uses tui::highlight from slice C).
- `src/bin/relay/src/recents.rs` — MRU ring, journal event emit, replay on start.
- `src/shared/journal/src/events.rs` — extend with `Event::PlanStep`, `Event::PlanProposal`, `Event::EntityTouched`, `Event::ForkOpen`, `Event::ForkClose`. (Or new module if events live elsewhere — verify existing layout first.)
- `src/shared/trnsprt/src/kern_rpc/` — typed `KernRpc` service: `query`, `ingest`, `link`, `neighbors`, `fork_at`. Mirror `SearchSvc` shape.
- `src/bin/agnt/src/kern_client.rs` — extends current `MemoryClient` to full KernRpc client; sub-agent recipes get a kern handle.
- `src/bin/kern/src/rpc/kern_rpc.rs` — server impl of typed KernRpc.
- `src/bin/kern/src/ingest/session_mirror.rs` — journal `fork_*` reader → ingest as `Document` w/ `source=session://`.
- `src/bin/relay/src/orchestrator/` — orchestrator state machine: when to spawn sub-agent, when to surface proposal, plan accept/reject.

## User flows

### F1 — Cold start
1. `relay` launched. View defaults to Main. ChatView empty. Plan panel empty. Agent strip empty. Palette closed.
2. Recents replay reads journal `entity_touched` events (last 64) to seed MRU ring.

### F2 — Search and drill
1. `Ctrl-P` opens palette. Empty input → recents shown (slice 11).
2. Type `borrow checker` → kern `SearchSvc::search` returns Cards.
3. `Enter` on file Card → records `entity_touched(Open, file)`, switches to Editor view (per #9), opens buffer.
4. `Ctrl-E` switches back to Main; palette state preserved.

### F3 — Plan write + sub-agent spawn
1. Orchestrator chat: user submits "Refactor auth middleware".
2. Orchestrator emits `plan_step { body: "Audit token expiry check", status: Active }` to journal.
3. Plan panel updates from journal stream.
4. Orchestrator decides to spawn sub-agent → `agnt::fork_open(parent=orch, recipe=audit)`.
5. Agent strip gains tile `[audit]`. Sub-agent runs, writes journal `entity_touched(AgentRead, file)`.
6. Sub-agent emits `plan_proposal { body: "Replace `<` with `<=` at auth.rs:42" }`.
7. Plan panel renders proposal as pending; orchestrator types `/accept` → proposal → `plan_step`.

### F4 — Editor replicate
1. Editor view, buffer at `auth.rs:42`, selection covers token check.
2. `Ctrl-Shift-A` → builds anchor `{entity_id=file://.../auth.rs, byte_range, selection}` → calls `agnt::fork_at(current_fork, anchor)`.
3. New fork created with anchor as opening message. Agent strip gains new tile.
4. Switch to Main: chat shows new fork's first turn (referencing the anchor).

### F5 — Recents back/forward
1. User drills through 5 Cards. Recents ring: `[c1, c2, c3, c4, c5]` (newest left).
2. `Ctrl-O` jumps to `c4`, sets cursor on ring.
3. `Ctrl-O` again → `c3`.
4. `Ctrl-Shift-O` → `c4`.
5. New touch (e.g. open `c6`) truncates forward history (browser semantics): `[c6, c4, c3, c2, c1]`.

## Test plan (TDD scaffolds, phase 4)

| Slice | Test |
|-------|------|
| F (rename + view switch) | Ctrl-E toggles View; per-view focus restored; legacy editor file_picker reachable from Editor view |
| G (plan panel) | journal `plan_step` event → panel update; status transitions render distinctly; ordering by ts |
| G (proposal) | journal `plan_proposal` → panel pending row; `/accept` flips to `plan_step`; `/reject` drops |
| H (agent strip) | spawn fork → tile appears w/ id+state; tile drill opens overlay w/ chat |
| I (recents) | touch sequence → MRU order; eviction at 64; journal replay on start; Ctrl-O/Ctrl-Shift-O cycle; truncate forward on new touch |
| J (KernRpc) | typed roundtrip for query/ingest/link/neighbors; mock server matches contract |
| K (session mirror) | journal `fork_open` → kern `Document` w/ `source=session://...`; palette `:session` facet finds it |
| L (replicate) | Ctrl-Shift-A in Editor → `fork_at` called w/ correct anchor; new tile in strip |
| M (Entity preview) | palette Card Enter on Document → Editor view opens preview pane w/ tree-sitter highlight |

## Risks

- **Bin rename diff size**: rename touches every import path of `repl::*`. Mitigation: solo step, mechanical sed + cargo check; commit one rename then incremental edits.
- **Journal event schema growth**: adding 5 new event kinds risks deserialisation drift. Mitigation: enum tagged with serde, test backwards-compat replay (against existing journals if any).
- **MRU vs heat collision**: two recency signals could confuse. Mitigation: clear glossary lock, separate code paths.
- **Plan panel re-render thrash on hot journal stream**: Mitigation: panel reads from a debounced subscriber, not raw stream.
- **Sub-agent surface explosion**: orchestrator decides spawns. Mitigation: keep recipe palette small in v1; sub-agent always has same KernRpc handle, no plugin variance.

## Out of scope

- Mouse, drag-resize, multi-window.
- Cross-fork plan visibility (all forks share orchestrator's plan in v1).
- Plan dependency graph beyond linear list (parent edge supported in schema, no UI in v1).
- LLM-summarised plan rollups.
- Voice input.

## Slice plan (phase 5)

Sequential first (foundation, mass rename):
- **F**: bin rename `repl→relay`; fold legacy editor; add View enum + Ctrl-E.

Parallel after F:
- **G**: rolling plan model (journal events) + plan panel UI.
- **H**: agent strip + drill overlay.
- **I**: recents (MRU ring + journal events + Ctrl-O nav + palette empty state).
- **J**: typed KernRpc service (server in kern, client in trnsprt, used by agnt+relay).
- **K**: session mirror (journal scan → kern Document `source=session://`).
- **L**: editor agent-replicate (Ctrl-Shift-A → fork_at w/ anchor).
- **M**: Entity preview in Editor view (palette Card → Editor).

## Open follow-ups (carry-overs from prior PRD)

1. kern-side SearchSvc server impl (not yet built; slice J overlaps).
2. TrnsprtSearchBackend adapter (palette uses MockSearchBackend today).
3. Watcher → kern `IngestSink` impl.
4. service! macro generic frame for bincode wire.
5. Pre-existing agnt star/poke build break — separate fix.
