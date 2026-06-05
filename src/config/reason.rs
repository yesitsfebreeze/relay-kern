use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReasonConfig {
	pub url: String,
	pub model: String,
	pub key: String,
}

/// Default reasoning endpoint — the same local Ollama that serves embeddings
/// ([`crate::config::DEFAULT_EMBED_URL`]). Empty-by-default broke the distill
/// and answer paths for any kern without an explicit `[reason] url`.
pub const DEFAULT_REASON_URL: &str = "http://localhost:11434";

impl Default for ReasonConfig {
	fn default() -> Self {
		Self {
			url: DEFAULT_REASON_URL.into(),
			model: "qwen2.5".into(),
			key: String::new(),
		}
	}
}
