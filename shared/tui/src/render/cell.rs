use super::grapheme::{ClusterId, BLANK, CONTINUATION};

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Color {
	#[default]
	Default,
	Rgb(u8, u8, u8),
	Indexed(u8),
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Attrs(pub u8);

impl Attrs {
	pub const NONE: Attrs = Attrs(0);
	pub const BOLD: Attrs = Attrs(1 << 0);
	pub const DIM: Attrs = Attrs(1 << 1);
	pub const ITALIC: Attrs = Attrs(1 << 2);
	pub const UNDERLINE: Attrs = Attrs(1 << 3);
	pub const INVERSE: Attrs = Attrs(1 << 4);
	pub const STRIKETHROUGH: Attrs = Attrs(1 << 5);

	pub fn contains(self, o: Attrs) -> bool {
		(self.0 & o.0) == o.0
	}
}

impl std::ops::BitOr for Attrs {
	type Output = Attrs;
	fn bitor(self, rhs: Attrs) -> Attrs {
		Attrs(self.0 | rhs.0)
	}
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Cell {
	pub cluster: ClusterId,
	pub width: u8,
	pub fg: Color,
	pub bg: Color,
	pub attrs: Attrs,
}

impl Default for Cell {
	fn default() -> Self {
		Cell {
			cluster: BLANK,
			width: 1,
			fg: Color::Default,
			bg: Color::Default,
			attrs: Attrs::NONE,
		}
	}
}

impl Cell {
	pub fn new(ch: char) -> Self {
		debug_assert!(
			ch.is_ascii(),
			"Cell::new is ASCII-only; use put_str for unicode"
		);
		Cell {
			cluster: ascii_cluster(ch),
			width: 1,
			..Cell::default()
		}
	}

	pub fn style(mut self, fg: Color, bg: Color, attrs: Attrs) -> Self {
		self.fg = fg;
		self.bg = bg;
		self.attrs = attrs;
		self
	}

	pub fn continuation(fg: Color, bg: Color, attrs: Attrs) -> Self {
		Cell {
			cluster: CONTINUATION,
			width: 0,
			fg,
			bg,
			attrs,
		}
	}

	pub fn is_continuation(&self) -> bool {
		self.cluster == CONTINUATION
	}
}

fn ascii_cluster(ch: char) -> ClusterId {
	let b = ch as u32 & 0x7F;
	ASCII_BASE + b
}

pub(crate) const ASCII_BASE: ClusterId = u32::MAX - 128;

pub(crate) fn ascii_from_cluster(id: ClusterId) -> Option<u8> {
	if (ASCII_BASE..CONTINUATION).contains(&id) {
		Some((id - ASCII_BASE) as u8)
	} else {
		None
	}
}
