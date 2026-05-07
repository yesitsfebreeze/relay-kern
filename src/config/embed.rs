use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EmbedConfig {
	pub url: String,
	pub model: String,
	pub key: String,
}

impl Default for EmbedConfig {
	fn default() -> Self {
		Self {
			url: "http://localhost:11434".into(),
			model: "nomic-embed-text".into(),
			key: String::new(),
		}
	}
}
