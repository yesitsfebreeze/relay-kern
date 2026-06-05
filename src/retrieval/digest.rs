//! Recall digest: a markdown snapshot of the kern's purpose plus its
//! hottest thoughts, written to disk for the Claude-Code SessionStart hook
//! to inject. Pure builder + a thin file writer; no live query path.

use crate::base::graph::GraphGnn;
use crate::base::types::{Entity, EntityKind, EntityStatus};

/// Render the digest markdown: purpose header + up to `k` hottest active
/// thoughts, hottest first.
///
/// `min_trust` is a posterior-confidence floor (`Entity::conf_mean`): claims
/// below it are quarantined out of the digest. The digest is replayed into
/// every future session, so it is the persistent re-injection surface for
/// memory-poisoning — gating it keeps low-trust and repeatedly-contradicted
/// claims off that surface. Pass `0.0` to disable the gate.
pub fn build_digest(graph: &GraphGnn, k: usize, min_trust: f64) -> String {
	let mut out = String::from("# kern memory\n\n");
	let purpose = graph.root.purpose_text.trim();
	if !purpose.is_empty() {
		out.push_str("Purpose: ");
		out.push_str(purpose);
		out.push_str("\n\n");
	}

	let mut ents: Vec<&Entity> = graph
		.kerns
		.values()
		.flat_map(|kern| kern.entities.values())
		.filter(|e| {
			matches!(e.status, EntityStatus::Active)
				&& !matches!(e.kind, EntityKind::Document | EntityKind::Question)
				&& e.statements.first().is_some_and(|s| !s.trim().is_empty())
				&& e.conf_mean() >= min_trust
		})
		.collect();
	ents.sort_by(|a, b| {
		b.heat
			.partial_cmp(&a.heat)
			.unwrap_or(std::cmp::Ordering::Equal)
	});

	let bullets: Vec<&Entity> = ents.into_iter().take(k).collect();
	if !bullets.is_empty() {
		out.push_str("## What I know\n\n");
		for e in bullets {
			if let Some(s) = e.statements.first() {
				out.push_str("- ");
				out.push_str(s.trim());
				out.push('\n');
			}
		}
	}
	out
}

/// Render and write the digest to `path`, creating parent dirs. Best effort.
pub fn write_digest(graph: &GraphGnn, path: &std::path::Path, k: usize, min_trust: f64) {
	if let Some(parent) = path.parent() {
		let _ = std::fs::create_dir_all(parent);
	}
	if let Err(e) = std::fs::write(path, build_digest(graph, k, min_trust)) {
		tracing::warn!(target: "kern.digest", path = %path.display(), error = %e, "digest write failed");
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::base::graph::GraphGnn;
	use crate::base::types::{mk_entity, EntityKind};

	#[test]
	fn digest_has_purpose_and_hottest_first_capped() {
		let mut g = GraphGnn::default();
		g.root.purpose_text = "remember durable facts".to_string();
		let root_id = g.root.id.clone();
		let kern = g.kerns.get_mut(&root_id).expect("root kern");
		kern.entities.insert("a".into(), mk_entity("a", "cold fact", 0.1, EntityKind::Claim));
		kern.entities.insert("b".into(), mk_entity("b", "hot fact", 9.0, EntityKind::Claim));

		let md = build_digest(&g, 1, 0.0);
		assert!(md.contains("remember durable facts"), "purpose present");
		assert!(md.contains("hot fact"), "hottest included");
		assert!(!md.contains("cold fact"), "capped at k=1");
	}

	#[test]
	fn documents_are_excluded_claims_kept() {
		let mut g = GraphGnn::default();
		let root_id = g.root.id.clone();
		let kern = g.kerns.get_mut(&root_id).expect("root kern");
		kern.entities.insert("doc".into(), mk_entity("doc", "raw document chunk", 9.0, EntityKind::Document));
		kern.entities.insert("clm".into(), mk_entity("clm", "a distilled claim", 0.5, EntityKind::Claim));

		let md = build_digest(&g, 10, 0.0);
		assert!(md.contains("a distilled claim"), "claim kept");
		assert!(!md.contains("raw document chunk"), "document excluded even though hotter");
	}

	#[test]
	fn empty_graph_yields_header_only() {
		let g = GraphGnn::default();
		let md = build_digest(&g, 10, 0.0);
		assert!(md.contains("# kern memory"));
	}

	#[test]
	fn low_trust_claim_quarantined_even_when_hottest() {
		let mut g = GraphGnn::default();
		let root_id = g.root.id.clone();
		let kern = g.kerns.get_mut(&root_id).expect("root kern");
		// Hottest entity, but repeatedly contradicted → low posterior trust.
		let mut poisoned = mk_entity("p", "poisoned hot claim", 99.0, EntityKind::Claim);
		poisoned.conf_alpha = 1.0;
		poisoned.conf_beta = 9.0; // conf_mean = 0.1
		poisoned.refresh_score();
		kern.entities.insert("p".into(), poisoned);
		kern.entities
			.insert("t".into(), mk_entity("t", "trusted cool claim", 0.5, EntityKind::Claim));

		// Gate at 0.35: poisoned (0.1) quarantined despite being hottest;
		// trusted (mk_entity mean 0.667) survives.
		let gated = build_digest(&g, 10, 0.35);
		assert!(!gated.contains("poisoned hot claim"), "low-trust claim quarantined");
		assert!(gated.contains("trusted cool claim"), "trusted claim kept");

		// Gate off: poisoned claim re-injected (hottest first).
		let ungated = build_digest(&g, 10, 0.0);
		assert!(ungated.contains("poisoned hot claim"), "gate off → re-injected");
	}
}
