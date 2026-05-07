use std::collections::HashSet;

pub fn ndcg_at_k(ranked_ids: &[String], expected_ids: &[String], k: usize) -> f64 {
	if expected_ids.is_empty() || k == 0 {
		return 0.0;
	}
	let expected: HashSet<&str> = expected_ids.iter().map(String::as_str).collect();

	let mut dcg = 0.0;
	for (i, id) in ranked_ids.iter().take(k).enumerate() {
		if expected.contains(id.as_str()) {
			dcg += 1.0 / ((i + 2) as f64).log2();
		}
	}

	let ideal_hits = expected_ids.len().min(k);
	let mut idcg = 0.0;
	for i in 0..ideal_hits {
		idcg += 1.0 / ((i + 2) as f64).log2();
	}
	if idcg == 0.0 {
		return 0.0;
	}
	dcg / idcg
}

pub fn mean_ndcg<I>(results: I, k: usize) -> f64
where
	I: IntoIterator<Item = (Vec<String>, Vec<String>)>,
{
	let mut sum = 0.0;
	let mut n = 0;
	for (ranked, expected) in results {
		sum += ndcg_at_k(&ranked, &expected, k);
		n += 1;
	}
	if n == 0 { 0.0 } else { sum / n as f64 }
}
