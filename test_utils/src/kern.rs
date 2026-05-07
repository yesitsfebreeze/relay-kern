use kern::base;
use kern::base::util::content_hash;

pub fn stub_embed(text: &str) -> Vec<f64> {
	let h = content_hash(text);
	let bytes = h.as_bytes();
	let mut vec = [0.0f64; 4];
	for i in 0..4 {
		let hi = hex_val(bytes[i * 2]);
		let lo = hex_val(bytes[i * 2 + 1]);
		let v = hi * 16 + lo;
		vec[i] = v as f64 / 255.0 - 0.5;
	}
	let norm: f64 = vec.iter().map(|v| v * v).sum::<f64>().sqrt();
	if norm > 0.0 {
		for v in &mut vec {
			*v /= norm;
		}
	}
	vec.to_vec()
}

fn hex_val(c: u8) -> u8 {
	match c {
		b'0'..=b'9' => c - b'0',
		b'a'..=b'f' => c - b'a' + 10,
		_ => 0,
	}
}

pub fn stub_llm_client() -> kern::llm::Client {
	kern::llm::Client::new_embed_only("http://127.0.0.1:1", "stub")
}

pub fn make_entity(id: &str, text: &str) -> base::types::Entity {
	base::types::Entity {
		id: id.to_string(),
		statements: vec![text.to_string()],
		chunks: vec![base::types::ChunkPart {
			kind: base::types::ChunkPartKind::StatementRef,
			text: String::new(),
			index: 0,
		}],
		vector: stub_embed(text),
		score: 0.5,
		kind: base::types::EntityKind::Claim,
		..Default::default()
	}
}

pub fn add_entity_to_kern(kern: &mut base::types::Kern, e: base::types::Entity) {
	kern.entities.insert(e.id.clone(), e);
}

pub fn add_entity_to_graph(g: &mut base::graph::GraphGnn, e: base::types::Entity) {
	let root_id = g.root.id.clone();
	if let Some(kern) = g.kerns.get_mut(&root_id) {
		kern.entities.insert(e.id.clone(), e.clone());
	}
	g.root.entities.insert(e.id.clone(), e);
}

pub fn link_entities_in_graph(
	g: &mut base::graph::GraphGnn,
	from_id: &str,
	to_id: &str,
	text: &str,
) {
	let root_id = g.root.id.clone();
	if let Some(kern) = g.kerns.get_mut(&root_id) {
		link_entities(kern, from_id, to_id, text);
	}
	link_entities(&mut g.root, from_id, to_id, text);
}

pub fn link_entities(kern: &mut base::types::Kern, from_id: &str, to_id: &str, text: &str) {
	let from_vec = kern
		.entities
		.get(from_id)
		.map(|e| e.vector.clone())
		.unwrap_or_default();
	let to_vec = kern
		.entities
		.get(to_id)
		.map(|e| e.vector.clone())
		.unwrap_or_default();
	let score = base::math::cosine(&from_vec, &to_vec);
	let rid = base::math::reason_id(
		from_id,
		to_id,
		base::types::ReasonKind::Similarity,
		text,
		"",
	);
	let reason = base::types::Reason {
		id: rid,
		from: from_id.to_string(),
		to: to_id.to_string(),
		kind: base::types::ReasonKind::Similarity,
		text: text.to_string(),
		vector: base::math::average_vec(&from_vec, &to_vec),
		score,
		..Default::default()
	};
	base::reason::add_reason(kern, reason);
}
