use std::path::PathBuf;
use std::time::SystemTime;

/// Kind of filesystem change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchKind {
	Created,
	Modified,
	Deleted,
	Renamed { from: PathBuf, to: PathBuf },
}

/// Single coalesced filesystem event emitted by [`crate::FileWatcher`].
///
/// `path` is the canonical path the event concerns. For `Renamed`, `path`
/// equals `to` (the new location); the rename payload also carries `from`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchEvent {
	pub path: PathBuf,
	pub kind: WatchKind,
	pub ts: SystemTime,
}
