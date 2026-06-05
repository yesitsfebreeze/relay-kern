use crate::input::{Key, KeyCode, Mods};
use crate::render::{Attrs, Cell, Color, FrameView, StyleRole};

use super::binding::{FormAction, FormBindings};
use super::list::ListState;

#[derive(Debug, Clone)]
pub enum FormField {
	Input {
		name: String,
		label: String,
		value: String,
		placeholder: String,
		masked: bool,
		cursor: usize,
	},
	Picker {
		name: String,
		label: String,
		list: ListState,
	},
}

impl FormField {
	pub fn input(name: impl Into<String>, label: impl Into<String>) -> Self {
		FormField::Input {
			name: name.into(),
			label: label.into(),
			value: String::new(),
			placeholder: String::new(),
			masked: false,
			cursor: 0,
		}
	}

	pub fn with_placeholder(mut self, p: impl Into<String>) -> Self {
		if let FormField::Input { placeholder, .. } = &mut self {
			*placeholder = p.into();
		}
		self
	}

	pub fn with_value(mut self, v: impl Into<String>) -> Self {
		if let FormField::Input { value, cursor, .. } = &mut self {
			*value = v.into();
			*cursor = value.len();
		}
		self
	}

	pub fn masked(mut self) -> Self {
		if let FormField::Input { masked, .. } = &mut self {
			*masked = true;
		}
		self
	}

	pub fn picker(name: impl Into<String>, label: impl Into<String>, list: ListState) -> Self {
		FormField::Picker {
			name: name.into(),
			label: label.into(),
			list,
		}
	}

	pub fn name(&self) -> &str {
		match self {
			FormField::Input { name, .. } | FormField::Picker { name, .. } => name,
		}
	}

	pub fn value(&self) -> String {
		match self {
			FormField::Input { value, .. } => value.clone(),
			FormField::Picker { list, .. } => list.selected_id().unwrap_or("").to_string(),
		}
	}

	fn height(&self, active: bool) -> u16 {
		match self {
			FormField::Input { .. } => 1,
			FormField::Picker { list, .. } => {
				if active {
					1u16.saturating_add(list.preferred_height())
				} else {
					1
				}
			}
		}
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormOutcome {
	Continue,
	Cancel,
	Submit,
}

#[derive(Debug, Clone)]
pub struct FormState {
	title: String,
	fields: Vec<FormField>,
	active: usize,
	bindings: FormBindings,
}

impl FormState {
	pub fn new(title: impl Into<String>, fields: Vec<FormField>) -> Self {
		Self {
			title: title.into(),
			fields,
			active: 0,
			bindings: FormBindings::default(),
		}
	}

	pub fn with_bindings(mut self, bindings: FormBindings) -> Self {
		self.bindings = bindings;
		self
	}

	pub fn set_bindings(&mut self, bindings: FormBindings) {
		self.bindings = bindings;
	}

	pub fn bindings(&self) -> &FormBindings {
		&self.bindings
	}

	pub fn title(&self) -> &str {
		&self.title
	}

	pub fn fields(&self) -> &[FormField] {
		&self.fields
	}

	pub fn active(&self) -> usize {
		self.active
	}

	pub fn values(&self) -> Vec<(String, String)> {
		self
			.fields
			.iter()
			.map(|f| (f.name().to_string(), f.value()))
			.collect()
	}

	fn focus_next(&mut self) {
		if self.fields.is_empty() {
			return;
		}
		self.active = (self.active + 1) % self.fields.len();
	}

	fn focus_prev(&mut self) {
		if self.fields.is_empty() {
			return;
		}
		if self.active == 0 {
			self.active = self.fields.len() - 1;
		} else {
			self.active -= 1;
		}
	}

	pub fn preferred_height(&self) -> u16 {
		let mut h: u16 = 2;
		for (i, f) in self.fields.iter().enumerate() {
			h = h.saturating_add(f.height(i == self.active));
			h = h.saturating_add(1);
		}
		h.saturating_add(1)
	}

	pub fn handle_input(&mut self, key: &Key) -> FormOutcome {
		if let Some(action) = self.bindings.dispatch(key).cloned() {
			match action {
				FormAction::Cancel => return FormOutcome::Cancel,
				FormAction::PrevField => {
					self.focus_prev();
					return FormOutcome::Continue;
				}
				FormAction::NextField => {
					self.focus_next();
					return FormOutcome::Continue;
				}
				FormAction::SubmitOrAdvance => {
					if self.active + 1 >= self.fields.len() {
						return FormOutcome::Submit;
					}
					self.focus_next();
					return FormOutcome::Continue;
				}
				FormAction::Submit => return FormOutcome::Submit,
				FormAction::Emit(_) => {
					return FormOutcome::Continue;
				}
			}
		}
		if let Some(field) = self.fields.get_mut(self.active) {
			match field {
				FormField::Input {
					value,
					cursor,
					..
				} => handle_input_field(value, cursor, key),
				FormField::Picker { list, .. } => match key.code {
					KeyCode::Up => list.move_up(),
					KeyCode::Down => list.move_down(),
					KeyCode::Char(c) => {
						let mut q = list
							.filter_query()
							.to_string();
						q.push(c);
						list.set_filter(&q);
					}
					KeyCode::Backspace => {
						let mut q = list.filter_query().to_string();
						q.pop();
						list.set_filter(&q);
					}
					_ => {}
				},
			}
		}
		FormOutcome::Continue
	}

	pub fn handle_paste(&mut self, text: &str) {
		if let Some(FormField::Input { value, cursor, .. }) = self.fields.get_mut(self.active) {
			value.insert_str(*cursor, text);
			*cursor = cursor.saturating_add(text.len());
		}
	}

	pub fn render(&self, view: &mut FrameView<'_>) {
		let w = view.width();
		let h = view.height();
		if w == 0 || h == 0 {
			return;
		}
		let s = RowStyle {
			fg: StyleRole::Text.fg(),
			bg: Color::Default,
			accent: StyleRole::Accent.fg(),
			dim: StyleRole::Muted.fg(),
			muted: StyleRole::Muted.fg(),
		};
		view.fill(Cell::new(' ').style(s.fg, s.bg, Attrs::NONE));

		let mut y: u16 = 0;
		if y < h {
			view.put_str(0, y, &self.title, s.accent, s.bg, Attrs::BOLD);
			y = y.saturating_add(1);
		}
		y = y.saturating_add(1);

		for (i, field) in self.fields.iter().enumerate() {
			let is_active = i == self.active;
			if y >= h {
				break;
			}
			y = match field {
				FormField::Input { label, value, placeholder, masked, cursor, .. } => {
					render_input_row(view, y, w, is_active, label, value, placeholder, *masked, *cursor, &s)
				}
				FormField::Picker { label, list, .. } => {
					render_picker_row(view, y, w, h, is_active, label, list, &s)
				}
			};
			y = y.saturating_add(1);
		}
		if y < h {
			let hint = "Tab next • Shift+Tab prev • Enter advance • Ctrl+Enter submit • Esc cancel";
			view.put_str(0, y, hint, s.dim, s.bg, Attrs::DIM);
		}
	}
}

struct RowStyle {
	fg: Color,
	bg: Color,
	accent: Color,
	dim: Color,
	muted: Color,
}

#[allow(clippy::too_many_arguments)]
fn render_input_row(
	view: &mut FrameView<'_>,
	y: u16,
	w: u16,
	is_active: bool,
	label: &str,
	value: &str,
	placeholder: &str,
	masked: bool,
	cursor: usize,
	s: &RowStyle,
) -> u16 {
	let marker = if is_active { "▸ " } else { "  " };
	let lbl_fg = if is_active { s.accent } else { s.muted };
	view.put_str(0, y, marker, lbl_fg, s.bg, Attrs::NONE);
	let label_text = format!("{label}: ");
	view.put_str(2, y, &label_text, lbl_fg, s.bg, Attrs::NONE);
	let x = 2u16 + label_text.chars().count() as u16;
	if value.is_empty() && !is_active {
		if !placeholder.is_empty() && x < w {
			view.put_str(x, y, placeholder, s.dim, s.bg, Attrs::DIM);
		}
	} else {
		let shown: String = if masked {
			"•".repeat(value.chars().count())
		} else {
			value.to_owned()
		};
		if x < w {
			view.put_str(x, y, &shown, s.fg, s.bg, Attrs::NONE);
		}
		if is_active {
			let cx = x.saturating_add(
				shown
					.chars()
					.take(cursor_char_index(value, cursor))
					.map(|_| 1)
					.sum::<u16>(),
			);
			if cx < w {
				let existing = view.get(cx, y).copied().unwrap_or_else(|| {
					Cell::new(' ').style(s.fg, s.bg, Attrs::NONE)
				});
				let mut cc = existing;
				cc.attrs = existing.attrs | Attrs::INVERSE;
				view.set(cx, y, cc);
			}
		}
	}
	y.saturating_add(1)
}

#[allow(clippy::too_many_arguments)] // cohesive render call; bundling into a struct adds indirection
fn render_picker_row(
	view: &mut FrameView<'_>,
	y: u16,
	w: u16,
	h: u16,
	is_active: bool,
	label: &str,
	list: &ListState,
	s: &RowStyle,
) -> u16 {
	let marker = if is_active { "▸ " } else { "  " };
	let lbl_fg = if is_active { s.accent } else { s.muted };
	view.put_str(0, y, marker, lbl_fg, s.bg, Attrs::NONE);
	let label_text = format!("{label}:");
	view.put_str(2, y, &label_text, lbl_fg, s.bg, Attrs::NONE);
	if !is_active {
		let sel = list.selected_id().unwrap_or("(none)");
		let x = 2u16 + label_text.chars().count() as u16 + 1;
		if x < w {
			view.put_str(x, y, sel, s.fg, s.bg, Attrs::NONE);
		}
		y.saturating_add(1)
	} else {
		let mut y = y.saturating_add(1);
		let list_h = list.preferred_height().min(h.saturating_sub(y));
		if list_h > 0 {
			let sub = crate::render::Region::new(0, y, w, list_h);
			let mut lv = view.sub(sub);
			list.render(&mut lv);
			y = y.saturating_add(list_h);
		}
		y
	}
}

fn handle_input_field(value: &mut String, cursor: &mut usize, key: &Key) {
	use unicode_segmentation::UnicodeSegmentation;
	match key.code {
		KeyCode::Char(c) => {
			if key.mods.contains(Mods::CTRL) {
				return;
			}
			let mut buf = [0u8; 4];
			let s = c.encode_utf8(&mut buf);
			value.insert_str(*cursor, s);
			*cursor += s.len();
		}
		KeyCode::Backspace => {
			if *cursor == 0 {
				return;
			}
			let prev_boundary = value[..*cursor]
				.grapheme_indices(true)
				.next_back()
				.map(|(i, _)| i)
				.unwrap_or(0);
			value.replace_range(prev_boundary..*cursor, "");
			*cursor = prev_boundary;
		}
		KeyCode::Delete => {
			if *cursor >= value.len() {
				return;
			}
			if let Some((_, g)) = value[*cursor..].grapheme_indices(true).next() {
				let end = *cursor + g.len();
				value.replace_range(*cursor..end, "");
			}
		}
		KeyCode::Left => {
			if *cursor > 0 {
				let prev = value[..*cursor]
					.grapheme_indices(true)
					.next_back()
					.map(|(i, _)| i)
					.unwrap_or(0);
				*cursor = prev;
			}
		}
		KeyCode::Right => {
			if *cursor < value.len() {
				if let Some((_, g)) = value[*cursor..].grapheme_indices(true).next() {
					*cursor += g.len();
				}
			}
		}
		KeyCode::Home => *cursor = 0,
		KeyCode::End => *cursor = value.len(),
		_ => {}
	}
}

fn cursor_char_index(value: &str, byte_cursor: usize) -> usize {
	let mut idx = 0;
	let mut count = 0;
	for (i, _) in value.char_indices() {
		if i >= byte_cursor {
			return count;
		}
		idx = i;
		count += 1;
	}
	let _ = idx;
	count
}
