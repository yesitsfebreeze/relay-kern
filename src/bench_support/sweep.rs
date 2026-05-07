use crate::base::graph::GraphGnn;
use crate::config::RetrievalConfig;

use super::replay::{replay, ReplayReport};
use super::trace::Trace;

#[derive(Debug, Clone)]
pub struct SweepRow {
	pub param: String,
	pub value: String,
	pub mean_ndcg10: f64,
	pub num_queries: usize,
}

#[derive(Debug, Clone, Copy)]
pub enum SweepParam {
	RrfK,
	MinDeliverScore,
	MmrLambda,
	SeedK,
}

impl SweepParam {
	pub fn parse(s: &str) -> Option<Self> {
		match s {
			"rrf_k" => Some(Self::RrfK),
			"min_deliver_score" => Some(Self::MinDeliverScore),
			"mmr_lambda" => Some(Self::MmrLambda),
			"seed_k" => Some(Self::SeedK),
			_ => None,
		}
	}

	pub fn name(&self) -> &'static str {
		match self {
			Self::RrfK => "rrf_k",
			Self::MinDeliverScore => "min_deliver_score",
			Self::MmrLambda => "mmr_lambda",
			Self::SeedK => "seed_k",
		}
	}
}

fn apply(cfg: &mut RetrievalConfig, param: SweepParam, value: f64) {
	match param {
		SweepParam::RrfK => cfg.rrf_k = value,
		SweepParam::MinDeliverScore => cfg.min_deliver_score = value,
		SweepParam::MmrLambda => cfg.mmr_lambda = value,
		SweepParam::SeedK => cfg.seed_k = value.max(1.0) as usize,
	}
}

pub fn sweep(g: &GraphGnn, trace: &Trace, param: SweepParam, values: &[f64]) -> Vec<SweepRow> {
	let mut rows = Vec::with_capacity(values.len());
	for &v in values {
		let mut cfg = RetrievalConfig::default();
		apply(&mut cfg, param, v);
		let report: ReplayReport = replay(g, &cfg, trace);
		rows.push(SweepRow {
			param: param.name().to_string(),
			value: format!("{v}"),
			mean_ndcg10: report.mean_ndcg10,
			num_queries: report.per_query.len(),
		});
	}
	rows
}

pub fn to_csv(rows: &[SweepRow]) -> String {
	let mut out = String::from("param,value,mean_ndcg10,num_queries\n");
	for r in rows {
		out.push_str(&format!(
			"{},{},{:.6},{}\n",
			r.param, r.value, r.mean_ndcg10, r.num_queries
		));
	}
	out
}
