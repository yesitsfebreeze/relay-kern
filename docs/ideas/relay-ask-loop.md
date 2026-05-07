# Relay Ask Loop Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the v1 ask-loop: per-file executor agents raise `//? Q#<ulid>` markers + journal `ask` entries; a top-5 bubble surfaces them; kern-on-tap loads file context; resolved asks are answered in journal and reflected back into code by direct edit. Drainer integration deferred.

**Architecture:** Extend the existing `relay-journal` `Kind` enum with four new variants (`Ask`, `Answer`, `Goal`, `Milestone`). A new `relay-ask` library hosts: payload schemas, ULID generation, marker grep, priority formula, and journal query helpers. A new plugin `relay-ask-bubble` renders the top-5 asks via the existing `ui_slots` push API on a 30s timer + journal-write push. kern-on-tap is a slash command in `relay/commands` that loads bundled asks for a file into the session prompt. Goal/milestone are journal entries with status criteria evaluated on demand.

**Tech Stack:** Rust 2021, existing workspace crates (`relay-journal`, `relay-render`, `relay-harness`, `relay-recipe`, `relay-commands`), `serde`/`serde_json`, `ulid` crate (new dep), workspace `tokio`, existing UI slot plugin pattern (see `src/plugins/clock/`).

**Spec:** `docs/superpowers/specs/2026-04-25-relay-ask-loop-design.md`.

---

## File Structure

**New crate:** `src/relay/ask/`
- `Cargo.toml` — workspace member.
- `src/lib.rs` — re-exports.
- `src/payload.rs` — `AskPayload`, `AnswerPayload`, `GoalPayload`, `MilestonePayload`, `AskTag`, `MilestoneStatus` types + serde.
- `src/marker.rs` — `Marker` struct (`Q#<ulid>`/`A#<ulid>`), parser, file scanner.
- `src/priority.rs` — `score_for(ask, now, weights)` pure function.
- `src/query.rs` — `open_asks_for_file(history, day_journal, file)`, `open_asks_all(...)`, `goals_open(...)` reading journal.
- `src/build.rs` — typed entry builders (`new_ask(...)`, `new_answer(...)`, `new_goal(...)`, `new_milestone(...)`).
- `tests/marker_scan.rs` — integration: write a fake file, scan, assert markers.
- `tests/priority_ranking.rs` — integration: ranking stability under formula.

**Modify:** `src/relay/journal/src/entry.rs` — add `Ask`, `Answer`, `Goal`, `Milestone` to `Kind` (`SCHEMA_VERSION` bump to 3).

**New plugin:** `src/plugins/ask-bubble/`
- `Cargo.toml` — workspace member, depends on `relay-ask`, existing UI slot push API.
- `src/lib.rs` — `Plugin` impl: timer task + `pre_turn`/`post_turn` push + `on_file_change` push.

**Modify:** `src/relay/commands/` — register `/ask <ulid>` and `/asks-for <file>` commands.

**Modify:** `src/bin/relay/kern/` — wire registration of `ask-bubble` plugin (gated by config).

**Workspace:** add new members to `Cargo.toml`.

---

### Task 1: Extend `Kind` with ask-loop variants

**Files:**
- Modify: `src/relay/journal/src/entry.rs`
- Test: `src/relay/journal/src/entry.rs` (inline `#[cfg(test)]`)

- [ ] **Step 1: Write failing test for new variants**

Append to `src/relay/journal/src/entry.rs` after the existing module body, inside or alongside any existing tests:

```rust
#[cfg(test)]
mod ask_kind_tests {
	use super::*;
	use serde_json::json;

	#[test]
	fn ask_kind_round_trips_through_json() {
		let e = Entry::new(Kind::Ask, "src/foo.rs", json!({"id":"01HW","text":"x"}));
		let s = serde_json::to_string(&e).unwrap();
		let d: Entry = serde_json::from_str(&s).unwrap();
		assert_eq!(d.kind, Kind::Ask);
	}

	#[test]
	fn answer_goal_milestone_kinds_round_trip() {
		for k in [Kind::Answer, Kind::Goal, Kind::Milestone] {
			let e = Entry::new(k.clone(), "", json!({}));
			let s = serde_json::to_string(&e).unwrap();
			let d: Entry = serde_json::from_str(&s).unwrap();
			assert_eq!(d.kind, k);
		}
	}

	#[test]
	fn schema_version_is_three() {
		assert_eq!(SCHEMA_VERSION, 3);
	}
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test -p relay-journal entry::ask_kind_tests --lib`
Expected: FAIL — `Kind::Ask` does not exist; `SCHEMA_VERSION` is `2`.

- [ ] **Step 3: Add the variants and bump schema**

Edit `src/relay/journal/src/entry.rs`. Bump `SCHEMA_VERSION` to `3`:

```rust
pub const SCHEMA_VERSION: u32 = 3;
```

Add four variants to `Kind` (preserve order; append at the end):

```rust
	/// Question raised by an executor agent, awaiting user answer.
	Ask,
	/// Answer to an `Ask`, referenced by `payload.ref_id`.
	Answer,
	/// User-stated goal, scoping milestones and asks.
	Goal,
	/// Recorded milestone outcome.
	Milestone,
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cargo test -p relay-journal --lib`
Expected: PASS, including the three new tests.

- [ ] **Step 5: Verify the wider workspace still builds**

Run: `cargo check --workspace`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src/relay/journal/src/entry.rs
git commit -m "feat(journal): add Ask/Answer/Goal/Milestone kinds, bump schema to 3"
```

---

### Task 2: Create the `relay-ask` crate skeleton

**Files:**
- Create: `src/relay/ask/Cargo.toml`
- Create: `src/relay/ask/src/lib.rs`
- Modify: `Cargo.toml` (workspace members)

- [ ] **Step 1: Add the workspace member**

Edit the root `Cargo.toml`. Insert after `"src/relay/journal"`:

```toml
	"src/relay/ask",
```

- [ ] **Step 2: Write the crate manifest**

Create `src/relay/ask/Cargo.toml`:

```toml
[package]
name = "relay-ask"
version = "0.1.0"
edition = "2021"
license = "MIT"

[lib]
path = "src/lib.rs"

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
relay-journal = { path = "../journal" }
ulid = "1"
time = { version = "0.3", features = ["formatting", "macros", "local-offset"] }

[dev-dependencies]
tempfile = "3"

[lints]
workspace = true
```

- [ ] **Step 3: Write the lib.rs stub**

Create `src/relay/ask/src/lib.rs`:

```rust
//! Ask-loop primitives: typed payloads, code markers, priority,
//! and journal queries that surface open asks.
//!
//! See `docs/superpowers/specs/2026-04-25-relay-ask-loop-design.md`.

#![deny(missing_docs)]

pub mod build;
pub mod marker;
pub mod payload;
pub mod priority;
pub mod query;

pub use build::{new_answer, new_ask, new_goal, new_milestone};
pub use marker::{scan_file, Marker, MarkerKind};
pub use payload::{
	AnswerPayload, AskPayload, AskTag, GoalPayload, GoalScope, MilestoneCriteria,
	MilestonePayload, MilestoneStatus, Status,
};
pub use priority::{score_for, Weights};
pub use query::{goals_open, open_asks_all, open_asks_for_file, OpenAsk};
```

This file references modules that don't exist yet — added in subsequent tasks.

- [ ] **Step 4: Verify the workspace member is recognised**

Run: `cargo check -p relay-ask`
Expected: FAIL — missing modules (`build`, `marker`, `payload`, `priority`, `query`). The error confirms `cargo` sees the new crate; it will compile once the modules land in later tasks.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml src/relay/ask/Cargo.toml src/relay/ask/src/lib.rs
git commit -m "feat(ask): scaffold relay-ask crate"
```

---

### Task 3: Implement payload types

**Files:**
- Create: `src/relay/ask/src/payload.rs`
- Test: `src/relay/ask/src/payload.rs` (inline `#[cfg(test)]`)

- [ ] **Step 1: Write the failing test (with the type signatures we want)**

Create `src/relay/ask/src/payload.rs`:

```rust
//! Typed payloads for ask-loop journal entries.

use serde::{Deserialize, Serialize};

/// Tag for ask categorisation. Drives bubble styling and weights.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AskTag {
	/// Architectural / design decision.
	Design,
	/// Behavioural choice within an existing contract.
	Behavior,
	/// Safety-relevant (panic, unsafe, bounds, secret).
	Safety,
	/// Trivial nit; lowest priority.
	Nit,
}

/// Lifecycle status of an ask or goal.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Status {
	/// Awaiting answer.
	Open,
	/// User answered; resolution applied.
	Answered,
	/// Aged out of the rolling window or cancelled.
	Stale,
}

/// Body of a `Kind::Ask` entry.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AskPayload {
	/// ULID; doubles as the `Q#<id>` code marker.
	pub id: String,
	/// Question prose.
	pub text: String,
	/// Authoring executor.
	pub agent_id: String,
	/// Lifecycle status.
	pub status: Status,
	/// Tags (zero or more; `vec![]` = untagged).
	pub tags: Vec<AskTag>,
}

/// Body of a `Kind::Answer` entry.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AnswerPayload {
	/// ULID of this answer.
	pub id: String,
	/// `id` of the `Ask` this answers.
	pub ref_id: String,
	/// Answer prose.
	pub text: String,
}

/// Goal scope.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GoalScope {
	/// Specific files.
	Files(Vec<String>),
	/// Directory subtree.
	Dir(String),
	/// Whole repo.
	All,
}

/// Body of a `Kind::Goal` entry.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GoalPayload {
	/// ULID.
	pub id: String,
	/// Free-form goal text.
	pub text: String,
	/// Scope of work.
	pub scope: GoalScope,
}

/// Criteria evaluated to decide whether a milestone is reached.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct MilestoneCriteria {
	/// Minimum mean `@score` across scope.
	pub min_score: Option<u32>,
	/// Require zero open asks within scope.
	pub asks_resolved: bool,
	/// Require full test suite green (caller-supplied verdict).
	pub tests_pass: bool,
}

/// Lifecycle of a milestone.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MilestoneStatus {
	/// Pending; criteria not yet met.
	Pending,
	/// All criteria met; emitted as `milestone_reached`.
	Reached,
}

/// Body of a `Kind::Milestone` entry.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MilestonePayload {
	/// ULID.
	pub id: String,
	/// `id` of the `Goal` this milestone belongs to.
	pub ref_id: String,
	/// Display label.
	pub text: String,
	/// Criteria.
	pub criteria: MilestoneCriteria,
	/// Status at the time the entry was written.
	pub status: MilestoneStatus,
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	#[test]
	fn ask_payload_round_trips() {
		let p = AskPayload {
			id: "01HW".into(),
			text: "should we cache?".into(),
			agent_id: "exec-foo".into(),
			status: Status::Open,
			tags: vec![AskTag::Design, AskTag::Safety],
		};
		let v = serde_json::to_value(&p).unwrap();
		assert_eq!(v["status"], json!("open"));
		assert_eq!(v["tags"][0], json!("design"));
		let back: AskPayload = serde_json::from_value(v).unwrap();
		assert_eq!(back.id, p.id);
		assert_eq!(back.tags, p.tags);
	}

	#[test]
	fn goal_scope_serialises_with_tag() {
		let s = GoalScope::Dir("src/relay".into());
		let v = serde_json::to_value(&s).unwrap();
		assert_eq!(v["kind"], json!("dir"));
		assert_eq!(v[0], serde_json::Value::Null);
	}

	#[test]
	fn milestone_criteria_default_means_no_gates() {
		let c = MilestoneCriteria::default();
		assert!(c.min_score.is_none());
		assert!(!c.asks_resolved);
		assert!(!c.tests_pass);
	}
}
```

- [ ] **Step 2: Run tests to verify they pass once compiled**

Run: `cargo test -p relay-ask payload --lib`
Expected: FAIL TO COMPILE for now — `relay-ask` has unresolved modules in `lib.rs` (`build`, `marker`, `priority`, `query`). Tests for this module will be exercised after Task 7. Continue — the type definitions are the deliverable here.

- [ ] **Step 3: Commit**

```bash
git add src/relay/ask/src/payload.rs
git commit -m "feat(ask): payload types for ask/answer/goal/milestone"
```

---

### Task 4: Implement `Marker` parser and file scanner

**Files:**
- Create: `src/relay/ask/src/marker.rs`
- Test: `src/relay/ask/src/marker.rs` (inline `#[cfg(test)]`)

- [ ] **Step 1: Write the failing test**

Create `src/relay/ask/src/marker.rs`:

```rust
//! Code-marker convention `//? Q#<ulid>` and `//? A#<ulid>`.
//!
//! Markers are the addressable anchor: `grep -n` finds them, the ULID
//! looks the entry up in the journal.

use std::fs;
use std::path::Path;

/// Whether the marker is a question or an answer trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerKind {
	/// `Q#<ulid>` — open question.
	Question,
	/// `A#<ulid>` — answer trace, retained for audit until resolution.
	Answer,
}

/// A parsed marker occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
	/// 1-indexed line number in the source file.
	pub line: usize,
	/// `Q` or `A`.
	pub kind: MarkerKind,
	/// ULID lookup key into the journal.
	pub id: String,
	/// Optional short hint following the ULID (trimmed). May be empty.
	pub hint: String,
}

/// Scan one file for markers. Returns markers in source order.
///
/// Errors only on I/O. Malformed lines are skipped silently.
pub fn scan_file(path: &Path) -> std::io::Result<Vec<Marker>> {
	let body = fs::read_to_string(path)?;
	Ok(scan_str(&body))
}

/// Scan a string for markers (test-friendly).
pub fn scan_str(body: &str) -> Vec<Marker> {
	let mut out = Vec::new();
	for (idx, raw) in body.lines().enumerate() {
		let line_no = idx + 1;
		let trimmed = raw.trim_start();
		let Some(rest) = trimmed.strip_prefix("//?") else { continue };
		let rest = rest.trim_start();
		let (kind, after_kind) = if let Some(r) = rest.strip_prefix("Q#") {
			(MarkerKind::Question, r)
		} else if let Some(r) = rest.strip_prefix("A#") {
			(MarkerKind::Answer, r)
		} else {
			continue;
		};
		let mut split = after_kind.splitn(2, char::is_whitespace);
		let id = split.next().unwrap_or("").trim().to_string();
		if !is_ulid_shape(&id) {
			continue;
		}
		let hint = split.next().unwrap_or("").trim().to_string();
		out.push(Marker {
			line: line_no,
			kind,
			id,
			hint,
		});
	}
	out
}

/// Cheap structural check: ULID is 26 base32 (Crockford) chars.
/// Avoids pulling the ulid crate into every scan.
fn is_ulid_shape(s: &str) -> bool {
	if s.len() != 26 {
		return false;
	}
	s.bytes().all(|b| {
		b.is_ascii_digit()
			|| (b'A'..=b'Z').contains(&b)
			|| (b'a'..=b'z').contains(&b)
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parses_question_marker_with_hint() {
		let body = "fn x() {}\n\
		            //? Q#01HW2K3M4N5P6Q7R8S9TABCDE  reduce alloc\n\
		            fn y() {}\n";
		let markers = scan_str(body);
		assert_eq!(markers.len(), 1);
		assert_eq!(markers[0].kind, MarkerKind::Question);
		assert_eq!(markers[0].line, 2);
		assert_eq!(markers[0].id, "01HW2K3M4N5P6Q7R8S9TABCDE");
		assert_eq!(markers[0].hint, "reduce alloc");
	}

	#[test]
	fn parses_answer_marker_no_hint() {
		let body = "//? A#01HW2K3M4N5P6Q7R8S9TABCDE\n";
		let markers = scan_str(body);
		assert_eq!(markers.len(), 1);
		assert_eq!(markers[0].kind, MarkerKind::Answer);
		assert_eq!(markers[0].hint, "");
	}

	#[test]
	fn ignores_regular_comments() {
		let body = "// Q#01HW2K3M4N5P6Q7R8S9TABCDE  not a marker\n\
		            /// docstring\n\
		            //! crate doc\n";
		assert!(scan_str(body).is_empty());
	}

	#[test]
	fn ignores_malformed_id() {
		let body = "//? Q#bogus  too short\n\
		            //? Q#0123456789012345678901234!  bad char\n";
		assert!(scan_str(body).is_empty());
	}

	#[test]
	fn skips_marker_without_known_kind() {
		let body = "//? X#01HW2K3M4N5P6Q7R8S9TABCDE  unknown\n";
		assert!(scan_str(body).is_empty());
	}
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test -p relay-ask marker --lib`
Expected: FAIL TO COMPILE — module imports the still-missing `priority`/`query` modules from `lib.rs`. Use `cargo test -p relay-ask --lib --no-run` first; once Task 5 + Task 6 + Task 7 land, the suite will run.

(If a check on this task in isolation is required, temporarily replace `src/relay/ask/src/lib.rs` with `pub mod marker; pub mod payload;` only; revert after Task 7.)

- [ ] **Step 3: Commit**

```bash
git add src/relay/ask/src/marker.rs
git commit -m "feat(ask): //? Q#<ulid> / A#<ulid> marker parser"
```

---

### Task 5: Priority formula

**Files:**
- Create: `src/relay/ask/src/priority.rs`
- Test: `src/relay/ask/src/priority.rs` (inline `#[cfg(test)]`)

- [ ] **Step 1: Write the failing test**

Create `src/relay/ask/src/priority.rs`:

```rust
//! Pure priority formula for bubble ranking. No I/O; deterministic.

use crate::payload::{AskPayload, AskTag};

/// Configurable weights. `Default` is the v1 tuning; tweak via config later.
#[derive(Debug, Clone, Copy)]
pub struct Weights {
	/// Multiplier on `(100 - file.@score)`.
	pub w_score: f32,
	/// Multiplier on `ln(1 + age_seconds)`.
	pub w_age: f32,
	/// Multiplier on `executor blocked?` (0/1).
	pub w_block: f32,
	/// Multiplier on `safety tag present?` (0/1).
	pub w_safety: f32,
}

impl Default for Weights {
	fn default() -> Self {
		Self {
			w_score: 1.0,
			w_age: 4.0,
			w_block: 30.0,
			w_safety: 50.0,
		}
	}
}

/// Compute a priority score. Higher = closer to top of bubble.
///
/// `now_ms` and `entry_ts_ms` are unix-epoch ms.
/// `file_score` is `@score` of the file the ask sits in (0-100).
/// `executor_blocked` is whether the file's executor is currently paused on this ask.
pub fn score_for(
	ask: &AskPayload,
	entry_ts_ms: u64,
	now_ms: u64,
	file_score: u32,
	executor_blocked: bool,
	w: Weights,
) -> f32 {
	let age_s = ((now_ms.saturating_sub(entry_ts_ms)) as f32) / 1000.0;
	let age_term = (1.0 + age_s).ln();
	let score_term = (100.0 - file_score as f32).max(0.0);
	let block_term = if executor_blocked { 1.0 } else { 0.0 };
	let safety_term = if ask.tags.contains(&AskTag::Safety) { 1.0 } else { 0.0 };
	w.w_score * score_term + w.w_age * age_term + w.w_block * block_term + w.w_safety * safety_term
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::payload::Status;

	fn ask(tags: Vec<AskTag>) -> AskPayload {
		AskPayload {
			id: "01HW2K3M4N5P6Q7R8S9TABCDE".into(),
			text: "q".into(),
			agent_id: "x".into(),
			status: Status::Open,
			tags,
		}
	}

	#[test]
	fn safety_tag_strictly_outranks_nit_at_same_age_and_score() {
		let w = Weights::default();
		let now = 1_700_000_000_000u64;
		let safety = score_for(&ask(vec![AskTag::Safety]), now, now, 80, false, w);
		let nit = score_for(&ask(vec![AskTag::Nit]), now, now, 80, false, w);
		assert!(safety > nit);
	}

	#[test]
	fn lower_file_score_outranks_higher_file_score_at_same_age() {
		let w = Weights::default();
		let now = 1_700_000_000_000u64;
		let low = score_for(&ask(vec![]), now, now, 30, false, w);
		let high = score_for(&ask(vec![]), now, now, 90, false, w);
		assert!(low > high);
	}

	#[test]
	fn blocked_executor_outranks_unblocked_at_otherwise_equal() {
		let w = Weights::default();
		let now = 1_700_000_000_000u64;
		let blocked = score_for(&ask(vec![]), now, now, 80, true, w);
		let free = score_for(&ask(vec![]), now, now, 80, false, w);
		assert!(blocked > free);
	}

	#[test]
	fn age_increases_score_monotonically() {
		let w = Weights::default();
		let now = 1_700_000_010_000u64;
		let young = score_for(&ask(vec![]), now - 1_000, now, 80, false, w);
		let old = score_for(&ask(vec![]), now - 1_000_000, now, 80, false, w);
		assert!(old > young);
	}
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p relay-ask priority --lib`
Expected: PASS once Task 7 lands and the crate compiles.

- [ ] **Step 3: Commit**

```bash
git add src/relay/ask/src/priority.rs
git commit -m "feat(ask): priority formula with weights"
```

---

### Task 6: Typed entry builders

**Files:**
- Create: `src/relay/ask/src/build.rs`
- Test: `src/relay/ask/src/build.rs` (inline `#[cfg(test)]`)

- [ ] **Step 1: Write the failing test**

Create `src/relay/ask/src/build.rs`:

```rust
//! Typed builders for ask-loop journal entries. Wrap `Entry::new`
//! with the right `Kind` and serde-encoded payload.

use relay_journal::{Entry, Kind};
use ulid::Ulid;

use crate::payload::{
	AnswerPayload, AskPayload, AskTag, GoalPayload, GoalScope,
	MilestoneCriteria, MilestonePayload, MilestoneStatus, Status,
};

/// Build a fresh `Ask` entry stamped with a new ULID. Returns the
/// entry plus the ULID string (for embedding into the code marker).
pub fn new_ask(
	file: impl Into<String>,
	text: impl Into<String>,
	agent_id: impl Into<String>,
	tags: Vec<AskTag>,
) -> (Entry, String) {
	let id = Ulid::new().to_string();
	let payload = AskPayload {
		id: id.clone(),
		text: text.into(),
		agent_id: agent_id.into(),
		status: Status::Open,
		tags,
	};
	let value = serde_json::to_value(&payload).expect("AskPayload serialises");
	(Entry::new(Kind::Ask, file, value), id)
}

/// Build an `Answer` entry. `ref_id` must be the originating ask's ULID.
pub fn new_answer(
	ref_id: impl Into<String>,
	text: impl Into<String>,
	file: impl Into<String>,
) -> Entry {
	let payload = AnswerPayload {
		id: Ulid::new().to_string(),
		ref_id: ref_id.into(),
		text: text.into(),
	};
	let value = serde_json::to_value(&payload).expect("AnswerPayload serialises");
	Entry::new(Kind::Answer, file, value)
}

/// Build a `Goal` entry.
pub fn new_goal(text: impl Into<String>, scope: GoalScope) -> (Entry, String) {
	let id = Ulid::new().to_string();
	let payload = GoalPayload {
		id: id.clone(),
		text: text.into(),
		scope,
	};
	let value = serde_json::to_value(&payload).expect("GoalPayload serialises");
	(Entry::new(Kind::Goal, "", value), id)
}

/// Build a `Milestone` entry. `goal_id` must be a known goal ULID.
pub fn new_milestone(
	goal_id: impl Into<String>,
	text: impl Into<String>,
	criteria: MilestoneCriteria,
	status: MilestoneStatus,
) -> Entry {
	let payload = MilestonePayload {
		id: Ulid::new().to_string(),
		ref_id: goal_id.into(),
		text: text.into(),
		criteria,
		status,
	};
	let value = serde_json::to_value(&payload).expect("MilestonePayload serialises");
	Entry::new(Kind::Milestone, "", value)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn new_ask_stamps_ulid_and_open_status() {
		let (e, id) = new_ask("src/foo.rs", "should X?", "exec-foo", vec![AskTag::Design]);
		assert_eq!(e.kind, Kind::Ask);
		assert_eq!(e.key, "src/foo.rs");
		assert_eq!(id.len(), 26);
		let body: AskPayload = serde_json::from_value(e.payload).unwrap();
		assert_eq!(body.id, id);
		assert_eq!(body.status, Status::Open);
		assert_eq!(body.tags, vec![AskTag::Design]);
	}

	#[test]
	fn new_answer_carries_ref_id() {
		let e = new_answer("01HW2K3M4N5P6Q7R8S9TABCDE", "yes", "src/foo.rs");
		assert_eq!(e.kind, Kind::Answer);
		let body: AnswerPayload = serde_json::from_value(e.payload).unwrap();
		assert_eq!(body.ref_id, "01HW2K3M4N5P6Q7R8S9TABCDE");
	}

	#[test]
	fn new_goal_returns_id_and_writes_payload() {
		let (e, id) = new_goal("ship ask loop v1", GoalScope::Dir("src/relay".into()));
		assert_eq!(e.kind, Kind::Goal);
		let body: GoalPayload = serde_json::from_value(e.payload).unwrap();
		assert_eq!(body.id, id);
	}

	#[test]
	fn new_milestone_links_goal() {
		let crit = MilestoneCriteria { asks_resolved: true, ..Default::default() };
		let e = new_milestone("01HG", "v1 done", crit.clone(), MilestoneStatus::Pending);
		assert_eq!(e.kind, Kind::Milestone);
		let body: MilestonePayload = serde_json::from_value(e.payload).unwrap();
		assert_eq!(body.ref_id, "01HG");
		assert_eq!(body.status, MilestoneStatus::Pending);
		assert!(body.criteria.asks_resolved);
	}
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p relay-ask build --lib`
Expected: PASS once Task 7 lands.

- [ ] **Step 3: Commit**

```bash
git add src/relay/ask/src/build.rs
git commit -m "feat(ask): typed builders for ask/answer/goal/milestone entries"
```

---

### Task 7: Journal query helpers

**Files:**
- Create: `src/relay/ask/src/query.rs`
- Test: `src/relay/ask/src/query.rs` (inline `#[cfg(test)]`)

- [ ] **Step 1: Write the failing test**

Create `src/relay/ask/src/query.rs`:

```rust
//! Read-side helpers: scan day journal + history, project entries
//! into `OpenAsk` records, filter by file or status.

use relay_journal::{Entry, Filter, History, Kind};

use crate::payload::{AnswerPayload, AskPayload, GoalPayload, Status};

/// Open ask projection used by the bubble.
#[derive(Debug, Clone)]
pub struct OpenAsk {
	/// File path the ask is anchored in (`Entry.key`).
	pub file: String,
	/// Wall-clock ms when the ask was raised.
	pub ts_ms: u64,
	/// Decoded payload.
	pub payload: AskPayload,
}

/// Return all open asks across journal + history.
pub fn open_asks_all(today: &[Entry], history: &History) -> Vec<OpenAsk> {
	let mut answered: std::collections::HashSet<String> = collect_answered(today);

	let mut out = Vec::new();
	collect_asks(today.iter().cloned(), &mut out);

	let hist_entries = history
		.query(Filter::all())
		.unwrap_or_default();
	for e in &hist_entries {
		if e.kind == Kind::Answer {
			if let Ok(p) = serde_json::from_value::<AnswerPayload>(e.payload.clone()) {
				answered.insert(p.ref_id);
			}
		}
	}
	collect_asks(hist_entries.into_iter(), &mut out);

	out.retain(|a| !answered.contains(&a.payload.id) && a.payload.status == Status::Open);
	out
}

/// Same, but filtered to a single source file.
pub fn open_asks_for_file(today: &[Entry], history: &History, file: &str) -> Vec<OpenAsk> {
	let mut all = open_asks_all(today, history);
	all.retain(|a| a.file == file);
	all
}

/// Return all open goals (no `milestone_reached` whose status flips them done).
pub fn goals_open(today: &[Entry], history: &History) -> Vec<GoalPayload> {
	let mut out = Vec::new();
	for e in today
		.iter()
		.cloned()
		.chain(history.query(Filter::all()).unwrap_or_default())
	{
		if e.kind == Kind::Goal {
			if let Ok(p) = serde_json::from_value::<GoalPayload>(e.payload) {
				out.push(p);
			}
		}
	}
	out
}

fn collect_asks<I: Iterator<Item = Entry>>(iter: I, out: &mut Vec<OpenAsk>) {
	for e in iter {
		if e.kind != Kind::Ask {
			continue;
		}
		let Ok(p) = serde_json::from_value::<AskPayload>(e.payload) else { continue };
		out.push(OpenAsk { file: e.key, ts_ms: e.ts_ms, payload: p });
	}
}

fn collect_answered(entries: &[Entry]) -> std::collections::HashSet<String> {
	let mut s = std::collections::HashSet::new();
	for e in entries {
		if e.kind == Kind::Answer {
			if let Ok(p) = serde_json::from_value::<AnswerPayload>(e.payload.clone()) {
				s.insert(p.ref_id);
			}
		}
	}
	s
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::build::{new_answer, new_ask, new_goal};
	use crate::payload::{AskTag, GoalScope};

	#[test]
	fn open_asks_excludes_answered_in_today() {
		let history = History::open_in_memory().unwrap();
		let (a1, id1) = new_ask("src/foo.rs", "q1", "exec", vec![AskTag::Design]);
		let (a2, _id2) = new_ask("src/foo.rs", "q2", "exec", vec![AskTag::Nit]);
		let ans = new_answer(&id1, "do X", "src/foo.rs");
		let today = vec![a1, a2.clone(), ans];

		let open = open_asks_all(&today, &history);
		assert_eq!(open.len(), 1);
		assert_eq!(open[0].payload.text, "q2");
	}

	#[test]
	fn open_asks_for_file_filters_by_path() {
		let history = History::open_in_memory().unwrap();
		let (a1, _) = new_ask("src/foo.rs", "q1", "exec", vec![]);
		let (a2, _) = new_ask("src/bar.rs", "q2", "exec", vec![]);
		let today = vec![a1, a2];

		let foo = open_asks_for_file(&today, &history, "src/foo.rs");
		assert_eq!(foo.len(), 1);
		assert_eq!(foo[0].payload.text, "q1");
	}

	#[test]
	fn goals_open_collects_goal_entries() {
		let history = History::open_in_memory().unwrap();
		let (g, _) = new_goal("ship v1", GoalScope::All);
		let goals = goals_open(&[g], &history);
		assert_eq!(goals.len(), 1);
		assert_eq!(goals[0].text, "ship v1");
	}
}
```

- [ ] **Step 2: Run all `relay-ask` tests**

Run: `cargo test -p relay-ask --lib`
Expected: PASS for all four modules (`payload`, `marker`, `priority`, `build`, `query`). The crate now compiles.

- [ ] **Step 3: Verify the workspace still builds**

Run: `cargo check --workspace`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add src/relay/ask/src/query.rs
git commit -m "feat(ask): journal query helpers for open asks + goals"
```

---

### Task 8: Marker scan integration test

**Files:**
- Create: `src/relay/ask/tests/marker_scan.rs`

- [ ] **Step 1: Write the failing integration test**

Create `src/relay/ask/tests/marker_scan.rs`:

```rust
use std::io::Write;

use relay_ask::{scan_file, MarkerKind};
use tempfile::NamedTempFile;

#[test]
fn finds_question_and_answer_markers_in_real_file() {
	let mut f = NamedTempFile::new().unwrap();
	writeln!(f, "fn entry() {{}}").unwrap();
	writeln!(f, "//? Q#01HW2K3M4N5P6Q7R8S9TABCDE  cache here?").unwrap();
	writeln!(f, "fn hot() {{}}").unwrap();
	writeln!(f, "//? A#01HW2K3M4N5P6Q7R8S9TABCDF  use arena").unwrap();
	f.flush().unwrap();

	let markers = scan_file(f.path()).unwrap();
	assert_eq!(markers.len(), 2);
	assert_eq!(markers[0].kind, MarkerKind::Question);
	assert_eq!(markers[0].id, "01HW2K3M4N5P6Q7R8S9TABCDE");
	assert_eq!(markers[1].kind, MarkerKind::Answer);
	assert_eq!(markers[1].line, 4);
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p relay-ask --test marker_scan`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/relay/ask/tests/marker_scan.rs
git commit -m "test(ask): integration test for file marker scan"
```

---

### Task 9: Bubble plugin scaffold (`relay-ask-bubble`)

**Files:**
- Create: `src/plugins/ask-bubble/Cargo.toml`
- Create: `src/plugins/ask-bubble/src/lib.rs`
- Modify: `Cargo.toml` (workspace members)

This task wires a UI slot plugin that renders the top-5 open asks. It depends on the existing `ui_slots` plugin and the workspace `harness::Plugin` trait. We mirror the structure of `src/plugins/clock/` (the reference timer-driven UI plugin).

- [ ] **Step 1: Add workspace member**

Edit root `Cargo.toml`. After `"src/plugins/relay"`:

```toml
	"src/plugins/ask-bubble",
```

- [ ] **Step 2: Crate manifest**

Create `src/plugins/ask-bubble/Cargo.toml`:

```toml
[package]
name = "relay-ask-bubble"
version = "0.1.0"
edition = "2021"
license = "MIT"

[lib]
path = "src/lib.rs"

[dependencies]
relay-ask = { path = "../../relay/ask" }
relay-journal = { path = "../../relay/journal" }
relay-harness = { path = "../../relay/harness" }
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true }

[lints]
workspace = true
```

- [ ] **Step 3: Plugin lib stub**

Create `src/plugins/ask-bubble/src/lib.rs`:

```rust
//! Renders the top-5 open asks into the `above_input.right` UI slot.
//!
//! The plugin reads the journal (today + history) on:
//! 1. a 30-second tokio timer,
//! 2. the harness `pre_turn` and `post_turn` lifecycle events,
//! 3. (future) a journal-write callback.
//!
//! Each tick, it scores all open asks via `relay_ask::score_for`, sorts
//! desc, and pushes the top 5 as one composed UI string per row.

#![deny(missing_docs)]

use std::sync::Arc;
use std::time::Duration;

use relay_ask::{open_asks_all, score_for, OpenAsk, Weights};
use relay_journal::{Entry, History};

/// Default refresh cadence in seconds.
pub const DEFAULT_TICK_SECS: u64 = 30;

/// Read-side handle the plugin needs from the host.
///
/// Trait kept narrow so the plugin can be unit-tested without spinning
/// up the full harness.
pub trait BubbleHost: Send + Sync {
	/// Snapshot of today's journal entries.
	fn today_entries(&self) -> Vec<Entry>;
	/// Shared access to the warm history store.
	fn history(&self) -> Arc<History>;
	/// Look up the `@score` for a given file path. 0–100; defaults to 50.
	fn file_score(&self, path: &str) -> u32;
	/// Whether the file's executor is currently paused on this ask.
	fn executor_blocked(&self, ask_id: &str) -> bool;
	/// Push a fully-rendered row into the UI slot.
	///
	/// `rows` is already trimmed to the top-N; the host is responsible
	/// for delivering them through the `ui_slots` push API.
	fn push_rows(&self, rows: Vec<String>);
}

/// One render pass: read journal, score, push top-5.
pub fn render_once(host: &dyn BubbleHost, weights: Weights, now_ms: u64) {
	let today = host.today_entries();
	let history = host.history();
	let mut open = open_asks_all(&today, history.as_ref());
	let mut scored: Vec<(f32, OpenAsk)> = open
		.drain(..)
		.map(|a| {
			let s = score_for(
				&a.payload,
				a.ts_ms,
				now_ms,
				host.file_score(&a.file),
				host.executor_blocked(&a.payload.id),
				weights,
			);
			(s, a)
		})
		.collect();
	scored.sort_by(|l, r| r.0.partial_cmp(&l.0).unwrap_or(std::cmp::Ordering::Equal));
	scored.truncate(5);

	let rows: Vec<String> = scored
		.into_iter()
		.map(|(_, a)| format_row(&a))
		.collect();
	host.push_rows(rows);
}

fn format_row(a: &OpenAsk) -> String {
	let head = a
		.payload
		.tags
		.first()
		.map(|t| match t {
			relay_ask::AskTag::Safety => '!',
			relay_ask::AskTag::Design => '·',
			relay_ask::AskTag::Behavior => '~',
			relay_ask::AskTag::Nit => '.',
		})
		.unwrap_or(' ');
	let trimmed = if a.payload.text.len() > 60 {
		format!("{}…", &a.payload.text[..59])
	} else {
		a.payload.text.clone()
	};
	format!("[{head}] {}  {}", a.file, trimmed)
}

/// Spawn the 30-second refresh task. Caller owns the returned handle.
pub fn spawn_timer(
	host: Arc<dyn BubbleHost>,
	weights: Weights,
	tick_secs: u64,
) -> tokio::task::JoinHandle<()> {
	tokio::spawn(async move {
		let mut interval = tokio::time::interval(Duration::from_secs(tick_secs));
		loop {
			interval.tick().await;
			let now_ms = std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.map(|d| d.as_millis() as u64)
				.unwrap_or(0);
			render_once(host.as_ref(), weights, now_ms);
		}
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use relay_ask::{new_ask, AskTag};
	use std::sync::Mutex;

	struct FakeHost {
		today: Vec<Entry>,
		history: Arc<History>,
		pushed: Mutex<Vec<Vec<String>>>,
	}
	impl BubbleHost for FakeHost {
		fn today_entries(&self) -> Vec<Entry> { self.today.clone() }
		fn history(&self) -> Arc<History> { self.history.clone() }
		fn file_score(&self, _: &str) -> u32 { 50 }
		fn executor_blocked(&self, _: &str) -> bool { false }
		fn push_rows(&self, rows: Vec<String>) { self.pushed.lock().unwrap().push(rows); }
	}

	#[test]
	fn render_once_pushes_top_five_sorted_desc() {
		let history = Arc::new(History::open_in_memory().unwrap());
		// Seed 7 asks across two files; expect top 5 chosen by score.
		let mut today = Vec::new();
		for i in 0..7 {
			let tags = if i == 0 { vec![AskTag::Safety] } else { vec![AskTag::Nit] };
			let (e, _) = new_ask(format!("src/f{i}.rs"), format!("q{i}"), "exec", tags);
			today.push(e);
		}
		let host = FakeHost { today, history, pushed: Mutex::new(Vec::new()) };
		render_once(&host, Weights::default(), 1_700_000_000_000);

		let pushed = host.pushed.lock().unwrap();
		assert_eq!(pushed.len(), 1);
		assert_eq!(pushed[0].len(), 5);
		assert!(pushed[0][0].starts_with("[!] "), "safety-tagged ask should be first: {:?}", pushed[0][0]);
	}
}
```

- [ ] **Step 4: Run the test**

Run: `cargo test -p relay-ask-bubble --lib`
Expected: PASS.

- [ ] **Step 5: Verify the workspace builds**

Run: `cargo check --workspace`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml src/plugins/ask-bubble/Cargo.toml src/plugins/ask-bubble/src/lib.rs
git commit -m "feat(ask-bubble): plugin scaffold with render_once + timer task"
```

---

### Task 10: Slash commands `/ask` and `/asks-for`

**Files:**
- Modify: `src/relay/commands/src/lib.rs` (or the file that registers built-in slash commands)
- Test: `src/relay/commands/tests/ask_commands.rs`

This task wires user input. `/ask <ulid>` opens a kern conversation pinned to one ask; `/asks-for <file>` opens kern with all open asks for that file bundled. Both produce a structured prompt that the kern binary surfaces to the active session.

- [ ] **Step 1: Locate the commands module's registration site**

Run: `cargo doc -p relay-commands --no-deps --open` (optional) or read `src/relay/commands/src/lib.rs` to identify the existing `register` / dispatch shape. Confirm whether commands are registered via a `phf` table, an enum, or a `Vec<&'static dyn Command>` — follow the existing pattern. Do not invent a new mechanism.

- [ ] **Step 2: Write the failing tests**

Create `src/relay/commands/tests/ask_commands.rs`:

```rust
//! Verifies that `/ask` and `/asks-for` are registered and produce
//! structured prompts that include the relevant journal context.

use relay_commands::{dispatch, Outcome};

// NOTE: this test assumes the host wires a fake `BubbleHost`-equivalent.
// The real test fixture lives in src/relay/commands/src/test_support.rs;
// re-use whatever the existing command tests already use.

#[test]
fn ask_command_returns_prompt_for_known_ulid() {
	let fixture = relay_commands::test_support::fixture_with_ask(
		"src/foo.rs",
		"01HW2K3M4N5P6Q7R8S9TABCDE",
		"should we cache?",
	);
	let out = dispatch("/ask 01HW2K3M4N5P6Q7R8S9TABCDE", &fixture);
	match out {
		Outcome::Openkern { prompt, file } => {
			assert!(prompt.contains("should we cache?"));
			assert_eq!(file.as_deref(), Some("src/foo.rs"));
		}
		other => panic!("expected Openkern, got {other:?}"),
	}
}

#[test]
fn asks_for_command_bundles_all_open_asks_for_file() {
	let fixture = relay_commands::test_support::fixture_with_two_asks_in_same_file();
	let out = dispatch("/asks-for src/foo.rs", &fixture);
	match out {
		Outcome::Openkern { prompt, file } => {
			assert_eq!(file.as_deref(), Some("src/foo.rs"));
			assert!(prompt.contains("Open asks for src/foo.rs"));
			assert!(prompt.matches("Q#").count() >= 2);
		}
		other => panic!("expected Openkern, got {other:?}"),
	}
}
```

- [ ] **Step 3: Run tests to verify failure**

Run: `cargo test -p relay-commands --test ask_commands`
Expected: FAIL — commands not registered, fixtures missing.

- [ ] **Step 4: Implement the commands**

Edit `src/relay/commands/src/lib.rs` (or wherever commands are wired):

```rust
// Existing imports + module skeleton stay as-is. Add at the top of the file:
use relay_ask::{open_asks_all, open_asks_for_file};
use relay_journal::{Entry, History};
use std::sync::Arc;
```

Add a new outcome variant (or use the existing equivalent — pattern-match the project):

```rust
/// Result of a slash command. Adapt to your existing enum.
#[derive(Debug)]
pub enum Outcome {
	/// Open a kern session prepopulated with `prompt`, optionally
	/// pinned to `file`.
	Openkern { prompt: String, file: Option<String> },
	/// Existing variants…
	Other(()),
}
```

Add the two command handlers (names match whatever convention the file already uses):

```rust
pub fn cmd_ask(ulid: &str, host: &dyn AskCommandHost) -> Outcome {
	let today = host.today_entries();
	let history = host.history();
	let asks = open_asks_all(&today, history.as_ref());
	let Some(found) = asks.into_iter().find(|a| a.payload.id == ulid) else {
		return Outcome::Openkern {
			prompt: format!("No open ask with id {ulid}."),
			file: None,
		};
	};
	let prompt = format!(
		"Resolving ask {ulid}\n\
		File: {file}\n\n\
		Question: {q}\n",
		ulid = found.payload.id,
		file = found.file,
		q = found.payload.text,
	);
	Outcome::Openkern { prompt, file: Some(found.file) }
}

pub fn cmd_asks_for(file: &str, host: &dyn AskCommandHost) -> Outcome {
	let today = host.today_entries();
	let history = host.history();
	let asks = open_asks_for_file(&today, history.as_ref(), file);
	let mut prompt = format!("Open asks for {file}:\n");
	for a in &asks {
		prompt.push_str(&format!("- Q#{} {}\n", a.payload.id, a.payload.text));
	}
	Outcome::Openkern { prompt, file: Some(file.into()) }
}

/// Read-only host trait for command tests.
pub trait AskCommandHost: Send + Sync {
	fn today_entries(&self) -> Vec<Entry>;
	fn history(&self) -> Arc<History>;
}
```

Wire them into the dispatch table per the file's existing pattern (e.g., `register("ask", cmd_ask_dyn)` etc.). Add `dispatch(input: &str, host: &dyn AskCommandHost) -> Outcome` if the file does not already have an equivalent.

- [ ] **Step 5: Add the test fixture module**

Create or extend `src/relay/commands/src/test_support.rs`:

```rust
//! Test fixtures for command tests.
#![cfg(any(test, feature = "test-support"))]

use std::sync::Arc;

use relay_ask::{new_ask, AskTag};
use relay_journal::{Entry, History};

use crate::AskCommandHost;

/// Fixture that exposes one open ask in the journal.
pub fn fixture_with_ask(file: &str, _ulid: &str, text: &str) -> Fixture {
	let history = Arc::new(History::open_in_memory().unwrap());
	let (e, _id) = new_ask(file, text, "exec", vec![AskTag::Design]);
	Fixture { today: vec![e], history }
}

/// Fixture with two open asks in the same file.
pub fn fixture_with_two_asks_in_same_file() -> Fixture {
	let history = Arc::new(History::open_in_memory().unwrap());
	let (e1, _) = new_ask("src/foo.rs", "q1", "exec", vec![AskTag::Design]);
	let (e2, _) = new_ask("src/foo.rs", "q2", "exec", vec![AskTag::Behavior]);
	Fixture { today: vec![e1, e2], history }
}

/// Concrete fixture host.
pub struct Fixture {
	today: Vec<Entry>,
	history: Arc<History>,
}

impl AskCommandHost for Fixture {
	fn today_entries(&self) -> Vec<Entry> { self.today.clone() }
	fn history(&self) -> Arc<History> { self.history.clone() }
}
```

Re-export from `lib.rs`: `pub mod test_support;` (gated `#[cfg(any(test, feature = "test-support"))]`).

- [ ] **Step 6: Run tests to verify pass**

Run: `cargo test -p relay-commands --test ask_commands`
Expected: PASS for both tests.

- [ ] **Step 7: Verify the workspace builds**

Run: `cargo check --workspace`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add src/relay/commands/src/lib.rs src/relay/commands/src/test_support.rs src/relay/commands/tests/ask_commands.rs src/relay/commands/Cargo.toml
git commit -m "feat(commands): /ask and /asks-for slash commands"
```

---

### Task 11: Goal-criteria evaluator + milestone emission

**Files:**
- Create: `src/relay/ask/src/milestone.rs`
- Modify: `src/relay/ask/src/lib.rs` (add module + re-exports)
- Test: `src/relay/ask/src/milestone.rs` (inline `#[cfg(test)]`)

- [ ] **Step 1: Write the failing test**

Create `src/relay/ask/src/milestone.rs`:

```rust
//! Evaluate `MilestoneCriteria` against current journal state. Used
//! by the harness to emit a `milestone_reached` entry when a goal's
//! criteria become satisfied.

use relay_journal::{Entry, History};

use crate::payload::{
	GoalPayload, GoalScope, MilestoneCriteria, MilestoneStatus,
};
use crate::query::{open_asks_all, open_asks_for_file};

/// Caller-supplied facts the evaluator cannot derive from the journal.
pub struct Facts<'a> {
	/// Mean `@score` across the relevant scope.
	pub mean_score: u32,
	/// Whether `cargo test` (or equivalent) is green.
	pub tests_pass: bool,
	/// Today's in-memory entries.
	pub today: &'a [Entry],
	/// Warm history store.
	pub history: &'a History,
}

/// Returns `Reached` iff every set criterion holds.
pub fn evaluate(
	goal: &GoalPayload,
	criteria: &MilestoneCriteria,
	facts: &Facts<'_>,
) -> MilestoneStatus {
	if let Some(min) = criteria.min_score {
		if facts.mean_score < min {
			return MilestoneStatus::Pending;
		}
	}
	if criteria.tests_pass && !facts.tests_pass {
		return MilestoneStatus::Pending;
	}
	if criteria.asks_resolved {
		let any_open = match &goal.scope {
			GoalScope::All => !open_asks_all(facts.today, facts.history).is_empty(),
			GoalScope::Files(files) => files.iter().any(|f| {
				!open_asks_for_file(facts.today, facts.history, f).is_empty()
			}),
			GoalScope::Dir(prefix) => {
				let asks = open_asks_all(facts.today, facts.history);
				asks.into_iter().any(|a| a.file.starts_with(prefix.as_str()))
			}
		};
		if any_open {
			return MilestoneStatus::Pending;
		}
	}
	MilestoneStatus::Reached
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::build::{new_answer, new_ask, new_goal};
	use crate::payload::AskTag;

	#[test]
	fn unmet_min_score_keeps_pending() {
		let history = History::open_in_memory().unwrap();
		let (g_entry, _) = new_goal("v1", GoalScope::All);
		let goal: GoalPayload =
			serde_json::from_value(g_entry.payload).unwrap();
		let crit = MilestoneCriteria { min_score: Some(80), ..Default::default() };
		let facts = Facts { mean_score: 60, tests_pass: true, today: &[], history: &history };
		assert_eq!(evaluate(&goal, &crit, &facts), MilestoneStatus::Pending);
	}

	#[test]
	fn open_ask_in_dir_scope_keeps_pending() {
		let history = History::open_in_memory().unwrap();
		let (a, _) = new_ask("src/relay/foo.rs", "q", "exec", vec![AskTag::Design]);
		let (g_entry, _) = new_goal("v1", GoalScope::Dir("src/relay".into()));
		let goal: GoalPayload =
			serde_json::from_value(g_entry.payload).unwrap();
		let crit = MilestoneCriteria { asks_resolved: true, ..Default::default() };
		let facts = Facts { mean_score: 100, tests_pass: true, today: &[a], history: &history };
		assert_eq!(evaluate(&goal, &crit, &facts), MilestoneStatus::Pending);
	}

	#[test]
	fn answered_ask_unblocks_milestone() {
		let history = History::open_in_memory().unwrap();
		let (a_entry, id) = new_ask("src/relay/foo.rs", "q", "exec", vec![AskTag::Design]);
		let answer = new_answer(&id, "yes", "src/relay/foo.rs");
		let (g_entry, _) = new_goal("v1", GoalScope::Dir("src/relay".into()));
		let goal: GoalPayload =
			serde_json::from_value(g_entry.payload).unwrap();
		let crit = MilestoneCriteria { asks_resolved: true, ..Default::default() };
		let facts = Facts {
			mean_score: 100,
			tests_pass: true,
			today: &[a_entry, answer],
			history: &history,
		};
		assert_eq!(evaluate(&goal, &crit, &facts), MilestoneStatus::Reached);
	}
}
```

- [ ] **Step 2: Add module + re-exports**

Edit `src/relay/ask/src/lib.rs`:

```rust
pub mod milestone;
pub use milestone::{evaluate as evaluate_milestone, Facts as MilestoneFacts};
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p relay-ask milestone --lib`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/relay/ask/src/milestone.rs src/relay/ask/src/lib.rs
git commit -m "feat(ask): milestone criteria evaluator"
```

---

### Task 12: Wire `relay-ask-bubble` into the kern binary (registration only)

**Files:**
- Modify: `src/bin/relay/kern/src/main.rs` (or the wiring entry point that builds plugins)
- Modify: `src/bin/relay/kern/Cargo.toml`

This is plumbing only — no new behaviour beyond what Task 9 already tested. The goal is that running `cargo run -p relay-kern` instantiates the bubble plugin so the timer is live.

- [ ] **Step 1: Read the existing plugin registration site**

Open `src/bin/relay/kern/src/main.rs` (or follow the chain from there to wherever plugins are added — likely a `build_registry()` helper). Confirm the pattern: each plugin gets `Arc::new(MyPlugin::new(...))` and is added to a `Registry` builder.

- [ ] **Step 2: Add the dependency**

Edit `src/bin/relay/kern/Cargo.toml`:

```toml
relay-ask-bubble = { path = "../../../plugins/ask-bubble" }
relay-ask        = { path = "../../../relay/ask" }
```

(Adjust relative paths to match the existing crate-path style in the file. Look at how `relay-journal` is referenced and mirror exactly.)

- [ ] **Step 3: Build a `BubbleHost` adapter**

Create `src/bin/relay/kern/src/ask_bubble_host.rs`:

```rust
//! Adapter that bridges the kern binary's existing journal + ui_slots
//! handles to the `BubbleHost` trait the bubble plugin expects.

use std::sync::Arc;

use relay_ask_bubble::BubbleHost;
use relay_journal::{Entry, History};

/// Host wiring all the read-only handles the bubble needs.
pub struct kernBubbleHost {
	/// Snapshot fn — supplied by the kern session's existing journal handle.
	pub today_fn: Arc<dyn Fn() -> Vec<Entry> + Send + Sync>,
	/// Warm history store, shared with the rest of relay.
	pub history: Arc<History>,
	/// File-`@score` lookup. Stub returning 50 for v1 until the score
	/// header reader lands; ranking degrades to age + tags.
	pub file_score_fn: Arc<dyn Fn(&str) -> u32 + Send + Sync>,
	/// Executor block-state lookup. Stub returning false for v1.
	pub blocked_fn: Arc<dyn Fn(&str) -> bool + Send + Sync>,
	/// `ui_slots` push closure, scoped to a single slot id.
	pub push_fn: Arc<dyn Fn(Vec<String>) + Send + Sync>,
}

impl BubbleHost for kernBubbleHost {
	fn today_entries(&self) -> Vec<Entry> { (self.today_fn)() }
	fn history(&self) -> Arc<History> { self.history.clone() }
	fn file_score(&self, p: &str) -> u32 { (self.file_score_fn)(p) }
	fn executor_blocked(&self, id: &str) -> bool { (self.blocked_fn)(id) }
	fn push_rows(&self, rows: Vec<String>) { (self.push_fn)(rows) }
}
```

- [ ] **Step 4: Spawn the timer at kern startup**

In `src/bin/relay/kern/src/main.rs`, after the existing `History` handle and `ui_slots` handle are built, call:

```rust
let bubble_host = std::sync::Arc::new(crate::ask_bubble_host::kernBubbleHost {
	today_fn: /* closure capturing the kern session's day_journal scan */,
	history: history.clone(),
	file_score_fn: std::sync::Arc::new(|_| 50),
	blocked_fn: std::sync::Arc::new(|_| false),
	push_fn: /* closure that calls ui_slots.push_rows for slot id "ask" */,
});
let _bubble_handle = relay_ask_bubble::spawn_timer(
	bubble_host,
	relay_ask::Weights::default(),
	relay_ask_bubble::DEFAULT_TICK_SECS,
);
```

The exact closure bodies depend on the existing `day_journal::scan` and `ui_slots::push_rows` shapes. Use whatever the kern binary already uses for those handles — do not introduce a new abstraction.

- [ ] **Step 5: Build and run**

Run: `cargo build -p relay-kern`
Expected: clean.

Run: `cargo run -p relay-kern -- --help`
Expected: existing help output; binary did not regress.

- [ ] **Step 6: Commit**

```bash
git add src/bin/relay/kern/Cargo.toml src/bin/relay/kern/src/ask_bubble_host.rs src/bin/relay/kern/src/main.rs
git commit -m "feat(kern): wire ask-bubble plugin via kernBubbleHost"
```

---

### Task 13: End-to-end loop integration test

**Files:**
- Create: `src/relay/ask/tests/loop_e2e.rs`

This test exercises the full v1 loop in-process (no harness, no plugin host):
1. Build a `History`.
2. Compose today's journal: an `Ask` entry from the executor.
3. Run `render_once` against an in-memory `BubbleHost` and assert the row appears.
4. Append an `Answer` to the journal.
5. Run `render_once` again and assert the row is gone.

- [ ] **Step 1: Write the failing test**

Create `src/relay/ask/tests/loop_e2e.rs`:

```rust
//! End-to-end: ask -> bubble -> answer -> bubble.

use std::sync::{Arc, Mutex};

use relay_ask::{new_answer, new_ask, AskTag, Weights};
use relay_ask_bubble::{render_once, BubbleHost};
use relay_journal::{Entry, History};

struct E2eHost {
	today: Mutex<Vec<Entry>>,
	history: Arc<History>,
	pushed: Mutex<Vec<Vec<String>>>,
}

impl BubbleHost for E2eHost {
	fn today_entries(&self) -> Vec<Entry> { self.today.lock().unwrap().clone() }
	fn history(&self) -> Arc<History> { self.history.clone() }
	fn file_score(&self, _: &str) -> u32 { 50 }
	fn executor_blocked(&self, _: &str) -> bool { false }
	fn push_rows(&self, rows: Vec<String>) { self.pushed.lock().unwrap().push(rows) }
}

#[test]
fn ask_appears_then_disappears_when_answered() {
	let host = E2eHost {
		today: Mutex::new(Vec::new()),
		history: Arc::new(History::open_in_memory().unwrap()),
		pushed: Mutex::new(Vec::new()),
	};

	let (ask_entry, ask_id) = new_ask(
		"src/foo.rs",
		"should we cache here?",
		"exec-foo",
		vec![AskTag::Design],
	);
	host.today.lock().unwrap().push(ask_entry);

	render_once(&host, Weights::default(), 1_700_000_000_000);
	{
		let p = host.pushed.lock().unwrap();
		assert_eq!(p.len(), 1);
		assert_eq!(p[0].len(), 1);
		assert!(p[0][0].contains("should we cache here?"));
	}

	let answer = new_answer(&ask_id, "yes, arena allocator", "src/foo.rs");
	host.today.lock().unwrap().push(answer);

	render_once(&host, Weights::default(), 1_700_000_010_000);
	{
		let p = host.pushed.lock().unwrap();
		assert_eq!(p.len(), 2);
		assert!(p[1].is_empty(), "expected empty bubble after answer; got {:?}", p[1]);
	}
}
```

- [ ] **Step 2: Add `relay-ask-bubble` as a dev-dep on `relay-ask`**

Edit `src/relay/ask/Cargo.toml`:

```toml
[dev-dependencies]
tempfile = "3"
relay-ask-bubble = { path = "../../plugins/ask-bubble" }
```

(Verify this does not create a cycle — `relay-ask-bubble` depends on `relay-ask`, but only as a regular dep; the dev-dep edge runs through `[dev-dependencies]` and is allowed by cargo.)

- [ ] **Step 3: Run the test**

Run: `cargo test -p relay-ask --test loop_e2e`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/relay/ask/tests/loop_e2e.rs src/relay/ask/Cargo.toml
git commit -m "test(ask): end-to-end ask -> bubble -> answer round trip"
```

---

### Task 14: README + ROADMAP updates

**Files:**
- Modify: `src/relay/ask/README.md` (create)
- Modify: `ROADMAP.md` (move ask-loop from "next" to "in flight")

- [ ] **Step 1: Write `relay-ask` README**

Create `src/relay/ask/README.md`:

```markdown
# relay-ask

Ask-loop primitives. Implements the design in
[`docs/superpowers/specs/2026-04-25-relay-ask-loop-design.md`](../../../docs/superpowers/specs/2026-04-25-relay-ask-loop-design.md).

- `payload` — typed bodies for `Ask`, `Answer`, `Goal`, `Milestone`.
- `marker` — `//? Q#<ulid>` / `//? A#<ulid>` parser.
- `priority` — pure ranking formula.
- `query` — read-side helpers over journal + history.
- `build` — typed `Entry` constructors.
- `milestone` — criteria evaluator.

The bubble plugin lives in `src/plugins/ask-bubble/`. Slash commands
(`/ask`, `/asks-for`) live in `src/relay/commands/`.

## Out of scope (v1)

- Drainer integration (executor applies edits directly).
- Cross-file ask clustering via relay edges.
- Compaction → relay ingest at the 30-day boundary (separate plan).
```

- [ ] **Step 2: Update `ROADMAP.md`**

Read the current `ROADMAP.md`. Append (or move into "current") a single bullet:

```markdown
- **Ask loop v1** — landed: `relay-ask` crate, `ask-bubble` plugin,
  `/ask` + `/asks-for` slash commands, journal kinds extended.
  Spec: `docs/superpowers/specs/2026-04-25-relay-ask-loop-design.md`.
  Plan: `docs/superpowers/plans/2026-04-25-relay-ask-loop.md`.
  Follow-up: drainer integration, relay compaction.
```

If `ROADMAP.md` has a more structured shape (e.g., milestones / sections), match it instead of pasting verbatim.

- [ ] **Step 3: Commit**

```bash
git add src/relay/ask/README.md ROADMAP.md
git commit -m "docs(ask): README + roadmap entry for ask loop v1"
```

---

## Verification

After all tasks complete, run from the repo root:

```bash
cargo check --workspace
cargo test  --workspace --all-features
```

Expected: green. New crate `relay-ask` and plugin `relay-ask-bubble`
appear in the workspace `cargo check` output. The end-to-end test
`loop_e2e::ask_appears_then_disappears_when_answered` passes.

## Out of scope for this plan

- Drainer integration — pending qtrace v1 build, separate plan.
- Compaction at the 30-day boundary into relay — separate plan.
- File `@score` reader (the `Weights::w_score` term degrades to a
  constant 50 in v1; ranking still works via tags + age).
- Executor pause/resume mechanics — v1 routes answers via journal;
  the executor is not yet a long-running daemon.
- Relay justification edges from resolved ask → goal → file thought.

These follow-ups will each get their own plan once this v1 loop is
exercising the bubble + kern surface in real sessions.
