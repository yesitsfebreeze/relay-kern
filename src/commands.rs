mod admin;
mod graph_ops;
mod ingest_cmd;
mod mcp_cmd;
mod query;

use std::sync::{Arc, RwLock};

use clap::{Parser, Subcommand};

use crate::base::graph::GraphGnn;
use crate::base::locks::read_recovered;
use crate::base::search::find_entity;
use crate::base::types::Kern;
use crate::base::util::{short_id, truncate};

#[derive(Parser)]
#[command(name = "kern", version, about = "Self-organizing knowledge graph")]
pub struct Cli {
	#[command(subcommand)]
	pub command: Option<Commands>,

	#[arg(short = 'd', long)]
	pub daemon: bool,

	#[arg(long, default_value = "")]
	pub mcp_addr: String,

	#[arg(long)]
	pub mcp_stdio: bool,

	#[arg(long, default_value = "http://localhost:11434")]
	pub embed_url: String,

	#[arg(long, default_value = "nomic-embed-text")]
	pub embed_model: String,

	#[arg(long, default_value = "")]
	pub reason_url: String,

	#[arg(long, default_value = "")]
	pub reason_model: String,
}

#[derive(Subcommand)]
pub enum Commands {
	Ingest {
		text: Vec<String>,
		#[arg(long)]
		file: Option<String>,
		#[arg(long)]
		no_llm: bool,
		#[arg(long)]
		embed_url: Option<String>,
		#[arg(long)]
		embed_model: Option<String>,
		#[arg(long)]
		reason_url: Option<String>,
		#[arg(long)]
		reason_model: Option<String>,
	},
	Query {
		text: String,
		#[arg(long, default_value = "hybrid")]
		mode: String,
		#[arg(long)]
		answer: bool,
		#[arg(long)]
		embed_url: Option<String>,
		#[arg(long)]
		embed_model: Option<String>,
		#[arg(long)]
		reason_url: Option<String>,
		#[arg(long)]
		reason_model: Option<String>,
	},
	Search {
		text: String,
		#[arg(long, default_value = "5")]
		k: usize,
		#[arg(long)]
		embed_url: Option<String>,
		#[arg(long)]
		embed_model: Option<String>,
	},
	Get {
		id: String,
	},
	List,
	Forget {
		id: String,
	},
	Link {
		from: String,
		to: String,
		#[arg(long, default_value = "")]
		reason: String,
		#[arg(long)]
		embed_url: Option<String>,
		#[arg(long)]
		embed_model: Option<String>,
		#[arg(long)]
		reason_url: Option<String>,
		#[arg(long)]
		reason_model: Option<String>,
	},
	Health,
	Purpose {
		text: String,
		#[arg(long)]
		embed_url: Option<String>,
		#[arg(long)]
		embed_model: Option<String>,
	},
	Degrade {
		id: String,
	},
	Descriptor {
		#[command(subcommand)]
		action: DescriptorAction,
	},
	Peers,
	Register {
		path: String,
	},
	Unnamed {
		#[command(subcommand)]
		action: UnnamedAction,
	},
	Mcp,
	Compress {
		src: String,
		#[arg(long, default_value = "int8")]
		mode: String,
		#[arg(long)]
		out: Option<String>,
	},
	#[cfg(feature = "hunt")]
	Hunt {
		#[arg(long, default_value = "60")]
		secs: u64,
	},
}

#[derive(Subcommand)]
pub enum DescriptorAction {
	Add { name: String, description: String },
	Rm { name: String },
}

#[derive(Subcommand)]
pub enum UnnamedAction {
	List,
}

pub(crate) fn load_graph(cfg: &crate::config::Config) -> GraphGnn {
	let mut g = match crate::base::persist::load_dir(&cfg.data_dir) {
		Ok(g) => g,
		Err(_) => {
			let mut g = GraphGnn::new();
			g.data_dir = cfg.data_dir.clone();
			g
		}
	};
	g.set_max_loaded_kerns(cfg.graph.max_kerns);
	g
}

pub(crate) fn save_graph(g: &GraphGnn) {
	if let Err(e) = crate::base::persist::save_all(g) {
		eprintln!("save: {e}");
	}
}

pub(crate) fn with_graph(cfg: &crate::config::Config, f: impl FnOnce(&mut GraphGnn)) {
	let mut g = load_graph(cfg);
	f(&mut g);
	save_graph(&g);
}

pub(crate) fn resolve<'a>(arg: &'a Option<String>, fallback: &'a str) -> &'a str {
	arg.as_deref().unwrap_or(fallback)
}

pub(crate) fn build_llm(
	embed_url: &str,
	embed_model: &str,
	embed_key: &str,
	reason_url: &str,
	reason_model: &str,
	reason_key: &str,
) -> crate::llm::Client {
	crate::llm::Client::new(
		reason_url,
		reason_model,
		reason_key,
		embed_url,
		embed_model,
		embed_key,
	)
}

pub(crate) fn find_entity_by_prefix(
	g: &GraphGnn,
	id: &str,
) -> Option<(crate::base::types::Entity, String)> {
	if let Some(pair) = find_entity(g, id) {
		return Some(pair);
	}
	for k in g.all() {
		for t in k.entities.values() {
			if t.id.starts_with(id) {
				return Some((t.clone(), k.id.clone()));
			}
		}
	}
	None
}

pub(crate) fn print_kern(kern: &Kern, g: &GraphGnn, depth: usize) {
	let indent = "  ".repeat(depth);
	let label = if kern.purpose_text.is_empty() {
		"[unnamed]".to_string()
	} else {
		kern.purpose_text.clone()
	};
	println!(
		"{}kern:{}  thoughts:{}  reasons:{}",
		indent,
		label,
		kern.entities.len(),
		kern.reasons.len(),
	);
	for t in kern.entities.values() {
		println!(
			"{}  [{}] {}",
			indent,
			short_id(&t.id),
			truncate(&t.text(), 72),
		);
	}
	for child_id in &kern.children {
		if let Some(child) = g.kerns.get(child_id) {
			print_kern(child, g, depth + 1);
		}
	}
}

pub async fn dispatch(cmd: Commands, cfg: &crate::config::Config) {
	match cmd {
		Commands::Ingest {
			text,
			file,
			no_llm,
			embed_url,
			embed_model,
			reason_url,
			reason_model,
		} => {
			ingest_cmd::cmd_ingest(
				cfg,
				text,
				file,
				no_llm,
				resolve(&embed_url, &cfg.embed.url),
				resolve(&embed_model, &cfg.embed.model),
				resolve(&reason_url, &cfg.reason.url),
				resolve(&reason_model, &cfg.reason.model),
			)
			.await
		}

		Commands::Query {
			text,
			mode,
			answer,
			embed_url,
			embed_model,
			reason_url,
			reason_model,
		} => {
			query::cmd_query(
				cfg,
				&text,
				&mode,
				answer,
				resolve(&embed_url, &cfg.embed.url),
				resolve(&embed_model, &cfg.embed.model),
				resolve(&reason_url, &cfg.reason.url),
				resolve(&reason_model, &cfg.reason.model),
			)
			.await
		}

		Commands::Search {
			text,
			k,
			embed_url,
			embed_model,
		} => {
			query::cmd_search(
				cfg,
				&text,
				k,
				resolve(&embed_url, &cfg.embed.url),
				resolve(&embed_model, &cfg.embed.model),
			)
			.await
		}

		Commands::Get { id } => graph_ops::cmd_get(cfg, &id),
		Commands::List => graph_ops::cmd_list(cfg),
		Commands::Forget { id } => graph_ops::cmd_forget(cfg, &id),

		Commands::Link {
			from,
			to,
			reason,
			embed_url,
			embed_model,
			reason_url,
			reason_model,
		} => {
			graph_ops::cmd_link(
				cfg,
				&from,
				&to,
				&reason,
				resolve(&embed_url, &cfg.embed.url),
				resolve(&embed_model, &cfg.embed.model),
				resolve(&reason_url, &cfg.reason.url),
				resolve(&reason_model, &cfg.reason.model),
			)
			.await
		}

		Commands::Health => admin::cmd_health(cfg),

		Commands::Purpose {
			text,
			embed_url,
			embed_model,
		} => {
			admin::cmd_purpose(
				cfg,
				&text,
				resolve(&embed_url, &cfg.embed.url),
				resolve(&embed_model, &cfg.embed.model),
			)
			.await
		}

		Commands::Degrade { id } => graph_ops::cmd_degrade(cfg, &id),
		Commands::Descriptor { action } => admin::cmd_descriptor(cfg, action),
		Commands::Peers => admin::cmd_peers(),
		Commands::Register { path } => admin::cmd_register(cfg, &path),
		Commands::Unnamed { action } => admin::cmd_unnamed(cfg, action),
		Commands::Mcp => mcp_cmd::cmd_mcp(cfg).await,
		Commands::Compress { src, mode, out } => admin::cmd_compress(&src, &mode, out.as_deref()),
		#[cfg(feature = "hunt")]
		Commands::Hunt { secs } => {
			// Run the full daemon (TCP MCP + tick worker) and bail after
			// `secs`. cmd_mcp is stdio-driven and would EOF immediately
			// when launched without an interactive parent.
			let cfg = cfg.clone();
			let cli = Cli {
				command: None,
				daemon: true,
				mcp_addr: String::new(),
				mcp_stdio: false,
				embed_url: "http://localhost:11434".to_string(),
				embed_model: "nomic-embed-text".to_string(),
				reason_url: String::new(),
				reason_model: String::new(),
			};
			tokio::select! {
				_ = run_server(&cli, &cfg) => {}
				_ = tokio::time::sleep(std::time::Duration::from_secs(secs)) => {
					eprintln!("kern hunt: {secs}s elapsed, exiting");
				}
			}
		}
	}
}

pub async fn run_server(cli: &Cli, cfg: &crate::config::Config) {
	if let Some(j) = journal::global() {
		j.set_max_bytes(cfg.journal.max_today_bytes);
	}
	let g = Arc::new(RwLock::new(load_graph(cfg)));
	let llm_client = build_llm(
		&cli.embed_url,
		&cli.embed_model,
		&cfg.embed.key,
		&cli.reason_url,
		&cli.reason_model,
		cfg.reason_key(),
	);

	let q = Arc::new(crate::tick::queue::Queue::new(cfg.tick.queue_capacity.max(1)));

	let save_g = g.clone();
	let save_fn: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
		let g = read_recovered(&save_g);
		save_graph(&g);
	});
	let llm_fn: Option<crate::ingest::LlmFunc> = if !cli.reason_url.is_empty() {
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

	// Slice K — session mirror. Tails the shared journal `fork_*`
	// lifecycle events and ingests each new fork as a `Document` entity
	// with `Source::Session`. Skipped silently if the project's history
	// SQLite cannot be opened (e.g. read-only filesystem during tests).
	{
		use crate::ingest::session_mirror::{run, SessionMirror, WorkerSink};
		let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
		match journal::History::open(&cwd) {
			Ok(history) => {
				if cfg.journal.retain_days > 0 {
					match history.retain_days(cfg.journal.retain_days) {
						Ok(n) if n > 0 => tracing::info!(
							target: "kern.journal",
							pruned = n,
							retain_days = cfg.journal.retain_days,
							"history.db pruned"
						),
						Ok(_) => {}
						Err(e) => tracing::warn!(
							target: "kern.journal",
							error = %e,
							"history.db prune failed"
						),
					}
				}
				let history = Arc::new(history);
				let sink = WorkerSink::new(worker.clone());
				let mut sm = SessionMirror::new(sink);
				sm.set_max_seen(cfg.ingest.session_mirror_max_seen);
				let mirror = Arc::new(tokio::sync::Mutex::new(sm));
				tokio::spawn(run(history, mirror, std::time::Duration::from_secs(2)));
			}
			Err(e) => {
				tracing::warn!(target: "kern.session_mirror", error = %e, "history open failed; session mirror disabled");
			}
		}
	}

	// Slice O — kern-side filesystem watcher. Off unless the project's
	// `[watcher]` section in `.relay/kern.toml` sets `enabled = true`.
	// Roots default to cwd when `enabled = true` but no roots are listed.
	if cfg.watcher.enabled {
		use crate::ingest::file_watcher::{run as run_file_watcher, KernFileWatcherSink};
		use watcher::IgnoreRules;
		let roots: Vec<std::path::PathBuf> = if cfg.watcher.roots.is_empty() {
			vec![std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))]
		} else {
			cfg.watcher.roots.iter().map(std::path::PathBuf::from).collect()
		};
		let ignore = IgnoreRules::from_roots(&roots);
		let sink = Arc::new(KernFileWatcherSink::new(worker.clone()));
		tokio::spawn(async move {
			if let Err(e) = run_file_watcher(roots, ignore, sink).await {
				tracing::warn!(target: "kern.file_watcher", error = %e, "watcher exited");
			}
		});
	}

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

	crate::tick::enqueue_all(&q, &g);

	let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
	tokio::spawn(async move {
		tokio::signal::ctrl_c().await.ok();
		let _ = shutdown_tx.send(());
	});

	let mcp_server = Arc::new(crate::mcp::Server {
		graph: g.clone(),
		worker: worker.clone(),
		llm: Some(llm_client.clone()),
		save_fn: save_fn.clone(),
		task_q: Some(q.clone()),
		cfg: Arc::new(cfg.clone()),
	});

	// Phase 1 typed-RPC listener (additive). Binds an ephemeral TCP port,
	// publishes it to `.relay/kern_memory.port` for agnt to discover, and
	// serves `MemoryRpc` requests via the new `Channel<JsonEnvelopeCodec>`
	// stack. Independent of MCP — runs whether or not stdio/SSE are used.
	// `MemoryHandler` is wired to the same `mcp::Server` the MCP listener
	// uses, so both paths invoke identical kern internals.
	{
		let mem = Arc::new(std::sync::Mutex::new(crate::memory_service::MemoryService::new()));

		// Slice J typed-RPC: KernRpc is a sibling listener to MemoryRpc,
		// publishing `.relay/kern_rpc.port`. Shares the in-memory store
		// with MemoryRpc so `truncate_after` is consistent across both
		// surfaces. Independent accept loop so neither service blocks
		// the other.
		{
			let kern_handle = mcp_server.clone();
			let mem = mem.clone();
			tokio::spawn(async move {
				let handler = crate::rpc::KernRpcHandler::new(kern_handle, mem);
				let dir = std::path::Path::new(".relay");
				match crate::rpc::kern_rpc_listen(handler, dir).await {
					Ok(_join) => {}
					Err(e) => {
						tracing::warn!(target: "kern.kern_rpc", error = %e, "listen failed");
					}
				}
			});
		}

		let kern_handle = mcp_server.clone();
		tokio::spawn(async move {
			let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
				Ok(l) => l,
				Err(e) => {
					tracing::warn!(target: "kern.memory_rpc", error = %e, "bind failed");
					return;
				}
			};
			let port = match listener.local_addr() {
				Ok(a) => a.port(),
				Err(e) => {
					tracing::warn!(target: "kern.memory_rpc", error = %e, "local_addr");
					return;
				}
			};
			let dir = std::path::Path::new(".relay");
			if let Err(e) = std::fs::create_dir_all(dir) {
				tracing::warn!(target: "kern.memory_rpc", error = %e, "mkdir .relay");
				return;
			}
			let final_path = dir.join("kern_memory.port");
			let tmp_path = dir.join("kern_memory.port.tmp");
			if let Err(e) = std::fs::write(&tmp_path, port.to_string()) {
				tracing::warn!(target: "kern.memory_rpc", error = %e, "write port tmp");
				return;
			}
			if let Err(e) = std::fs::rename(&tmp_path, &final_path) {
				tracing::warn!(target: "kern.memory_rpc", error = %e, "rename port");
				return;
			}
			tracing::info!(target: "kern.memory_rpc", port, "listening");
			loop {
				let (stream, _peer) = match listener.accept().await {
					Ok(p) => p,
					Err(e) => {
						tracing::warn!(target: "kern.memory_rpc", error = %e, "accept");
						continue;
					}
				};
				let mem = mem.clone();
				let kern_handle = kern_handle.clone();
				tokio::spawn(async move {
					let adapter = trnsprt::typed::TcpAdapter::new(stream);
					let channel = trnsprt::typed::Channel::new(
						adapter,
						trnsprt::typed::JsonEnvelopeCodec::new(),
					);
					let handler = crate::memory_service::MemoryHandler::new(mem, kern_handle);
					if let Err(e) = protocol::memory::serve_memory_rpc(channel, handler).await {
						tracing::warn!(target: "kern.memory_rpc", error = %e, "serve loop");
					}
				});
			}
		});
	}

	if cli.mcp_stdio {
		mcp_server.run_stdio();
	} else {
		if !cli.mcp_addr.is_empty() {
			let mcp_addr = cli.mcp_addr.clone();
			let mcp_s = mcp_server.clone();
			tokio::spawn(async move {
				if let Err(e) = crate::mcp::sse::run_sse(mcp_s, &mcp_addr).await {
					eprintln!("mcp-sse: {e}");
				}
			});
		}

		if !cli.daemon {
			crate::repl::run(
				g.clone(),
				worker,
				llm_client,
				Some(q.clone()),
				cfg.ingest.dedup_threshold,
			)
			.await;
		} else {
			println!("kern running in daemon mode (ctrl-c to stop)");
			let _ = shutdown_rx.await;
		}
	}

	drop(q);

	eprintln!("shutting down...");
	{
		let g = read_recovered(&g);
		save_graph(&g);
	}
	eprintln!("done");
}
