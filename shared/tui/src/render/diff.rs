use super::frame::Frame;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Strategy {
	Full,
	Lines,
	Cells,
	Noop,
}

pub struct Stats {
	pub changed: usize,
	pub total: usize,
	pub changed_rows: u16,
	pub runs: usize,
}

pub fn stats(a: &Frame, b: &Frame) -> Stats {
	debug_assert_eq!(a.w, b.w);
	debug_assert_eq!(a.h, b.h);
	let w = a.w as usize;
	let h = a.h as usize;
	let total = w * h;
	let mut changed = 0usize;
	let mut changed_rows = 0u16;
	let mut runs = 0usize;
	for y in 0..h {
		let mut row_changed = false;
		let mut in_run = false;
		for x in 0..w {
			let i = y * w + x;
			if a.cells[i] != b.cells[i] {
				changed += 1;
				row_changed = true;
				if !in_run {
					runs += 1;
					in_run = true;
				}
			} else {
				in_run = false;
			}
		}
		if row_changed {
			changed_rows += 1;
		}
	}
	Stats {
		changed,
		total,
		changed_rows,
		runs,
	}
}

pub fn pick(s: &Stats) -> Strategy {
	if s.changed == 0 {
		return Strategy::Noop;
	}
	let ratio = s.changed as f32 / s.total.max(1) as f32;
	if ratio > 0.70 {
		Strategy::Full
	} else if ratio > 0.20 || (s.runs > 0 && s.runs as f32 / s.changed_rows.max(1) as f32 > 3.0) {
		Strategy::Lines
	} else {
		Strategy::Cells
	}
}
