use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceDoc {
	pub id: String,
	pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceQuery {
	pub id: String,
	pub query: String,
	pub expected_ids: Vec<String>,
	#[serde(default = "default_mode")]
	pub mode: String,
}

fn default_mode() -> String {
	"hybrid".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trace {
	pub name: String,
	pub docs: Vec<TraceDoc>,
	pub queries: Vec<TraceQuery>,
}

pub fn load<P: AsRef<Path>>(path: P) -> Result<Trace, TraceError> {
	let data = std::fs::read_to_string(path.as_ref())
		.map_err(|e| TraceError::Io(path.as_ref().display().to_string(), e))?;
	let trace: Trace = serde_json::from_str(&data)
		.map_err(|e| TraceError::Parse(path.as_ref().display().to_string(), e))?;
	Ok(trace)
}

#[derive(Debug, thiserror::Error)]
pub enum TraceError {
	#[error("failed to read trace {0}: {1}")]
	Io(String, #[source] std::io::Error),
	#[error("failed to parse trace {0}: {1}")]
	Parse(String, #[source] serde_json::Error),
}
