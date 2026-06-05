use std::path::Path;

use tokio::sync::mpsc;

use crate::event::{WatchEvent, WatchKind};

/// Hard cap on the size of files we read into an [`IngestRecord`]. Anything
/// larger is silently skipped — the search index is for source-shaped text,
/// not blobs.
pub const MAX_INGEST_BYTES: u64 = 1024 * 1024;

/// Payload handed to a downstream sink. `source_uri` is always a `file://`
/// URI built from the absolute path; kern's `ingest` MCP tool expects this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestRecord {
	pub source_uri: String,
	pub content: String,
	pub language_hint: Option<String>,
}

/// Downstream consumer. Implemented by the kern wiring (slice F) — this
/// crate must NOT depend on kern.
#[async_trait::async_trait]
pub trait IngestSink: Send + Sync + 'static {
	async fn ingest(&self, record: IngestRecord);
}

// We avoid pulling in `async-trait` as a workspace dep just for one trait —
// fall back to a hand-rolled boxed-future trait if the dep is unwelcome.
// (Pre-emptive note: if reviewers object to `async-trait`, swap to a
// `Pin<Box<dyn Future>>` returning method.)

/// Drives a stream of [`WatchEvent`]s into an [`IngestSink`].
///
/// * `Created` / `Modified` → read file (≤ [`MAX_INGEST_BYTES`]) → ingest.
/// * `Deleted` → no read; ignored at this layer (kern handles deletion via
///   a separate call path; slice E only does *content* ingest).
/// * `Renamed { from, to }` → treated as `Created(to)`.
/// * Files larger than `MAX_INGEST_BYTES` or with non-UTF-8 content are
///   skipped silently (logged at `debug`).
pub struct IngestPipeline<S: IngestSink> {
	sink: S,
}

impl<S: IngestSink> IngestPipeline<S> {
	pub fn new(sink: S) -> Self {
		Self { sink }
	}

	/// Consume events from `rx` until the channel closes.
	pub async fn run(self, mut rx: mpsc::UnboundedReceiver<WatchEvent>) {
		while let Some(ev) = rx.recv().await {
			if let Some(rec) = build_record(&ev).await {
				self.sink.ingest(rec).await;
			}
		}
	}

	/// Process a single event. Exposed for tests and synchronous callers.
	pub async fn handle(&self, ev: WatchEvent) {
		if let Some(rec) = build_record(&ev).await {
			self.sink.ingest(rec).await;
		}
	}
}

async fn build_record(ev: &WatchEvent) -> Option<IngestRecord> {
	let path: &Path = match &ev.kind {
		WatchKind::Created | WatchKind::Modified => &ev.path,
		WatchKind::Renamed { to, .. } => to,
		WatchKind::Deleted => return None,
	};

	let meta = tokio::fs::metadata(path).await.ok()?;
	if !meta.is_file() {
		return None;
	}
	if meta.len() > MAX_INGEST_BYTES {
		tracing::debug!(?path, size = meta.len(), "skipping oversize file");
		return None;
	}
	let bytes = tokio::fs::read(path).await.ok()?;
	let content = match String::from_utf8(bytes) {
		Ok(s) => s,
		Err(_) => {
			tracing::debug!(?path, "skipping non-utf8 file");
			return None;
		}
	};

	Some(IngestRecord {
		source_uri: file_uri(path),
		content,
		language_hint: language_hint(path),
	})
}

fn file_uri(path: &Path) -> String {
	let abs = match path.canonicalize() {
		Ok(p) => p,
		Err(_) => path.to_path_buf(),
	};
	let s = abs.to_string_lossy().replace('\\', "/");
	// Windows canonical paths come back as `\\?\C:\foo`; normalise.
	let trimmed = s.strip_prefix("//?/").unwrap_or(&s);
	if trimmed.starts_with('/') {
		format!("file://{trimmed}")
	} else {
		format!("file:///{trimmed}")
	}
}

fn language_hint(path: &Path) -> Option<String> {
	let ext = path.extension()?.to_str()?.to_ascii_lowercase();
	let hint = match ext.as_str() {
		"rs" => "rust",
		"ts" | "tsx" => "typescript",
		"js" | "jsx" | "mjs" | "cjs" => "javascript",
		"py" => "python",
		"go" => "go",
		"md" => "markdown",
		"toml" => "toml",
		"json" => "json",
		"yaml" | "yml" => "yaml",
		_ => return Some(ext),
	};
	Some(hint.to_string())
}
