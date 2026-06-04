# kern Unified Memory Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the per-cwd `kern` daemon the single memory substrate for both Claude Code and the native `agnt` loop — it auto-learns durable facts from sessions and serves them back into context — and retire the Claude Code file-memory, Vicky, and context-mode stores.

**Architecture:** All memory traffic is **file-mediated through the running daemon** to avoid the CLI-vs-daemon graph race. Capture: a Claude Code `Stop` hook writes a plain-text conversation delta into `.relay/capture/`; a new daemon task (`capture_spool`) distills it into durable claims with an LLM (`distill.rs`) and ingests them through the canonical `Worker`. Recall: a new daemon task keeps `.relay/kern/digest.md` fresh; a `SessionStart` hook `cat`s it into context. The live `query` MCP tool (already shipped) handles mid-session deep recall.

**Tech Stack:** Rust (kern daemon — tokio, serde, clap), Node ESM (`.mjs` Claude Code hooks), TOML config (`.relay/kern.toml`), JSON settings (`~/.claude/settings.json`).

**Spec:** `docs/superpowers/specs/2026-06-04-kern-unified-memory-design.md`

---

## Orientation for the implementer (read once)

You do not need deep knowledge of kern. The facts you need:

- **The daemon owns the graph.** `kern -d` loads the graph into memory, is the only writer, and saves on shutdown. Any CLI subcommand that touches the graph (`ingest`, `query`, `purpose`, `descriptor`) loads a separate on-disk copy and will be overwritten by the daemon — **never** call those from a hook. This is why capture and recall go through files the daemon reads/writes.
- **Ingest entry point:** `crate::ingest::Worker` (`src/ingest/worker.rs`). `worker.enqueue(text, source, kind, descriptor, confidence, config) -> String` queues a fire-and-forget ingest. `Worker` is constructed in the daemon and handed to tasks as `Arc<Worker>` (see how `session_mirror` receives it in `src/commands.rs:462`).
- **Types you will use** (all in `src/base/types.rs` unless noted):
  - `EntityKind` — variants `Fact`, `Claim` (default), `Document`, `Question`, `Answer`, …. Distilled claims use `EntityKind::Claim`.
  - `Source` — use `Source::Session { session_id, section, title }` for captured claims (provenance + feedback-loop filtering).
  - `crate::types::LlmFunc` = `Arc<dyn Fn(&str) -> String + Send + Sync>`.
  - `crate::ingest::Config { dedup_threshold, ttl_secs, hnsw_k, hnsw_ef, rephrase_lower, rephrase_upper }` (`src/ingest/config.rs`) — build with `Config { dedup_threshold: cfg.ingest.dedup_threshold, ..Default::default() }`.
- **Config** (`src/config/mod.rs`): `Config::load(cwd)` merges `<XDG>/relay/kern.toml` then `<cwd>/.relay/kern.toml`. Add new sections by creating a module under `src/config/` and adding a field to `Config` (follow `src/config/watcher.rs` exactly).
- **Daemon wiring** lives in `run_server` in `src/commands.rs` (the `session_mirror` block at lines ~440–472 and the `file_watcher` block at ~474–492 are your templates for spawning a background task).
- **Build/test:** `cargo build` and `cargo test` from the repo root (`C:\Users\sayhe\dev\relay\kern`). Run a single test with `cargo test <name> -- --nocapture`.
- **No compat shims, version stays 1.0.0** (repo rule in `CLAUDE.md`).

---

## File Structure

**Create (kern):**
- `src/config/capture.rs` — `CaptureConfig` (enable + tunables for spool/digest).
- `src/ingest/distill.rs` — `Claim` type + `distill()` LLM extraction + `parse_claims()`.
- `src/ingest/capture_spool.rs` — daemon task: consume delta files → distill → enqueue → archive.
- `src/retrieval/digest.rs` — `build_digest()` pure function (graph → markdown).

**Modify (kern):**
- `src/config/mod.rs` — register `capture` module + field.
- `src/ingest/mod.rs` — `pub mod distill; pub mod capture_spool;`
- `src/retrieval.rs` (or `src/retrieval/mod.rs`) — `pub mod digest;`
- `src/commands.rs` — spawn `capture_spool` + digest writer in `run_server` behind `cfg.capture.enabled`.

**Create (Claude Code glue):**
- `C:\Users\sayhe\.claude\hooks\kern-capture.mjs` — Stop hook: transcript delta → spool file.
- `C:\Users\sayhe\.claude\hooks\kern-recall.mjs` — SessionStart hook: cat digest.
- `C:\Users\sayhe\.claude\hooks\__tests__\kern-capture.test.mjs` — Node test for the extractor.
- `C:\Users\sayhe\dev\relay\kern\.relay\kern.toml` — project config: `[reason]`, `[capture]`.

**Modify (Claude Code glue):**
- `C:\Users\sayhe\.claude\settings.json` — register hooks; disable `vicky`, `context-mode`.
- `C:\Users\sayhe\dev\relay\CLAUDE.md` — directive: memory lives in kern.

---

# Phase A — kern daemon

### Task A1: `CaptureConfig`

**Files:**
- Create: `src/config/capture.rs`
- Modify: `src/config/mod.rs`

- [ ] **Step 1: Write the failing test**

Append to `src/config/capture.rs` (created in step 3, but write the test first in the same file):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_off_with_sane_tunables() {
        let c = CaptureConfig::default();
        assert!(!c.enabled);
        assert_eq!(c.dir, ".relay/capture");
        assert_eq!(c.poll_secs, 5);
        assert_eq!(c.digest_path, ".relay/kern/digest.md");
        assert_eq!(c.digest_secs, 30);
        assert_eq!(c.digest_k, 40);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kern capture::tests::defaults_are_off -- --nocapture`
Expected: FAIL — `CaptureConfig` does not exist (compile error).

- [ ] **Step 3: Write minimal implementation**

Create `src/config/capture.rs` (above the test module):

```rust
use serde::{Deserialize, Serialize};

/// Configuration for Claude-Code memory capture + recall.
///
/// OFF by default. Opt in via a `[capture]` section in `.relay/kern.toml`:
///
/// ```toml
/// [capture]
/// enabled = true
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureConfig {
    /// Master switch for the capture_spool + digest tasks.
    pub enabled: bool,
    /// Spool directory (relative to cwd) the Stop hook writes deltas into.
    pub dir: String,
    /// How often the spool is drained, in seconds.
    pub poll_secs: u64,
    /// Output path (relative to cwd) for the recall digest.
    pub digest_path: String,
    /// How often the digest is regenerated, in seconds.
    pub digest_secs: u64,
    /// Max thoughts included in the digest.
    pub digest_k: usize,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            dir: ".relay/capture".into(),
            poll_secs: 5,
            digest_path: ".relay/kern/digest.md".into(),
            digest_secs: 30,
            digest_k: 40,
        }
    }
}
```

Then wire it into `src/config/mod.rs`:
- Add `mod capture;` next to the other `mod` lines (alphabetical: after `mod ingest;`... place `mod capture;` near the top of the `mod` block).
- Add `pub use capture::CaptureConfig;` next to the other `pub use` lines.
- Add the field to `struct Config` (after `pub watcher: WatcherConfig,`):
  ```rust
      pub capture: CaptureConfig,
  ```
- Add to `Default for Config` (after `watcher: WatcherConfig::default(),`):
  ```rust
            capture: CaptureConfig::default(),
  ```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p kern capture::tests::defaults_are_off -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/config/capture.rs src/config/mod.rs
git commit -m "feat(config): add CaptureConfig for claude-code memory"
```

---

### Task A2: `distill.rs` — claim extraction

**Files:**
- Create: `src/ingest/distill.rs`
- Modify: `src/ingest/mod.rs`

- [ ] **Step 1: Write the failing test**

Create `src/ingest/distill.rs` with the test module at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn stub(json: &'static str) -> impl Fn(&str) -> String {
        move |_q: &str| json.to_string()
    }

    #[test]
    fn extracts_claims_and_maps_kind() {
        let llm = stub(r#"[{"text":"User prefers tabs","kind":"preference"},{"text":"kern owns the graph","kind":"code-fact"}]"#);
        let claims = distill("some conversation", &llm);
        assert_eq!(claims.len(), 2);
        assert_eq!(claims[0].text, "User prefers tabs");
        assert_eq!(claims[0].descriptor, "preference");
        assert_eq!(claims[1].descriptor, "code-fact");
    }

    #[test]
    fn unknown_kind_falls_back_to_fact() {
        let llm = stub(r#"[{"text":"x","kind":"banana"}]"#);
        let claims = distill("c", &llm);
        assert_eq!(claims[0].descriptor, "fact");
    }

    #[test]
    fn bad_json_yields_empty() {
        let llm = stub("I could not find anything useful, sorry!");
        assert!(distill("c", &llm).is_empty());
    }

    #[test]
    fn empty_conversation_skips_llm() {
        let llm = stub(r#"[{"text":"should not appear","kind":"fact"}]"#);
        assert!(distill("   \n  ", &llm).is_empty());
    }

    #[test]
    fn tolerates_prose_around_json() {
        let llm = stub("Here you go:\n[{\"text\":\"a\",\"kind\":\"fact\"}]\nHope that helps");
        let claims = distill("c", &llm);
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].text, "a");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kern distill::tests -- --nocapture`
Expected: FAIL — `distill` / `Claim` undefined.

- [ ] **Step 3: Write minimal implementation**

At the top of `src/ingest/distill.rs` (above the test module):

```rust
//! LLM-gated distillation of a raw conversation into durable claims.
//!
//! Pure-ish: the only side effect is the injected LLM call. The caller
//! (capture_spool) turns each `Claim` into an ingested thought.

/// One durable, reusable piece of knowledge extracted from a conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    /// Self-contained statement worth remembering across sessions.
    pub text: String,
    /// Descriptor key (the typed-memory taxonomy). One of `DESCRIPTORS`.
    pub descriptor: String,
}

/// The typed-memory taxonomy. Mirrors the descriptors seeded into the kern.
pub const DESCRIPTORS: [&str; 6] = [
    "preference", "decision", "project", "fact", "code-fact", "reference",
];

/// Extract durable claims from `conversation`. Returns `[]` when the
/// conversation is empty or the LLM produces no parseable JSON array.
pub fn distill(conversation: &str, llm: &dyn Fn(&str) -> String) -> Vec<Claim> {
    if conversation.trim().is_empty() {
        return Vec::new();
    }
    let prompt = format!(
        "Extract durable, reusable knowledge from this conversation between a \
user and an AI coding assistant. Output ONLY a JSON array. Each element must be \
{{\"text\": \"<one self-contained statement>\", \"kind\": \"<one of: preference, \
decision, project, fact, code-fact, reference>\"}}. Include only knowledge worth \
remembering across future sessions: user preferences, decisions and their \
rationale, ongoing project state, durable facts, structural code facts, and \
external references. Skip greetings, acknowledgements, one-off task mechanics, \
and anything ephemeral. If nothing is worth keeping, output []. Do not wrap the \
array in markdown.\n\nCONVERSATION:\n{conversation}\n"
    );
    parse_claims(&llm(&prompt))
}

/// Pull the first top-level JSON array out of `raw` and parse claims from it.
/// Tolerant of leading/trailing prose around the array.
fn parse_claims(raw: &str) -> Vec<Claim> {
    let (start, end) = match (raw.find('['), raw.rfind(']')) {
        (Some(s), Some(e)) if e > s => (s, e),
        _ => return Vec::new(),
    };
    let items: Vec<serde_json::Value> = match serde_json::from_str(&raw[start..=end]) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for it in items {
        let text = it
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if text.is_empty() {
            continue;
        }
        let kind_raw = it
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("fact")
            .trim();
        let descriptor = if DESCRIPTORS.contains(&kind_raw) {
            kind_raw.to_string()
        } else {
            "fact".to_string()
        };
        out.push(Claim { text, descriptor });
    }
    out
}
```

Then add to `src/ingest/mod.rs` (next to the other `pub mod` lines):

```rust
pub mod distill;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p kern distill::tests -- --nocapture`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add src/ingest/distill.rs src/ingest/mod.rs
git commit -m "feat(ingest): add LLM-gated conversation distillation"
```

---

### Task A3: `capture_spool.rs` — drain → distill → ingest → archive

**Files:**
- Create: `src/ingest/capture_spool.rs`
- Modify: `src/ingest/mod.rs`

This task splits a testable core (`consume_file`) from the daemon loop (`run`). The core takes an injected `sink` so tests need neither a `Worker` nor an embed service (mirrors the `DirectSink` pattern used by `session_mirror`/`file_watcher` tests).

- [ ] **Step 1: Write the failing test**

Create `src/ingest/capture_spool.rs` with this test module at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::tempdir;

    fn stub_two(_q: &str) -> String {
        r#"[{"text":"fact one","kind":"fact"},{"text":"a preference","kind":"preference"}]"#
            .to_string()
    }

    #[test]
    fn consumes_distills_and_archives() {
        let dir = tempdir().unwrap();
        let spool = dir.path().to_path_buf();
        let done = spool.join("done");
        let delta = spool.join("sess-1.txt");
        std::fs::write(&delta, "user: hi\nassistant: here is a fact").unwrap();

        let captured: Mutex<Vec<Claim>> = Mutex::new(Vec::new());
        let n = consume_file(&delta, &done, &stub_two, &|c| {
            captured.lock().unwrap().push(c);
        });

        assert_eq!(n, 2);
        assert_eq!(captured.lock().unwrap().len(), 2);
        assert!(!delta.exists(), "delta should be moved out of the spool");
        assert!(done.join("sess-1.txt").exists(), "delta should be archived");
    }

    #[test]
    fn empty_distillation_still_archives() {
        let dir = tempdir().unwrap();
        let spool = dir.path().to_path_buf();
        let done = spool.join("done");
        let delta = spool.join("sess-2.txt");
        std::fs::write(&delta, "user: thanks").unwrap();

        let n = consume_file(&delta, &done, &|_q| "[]".to_string(), &|_c| {});
        assert_eq!(n, 0);
        assert!(done.join("sess-2.txt").exists());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kern capture_spool::tests -- --nocapture`
Expected: FAIL — `consume_file` / `Claim` undefined.

- [ ] **Step 3: Write minimal implementation**

At the top of `src/ingest/capture_spool.rs`:

```rust
//! Slice — Claude-Code capture spool.
//!
//! The CC `Stop` hook drops plain-text conversation deltas into the spool
//! directory. This task drains them: each delta is distilled into durable
//! `Claim`s (LLM), each claim is enqueued through the canonical `Worker`,
//! and the consumed file is archived to `<spool>/done/`. Archiving makes the
//! drain idempotent — a delta is processed exactly once.
//!
//! The daemon is the single graph owner, so ingest happens in-process with
//! no CLI race.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::base::types::{EntityKind, Source};
use crate::ingest::distill::{distill, Claim};
use crate::ingest::Worker;
use crate::types::LlmFunc;

/// Drain `spool_dir` once: process every `*.txt` delta and archive it.
/// `sink` receives every extracted claim (the daemon wires this to
/// `Worker::enqueue`; tests pass a collector).
pub fn drain_once(
    spool_dir: &Path,
    llm: &dyn Fn(&str) -> String,
    sink: &dyn Fn(Claim),
) {
    let done = spool_dir.join("done");
    let entries = match std::fs::read_dir(spool_dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for ent in entries.flatten() {
        let path = ent.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("txt") {
            continue;
        }
        consume_file(&path, &done, llm, sink);
    }
}

/// Process one delta file: distill, emit claims to `sink`, archive. Returns
/// the number of claims emitted.
pub fn consume_file(
    path: &Path,
    done_dir: &Path,
    llm: &dyn Fn(&str) -> String,
    sink: &dyn Fn(Claim),
) -> usize {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return 0,
    };
    let claims = distill(&text, llm);
    let n = claims.len();
    for c in claims {
        sink(c);
    }
    archive(path, done_dir);
    n
}

fn archive(path: &Path, done_dir: &Path) {
    let _ = std::fs::create_dir_all(done_dir);
    if let Some(name) = path.file_name() {
        if std::fs::rename(path, done_dir.join(name)).is_err() {
            // Best effort: if rename across devices fails, drop the file so
            // it is not re-processed.
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Daemon loop. Polls `spool_dir` every `interval`, enqueueing every claim
/// through `worker`. Runs forever (until the task is aborted on shutdown).
pub async fn run(
    spool_dir: PathBuf,
    worker: Arc<Worker>,
    llm: LlmFunc,
    dedup_threshold: f64,
    interval: Duration,
) {
    let _ = std::fs::create_dir_all(&spool_dir);
    loop {
        tokio::time::sleep(interval).await;
        let worker = worker.clone();
        let llm_ref = llm.as_ref();
        let sink = |c: Claim| {
            let src = Source::Session {
                session_id: "claude-code".to_string(),
                section: String::new(),
                title: format!("claude://{}", c.descriptor),
            };
            worker.enqueue(
                c.text,
                src,
                EntityKind::Claim,
                c.descriptor,
                0.6,
                crate::ingest::Config {
                    dedup_threshold,
                    ..Default::default()
                },
            );
        };
        drain_once(&spool_dir, llm_ref, &sink);
    }
}
```

Then add to `src/ingest/mod.rs`:

```rust
pub mod capture_spool;
```

> Note: if `tempfile` is not already a dev-dependency, it is — `file_watcher.rs` tests use it (`use tempfile::tempdir;`). No `Cargo.toml` change needed.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p kern capture_spool::tests -- --nocapture`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add src/ingest/capture_spool.rs src/ingest/mod.rs
git commit -m "feat(ingest): add capture_spool drain task"
```

---

### Task A4: `digest.rs` — graph → markdown digest

**Files:**
- Create: `src/retrieval/digest.rs`
- Modify: `src/retrieval.rs` (or `src/retrieval/mod.rs` — whichever declares the submodules)

First confirm the module declaration site: run `Grep` for `pub mod` in `src/retrieval.rs`. If `src/retrieval.rs` contains `mod` declarations, add there; if there is a `src/retrieval/mod.rs`, add there.

- [ ] **Step 1: Write the failing test**

Create `src/retrieval/digest.rs` with this test at the bottom. The test builds a tiny graph with the same helpers `file_watcher`/`session_mirror` tests use.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::graph::GraphGnn;
    use crate::base::types::{
        Acl, ChunkPart, ChunkPartKind, Entity, EntityKind, EntityStatus, Source,
    };
    use crate::crdt::GCounter;

    fn mk_entity(id: &str, text: &str, heat: f64) -> Entity {
        let mut e = Entity {
            id: id.to_string(),
            root_id: String::new(),
            external_id: String::new(),
            superseded_by: String::new(),
            kind: EntityKind::Claim,
            status: EntityStatus::Active,
            statements: vec![text.to_string()],
            chunks: vec![ChunkPart {
                kind: ChunkPartKind::StatementRef,
                text: String::new(),
                index: 0,
            }],
            vector: vec![0.0; 8],
            gnn_vector: Vec::new(),
            score: 0.0,
            conf_alpha: 2.0,
            conf_beta: 1.0,
            source: Source::Inline { hash: id.into(), section: String::new() },
            created_at: None,
            acl: Acl::default(),
            access_count: GCounter::new(),
            accessed_at: None,
            heat,
            heat_updated_at: None,
            updated_at: None,
            valid_until: None,
            producer_id: String::new(),
            unlinked_count: 0,
        };
        e.refresh_score();
        e
    }

    #[test]
    fn digest_has_purpose_and_hottest_first_capped() {
        let mut g = GraphGnn::default();
        g.root.purpose_text = "remember durable facts".to_string();
        let root_id = g.root.id.clone();
        let kern = g.map_mut().get_mut(&root_id).expect("root kern");
        kern.entities.insert("a".into(), mk_entity("a", "cold fact", 0.1));
        kern.entities.insert("b".into(), mk_entity("b", "hot fact", 9.0));

        let md = build_digest(&g, 1);
        assert!(md.contains("remember durable facts"), "purpose present");
        assert!(md.contains("hot fact"), "hottest included");
        assert!(!md.contains("cold fact"), "capped at k=1");
    }

    #[test]
    fn empty_graph_yields_header_only() {
        let g = GraphGnn::default();
        let md = build_digest(&g, 10);
        assert!(md.contains("# kern memory"));
    }
}
```

> If `GraphGnn::default()` or `map_mut()` are not the exact accessors, inspect `src/base/graph.rs` and adjust the test's graph construction to whatever the `file_watcher.rs` / `session_mirror.rs` tests use to insert entities into the root kern. The assertions on `build_digest` output stay the same.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kern digest::tests -- --nocapture`
Expected: FAIL — `build_digest` undefined.

- [ ] **Step 3: Write minimal implementation**

At the top of `src/retrieval/digest.rs`:

```rust
//! Recall digest: a markdown snapshot of the kern's purpose plus its
//! hottest thoughts, written to disk for the Claude-Code SessionStart hook
//! to inject. Pure function + a thin file writer; no live query path.

use crate::base::graph::GraphGnn;
use crate::base::types::{Entity, EntityStatus};

/// Render the digest markdown: purpose header + up to `k` hottest active
/// thoughts, hottest first.
pub fn build_digest(graph: &GraphGnn, k: usize) -> String {
    let mut out = String::from("# kern memory\n\n");
    let purpose = graph.root.purpose_text.trim();
    if !purpose.is_empty() {
        out.push_str("Purpose: ");
        out.push_str(purpose);
        out.push_str("\n\n");
    }

    let mut ents: Vec<&Entity> = graph
        .map()
        .values()
        .flat_map(|kern| kern.entities.values())
        .filter(|e| matches!(e.status, EntityStatus::Active))
        .collect();
    ents.sort_by(|a, b| {
        b.heat
            .partial_cmp(&a.heat)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    out.push_str("## What I know\n\n");
    for e in ents.into_iter().take(k) {
        if let Some(s) = e.statements.first() {
            out.push_str("- ");
            out.push_str(s.trim());
            out.push('\n');
        }
    }
    out
}

/// Render and write the digest to `path`, creating parent dirs. Best effort.
pub fn write_digest(graph: &GraphGnn, path: &std::path::Path, k: usize) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, build_digest(graph, k));
}
```

Add the module declaration (in `src/retrieval.rs` or `src/retrieval/mod.rs`):

```rust
pub mod digest;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p kern digest::tests -- --nocapture`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add src/retrieval/digest.rs src/retrieval.rs
git commit -m "feat(retrieval): add recall digest builder"
```

---

### Task A5: wire capture_spool + digest writer into the daemon

**Files:**
- Modify: `src/commands.rs` (in `run_server`, after the `file_watcher` block at ~492, before the `shutdown_tx` block at ~494)

This is an integration step. No new unit test — it is covered by the build plus the Phase C E2E. Verify by building and by a manual smoke run.

- [ ] **Step 1: Add the spawn block**

Insert after the `if cfg.watcher.enabled { … }` block (around line 492 in `src/commands.rs`):

```rust
    // Claude-Code memory: capture spool drain + recall digest writer.
    // Both file-mediated; off unless `[capture] enabled = true` in
    // `.relay/kern.toml`.
    if cfg.capture.enabled {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

        // Capture drain: spool deltas -> distill -> enqueue -> archive.
        if let Some(llm_fn) = llm_fn.clone() {
            let spool = cwd.join(&cfg.capture.dir);
            let worker_c = worker.clone();
            let dedup = cfg.ingest.dedup_threshold;
            let poll = std::time::Duration::from_secs(cfg.capture.poll_secs);
            tokio::spawn(crate::ingest::capture_spool::run(
                spool, worker_c, llm_fn, dedup, poll,
            ));
        } else {
            tracing::warn!(
                target: "kern.capture",
                "capture enabled but no reason LLM configured; distillation disabled"
            );
        }

        // Digest writer: periodically snapshot purpose + hot thoughts.
        {
            let digest_path = cwd.join(&cfg.capture.digest_path);
            let g_digest = g.clone();
            let k = cfg.capture.digest_k;
            let every = std::time::Duration::from_secs(cfg.capture.digest_secs);
            tokio::spawn(async move {
                loop {
                    {
                        let g = crate::base::locks::read_recovered(&g_digest);
                        crate::retrieval::digest::write_digest(&g, &digest_path, k);
                    }
                    tokio::time::sleep(every).await;
                }
            });
        }
    }
```

> `llm_fn` is the `Option<crate::ingest::LlmFunc>` already built earlier in `run_server` (see line ~399). `g` is `entry.graph.clone()` (line 431). `worker` is `entry.worker.clone()` (line 432). `read_recovered` is already imported (`use crate::base::locks::read_recovered;` at top of `commands.rs`). If `llm_fn` is not in scope at this point, build it the same way the surrounding code does and reuse it.

- [ ] **Step 2: Build**

Run: `cargo build -p kern`
Expected: compiles clean (warnings OK).

- [ ] **Step 3: Smoke test the daemon path manually**

```bash
# Terminal 1: create config enabling capture (see Task B3 for the file),
# then start the daemon from the repo root:
cargo run -p kern -- -d
```
Expected: log line shows the daemon listening; no panic. Stop with ctrl-c.

- [ ] **Step 4: Commit**

```bash
git add src/commands.rs
git commit -m "feat(daemon): spawn capture_spool + digest writer when enabled"
```

---

# Phase B — Claude Code glue

### Task B1: capture hook (Stop) — transcript delta → spool file

**Files:**
- Create: `C:\Users\sayhe\.claude\hooks\kern-capture.mjs`
- Create: `C:\Users\sayhe\.claude\hooks\__tests__\kern-capture.test.mjs`

The hook reads the Stop event JSON from stdin (`{ transcript_path, session_id, cwd, ... }`), extracts the conversation delta since the last run, and writes it to `<cwd>/.relay/capture/<session>-<n>.txt`. All graph work happens later in the daemon. **Fail-open: any error → exit 0 with no output.**

- [ ] **Step 1: Write the failing test**

Create `C:\Users\sayhe\.claude\hooks\__tests__\kern-capture.test.mjs`:

```js
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { extractDelta } from '../kern-capture.mjs';

const lines = [
  JSON.stringify({ type: 'user', message: { role: 'user', content: 'how do I build it?' } }),
  JSON.stringify({ type: 'assistant', message: { role: 'assistant', content: [
    { type: 'thinking', thinking: 'secret reasoning' },
    { type: 'text', text: 'Run cargo build.' },
    { type: 'tool_use', name: 'Bash', input: {} },
  ] } }),
  JSON.stringify({ type: 'system', subtype: 'x' }),
  JSON.stringify({ type: 'user', message: { role: 'user', content: [
    { type: 'tool_result', content: 'compiled' },
  ] } }),
];

test('extracts user strings and assistant text only', () => {
  const { text, consumed } = extractDelta(lines, 0);
  assert.equal(consumed, 4);
  assert.match(text, /user: how do I build it\?/);
  assert.match(text, /assistant: Run cargo build\./);
  assert.doesNotMatch(text, /secret reasoning/);
  assert.doesNotMatch(text, /tool_result|compiled|Bash/);
});

test('offset skips already-consumed lines', () => {
  const { text } = extractDelta(lines, 4);
  assert.equal(text.trim(), '');
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `node --test "C:\Users\sayhe\.claude\hooks\__tests__\kern-capture.test.mjs"`
Expected: FAIL — cannot import `extractDelta`.

- [ ] **Step 3: Write minimal implementation**

Create `C:\Users\sayhe\.claude\hooks\kern-capture.mjs`:

```js
#!/usr/bin/env node
// Claude Code Stop hook: extract the new conversation delta from the
// transcript and write it to the kern capture spool. Fail-open.
import fs from 'node:fs';
import path from 'node:path';

/** Extract user prompts + assistant text from transcript lines past `offset`.
 *  Returns { text, consumed } where consumed is the new line count. */
export function extractDelta(lines, offset) {
  const out = [];
  let i = offset;
  for (; i < lines.length; i++) {
    const raw = lines[i];
    if (!raw || !raw.trim()) continue;
    let o;
    try { o = JSON.parse(raw); } catch { continue; }
    if (o.type === 'user') {
      const c = o.message?.content;
      if (typeof c === 'string' && c.trim()) out.push(`user: ${c.trim()}`);
      // user content that is an array = tool_result; skip.
    } else if (o.type === 'assistant') {
      const c = o.message?.content;
      if (Array.isArray(c)) {
        for (const b of c) {
          if (b?.type === 'text' && b.text?.trim()) {
            out.push(`assistant: ${b.text.trim()}`);
          }
        }
      }
    }
  }
  return { text: out.join('\n\n'), consumed: lines.length };
}

function offsetsFile(spool) { return path.join(spool, '.offsets.json'); }

function readOffsets(spool) {
  try { return JSON.parse(fs.readFileSync(offsetsFile(spool), 'utf8')); }
  catch { return {}; }
}

function writeOffsets(spool, offsets) {
  try { fs.writeFileSync(offsetsFile(spool), JSON.stringify(offsets)); } catch {}
}

async function main() {
  const input = await new Promise((res) => {
    let buf = '';
    process.stdin.on('data', (d) => (buf += d));
    process.stdin.on('end', () => res(buf));
  });
  let ev;
  try { ev = JSON.parse(input); } catch { return; }
  const { transcript_path, cwd, session_id } = ev;
  if (!transcript_path || !cwd || !fs.existsSync(transcript_path)) return;

  const spool = path.join(cwd, '.relay', 'capture');
  fs.mkdirSync(spool, { recursive: true });

  const lines = fs.readFileSync(transcript_path, 'utf8').split('\n');
  const offsets = readOffsets(spool);
  const start = offsets[session_id] || 0;
  if (start >= lines.length) return;

  const { text, consumed } = extractDelta(lines, start);
  if (text.trim()) {
    const file = path.join(spool, `${session_id}-${consumed}.txt`);
    fs.writeFileSync(file, text);
  }
  offsets[session_id] = consumed;
  writeOffsets(spool, offsets);
}

// Only run main when invoked directly (not when imported by tests).
if (process.argv[1] && process.argv[1].endsWith('kern-capture.mjs')) {
  main().catch(() => {}).finally(() => process.exit(0));
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `node --test "C:\Users\sayhe\.claude\hooks\__tests__\kern-capture.test.mjs"`
Expected: PASS (2 tests).

- [ ] **Step 5: Manual smoke against the live transcript**

```powershell
$ev = @{ transcript_path = "C:\Users\sayhe\.claude\projects\C--Users-sayhe-dev-relay-kern\6ca1a034-626a-4622-b60f-ea5fd14137f2.jsonl"; cwd = "C:\Users\sayhe\dev\relay\kern"; session_id = "smoke" } | ConvertTo-Json
$ev | & "C:/Program Files/nodejs/node.exe" "C:\Users\sayhe\.claude\hooks\kern-capture.mjs"
Get-ChildItem "C:\Users\sayhe\dev\relay\kern\.relay\capture"
```
Expected: a `smoke-<n>.txt` file containing user/assistant lines; `.offsets.json` present. Delete the smoke file afterward.

- [ ] **Step 6: Commit** (these files live outside the kern repo; commit only repo files. The hooks live in `~/.claude` — note them in the plan, no repo commit needed. Skip commit for this task.)

---

### Task B2: recall hook (SessionStart) — cat the digest

**Files:**
- Create: `C:\Users\sayhe\.claude\hooks\kern-recall.mjs`

- [ ] **Step 1: Write implementation** (trivial; verified by manual run)

Create `C:\Users\sayhe\.claude\hooks\kern-recall.mjs`:

```js
#!/usr/bin/env node
// Claude Code SessionStart hook: inject the kern recall digest. Fail-open.
import fs from 'node:fs';
import path from 'node:path';

async function main() {
  const input = await new Promise((res) => {
    let buf = '';
    process.stdin.on('data', (d) => (buf += d));
    process.stdin.on('end', () => res(buf));
  });
  let ev = {};
  try { ev = JSON.parse(input); } catch {}
  const cwd = ev.cwd || process.cwd();
  const digestPath = path.join(cwd, '.relay', 'kern', 'digest.md');

  let digest = '';
  try { digest = fs.readFileSync(digestPath, 'utf8'); } catch { return; }
  if (!digest.trim()) return;

  const out = {
    hookSpecificOutput: {
      hookEventName: 'SessionStart',
      additionalContext: digest,
    },
  };
  process.stdout.write(JSON.stringify(out));
}

main().catch(() => {}).finally(() => process.exit(0));
```

- [ ] **Step 2: Manual smoke**

```powershell
echo '{"cwd":"C:\\Users\\sayhe\\dev\\relay\\kern"}' | & "C:/Program Files/nodejs/node.exe" "C:\Users\sayhe\.claude\hooks\kern-recall.mjs"
```
Expected: with no `digest.md`, prints nothing (exit 0). After the daemon has written a digest, prints the JSON wrapper containing the digest.

---

### Task B3: project config — enable capture + point reason LLM

**Files:**
- Create: `C:\Users\sayhe\dev\relay\kern\.relay\kern.toml`

The daemon needs a `reason` LLM for distillation and `capture.enabled = true`. The `reason.url` should be a cheap/local model endpoint (distillation runs once per captured delta).

- [ ] **Step 1: Write the config**

Create `.relay/kern.toml`:

```toml
# Project-scope kern config. Merges over the user-scope kern.toml.

[reason]
# Cheap model for distillation + smart-split. Point at your local Ollama
# (or any OpenAI-compatible endpoint). Falls back to [embed].url if unset.
url = "http://localhost:11434"
model = "llama3"

[capture]
enabled = true
# dir, poll_secs, digest_path, digest_secs, digest_k use CaptureConfig defaults.
```

> Confirm the embed endpoint already works (`embed.url` defaults to `http://localhost:11434`, `nomic-embed-text`). If your reason model differs, set `model` accordingly. Add `key` under `[reason]` only if the endpoint needs auth.

- [ ] **Step 2: Verify the daemon loads it**

```bash
cargo run -p kern -- -d
```
Expected: no config error; capture spawn block runs (add a temporary `tracing::info!` if you want to confirm, then remove). Ctrl-c to stop.

- [ ] **Step 3: Commit**

```bash
git add .relay/kern.toml
git commit -m "chore(config): enable claude-code capture in kern.toml"
```

> Check `.gitignore` first — if `.relay/` is ignored, do NOT force-add; instead document the file contents in the README and skip the commit. (`.gitignore` currently is 19 bytes — inspect it.)

---

### Task B4: register hooks in settings.json

**Files:**
- Modify: `C:\Users\sayhe\.claude\settings.json`

- [ ] **Step 1: Add the Stop hook and the SessionStart recall hook**

Edit the `hooks` object. The existing `SessionStart` array already has the context-mode entry; append the recall hook as a second entry, and add a new `Stop` array:

```json
  "hooks": {
    "SessionStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "\"C:/Program Files/nodejs/node.exe\" \"C:/Users/sayhe/.claude/hooks/context-mode-cache-heal.mjs\""
          }
        ]
      },
      {
        "hooks": [
          {
            "type": "command",
            "command": "\"C:/Program Files/nodejs/node.exe\" \"C:/Users/sayhe/.claude/hooks/kern-recall.mjs\""
          }
        ]
      }
    ],
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "\"C:/Program Files/nodejs/node.exe\" \"C:/Users/sayhe/.claude/hooks/kern-capture.mjs\""
          }
        ]
      }
    ]
  },
```

> Note: the context-mode SessionStart hook is preserved here. It is removed in Phase C only after kern recall is confirmed working — do NOT delete it in this task.

- [ ] **Step 2: Verify JSON is valid**

```powershell
Get-Content "C:\Users\sayhe\.claude\settings.json" -Raw | ConvertFrom-Json | Out-Null; "ok"
```
Expected: prints `ok` (no parse error).

- [ ] **Step 3: Verify in a fresh session**

Open a new Claude Code session in the kern repo. Confirm: (a) no hook errors at startup, (b) after the daemon has run, the recall digest appears in context. (No commit — settings.json is outside the repo.)

---

# Phase C — cutover + verification

### Task C1: seed the kern (purpose + descriptors)

**Files:** none (one-time, through the running daemon's MCP surface).

Because CLI subcommands race the daemon, seed via the kern MCP tools against the live daemon. The implementer runs these through the MCP client (Claude Code's `mcp__kern__*` tools, or any MCP client pointed at the kern server).

- [ ] **Step 1: Set the purpose**

`mcp__kern__purpose` with:
```
text: "Personal and project memory for relay/kern work. Stores durable facts, decisions, preferences, project state, code facts, and references; auto-learned from sessions and recalled into context."
```

- [ ] **Step 2: Add the six descriptors**

Call `mcp__kern__descriptor` (action `add`) once per descriptor:
- `preference` — "How the user wants work done: style, tooling, workflow choices."
- `decision` — "A choice made and the reasoning behind it."
- `project` — "Ongoing work, goals, and constraints not derivable from code."
- `fact` — "A durable factual claim worth remembering across sessions."
- `code-fact` — "A structural truth about a codebase: where things live, how modules relate."
- `reference` — "A pointer to an external resource: URL, dashboard, ticket."

- [ ] **Step 3: Verify**

`mcp__kern__health` → expect `descriptors: 6` and a non-empty `purpose`.

---

### Task C2: E2E — capture → distill → recall

**Files:** none (manual end-to-end with the running daemon).

- [ ] **Step 1: Ensure daemon running with capture enabled**

`cargo run -p kern -- -d` (with `.relay/kern.toml` from B3). Confirm `mcp__kern__health` responds.

- [ ] **Step 2: Drop a delta stating a durable fact**

```powershell
$spool = "C:\Users\sayhe\dev\relay\kern\.relay\capture"
New-Item -ItemType Directory -Force $spool | Out-Null
Set-Content "$spool\e2e-1.txt" "user: Remember that I always want tabs, never spaces.`n`nassistant: Noted — tabs, never spaces, recorded as a preference."
```

- [ ] **Step 3: Wait for the drain + verify ingest**

Wait `poll_secs` (5s) + a moment. Then:
```powershell
Get-ChildItem "$spool\done"   # e2e-1.txt should be archived here
```
And `mcp__kern__health` → `entities` count increased. And `mcp__kern__query` with `text: "tabs or spaces preference"` → returns the captured claim.

- [ ] **Step 4: Verify the digest**

Wait `digest_secs` (30s) or restart not needed. Then:
```powershell
Get-Content "C:\Users\sayhe\dev\relay\kern\.relay\kern\digest.md"
```
Expected: contains the purpose and the tabs/spaces preference.

- [ ] **Step 5: Verify idempotency**

Re-drop the same content as `e2e-2.txt`. After drain, `mcp__kern__health` entity count should NOT increase (dedup at `dedup_threshold = 0.95`). If it does, lower `dedup_threshold` in `.relay/kern.toml` and document the chosen value.

---

### Task C3: hard cutover — disable old memory systems

**Files:**
- Modify: `C:\Users\sayhe\.claude\settings.json`
- Modify: `C:\Users\sayhe\dev\relay\CLAUDE.md`
- Modify/retire: `C:\Users\sayhe\.claude\projects\C--Users-sayhe-dev-relay-kern\memory\`

Do this ONLY after C2 passes (recall digest demonstrably works).

- [ ] **Step 1: Disable Vicky + context-mode plugins**

In `settings.json` `enabledPlugins`, set:
```json
    "vicky@stack": false,
    "context-mode@stack": false,
    "context-mode@context-mode": false,
```

- [ ] **Step 2: Remove the context-mode SessionStart hook**

Delete the context-mode-cache-heal entry from the `SessionStart` array (leave the kern-recall entry). Validate JSON as in B4 step 2.

- [ ] **Step 3: Retire file-memory**

```powershell
$mem = "C:\Users\sayhe\.claude\projects\C--Users-sayhe-dev-relay-kern\memory"
if (Test-Path $mem) { Rename-Item $mem "memory.retired-2026-06-04" }
```
> Rename, don't delete — reversible if recall quality regresses (per CLAUDE.md "look before you overwrite").

- [ ] **Step 4: Add the CLAUDE.md directive**

Append to `C:\Users\sayhe\dev\relay\CLAUDE.md`:
```markdown
- Memory lives in kern (the per-cwd daemon). Do not use file-memory, Vicky, or context-mode for durable memory. Capture is automatic via the Stop hook; recall is the SessionStart digest plus the `mcp__kern__query` tool.
```

- [ ] **Step 5: Commit the repo-side change**

```bash
git add ../CLAUDE.md
git commit -m "docs: route durable memory through kern; retire other stores"
```
> `settings.json` and the `memory/` dir are outside the repo — not committed.

- [ ] **Step 6: Final verification in a fresh session**

Open a new Claude Code session in the kern repo. Confirm: (a) startup shows the kern recall digest and no context-mode/vicky activity, (b) ending the session writes a spool delta, (c) the daemon ingests it (entity count rises). Cutover complete.

---

## Self-Review (completed by author)

- **Spec coverage:** Seed (C1), distilled capture (A2 distill + A3 spool + B1 hook), recall digest (A4 + A5 + B2 hook), agnt-native paths (unchanged — already wired via session_mirror/pre_turn, no task needed), hard cutover (C3), fresh start (no import task — intentional), error handling/fail-open (B1/B2/A3 best-effort), testing (A1–A4 unit, B1 node test, C2 E2E). All spec sections map to a task.
- **Placeholder scan:** No TBD/TODO; every code step has complete code. The two "confirm the accessor" notes (A4 graph construction, retrieval module decl site) are explicit verification steps with a concrete fallback, not placeholders.
- **Type consistency:** `Claim { text, descriptor }` defined in A2, consumed identically in A3. `build_digest(graph, k)` defined in A4, called in A5. `extractDelta(lines, offset) -> { text, consumed }` defined and tested in B1. `CaptureConfig` fields defined in A1 match their use in A5 (`dir`, `poll_secs`, `digest_path`, `digest_secs`, `digest_k`).
- **Known risk to watch during impl:** `EntityKind::Claim` confidence/verification semantics and whether the Document-vs-Claim distinction affects retrieval ranking — verify the captured claims actually surface in `query`/`digest` (covered by C2 step 3–4).
```
