use crate::base::graph::GraphGnn;
use crate::base::math::OnlineSoftmax;
use crate::base::search::EntityHit;
use crate::retrieval::expand::{find_entity_in_graph, ScoredEntity};
use std::collections::HashMap;

pub fn merge(g: &GraphGnn, seeds: &[EntityHit], beam: Vec<ScoredEntity>) -> Vec<ScoredEntity> {
	let mut scores: HashMap<String, OnlineSoftmax> = HashMap::new();
	let mut thoughts: HashMap<String, ScoredEntity> = HashMap::new();

	for st in beam {
		scores
			.entry(st.entity.id.clone())
			.or_default()
			.update(st.score);
		thoughts.entry(st.entity.id.clone()).or_insert(st);
	}

	for s in seeds {
		scores
			.entry(s.entity_id.clone())
			.or_default()
			.update(s.score);
		if !thoughts.contains_key(&s.entity_id) {
			if let Some(t) = find_entity_in_graph(g, &s.entity_id) {
				thoughts.insert(
					s.entity_id.clone(),
					ScoredEntity {
						entity: t,
						score: s.score,
					},
				);
			}
		}
	}

	let mut results: Vec<ScoredEntity> = thoughts
		.into_iter()
		.filter_map(|(id, mut st)| {
			let merged = scores.get(&id)?.finalize();
			st.score = merged;
			Some(st)
		})
		.collect();

	results.sort_by(|a, b| {
		b.score
			.partial_cmp(&a.score)
			.unwrap_or(std::cmp::Ordering::Equal)
	});
	results
}
