//! Claude-Code capture spool.
//!
//! The CC `Stop` hook drops plain-text conversation deltas into the spool
//! directory. This task drains them: each delta is distilled into durable
//! `Claim`s (LLM), each claim is ingested through the canonical `Worker`,
//! and the consumed file is archived to `<spool>/done/` — but ONLY once every
//! claim has ingested successfully. If ingest fails (e.g. the embed endpoint
//! is down) the delta is left in the spool and retried on the next drain, so
//! a transient LLM outage never loses captured knowledge.
//!
//! The daemon is the single graph owner, so ingest happens in-process with
//! no CLI race.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::base::types::{EntityKind, Source};
use crate::ingest::distill::{distill, Claim};
use crate::ingest::outcome::OutcomeStatus;
use crate::ingest::Worker;
use crate::types::LlmFunc;

/// Read + distill one delta file into `(stem, claims)`. The stem is the file
/// name without extension (used as the session id for provenance). Returns
/// `None` on read failure (the file is left in place for retry).
pub fn extract_claims(path: &Path, llm: &dyn Fn(&str) -> String) -> Option<(String, Vec<Claim>)> {
	let text = match std::fs::read_to_string(path) {
		Ok(t) => t,
		Err(e) => {
			tracing::warn!(target: "kern.capture_spool", path = %path.display(), error = %e, "failed to read delta; leaving in spool");
			return None;
		}
	};
	let stem = path
		.file_stem()
		.and_then(|s| s.to_str())
		.unwrap_or("claude")
		.to_string();
	Some((stem, distill(&text, llm)))
}

/// Move a fully-ingested delta into `<done>/`. Best effort: on rename failure
/// (e.g. cross-device) the source is removed so it is not re-processed.
pub fn archive(path: &Path, done_dir: &Path) {
	let _ = std::fs::create_dir_all(done_dir);
	if let Some(name) = path.file_name() {
		if std::fs::rename(path, done_dir.join(name)).is_err() {
			let _ = std::fs::remove_file(path);
		}
	}
}

/// Archive `path` iff every claim ingested successfully (`results` all true),
/// or there were no claims at all. Returns whether the file was archived.
/// A delta with any failed claim is left in the spool for a later retry.
pub fn finalize(path: &Path, done_dir: &Path, results: &[bool]) -> bool {
	if results.iter().all(|&ok| ok) {
		archive(path, done_dir);
		true
	} else {
		false
	}
}

/// Daemon loop. Polls `spool_dir` every `interval`. For each `*.txt` delta:
/// distill, ingest each claim through `worker` (awaiting the outcome), and
/// archive the file only if all claims committed.
pub async fn run(
	spool_dir: PathBuf,
	worker: Arc<Worker>,
	llm: LlmFunc,
	dedup_threshold: f64,
	interval: Duration,
) {
	let _ = std::fs::create_dir_all(&spool_dir);
	let done = spool_dir.join("done");
	let cfg = crate::ingest::Config {
		dedup_threshold,
		..Default::default()
	};
	loop {
		tokio::time::sleep(interval).await;
		let entries = match std::fs::read_dir(&spool_dir) {
			Ok(e) => e,
			Err(e) => {
				tracing::warn!(target: "kern.capture_spool", dir = %spool_dir.display(), error = %e, "failed to read spool dir");
				continue;
			}
		};
		for ent in entries.flatten() {
			let path = ent.path();
			if !path.is_file() {
				continue;
			}
			if path.extension().and_then(|s| s.to_str()) != Some("txt") {
				continue;
			}
			let (stem, claims) = match extract_claims(&path, llm.as_ref()) {
				Some(v) => v,
				None => continue,
			};
			let mut results = Vec::with_capacity(claims.len());
			for c in claims {
				let src = Source::Session {
					session_id: format!("claude:{stem}"),
					section: String::new(),
					title: format!("claude://{}", c.descriptor),
				};
				let outcome = worker
					.run(c.text, src, EntityKind::Claim, c.descriptor, 0.6, cfg.clone())
					.await;
				let ok = !matches!(outcome.status, OutcomeStatus::Failed);
				if !ok {
					tracing::warn!(target: "kern.capture_spool", stem = %stem, status = outcome.status.as_str(), "claim ingest failed; leaving delta for retry");
				}
				results.push(ok);
			}
			finalize(&path, &done, &results);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use tempfile::tempdir;

	fn stub_two(_q: &str) -> String {
		r#"[{"text":"fact one","kind":"fact"},{"text":"a preference","kind":"preference"}]"#
			.to_string()
	}

	#[test]
	fn extract_reads_and_distills() {
		let dir = tempdir().unwrap();
		let delta = dir.path().join("sess-1.txt");
		std::fs::write(&delta, "user: hi\nassistant: here is a fact").unwrap();
		let (stem, claims) = extract_claims(&delta, &stub_two).expect("some");
		assert_eq!(stem, "sess-1");
		assert_eq!(claims.len(), 2);
	}

	#[test]
	fn extract_missing_file_is_none() {
		let dir = tempdir().unwrap();
		let missing = dir.path().join("nope.txt");
		assert!(extract_claims(&missing, &stub_two).is_none());
	}

	#[test]
	fn finalize_archives_when_all_ok() {
		let dir = tempdir().unwrap();
		let spool = dir.path().to_path_buf();
		let done = spool.join("done");
		let delta = spool.join("sess-1.txt");
		std::fs::write(&delta, "x").unwrap();
		assert!(finalize(&delta, &done, &[true, true]));
		assert!(!delta.exists());
		assert!(done.join("sess-1.txt").exists());
	}

	#[test]
	fn finalize_archives_when_no_claims() {
		let dir = tempdir().unwrap();
		let spool = dir.path().to_path_buf();
		let done = spool.join("done");
		let delta = spool.join("sess-2.txt");
		std::fs::write(&delta, "x").unwrap();
		assert!(finalize(&delta, &done, &[]));
		assert!(done.join("sess-2.txt").exists());
	}

	#[test]
	fn finalize_skips_archive_when_any_fail() {
		let dir = tempdir().unwrap();
		let spool = dir.path().to_path_buf();
		let done = spool.join("done");
		let delta = spool.join("sess-3.txt");
		std::fs::write(&delta, "x").unwrap();
		assert!(!finalize(&delta, &done, &[true, false]));
		assert!(delta.exists(), "delta left in spool for retry");
		assert!(!done.join("sess-3.txt").exists());
	}
}
