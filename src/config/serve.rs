use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServeConfig {
	pub addr: String,
	pub core_addr: String,
	pub gossip: String,
	pub mcp_sse: String,
}

impl Default for ServeConfig {
	fn default() -> Self {
		Self {
			addr: ":8080".into(),
			core_addr: ":2666".into(),
			gossip: ":7946".into(),
			mcp_sse: ":3000".into(),
		}
	}
}
