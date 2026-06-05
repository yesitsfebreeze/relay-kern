//! Detached cold storage for evicted thoughts.
//!
//! Stigmergy GC spills cold, abandoned, non-durable entities here before
//! dropping them from the hot graph, so eviction does not lose data
//! immediately. The cold tier is bounded: `compact` keeps the newest
//! `COLD_MAX_ENTRIES` and drops the oldest, so data is retained until that cap
//! is exceeded. The store is an append-only JSONL file under `<data_dir>/cold/`;
//! the latest
//! line for an id wins on read (so a re-spilled, merged entity supersedes
//! an older copy). Retrieval rehydrates by id on demand (lazy-link).

use std::path::{Path, PathBuf};

use crate::base::types::Entity;
use crate::base::util::cmp_partial;

fn store_path(cold_dir: &Path) -> PathBuf {
	cold_dir.join("cold.jsonl")
}

/// Append `entity` to the cold store. Best-effort: creates the dir, ignores
/// write errors (a failed spill must not crash GC).
pub fn spill(cold_dir: &Path, entity: &Entity) {
	let _ = std::fs::create_dir_all(cold_dir);
	let line = match serde_json::to_string(entity) {
		Ok(s) => s,
		Err(_) => return,
	};
	use std::io::Write;
	match std::fs::OpenOptions::new()
		.create(true)
		.append(true)
		.open(store_path(cold_dir))
	{
		Ok(mut f) => {
			if writeln!(f, "{line}").is_err() {
				tracing::warn!(target: "kern.cold", "spill failed; entity not persisted to cold store");
			}
		}
		Err(_) => {
			tracing::warn!(target: "kern.cold", "spill failed; entity not persisted to cold store");
		}
	}
}

/// Compact the cold store only if it has grown past
/// `COLD_COMPACT_MIN_BYTES`. Compaction rewrites the whole file (O(total)),
/// so this gate keeps steady-state GC from rewriting the entire store every
/// sweep when only a few victims were spilled; because compaction shrinks the
/// file, it self-rate-limits. Reads remain correct meanwhile (latest-line-wins
/// is resolved in memory by [`load_all`]). Best-effort.
pub fn maybe_compact(cold_dir: &Path) {
	maybe_compact_with(
		cold_dir,
		crate::base::constants::COLD_COMPACT_MIN_BYTES,
		crate::base::constants::COLD_MAX_ENTRIES,
	);
}

fn maybe_compact_with(cold_dir: &Path, min_bytes: u64, max_entries: usize) {
	let size = std::fs::metadata(store_path(cold_dir))
		.map(|m| m.len())
		.unwrap_or(0);
	if size < min_bytes {
		return;
	}
	compact_capped(cold_dir, max_entries);
}

/// Rewrite the cold store keeping only the latest entry per id, bounding
/// file growth. Best-effort; a failure leaves the existing file intact.
pub fn compact(cold_dir: &Path) {
	compact_capped(cold_dir, crate::base::constants::COLD_MAX_ENTRIES);
}

/// Rewrite the cold store keeping only the latest entry per id, then cap the
/// tier at `max_entries`, dropping the oldest (by `created_at`; entries with no
/// timestamp sort oldest and are evicted first). Bounds cold-tier growth over
/// the daemon's lifetime. Best-effort; failure leaves the file intact.
fn compact_capped(cold_dir: &Path, max_entries: usize) {
	let mut entities = load_all(cold_dir);
	if entities.is_empty() {
		return;
	}
	if entities.len() > max_entries {
		// Newest first (None sorts last = oldest), then keep the head.
		entities.sort_by(|a, b| b.created_at.cmp(&a.created_at));
		entities.truncate(max_entries);
	}
	let tmp = cold_dir.join("cold.jsonl.tmp");
	let mut buf = String::new();
	for e in &entities {
		if let Ok(line) = serde_json::to_string(e) {
			buf.push_str(&line);
			buf.push('\n');
		}
	}
	if std::fs::write(&tmp, buf).is_ok() {
		let _ = std::fs::rename(&tmp, store_path(cold_dir));
	}
}

/// Load every entity from the cold store, latest-line-wins per id.
pub fn load_all(cold_dir: &Path) -> Vec<Entity> {
	let text = match std::fs::read_to_string(store_path(cold_dir)) {
		Ok(t) => t,
		Err(_) => return Vec::new(),
	};
	let mut by_id: std::collections::HashMap<String, Entity> = std::collections::HashMap::new();
	for line in text.lines() {
		if line.trim().is_empty() {
			continue;
		}
		if let Ok(e) = serde_json::from_str::<Entity>(line) {
			by_id.insert(e.id.clone(), e);
		}
	}
	by_id.into_values().collect()
}

/// Fetch one entity from the cold store by id (latest wins). None if absent.
pub fn get(cold_dir: &Path, id: &str) -> Option<Entity> {
	load_all(cold_dir).into_iter().find(|e| e.id == id)
}

/// Vector search over the cold store. Returns up to `k` entities with the
/// highest cosine similarity to `query_vec` (descending), skipping entities
/// whose stored vector is empty or a different dimension. Read-only.
///
/// Scores against a lightweight `{id, vector}` projection of each line (so the
/// full `Entity` — text, metadata, gnn vector — is decoded only for the `k`
/// survivors, not every row) and keeps latest-line-wins per id by storing a
/// borrow into the file buffer rather than a materialized struct.
pub fn search(cold_dir: &Path, query_vec: &[f64], k: usize) -> Vec<(Entity, f64)> {
	if query_vec.is_empty() || k == 0 {
		return Vec::new();
	}
	let text = match std::fs::read_to_string(store_path(cold_dir)) {
		Ok(t) => t,
		Err(_) => return Vec::new(),
	};

	/// Minimal projection of a cold line: enough to score and to keep
	/// latest-wins, without decoding the rest of the `Entity`.
	#[derive(serde::Deserialize)]
	struct ColdVec {
		id: String,
		#[serde(default)]
		vector: Vec<f64>,
	}

	// Latest line per id wins. A wrong-dimension latest line still supersedes
	// (matching `load_all` semantics) but scores `-inf` so it is excluded.
	let mut latest: std::collections::HashMap<String, (f64, &str)> =
		std::collections::HashMap::new();
	for line in text.lines() {
		if line.trim().is_empty() {
			continue;
		}
		let Ok(cv) = serde_json::from_str::<ColdVec>(line) else {
			continue;
		};
		let score = if cv.vector.len() == query_vec.len() {
			crate::base::math::cosine(query_vec, &cv.vector)
		} else {
			f64::NEG_INFINITY
		};
		latest.insert(cv.id, (score, line));
	}

	let mut top: Vec<(f64, &str)> = latest
		.into_values()
		.filter(|(s, _)| s.is_finite())
		.collect();
	top.sort_by(|a, b| cmp_partial(&b.0, &a.0));
	top.truncate(k);

	// Decode the full Entity only for the survivors.
	top.into_iter()
		.filter_map(|(s, line)| serde_json::from_str::<Entity>(line).ok().map(|e| (e, s)))
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::base::types::{mk_entity, EntityKind};

	#[test]
	fn spill_then_get_roundtrips() {
		let dir = tempfile::tempdir().unwrap();
		let e = mk_entity("a", "hello cold", 0.0, EntityKind::Claim);
		spill(dir.path(), &e);
		let got = get(dir.path(), "a").expect("entity should be in cold store");
		assert_eq!(got.id, "a");
		assert_eq!(got.text(), "hello cold");
	}

	#[test]
	fn latest_spill_wins() {
		let dir = tempfile::tempdir().unwrap();
		spill(dir.path(), &mk_entity("x", "v1", 1.0, EntityKind::Claim));
		spill(dir.path(), &mk_entity("x", "v2", 5.0, EntityKind::Claim));
		let got = get(dir.path(), "x").expect("entity should be present");
		assert_eq!(got.heat, 5.0);
		let all = load_all(dir.path());
		assert_eq!(all.iter().filter(|e| e.id == "x").count(), 1);
	}

	#[test]
	fn get_absent_is_none() {
		let dir = tempfile::tempdir().unwrap();
		assert!(get(dir.path(), "missing").is_none());
	}

	#[test]
	fn search_ranks_by_cosine() {
		let dir = tempfile::tempdir().unwrap();
		let mut ex = mk_entity("ex", "x axis", 0.0, EntityKind::Claim);
		ex.vector = vec![1.0, 0.0];
		let mut ey = mk_entity("ey", "y axis", 0.0, EntityKind::Claim);
		ey.vector = vec![0.0, 1.0];
		let mut enear = mk_entity("enear", "near x", 0.0, EntityKind::Claim);
		enear.vector = vec![0.9, 0.1];
		spill(dir.path(), &ex);
		spill(dir.path(), &ey);
		spill(dir.path(), &enear);

		let hits = search(dir.path(), &[1.0, 0.0], 2);
		assert_eq!(hits.len(), 2);
		assert_eq!(hits[0].0.id, "ex");

		// Dimension mismatch yields no results.
		let none = search(dir.path(), &[1.0, 0.0, 0.0], 2);
		assert!(none.is_empty());
	}

	#[test]
	fn search_uses_latest_spilled_vector() {
		let dir = tempfile::tempdir().unwrap();
		// First spill points away from the query...
		let mut v1 = mk_entity("z", "v1", 0.0, EntityKind::Claim);
		v1.vector = vec![0.0, 1.0];
		spill(dir.path(), &v1);
		// ...re-spill flips it to align with the query. Latest wins.
		let mut v2 = mk_entity("z", "v2", 0.0, EntityKind::Claim);
		v2.vector = vec![1.0, 0.0];
		spill(dir.path(), &v2);

		let hits = search(dir.path(), &[1.0, 0.0], 5);
		assert_eq!(hits.len(), 1, "one id, latest-wins");
		assert_eq!(hits[0].0.id, "z");
		assert_eq!(hits[0].0.text(), "v2");
		assert!(hits[0].1 > 0.99, "scored against the latest (aligned) vector");
	}

	#[test]
	fn search_truncates_to_k() {
		let dir = tempfile::tempdir().unwrap();
		for i in 0..5 {
			let mut e = mk_entity(&format!("e{i}"), "t", 0.0, EntityKind::Claim);
			e.vector = vec![1.0, i as f64 / 10.0];
			spill(dir.path(), &e);
		}
		assert_eq!(search(dir.path(), &[1.0, 0.0], 2).len(), 2);
	}

	fn count_lines(dir: &Path) -> usize {
		std::fs::read_to_string(store_path(dir))
			.map(|t| t.lines().filter(|l| !l.trim().is_empty()).count())
			.unwrap_or(0)
	}

	#[test]
	fn maybe_compact_skips_below_size_threshold() {
		let dir = tempfile::tempdir().unwrap();
		// Two lines for the same id: a real compaction would dedup to 1.
		spill(dir.path(), &mk_entity("x", "v1", 1.0, EntityKind::Claim));
		spill(dir.path(), &mk_entity("x", "v2", 2.0, EntityKind::Claim));
		assert_eq!(count_lines(dir.path()), 2);
		// Huge min_bytes → below threshold → no rewrite (duplicate line kept).
		maybe_compact_with(dir.path(), u64::MAX, 50_000);
		assert_eq!(count_lines(dir.path()), 2, "no rewrite below threshold");
		// Reads still resolve latest-wins despite the un-compacted duplicate.
		assert_eq!(get(dir.path(), "x").unwrap().heat, 2.0);
	}

	#[test]
	fn maybe_compact_runs_above_threshold() {
		let dir = tempfile::tempdir().unwrap();
		spill(dir.path(), &mk_entity("x", "v1", 1.0, EntityKind::Claim));
		spill(dir.path(), &mk_entity("x", "v2", 2.0, EntityKind::Claim));
		// min_bytes 0 → always compacts → dedups to 1 line.
		maybe_compact_with(dir.path(), 0, 50_000);
		assert_eq!(count_lines(dir.path()), 1, "compacted above threshold");
	}

	#[test]
	fn compact_caps_tier_dropping_oldest() {
		use std::time::{Duration, UNIX_EPOCH};
		let dir = tempfile::tempdir().unwrap();
		// Three distinct ids with increasing created_at; cap=2 keeps newest 2.
		for (i, id) in ["old", "mid", "new"].iter().enumerate() {
			let mut e = mk_entity(id, id, 1.0, EntityKind::Claim);
			e.created_at = Some(UNIX_EPOCH + Duration::from_secs(100 * (i as u64 + 1)));
			spill(dir.path(), &e);
		}
		maybe_compact_with(dir.path(), 0, 2);
		assert_eq!(count_lines(dir.path()), 2, "tier capped at 2");
		assert!(get(dir.path(), "new").is_some(), "newest kept");
		assert!(get(dir.path(), "mid").is_some(), "second-newest kept");
		assert!(get(dir.path(), "old").is_none(), "oldest evicted");
	}

	#[test]
	fn compact_dedups_to_latest() {
		let dir = tempfile::tempdir().unwrap();
		spill(dir.path(), &mk_entity("x", "v1", 1.0, EntityKind::Claim));
		spill(dir.path(), &mk_entity("x", "v2", 3.0, EntityKind::Claim));
		spill(dir.path(), &mk_entity("x", "v3", 5.0, EntityKind::Claim));
		spill(dir.path(), &mk_entity("y", "y1", 1.0, EntityKind::Claim));

		compact(dir.path());

		let raw = std::fs::read_to_string(store_path(dir.path())).unwrap();
		let lines = raw.lines().filter(|l| !l.trim().is_empty()).count();
		assert_eq!(lines, 2);
		assert_eq!(get(dir.path(), "x").unwrap().heat, 5.0);
	}
}
