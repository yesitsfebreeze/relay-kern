use crate::base::graph::GraphGnn;
use crate::base::search::EntityHit;
use std::collections::HashMap;

pub fn pagerank(g: &GraphGnn, damping: f64, iters: usize, top_k: usize) -> Vec<EntityHit> {
	let mut id_to_idx: HashMap<String, usize> = HashMap::new();
	let mut ids: Vec<String> = Vec::new();
	for kern in g.map().values() {
		for t in kern.entities.values() {
			if !id_to_idx.contains_key(&t.id) {
				id_to_idx.insert(t.id.clone(), ids.len());
				ids.push(t.id.clone());
			}
		}
	}
	let n = ids.len();
	if n == 0 {
		return Vec::new();
	}

	let mut out: Vec<Vec<usize>> = vec![Vec::new(); n];
	for kern in g.map().values() {
		for r in kern.reasons.values() {
			if r.from == r.to {
				continue;
			}
			let (Some(&fi), Some(&ti)) = (id_to_idx.get(&r.from), id_to_idx.get(&r.to)) else {
				continue;
			};
			out[fi].push(ti);
		}
	}

	let d = damping.clamp(0.0, 1.0);
	let teleport = (1.0 - d) / (n as f64);

	let mut rank = vec![1.0 / (n as f64); n];
	let mut next = vec![0.0f64; n];

	for _ in 0..iters.max(1) {
		let mut dangling = 0.0;
		for (j, outs) in out.iter().enumerate() {
			if outs.is_empty() {
				dangling += rank[j];
			}
		}
		let dangling_share = d * dangling / (n as f64);

		for slot in next.iter_mut() {
			*slot = teleport + dangling_share;
		}
		for (j, outs) in out.iter().enumerate() {
			if outs.is_empty() {
				continue;
			}
			let share = d * rank[j] / (outs.len() as f64);
			for &ti in outs {
				next[ti] += share;
			}
		}
		std::mem::swap(&mut rank, &mut next);
	}

	let mut scored: Vec<(usize, f64)> = rank.iter().copied().enumerate().collect();
	scored.sort_by(|a, b| {
		b.1
			.partial_cmp(&a.1)
			.unwrap_or(std::cmp::Ordering::Equal)
			.then_with(|| ids[a.0].cmp(&ids[b.0]))
	});

	let take = top_k.min(n);
	let mut out_list: Vec<EntityHit> = Vec::with_capacity(take);
	for (idx, score) in scored.into_iter().take(take) {
		out_list.push(EntityHit {
			entity_id: ids[idx].clone(),
			score,
		});
	}
	out_list
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::base::types::{Entity, Kern, Reason};

	fn ent(id: &str) -> Entity {
		Entity {
			id: id.into(),
			..Default::default()
		}
	}
	fn edge(from: &str, to: &str) -> Reason {
		Reason {
			from: from.into(),
			to: to.into(),
			id: format!("{from}->{to}"),
			..Default::default()
		}
	}

	#[test]
	fn empty_graph_is_empty() {
		assert!(pagerank(&GraphGnn::new(), 0.85, 10, 5).is_empty());
	}

	#[test]
	fn ranks_hub_above_leaves_and_sums_to_one() {
		let mut g = GraphGnn::new();
		let mut k = Kern::new("k", "");
		for id in ["A", "B", "C"] {
			k.entities.insert(id.into(), ent(id));
		}
		// B -> A and C -> A : A is the hub.
		for e in [edge("B", "A"), edge("C", "A")] {
			k.reasons.insert(e.id.clone(), e);
		}
		g.register(k);

		let ranks = pagerank(&g, 0.85, 100, 3);
		assert_eq!(ranks.len(), 3);
		let score = |id: &str| ranks.iter().find(|h| h.entity_id == id).unwrap().score;
		assert!(score("A") > score("B"), "hub A must outrank leaf B");
		let sum: f64 = ranks.iter().map(|h| h.score).sum();
		assert!((sum - 1.0).abs() < 1e-6, "ranks sum ~1, got {sum}");
	}
}
