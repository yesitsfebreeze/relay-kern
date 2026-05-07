use crate::config::RetrievalConfig;
use crate::retrieval::{EmbedFunc, LlmFunc};

pub fn expand_query(
	cfg: &RetrievalConfig,
	llm: Option<&LlmFunc>,
	embed: Option<&EmbedFunc>,
	query_vec: &[f64],
	query_text: &str,
) -> Vec<f64> {
if !cfg.hyde_enabled {
		return query_vec.to_vec();
	}
	let tokens = query_text.split_whitespace().count();
	if tokens == 0 || tokens >= cfg.hyde_min_query_tokens {
		return query_vec.to_vec();
	}
	let (llm_fn, embed_fn) = match (llm, embed) {
		(Some(l), Some(e)) => (l, e),
		_ => return query_vec.to_vec(),
	};

	let prompt = format!("Write one paragraph that would answer: {query_text}");
	let hypo = llm_fn(&prompt);
	if hypo.trim().is_empty() {
		return query_vec.to_vec();
	}
	let hypo_vec = match embed_fn(&hypo) {
		Ok(v) => v,
		Err(_) => return query_vec.to_vec(),
	};
	if hypo_vec.len() != query_vec.len() || hypo_vec.is_empty() {
		return query_vec.to_vec();
	}

	let mut fused: Vec<f64> = query_vec
		.iter()
		.zip(hypo_vec.iter())
		.map(|(a, b)| (a + b) * 0.5)
		.collect();
	l2_normalize(&mut fused);
	fused
}

fn l2_normalize(v: &mut [f64]) {
let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
	if norm > 0.0 {
		for x in v.iter_mut() {
			*x /= norm;
		}
	}
}
