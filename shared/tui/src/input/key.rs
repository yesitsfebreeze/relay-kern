use crossterm::event::{KeyCode as XKeyCode, KeyEvent, KeyModifiers};

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mods(u8);

impl Mods {
	pub const NONE: Mods = Mods(0);
	pub const SHIFT: Mods = Mods(1 << 0);
	pub const CTRL: Mods = Mods(1 << 1);
	pub const ALT: Mods = Mods(1 << 2);
	pub const SUPER: Mods = Mods(1 << 3);

	pub const fn contains(self, other: Mods) -> bool {
		(self.0 & other.0) == other.0
	}

	pub const fn union(self, other: Mods) -> Mods {
		Mods(self.0 | other.0)
	}

	pub const fn bits(self) -> u8 {
		self.0
	}
}

impl std::ops::BitOr for Mods {
	type Output = Mods;
	fn bitor(self, rhs: Mods) -> Mods {
		self.union(rhs)
	}
}

impl std::ops::BitOrAssign for Mods {
	fn bitor_assign(&mut self, rhs: Mods) {
		self.0 |= rhs.0;
	}
}

impl From<KeyModifiers> for Mods {
	fn from(m: KeyModifiers) -> Self {
		let mut out = Mods::NONE;
		if m.contains(KeyModifiers::SHIFT) {
			out |= Mods::SHIFT;
		}
		if m.contains(KeyModifiers::CONTROL) {
			out |= Mods::CTRL;
		}
		if m.contains(KeyModifiers::ALT) {
			out |= Mods::ALT;
		}
		if m.contains(KeyModifiers::SUPER) {
			out |= Mods::SUPER;
		}
		out
	}
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum KeyCode {
	Char(char),
	Enter,
	Esc,
	Backspace,
	Delete,
	Tab,
	BackTab,
	Left,
	Right,
	Up,
	Down,
	Home,
	End,
	PageUp,
	PageDown,
	F(u8),
	Insert,
	Other,
}

impl From<XKeyCode> for KeyCode {
	fn from(c: XKeyCode) -> Self {
		match c {
			XKeyCode::Char(ch) => KeyCode::Char(ch),
			XKeyCode::Enter => KeyCode::Enter,
			XKeyCode::Esc => KeyCode::Esc,
			XKeyCode::Backspace => KeyCode::Backspace,
			XKeyCode::Delete => KeyCode::Delete,
			XKeyCode::Tab => KeyCode::Tab,
			XKeyCode::BackTab => KeyCode::BackTab,
			XKeyCode::Left => KeyCode::Left,
			XKeyCode::Right => KeyCode::Right,
			XKeyCode::Up => KeyCode::Up,
			XKeyCode::Down => KeyCode::Down,
			XKeyCode::Home => KeyCode::Home,
			XKeyCode::End => KeyCode::End,
			XKeyCode::PageUp => KeyCode::PageUp,
			XKeyCode::PageDown => KeyCode::PageDown,
			XKeyCode::F(n) => KeyCode::F(n),
			XKeyCode::Insert => KeyCode::Insert,
			_ => KeyCode::Other,
		}
	}
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Key {
	pub code: KeyCode,
	pub mods: Mods,
}

impl Key {
	pub const fn new(code: KeyCode) -> Self {
		Key {
			code,
			mods: Mods::NONE,
		}
	}

	pub const fn with(code: KeyCode, mods: Mods) -> Self {
		Key { code, mods }
	}

	pub const fn ctrl_char(ch: char) -> Self {
		Key {
			code: KeyCode::Char(ch),
			mods: Mods::CTRL,
		}
	}

	pub const fn alt_char(ch: char) -> Self {
		Key {
			code: KeyCode::Char(ch),
			mods: Mods::ALT,
		}
	}
}

impl From<KeyEvent> for Key {
	fn from(k: KeyEvent) -> Self {
		Key {
			code: k.code.into(),
			mods: k.modifiers.into(),
		}
	}
}

impl From<&KeyEvent> for Key {
	fn from(k: &KeyEvent) -> Self {
		Key {
			code: k.code.into(),
			mods: k.modifiers.into(),
		}
	}
}
