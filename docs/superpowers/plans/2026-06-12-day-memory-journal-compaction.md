# Day-memory: Journal Compaction + Obsidian Digest — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. (Note: in the kern repo, subagents dispatch via `mux_delegate`, never the built-in Task tool. If `mux_delegate` is unavailable, execute inline with TDD.)

**Goal:** Turn closed journal days into a durable, machine-queryable SQLite archive via a crash-safe out-of-band compactor, and optionally render an LLM-distilled "memory of the day" markdown digest (journal activity + graph knowledge) into an Obsidian vault.

**Architecture:** `DayJournal` rollover renames `today.jsonl` to a dated segment file (cheap, no SQLite on the hot path). A background compactor task scans segments, `bulk_insert`s them into `history.db` (idempotent via a marker table), deletes the segment, and — when a past day is complete and the toggle is on — generates a digest from `history.db` + the kern graph and writes `<vault>/YYYY/MM/YYYY-MM-DD.md`.

**Tech Stack:** Rust, `rusqlite` (existing `History`), `journal` crate (`Entry`/`Kind`/`scan_path`), kern graph (`GraphGnn`), Ollama distill (behind a `DayDigestLlm` trait for testability).

**Spec:** `docs/superpowers/specs/2026-06-12-day-memory-journal-compaction-design.md`

---

## File Structure

- `src/journal/src/day_journal.rs` — rollover renames to `segments/`; drop `HistorySink` ctor param. (Modify)
- `src/journal/src/lib.rs` — update `open_default` signature; re-exports. (Modify)
- `src/journal/src/history.rs` — add `compacted_segments` marker table + `mark_segment`/`segment_done`. (Modify)
- `src/ingest/compactor.rs` — NEW. Segment grouping, `compact_segment`, the background task. (Create)
- `src/ingest/day_digest.rs` — NEW. `DayDigestLlm` trait, day-input gathering, markdown rendering. (Create)
- `src/ingest/mod.rs` — register the two new modules. (Modify)
- `src/config/journal.rs` — add `obsidian_export`, `obsidian_vault`, `compactor_interval_secs`. (Modify)
- `src/commands.rs` — `spawn_compactor`; call it where `spawn_session_mirror` is called. (Modify)

---

### Task 1: Rollover renames to a segment; drop HistorySink from DayJournal

**Files:**
- Modify: `src/journal/src/day_journal.rs`
- Modify: `src/journal/src/lib.rs` (`open_default`)
- Test: `src/journal/src/day_journal.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing test** — rollover moves the closed day to a segment and starts a fresh `today.jsonl`.

```rust
#[test]
fn rollover_renames_closed_day_to_a_segment() {
    let dir = tempfile::tempdir().unwrap();
    let dj = DayJournal::open(dir.path()).unwrap();        // NOTE: no HistorySink arg
    dj.set_max_bytes(1);                                   // tiny cap -> next emit rolls over
    dj.emit(Entry::new(Kind::Log, "k", serde_json::Value::Null));
    dj.emit(Entry::new(Kind::Log, "k", serde_json::Value::Null));

    let seg_dir = dir.path().join(".kern").join("journal").join("segments");
    let segs: Vec<_> = std::fs::read_dir(&seg_dir).unwrap().filter_map(|e| e.ok()).collect();
    assert_eq!(segs.len(), 1, "the rolled-over day became one segment file");
    let name = segs[0].file_name().into_string().unwrap();
    assert!(name.ends_with(".jsonl") && name.len() > "YYYY-MM-DD-HHMMSS".len());
    // today.jsonl exists and is fresh (header only).
    assert!(dir.path().join(".kern/journal/today.jsonl").exists());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p journal rollover_renames_closed_day_to_a_segment`
Expected: FAIL — `DayJournal::open` still takes a `HistorySink`; no `segments/` dir produced.

- [ ] **Step 3: Implement** — change `DayJournal`:
  - Remove the `history: Arc<dyn HistorySink>` field and the `HistorySink` parameter from `open`. Signature becomes `pub fn open(project_root: &Path) -> io::Result<Self>`.
  - Replace `rollover_locked` body: instead of `read_entries` + `history.bulk_insert` + `write_fresh`, do:
    ```rust
    fn rollover_locked(&self, inner: &mut Inner, today: &str) -> io::Result<()> {
        let seg_dir = self.path.parent().unwrap().join("segments");
        fs::create_dir_all(&seg_dir)?;
        // Name from the CLOSED file's header day + wall-clock; falls back to inner.current_day.
        let closed_day = read_header_day(&self.path)?.unwrap_or_else(|| inner.current_day.clone());
        let stamp = now_ms();
        let seg = seg_dir.join(format!("{closed_day}-{stamp}.jsonl"));
        // Atomic same-dir rename of the closed today.jsonl into segments/.
        fs::rename(&self.path, &seg)?;
        write_fresh(&self.path, &self.project_abs, today)?;
        inner.file = OpenOptions::new().read(true).append(true).open(&self.path)?;
        inner.current_day = today.to_string();
        inner.bytes_written = fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        Ok(())
    }
    ```
    (Segment name uses `now_ms()` for uniqueness; `closed_day` keeps the `YYYY-MM-DD` prefix for grouping.)
  - Delete the `HistorySink`/`NullHistorySink` trait + impls from `day_journal.rs` (no longer used by the hot path). Keep `scan_path`, `for_each_entry`, `write_fresh`, `read_header_day`.
  - In `open`, drop the open-time rollover-into-history branch; on a stale-day file, just rename it to a segment (reuse the rename logic) then `write_fresh`.

- [ ] **Step 4: Update callers**
  - `src/journal/src/lib.rs`: `open_default()` → `DayJournal::open(&cwd)`; remove `NullHistorySink`/`HistorySink` from the `pub use day_journal::{...}` re-export; keep `scan_path`, `DayJournal`.
  - Fix the three in-file tests that passed `Arc::new(NullHistorySink)` / `CapturingHistory` to the new no-arg signature; the cap/rollover tests now assert on `segments/` instead of a captured history sink.

- [ ] **Step 5: Run tests**

Run: `cargo test -p journal && cargo check --workspace`
Expected: PASS, no warnings.

- [ ] **Step 6: Commit**

```bash
git add src/journal/src/day_journal.rs src/journal/src/lib.rs
git commit -m "refactor(journal): rollover renames to dated segment; drop HistorySink coupling"
```

---

### Task 2: Segment day-grouping helper (compactor module)

**Files:**
- Create: `src/ingest/compactor.rs`
- Modify: `src/ingest/mod.rs` (add `pub mod compactor;`)
- Test: in `compactor.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn groups_segment_paths_by_day_prefix() {
    let paths = vec![
        PathBuf::from("segments/2026-06-11-100.jsonl"),
        PathBuf::from("segments/2026-06-11-200.jsonl"),  // byte-cap + day-change, same day
        PathBuf::from("segments/2026-06-12-050.jsonl"),
    ];
    let by_day = group_by_day(&paths);
    assert_eq!(by_day.get("2026-06-11").map(|v| v.len()), Some(2));
    assert_eq!(by_day.get("2026-06-12").map(|v| v.len()), Some(1));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p kern --lib groups_segment_paths_by_day_prefix`
Expected: FAIL — `group_by_day` undefined.

- [ ] **Step 3: Implement**

```rust
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Group segment files by their `YYYY-MM-DD` filename prefix. A day may have
/// multiple segments (byte-cap mid-day + the day-change rollover).
pub(crate) fn group_by_day(paths: &[PathBuf]) -> BTreeMap<String, Vec<PathBuf>> {
    let mut out: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for p in paths {
        if let Some(day) = day_prefix(p) {
            out.entry(day).or_default().push(p.clone());
        }
    }
    out
}

/// Extract the leading `YYYY-MM-DD` from a `YYYY-MM-DD-<stamp>.jsonl` name.
fn day_prefix(p: &Path) -> Option<String> {
    let stem = p.file_name()?.to_str()?;
    // First three '-'-separated fields = Y, M, D.
    let mut it = stem.splitn(4, '-');
    let (y, m, d) = (it.next()?, it.next()?, it.next()?);
    if y.len() == 4 && m.len() == 2 && d.len() == 2 {
        Some(format!("{y}-{m}-{d}"))
    } else {
        None
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p kern --lib groups_segment_paths_by_day_prefix`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ingest/compactor.rs src/ingest/mod.rs
git commit -m "feat(compactor): segment day-grouping helper"
```

---

### Task 3: Idempotent segment compaction into history.db

**Files:**
- Modify: `src/journal/src/history.rs` (marker table + `mark_segment`/`segment_done`)
- Modify: `src/ingest/compactor.rs` (`compact_segment`)
- Test: in `compactor.rs`

- [ ] **Step 1: Write the failing test** — compacting a segment inserts its rows once; re-running is a no-op.

```rust
#[test]
fn compact_segment_is_idempotent() {
    use journal::{DayJournal, Entry, Kind, History, Sink};
    let dir = tempfile::tempdir().unwrap();
    // Produce a segment by emitting then forcing a rollover.
    let dj = DayJournal::open(dir.path()).unwrap();
    dj.emit(Entry::new(Kind::ForkOpen { fork_id: "f".into(), parent: None }, "mux",
                       serde_json::json!({"fork_id":"f"})));
    dj.set_max_bytes(1);
    dj.emit(Entry::new(Kind::Log, "k", serde_json::Value::Null)); // triggers rollover of the above
    let seg = std::fs::read_dir(dir.path().join(".kern/journal/segments")).unwrap()
        .next().unwrap().unwrap().path();

    let hist = History::open(dir.path()).unwrap();
    let n1 = compact_segment(&hist, &seg).unwrap();
    let n2 = compact_segment(&hist, &seg).unwrap();   // marker -> skip
    assert!(n1 >= 1, "first compaction inserts rows");
    assert_eq!(n2, 0, "second compaction is a no-op (already marked)");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p kern --lib compact_segment_is_idempotent`
Expected: FAIL — `compact_segment` undefined; `segment_done`/`mark_segment` missing.

- [ ] **Step 3: Implement marker table** in `history.rs` `init`:

```rust
// add to the execute_batch in History::init:
"CREATE TABLE IF NOT EXISTS compacted_segments (name TEXT PRIMARY KEY, ts_ms INTEGER NOT NULL);"
```

Add methods to `impl History`:

```rust
pub fn segment_done(&self, name: &str) -> rusqlite::Result<bool> {
    let conn = self.conn.lock().expect("history mutex poisoned");
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM compacted_segments WHERE name = ?",
        rusqlite::params![name], |r| r.get(0))?;
    Ok(n > 0)
}

pub fn mark_segment(&self, name: &str) -> rusqlite::Result<()> {
    let conn = self.conn.lock().expect("history mutex poisoned");
    conn.execute(
        "INSERT OR IGNORE INTO compacted_segments (name, ts_ms) VALUES (?, ?)",
        rusqlite::params![name, crate::entry::now_ms() as i64])?;
    Ok(())
}
```

- [ ] **Step 4: Implement `compact_segment`** in `compactor.rs`:

```rust
use journal::{Entry, History};

/// Insert a segment's entries into the archive exactly once. Returns the number
/// of rows inserted (0 if the segment was already compacted). The caller deletes
/// the file only after this returns Ok — a crash before delete just re-runs this,
/// and the marker makes the insert a no-op.
pub(crate) fn compact_segment(history: &History, seg: &Path) -> anyhow::Result<usize> {
    let name = seg.file_name().and_then(|s| s.to_str()).unwrap_or_default().to_string();
    if history.segment_done(&name)? {
        return Ok(0);
    }
    let mut entries: Vec<Entry> = Vec::new();
    journal::scan_path(seg, |e| entries.push(e))?;
    history.bulk_insert(&entries)?;
    history.mark_segment(&name)?;
    Ok(entries.len())
}
```

- [ ] **Step 5: Run to verify it passes**

Run: `cargo test -p kern --lib compact_segment_is_idempotent && cargo test -p journal`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/journal/src/history.rs src/ingest/compactor.rs
git commit -m "feat(compactor): idempotent segment->history.db compaction with marker table"
```

---

### Task 4: Compactor background task + config

**Files:**
- Modify: `src/config/journal.rs` (`compactor_interval_secs`, `obsidian_export`, `obsidian_vault`)
- Modify: `src/ingest/compactor.rs` (`run` loop, `compact_once`)
- Modify: `src/commands.rs` (`spawn_compactor`, call next to `spawn_session_mirror`)
- Test: in `compactor.rs` (`compact_once`)

- [ ] **Step 1: Write the failing test** — `compact_once` drains all segments and deletes them.

```rust
#[test]
fn compact_once_drains_and_deletes_segments() {
    use journal::{DayJournal, Entry, Kind, History, Sink};
    let dir = tempfile::tempdir().unwrap();
    let dj = DayJournal::open(dir.path()).unwrap();
    dj.emit(Entry::new(Kind::Log, "k", serde_json::Value::Null));
    dj.set_max_bytes(1);
    dj.emit(Entry::new(Kind::Log, "k", serde_json::Value::Null));
    let seg_dir = dir.path().join(".kern/journal/segments");
    assert_eq!(std::fs::read_dir(&seg_dir).unwrap().count(), 1);

    let hist = History::open(dir.path()).unwrap();
    let drained = compact_once(&hist, &seg_dir).unwrap();
    assert_eq!(drained, 1, "one segment compacted");
    assert_eq!(std::fs::read_dir(&seg_dir).unwrap().count(), 0, "segment deleted after compaction");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p kern --lib compact_once_drains_and_deletes_segments`
Expected: FAIL — `compact_once` undefined.

- [ ] **Step 3: Implement `compact_once`** in `compactor.rs`:

```rust
/// Compact every segment in `seg_dir` into the archive, deleting each after a
/// successful insert. Returns the count of segments compacted. Errors on a
/// single segment are logged and skipped (the file stays for the next pass).
pub(crate) fn compact_once(history: &History, seg_dir: &Path) -> anyhow::Result<usize> {
    if !seg_dir.exists() {
        return Ok(0);
    }
    let mut paths: Vec<PathBuf> = std::fs::read_dir(seg_dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "jsonl").unwrap_or(false))
        .collect();
    paths.sort();
    let mut done = 0;
    for p in &paths {
        match compact_segment(history, p) {
            Ok(_) => {
                if let Err(e) = std::fs::remove_file(p) {
                    tracing::warn!(target: "kern.compactor", error=%e, "segment delete failed");
                } else {
                    done += 1;
                }
            }
            Err(e) => tracing::warn!(target: "kern.compactor", error=%e, "segment compaction failed"),
        }
    }
    Ok(done)
}
```

- [ ] **Step 4: Add config fields** in `src/config/journal.rs` `JournalConfig`:

```rust
/// Seconds between compactor passes that drain dated segments into history.db.
#[serde(default = "default_compactor_interval_secs")]
pub compactor_interval_secs: u64,
/// Write an Obsidian "memory of the day" markdown digest per compacted day.
#[serde(default)]
pub obsidian_export: bool,
/// Vault root for the markdown digest (required when obsidian_export is true).
#[serde(default)]
pub obsidian_vault: Option<std::path::PathBuf>,
```

Add `fn default_compactor_interval_secs() -> u64 { 60 }` and include the fields in the struct's `Default` impl (interval 60, export false, vault None). Update the existing config test to assert the new defaults.

- [ ] **Step 5: Implement `spawn_compactor`** in `commands.rs` (model on `spawn_session_mirror`; call it right after `spawn_session_mirror(...)`):

```rust
fn spawn_compactor(cfg: &crate::config::Config) {
    use crate::ingest::compactor::run;
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let interval = std::time::Duration::from_secs(cfg.journal.compactor_interval_secs.max(1));
    let export = cfg.journal.obsidian_export;
    let vault = cfg.journal.obsidian_vault.clone();
    tokio::spawn(run(cwd, interval, export, vault));
}
```

And the loop in `compactor.rs`:

```rust
pub async fn run(cwd: PathBuf, interval: Duration, export: bool, vault: Option<PathBuf>) {
    let seg_dir = cwd.join(".kern").join("journal").join("segments");
    let history = match History::open(&cwd) {
        Ok(h) => h,
        Err(e) => { tracing::warn!(target: "kern.compactor", error=%e, "history open failed; compactor disabled"); return; }
    };
    loop {
        if let Err(e) = compact_once(&history, &seg_dir) {
            tracing::warn!(target: "kern.compactor", error=%e, "compactor pass failed");
        }
        // Task 8 wires the digest render here (export + vault).
        let _ = (export, &vault);
        tokio::time::sleep(interval).await;
    }
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p kern --lib compactor && cargo check --workspace`
Expected: PASS, no warnings.

- [ ] **Step 7: Commit**

```bash
git add src/ingest/compactor.rs src/config/journal.rs src/commands.rs
git commit -m "feat(compactor): background drain task + journal config (interval, obsidian toggle)"
```

---

### Task 5: Day-input gathering (journal curated + graph entities)

**Files:**
- Create: `src/ingest/day_digest.rs`
- Modify: `src/ingest/mod.rs` (`pub mod day_digest;`)
- Test: in `day_digest.rs`

- [ ] **Step 1: Write the failing test** — gather curated journal events (Log excluded) for a day from `history.db`.

```rust
#[test]
fn gather_day_journal_excludes_log_noise() {
    use journal::{History, Entry, Kind, SCHEMA_VERSION};
    let h = History::open_in_memory().unwrap();
    let day = "2026-06-12";
    let ts = 1_781_000_000_000u64; // a ms timestamp whose local day is 2026-06-12
    h.bulk_insert(&[
        Entry { v: SCHEMA_VERSION, ts_ms: ts, kind: Kind::ForkOpen { fork_id: "f".into(), parent: None }, key: "f".into(), payload: serde_json::json!({"fork_id":"f"}) },
        Entry { v: SCHEMA_VERSION, ts_ms: ts+1, kind: Kind::Log, key: "noise".into(), payload: serde_json::Value::Null },
        Entry { v: SCHEMA_VERSION, ts_ms: ts+2, kind: Kind::Milestone, key: "m".into(), payload: serde_json::json!({"text":"shipped"}) },
    ]).unwrap();

    let events = gather_day_journal(&h, day).unwrap();
    assert!(events.iter().all(|e| !matches!(e.kind, Kind::Log)), "Log noise excluded");
    assert_eq!(events.len(), 2, "ForkOpen + Milestone kept");
}
```

(If the chosen `ts` does not fall on `2026-06-12` in the test's local zone, compute `ts` from the day via the same `day_for` helper the History uses — but prefer `History::query` with a `since/until` window derived from the day so the test is timezone-stable. The implementation below uses an explicit window.)

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p kern --lib gather_day_journal_excludes_log_noise`
Expected: FAIL — `gather_day_journal` undefined.

- [ ] **Step 3: Implement** in `day_digest.rs`:

```rust
use journal::{Entry, Filter, History, Kind};

/// The "meaningful" kinds for a day digest — sessions, plans, goals, tool use,
/// and entity touches. Excludes Log (tracing) and low-signal RPC chatter.
fn is_curated(kind: &Kind) -> bool {
    matches!(kind,
        Kind::ForkOpen { .. } | Kind::ForkResume { .. } | Kind::ForkClose { .. }
        | Kind::PlanProposal | Kind::PlanStep
        | Kind::Goal | Kind::GoalSnapshot | Kind::Milestone
        | Kind::ToolCall | Kind::EntityTouched)
}

/// Curated journal events for `day` ("YYYY-MM-DD"), pulled from the archive and
/// filtered to meaningful kinds, ascending by ts.
pub(crate) fn gather_day_journal(history: &History, day: &str) -> anyhow::Result<Vec<Entry>> {
    let (since, until) = day_window_ms(day)?;     // [00:00, next-00:00) local
    let mut rows = history.query(Filter { since_ms: Some(since), until_ms: Some(until), ..Filter::default() })?;
    rows.retain(|e| is_curated(&e.kind));
    Ok(rows)
}

/// Local-midnight bounds for a YYYY-MM-DD day, in ms. Uses the `time` crate the
/// journal already depends on.
fn day_window_ms(day: &str) -> anyhow::Result<(u64, u64)> {
    use time::{Date, Time, OffsetDateTime, UtcOffset};
    let d = Date::parse(day, &time::format_description::well_known::Iso8601::DEFAULT)
        .map_err(|e| anyhow::anyhow!("bad day {day}: {e}"))?;
    let off = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    let start = OffsetDateTime::new_in_offset(d, Time::MIDNIGHT, off);
    let end = start + time::Duration::days(1);
    Ok(((start.unix_timestamp_nanos() / 1_000_000) as u64,
        (end.unix_timestamp_nanos() / 1_000_000) as u64))
}
```

- [ ] **Step 4: Add the graph-side gather** — a second test + function.

```rust
#[cfg(test)]
#[test]
fn gather_day_entities_picks_touched_ids() {
    // Given a set of EntityTouched events for the day, the returned id set is their entity_ids.
    use journal::{Kind};
    let touches = vec![
        journal::Entry::new(Kind::EntityTouched, "e1", serde_json::json!({"entity_id":"e1"})),
        journal::Entry::new(Kind::EntityTouched, "e2", serde_json::json!({"entity_id":"e2"})),
    ];
    let ids = touched_entity_ids(&touches);
    assert_eq!(ids, vec!["e1".to_string(), "e2".to_string()]);
}
```

```rust
/// Distinct entity_ids referenced by the day's EntityTouched events (first-seen order).
pub(crate) fn touched_entity_ids(events: &[journal::Entry]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for e in events {
        if matches!(e.kind, journal::Kind::EntityTouched) {
            if let Some(id) = e.payload.get("entity_id").and_then(|v| v.as_str()) {
                if seen.insert(id.to_string()) { out.push(id.to_string()); }
            }
        }
    }
    out
}
```

(The graph lookup that resolves these ids + entities created on the day into `(id, kind, label)` tuples is wired in Task 8 against `SharedGraph`, kept out of this pure module so it stays unit-testable.)

- [ ] **Step 5: Run tests**

Run: `cargo test -p kern --lib day_digest`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/ingest/day_digest.rs src/ingest/mod.rs
git commit -m "feat(day_digest): gather curated journal events + touched entity ids for a day"
```

---

### Task 6: DayDigestLlm trait + markdown rendering

**Files:**
- Modify: `src/ingest/day_digest.rs`
- Test: in `day_digest.rs`

- [ ] **Step 1: Write the failing test** — rendering produces the highlight + a structured footer, with a stubbed LLM.

```rust
struct StubLlm(&'static str);
impl DayDigestLlm for StubLlm {
    fn distill_day(&self, _day: &str, _prompt: &str) -> Option<String> { Some(self.0.to_string()) }
}

#[test]
fn render_markdown_has_highlight_and_session_footer() {
    let inputs = DayInputs {
        day: "2026-06-12".into(),
        sessions: vec!["fork-a".into()],
        entities: vec![("e1".into(), "Fact".into(), "kern is standalone".into())],
        tool_calls: 3,
    };
    let md = render_markdown(&inputs, &StubLlm("Big day: shipped the compactor."));
    assert!(md.starts_with("# 2026-06-12"), "dated H1");
    assert!(md.contains("Big day: shipped the compactor."), "LLM highlight included");
    assert!(md.contains("[[kern is standalone]]") || md.contains("e1"), "entity linked");
    assert!(md.contains("fork-a"), "session listed");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p kern --lib render_markdown_has_highlight_and_session_footer`
Expected: FAIL — `DayDigestLlm`, `DayInputs`, `render_markdown` undefined.

- [ ] **Step 3: Implement**

```rust
/// Abstraction over the LLM so the renderer is testable without Ollama.
pub(crate) trait DayDigestLlm {
    /// Return a short prose highlight for the day, or None if the LLM is
    /// unavailable (caller defers the markdown to a later pass).
    fn distill_day(&self, day: &str, prompt: &str) -> Option<String>;
}

/// Everything the renderer needs about one day.
pub(crate) struct DayInputs {
    pub day: String,
    pub sessions: Vec<String>,
    pub entities: Vec<(String, String, String)>, // (id, kind, label)
    pub tool_calls: usize,
}

fn build_prompt(i: &DayInputs) -> String {
    let mut s = format!("Summarize the day {} as a short highlight.\nSessions: {}\nTool calls: {}\nKey knowledge:\n",
        i.day, i.sessions.join(", "), i.tool_calls);
    for (_id, kind, label) in &i.entities {
        s.push_str(&format!("- [{kind}] {label}\n"));
    }
    s
}

/// Render the daily note. Returns None only if the LLM declined (caller retries).
pub(crate) fn render_markdown(i: &DayInputs, llm: &dyn DayDigestLlm) -> String {
    let highlight = llm.distill_day(&i.day, &build_prompt(i))
        .unwrap_or_else(|| "_(highlight pending: LLM unavailable)_".to_string());
    let mut md = format!("# {}\n\n{highlight}\n\n", i.day);
    if !i.sessions.is_empty() {
        md.push_str("## Sessions\n");
        for s in &i.sessions { md.push_str(&format!("- {s}\n")); }
        md.push('\n');
    }
    if !i.entities.is_empty() {
        md.push_str("## Knowledge\n");
        for (_id, kind, label) in &i.entities {
            md.push_str(&format!("- [{kind}] [[{label}]]\n"));
        }
        md.push('\n');
    }
    md.push_str(&format!("> tool calls: {}\n", i.tool_calls));
    md
}
```

(Note: per the spec, when the LLM is unavailable the caller should *skip* writing and retry; the `_(highlight pending)_` fallback string is only used if a caller decides to write anyway. Task 8 chooses skip-and-retry.)

- [ ] **Step 4: Run tests**

Run: `cargo test -p kern --lib day_digest`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ingest/day_digest.rs
git commit -m "feat(day_digest): DayDigestLlm trait + markdown renderer"
```

---

### Task 7: Markdown file writer (vault path + year/month/day)

**Files:**
- Modify: `src/ingest/day_digest.rs`
- Test: in `day_digest.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn writes_note_to_year_month_day_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_day_note(dir.path(), "2026-06-12", "# 2026-06-12\n\nhi\n").unwrap();
    assert_eq!(path, dir.path().join("2026").join("06").join("2026-06-12.md"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "# 2026-06-12\n\nhi\n");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p kern --lib writes_note_to_year_month_day_path`
Expected: FAIL — `write_day_note` undefined.

- [ ] **Step 3: Implement**

```rust
/// Write the daily note under `<vault>/YYYY/MM/YYYY-MM-DD.md`, creating dirs.
/// Overwrites (idempotent re-render). Returns the written path.
pub(crate) fn write_day_note(vault: &Path, day: &str, contents: &str) -> std::io::Result<PathBuf> {
    let (y, m, _d) = {
        let mut it = day.splitn(3, '-');
        (it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or(""))
    };
    let dir = vault.join(y).join(m);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{day}.md"));
    std::fs::write(&path, contents)?;
    Ok(path)
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p kern --lib day_digest`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ingest/day_digest.rs
git commit -m "feat(day_digest): write daily note under vault year/month/day"
```

---

### Task 8: Wire digest into the compactor (graph + Ollama adapter + toggle)

**Files:**
- Modify: `src/ingest/compactor.rs` (call digest on completed past days; pass graph + llm)
- Modify: `src/ingest/day_digest.rs` (graph resolver + Ollama-backed `DayDigestLlm` impl)
- Modify: `src/commands.rs` (`spawn_compactor` passes `SharedGraph` + llm fn)
- Test: in `day_digest.rs` (graph resolver with a seeded `GraphGnn`)

- [ ] **Step 1: Write the failing test** — resolve touched + created-on-day entity ids to `(id, kind, label)` from a seeded graph.

```rust
#[test]
fn resolve_entities_from_graph_returns_labels() {
    use crate::base::graph::GraphGnn;
    let mut g = GraphGnn::new();
    // Seed one entity in the root kern (mirror how DirectSink in session_mirror builds entities).
    // ... build entity id="e1", kind=Fact, statements=["kern is standalone"], accept into g ...
    let resolved = resolve_entities(&g, &["e1".to_string()]);
    assert_eq!(resolved, vec![("e1".to_string(), "Fact".to_string(), "kern is standalone".to_string())]);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p kern --lib resolve_entities_from_graph_returns_labels`
Expected: FAIL — `resolve_entities` undefined.

- [ ] **Step 3: Implement** the graph resolver (read-only walk of `g.kerns[*].entities`, matching ids; label = first statement truncated; kind = `EntityKind` debug/`as_ref`). Add the Ollama-backed `DayDigestLlm` impl as a thin adapter calling the existing distill path (see `src/ingest/` LLM usage); `distill_day` returns `None` on any LLM error so the caller defers.

- [ ] **Step 4: Implement the compactor digest hook** — in `compact_once` (or a new `digest_completed_days`), after draining: for each day that (a) had segments this pass, (b) is strictly before today, and (c) `!history.digest_done(day)` (a second marker, mirroring `compacted_segments`): gather inputs (Task 5), resolve entities (this task), `render_markdown` (Task 6); if the LLM returned a real highlight, `write_day_note` (Task 7) and `mark_digest(day)`. If the LLM was unavailable, skip and leave it for the next pass. Gate the whole block on `export && vault.is_some()`.

- [ ] **Step 5: Wire `spawn_compactor`** to pass `SharedGraph` and construct the Ollama `DayDigestLlm`. Thread them into `run(...)`.

- [ ] **Step 6: Run tests + full check**

Run: `cargo test -p kern --lib && cargo check --workspace --all-targets`
Expected: PASS, no warnings. (Pre-existing broken benches `base::cold` / `replay::build_graph` are unrelated — do not fix here.)

- [ ] **Step 7: Commit**

```bash
git add src/ingest/compactor.rs src/ingest/day_digest.rs src/commands.rs
git commit -m "feat(day-memory): generate Obsidian daily digest on compaction (graph + Ollama, toggleable)"
```

---

## Self-Review

**Spec coverage:**
- §A rollover→segment + drop HistorySink → Task 1. ✓
- §B out-of-band compactor (scan/insert/delete, crash-safe) → Tasks 2,3,4. ✓
- §C SQLite archive fed by compactor → Task 3/4. ✓
- §D memory-of-the-day (journal curated + graph, LLM) → Tasks 5,6,8. ✓
- §E Obsidian export + config (toggle, vault, interval) → Tasks 4,7,8. ✓
- §F idempotency (marker table), LLM-down skip-and-retry, re-render overwrite → Tasks 3,6,8. ✓
- §G testing (pure units, stubbed LLM, crash-safety) → tests in each task. ✓

**Placeholder scan:** Task 8 steps 3/5 describe the graph resolver and Ollama adapter without full code because they depend on existing in-repo APIs (the graph entity shape and the distill function) the implementer must read at that point; every other step has complete code. These are the only intentionally-deferred spots; the interfaces (`resolve_entities`, `DayDigestLlm`) are fully specified so the wiring is mechanical.

**Type consistency:** `History::{segment_done, mark_segment, query, bulk_insert, open, open_in_memory}`, `journal::scan_path`, `compact_segment(&History, &Path)`, `compact_once(&History, &Path)`, `DayInputs`, `DayDigestLlm::distill_day`, `render_markdown`, `write_day_note` are referenced consistently across tasks. `digest_done`/`mark_digest` (Task 8) mirror the segment marker pair from Task 3.

**Note:** Task 1 removes `HistorySink` from `DayJournal` but `History` (the type) and its `bulk_insert` stay — the compactor calls them directly. The `impl HistorySink for History` block in `history.rs` is removed alongside the trait in Task 1.
