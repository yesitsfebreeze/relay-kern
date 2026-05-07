use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct TickConfig {
	pub unnamed_stall_threshold: usize,
	pub max_cluster_sample: usize,
	pub queue_capacity: usize,
}

impl Default for TickConfig {
	fn default() -> Self {
		Self {
			unnamed_stall_threshold: 10,
			max_cluster_sample: 200,
			queue_capacity: 512,
		}
	}
}
