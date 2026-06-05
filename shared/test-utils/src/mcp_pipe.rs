use serde_json::{json, Value};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use trnsprt::{McpError, McpServer, ToolResult, ToolSchema, Transport};

#[derive(Default)]
struct Wire {
	to_server: Vec<u8>,
	from_server: Vec<u8>,
	server_pos: usize,
	killed: bool,
}

struct ClientEnd(Arc<Mutex<Wire>>);

impl Read for ClientEnd {
	fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
		let mut w = self.0.lock().expect("lock");
		let available = &w.from_server[w.server_pos..];
		let n = available.len().min(buf.len());
		buf[..n].copy_from_slice(&available[..n]);
		w.server_pos += n;
		Ok(n)
	}
}

impl Write for ClientEnd {
	fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
		let mut w = self.0.lock().expect("lock");
		w.to_server.extend_from_slice(buf);
		Ok(buf.len())
	}
	fn flush(&mut self) -> std::io::Result<()> {
		Ok(())
	}
}

pub struct PipeTransport {
	reader: ClientEnd,
	writer: ClientEnd,
	wire: Arc<Mutex<Wire>>,
}

impl Transport for PipeTransport {
	fn reader(&mut self) -> &mut dyn Read {
		&mut self.reader
	}
	fn writer(&mut self) -> &mut dyn Write {
		&mut self.writer
	}
	fn kill(&mut self) -> std::io::Result<()> {
		self.wire.lock().expect("lock").killed = true;
		Ok(())
	}
}

pub struct PipeHandle(Arc<Mutex<Wire>>);

impl PipeHandle {
	pub fn drain_frames(&self) -> Vec<Value> {
		let mut w = self.0.lock().expect("lock");
		let bytes = std::mem::take(&mut w.to_server);
		let s = String::from_utf8(bytes).expect("utf8");
		s.lines()
			.filter(|l| !l.is_empty())
			.map(|l| serde_json::from_str(l).expect("json"))
			.collect()
	}

	pub fn push_reply(&self, msg: &Value) {
		let mut w = self.0.lock().expect("lock");
		let mut s = serde_json::to_string(msg).expect("json");
		s.push('\n');
		w.from_server.extend_from_slice(s.as_bytes());
	}

	pub fn killed(&self) -> bool {
		self.0.lock().expect("lock").killed
	}
}

pub fn new_pipe() -> (PipeTransport, PipeHandle) {
	let wire = Arc::new(Mutex::new(Wire::default()));
	let t = PipeTransport {
		reader: ClientEnd(wire.clone()),
		writer: ClientEnd(wire.clone()),
		wire: wire.clone(),
	};
	(t, PipeHandle(wire))
}

pub fn reply_result(id: u64, result: Value) -> Value {
	json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

pub fn reply_error(id: u64, code: i64, message: &str) -> Value {
	json!({
		"jsonrpc": "2.0",
		"id": id,
		"error": { "code": code, "message": message },
	})
}

pub struct AdderServer;

impl McpServer for AdderServer {
	fn tools_list(&self) -> Vec<ToolSchema> {
		vec![ToolSchema {
			name: "add".into(),
			description: Some("a+b".into()),
			input_schema: None,
		}]
	}
	fn call_tool(&self, name: &str, args: &Value) -> Result<ToolResult, McpError> {
		if name != "add" {
			return Err(McpError::Rpc {
				code: -32601,
				message: format!("unknown tool: {name}"),
			});
		}
		let a = args.get("a").and_then(Value::as_i64).unwrap_or(0);
		let b = args.get("b").and_then(Value::as_i64).unwrap_or(0);
		Ok(ToolResult {
			content: vec![json!({ "type": "text", "text": (a + b).to_string() })],
			is_error: false,
			structured_content: None,
		})
	}
}
