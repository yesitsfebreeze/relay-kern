use crate::base::util;
use crate::config::RetrievalConfig;
use crate::retrieval::expand::ScoredEntity;
use crate::retrieval::LlmFunc;

pub fn llm_rerank(
	cfg: &RetrievalConfig,
	llm: Option<&LlmFunc>,
	query_text: &str,
	results: &mut Vec<ScoredEntity>,
) {
	if !cfg.rerank_enabled || query_text.is_empty() {
		return;
	}
	let pool_size = cfg.rerank_pool_size.min(results.len());
	let llm_fn = match llm {
		Some(f) => f,
		None => return,
	};
	let pool = pool_size;
	if pool < 2 {
		return;
	}

	let mut prompt = String::from(
		"You are re-ranking search results by relevance to a query. \
		Return ONLY a JSON array of integer indices in best-to-worst order, no prose, no decimal points. \
		Example: [2,0,1,3] — integers only, never [2.0,0.0,1.0,3.0]\n\n",
	);
	prompt.push_str(&format!("Query: {query_text}\n\nCandidates:\n"));
	for (i, st) in results.iter().take(pool).enumerate() {
		let text = st.entity.text();
		let truncated = util::truncate(&text, 300);
		prompt.push_str(&format!("[{i}] {truncated}\n"));
	}
	prompt.push_str("\nRanking (JSON array of indices):");

	let response = llm_fn(&prompt);
	let order = match parse_ranking(&response, pool) {
		Some(o) => o,
		None => return,
	};

	let tail = results.split_off(pool);
	let head = std::mem::take(results);
	let mut reordered: Vec<ScoredEntity> = Vec::with_capacity(pool);
	let mut used = vec![false; head.len()];
	for i in &order {
		if *i < head.len() && !used[*i] {
			used[*i] = true;
			reordered.push(head[*i].clone());
		}
	}
	for (i, st) in head.into_iter().enumerate() {
		if !used[i] {
			reordered.push(st);
		}
	}
	reordered.extend(tail);
	*results = reordered;
}

pub fn parse_ranking(response: &str, pool: usize) -> Option<Vec<usize>> {
	let trimmed = response.trim();
	let start = trimmed.find('[')?;
	let end = trimmed.rfind(']')?;
	if end <= start {
		return None;
	}
	let slice = &trimmed[start..=end];
	let arr: serde_json::Value = serde_json::from_str(slice).ok()?;
	let list = arr.as_array()?;
	let mut out = Vec::with_capacity(list.len());
	for v in list {
		// Accept integer JSON (1) or whole-number float JSON (1.0); reject fractions (1.5).
		let i = v
			.as_i64()
			.or_else(|| v.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64))?
			as usize;
		if i < pool {
			out.push(i);
		}
	}
	if out.is_empty() {
		None
	} else {
		Some(out)
	}
}

#[cfg(test)]
mod tests {
	use super::parse_ranking;

	#[test]
	fn parses_clean_array() {
		assert_eq!(parse_ranking("[2,0,1]", 3), Some(vec![2, 0, 1]));
	}

	#[test]
	fn tolerates_surrounding_prose() {
		assert_eq!(parse_ranking("Ranking: [1,0] done", 2), Some(vec![1, 0]));
	}

	#[test]
	fn filters_out_of_range_indices() {
		// 5 >= pool(2) is dropped; 0 kept.
		assert_eq!(parse_ranking("[5,0]", 2), Some(vec![0]));
	}

	#[test]
	fn negative_index_is_filtered_not_panic() {
		// -1 as usize is huge -> filtered by the `< pool` check, no panic.
		assert_eq!(parse_ranking("[-1,1]", 2), Some(vec![1]));
	}

	#[test]
	fn no_brackets_is_none() {
		assert_eq!(parse_ranking("no ranking here", 3), None);
	}

	#[test]
	fn empty_array_is_none() {
		assert_eq!(parse_ranking("[]", 3), None);
	}

	#[test]
	fn whole_number_floats_accepted() {
		// Some LLMs emit [1.0, 0.0, 2.0] instead of [1, 0, 2] — still valid.
		assert_eq!(parse_ranking("[1.0, 0.0, 2.0]", 3), Some(vec![1, 0, 2]));
	}

	#[test]
	fn fractional_float_discards_ranking() {
		// 1.5 is not a valid index — bail the whole ranking (don't trust partial).
		assert_eq!(parse_ranking("[1.5, 0]", 3), None);
	}
}
