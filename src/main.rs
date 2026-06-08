use clap::Parser;

use kern::commands::{Cli, dispatch, run_server};
use kern::config::Config;

fn main() {
	use tracing_subscriber::prelude::*;
	let _ = tracing_subscriber::registry()
		.with(journal::JournalTracingLayer::new("kern"))
		.try_init();

	// Floor the worker-thread count. The daemon runs several blocking bridges
	// (tick distillation, ingest embedding, the keepalive ping) that each pin a
	// worker via `block_in_place`/`block_on`. On a low-core box the default
	// (one worker per core) lets those consume every worker, starving the time
	// driver — which freezes the heartbeat AND the watchdog's liveness beat, the
	// exact total stall that wedges the hub. The tick/ingest consumers are serial
	// (≤1 in-flight blocking LLM call each), so ≥4 workers guarantees the async
	// UI/RPC paths and timers always have a thread to run on.
	let workers = std::thread::available_parallelism()
		.map(|n| n.get())
		.unwrap_or(4)
		.max(4);
	let rt = tokio::runtime::Builder::new_multi_thread()
		.worker_threads(workers)
		.enable_all()
		.build()
		.expect("build tokio runtime");

	rt.block_on(async {
		// Pin this instance to its project root (the nearest ancestor holding a
		// `.kern`), so the endpoint tag, data_dir, and capture spool all anchor
		// to the same directory. Without this, a daemon launched from a subdir
		// or the wrong cwd resolves the relative `.kern/data` against an empty
		// location and silently boots an empty graph while still serving queries.
		let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
		let root = Config::resolve_root(&cwd);
		if root != cwd {
			let _ = std::env::set_current_dir(&root);
		}
		let cfg = Config::load(&root).unwrap_or_default();
		let cli = Cli::parse();

		match cli.command {
			Some(cmd) => dispatch(cmd, &cfg).await,
			None => run_server(&cli, &cfg).await,
		}
	});
}
