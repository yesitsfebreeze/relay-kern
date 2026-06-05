//! Shared append-only JSONL journal — used by every binary (repl, agnt,
//! kern, plugins) so events from each process land in the same file.
//! Originally lived in `agnt/src/journal/`; relay-specific consumers
//! (`relay_sink`, `recipe::trace`) stay in agnt and depend on this crate.

pub mod day_journal;
pub mod entry;
pub mod events;
pub mod history;
pub mod state;
pub mod tracing_layer;
pub use events::{
	EntityTouchedEvent, EntityTouchedPayload, FinalEvent, FinalPayload, PlanEvent,
	PlanProposalPayload, PlanStatus, PlanStepPayload, ToolCallEvent, ToolCallPayload, TouchOp,
	TurnEndEvent, TurnEndPayload, TurnStartEvent, TurnStartPayload, system_time_from_ms,
};
pub use tracing_layer::{FieldRecorder, JournalTracingLayer};

pub use day_journal::{DayJournal, HistorySink, NullHistorySink};

/// Open the workspace-default journal: `<cwd>/.relay/journal/today.jsonl`
/// with a `NullHistorySink` (no warm SQLite store). Use this from any
/// binary that wants to emit cross-process events into the shared file.
pub fn open_default() -> std::io::Result<DayJournal> {
	let cwd = std::env::current_dir()?;
	DayJournal::open(&cwd, std::sync::Arc::new(NullHistorySink))
}

use std::sync::OnceLock;

static GLOBAL: OnceLock<DayJournal> = OnceLock::new();

/// Lazily open and return the process-global journal handle. Subsequent
/// calls reuse the same `DayJournal`. Returns `None` if open failed
/// (e.g. cwd has no `.relay`); callers should treat journaling as best-
/// effort and silently no-op on failure.
pub fn global() -> Option<&'static DayJournal> {
	if let Some(j) = GLOBAL.get() {
		return Some(j);
	}
	match open_default() {
		Ok(j) => Some(GLOBAL.get_or_init(|| j)),
		Err(_) => None,
	}
}

/// Convenience: emit a single entry via the global journal. No-op on open
/// failure.
pub fn emit(entry: Entry) {
		if let Some(j) = global() {
			j.emit(entry);
		}
}

/// `Sink` that forwards every emit to `global()`. Plug into anything that
/// already takes `Arc<dyn Sink>` to land its events in the shared journal.
pub struct GlobalSink;

impl Sink for GlobalSink {
	fn emit(&self, entry: Entry) {
		if let Some(j) = global() {
			j.emit(entry);
		}
}
}
pub use entry::{Entry, Kind, NullSink, Sink, SCHEMA_VERSION, now_ms};
pub use history::{Filter, History};
pub use state::{State, StateHandle};

#[derive(Debug)]
pub enum JournalError {
	Io(std::io::Error),
	Parse {
		line: usize,
		source: serde_json::Error,
	},
}

impl std::fmt::Display for JournalError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Io(e) => write!(f, "journal io error: {e}"),
			Self::Parse { line, source } if *line > 0 => {
				write!(f, "journal parse error on line {line}: {source}")
			}
			Self::Parse { source, .. } => write!(f, "journal serialise error: {source}"),
		}
}
}

impl std::error::Error for JournalError {}

impl From<std::io::Error> for JournalError {
	fn from(e: std::io::Error) -> Self {
		Self::Io(e)
}
}
