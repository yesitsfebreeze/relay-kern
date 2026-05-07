use clap::Parser;
use kern::bench_support::replay::{build_graph, replay};
use kern::bench_support::sweep::{sweep, to_csv, SweepParam};
use kern::bench_support::trace;
use kern::config::RetrievalConfig;

#[derive(Parser, Debug)]
#[command(name = "retrieval_bench", about = "Replay retrieval traces and compute NDCG@10.")]
struct Args {
	#[arg(long)]
	trace: String,
	#[arg(long)]
	sweep: Option<String>,
	#[arg(long, default_value = "")]
	values: String,
	#[arg(long)]
	csv: Option<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let args = Args::parse();
	let t = trace::load(&args.trace)?;
	let g = build_graph(&t);

	match args.sweep {
		None => {
			let report = replay(&g, &RetrievalConfig::default(), &t);
			println!("trace: {}", report.trace_name);
			println!("queries: {}", report.per_query.len());
			for q in &report.per_query {
				println!("  {:<20} mode={:?} ndcg@10={:.4}", q.id, q.mode, q.ndcg10);
			}
			println!("mean NDCG@10: {:.4}", report.mean_ndcg10);
		}
		Some(name) => {
			let param = SweepParam::parse(&name)
				.ok_or_else(|| format!("unknown sweep param: {name}"))?;
			let values: Vec<f64> = args
				.values
				.split(',')
				.filter(|s| !s.is_empty())
				.map(|s| s.trim().parse::<f64>())
				.collect::<Result<_, _>>()?;
			if values.is_empty() {
				return Err("--values required for sweep".into());
			}
			let rows = sweep(&g, &t, param, &values);
			let csv = to_csv(&rows);
			match args.csv.as_deref() {
				Some(path) => {
					std::fs::write(path, &csv)?;
					println!("wrote {} sweep rows to {}", rows.len(), path);
				}
				None => print!("{csv}"),
			}
		}
	}
	Ok(())
}
