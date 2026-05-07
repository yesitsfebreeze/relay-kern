use super::graph::GraphGnn;
use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Debug, Clone)]
pub struct LexicalHit {
	pub entity_id: String,
	pub score: f32,
}

#[derive(Default)]
struct Posting {
	tf: u32,
}

struct Inner {
	k1: f32,
	b: f32,
	postings: HashMap<String, HashMap<String, Posting>>,
	doc_len: HashMap<String, u32>,
	total_len: u64,
}

pub struct LexicalIndex {
	inner: RwLock<Inner>,
}

impl LexicalIndex {
	pub fn new_in_ram(k1: f32, b: f32) -> Self {
		Self {
			inner: RwLock::new(Inner {
				k1,
				b,
				postings: HashMap::new(),
				doc_len: HashMap::new(),
				total_len: 0,
			}),
		}
	}

	pub fn insert(&self, entity_id: &str, text: &str) {
		let tokens = tokenize(text);
		let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
		inner_remove(&mut inner, entity_id);
		let dl = tokens.len() as u32;
		if dl == 0 {
			return;
		}
		let mut tfs: HashMap<String, u32> = HashMap::new();
		for tok in tokens {
			*tfs.entry(tok).or_insert(0) += 1;
		}
		for (tok, tf) in tfs {
			inner
				.postings
				.entry(tok)
				.or_default()
				.insert(entity_id.to_string(), Posting { tf });
		}
		inner.doc_len.insert(entity_id.to_string(), dl);
		inner.total_len += dl as u64;
	}

	pub fn remove(&self, entity_id: &str) {
		let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
		inner_remove(&mut inner, entity_id);
	}

	pub fn search(&self, query: &str, k: usize) -> Vec<LexicalHit> {
		let tokens = tokenize(query);
		if tokens.is_empty() || k == 0 {
			return Vec::new();
		}
		let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
		let n_docs = inner.doc_len.len() as f32;
		if n_docs <= 0.0 {
			return Vec::new();
		}
		let avgdl = (inner.total_len as f32 / n_docs).max(1.0);
		let k1 = inner.k1;
		let b = inner.b;

		let mut scores: HashMap<String, f32> = HashMap::new();
		for tok in &tokens {
			let postings = match inner.postings.get(tok) {
				Some(p) => p,
				None => continue,
			};
			let df = postings.len() as f32;
			let idf = ((n_docs - df + 0.5) / (df + 0.5) + 1.0).ln();
			for (doc_id, post) in postings {
				let dl = *inner.doc_len.get(doc_id).unwrap_or(&0) as f32;
				let tf = post.tf as f32;
				let denom = tf + k1 * (1.0 - b + b * dl / avgdl);
				let s = idf * (tf * (k1 + 1.0)) / denom;
				*scores.entry(doc_id.clone()).or_insert(0.0) += s;
			}
		}
		let mut hits: Vec<LexicalHit> = scores
			.into_iter()
			.map(|(id, s)| LexicalHit {
				entity_id: id,
				score: s,
			})
			.collect();
		hits.sort_by(|a, b| {
			b.score
				.partial_cmp(&a.score)
				.unwrap_or(std::cmp::Ordering::Equal)
		});
		hits.truncate(k);
		hits
	}

	pub fn rebuild_from_graph(&self, g: &GraphGnn) {
		{
			let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
			inner.postings.clear();
			inner.doc_len.clear();
			inner.total_len = 0;
		}
		for kern in g.all() {
			for t in kern.entities.values() {
				let joined = t.statements.join(" ");
				if !joined.is_empty() {
					self.insert(&t.id, &joined);
				}
			}
		}
	}

	pub fn doc_count(&self) -> usize {
		self
			.inner
			.read()
			.unwrap_or_else(|e| e.into_inner())
			.doc_len
			.len()
	}
}

fn inner_remove(inner: &mut Inner, entity_id: &str) {
	if let Some(dl) = inner.doc_len.remove(entity_id) {
		inner.total_len = inner.total_len.saturating_sub(dl as u64);
	} else {
		return;
	}
	let mut empty: Vec<String> = Vec::new();
	for (tok, postings) in inner.postings.iter_mut() {
		postings.remove(entity_id);
		if postings.is_empty() {
			empty.push(tok.clone());
		}
	}
	for tok in empty {
		inner.postings.remove(&tok);
	}
}

fn tokenize(text: &str) -> Vec<String> {
	let mut out = Vec::new();
	let mut cur = String::new();
	for ch in text.chars() {
		if ch.is_alphanumeric() {
			for lc in ch.to_lowercase() {
				cur.push(lc);
			}
		} else if !cur.is_empty() {
			out.push(stem(&cur));
			cur.clear();
		}
	}
	if !cur.is_empty() {
		out.push(stem(&cur));
	}
	out
}

fn stem(t: &str) -> String {
	let s = t;
	for suf in &["ing", "edly", "ed", "ly", "ies", "es", "s"] {
		if s.len() > suf.len() + 2 && s.ends_with(suf) {
			return s[..s.len() - suf.len()].to_string();
		}
	}
	s.to_string()
}
