//! Claude-Code capture spool.
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
pub fn drain_once(spool_dir: &Path, llm: &dyn Fn(&str) -> String, sink: &dyn Fn(Claim, &str)) {
	let done = spool_dir.join("done");
	let entries = match std::fs::read_dir(spool_dir) {
		Ok(e) => e,
		Err(e) => {
			tracing::warn!(target: "kern.capture_spool", dir = %spool_dir.display(), error = %e, "failed to read spool dir");
			return;
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
		consume_file(&path, &done, llm, sink);
	}
}

/// Process one delta file: distill, emit claims to `sink`, archive. Returns
/// the number of claims emitted.
pub fn consume_file(
	path: &Path,
	done_dir: &Path,
	llm: &dyn Fn(&str) -> String,
	sink: &dyn Fn(Claim, &str),
) -> usize {
	let text = match std::fs::read_to_string(path) {
		Ok(t) => t,
		Err(e) => {
			tracing::warn!(target: "kern.capture_spool", path = %path.display(), error = %e, "failed to read delta; leaving in spool");
			return 0;
		}
	};
	let stem = path
		.file_stem()
		.and_then(|s| s.to_str())
		.unwrap_or("claude")
		.to_string();
	let claims = distill(&text, llm);
	let n = claims.len();
	for c in claims {
		sink(c, &stem);
	}
	archive(path, done_dir);
	n
}

fn archive(path: &Path, done_dir: &Path) {
	let _ = std::fs::create_dir_all(done_dir);
	if let Some(name) = path.file_name() {
		if std::fs::rename(path, done_dir.join(name)).is_err() {
			// Best effort: if rename fails (e.g. cross-device), drop the file
			// so it is not re-processed on the next drain.
			let _ = std::fs::remove_file(path);
		}
	}
}

/// Daemon loop. Polls `spool_dir` every `interval`, enqueueing every claim
/// through `worker`. Runs until the task is aborted on shutdown.
pub async fn run(
	spool_dir: PathBuf,
	worker: Arc<Worker>,
	llm: LlmFunc,
	dedup_threshold: f64,
	interval: Duration,
) {
	let _ = std::fs::create_dir_all(&spool_dir);
	let cfg = crate::ingest::Config {
		dedup_threshold,
		..Default::default()
	};
	loop {
		tokio::time::sleep(interval).await;
		let llm_ref = llm.as_ref();
		let sink = |c: Claim, stem: &str| {
			let src = Source::Session {
				session_id: format!("claude:{stem}"),
				section: String::new(),
				title: format!("claude://{}", c.descriptor),
			};
			worker.enqueue(
				c.text,
				src,
				EntityKind::Claim,
				c.descriptor,
				0.6,
				cfg.clone(),
			);
		};
		drain_once(&spool_dir, llm_ref, &sink);
	}
}

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
		let n = consume_file(&delta, &done, &stub_two, &|c, _stem| {
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

		let n = consume_file(&delta, &done, &|_q| "[]".to_string(), &|_c, _stem| {});
		assert_eq!(n, 0);
		assert!(done.join("sess-2.txt").exists());
	}

	#[test]
	fn drain_skips_non_txt_and_done_dir() {
		let dir = tempdir().unwrap();
		let spool = dir.path().to_path_buf();
		std::fs::write(spool.join("keep.txt"), "user: remember x").unwrap();
		std::fs::write(spool.join("ignore.md"), "not a delta").unwrap();
		let count = std::sync::Mutex::new(0usize);
		drain_once(&spool, &stub_two, &|_c, _stem| { *count.lock().unwrap() += 1; });
		// keep.txt -> 2 claims; ignore.md skipped
		assert_eq!(*count.lock().unwrap(), 2);
		assert!(spool.join("done").join("keep.txt").exists());
		assert!(spool.join("ignore.md").exists(), "non-txt left untouched");
	}
}
