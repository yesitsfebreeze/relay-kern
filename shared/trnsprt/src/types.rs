//! Tool schema, tool result, and server-id value types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
	pub name: String,
	#[serde(default)]
	pub description: Option<String>,
	#[serde(default, rename = "inputSchema")]
	pub input_schema: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
	#[serde(default)]
	pub content: Vec<Value>,
	#[serde(default, rename = "isError")]
	pub is_error: bool,
	#[serde(default, skip_serializing_if = "Option::is_none", rename = "structuredContent")]
	pub structured_content: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ServerId(pub String);

impl ServerId {
	pub fn new<S: Into<String>>(id: S) -> Self {
		Self(id.into())
	}
}
