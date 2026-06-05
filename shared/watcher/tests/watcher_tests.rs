//! Integration tests for the watcher crate.
//!
//! Filesystem-event tests are inherently racy on Windows; we use generous
//! timeouts and tolerate platform variation in *which* events fire (e.g.
//! Windows often reports a Modified before a Created on first write) by
//! asserting on observed *kinds* across a window rather than exact ordering.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tempfile::TempDir;
use tokio::sync::Mutex;

use watcher::{
	FileWatcher, IgnoreRules, IngestPipeline, IngestRecord, IngestSink, WatchEvent, WatchKind,
};

const POLL_BUDGET: Duration = Duration::from_secs(5);

async fn collect_events(w: &mut FileWatcher, budget: Duration) -> Vec<WatchEvent> {
	let mut out = Vec::new();
	let deadline = tokio::time::Instant::now() + budget;
	loop {
		let now = tokio::time::Instant::now();
		if now >= deadline {
			break;
		}
		match tokio::time::timeout(deadline - now, w.next_event()).await {
			Ok(Some(ev)) => out.push(ev),
			Ok(None) => break,
			Err(_) => break,
		}
	}
	out
}

fn has_kind(events: &[WatchEvent], path: &PathBuf, want: fn(&WatchKind) -> bool) -> bool {
	events.iter().any(|e| &e.path == path && want(&e.kind))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn create_modify_delete_cycle_emits_expected_events() {
	let tmp = TempDir::new().unwrap();
	let root = tmp.path().to_path_buf();
	let mut w = FileWatcher::new(vec![root.clone()], IgnoreRules::empty()).unwrap();

	// Give notify a moment to install its OS-level hook before we mutate.
	tokio::time::sleep(Duration::from_millis(100)).await;

	let file = root.join("a.txt");
	tokio::fs::write(&file, b"hello").await.unwrap();
	tokio::time::sleep(Duration::from_millis(150)).await;
	tokio::fs::write(&file, b"world").await.unwrap();
	tokio::time::sleep(Duration::from_millis(150)).await;
	tokio::fs::remove_file(&file).await.unwrap();

	let events = collect_events(&mut w, POLL_BUDGET).await;

	// We require *at least* a Created (or Modified — Windows often collapses
	// the initial write into Modified) and a Deleted. The platform decides
	// whether the second write becomes a separate Modified or is folded
	// into the first event by debouncing.
	let created_or_modified = has_kind(&events, &file, |k| {
		matches!(k, WatchKind::Created | WatchKind::Modified)
	});
	let deleted = has_kind(&events, &file, |k| matches!(k, WatchKind::Deleted));
	assert!(
		created_or_modified,
		"expected create/modify event for {file:?}, saw {events:?}"
	);
	assert!(deleted, "expected delete event for {file:?}, saw {events:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn debounce_collapses_rapid_modifies_to_one_event() {
	let tmp = TempDir::new().unwrap();
	let root = tmp.path().to_path_buf();
	let file = root.join("burst.txt");
	tokio::fs::write(&file, b"seed").await.unwrap();

	// Start watching *after* the seed write so the seed Created doesn't
	// pollute the count.
	let mut w = FileWatcher::new(vec![root.clone()], IgnoreRules::empty()).unwrap();
	tokio::time::sleep(Duration::from_millis(100)).await;

	// 5 modifies inside the 50 ms debounce window. We use `set_len` to bump
	// content cheaply; on Windows a write+close burst produces several raw
	// notify events per syscall — that's exactly what we want to coalesce.
	for i in 0..5u8 {
		tokio::fs::write(&file, [b'x', b'0' + i]).await.unwrap();
		tokio::time::sleep(Duration::from_millis(5)).await;
	}

	// Wait long enough for the debouncer to flush.
	tokio::time::sleep(Duration::from_millis(250)).await;

	let events = collect_events(&mut w, Duration::from_millis(500)).await;
	let for_file: Vec<_> = events.iter().filter(|e| e.path == file).collect();

	// Coalesced should be a single event for `file`. We allow up to 2 to
	// tolerate the case where the debouncer flushes mid-burst on slow CI.
	assert!(
		!for_file.is_empty() && for_file.len() <= 2,
		"expected 1-2 coalesced events for {file:?}, got {}: {events:?}",
		for_file.len()
	);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gitignore_is_respected() {
	let tmp = TempDir::new().unwrap();
	let root = tmp.path().to_path_buf();
	tokio::fs::write(root.join(".gitignore"), b"ignored.txt\n").await.unwrap();

	let mut w =
		FileWatcher::new(vec![root.clone()], IgnoreRules::from_roots(std::slice::from_ref(&root))).unwrap();
	tokio::time::sleep(Duration::from_millis(100)).await;

	let ignored = root.join("ignored.txt");
	let kept = root.join("kept.txt");
	tokio::fs::write(&ignored, b"x").await.unwrap();
	tokio::fs::write(&kept, b"x").await.unwrap();
	tokio::time::sleep(Duration::from_millis(250)).await;

	let events = collect_events(&mut w, Duration::from_millis(800)).await;

	assert!(
		events.iter().any(|e| e.path == kept),
		"expected event for kept.txt, got {events:?}"
	);
	assert!(
		!events.iter().any(|e| e.path == ignored),
		"did not expect event for ignored.txt, got {events:?}"
	);
}

// -- IngestPipeline tests --------------------------------------------------

#[derive(Default, Clone)]
struct CapturingSink {
	records: Arc<Mutex<Vec<IngestRecord>>>,
}

#[async_trait::async_trait]
impl IngestSink for CapturingSink {
	async fn ingest(&self, record: IngestRecord) {
		self.records.lock().await.push(record);
	}
}

#[tokio::test]
async fn pipeline_skips_files_over_one_megabyte() {
	let tmp = TempDir::new().unwrap();
	let root = tmp.path().to_path_buf();

	let big = root.join("big.bin");
	let big_bytes = vec![b'a'; (1024 * 1024) + 1];
	tokio::fs::write(&big, &big_bytes).await.unwrap();

	let small = root.join("small.txt");
	tokio::fs::write(&small, b"hi").await.unwrap();

	let sink = CapturingSink::default();
	let pipeline = IngestPipeline::new(sink.clone());

	pipeline
		.handle(WatchEvent {
			path: big.clone(),
			kind: WatchKind::Created,
			ts: SystemTime::now(),
		})
		.await;
	pipeline
		.handle(WatchEvent {
			path: small.clone(),
			kind: WatchKind::Modified,
			ts: SystemTime::now(),
		})
		.await;

	let recs = sink.records.lock().await.clone();
	assert_eq!(recs.len(), 1, "only the small file should ingest, got {recs:?}");
	assert!(recs[0].source_uri.starts_with("file://"));
	assert_eq!(recs[0].content, "hi");
	assert_eq!(recs[0].language_hint.as_deref(), Some("txt"));
}

#[tokio::test]
async fn pipeline_drops_delete_events() {
	let tmp = TempDir::new().unwrap();
	let path = tmp.path().join("gone.txt");

	let sink = CapturingSink::default();
	let pipeline = IngestPipeline::new(sink.clone());
	pipeline
		.handle(WatchEvent { path, kind: WatchKind::Deleted, ts: SystemTime::now() })
		.await;

	assert!(sink.records.lock().await.is_empty());
}
