use super::cell::{Attrs, Cell, Color};
use super::diff::Strategy;
use super::frame::Frame;
use super::grapheme::{GraphemeArena, BLANK, CONTINUATION};

#[derive(Clone, Debug)]
pub struct Snapshot {
	pub size: (u16, u16),
	pub current: Vec<Cell>,
	pub next: Vec<Cell>,
	pub arena_clusters: Vec<String>,
	pub force_full_next: bool,
	pub last_strategy: Strategy,
	pub frame_counter: u64,
}

impl Snapshot {
	pub(crate) fn capture(
		size: (u16, u16),
		current: &Frame,
		next: &Frame,
		arena: &GraphemeArena,
		force_full_next: bool,
		last_strategy: Strategy,
		frame_counter: u64,
	) -> Self {
		let mut arena_clusters = Vec::with_capacity(arena.len());
		for i in 0..arena.len() {
			arena_clusters.push(arena.get(i as u32).to_string());
		}
		Snapshot {
			size,
			current: current.cells.clone(),
			next: next.cells.clone(),
			arena_clusters,
			force_full_next,
			last_strategy,
			frame_counter,
		}
	}

	pub fn arena_len(&self) -> usize {
		self.arena_clusters.len()
	}
}

#[derive(Debug)]
pub struct VtReplay {
	w: u16,
	h: u16,
	pub(crate) cells: Vec<Cell>,
	cursor: (u16, u16),
	fg: Color,
	bg: Color,
	attrs: Attrs,
	arena: GraphemeArena,
}

impl VtReplay {
	pub fn new(w: u16, h: u16) -> Self {
		VtReplay {
			w,
			h,
			cells: vec![Cell::default(); (w as usize) * (h as usize)],
			cursor: (0, 0),
			fg: Color::Default,
			bg: Color::Default,
			attrs: Attrs::NONE,
			arena: GraphemeArena::new(),
		}
	}

	pub fn feed(&mut self, bytes: &[u8]) -> Result<(), ReplayError> {
		let s = std::str::from_utf8(bytes).map_err(|_| ReplayError::InvalidUtf8)?;
		let mut iter = s.chars().peekable();
		while let Some(c) = iter.next() {
			if c == '\x1b' {
				match iter.next() {
					Some('[') => self.handle_csi(&mut iter)?,
					Some(other) => return Err(ReplayError::UnknownEscape(other)),
					None => return Err(ReplayError::TruncatedEscape),
				}
			} else {
				self.put_grapheme_char(c);
			}
		}
		Ok(())
	}

	fn handle_csi(
		&mut self,
		iter: &mut std::iter::Peekable<std::str::Chars<'_>>,
	) -> Result<(), ReplayError> {
		let mut private = false;
		if let Some(&'?') = iter.peek() {
			private = true;
			iter.next();
		}
		let mut params = String::new();
		let final_byte = loop {
			match iter.next() {
				Some(c) if c.is_ascii_digit() || c == ';' => params.push(c),
				Some(c) if c.is_ascii_alphabetic() => break c,
				Some(other) => return Err(ReplayError::InvalidCsiByte(other)),
				None => return Err(ReplayError::TruncatedCsi),
			}
		};
		let nums: Vec<u32> = if params.is_empty() {
			Vec::new()
		} else {
			params
				.split(';')
				.map(|p| p.parse::<u32>().unwrap_or(0))
				.collect()
		};
		match (private, final_byte) {
			(true, 'h') | (true, 'l') => Ok(()),
			(false, 'H') => {
				let row = nums.first().copied().unwrap_or(1).saturating_sub(1) as u16;
				let col = nums.get(1).copied().unwrap_or(1).saturating_sub(1) as u16;
				self.cursor = (
					col.min(self.w.saturating_sub(1)),
					row.min(self.h.saturating_sub(1)),
				);
				Ok(())
			}
			(false, 'J') => {
				let arg = nums.first().copied().unwrap_or(0);
				if arg == 2 {
					for c in &mut self.cells {
						*c = Cell::default();
					}
				}
				Ok(())
			}
			(false, 'm') => {
				self.apply_sgr(&nums);
				Ok(())
			}
			_ => Err(ReplayError::UnknownCsi(final_byte)),
		}
	}

	fn apply_sgr(&mut self, nums: &[u32]) {
		if nums.is_empty() {
			self.fg = Color::Default;
			self.bg = Color::Default;
			self.attrs = Attrs::NONE;
			return;
		}
		let mut i = 0;
		while i < nums.len() {
			let n = nums[i];
			match n {
				0 => {
					self.fg = Color::Default;
					self.bg = Color::Default;
					self.attrs = Attrs::NONE;
				}
				1 => self.attrs = self.attrs | Attrs::BOLD,
				2 => self.attrs = self.attrs | Attrs::DIM,
				3 => self.attrs = self.attrs | Attrs::ITALIC,
				4 => self.attrs = self.attrs | Attrs::UNDERLINE,
				7 => self.attrs = self.attrs | Attrs::INVERSE,
				22 => self.attrs = Attrs(self.attrs.0 & !(Attrs::BOLD.0 | Attrs::DIM.0)),
				23 => self.attrs = Attrs(self.attrs.0 & !Attrs::ITALIC.0),
				24 => self.attrs = Attrs(self.attrs.0 & !Attrs::UNDERLINE.0),
				27 => self.attrs = Attrs(self.attrs.0 & !Attrs::INVERSE.0),
				38 => {
					self.fg = parse_ext_color(nums, &mut i).unwrap_or(Color::Default);
				}
				48 => {
					self.bg = parse_ext_color(nums, &mut i).unwrap_or(Color::Default);
				}
				39 => self.fg = Color::Default,
				49 => self.bg = Color::Default,
				_ => {}
			}
			i += 1;
		}
	}

	fn put_grapheme_char(&mut self, c: char) {
		let mut tmp = [0u8; 4];
		let s: &str = c.encode_utf8(&mut tmp);
		let w = unicode_width::UnicodeWidthStr::width(s) as u8;
		if w == 0 {
			let (cx, cy) = self.cursor;
			let prev_x = cx.saturating_sub(1);
			if let Some(idx) = self.idx(prev_x, cy) {
				let prev = &self.cells[idx];
				if !prev.is_continuation() {
					let base = self.arena.get(prev.cluster).to_string();
					let combined = format!("{}{}", base, s);
					let new_id = self.arena.intern(&combined);
					self.cells[idx].cluster = new_id;
				}
			}
			return;
		}
		let (cx, cy) = self.cursor;
		if cx >= self.w || cy >= self.h {
			return;
		}
		let id = self.arena.intern(s);
		if let Some(idx) = self.idx(cx, cy) {
			self.cells[idx] = Cell {
				cluster: id,
				width: w,
				fg: self.fg,
				bg: self.bg,
				attrs: self.attrs,
			};
			if w == 2 {
				if let Some(cidx) = self.idx(cx + 1, cy) {
					self.cells[cidx] = Cell::continuation(self.fg, self.bg, self.attrs);
				}
			}
		}
		self.cursor.0 = cx.saturating_add(w as u16);
	}

	fn idx(&self, x: u16, y: u16) -> Option<usize> {
		if x >= self.w || y >= self.h {
			return None;
		}
		Some((y as usize) * (self.w as usize) + (x as usize))
	}

	pub fn matches_frame(&self, expected: &Frame, expected_arena: &GraphemeArena) -> bool {
		if (self.w, self.h) != (expected.w, expected.h) {
			return false;
		}
		for (i, c_replay) in self.cells.iter().enumerate() {
			let c_exp = &expected.cells[i];
			if !cell_visually_equal(c_replay, &self.arena, c_exp, expected_arena) {
				return false;
			}
		}
		true
	}
}

fn parse_ext_color(nums: &[u32], i: &mut usize) -> Option<Color> {
	let kind = *nums.get(*i + 1)?;
	match kind {
		5 => {
			let n = *nums.get(*i + 2)? as u8;
			*i += 2;
			Some(Color::Indexed(n))
		}
		2 => {
			let r = *nums.get(*i + 2)? as u8;
			let g = *nums.get(*i + 3)? as u8;
			let b = *nums.get(*i + 4)? as u8;
			*i += 4;
			Some(Color::Rgb(r, g, b))
		}
		_ => None,
	}
}

fn cell_visually_equal(
	a: &Cell,
	a_arena: &GraphemeArena,
	b: &Cell,
	b_arena: &GraphemeArena,
) -> bool {
	if a.fg != b.fg || a.bg != b.bg || a.attrs != b.attrs {
		return false;
	}
	if a.is_continuation() || b.is_continuation() {
		return a.is_continuation() && b.is_continuation();
	}
	if a.width != b.width {
		return false;
	}
	resolve_glyph(a, a_arena) == resolve_glyph(b, b_arena)
}

fn resolve_glyph<'s>(c: &Cell, arena: &'s GraphemeArena) -> std::borrow::Cow<'s, str> {
	use super::cell::ascii_from_cluster;
	if c.cluster == BLANK {
		return std::borrow::Cow::Borrowed(" ");
	}
	if c.cluster == CONTINUATION {
		return std::borrow::Cow::Borrowed("");
	}
	if let Some(b) = ascii_from_cluster(c.cluster) {
		let mut s = String::with_capacity(1);
		s.push(b as char);
		return std::borrow::Cow::Owned(s);
	}
	std::borrow::Cow::Borrowed(arena.get(c.cluster))
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReplayError {
	InvalidUtf8,
	TruncatedEscape,
	TruncatedCsi,
	UnknownEscape(char),
	UnknownCsi(char),
	InvalidCsiByte(char),
}

impl std::fmt::Display for ReplayError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			ReplayError::InvalidUtf8 => write!(f, "invalid utf-8 in byte stream"),
			ReplayError::TruncatedEscape => write!(f, "truncated escape sequence"),
			ReplayError::TruncatedCsi => write!(f, "truncated CSI sequence"),
			ReplayError::UnknownEscape(c) => write!(f, "unknown escape char: {:?}", c),
			ReplayError::UnknownCsi(c) => write!(f, "unknown CSI final byte: {:?}", c),
			ReplayError::InvalidCsiByte(c) => write!(f, "invalid byte in CSI params: {:?}", c),
		}
	}
}

impl std::error::Error for ReplayError {}
