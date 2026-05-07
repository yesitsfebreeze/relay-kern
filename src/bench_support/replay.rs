use crate::base::graph::GraphGnn;
use crate::base::math::{average_vec, cosine, reason_id};
use crate::base::reason::add_reason;
use crate::base::types::*;
use crate::config::RetrievalConfig;
use crate::retrieval::seed::Mode;

use super::embed;
use super::ndcg;
use super::trace::{Trace, TraceQuery};

#[derive(Debug, Clone)]
pub struct QueryReport {
	pub id: String,
	pub mode: Mode,
	pub ranked_ids: Vec<String>,
	pub expected_ids: Vec<String>,
	pub ndcg10: f64,
}

#[derive(Debug, Clone)]
pub struct ReplayReport {
	pub trace_name: String,
	pub per_query: Vec<QueryReport>,
	pub mean_ndcg10: f64,
}

pub fn build_graph(trace: &Trace) -> GraphGnn {
	let mut g = GraphGnn::new();
	let root_id = g.root.id.clone();

	for doc in &trace.docs {
		let vec = embed::embed(&doc.text);
		let t = Entity {
			id: doc.id.clone(),
			statements: vec![doc.text.clone()],
			chunks: vec![ChunkPart {
				kind: ChunkPartKind::StatementRef,
				text: String::new(),
				index: 0,
			}],
			vector: vec,
			score: 0.5,
			kind: EntityKind::Claim,
			..Default::default()
		};
		if let Some(kern) = g.kerns.get_mut(&root_id) {
			kern.entities.insert(t.id.clone(), t);
		}
	}

	let ids: Vec<String> = trace.docs.iter().map(|d| d.id.clone()).collect();
	for i in 0..ids.len() {
		for j in (i + 1)..ids.len() {
			let from = ids[i].clone();
			let to = ids[j].clone();
			let kern = g.kerns.get(&root_id).expect("root kern exists");
			let from_vec = kern.entities.get(&from).expect("inserted above").vector.clone();
			let to_vec = kern.entities.get(&to).expect("inserted above").vector.clone();
			let score = cosine(&from_vec, &to_vec);
			if score < 0.1 {
				continue;
			}
			let rid = reason_id(&from, &to, ReasonKind::Similarity, "", "");
			let r = Reason {
				id: rid,
				from,
				to,
				kind: ReasonKind::Similarity,
				vector: average_vec(&from_vec, &to_vec),
				score,
				..Default::default()
			};
			if let Some(kern) = g.kerns.get_mut(&root_id) {
				add_reason(kern, r);
			}
		}
	}

	g.rebuild_index();
	g
}

pub fn replay(g: &GraphGnn, cfg: &RetrievalConfig, trace: &Trace) -> ReplayReport {
	let mut per_query = Vec::with_capacity(trace.queries.len());
	let mut sum = 0.0;
	for q in &trace.queries {
		let rep = run_one(g, cfg, q);
		sum += rep.ndcg10;
		per_query.push(rep);
	}
	let mean = if per_query.is_empty() { 0.0 } else { sum / per_query.len() as f64 };
	ReplayReport {
		trace_name: trace.name.clone(),
		per_query,
		mean_ndcg10: mean,
	}
}

fn run_one(g: &GraphGnn, cfg: &RetrievalConfig, q: &TraceQuery) -> QueryReport {
	let mode = Mode::parse(&q.mode);
	let qvec = embed::embed(&q.query);
	let result = crate::retrieval::answer::query(g, cfg, &qvec, &q.query, mode, None, None, None);
	let ranked: Vec<String> = result.entities.iter().map(|st| st.entity.id.clone()).collect();
	let ndcg10 = ndcg::ndcg_at_k(&ranked, &q.expected_ids, 10);
	QueryReport {
		id: q.id.clone(),
		mode,
		ranked_ids: ranked,
		expected_ids: q.expected_ids.clone(),
		ndcg10,
	}
}
