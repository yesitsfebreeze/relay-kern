use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct JournalConfig {
	/// Number of days of history.db rows to keep. Older rows are pruned
	/// at kern startup. `0` disables pruning.
	pub retain_days: u32,
	/// Soft cap on today.jsonl size in bytes before forcing a mid-day
	/// rollover (current contents flushed into history.db, file rewritten).
	/// `0` disables the cap. Default 50 MiB.
	pub max_today_bytes: u64,
}

impl Default for JournalConfig {
	fn default() -> Self {
		Self {
			retain_days: 30,
			max_today_bytes: 50 * 1024 * 1024,
		}
	}
}
