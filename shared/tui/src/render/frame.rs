use super::cell::{Attrs, Cell, Color};
use super::grapheme::{ClusterId, GraphemeArena, BLANK, CONTINUATION};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Debug)]
pub struct Frame {
	pub w: u16,
	pub h: u16,
	pub cells: Vec<Cell>,
}

impl Frame {
	pub fn new(w: u16, h: u16) -> Self {
		let len = (w as usize) * (h as usize);
		Frame {
			w,
			h,
			cells: vec![Cell::default(); len],
		}
	}

	pub fn resize(&mut self, w: u16, h: u16) {
		self.w = w;
		self.h = h;
		self.cells.clear();
		self
			.cells
			.resize((w as usize) * (h as usize), Cell::default());
	}

	pub fn clear(&mut self) {
		for c in &mut self.cells {
			*c = Cell::default();
		}
	}

	pub fn fill(&mut self, cell: Cell) {
		for c in &mut self.cells {
			*c = cell;
		}
	}

	#[inline]
	fn idx(&self, x: u16, y: u16) -> Option<usize> {
		if x >= self.w || y >= self.h {
			return None;
		}
		Some((y as usize) * (self.w as usize) + (x as usize))
	}

	pub fn set(&mut self, x: u16, y: u16, cell: Cell) {
		if let Some(i) = self.idx(x, y) {
			self.cells[i] = cell;
		}
	}

	pub fn get(&self, x: u16, y: u16) -> Option<&Cell> {
		self.idx(x, y).map(|i| &self.cells[i])
	}

	#[allow(clippy::too_many_arguments)]
	pub fn put_str(
		&mut self,
		arena: &mut GraphemeArena,
		x: u16,
		y: u16,
		s: &str,
		fg: Color,
		bg: Color,
		attrs: Attrs,
	) {
		let mut cx = x;
		for g in s.graphemes(true) {
			let w = UnicodeWidthStr::width(g) as u16;
			if w == 0 {
				continue;
			}
			if cx >= self.w {
				break;
			}
			if cx + w > self.w {
				self.set(
					cx,
					y,
					Cell {
						cluster: BLANK,
						width: 1,
						fg,
						bg,
						attrs,
					},
				);
				break;
			}
			let id = arena.intern(g);
			self.set(
				cx,
				y,
				Cell {
					cluster: id,
					width: w as u8,
					fg,
					bg,
					attrs,
				},
			);
			if w == 2 {
				self.set(cx + 1, y, Cell::continuation(fg, bg, attrs));
			}
			cx = cx.saturating_add(w);
		}
	}
}

pub fn is_continuation_at(f: &Frame, x: u16, y: u16) -> bool {
	f.get(x, y)
		.map(|c| c.cluster == CONTINUATION)
		.unwrap_or(false)
}

pub fn cluster_at(f: &Frame, x: u16, y: u16) -> Option<ClusterId> {
	f.get(x, y).map(|c| c.cluster)
}
