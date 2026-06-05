use crate::input::{Key, KeyCode, Mods};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyChord {
	pub code: KeyCode,
	pub mods: Mods,
}

impl KeyChord {
	pub const fn new(code: KeyCode, mods: Mods) -> Self {
		Self { code, mods }
	}

	pub fn parse(s: &str) -> Option<Self> {
		let s = s.trim();
		if s.is_empty() {
			return None;
		}
		let parts: Vec<&str> = s.split('+').map(str::trim).collect();
		let mut mods = Mods::NONE;
		for part in &parts[..parts.len().saturating_sub(1)] {
			match part.to_ascii_lowercase().as_str() {
				"ctrl" | "control" => mods |= Mods::CTRL,
				"shift" => mods |= Mods::SHIFT,
				"alt" | "option" => mods |= Mods::ALT,
				_ => return None,
			}
		}
		let key = parts.last()?;
		let code = parse_key_name(key)?;
		Some(Self { code, mods })
	}

	pub fn matches(&self, key: &Key) -> bool {
		if self.mods != key.mods {
			return false;
		}
		match (self.code, key.code) {
			(KeyCode::Char(a), KeyCode::Char(b)) => a.eq_ignore_ascii_case(&b),
			(a, b) => a == b,
		}
	}
}

fn parse_key_name(s: &str) -> Option<KeyCode> {
	let lower = s.to_ascii_lowercase();
	Some(match lower.as_str() {
		"up" => KeyCode::Up,
		"down" => KeyCode::Down,
		"left" => KeyCode::Left,
		"right" => KeyCode::Right,
		"enter" | "return" => KeyCode::Enter,
		"tab" => KeyCode::Tab,
		"esc" | "escape" => KeyCode::Esc,
		"home" => KeyCode::Home,
		"end" => KeyCode::End,
		"pageup" | "pgup" => KeyCode::PageUp,
		"pagedown" | "pgdn" | "pgdown" => KeyCode::PageDown,
		"backspace" | "bs" => KeyCode::Backspace,
		"delete" | "del" => KeyCode::Delete,
		"space" => KeyCode::Char(' '),
		other if other.chars().count() == 1 => KeyCode::Char(other.chars().next().unwrap()),
		_ => return None,
	})
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListAction {
	MoveUp,
	MoveDown,
	Pick,
	Cancel,
	Emit(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormAction {
	NextField,
	PrevField,
	Submit,
	SubmitOrAdvance,
	Cancel,
	Emit(String),
}

#[derive(Debug, Clone)]
pub struct ListBindings {
	table: Vec<(KeyChord, ListAction)>,
}

impl Default for ListBindings {
	fn default() -> Self {
		Self {
			table: vec![
				(KeyChord::new(KeyCode::Up, Mods::NONE), ListAction::MoveUp),
				(KeyChord::new(KeyCode::Down, Mods::NONE), ListAction::MoveDown),
				(KeyChord::new(KeyCode::Enter, Mods::NONE), ListAction::Pick),
				(KeyChord::new(KeyCode::Tab, Mods::NONE), ListAction::Pick),
				(KeyChord::new(KeyCode::Esc, Mods::NONE), ListAction::Cancel),
			],
		}
	}
}

impl ListBindings {
	pub fn new(table: Vec<(KeyChord, ListAction)>) -> Self {
		Self { table }
	}

	pub fn push(&mut self, chord: KeyChord, action: ListAction) {
		self.table.push((chord, action));
	}

	pub fn dispatch(&self, key: &Key) -> Option<&ListAction> {
		self.table.iter().find(|(c, _)| c.matches(key)).map(|(_, a)| a)
	}
}

#[derive(Debug, Clone)]
pub struct FormBindings {
	table: Vec<(KeyChord, FormAction)>,
}

impl Default for FormBindings {
	fn default() -> Self {
		Self {
			table: vec![
				(KeyChord::new(KeyCode::Tab, Mods::NONE), FormAction::NextField),
				(KeyChord::new(KeyCode::Tab, Mods::SHIFT), FormAction::PrevField),
				(KeyChord::new(KeyCode::Enter, Mods::NONE), FormAction::SubmitOrAdvance),
				(KeyChord::new(KeyCode::Enter, Mods::CTRL), FormAction::Submit),
				(KeyChord::new(KeyCode::Esc, Mods::NONE), FormAction::Cancel),
			],
		}
	}
}

impl FormBindings {
	pub fn new(table: Vec<(KeyChord, FormAction)>) -> Self {
		Self { table }
	}

	pub fn push(&mut self, chord: KeyChord, action: FormAction) {
		self.table.push((chord, action));
	}

	pub fn dispatch(&self, key: &Key) -> Option<&FormAction> {
		self.table.iter().find(|(c, _)| c.matches(key)).map(|(_, a)| a)
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[derive(Default)]
pub enum OnPickAction {
	#[default]
 FillBuffer,
	EmitEvent,
	Insert(String),
}

