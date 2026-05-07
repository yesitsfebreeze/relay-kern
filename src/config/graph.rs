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
			max_kerns: 1024,
			max_ledger_entries: 10_000,
		}
	}
}
