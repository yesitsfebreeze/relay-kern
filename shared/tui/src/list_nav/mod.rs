#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelCursor {
	sel: usize,
	len: usize,
}

impl SelCursor {
	pub fn new(len: usize) -> Self {
		Self { sel: 0, len }
	}

	pub fn sel(&self) -> usize {
		self.sel
	}

	pub fn len(&self) -> usize {
		self.len
	}

	pub fn is_empty(&self) -> bool {
		self.len == 0
	}

	pub fn move_up(&mut self) {
		if self.sel > 0 {
			self.sel -= 1;
		}
	}

	pub fn move_down(&mut self) {
		if self.sel + 1 < self.len {
			self.sel += 1;
		}
	}

	pub fn set_sel(&mut self, sel: usize) {
		self.sel = sel.min(self.len.saturating_sub(1));
	}
}

#[cfg(test)]
mod tests;
