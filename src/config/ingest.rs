use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct IngestConfig {
	pub dedup_threshold: f64,
	pub hnsw_k: usize,
	pub hnsw_ef: usize,
	pub rephrase_lower: f64,
	pub rephrase_upper: f64,
	/// Max number of `fork_id`s remembered in the session-mirror dedup set
	/// before FIFO eviction kicks in. Bounds memory under long-running
	/// daemons that accumulate many forks.
	pub session_mirror_max_seen: usize,
}

impl Default for IngestConfig {
	fn default() -> Self {
		Self {
			dedup_threshold: 0.95,
			hnsw_k: 8,
			hnsw_ef: 32,
			rephrase_lower: 0.85,
			rephrase_upper: 0.95,
			session_mirror_max_seen: 4096,
		}
	}
}
