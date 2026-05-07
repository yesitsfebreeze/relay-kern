#[derive(Debug, Clone)]
pub struct Config {
	pub dedup_threshold: f64,
	pub ttl_secs: Option<u64>,
	pub hnsw_k: usize,
	pub hnsw_ef: usize,
	pub rephrase_lower: f64,
	pub rephrase_upper: f64,
}

impl Default for Config {
	fn default() -> Self {
		Self {
			dedup_threshold: 0.95,
			ttl_secs: None,
			hnsw_k: 8,
			hnsw_ef: 32,
			rephrase_lower: 0.85,
			rephrase_upper: 0.95,
		}
	}
}
