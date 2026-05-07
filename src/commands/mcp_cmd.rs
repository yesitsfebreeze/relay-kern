use std::sync::{Arc, RwLock};

use crate::base::locks::read_recovered;

use super::{load_graph, save_graph};

pub(super) async fn cmd_mcp(cfg: &crate::config::Config) {
	let g = Arc::new(RwLock::new(load_graph(cfg)));
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
