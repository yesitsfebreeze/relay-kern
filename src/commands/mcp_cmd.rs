//! `kern mcp` subcommand: serve stdio MCP.
//!
//! Two modes auto-selected:
//!
//! - **Proxy** (preferred): if a kern singleton is already running at
//!   `kern.sock`, attach as a `KernRpcClient` and forward every stdio
//!   MCP `tools/call` over kern.sock via the typed `call_tool` escape
//!   hatch. The proxy holds no graph, no tick worker, no ingest queue —
//!   every heavy bit lives in the daemon.
//!
//! - **Standalone** (fallback): no daemon is reachable, so load a full
//!   graph + worker + tick locally and serve stdio MCP directly from
//!   them. Matches the pre-singleton behavior so external MCP clients
//!   (Claude Desktop, etc.) keep working when no daemon is up.

use std::sync::Arc;
use std::sync::RwLock as StdRwLock;

use tokio::sync::Mutex as TokioMutex;
use trnsprt::kern_rpc::{CallToolReq, KernRpcClient};
use trnsprt::typed::JsonEnvelopeCodec;
use trnsprt::{McpError, McpServer, ToolResult, ToolSchema};

use crate::base::locks::read_recovered;

use super::{load_graph, save_graph};

pub(super) async fn cmd_mcp(cfg: &crate::config::Config) {
	match KernRpcClient::<JsonEnvelopeCodec>::connect_local().await {
		Ok(client) => {
			tracing::info!(
				target: "kern.mcp_proxy",
				"attached to running daemon — proxy mode"
			);
			let proxy = ProxyServer {
				client: Arc::new(TokioMutex::new(client)),
			};
			// `serve_stdio` is sync (BufRead/Write on stdin/stdout). Run
			// it on a blocking thread so it doesn't park a runtime worker.
			// Each `call_tool` invocation crosses back into async via
			// `block_in_place` + `Handle::current().block_on`, which is
			// supported on the multi-thread runtime kern uses.
			if let Err(e) =
				tokio::task::spawn_blocking(move || trnsprt::serve_stdio(&proxy)).await
			{
				tracing::warn!(target: "kern.mcp_proxy", error = %e, "stdio loop");
			}
		}
		Err(e) => {
			tracing::info!(
				target: "kern.mcp",
				error = %e,
				"no daemon at kern.sock — standalone mode"
			);
			run_standalone(cfg).await;
		}
	}
}

// ---- Proxy ---------------------------------------------------------------

struct ProxyServer {
	client: Arc<TokioMutex<KernRpcClient<JsonEnvelopeCodec>>>,
}

impl McpServer for ProxyServer {
	fn server_name(&self) -> &str {
		"kern"
	}
	fn server_version(&self) -> &str {
		env!("CARGO_PKG_VERSION")
	}

	fn tools_list(&self) -> Vec<ToolSchema> {
		// Tool schema is static — no graph state needed. Serve directly
		// from the same source the standalone path uses so a proxy and
		// a standalone instance advertise byte-identical tool lists.
		crate::mcp::tools::tool_definitions()
			.into_iter()
			.filter_map(|v| serde_json::from_value(v).ok())
			.collect()
	}

	fn call_tool(
		&self,
		name: &str,
		args: &serde_json::Value,
	) -> Result<ToolResult, McpError> {
		let client = self.client.clone();
		let req = CallToolReq {
			name: name.to_string(),
			args: args.clone(),
		};
		let res = tokio::task::block_in_place(|| {
			tokio::runtime::Handle::current().block_on(async move {
				let c = client.lock().await;
				c.call_tool(req).await
			})
		})
		.map_err(|e| McpError::Rpc {
			code: -32000,
			message: format!("kern_rpc call_tool: {e}"),
		})?;

		let content = res
			.envelope
			.get("content")
			.and_then(|v| v.as_array())
			.cloned()
			.unwrap_or_default();
		let is_error = res
			.envelope
			.get("isError")
			.and_then(|v| v.as_bool())
			.unwrap_or(false);
		Ok(ToolResult {
			content,
			is_error,
			structured_content: None,
		})
	}

	fn extra_capabilities(&self) -> serde_json::Value {
		// Match the standalone server so a client probing capabilities
		// can't tell the two apart. Resources/prompts handlers fall
		// through to method-not-found until they're proxied too
		// (follow-up: route resources/* via a future KernRpc method).
		serde_json::json!({"resources": {}, "prompts": {}})
	}
}

// ---- Standalone (legacy heavy path) --------------------------------------

async fn run_standalone(cfg: &crate::config::Config) {
	let g = Arc::new(StdRwLock::new(load_graph(cfg)));
	let llm_client = crate::llm::Client::new(
		cfg.reason_url(),
		&cfg.reason.model,
		cfg.reason_key(),
		&cfg.embed.url,
		&cfg.embed.model,
		&cfg.embed.key,
	);
	let save_g = g.clone();
	let save_fn: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
		let g = read_recovered(&save_g);
		save_graph(&g);
	});
	let llm_fn: Option<crate::ingest::LlmFunc> = if !cfg.reason_url().is_empty() {
		Some(Arc::new(llm_client.complete_func()))
	} else {
		None
	};
	let worker = Arc::new(crate::ingest::Worker::new(
		g.clone(),
		llm_client.clone(),
		llm_fn,
		Some(save_fn.clone()),
	));

	let q = Arc::new(crate::tick::queue::Queue::new(512));
	let tick_llm: crate::tick::tasks::LlmFunc = Arc::new(llm_client.complete_func());
	let tick_embed: crate::tick::tasks::EmbedFunc = {
		let c = llm_client.clone();
		Arc::new(move |text: &str| -> Result<Vec<f64>, String> {
			let c = c.clone();
			let text = text.to_string();
			match tokio::runtime::Handle::try_current() {
				Ok(h) => {
					let result = std::thread::scope(|_| h.block_on(c.embed(&text)));
					result.map_err(|e: crate::llm::LlmError| e.to_string())
				}
				Err(_) => Err("no runtime".to_string()),
			}
		})
	};
	crate::tick::start(
		q.clone(),
		g.clone(),
		Some(tick_llm),
		Some(tick_embed),
		None,
		cfg.gnn.into(),
		cfg.tick,
	);

	let server = crate::mcp::Server {
		graph: g,
		worker,
		llm: Some(llm_client),
		save_fn,
		task_q: Some(q),
		cfg: Arc::new(cfg.clone()),
	};
	server.run_stdio();
}
