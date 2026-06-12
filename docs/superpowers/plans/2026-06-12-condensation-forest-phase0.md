# Condensation Forest — Phase 0 (reader-check + compaction) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop and recover the LMDB freelist bloat that crashes the daemon with
`MDB_MAP_FULL` — without any forest/condensation work yet.

**Architecture:** Two halves. (1) **Live, in-daemon:** call
`env.clear_stale_readers()` on store open and on the maintenance tick so LMDB can
reuse freed pages — this stops the file from *growing*. Expose a bloat ratio via
`kern health`. (2) **Daemon-down:** a `kern compact` subcommand that
`copy_to_file(CompactionOption::Enabled)` into a fresh dir and atomically swaps it
in (with a retained `*.bloated.bak`) — this *shrinks* an already-bloated file.
A live in-place swap is impossible on Windows while the daemon holds the mmap
(verified: `Access denied` renaming `.kern\data`), so `kern compact` refuses when
the dir is locked.

**Tech Stack:** Rust, heed 0.20.5 (`clear_stale_readers`, `real_disk_size`,
`non_free_pages_size`, `copy_to_file`, `CompactionOption`), existing `Store`
wrapper in `src/base/store.rs`.

**Reference:** design spec `docs/superpowers/specs/2026-06-12-kern-condensation-subdb-design.md`
(§Component 5, §Crash safety, §Phasing Phase 0).

---

## File Structure

- **Modify** `src/base/store.rs` — add three `Store` methods: `clear_stale_readers`,
  `bloat_stats`, `compact_into`. Call `clear_stale_readers` at the end of `open`.
- **Modify** `src/commands.rs` — add a `Compact` subcommand variant + dispatch arm;
  call `store.clear_stale_readers()` inside `spawn_maintenance_tick`.
- **Modify** `src/commands/admin.rs` — add `cmd_compact`; extend `cmd_health` to
  print the bloat ratio.
- **Modify** `src/base/constants.rs` (or wherever `[graph]` defaults live) — add
  `COMPACT_FLOOR_BYTES` and `COMPACT_BLOAT_RATIO` constants (used by `health` for
  the advisory and later by an automated trigger).

No new files: Phase 0 is additive to the existing `Store`/CLI surface (avoids a
premature `forest.rs` before the forest exists).

---

## Task 1: `Store::clear_stale_readers`

**Files:**
- Modify: `src/base/store.rs` (add method to `impl Store`, ~after `open` at :230)
- Test: `src/base/store.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn clear_stale_readers_returns_count_and_does_not_error() {
    let d = tmp();
    let s = Store::open(&dir_of(&d)).unwrap();
    // With no crashed processes there are no stale readers; the call must
    // succeed and return a count (0 here). The contract under test is "callable,
    // non-fatal", not a specific count.
    let n = s.clear_stale_readers().unwrap();
    assert_eq!(n, 0, "fresh env has no stale reader slots to reap");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kern store::tests::clear_stale_readers_returns_count -- --nocapture`
Expected: FAIL — `no method named clear_stale_readers found for struct Store`.

- [ ] **Step 3: Write minimal implementation**

Add to `impl Store` in `src/base/store.rs`:

```rust
/// Reap reader slots left behind by crashed/aborted processes. LMDB cannot
/// reuse freed pages while any reader (even a dead one) pins an old snapshot;
/// the daemon + CLI + hook + MCP all open read txns, so a crashed reader is the
/// mechanism that lets the freelist — and the file — grow without bound. Called
/// on open and on the maintenance tick. Returns the number of slots cleared.
pub fn clear_stale_readers(&self) -> Result<usize, StoreError> {
    Ok(self.env.clear_stale_readers()?)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p kern store::tests::clear_stale_readers_returns_count -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/base/store.rs
git commit -m "feat(store): clear_stale_readers to free pinned LMDB pages"
```

---

## Task 2: Call `clear_stale_readers` on open

**Files:**
- Modify: `src/base/store.rs` (`open`, the block at :213-223)
- Test: `src/base/store.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn open_reaps_stale_readers_and_still_serves_data() {
    let d = tmp();
    let dir = dir_of(&d);
    {
        let s = Store::open(&dir).unwrap();
        s.put(s.kern, "k", &Sample { name: "v".into(), nums: vec![1.0] }).unwrap();
    }
    // Reopen: open() must call clear_stale_readers internally and not break I/O.
    // We assert behavior indirectly — the reopened store reads the prior value.
    let s2 = Store::open(&dir).unwrap();
    assert_eq!(s2.get::<Sample>(s2.kern, "k").unwrap().unwrap().name, "v");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kern store::tests::open_reaps_stale_readers -- --nocapture`
Expected: FAIL at first compile only if the call is missing won't fail the
assertion — so this test guards against regression once the call is added. Run it
now; it PASSES already (open works). Treat Step 3 as the change and keep this test
as the regression guard. (If you prefer a strict red: temporarily assert the call
via a counter; not worth the test seam here.)

- [ ] **Step 3: Add the call**

In `Store::open`, after `wtxn.commit()?;` and before `Ok(Self { … })`, insert:

```rust
        // Reap reader slots from any process that died holding a read txn, so the
        // freelist is reusable from the first write of this session.
        let store = Self { env, kern, cold, meta };
        let _ = store.clear_stale_readers(); // best-effort; never block open
        Ok(store)
```

Replace the existing `Ok(Self { env, kern, cold, meta })` tail with the above.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kern store:: -- --nocapture`
Expected: PASS (all store tests green).

- [ ] **Step 5: Commit**

```bash
git add src/base/store.rs
git commit -m "feat(store): reap stale readers on env open"
```

---

## Task 3: `Store::bloat_stats`

**Files:**
- Modify: `src/base/store.rs`
- Test: `src/base/store.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn bloat_stats_reports_disk_and_live_sizes() {
    let d = tmp();
    let s = Store::open(&dir_of(&d)).unwrap();
    for i in 0..50 {
        s.put(s.kern, &format!("k{i}"), &Sample { name: format!("n{i}"), nums: vec![i as f64; 8] }).unwrap();
    }
    let st = s.bloat_stats().unwrap();
    assert!(st.real_disk_bytes >= st.live_bytes, "disk >= live always");
    assert!(st.live_bytes > 0, "non-empty store has live pages");
    assert!(st.ratio() >= 1.0, "ratio is disk/live, >= 1");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kern store::tests::bloat_stats_reports -- --nocapture`
Expected: FAIL — `no method named bloat_stats` / `BloatStats` undefined.

- [ ] **Step 3: Implement**

Add to `src/base/store.rs` (near the top-level types):

```rust
/// On-disk vs live-page accounting for one env. `ratio` is the bloat factor:
/// 1.0 = perfectly compact, large = mostly dead freelist pages.
#[derive(Debug, Clone, Copy)]
pub struct BloatStats {
    pub real_disk_bytes: u64,
    pub live_bytes: u64,
}

impl BloatStats {
    pub fn ratio(&self) -> f64 {
        if self.live_bytes == 0 { 1.0 } else { self.real_disk_bytes as f64 / self.live_bytes as f64 }
    }
}
```

And to `impl Store`:

```rust
/// Real file size vs the bytes actually occupied by live B-tree pages. A high
/// ratio means the env is mostly dead freelist pages and wants compaction.
pub fn bloat_stats(&self) -> Result<BloatStats, StoreError> {
    Ok(BloatStats {
        real_disk_bytes: self.env.real_disk_size()?,
        live_bytes: self.env.non_free_pages_size()?,
    })
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p kern store::tests::bloat_stats_reports -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/base/store.rs
git commit -m "feat(store): bloat_stats (real_disk vs live pages)"
```

---

## Task 4: `Store::compact_into` (offline compaction primitive)

**Files:**
- Modify: `src/base/store.rs`
- Test: `src/base/store.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn compact_into_produces_smaller_env_that_round_trips() {
    let d = tmp();
    let dir = dir_of(&d);
    let s = Store::open(&dir).unwrap();
    // Create bloat: write then overwrite the same keys many times so old pages
    // pile into the freelist (the env file grows; live set stays small).
    for round in 0..40 {
        for i in 0..50 {
            s.put(s.kern, &format!("k{i}"), &Sample { name: format!("r{round}n{i}"), nums: vec![round as f64; 16] }).unwrap();
        }
    }
    let before = s.bloat_stats().unwrap();

    let out = tmp();
    let out_dir = dir_of(&out);
    std::fs::remove_dir_all(&out_dir).ok(); // compact_into creates it fresh
    s.compact_into(&out_dir).unwrap();

    // Reopen the compacted copy: same live data, smaller-or-equal real size.
    let s2 = Store::open(&out_dir).unwrap();
    assert_eq!(s2.get::<Sample>(s2.kern, "k7").unwrap().unwrap().name, "r39n7", "latest value survives");
    let after = s2.bloat_stats().unwrap();
    assert!(after.real_disk_bytes <= before.real_disk_bytes, "compacted env is no larger");
    assert!(after.ratio() <= before.ratio() + f64::EPSILON, "compaction does not increase bloat");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kern store::tests::compact_into_produces_smaller -- --nocapture`
Expected: FAIL — `no method named compact_into`.

- [ ] **Step 3: Implement**

heed's `copy_to_file` writes a single LMDB data file (not a dir), so we create the
destination dir and copy into `<dir>/data.mdb`, matching `Store::open`'s layout.

Add `use heed::CompactionOption;` to the imports, then to `impl Store`:

```rust
/// Write a compacted copy of this env into a fresh directory `out_dir`
/// (created if absent, must be empty). Uses LMDB's `MDB_CP_COMPACT`, which
/// copies only live pages — the output has no freelist bloat. The source is
/// untouched; this is the offline-shrink primitive behind `kern compact`.
pub fn compact_into(&self, out_dir: &str) -> Result<(), StoreError> {
    std::fs::create_dir_all(out_dir)?;
    let data = Path::new(out_dir).join("data.mdb");
    if data.exists() {
        return Err(StoreError::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "compact output already has a data.mdb",
        )));
    }
    // copy_to_file creates and returns the file handle; drop it to flush/close.
    let _f = self.env.copy_to_file(&data, CompactionOption::Enabled)?;
    Ok(())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p kern store::tests::compact_into_produces_smaller -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/base/store.rs
git commit -m "feat(store): compact_into — MDB_CP_COMPACT to a fresh dir"
```

---

## Task 5: `kern compact` subcommand (daemon-down swap)

**Files:**
- Modify: `src/commands.rs` (enum near :175 next to `Compress`; dispatch near :413)
- Modify: `src/commands/admin.rs` (add `cmd_compact`)
- Test: `src/commands/admin.rs` (`#[cfg(test)] mod cmd_tests`)

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn cmd_compact_swaps_in_a_smaller_store_and_keeps_a_backup() {
    let (_dir, cfg) = temp_cfg();
    let data = cfg.data_dir.clone();
    {
        // Seed + bloat the configured data dir.
        let s = crate::base::store::Store::open(&data).unwrap();
        for round in 0..30 {
            for i in 0..40 {
                s.put_kern_test(&format!("k{i}"), round, i); // helper below
            }
        }
    }
    let before = crate::base::store::Store::open(&data).unwrap().bloat_stats().unwrap().real_disk_bytes;
    cmd_compact(&cfg, None);
    let after = crate::base::store::Store::open(&data).unwrap().bloat_stats().unwrap().real_disk_bytes;
    assert!(after <= before, "data dir shrank or held");
    assert!(std::path::Path::new(&format!("{data}.bloated.bak")).exists(), "backup retained");
    // Data intact:
    let g = load_graph(&cfg); // load_graph opens the (now compacted) store
    let _ = g; // presence-only: load must not error on the swapped store
}
```

> If `put_kern_test` doesn't exist, inline the bloat loop with real `Store::put`
> over `s.kern` as in Task 4's test instead of a helper — do not add test-only
> public methods to `Store`. (Author's note: prefer the inline form.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kern admin::cmd_tests::cmd_compact_swaps -- --nocapture`
Expected: FAIL — `cannot find function cmd_compact`.

- [ ] **Step 3: Implement `cmd_compact`**

Add to `src/commands/admin.rs`:

```rust
/// Offline compaction: shrink a bloated LMDB env by copying only live pages into
/// a fresh dir and atomically swapping it in, keeping the old dir as a
/// `*.bloated.bak`. MUST run with the cwd daemon stopped — the daemon holds the
/// env mmap and (on Windows) the dir rename will be denied. On a locked dir the
/// swap fails loudly and the temp copy is cleaned up; nothing is lost.
pub(super) fn cmd_compact(cfg: &crate::config::Config, dir: Option<&str>) {
    use std::path::Path;
    let data = dir.map(str::to_string).unwrap_or_else(|| cfg.data_dir.clone());
    let tmp = format!("{data}.compact.tmp");
    let bak = format!("{data}.bloated.bak");

    let _ = std::fs::remove_dir_all(&tmp); // clear any stale temp

    // 1. Compact-copy into the temp dir.
    let store = match crate::base::store::Store::open(&data) {
        Ok(s) => s,
        Err(e) => { eprintln!("compact: open {data}: {e}"); return; }
    };
    let before = store.bloat_stats().ok();
    if let Err(e) = store.compact_into(&tmp) {
        eprintln!("compact: copy: {e}");
        let _ = std::fs::remove_dir_all(&tmp);
        return;
    }
    drop(store); // release our own handle before the swap

    // 2. Swap: data -> bak, tmp -> data. A held mmap (live daemon) denies the
    //    first rename on Windows — surface that clearly.
    let _ = std::fs::remove_dir_all(&bak);
    if let Err(e) = std::fs::rename(&data, &bak) {
        eprintln!("compact: cannot move {data} (is the daemon running? stop it first): {e}");
        let _ = std::fs::remove_dir_all(&tmp);
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, &data) {
        eprintln!("compact: swap-in failed, restoring: {e}");
        let _ = std::fs::rename(&bak, &data); // roll back
        return;
    }

    // 3. Verify the swapped store opens and loads; roll back on failure.
    match crate::base::store::Store::open(&data).and_then(|s| s.load_all_kerns().map(|_| s.bloat_stats())) {
        Ok(Ok(after)) => {
            let b = before.map(|b| b.real_disk_bytes).unwrap_or(0);
            println!("compacted {data}: {} MB -> {} MB (backup at {bak})",
                b / 1_048_576, after.real_disk_bytes / 1_048_576);
        }
        _ => {
            eprintln!("compact: swapped store failed to verify; restoring backup");
            let _ = std::fs::remove_dir_all(&data);
            let _ = std::fs::rename(&bak, &data);
        }
    }
    let _ = Path::new(&tmp); // tmp consumed by rename; nothing to clean on success
}
```

- [ ] **Step 4: Wire the subcommand**

In `src/commands.rs`, add to the `Commands` enum next to `Compress` (~:175):

```rust
    /// Offline-shrink a bloated LMDB store (run with the daemon stopped).
    /// Keeps the old dir as <dir>.bloated.bak.
    Compact {
        /// Data dir to compact; defaults to the configured data_dir.
        path: Option<String>,
    },
```

And to the dispatch `match` next to `Compress` (~:413):

```rust
        Commands::Compact { path } => admin::cmd_compact(cfg, path.as_deref()),
```

- [ ] **Step 5: Run test + build**

Run: `cargo test -p kern admin::cmd_tests::cmd_compact_swaps -- --nocapture`
Expected: PASS.
Run: `cargo build`
Expected: green (new subcommand compiles, dispatch exhaustive).

- [ ] **Step 6: Commit**

```bash
git add src/commands.rs src/commands/admin.rs
git commit -m "feat(cli): kern compact — daemon-down LMDB shrink with backup"
```

---

## Task 6: Maintenance-tick reader reap

**Files:**
- Modify: `src/commands.rs` (`spawn_maintenance_tick`, ~:970-990)

- [ ] **Step 1: Locate the tick body**

`spawn_maintenance_tick` builds `g_tick = g.clone()` and loops on
`tokio::time::interval(cfg.tick.interval_secs)`. Inside the loop body, after the
pulse/GC work, the graph is readable via `read_recovered(&g_tick)`.

- [ ] **Step 2: Add the reap (no new test — exercised via Task 1/2 unit coverage; this is wiring)**

Inside the tick loop, after the existing maintenance block, add:

```rust
            // Reap reader slots from any crashed CLI/hook/MCP process so LMDB can
            // reuse freed pages — keeps the env from ratcheting toward MAP_FULL.
            if let Some(store) = crate::base::locks::read_recovered(&g_tick).store() {
                let _ = store.clear_stale_readers();
            }
```

(Confirm `GraphGnn::store()` returns `Option<Arc<Store>>`; it does — used by
`persist::save_all` at `persist.rs:330`.)

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: green.

- [ ] **Step 4: Commit**

```bash
git add src/commands.rs
git commit -m "feat(daemon): reap stale LMDB readers each maintenance tick"
```

---

## Task 7: `kern health` shows the bloat ratio

**Files:**
- Modify: `src/commands/admin.rs` (`cmd_health`, :34-64)
- Test: existing `cmd_health_runs_on_a_fresh_graph_without_panicking` covers no-panic;
  add an assertion-light unit if a pure formatter is extracted.

- [ ] **Step 1: Extract a pure formatter + failing test**

Add to `src/commands/admin.rs`:

```rust
/// Human line for store bloat. Pulled out so it is unit-testable without a daemon.
fn bloat_line(st: &crate::base::store::BloatStats) -> String {
    format!("store:       {} MB on disk, {} MB live (bloat x{:.1})",
        st.real_disk_bytes / 1_048_576, st.live_bytes / 1_048_576, st.ratio())
}

#[cfg(test)]
#[test]
fn bloat_line_reports_ratio() {
    let st = crate::base::store::BloatStats { real_disk_bytes: 400 * 1_048_576, live_bytes: 100 * 1_048_576 };
    let s = bloat_line(&st);
    assert!(s.contains("400 MB on disk"), "disk shown");
    assert!(s.contains("100 MB live"), "live shown");
    assert!(s.contains("x4.0"), "ratio shown");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kern admin::bloat_line_reports_ratio -- --nocapture`
Expected: FAIL — `cannot find function bloat_line`.

- [ ] **Step 3: Implement + wire into cmd_health**

Implement `bloat_line` (Step 1). In `cmd_health`, after the `data_dir` print, add:

```rust
    if let Some(store) = g.store() {
        if let Ok(st) = store.bloat_stats() {
            println!("{}", bloat_line(&st));
        }
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p kern admin:: -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/commands/admin.rs
git commit -m "feat(cli): kern health reports store bloat ratio"
```

---

## Task 8: Full gate + manual recovery verification

**Files:** none (verification only)

- [ ] **Step 1: Full suite**

Run: `cargo test` (workspace) and `cargo clippy --all-targets -- -D warnings`
Expected: green, no new lints.

- [ ] **Step 2: Manual recovery dry-run (the live bug)**

With the cwd daemon **stopped**:

```
kern health                         # note bloat xN on the 4 GiB dir
kern compact                        # swaps in the compacted store, keeps .bloated.bak
kern health                         # bloat ~x1, MB on disk collapses (~50 MB)
```

Expected: second `health` shows live≈disk; `.kern/data.bloated.bak` present.
Then start the daemon; confirm recall works and no `MDB_MAP_FULL` on save.

- [ ] **Step 3: Negative — daemon-up refusal**

With the daemon **running**, `kern compact` must print the "stop the daemon first"
error and leave `.kern/data` untouched (no partial swap, temp cleaned).

- [ ] **Step 4: Commit any fixes, then hand off to `/personas` for plan review.**

---

## Self-review notes

- **Spec coverage:** §Component 5 (reader-check live + daemon-down compaction) →
  Tasks 1,2,4,5,6. Bloat observability via `kern health` → Tasks 3,7. The "no live
  dir swap" constraint → Task 5 refusal path + Task 8 Step 3. Forest/condensation
  (Phases 1-3) intentionally out of scope.
- **Placeholders:** none — every step has real code/commands. The one helper note
  in Task 5 Step 1 explicitly instructs the inline form (no test-only public API).
- **Type consistency:** `BloatStats { real_disk_bytes, live_bytes }` + `ratio()`
  used identically in Tasks 3, 5, 7. `compact_into(out_dir)`, `clear_stale_readers`,
  `bloat_stats` names match across tasks. `Store.kern` field + `put/get` already
  exist (store.rs).
- **Windows note:** `kern compact` is the only shrink path; the daemon never swaps
  a live dir. This is the load-bearing correction from the persona panel.
