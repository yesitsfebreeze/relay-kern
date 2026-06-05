use std::collections::HashMap;

pub type ClusterId = u32;

pub const BLANK: ClusterId = 0;
pub const CONTINUATION: ClusterId = u32::MAX;

/// Soft cap on distinct grapheme clusters retained in the arena.
///
/// The arena grows monotonically as new clusters are seen; nothing is evicted
/// in the normal `intern` path because live ClusterIds are referenced from
/// both `current` and `next` frames across renderer flushes. To bound memory
/// in long-running TUI sessions with diverse text, callers should observe
/// `over_capacity()` at a safe boundary (e.g. right after a `Full` repaint,
/// where `current` will be discarded on the next full emit) and invoke
/// `reset()` then. After reset, ALL existing ClusterIds (other than `BLANK`
/// and `CONTINUATION`) are invalidated; the caller is responsible for
/// forcing a full repaint so stale ids in `current` are not read.
pub const MAX_CLUSTERS: usize = 8192;

#[derive(Debug)]
pub struct GraphemeArena {
	bytes: Vec<u8>,
	spans: Vec<(u32, u16)>,
	lookup: HashMap<Vec<u8>, ClusterId>,
}

impl GraphemeArena {
	pub fn new() -> Self {
		let mut a = GraphemeArena {
			bytes: Vec::with_capacity(256),
			spans: Vec::with_capacity(32),
			lookup: HashMap::with_capacity(32),
		};
		let id = a.intern(" ");
		debug_assert_eq!(id, BLANK);
		a
	}

	pub fn intern(&mut self, cluster: &str) -> ClusterId {
		if cluster.is_empty() {
			return BLANK;
		}
		if let Some(&id) = self.lookup.get(cluster.as_bytes()) {
			return id;
		}
		let start = self.bytes.len() as u32;
		let len = cluster.len() as u16;
		self.bytes.extend_from_slice(cluster.as_bytes());
		let id = self.spans.len() as ClusterId;
		self.spans.push((start, len));
		self.lookup.insert(cluster.as_bytes().to_vec(), id);
		id
	}

	pub fn get(&self, id: ClusterId) -> &str {
		if id == CONTINUATION {
			return "";
		}
		let i = id as usize;
		if i >= self.spans.len() {
			return "";
		}
		let (start, len) = self.spans[i];
		let s = start as usize;
		let e = s + len as usize;
		std::str::from_utf8(&self.bytes[s..e]).unwrap_or("")
	}

	pub fn len(&self) -> usize {
		self.spans.len()
	}

	pub fn is_empty(&self) -> bool {
		self.spans.is_empty()
	}

	/// Returns true when the arena has grown past the soft cap and should be
	/// reset at the next safe boundary.
	pub fn over_capacity(&self) -> bool {
		self.spans.len() > MAX_CLUSTERS
	}

	/// Hard reset: clears all interned clusters and re-seeds `BLANK` so id 0
	/// remains valid. INVALIDATES every other previously issued ClusterId.
	/// Caller must ensure no live frame data is read with the old ids before
	/// the next repopulation (e.g. force a full repaint).
	pub fn reset(&mut self) {
		self.bytes.clear();
		self.spans.clear();
		self.lookup.clear();
		let id = self.intern(" ");
		debug_assert_eq!(id, BLANK);
	}
}

impl Default for GraphemeArena {
	fn default() -> Self {
		Self::new()
	}
}
