use thiserror::Error;

#[derive(Debug, Error)]
pub enum McpError {
	#[error("mcp transport i/o: {0}")]
	Io(#[from] std::io::Error),
	#[error("mcp protocol: {0}")]
	Protocol(String),
	#[error("mcp json: {0}")]
	Json(#[from] serde_json::Error),
	#[error("mcp rpc error {code}: {message}")]
	Rpc { code: i64, message: String },
	#[error("unknown mcp server: {0}")]
	UnknownServer(String),
	#[error("mcp server already registered: {0}")]
	DuplicateServer(String),
	#[error("mcp child process not running")]
	NotRunning,
}
