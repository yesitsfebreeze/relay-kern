use crate::list_nav::SelCursor;
use crate::render::{Attrs, Cell, Color, FrameView, StyleRole};

use super::binding::{ListBindings, OnPickAction};

#[derive(Debug, Clone)]
pub struct ListItem {
	pub id: String,
	pub label: String,
	pub hint: String,
}

impl ListItem {
	pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
		Self {
			id: id.into(),
			label: label.into(),
			hint: String::new(),
		}
	}
	pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
		self.hint = hint.into();
		self
	}
}

pub const MAX_ROWS: usize = 10;

#[derive(Debug, Clone)]
pub struct ListState {
	pub(crate) items: Vec<ListItem>,
	pub(crate) filtered: Vec<usize>,
	pub(crate) cursor: SelCursor,
	top: usize,
	max_rows: usize,
	filter_query: Option<String>,
	bindings: ListBindings,
	on_pick: OnPickAction,
}

impl ListState {
	pub fn new(items: Vec<ListItem>) -> Self {
		let mut s = Self {
			items,
			filtered: Vec::new(),
			cursor: SelCursor::new(0),
			top: 0,
			max_rows: MAX_ROWS,
			filter_query: None,
			bindings: ListBindings::default(),
			on_pick: OnPickAction::default(),
		};
		s.set_filter("");
		s
	}

	pub fn with_bindings(mut self, bindings: ListBindings) -> Self {
		self.bindings = bindings;
		self
	}

	pub fn set_bindings(&mut self, bindings: ListBindings) {
		self.bindings = bindings;
	}

	pub fn bindings(&self) -> &ListBindings {
		&self.bindings
	}

	pub fn with_on_pick(mut self, action: OnPickAction) -> Self {
		self.on_pick = action;
		self
	}

	pub fn set_on_pick(&mut self, action: OnPickAction) {
		self.on_pick = action;
	}

	pub fn on_pick(&self) -> &OnPickAction {
		&self.on_pick
	}

	pub fn filter_query(&self) -> &str {
		self.filter_query.as_deref().unwrap_or("")
	}

	pub fn set_max_rows(&mut self, n: usize) {
		self.max_rows = n.max(1);
	}

	pub fn set_items(&mut self, items: Vec<ListItem>) {
		self.items = items;
		self.filter_query = None;
		self.set_filter("");
	}

	pub fn set_filter(&mut self, query: &str) {
		if matches!(self.filter_query.as_deref(), Some(q) if q == query) {
			return;
		}
		let q = query.to_ascii_lowercase();
		let mut scored: Vec<(usize, i32)> = self
			.items
			.iter()
			.enumerate()
			.filter_map(|(i, it)| score(&q, &it.label).map(|s| (i, s)))
			.collect();
		scored.sort_by(|a, b| b.1.cmp(&a.1));
		self.filtered = scored.into_iter().map(|(i, _)| i).collect();
		self.cursor = SelCursor::new(self.filtered.len());
		self.top = 0;
		self.filter_query = Some(query.to_string());
	}

	pub fn move_up(&mut self) {
		self.cursor.move_up();
		self.clamp_viewport();
	}

	pub fn move_down(&mut self) {
		self.cursor.move_down();
		self.clamp_viewport();
	}

	fn clamp_viewport(&mut self) {
		let sel = self.cursor.sel();
		if sel < self.top {
			self.top = sel;
		} else if sel >= self.top + self.max_rows {
			self.top = sel + 1 - self.max_rows;
		}
	}

	pub fn is_empty(&self) -> bool {
		self.filtered.is_empty()
	}

	pub fn selected(&self) -> Option<&ListItem> {
		self
			.filtered
			.get(self.cursor.sel())
			.and_then(|&i| self.items.get(i))
	}

	pub fn selected_id(&self) -> Option<&str> {
		self.selected().map(|it| it.id.as_str())
	}

	pub fn row_count(&self) -> usize {
		self.filtered.len().min(self.max_rows)
	}

	pub fn preferred_height(&self) -> u16 {
		if self.filtered.is_empty() {
			return 0;
		}
		(self.row_count() as u16).saturating_add(1)
	}

	pub fn render(&self, view: &mut FrameView<'_>) {
		let w = view.width();
		let h = view.height();
		if w == 0 || h == 0 || self.filtered.is_empty() {
			return;
		}
		let fg = StyleRole::Text.fg();
		let bg = Color::Default;
		let accent = StyleRole::Accent.fg();
		let dim = StyleRole::Muted.fg();
		view.fill(Cell::new(' ').style(fg, bg, Attrs::NONE));
		let sep: String = "─".repeat(w as usize);
		view.put_str(0, 0, &sep, dim, bg, Attrs::DIM);
		let start = 1u16;
		let rows = self.row_count();
		let top = self.top.min(self.filtered.len().saturating_sub(1));
		let end = (top + rows).min(self.filtered.len());
		for (row, &idx) in self.filtered[top..end].iter().enumerate() {
			let y = start + row as u16;
			if y >= h {
				break;
			}
			let is_sel = top + row == self.cursor.sel();
			let it = &self.items[idx];
			let (name_fg, pointer) = if is_sel { (accent, "▸ ") } else { (fg, "  ") };
			view.put_str(0, y, pointer, name_fg, bg, Attrs::NONE);
			let name_x = 2u16;
			view.put_str(
				name_x,
				y,
				&it.label,
				name_fg,
				bg,
				if is_sel { Attrs::BOLD } else { Attrs::NONE },
			);
			if !it.hint.is_empty() {
				let name_w = it.label.chars().count() as u16;
				let hint_x = name_x + name_w + 2;
				if hint_x < w {
					view.put_str(hint_x, y, &it.hint, dim, bg, Attrs::DIM);
				}
			}
		}
	}
}

fn score(query: &str, label: &str) -> Option<i32> {
	if query.is_empty() {
		return Some(0);
	}
	let lname = label.to_ascii_lowercase();
	if lname.starts_with(query) {
		return Some(10_000 - label.len() as i32);
	}
	let qchars: Vec<char> = query.chars().collect();
	let nchars: Vec<char> = lname.chars().collect();
	let mut qi = 0usize;
	let mut score = 0i32;
	let mut first: Option<usize> = None;
	let mut last: Option<usize> = None;
	for (i, &c) in nchars.iter().enumerate() {
		if qi >= qchars.len() {
			break;
		}
		if c == qchars[qi] {
			if first.is_none() {
				first = Some(i);
			}
			if let Some(p) = last {
				if p + 1 == i {
					score += 5;
				}
			}
			last = Some(i);
			qi += 1;
		}
	}
	if qi < qchars.len() {
		return None;
	}
	score -= first.unwrap_or(0) as i32;
	score -= (nchars.len() as i32) / 4;
	Some(score)
}
