//! Multi-backend comparison harness — Phase 1b of the Qdrant-baseline SPEC
//! (`docs/superpowers/specs/2026-06-12-qdrant-baseline-harness-design.md`).
//!
//! Index one [`Corpus`] into every [`VectorBackend`] and score each backend's
//! rankings through the **same** `ndcg` functions plus mean per-query latency, so
//! the resulting [`BackendReport`]s are directly comparable. When a feature-gated
//! `QdrantBackend` is added, it slots in here with zero metric-code changes — the
//! whole point of the seam.

use std::time::Instant;

use crate::base::types::EntityKind;

use super::backend::{Doc, VectorBackend};
use super::ndcg;

/// A query against the corpus: the pre-embedded query vector, the expected
/// relevant ids (ground truth), and an optional kind filter.
#[derive(Debug, Clone)]
pub struct CompareQuery {
	pub id: String,
	pub vector: Vec<f64>,
	pub expected_ids: Vec<String>,
	pub kind_filter: Option<EntityKind>,
}

/// A shared corpus + query set, embedded once and handed to every backend.
pub struct Corpus {
	pub docs: Vec<Doc>,
	pub queries: Vec<CompareQuery>,
}

/// One backend's scored result — the row in the head-to-head table.
#[derive(Debug, Clone)]
pub struct BackendReport {
	pub name: String,
	pub mean_recall10: f64,
	pub mean_ndcg10: f64,
	pub mean_latency_ms: f64,
	pub vector_bytes: usize,
}

/// `k` for recall@k / NDCG@k in the baseline.
pub const K: usize = 10;

/// Index `corpus` into each backend, run every query, and score recall@[`K`] /
/// NDCG@[`K`] via the shared [`ndcg`] functions plus mean per-query latency. The
/// metric code is identical for every backend, so any difference between rows is
/// the index/fusion — not the measurement.
pub fn compare(backends: &mut [Box<dyn VectorBackend>], corpus: &Corpus) -> Vec<BackendReport> {
	let mut out = Vec::with_capacity(backends.len());
	for b in backends.iter_mut() {
		b.index(&corpus.docs);
		let mut recall_sum = 0.0;
		let mut ndcg_sum = 0.0;
		let mut lat_sum = 0.0;
		for q in &corpus.queries {
			let t0 = Instant::now();
			let hits = b.query(&q.vector, K, q.kind_filter);
			lat_sum += t0.elapsed().as_secs_f64() * 1000.0;
			let ranked: Vec<String> = hits.into_iter().map(|h| h.id).collect();
			recall_sum += ndcg::recall_at_k(&ranked, &q.expected_ids, K);
			ndcg_sum += ndcg::ndcg_at_k(&ranked, &q.expected_ids, K);
		}
		let n = corpus.queries.len().max(1) as f64;
		out.push(BackendReport {
			name: b.name().to_string(),
			mean_recall10: recall_sum / n,
			mean_ndcg10: ndcg_sum / n,
			mean_latency_ms: lat_sum / n,
			vector_bytes: b.vector_bytes(),
		});
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::bench_support::backend::KernBackend;

	fn doc(id: &str, v: Vec<f64>, kind: EntityKind) -> Doc {
		Doc { id: id.into(), vector: v, kind: Some(kind) }
	}

	fn corpus() -> Corpus {
		Corpus {
			docs: vec![
				doc("a", vec![1.0, 0.0, 0.0], EntityKind::Fact),
				doc("b", vec![0.0, 1.0, 0.0], EntityKind::Claim),
				doc("c", vec![0.0, 0.0, 1.0], EntityKind::Claim),
				doc("d", vec![1.0, 1.0, 0.0], EntityKind::Claim),
			],
			queries: vec![
				CompareQuery { id: "qa".into(), vector: vec![1.0, 0.0, 0.0], expected_ids: vec!["a".into()], kind_filter: None },
				CompareQuery { id: "qb".into(), vector: vec![0.0, 1.0, 0.0], expected_ids: vec!["b".into()], kind_filter: None },
			],
		}
	}

	#[test]
	fn compare_scores_a_backend_through_the_shared_metrics() {
		let mut backends: Vec<Box<dyn VectorBackend>> = vec![Box::new(KernBackend::new())];
		let reports = compare(&mut backends, &corpus());
		assert_eq!(reports.len(), 1);
		assert_eq!(reports[0].name, "kern");
		assert_eq!(reports[0].mean_recall10, 1.0, "each query's expected doc is its nearest");
		assert!(reports[0].mean_ndcg10 > 0.0);
		assert!(reports[0].mean_latency_ms >= 0.0);
		assert!(reports[0].vector_bytes > 0);
	}

	#[test]
	fn identical_backends_produce_identical_quality_and_memory() {
		// The apples-to-apples guarantee: two identical backends over one corpus
		// must score identically, so the harness itself adds no per-backend bias.
		// (Latency is wall-clock and excluded.)
		let mut backends: Vec<Box<dyn VectorBackend>> =
			vec![Box::new(KernBackend::new()), Box::new(KernBackend::new())];
		let r = compare(&mut backends, &corpus());
		assert_eq!(r[0].mean_recall10, r[1].mean_recall10, "recall is harness-deterministic");
		assert_eq!(r[0].mean_ndcg10, r[1].mean_ndcg10, "ndcg is harness-deterministic");
		assert_eq!(r[0].vector_bytes, r[1].vector_bytes, "memory is harness-deterministic");
	}
}
