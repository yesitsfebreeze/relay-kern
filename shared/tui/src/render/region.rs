use super::cell::{Attrs, Cell, Color};
use super::frame::Frame;
use super::grapheme::GraphemeArena;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct Region {
	pub x: u16,
	pub y: u16,
	pub w: u16,
	pub h: u16,
}

impl Region {
	pub const fn new(x: u16, y: u16, w: u16, h: u16) -> Self {
		Region { x, y, w, h }
	}

	pub const fn from_frame(w: u16, h: u16) -> Self {
		Region { x: 0, y: 0, w, h }
	}

	pub const fn is_empty(&self) -> bool {
		self.w == 0 || self.h == 0
	}

	pub const fn right(&self) -> u16 {
		self.x.saturating_add(self.w)
	}

	pub const fn bottom(&self) -> u16 {
		self.y.saturating_add(self.h)
	}

	pub const fn contains(&self, fx: u16, fy: u16) -> bool {
		fx >= self.x && fx < self.right() && fy >= self.y && fy < self.bottom()
	}

	pub fn split_h(self, left_w: u16) -> (Region, Region) {
		let lw = left_w.min(self.w);
		let left = Region {
			x: self.x,
			y: self.y,
			w: lw,
			h: self.h,
		};
		let right = Region {
			x: self.x.saturating_add(lw),
			y: self.y,
			w: self.w - lw,
			h: self.h,
		};
		(left, right)
	}

	pub fn split_v(self, top_h: u16) -> (Region, Region) {
		let th = top_h.min(self.h);
		let top = Region {
			x: self.x,
			y: self.y,
			w: self.w,
			h: th,
		};
		let bottom = Region {
			x: self.x,
			y: self.y.saturating_add(th),
			w: self.w,
			h: self.h - th,
		};
		(top, bottom)
	}

	pub fn center(self, w: u16, h: u16) -> Region {
		let cw = w.min(self.w);
		let ch = h.min(self.h);
		Region {
			x: self.x + (self.w - cw) / 2,
			y: self.y + (self.h - ch) / 2,
			w: cw,
			h: ch,
		}
	}

	pub fn pad(self, left: u16, top: u16, right: u16, bottom: u16) -> Region {
		let dx = left.min(self.w);
		let dy = top.min(self.h);
		let w = self.w.saturating_sub(dx).saturating_sub(right);
		let h = self.h.saturating_sub(dy).saturating_sub(bottom);
		Region {
			x: self.x + dx,
			y: self.y + dy,
			w,
			h,
		}
	}
}

pub struct FrameView<'a> {
	frame: &'a mut Frame,
	arena: &'a mut GraphemeArena,
	region: Region,
}

impl<'a> FrameView<'a> {
	pub fn new(frame: &'a mut Frame, arena: &'a mut GraphemeArena, region: Region) -> Self {
		let region = clamp_to_frame(region, frame.w, frame.h);
		FrameView {
			frame,
			arena,
			region,
		}
	}

	pub const fn region(&self) -> Region {
		self.region
	}

	pub const fn width(&self) -> u16 {
		self.region.w
	}

	pub const fn height(&self) -> u16 {
		self.region.h
	}

	pub fn sub(&mut self, sub: Region) -> FrameView<'_> {
		let abs = Region {
			x: self.region.x.saturating_add(sub.x),
			y: self.region.y.saturating_add(sub.y),
			w: sub.w,
			h: sub.h,
		};
		let clipped = intersect(abs, self.region);
		FrameView {
			frame: self.frame,
			arena: self.arena,
			region: clipped,
		}
	}

	pub fn set(&mut self, x: u16, y: u16, cell: Cell) {
		if x >= self.region.w || y >= self.region.h {
			return;
		}
		self.frame.set(self.region.x + x, self.region.y + y, cell);
	}

	pub fn get(&self, x: u16, y: u16) -> Option<&Cell> {
		if x >= self.region.w || y >= self.region.h {
			return None;
		}
		self.frame.get(self.region.x + x, self.region.y + y)
	}

	pub fn fill(&mut self, cell: Cell) {
		for dy in 0..self.region.h {
			for dx in 0..self.region.w {
				self.frame.set(self.region.x + dx, self.region.y + dy, cell);
			}
		}
	}

	pub fn put_str(&mut self, x: u16, y: u16, s: &str, fg: Color, bg: Color, attrs: Attrs) {
		if y >= self.region.h {
			return;
		}
		use super::grapheme::BLANK;
		use unicode_segmentation::UnicodeSegmentation;
		use unicode_width::UnicodeWidthStr;
		let mut cx = x;
		for g in s.graphemes(true) {
			let w = UnicodeWidthStr::width(g) as u16;
			if w == 0 {
				continue;
			}
			if cx >= self.region.w {
				break;
			}
			if cx + w > self.region.w {
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
			let id = self.arena.intern(g);
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

fn clamp_to_frame(r: Region, fw: u16, fh: u16) -> Region {
	intersect(r, Region::from_frame(fw, fh))
}

fn intersect(a: Region, b: Region) -> Region {
	let x0 = a.x.max(b.x);
	let y0 = a.y.max(b.y);
	let x1 = a.right().min(b.right());
	let y1 = a.bottom().min(b.bottom());
	if x1 <= x0 || y1 <= y0 {
		Region {
			x: x0,
			y: y0,
			w: 0,
			h: 0,
		}
	} else {
		Region {
			x: x0,
			y: y0,
			w: x1 - x0,
			h: y1 - y0,
		}
	}
}
