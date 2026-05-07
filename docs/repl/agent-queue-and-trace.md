# Agent Queue & Trace

Core-engine component. Coordinates multiple agents editing one shared branch without worktrees. Transactional write model: queue = WAL, drainer = scheduler, conflict check = optimistic concurrency control.

Part of Kern runtime. Bound to the project's Kern graph — queue entries reference Kern anchors (`path:line`, symbol, span) so dependency edges come from the graph, not a side-channel.

---

## Goals

- Multiple agents plan and read in parallel.
- Writes serialize through a single drainer that applies non-conflicting edits simultaneously.
- Every agent sees the queue → shared situational awareness, no duplicate work.
- Human commits at checkpoints, not per-edit.

## Non-goals

- Not a replacement for Git. Queue lives above the working tree; commits still flush to Git at checkpoint boundaries.
- Not a distributed consensus system. Single drainer, single branch, single host.
- Not worktree-parallel. This is the alternative to worktrees.

---

## Model

### Entities

**Edit** — proposed mutation. Fields:
- `id` — ULID
- `agent_id` — author
- `anchors[]` — Kern anchors touched (file, symbol, span)
- `read_set[]` — anchors observed during planning (for OCC validation)
- `patch` — textual diff or structured op
- `justification` — why, links to Kern thought nodes
- `deps[]` — edit ids this edit assumes applied
- `status` — `pending | applying | applied | rejected | superseded`
- `created_at`, `applied_at`

**Queue** — append-only log of Edits. Single writer (drainer) mutates status; agents append new edits.

**Trace** — ordered history of applied edits + rejections + reasons. Replayable. Feeds Kern as reasoning edges between thoughts and resulting code deltas.

### Conflict predicate

Two edits conflict if:

1. `anchors` overlap at chosen granularity (see below), **or**
2. Edit B's `read_set` intersects Edit A's `anchors` and A not yet in B's `deps` (stale read).

Granularity:

- **v1: file-level.** Coarse, safe, cheap. Two edits on same file → serialize.
- **v2: symbol-level.** Uses Kern symbol anchors. Two edits on different functions in same file → parallel.
- **v3: span-level.** Line ranges. Maximum parallelism, needs robust range-merge.

Start v1. Upgrade only when measurable contention.

---

## Drainer

Single agent. Loop:

1. Read queue head window (N pending edits).
2. Build conflict graph over window.
3. Select maximal independent set → apply batch in parallel (async writes).
4. For each applied edit: update status, append to Trace, emit Kern edges.
5. For each conflicting edit: either serialize behind blocker or reject with reason.
6. Repeat.

Drainer is deterministic and cheap per-edit *when edits disjoint*. Semantic merge (LLM call) only invoked when textual patch fails or read-set validation trips.

### Scheduling policy

- FIFO by default.
- **Aging** to prevent starvation: edits waiting > T get priority boost.
- **Dependency order**: edit with satisfied `deps` runs before edit waiting on unapplied deps.
- **Priority override**: human/checkpoint edits jump queue.

---

## Worker agents

Flow:

1. Pull task.
2. **Read queue** — see pending edits touching planned area. Avoid duplicate plan.
3. Read code + Kern graph. Capture `read_set`.
4. Produce Edit. Declare `anchors`, `read_set`, `deps`, `justification`.
5. Append to queue.
6. Await drainer ack (`applied` or `rejected`).
7. On reject: read reason, replan or drop.

Agents never touch the working tree directly. All writes via queue.

---

## Rejection & replan

Reject reasons:
- `stale_read` — read_set invalidated by applied edit.
- `unresolved_dep` — declared dep rejected.
- `patch_conflict` — textual apply failed.
- `superseded` — later edit covers same intent.

Policy: worker receives rejection + updated snapshot + list of invalidating edits. Decides: replan, merge, or drop. Cheap replan is the gate on throughput — keep planning prompts short and snapshot-driven.

---

## Checkpoint & commit

- Queue applies continuously to working tree.
- Human (or orchestrator) triggers checkpoint.
- Checkpoint = `git commit` of current working tree + snapshot of Trace segment.
- Trace segment stored with commit metadata → reasoning-to-code mapping persisted.

No per-edit commits. Commits are editorial boundaries, not write boundaries.

---

## Trace

Append-only. Entry per edit outcome:

```
{ edit_id, status, applied_at, anchors, justification, deps_resolved, conflicts[], kern_thought_ids[] }
```

Uses:
- Replay to rebuild state.
- Feed Kern as reasoning edges (thought → code delta).
- Debug: why did agent X's edit get rejected.
- Audit: who changed what, when, why.

---

## Failure modes

- **Drainer dies** — queue halts. Restart replays from last applied edit. Idempotent apply required.
- **Agent dies mid-plan** — edit never appended, no state change.
- **Agent dies after append, before ack** — drainer processes normally, agent on restart reads Trace for outcome.
- **Build breaks after apply** — drainer detects (hook), rolls back last batch, marks edits `rejected: build_break`, notifies authors.
- **Cycle in deps** — drainer detects, rejects all in cycle with reason, asks a coordinator agent to resolve.

---

## Integration

- **Kern** — anchors, read/write sets, justifications, dependency edges, thought linkage all live in the graph. Queue entries are graph-addressable.
- **Kern** — drainer is a Kern plugin (`harness::Plugin`). Worker agents are Kern agents. Queue is a Kern-hosted resource.
- **board** (parked) — surfaces queue + trace in the board view. Operator can pause drainer, promote edits, force rejections, trigger checkpoints.

---

## Status

Design. Not implemented. Build order:

1. Queue storage + append/read API.
2. File-level conflict predicate + single-threaded drainer.
3. Worker agent shim that routes writes through queue.
4. Trace persistence + Kern edge emission.
5. Parallel batch apply.
6. Symbol-level granularity.
7. board UI (parked).

Measure at each step: queue depth, drain latency, rejection rate, replan cost. Widen granularity only when contention visible.

---

## Open questions

- Snapshot isolation strategy for reads — MVCC vs lock-read.
- Cross-file atomic edits (rename across 20 files) — single edit or edit-group?
- LLM cost of semantic merge — cache? fall back to reject?
- Interaction with external edits (human typing in editor) — treat as queue entry or forbid?
