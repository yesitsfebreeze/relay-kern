use clap::Parser;

use kern::commands::{Cli, dispatch, run_server};
use kern::config::Config;

#[tokio::main]
async fn main() {
	use tracing_subscriber::prelude::*;
	let _ = tracing_subscriber::registry()
		.with(journal::JournalTracingLayer::new("kern"))
		.try_init();
	let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
	let cfg = Config::load(&cwd).unwrap_or_default();
	let cli = Cli::parse();

	match cli.command {
		Some(cmd) => dispatch(cmd, &cfg).await,
		None => run_server(&cli, &cfg).await,
	}
}
