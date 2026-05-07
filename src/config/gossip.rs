use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct GossipConfig {
	pub ingest_clip_enabled: bool,
	pub ingest_clip_max_per_window: u64,
	pub ingest_clip_window_secs: u64,
	pub trimmed_mean_enabled: bool,
	pub trimmed_mean_trim_pct: f64,
}

impl Default for GossipConfig {
	fn default() -> Self {
		Self {
			ingest_clip_enabled: false,
			ingest_clip_max_per_window: 1000,
			ingest_clip_window_secs: 1,
			trimmed_mean_enabled: false,
			trimmed_mean_trim_pct: 0.10,
		}
	}
}
