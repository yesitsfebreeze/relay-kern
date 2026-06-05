use std::io::{self, BufRead, BufReader, Write};

use serde_json::{json, Value};

use crate::error::McpError;
use crate::types::{ToolResult, ToolSchema};
use crate::PROTOCOL_VERSION;

pub trait McpServer: Send {
	fn server_name(&self) -> &str {
		"inproc"
	}
	fn server_version(&self) -> &str {
		env!("CARGO_PKG_VERSION")
	}
	fn tools_list(&self) -> Vec<ToolSchema>;
	fn call_tool(&self, name: &str, args: &Value) -> Result<ToolResult, McpError>;
	/// Extra MCP capabilities beyond `tools` (e.g. `{"resources": {}, "prompts": {}}`).
	fn extra_capabilities(&self) -> Value {
		Value::Object(serde_json::Map::new())
	}
	/// Handle MCP methods not covered by the standard tool/init/shutdown dispatch.
	/// Return `None` to fall through to method-not-found.
	fn handle_method(&self, _method: &str, _params: Value) -> Option<Result<Value, McpError>> {
		None
	}
}

pub fn serve_stdio(server: &impl McpServer) -> io::Result<i32> {
	let stdin = io::stdin();
	let stdout = io::stdout();
	let mut reader = BufReader::new(stdin.lock());
	let mut writer = stdout.lock();
	serve_rw(&mut reader, &mut writer, server)
}

pub fn serve_rw<R, W>(reader: &mut R, writer: &mut W, server: &impl McpServer) -> io::Result<i32>
where
	R: BufRead,
	W: Write,
{
	let mut line = String::new();
	loop {
		line.clear();
		let n = reader.read_line(&mut line)?;
		if n == 0 {
			return Ok(0);
		}
		let trimmed = line.trim_end_matches(['\r', '\n']);
		if trimmed.is_empty() {
			continue;
		}
		let frame: Value = match serde_json::from_str(trimmed) {
			Ok(v) => v,
			Err(e) => {
				write_frame(
					writer,
					&error_response(Value::Null, -32700, &format!("parse error: {e}")),
				)?;
				continue;
			}
		};
		let is_shutdown = frame.get("method").and_then(Value::as_str) == Some("shutdown");
		if let Some(response) = dispatch(server, &frame) {
			write_frame(writer, &response)?;
		}
		if is_shutdown {
			return Ok(0);
		}
	}
}

pub(crate) fn dispatch(server: &dyn McpServer, frame: &Value) -> Option<Value> {
	let id = frame.get("id").cloned();
	let method = frame.get("method").and_then(Value::as_str).unwrap_or("");
	let params = frame.get("params").cloned().unwrap_or(Value::Null);
	let is_notification = id.is_none() || id.as_ref() == Some(&Value::Null);

	match method {
		"initialize" => {
			let mut caps = serde_json::Map::new();
			caps.insert("tools".to_string(), json!({}));
			if let Value::Object(extra) = server.extra_capabilities() {
				caps.extend(extra);
			}
			let reply = json!({
				"protocolVersion": PROTOCOL_VERSION,
				"capabilities": caps,
				"serverInfo": {
					"name": server.server_name(),
					"version": server.server_version(),
				},
			});
			id.map(|id| ok_response(id, reply))
		}
		"notifications/initialized" | "initialized" => None,
		"tools/list" => {
			if is_notification {
				return None;
			}
			let tools = server.tools_list();
			let list_val = serde_json::to_value(&tools).unwrap_or(Value::Array(vec![]));
			id.map(|id| ok_response(id, json!({ "tools": list_val })))
		}
		"tools/call" => {
			if is_notification {
				return None;
			}
			let name = params.get("name").and_then(Value::as_str).unwrap_or("");
			let args = params.get("arguments").cloned().unwrap_or(Value::Null);
			let result = server
				.call_tool(name, &args)
				.and_then(|r| serde_json::to_value(&r).map_err(McpError::Json));
			id.map(|id| match result {
				Ok(v) => ok_response(id, v),
				Err(e) => {
					let (code, message) = match e {
						McpError::Rpc { code, message } => (code, message),
						other => (-32000, other.to_string()),
					};
					error_response(id, code, &message)
				}
			})
		}
		"shutdown" => id.map(|id| ok_response(id, Value::Null)),
		_ => {
			if is_notification {
				return None;
			}
			match server.handle_method(method, params) {
				Some(Ok(v)) => id.map(|id| ok_response(id, v)),
				Some(Err(e)) => id.map(|id| {
					let (code, msg) = match e {
						McpError::Rpc { code, message } => (code, message),
						other => (-32000, other.to_string()),
					};
					error_response(id, code, &msg)
				}),
				None => id.map(|id| error_response(id, -32601, &format!("method not found: {method}"))),
			}
		}
	}
}

pub(crate) fn ok_response(id: Value, result: Value) -> Value {
	json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

pub(crate) fn error_response(id: Value, code: i64, message: &str) -> Value {
	json!({
		"jsonrpc": "2.0",
		"id": id,
		"error": { "code": code, "message": message },
	})
}

pub(crate) fn write_frame<W: Write>(w: &mut W, value: &Value) -> io::Result<()> {
	let mut line = serde_json::to_string(value)
		.map_err(|e| io::Error::other(format!("serialise frame: {e}")))?;
	if line.contains('\n') {
		return Err(io::Error::other("frame contained newline"));
	}
	line.push('\n');
	w.write_all(line.as_bytes())?;
	w.flush()?;
	Ok(())
}
