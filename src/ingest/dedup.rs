use crate::base::graph::GraphGnn;
use crate::base::types::*;
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

pub fn find_duplicate(
	graph: &Arc<RwLock<GraphGnn>>,
	vec: &[f64],
	threshold: f64,
) -> Option<String> {
	let g = graph.read().ok()?;
	let hits = g.entity_idx.search(vec, 1, 1);
	hits
		.into_iter()
		.find(|h| h.score >= threshold)
		.map(|h| h.id)
}

pub fn update_existing_entity(
	graph: &Arc<RwLock<GraphGnn>>,
	entity_id: &str,
	new_text: &str,
	new_vec: Vec<f64>,
	new_score: f64,
) {
	let lex = {
		let mut g = match graph.write() {
			Ok(g) => g,
			Err(_) => return,
		};
		let kern_id = match g.kern_of_entity(entity_id) {
			Some(kid) => kid.to_string(),
			None => return,
		};
		let kern = match g.get_mut(&kern_id) {
			Some(k) => k,
			None => return,
		};
		if let Some(t) = kern.entities.get_mut(entity_id) {
			t.statements = vec![new_text.to_string()];
			t.chunks = vec![ChunkPart {
				kind: ChunkPartKind::StatementRef,
				text: String::new(),
				index: 0,
			}];
			t.vector = new_vec;
			t.observe_support(new_score);
			t.updated_at = Some(SystemTime::now());
		}
		g.lexical()
	};
	if let Some(lex) = lex {
		lex.insert(entity_id, new_text);
	}
}
