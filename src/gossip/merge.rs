use std::collections::HashMap;

use crate::base::math::OnlineSoftmax;
use crate::base::search::EntityHit;

pub fn online_softmax_merge_hits(lists: &[&[EntityHit]], top_k: usize) -> Vec<EntityHit> {
	let mut acc: HashMap<String, OnlineSoftmax> = HashMap::new();
	for list in lists {
		for hit in list.iter() {
			acc
				.entry(hit.entity_id.clone())
				.or_default()
				.update(hit.score);
		}
	}
	let mut out: Vec<EntityHit> = acc
		.into_iter()
		.map(|(id, s)| EntityHit {
			entity_id: id,
			score: s.finalize(),
		})
		.collect();
	out.sort_by(|a, b| {
		b.score
			.partial_cmp(&a.score)
			.unwrap_or(std::cmp::Ordering::Equal)
			.then_with(|| a.entity_id.cmp(&b.entity_id))
	});
	if top_k < out.len() {
		out.truncate(top_k);
	}
	out
}
