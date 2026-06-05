use std::collections::hash_map::Entry;
use std::collections::HashMap;

use serde_json::Value;

use crate::client::Client;
use crate::error::McpError;
use crate::inproc::InProcTransport;
use crate::server::McpServer;
use crate::transport::ChildStdio;
use crate::types::{ServerId, ToolResult, ToolSchema};

pub struct LiveServer {
	pub(crate) client: Client,
	pub(crate) tools: Vec<ToolSchema>,
}

impl LiveServer {
	pub fn new(client: Client, tools: Vec<ToolSchema>) -> Self {
		Self { client, tools }
	}

	pub fn tools(&self) -> &[ToolSchema] {
&self.tools
}

	pub fn refresh_tools(&mut self) -> Result<&[ToolSchema], McpError> {
self.tools = self.client.list_tools()?;
		Ok(&self.tools)
}

	pub fn call_tool(&mut self, name: &str, args: &Value) -> Result<ToolResult, McpError> {
		self.client.call_tool(name, args)
	}
}

#[derive(Default)]
pub struct Registry {
	servers: HashMap<ServerId, LiveServer>,
}

impl Registry {
	pub fn new() -> Self {
Self::default()
}

	pub fn insert(&mut self, id: ServerId, server: LiveServer) {
self.servers.insert(id, server);
}

	pub fn spawn_stdio(
		&mut self,
		id: ServerId,
		program: &str,
		args: &[&str],
	) -> Result<&LiveServer, McpError> {
		let transport = ChildStdio::spawn(program, args)?;
		self.install(id, Box::new(transport))
	}

	pub fn register_inproc(
		&mut self,
		id: ServerId,
		server: Box<dyn McpServer>,
	) -> Result<&LiveServer, McpError> {
		let transport = InProcTransport::new(server);
		self.install(id, Box::new(transport))
	}

	fn install(
		&mut self,
		id: ServerId,
		transport: Box<dyn crate::transport::Transport>,
	) -> Result<&LiveServer, McpError> {
		let mut client = Client::new(transport);
		client.initialize("kern", env!("CARGO_PKG_VERSION"))?;
		let tools = client.list_tools()?;
		match self.servers.entry(id) {
			Entry::Vacant(e) => Ok(e.insert(LiveServer { client, tools })),
			Entry::Occupied(e) => Err(McpError::DuplicateServer(e.key().0.clone())),
		}
	}

	pub fn server_ids(&self) -> Vec<ServerId> {
self.servers.keys().cloned().collect()
}

	pub fn list_tools(&self, id: &ServerId) -> Result<&[ToolSchema], McpError> {
self
			.servers
			.get(id)
			.map(|s| s.tools.as_slice())
			.ok_or_else(|| McpError::UnknownServer(id.0.clone()))
}

	pub fn call_tool(
		&mut self,
		id: &ServerId,
		name: &str,
		args: &Value,
	) -> Result<ToolResult, McpError> {
let server = self
			.servers
			.get_mut(id)
			.ok_or_else(|| McpError::UnknownServer(id.0.clone()))?;
		server.call_tool(name, args)
}

	pub fn remove(&mut self, id: &ServerId) {
self.servers.remove(id);
}
}
