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
