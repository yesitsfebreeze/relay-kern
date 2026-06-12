# Day-memory: out-of-band journal compaction + Obsidian daily digest

- **Date:** 2026-06-12
- **Status:** Design approved; pending implementation plan
- **Scope:** kern only (kern is a standalone workspace; no relay/agnt coupling)
- **Related work this session:**
  - `0be4e26` — relocated `shared/*` crates into `src/` (kern self-contained)
  - `ba1dcce` — fix(session_mirror): tail `today.jsonl` instead of the empty SQLite archive (Fix B)

## Problem / background

The journal has three intended layers but only one worked:

- `today.jsonl` — live, append-only JSONL event stream for the current day. Works.
- `history.db` — SQLite archive of past days, intended to be **machine-queryable** by kind/key/time. **Never populated in production**: every `DayJournal::open` passed `NullHistorySink`, so the rollover → `History::bulk_insert` bridge dropped every closed day. Old days were *discarded*, not archived.
- (new) a human-browsable **Obsidian** view of past days.

Fix B already repointed `session_mirror` at the live `today.jsonl`, so **nothing machine-reads `history.db` today**. This design makes the archive real *and* adds a condensed "memory of the day" digest, driven from both the journal and the kern knowledge graph.

## Goals

1. **Durable, machine-queryable archive of past days** in `history.db` (kind/key/time), actually populated.
2. **Crash-safe, out-of-band compaction** that keeps the hot emit/rollover path cheap.
3. **Optional "memory of the day"**: an LLM-distilled highlight per past day, synthesized from the journal (activity) + the kern graph (knowledge), written as Obsidian markdown organized `year/month/day`. Toggleable in settings, **off by default**.

## Non-goals

- No change to `today.jsonl` as the live hot stream.
- No replacement of the kern graph as the durable knowledge store; the digest is a *view*.
- No dependency on an external "Obsidian CLI"; kern writes plain `.md` via `std::fs` and Obsidian opens the folder as a vault.
- No relay/agnt changes.

## Architecture

```
emit ──► today.jsonl ──(rollover: day-change or byte-cap)──► rename to
          (live)                                              journal/segments/YYYY-MM-DD-HHMMSS.jsonl
                                                                       │
                                          (background, out-of-band)    ▼
                                              compactor task ──► History::bulk_insert → history.db   (machine archive)
                                                     │
                                                     └─(day fully compacted & toggle on)─► day-memory generator
                                                                                              │  journal events (curated)
                                                                                              │  + graph entities of the day
                                                                                              ▼
                                                                                          Ollama distill ─► <vault>/YYYY/MM/YYYY-MM-DD.md
```

### A. Hot path — rollover becomes a rename (journal crate)

`DayJournal` rollover (in `day_journal.rs`) currently: read all entries → `history.bulk_insert` → rewrite `today.jsonl` with a fresh header. Change to:

- On rollover (day-change **or** byte-cap), **rename** the closed `today.jsonl` to `journal/segments/<created_day>-<HHMMSS>.jsonl`, then write a fresh `today.jsonl`.
- The `<created_day>` prefix comes from the closed file's header (`Header.created_day`), so a byte-cap segment and the later day-change segment for the same day share the `YYYY-MM-DD` prefix and compact into the same day. `<HHMMSS>` (wall-clock at rollover) disambiguates multiple segments per day.
- **Remove** the `HistorySink` coupling from `DayJournal`: drop the `Arc<dyn HistorySink>` constructor parameter and the rollover `bulk_insert` call. Archival is now the compactor's responsibility. Update `open_default()` and tests to the simpler `DayJournal::open(project_root)` signature.
- `Date.now`-style wall-clock is already used (`today_str`, `now_ms`); segment timestamp reuses `now_ms`/local time.

Rationale: the hot path no longer touches SQLite under the journal mutex, and the dead `NullHistorySink` bridge is gone. Renames are atomic on the same filesystem, so a closed day is never partially observed.

### B. Compactor task (kern) — out-of-band, crash-safe

A background task spawned at daemon startup (sibling of `spawn_session_mirror` in `commands.rs`). Every `compactor_interval`:

1. Glob `journal/segments/*.jsonl`.
2. For each segment (oldest first): parse entries via `journal::scan_path`; `History::bulk_insert` the rows into `history.db`.
3. **Delete the segment only after the insert commits.** A crash between insert and delete re-inserts on retry — so inserts must be idempotent (see F).
4. Track which days became "complete" this pass (a day is complete when no `today.jsonl` still carries that `created_day` and all its segments are compacted). For each newly-complete *past* day, enqueue a day-memory render (D) if the toggle is on.

The compactor owns the only writer to `history.db` in the daemon, avoiding concurrent-writer contention.

### C. SQLite archive (mostly exists)

`History` (`history.rs`) is unchanged in shape: `bulk_insert`, `query(Filter)`, `count_by_key`, `retain_days`/`prune_before`. It is now fed by the compactor. The `kind_tag`/`kind_from_tag` dual-encoding stays — it is the archive's serialization and is justified now that the archive is real and machine-queried. `retain_days` pruning stays (best-effort, at startup as today).

### D. "Memory of the day" generator (kern)

For a fully-compacted past day `D`, gather:

- **Journal side (from `history.db`, day = D):** curated kinds only — `ForkOpen/Resume/Close` (sessions), `PlanProposal`/`PlanStep`, `Goal`/`GoalSnapshot`/`Milestone`, `ToolCall` (aggregated by key/count), `EntityTouched`. Exclude `Log` (tracing noise).
- **Graph side (kern memory):** entities with `created_at` on day D, plus entities referenced by that day's `EntityTouched` events (AgentRead/AgentWrite/FsWrite) — i.e. what the day actually engaged with. Pull id, kind, a short label/statement.

Feed a compact rendering of both to the LLM via kern's existing distill path (Ollama). The LLM is invoked behind a trait (`DayDigestLlm`) so tests stub it. Output: a short highlight (prose) — the condensed "memory of the day".

### E. Obsidian export + config

- Write `<vault>/YYYY/MM/YYYY-MM-DD.md`: the LLM highlight, followed by a compact structured footer (sessions list; key entities as `[[wikilinks]]` by external_id/label). Directories created on demand.
- New `[journal]` config keys:
  - `obsidian_export: bool` — default `false`.
  - `obsidian_vault: PathBuf` — required when export is on; no default vault is written when off.
  - `compactor_interval_secs: u64` — default `60`.
- Unchanged: `today.jsonl`, `retain_days`.

### F. Failure handling, idempotency, crash-safety

- **Commit point = SQLite insert.** Markdown is best-effort *after* archival; a markdown/LLM failure never blocks or loses the archive.
- **Idempotent insert.** A crash after insert / before segment-delete re-inserts on retry. Either (a) make `bulk_insert` upsert-safe with a natural key (e.g. `(ts_ms, kind, key)`), or (b) gate per-segment with a small `compacted_segments(name)` table checked before insert. Plan picks one; (b) is simpler and exact.
- **Ollama down:** the day's `.md` is left pending (a per-day "digest done" marker is *not* written); retried on a later compactor cycle. Archive is already durable. Persistent outage → the day stays archived in SQLite without a note (acceptable; user can regenerate later).
- **Markdown re-render** overwrites the same `YYYY-MM-DD.md` path — idempotent.

### G. Testing strategy (TDD)

Pure, deterministic units, LLM stubbed:

- Segment naming + day-grouping (byte-cap + day-change segments for one day group together).
- `compact_segment(path) -> Vec<Entry>` round-trips through `History` and is idempotent under re-run (crash-retry).
- Day-input gathering: given seeded `history.db` rows + a seeded graph, the curated journal set excludes `Log` and the graph set picks created-on-day + touched entities.
- Markdown rendering: deterministic given a stubbed `DayDigestLlm` (assert path, sections, wikilinks).
- Crash-safety: a segment survives a simulated failure between insert and delete and is not double-inserted (marker table).

### H. Out of scope / interactions / open questions

- **`session_mirror` (Fix B)** is unaffected: it tails live `today.jsonl`. Minor accepted edge: a fork opened within one poll interval before a midnight rollover lands in a segment, not `today.jsonl`, so the live mirror may miss it (the session is still archived).
- **`history.db` writer:** the compactor becomes the sole daemon writer; the startup `retain_days` prune also opens `History` — both use the same path, serialized by SQLite WAL. Confirmed acceptable.
- **Open (for the plan):** exact idempotency mechanism (marker table vs upsert key); whether the compactor runs on a timer only or also wakes on rollover; vault-path validation/UX when `obsidian_export` is on but `obsidian_vault` is unset (fail-soft warn + disable).

## Migration

Per repo law (no compat, clean base, version stays 1.0.0): the `DayJournal` ctor signature change and the removal of the rollover `bulk_insert` are a clean break; existing empty `history.db` files and any `today.jsonl` are compatible (the compactor simply starts populating going forward). No on-disk `Kind`/`Entry` schema change (`SCHEMA_VERSION` unchanged), so existing JSONL replays.
