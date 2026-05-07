#[derive(Clone, Debug)]
pub struct HeapItem {
	pub entity_id: String,
	pub score: f64,
	pub chain: Vec<String>,
}

pub struct BeamHeap {
	items: Vec<HeapItem>,
}

impl Default for BeamHeap {
	fn default() -> Self {
		Self::new()
	}
}

impl BeamHeap {
	pub fn new() -> Self {
		Self { items: Vec::new() }
	}

	pub fn push(&mut self, item: HeapItem) {
		self.items.push(item);
		let mut i = self.items.len() - 1;
		while i > 0 {
			let p = (i - 1) / 2;
			if self.items[i].score <= self.items[p].score {
				break;
			}
			self.items.swap(i, p);
			i = p;
		}
	}

	pub fn pop(&mut self) -> Option<HeapItem> {
		if self.items.is_empty() {
			return None;
		}
		let n = self.items.len() - 1;
		self.items.swap(0, n);
		let top = self.items.pop().unwrap();
		let sz = self.items.len();
		let mut i = 0;
		loop {
			let (l, r) = (2 * i + 1, 2 * i + 2);
			let mut s = i;
			if l < sz && self.items[l].score > self.items[s].score {
				s = l;
			}
			if r < sz && self.items[r].score > self.items[s].score {
				s = r;
			}
			if s == i {
				break;
			}
			self.items.swap(i, s);
			i = s;
		}
		Some(top)
	}

	pub fn len(&self) -> usize {
		self.items.len()
	}

	pub fn is_empty(&self) -> bool {
		self.items.is_empty()
	}
}
