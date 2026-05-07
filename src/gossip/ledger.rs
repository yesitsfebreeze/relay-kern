use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::RwLock;
use std::time::Instant;

use crate::base::constants::{LEDGER_ROUTING_TTL, LEDGER_THOUGHT_TTL};

const DEFAULT_LEDGER_CAP: usize = 10_000;

struct Entry {
	addr: String,
	expires: Instant,
}

pub struct Ledger {
	entities: RwLock<HashMap<String, Entry>>,
	routing: RwLock<HashMap<String, Entry>>,
	max_entries: AtomicUsize,
}

impl Ledger {
	pub fn new() -> Self {
		Self {
			entities: RwLock::new(HashMap::new()),
			routing: RwLock::new(HashMap::new()),
			max_entries: AtomicUsize::new(DEFAULT_LEDGER_CAP),
		}
	}

	pub fn set_max_entries(&self, cap: usize) {
		self.max_entries.store(cap.max(1), Ordering::Relaxed);
	}

	fn cap(&self) -> usize {
		self.max_entries.load(Ordering::Relaxed)
	}

	pub fn put_thought(&self, id: &str, addr: &str) {
		let mut m = self.entities.write().unwrap();
		evict_if_full(&mut m, self.cap());
		m.insert(
			id.to_string(),
			Entry {
				addr: addr.to_string(),
				expires: Instant::now() + LEDGER_THOUGHT_TTL,
			},
		);
	}

	pub fn put_routing(&self, kern_id: &str, addr: &str) {
		let mut m = self.routing.write().unwrap();
		evict_if_full(&mut m, self.cap());
		m.insert(
			kern_id.to_string(),
			Entry {
				addr: addr.to_string(),
				expires: Instant::now() + LEDGER_ROUTING_TTL,
			},
		);
	}

	pub fn lookup_thought(&self, id: &str) -> Option<String> {
		let m = self.entities.read().unwrap();
		m.get(id).and_then(|e| {
			if e.expires > Instant::now() {
				Some(e.addr.clone())
			} else {
				None
			}
		})
	}

	pub fn lookup_routing(&self, kern_id: &str) -> Option<String> {
		let m = self.routing.read().unwrap();
		m.get(kern_id).and_then(|e| {
			if e.expires > Instant::now() {
				Some(e.addr.clone())
			} else {
				None
			}
		})
	}
}

impl Default for Ledger {
	fn default() -> Self {
		Self::new()
	}
}

// Drop the entry with the soonest expiry when at-or-over capacity. Cheaper
// than a sweep and keeps the freshest TTLs around. Called on every insert
// path so the map cannot exceed `cap` after the next put.
fn evict_if_full(m: &mut HashMap<String, Entry>, cap: usize) {
	while m.len() >= cap {
		let oldest_key = m.iter().min_by_key(|(_, e)| e.expires).map(|(k, _)| k.clone());
		match oldest_key {
			Some(k) => {
				m.remove(&k);
			}
			None => break,
		}
	}
}
