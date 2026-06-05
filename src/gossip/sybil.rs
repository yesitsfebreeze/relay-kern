use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::base::locks::lock_recovered;
use crate::base::search::EntityHit;
use crate::base::util::cmp_partial;

pub struct RateClipper {
	state: Mutex<HashMap<String, PeerBucket>>,
	max_per_window: u64,
	window: Duration,
	dropped: AtomicU64,
}

#[derive(Clone, Copy)]
struct PeerBucket {
	count: u64,
	window_start: Instant,
}

impl RateClipper {
	pub fn new(max_per_window: u64, window: Duration) -> Self {
		Self {
			state: Mutex::new(HashMap::new()),
			max_per_window,
			window,
			dropped: AtomicU64::new(0),
		}
	}

	pub fn admit(&self, peer: &str) -> bool {
		self.admit_at(peer, Instant::now())
	}

	pub fn admit_at(&self, peer: &str, now: Instant) -> bool {
		if self.max_per_window == 0 {
			return true;
		}
		let mut state = lock_recovered(&self.state);
		let bucket = state.entry(peer.to_string()).or_insert(PeerBucket {
			count: 0,
			window_start: now,
		});
		if now.duration_since(bucket.window_start) >= self.window {
			bucket.count = 0;
			bucket.window_start = now;
		}
		if bucket.count >= self.max_per_window {
			self.dropped.fetch_add(1, Ordering::Relaxed);
			return false;
		}
		bucket.count += 1;
		true
	}

	pub fn dropped_count(&self) -> u64 {
		self.dropped.load(Ordering::Relaxed)
	}
}

pub fn trimmed_mean(values: &[f64], trim_pct: f64) -> Option<f64> {
	if values.is_empty() {
		return None;
	}
	let pct = trim_pct.clamp(0.0, 0.4999);
	let n = values.len();
	let k = ((n as f64) * pct).floor() as usize;
	if 2 * k >= n {
		return None;
	}
	let mut sorted: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
	if sorted.is_empty() {
		return None;
	}
	sorted.sort_by(|a, b| cmp_partial(a, b));
	let m = sorted.len();
	let k = ((m as f64) * pct).floor() as usize;
	if 2 * k >= m {
		return None;
	}
	let slice = &sorted[k..m - k];
	let sum: f64 = slice.iter().sum();
	Some(sum / slice.len() as f64)
}

pub fn trimmed_mean_merge_hits(
	per_peer: &[&[EntityHit]],
	trim_pct: f64,
	top_k: usize,
) -> Vec<EntityHit> {
	let mut acc: HashMap<String, Vec<f64>> = HashMap::new();
	for list in per_peer {
		for hit in list.iter() {
			acc
				.entry(hit.entity_id.clone())
				.or_default()
				.push(hit.score);
		}
	}
	let mut out: Vec<EntityHit> = acc
		.into_iter()
		.filter_map(|(id, scores)| {
			let merged = trimmed_mean(&scores, trim_pct).or_else(|| {
				let finite: Vec<f64> = scores.iter().copied().filter(|v| v.is_finite()).collect();
				if finite.is_empty() {
					None
				} else {
					Some(finite.iter().sum::<f64>() / finite.len() as f64)
				}
			})?;
			Some(EntityHit {
				entity_id: id,
				score: merged,
			})
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
