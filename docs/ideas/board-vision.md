# board: vision & architecture

> **Parked.** The board is not the current focus. Code lives under
> `planned/board/` and still uses the crate name `cranyum/` internally;
> the project-level name is `board`. This doc is kept for when the board
> comes back online.

board is a kanban board. relay is a self-organizing knowledge graph. Together
they are the substrate for an automated workflow where many agents work in
parallel, with minimal per-ticket context, on tickets they can address,
resume, and hand off deterministically.

## Premise

Tickets carry the smallest payload that still lets an agent resolve them.
Everything else — project background, prior decisions, file-level history,
cross-ticket relationships — lives in relay and is pulled on demand. The
board is the coordination surface; the graph (relay) is the shared
memory.

A human seeds intent. Agents do the work. The board stays coherent.

## Four properties that must hold

**Repeatability.** Given a ticket and the current repo state, any agent
should reach the same resolution. Tickets describe outcomes, not
procedures. Context is fetched, not baked in — so re-running a ticket on a
later commit still produces the right change.

**Addressability.** Every unit of work points at a concrete anchor:
`path:line`, a symbol, a function span, or a file range. The board never
says "fix the login bug" — it says
`cranyum/agent/harness/adapter.rs:208 — resume ignores session_id`. Agents
negotiate for anchors, not topics.

**Canonicity.** One fact, one home. Decisions, rationale, and invariants
live in relay and are referenced by id. Tickets link to relay nodes instead
of restating them. When a fact changes, it changes in one place and every
dependent ticket sees the update.

**Fine-grained tracking.** Work is decomposed to file+line scope so two
agents rarely touch the same span. When they do, the overlap is visible
before execution — not discovered at merge time.

## Division of labour

- **board** — columns, tickets, links (`blocks`, `related`,
  parent/child), priority, readiness. The ordering and arbitration layer.
- **relay (graph)** — thoughts, reasons, and their edges. Queried via MCP
  for the context a ticket doesn't carry. Agents write back what they
  learn so the next agent starts ahead of them.
- **Organizer agent** — watches the board: promotes ready tickets,
  completes resolved ones, invalidates broken ones, maintains link
  hygiene. Never writes code.
- **Executor agents** — pick a ticket, pull context from relay, produce a
  patch scoped to the ticket's anchors, report back.
- **Human** — drives vision, seeds tickets, steers priority.

## Parallelism model

Agents run concurrently because tickets are scoped to disjoint anchors.
The board is the scheduler: if two tickets touch the same span, a
`blocks` link keeps them serial. If they don't, they run in parallel.
Merge conflicts are a symptom of ticket granularity being wrong, not a
runtime concern — the fix is to re-slice the tickets, not to coordinate
the agents.

Patch-based concurrency (see ticket
`16327973-08db-4d6a-b4a3-1df683c1d02d`) extends this: agents work against
file snapshots, emit patches, and the board applies and reconciles them.
Reads are never blocked by in-flight work.

## Minimal-context ticket flow

1. Human types a line into the chat input. It becomes a candidate ticket.
2. Organizer enriches: resolves anchors, links related tickets, sets
   readiness. Ticket stays terse — enrichment is links, not prose.
3. Executor claims the ticket. Queries relay for background. Reads the
   anchored files. Produces a patch.
4. Organizer verifies, completes the ticket, and records the outcome in
   relay so the next ticket inherits the new state.

The ticket itself never grows. Context grows in relay.

## What this unlocks

- **Resume without replay.** An agent can be stopped mid-ticket and
  resumed later because state lives in relay and patch snapshots, not in
  the agent's conversation buffer.
- **N agents, one repo.** Parallelism is bounded by ticket granularity,
  not by agent count or shared-file contention.
- **Auditable automation.** Every change traces back to a ticket, every
  ticket to anchors and relay nodes. The board is the ledger.

## Related tickets

- `9a2a47ef` — session persistence for harness resume (unlocks resume
  without replay).
- `16327973` — patch-based concurrency control (unlocks safe parallel
  execution).
- `44429973` — threaded Q&A comments (the mechanism by which agents ask
  the board for missing context).
