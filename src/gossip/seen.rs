use std::sync::Mutex;
use std::time::Instant;

use crate::base::constants::{GOSSIP_SEEN_SET_CAP, GOSSIP_SEEN_TTL};

struct Entry {
	id: String,
	expires: Instant,
}

pub struct SeenSet {
	entries: Mutex<SeenInner>,
}

struct SeenInner {
	buf: Vec<Option<Entry>>,
	head: usize,
}

impl SeenSet {
	pub fn new() -> Self {
		let mut buf = Vec::with_capacity(GOSSIP_SEEN_SET_CAP);
		buf.resize_with(GOSSIP_SEEN_SET_CAP, || None);
		Self {
			entries: Mutex::new(SeenInner { buf, head: 0 }),
		}
	}

	pub fn add_and_check(&self, id: &str) -> bool {
		let mut inner = self.entries.lock().unwrap();
		let now = Instant::now();

		for entry in inner.buf.iter().flatten() {
			if entry.id == id && entry.expires > now {
				return true;
			}
		}

		let head = inner.head;
		inner.buf[head] = Some(Entry {
			id: id.to_string(),
			expires: now + GOSSIP_SEEN_TTL,
		});
		inner.head = (head + 1) % GOSSIP_SEEN_SET_CAP;
		false
	}
}

impl Default for SeenSet {
	fn default() -> Self {
		Self::new()
	}
}
