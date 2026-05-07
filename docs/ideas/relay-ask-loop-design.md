# Relay Ask Loop — Design

Date: 2026-04-25
Status: Design (approved)
Replaces: ad-hoc ticket boards for the relay/relay repo

## Goal

Replace external ticket systems with an inline, journal-backed
conversation loop between long-lived per-file executor agents and
the user. Agents work toward goals; when blocked on a decision, they
raise a question into the journal. The user answers on a fixed
cadence via a top-five notification bubble. Reactive throughout — no
cascading edits without the user's engagement.

## Layered model

The ask loop sits inside a three-layer architecture. This spec defines
L2's surface and L3's kern-on-tap routing; L1 is named here so the
later layers have a stable place to plug in.

- **L1 — Orchestrator.** Reads the global journal + kern history +
  ambient state, derives what the user is currently steering toward,
  and decides which file-scoped executor agents to spawn or wake. The
  orchestrator is *the* long-running agent for a project session;
  everything else is short-lived and file-scoped. Not implemented in
  v1; this spec leaves a single insertion point (the kern host
  forwarding goal entries to the executor pool).
- **L2 — Milestones, not tickets.** Goals and milestones are journal
  entries. Searchable, scoped, and gating. Replaces external ticket
  boards: "what's next" is a journal scan filtered by `Goal` /
  `Milestone` kinds. Releases are cut when a milestone reaches
  `Reached` status.
- **L3 — kern as facilitator.** One kern window per project. The
  window facilitates milestone-driven work: search the journal,
  surface open asks via the bubble, route resolutions back through
  the executor. Multiple projects = multiple windows (or tabs); a
  single project never needs more than one.

### Branching posture

The inner loop **does not require a git branching workflow**. The
project commits continuously toward the active goal on the main
branch; the milestone-gated journal entries are what mark
releaseable points. Humans review code, exercise features, and gate
releases by flipping a milestone status (or by approving the
orchestrator's release proposal). Worktrees still exist for the
filesystem, but the loop does not orchestrate `git worktree` for
isolation — that ceremony is replaced by per-file executor scoping.

### Single project, single window

Project context lives in one kern window. Tabs (or extra windows)
exist only when working across multiple projects, so the L1
orchestrator's view of "the journal" stays unambiguous. The window
shows the project root prominently so the human always knows which
journal is in scope.

## Why

The repo already has every primitive needed:

- `ping-pong` rolling pattern: `.rs` + `.rs.prev` baseline per file,
  `@score` worst-first triage, version-bump promotion.
- `agent-queue-and-trace`: edit queue + drainer + replay journal.
- `journal/`: append-only event log.
- `ui-slots`: declarative chrome with TTL, priority, plugin-driven
  cadence (timer / lifecycle / file-watch).

Missing piece: a **structured way for an agent to surface a decision
to the user, and for the answer to flow back into the file's
context** without spawning kern-orphan threads or growing context.

This design connects the existing pieces into one loop and makes
ticket boards redundant for the relay use case.

## Invariants honored

- **`relay` is the product.** This loop produces journal entries and
  Edits; it does not put an LLM call on retrieval.
- **Writes pay once; reads pay nothing.** Bubble priority = pure
  journal scan + fixed formula. No model call on render.
- **Addressable anchors.** Question lives in code as a greppable
  marker (`//? Q#<ulid>`), in journal as the structured entry.
- **Edges carry justification.** Answers reference the ask; resolved
  asks become trace edges in relay.
- **Agent roles stay hookable.** Executor today; later roles can
  raise asks the same way.

## Architecture

```
                ┌──────────────────────┐
   goal ──►     │  executor (warm)     │  per .rs file
                │  ctx: .rs+.rs.prev   │
                │       + journal IDs  │
                └──────────┬───────────┘
                           │ writes Q#<ulid>
                           │ appends ask
                           ▼
                   ┌────────────────┐
                   │   journal      │  source of truth
                   └───┬────────┬───┘
                       │        │
            scan+rank  │        │  ref by ulid
                       ▼        ▼
                ┌──────────┐  ┌──────────────────┐
                │ bubble   │  │ kern (on tap)    │
                │ top-5    │  │ exec-warm ctx    │
                │ slot     │  │ + all open asks  │
                │ above-in │  │   for that file  │
                └────┬─────┘  └────────┬─────────┘
                     │                 │
                     │ user taps       │ resolve
                     └────────►◄───────┘
                                       │
                                       ▼
                              ┌────────────────┐
                              │ Edit → drainer │
                              │ → applied      │
                              │ → journal      │
                              └────────────────┘
```

### Components

#### Executor agent (per `.rs`)

- **Lifespan**: warm. One per file under active work. Stays resident.
- **Context**: `.rs` + `.rs.prev` + ordered list of journal entry IDs
  the executor has authored or that reference its file. Storage is
  small — IDs are ULIDs, full bodies fetched on demand.
- **Behavior**: ping-pong iteration on its file. When blocked on a
  decision it cannot resolve from `.rs.prev` alone, it:
  1. Writes `//? Q#<ulid> <short hint>` in `.rs` at the decision point.
  2. Appends a journal entry `{kind: ask, id: <ulid>, file, text, …}`.
  3. Pauses work on that file. Waits for `answer` referencing the ask.
- **Resume**: poll-on-tick or push-on-journal-write. When answer lands,
  executor either applies the decision directly (for trivial answers)
  or enqueues an `Edit` to the existing drainer. After application,
  marks the ask `answered`, removes (or rewrites) the `Q#<ulid>` marker.

#### Journal entry shape

```
JournalEntry {
  id:        ULID              // doubles as Q#<id> code marker
  kind:      "ask" | "answer" | "goal" | "milestone"
  file:      path?             // present for ask/answer; absent for goal
  ts:        timestamp
  agent_id:  ULID
  text:      string
  ref_id:    ULID?             // answer→ask, milestone→goal
  status:    "open" | "answered" | "stale"
  tags:      ["design"|"behavior"|"safety"|"nit"]
}
```

- ULID gives sortable-by-time + unique-by-construction.
- Priority is **not** a stored field — computed by the bubble plugin at
  scan time from `@score`, age, executor block-state, and tags.
- `status` transitions: `open → answered` on resolve;
  `open → stale` if file deleted or goal abandoned.

#### Markers in code

```rust
//? Q#01HW2K3M4N5P6Q7R  reduce alloc here? alt: arena
fn hot_path() { ... }
```

- `//?` is the discriminator. Greppable. Doesn't collide with rustdoc.
- Optional short hint after the ULID is for the human reading the file
  in an editor; full text lives in the journal.
- After resolution, the executor removes the marker as part of the
  Edit it dispatches. Trace edge in relay preserves the history.

#### Bubble

- **Plugin**: `relay-ask-bubble` ui-slot plugin.
- **Slot**: `above_input`, right zone, priority 80.
- **Cadence**: timer 30s + push on journal-write of `kind=ask`. Slot
  TTL refreshes each tick.
- **Render**: top-5 by priority. Each row:
  `[!|⚠|·] <file>:<symbol-hint>  <truncated text>  <age>`
  Style by tag (`safety` red, `design` cyan, `behavior` yellow,
  `nit` dim).
- **Action**: pressing the bubble's hotkey (or tapping a row) selects
  an ask; relay loads its file's kern (see below).

#### Priority formula

```
priority = w_score · (100 - file.@score)
         + w_age   · age_seconds_log
         + w_block · (1 if executor is paused else 0)
         + w_safety · (1 if "safety" in tags else 0)
```

Default weights tuned during impl; pinned in config. No LLM in this
formula. Bubble re-ranks on each tick.

#### kern-on-tap routing

When the user taps an ask:

1. Relay resolves the file's executor agent (warm; spawn if absent).
2. Loads kern with:
   - The selected `ask` (full text from journal).
   - **All other open asks for the same file** (B in design Q).
   - The executor's current `.rs` + `.rs.prev` (truncated, hook-budgeted).
   - The most recent N journal entries for that file.
3. User and executor converse. Decision reached.
4. Executor enqueues `Edit` (per `agent-queue-and-trace.md`).
5. Drainer applies, journal `applied` entry references all asks the
   Edit resolves.
6. Asks flip to `answered`. Bubble drops them on next tick.

#### Goal / milestone

- **Goal** = `{id, text, scope: file[]|dir|all, milestones: [milestone_id…]}`.
  Pinned to a relay thought node so the conversation can reference it
  by ID without bloating kern context.
- **Milestone** = `{id, criteria, status}` where criteria can include:
  - `min_score: int` — repo or scope-mean `@score` floor.
  - `asks_resolved: true` — no `open` asks within scope.
  - `tests_pass: true` — full suite green.
- When all criteria met, relay emits a journal `milestone_reached`
  entry; bubble surfaces it in a separate row style ("done" not "todo").
- Replaces ticket boards: goals are the user-stated direction, asks
  are the only friction surface, milestones are the done signal.

## Failure modes

- **Marker drift**: someone hand-edits `Q#<id>` text. Source of truth
  is journal; marker is just an anchor. Drift is cosmetic.
- **Orphan marker**: marker present, journal entry missing. Detected
  on scan; logged, marker removed by next executor pass.
- **Orphan ask**: journal entry present, marker missing. Status flips
  to `stale` on next file-watch event.
- **Executor restart**: warm context lost. Re-warm by reading `.rs` +
  `.rs.prev` + journal-IDs-for-file. Cost = one file read + bounded
  journal scan. Acceptable.
- **Drainer rejects Edit after answer**: ask reverts to `open` with
  appended note; user re-engages.

## Journal lifecycle (rolling cache)

The journal is **not** long-term storage. It is a rolling working-set
cache. Relay is the long-term memory. Both are ephemeral by design:
the journal expires by time, relay expires by similarity and
confidence. Nothing about either is permanent — both are scaffolding
for the user's current direction.

The wiring already exists in the tree:

- `src/relay/journal/day_journal.rs` — daily JSONL files.
- `src/relay/journal/history.rs` — multi-day window readback.
- `src/relay/journal/relay_sink.rs` — outbound ingest path.
- `src/relay/journal/state.rs` — replay state.

This design adds entry kinds (`ask`, `answer`, `goal`, `milestone`),
a compaction job, and the bubble — it does not invent the storage.

### Three-stage compaction

Memory shrinks at three layers; each layer pays once and the next
layer reads near-free.

1. **In-day (per orchestrator turn)**. The orchestrator runs against
   the day's journal + ambient state and compacts its own working
   context as turns accumulate. What survives the turn is a
   summary + the pinned goal + the latest tool-call results — not
   the full transcript. Journal still records full prose; the
   orchestrator's *prompt* is what shrinks.

2. **Day rollover**. At local-day boundary `DayJournal::rollover_locked`
   already moves entries to history. We extend that hook to summarise
   resolved ask/answer pairs, milestones, and goal updates into relay
   thoughts before the SQLite handoff. The day's narrative is gone;
   its decisions live as graph nodes.

3. **Window expiry (30-day boundary)**. Entries leaving the rolling
   window get a final compaction pass into relay and are pruned from
   `History` (`History::prune_before` already does the prune; the
   compaction step is what's new). Anything load-bearing has been
   referenced often enough that relay retains it; the rest fades.

Net effect: long-term storage is the relay graph plus, at most, the
last 30 days of warm journal history. Decisions persist; prose
doesn't. The orchestrator's per-turn budget stays bounded because
stage 1 keeps the working slice small.

- **Window**: 30-day rolling. On startup, drop entries older than the
  window after ensuring they ingested cleanly into relay. Default;
  configurable.
- **Compaction**: at day rollover (stage 2), entries leaving the
  current day pass through `relay_sink`:
  - Resolved `ask` + matching `answer` → one relay `thought` (the
    decision) with a justification edge to the file's crate-doc
    thought, plus an edge from the executor's running goal.
  - `milestone_reached` → `thought` with edges to all asks it closed.
  - Unresolved `ask` older than the window → flipped to `stale`,
    surfaced once in the bubble before drop. User can re-raise or
    discard.
- **Relay self-compaction** (mechanism, not new):
  - **Similarity guard at ingest**: near-duplicate thoughts are
    skipped or merged into the existing thought rather than added.
  - **Supersede**: a refined thought replaces an older one via
    `forget` + re-`ingest`; old ID becomes a redirect.
  - **Confidence fade**: low-confidence thoughts surface for prune
    on periodic sweeps. Stale knowledge fades; load-bearing
    knowledge gets reinforced by repeated retrieval.
- **Replay**: rollback inside the window = journal replay (lossless,
  prose-level). Outside the window = relay thoughts (lossy on prose,
  lossless on decisions and edges).
- **Cost shape**: compaction is the only LLM-heavy step in this loop
  and runs off the read path at day rollover. Honors "writes pay
  once, reads pay nothing."

Net effect: "what did I do last month" stops being a journal problem.
Decisions live as graph nodes, weighted by use, fading when no longer
load-bearing. The user steers; the system controls direction by
shaping relay, not by accumulating logs.

## Journal as roadmap

The journal does double duty: working memory **and** roadmap. Goals
and milestones live in the same JSONL stream as ask/answer entries,
so "find the current goal" and "show me what's blocking release" are
both `History::query(Filter)` calls. No separate roadmap file, no
separate ticket store.

A new kern-side helper (post-v1) will let the user define a
milestone in conversation:

```
> /milestone create "ship ask-loop v1" criteria asks_resolved tests_pass
```

This appends a `Milestone` entry; the orchestrator subsequently
references it when narrating progress. Search-by-text (existing
`History` filter machinery + relay retrieval for older entries) is
how we pull context to reach a milestone.

## Out of scope (v1)

- Cross-file ask clustering via relay edges. Defer (v2).
- Symbol-level conflict granularity. Defer (qtrace v2).
- Human-driven priority override on bubble. Add when triage shows it
  matters.
- Multiple users. Single-user assumption holds.
- Notification routing outside relay (no Slack, no email). Bubble is
  the only surface.

## Build order

1. Journal entry kinds `ask` / `answer` (extend existing journal).
2. ULID code marker convention + grep utility (`relay-ask-scan`).
3. Bubble ui-slot plugin reading journal head + priority formula.
4. kern-on-tap routing through existing executor agent.
5. Executor pause/resume on `ask` write / `answer` arrival.
6. Edit emission on resolve, wired into existing drainer.
7. `goal` + `milestone` entries + done-signal rendering.
8. Daily journal compaction → relay ingest at 30-day boundary.
9. Relay edges for resolved-ask provenance (after v1 lands).

Each step lands behind the existing journal/queue, so partial rollout
degrades to "asks visible, not actionable" — never breaks the loop.

## Open questions (post-spec, for impl)

- Exact cadence: 30s default, configurable via `ui-slots.toml`?
- Marker rewrite vs removal on resolve — keep `A#<id>` ghost for
  audit, or rely on relay trace? Lean toward removal + trace.
- Goal-pinned kern: pin via `relay.thought_id` in kern metadata, or
  reference by goal ULID? Prefer ULID; thought_id pinning is for
  long-term memory.

These are tuning, not architecture. Defer to plan.
