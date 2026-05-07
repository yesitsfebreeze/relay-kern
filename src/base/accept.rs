use super::constants::*;
use super::graph::GraphGnn;
use super::math::{average_vec, cosine_distance, reason_id};
use super::reason::add_reason;
use super::search::search_all_unlocked;
use super::types::*;
use crate::crdt::GCounter;

#[derive(Debug)]
pub struct AcceptResult {
	pub placed_in: String,
	pub entity_id: String,
	pub deduped: bool,
	pub reason_ids: Vec<String>,
}

const MAX_ACCEPT_DEPTH: usize = 64;

pub fn accept(g: &mut GraphGnn, kern_id: &str, thought: Entity, doc_id: &str) -> AcceptResult {
	let target_id = route_entity(g, kern_id, &thought);
	commit_entity(g, &target_id, thought, doc_id)
}

fn route_entity(g: &mut GraphGnn, kern_id: &str, thought: &Entity) -> String {
	let mut current_id = kern_id.to_string();

	for _depth in 0..MAX_ACCEPT_DEPTH {
		let hits = search_all_unlocked(g, &thought.vector, 1);
		if !hits.is_empty() && hits[0].score > DEFAULT_DEDUP_THRESHOLD {
			return current_id;
		}

		let children = g
			.loaded(&current_id)
			.map(|k| k.children.clone())
			.unwrap_or_default();
		if let Some(child_id) = route_to_child_id(&children, g, &thought.vector) {
			current_id = child_id;
			continue;
		}

		let reject = {
			let kern = match g.loaded(&current_id) {
				Some(k) => k,
				None => break,
			};
			if kern.has_purpose() {
				let dist = cosine_distance(&thought.vector, &kern.purpose_vec);
				let p = acceptance_probability(dist, kern.inner_radius, kern.outer_radius);
				p < 0.5
			} else {
				false
			}
		};

		if reject {
			let child_id = get_or_spawn_unnamed_child(g, &current_id);
			current_id = child_id;
			continue;
		}

		break;
	}
	current_id
}

fn commit_entity(
	g: &mut GraphGnn,
	kern_id: &str,
	mut thought: Entity,
	doc_id: &str,
) -> AcceptResult {
	let hits = search_all_unlocked(g, &thought.vector, 1);
	if !hits.is_empty() && hits[0].score > DEFAULT_DEDUP_THRESHOLD {
		return AcceptResult {
			placed_in: kern_id.to_string(),
			entity_id: thought.id.clone(),
			deduped: true,
			reason_ids: Vec::new(),
		};
	}

	let root_id = g
		.loaded(kern_id)
		.map(|k| k.root_id.clone())
		.unwrap_or_default();
	thought.root_id = root_id;
	let entity_id = thought.id.clone();
	let thought_vec = thought.vector.clone();
	let external_id = thought.external_id.clone();

	if thought.has_vector() {
		g.entity_idx
			.insert(entity_id.clone(), thought_vec.clone());
	}

	if let Some(kern) = g.get_mut(kern_id) {
		kern.entities.insert(entity_id.clone(), thought);
	}
	g.index_entity(&entity_id, kern_id);

	let mut reason_ids = Vec::new();

	reason_ids.extend(add_similarity_reason(g, kern_id, &entity_id, &thought_vec));

	reason_ids.extend(add_provenance_reason(
		g,
		kern_id,
		&entity_id,
		&thought_vec,
		doc_id,
	));

	if !external_id.is_empty() {
		reason_ids.extend(supersede(
			g,
			kern_id,
			&entity_id,
			&thought_vec,
			&external_id,
		));
	}

	AcceptResult {
		placed_in: kern_id.to_string(),
		entity_id,
		deduped: false,
		reason_ids,
	}
}

fn add_similarity_reason(
	g: &mut GraphGnn,
	kern_id: &str,
	entity_id: &str,
	thought_vec: &[f64],
) -> Vec<String> {
	let hits = search_all_unlocked(g, thought_vec, 2);
	for h in &hits {
		if h.entity_id == entity_id {
			continue;
		}
		let nearest_vec = g
			.kern_of_entity(&h.entity_id)
			.and_then(|kid| g.loaded(kid))
			.and_then(|kern| kern.entities.get(&h.entity_id))
			.map(|t| t.vector.clone())
			.unwrap_or_default();

		let vec = if !thought_vec.is_empty() && !nearest_vec.is_empty() {
			average_vec(thought_vec, &nearest_vec)
		} else {
			Vec::new()
		};

		let rid = reason_id(entity_id, &h.entity_id, ReasonKind::Similarity, "", "");
		let reason = Reason {
			id: rid.clone(),
			from: entity_id.to_string(),
			to: h.entity_id.clone(),
			to_kern_id: String::new(),
			to_net_id: String::new(),
			kind: ReasonKind::Similarity,
			text: String::new(),
			vector: vec.clone(),
			score: h.score,
			traversal_count: GCounter::new(),
			producer_id: String::new(),
		};

		if !vec.is_empty() {
			g.reason_idx.insert(rid.clone(), vec);
		}
		if let Some(kern) = g.get_mut(kern_id) {
			add_reason(kern, reason);
		}
		g.index_reason(&rid, kern_id);
		return vec![rid];
	}
	Vec::new()
}

fn add_provenance_reason(
	g: &mut GraphGnn,
	kern_id: &str,
	entity_id: &str,
	thought_vec: &[f64],
	doc_id: &str,
) -> Vec<String> {
	if doc_id.is_empty() {
		return Vec::new();
	}
	let doc_vec = g
		.loaded(kern_id)
		.and_then(|k| k.entities.get(doc_id))
		.filter(|t| t.has_vector())
		.map(|t| t.vector.clone());

	let vec = match (&doc_vec, thought_vec.is_empty()) {
		(Some(dv), false) => average_vec(thought_vec, dv),
		_ => Vec::new(),
	};

	let rid = reason_id(entity_id, doc_id, ReasonKind::Provenance, "", "");
	let reason = Reason {
		id: rid.clone(),
		from: entity_id.to_string(),
		to: doc_id.to_string(),
		to_kern_id: String::new(),
		to_net_id: String::new(),
		kind: ReasonKind::Provenance,
		text: String::new(),
		vector: vec.clone(),
		score: PROVENANCE_SCORE,
		traversal_count: GCounter::new(),
		producer_id: String::new(),
	};

	if !vec.is_empty() {
		g.reason_idx.insert(rid.clone(), vec);
	}
	if let Some(kern) = g.get_mut(kern_id) {
		add_reason(kern, reason);
	}
	g.index_reason(&rid, kern_id);
	vec![rid]
}

fn supersede(
	g: &mut GraphGnn,
	placed_kern_id: &str,
	entity_id: &str,
	thought_vec: &[f64],
	external_id: &str,
) -> Vec<String> {
	let index_kern_id = g.kern_of_source(external_id).map(|s| s.to_string());
	let old_id = index_kern_id.as_ref().and_then(|kid| {
		g.loaded(kid)
			.and_then(|k| k.source_index.get(external_id).cloned())
	});

	if old_id.as_deref() == Some(entity_id) {
		return Vec::new();
	}

	if let Some(ref ik) = index_kern_id {
		if ik != placed_kern_id {
			if let Some(kern) = g.get_mut(ik) {
				kern.source_index.remove(external_id);
			}
		}
	}
	if let Some(kern) = g.get_mut(placed_kern_id) {
		kern
			.source_index
			.insert(external_id.to_string(), entity_id.to_string());
	}
	g.set_source_entry(external_id.to_string(), placed_kern_id.to_string());

	let old_id = match old_id {
		Some(id) => id,
		None => return Vec::new(),
	};

	let (old_vec, old_kern_id) = {
		let mut found = None;
		if let Some(ref ik) = index_kern_id {
			if let Some(kern) = g.loaded(ik) {
				if let Some(t) = kern.entities.get(&old_id) {
					found = Some((t.vector.clone(), ik.clone()));
				}
			}
		}
		if found.is_none() {
			for kern in g.all() {
				if let Some(t) = kern.entities.get(&old_id) {
					found = Some((t.vector.clone(), kern.id.clone()));
					break;
				}
			}
		}
		match found {
			Some(f) => f,
			None => return Vec::new(),
		}
	};

	if let Some(kern) = g.get_mut(&old_kern_id) {
		if let Some(old) = kern.entities.get_mut(&old_id) {
			old.status = EntityStatus::Superseded;
			old.superseded_by = entity_id.to_string();
		}
	}

	let vec = if !thought_vec.is_empty() && !old_vec.is_empty() {
		average_vec(thought_vec, &old_vec)
	} else {
		Vec::new()
	};

	let rid = reason_id(entity_id, &old_id, ReasonKind::Supersedes, "", "");
	let reason = Reason {
		id: rid.clone(),
		from: entity_id.to_string(),
		to: old_id.clone(),
		to_kern_id: String::new(),
		to_net_id: String::new(),
		kind: ReasonKind::Supersedes,
		text: String::new(),
		vector: vec.clone(),
		score: 1.0,
		traversal_count: GCounter::new(),
		producer_id: String::new(),
	};

	if !vec.is_empty() {
		g.reason_idx.insert(rid.clone(), vec);
	}
	if let Some(kern) = g.get_mut(placed_kern_id) {
		add_reason(kern, reason);
	}
	g.index_reason(&rid, placed_kern_id);

	vec![rid]
}

pub fn get_or_spawn_unnamed_child(g: &mut GraphGnn, kern_id: &str) -> String {
	let children = g
		.loaded(kern_id)
		.map(|k| k.children.clone())
		.unwrap_or_default();
	for child_id in &children {
		if let Some(c) = g.loaded(child_id) {
			if c.is_unnamed() {
				return child_id.clone();
			}
		}
	}
	let root_id = g
		.loaded(kern_id)
		.map(|k| k.root_id.clone())
		.unwrap_or_default();
	let child = Kern::new_unnamed(kern_id, &root_id);
	let child_id = child.id.clone();
	g.register(child);
	if let Some(kern) = g.get_mut(kern_id) {
		kern.children.push(child_id.clone());
	}
	child_id
}

fn route_to_child_id(children: &[String], g: &GraphGnn, vec: &[f64]) -> Option<String> {
	let mut best_id = None;
	let mut best_p = 0.0;
	for id in children {
		let c = match g.loaded(id) {
			Some(k) if k.is_named() && !k.purpose_vec.is_empty() => k,
			_ => continue,
		};
		let dist = cosine_distance(vec, &c.purpose_vec);
		let p = acceptance_probability(dist, c.inner_radius, c.outer_radius);
		if p > best_p {
			best_p = p;
			best_id = Some(id.clone());
		}
	}
	best_id
}

pub fn acceptance_probability(dist: f64, inner: f64, outer: f64) -> f64 {
	if dist <= inner {
		1.0
	} else if dist >= outer {
		0.0
	} else {
		let x = (dist - inner) / (outer - inner);
		1.0 / (1.0 + (8.0 * (x - 0.5)).exp())
	}
}
