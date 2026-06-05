use super::cell::{Attrs, Color};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum StyleRole {
	Text,
	Muted,
	Accent,
	Ok,
	Warn,
	Error,
	Selected,
	Border,
	Focus,
}

impl StyleRole {
	pub const ALL: [StyleRole; 9] = [
		StyleRole::Text,
		StyleRole::Muted,
		StyleRole::Accent,
		StyleRole::Ok,
		StyleRole::Warn,
		StyleRole::Error,
		StyleRole::Selected,
		StyleRole::Border,
		StyleRole::Focus,
	];

	#[inline]
	fn index(self) -> usize {
		match self {
			StyleRole::Text => 0,
			StyleRole::Muted => 1,
			StyleRole::Accent => 2,
			StyleRole::Ok => 3,
			StyleRole::Warn => 4,
			StyleRole::Error => 5,
			StyleRole::Selected => 6,
			StyleRole::Border => 7,
			StyleRole::Focus => 8,
		}
	}

	pub const fn fg(self) -> Color {
		match self {
			StyleRole::Text     => Color::Indexed(7),
			StyleRole::Muted    => Color::Indexed(8),
			StyleRole::Accent   => Color::Indexed(5),
			StyleRole::Ok       => Color::Indexed(2),
			StyleRole::Warn     => Color::Indexed(3),
			StyleRole::Error    => Color::Indexed(1),
			StyleRole::Selected => Color::Indexed(15),
			StyleRole::Border   => Color::Indexed(8),
			StyleRole::Focus    => Color::Indexed(13),
		}
	}

	pub const fn bg(self) -> Color {
		match self {
			StyleRole::Selected => Color::Indexed(5),
			_ => Color::Default,
		}
	}

	pub const fn attrs(self) -> Attrs {
		match self {
			StyleRole::Muted    => Attrs::DIM,
			StyleRole::Selected => Attrs::BOLD,
			StyleRole::Focus    => Attrs::BOLD,
			_ => Attrs::NONE,
		}
	}
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Style {
	pub fg: Color,
	pub bg: Color,
	pub attrs: Attrs,
}

impl Style {
	pub const fn new(fg: Color, bg: Color, attrs: Attrs) -> Self {
		Style { fg, bg, attrs }
	}

	pub const fn fg(fg: Color) -> Self {
		Style {
			fg,
			bg: Color::Default,
			attrs: Attrs::NONE,
		}
	}
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StyleSet {
	styles: [Style; 9],
}

impl StyleSet {
	pub fn get(&self, role: StyleRole) -> Style {
		self.styles[role.index()]
	}

	pub fn set(&mut self, role: StyleRole, style: Style) {
		self.styles[role.index()] = style;
	}

	pub const fn default_ansi() -> Self {
		use Color::Indexed;
		StyleSet {
			styles: [
				Style::new(Indexed(7), Color::Default, Attrs::NONE),
				Style::new(Indexed(8), Color::Default, Attrs::DIM),
				Style::new(Indexed(5), Color::Default, Attrs::NONE),
				Style::new(Indexed(2), Color::Default, Attrs::NONE),
				Style::new(Indexed(3), Color::Default, Attrs::NONE),
				Style::new(Indexed(1), Color::Default, Attrs::NONE),
				Style::new(Indexed(15), Indexed(5), Attrs::BOLD),
				Style::new(Indexed(8), Color::Default, Attrs::NONE),
				Style::new(Indexed(13), Color::Default, Attrs::BOLD),
			],
		}
	}

	pub const fn dark_rgb() -> Self {
		use Color::Rgb;
		let bg = Rgb(18, 18, 24);
		StyleSet {
			styles: [
				Style::new(Rgb(220, 225, 240), bg, Attrs::NONE),
				Style::new(Rgb(120, 130, 160), bg, Attrs::NONE),
				Style::new(Rgb(80, 160, 240), bg, Attrs::NONE),
				Style::new(Rgb(120, 210, 140), bg, Attrs::NONE),
				Style::new(Rgb(230, 190, 100), bg, Attrs::NONE),
				Style::new(Rgb(235, 105, 110), bg, Attrs::NONE),
				Style::new(Rgb(255, 255, 255), Rgb(80, 160, 240), Attrs::BOLD),
				Style::new(Rgb(90, 100, 130), bg, Attrs::NONE),
				Style::new(Rgb(120, 220, 230), bg, Attrs::BOLD),
			],
		}
	}
}

impl Default for StyleSet {
	fn default() -> Self {
		Self::default_ansi()
	}
}
