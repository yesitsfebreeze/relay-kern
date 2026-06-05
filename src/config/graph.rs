use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphConfig {
	pub max_kerns: usize,
	pub max_ledger_entries: usize,
}

impl Default for GraphConfig {
	fn default() -> Self {
		Self {
			// Kern eviction is DISABLED by default (usize::MAX is the no-cap
			// sentinel honored by GraphGnn::enforce_kern_cap). A finite cap
			// currently corrupts the graph: evicting a parent kern can drop an
			// in-memory `children` push before it is persisted, so the unnamed-
			// child lookup re-spawns a fresh child every tick — a runaway that
			// fragments the graph to `max_kerns` near-empty kerns (observed:
			// 1024 kerns / 13 entities on a real graph). Re-enable a finite cap
			// only once the evict/persist consistency bug is fixed; bounded RAM
			// for huge corpora is the DiskANN index's job, not this cap.
			max_kerns: usize::MAX,
			max_ledger_entries: 10_000,
		}
	}
}
