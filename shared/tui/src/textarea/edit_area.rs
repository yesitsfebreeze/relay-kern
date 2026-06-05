use crossterm::event::KeyEvent;
use crate::input::{Key, KeyCode as IKeyCode, Mods};
use crate::render::{Attrs, Cell, Color, FrameView, Region};

use super::binding::ListAction;
use super::buffer::{Buffer, Pos};
use super::form::{FormOutcome, FormState};
use super::history::{Edit, EditKind, History};
use super::list::ListState;
use super::words::{next_word_boundary_bytes, prev_word_boundary_bytes};
use super::wrap::{wrap_line, VisualRow};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditOutcome {
	Handled,
	Submit,
	Cancel,
	Unhandled,
	Pick(String),
	FormSubmit(Vec<(String, String)>),
	Emit(String),
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum WrapMode {
	None,
	#[default]
	Soft,
	Hard,
}

pub struct EditArea {
	buffer: Buffer,
	cursor: Pos,
	anchor: Option<Pos>,
	pub(crate) history: History,
	wrap: WrapMode,
	goal_col: Option<usize>,
	view_w: u16,
	pub(crate) view_h: u16,
	scroll: usize,
	hard_cols: u16,
	list: Option<ListState>,
	form: Option<FormState>,
	banner: Option<Vec<String>>,
	pub style_text: (Color, Color, Attrs),
	pub style_selection: (Color, Color, Attrs),
	pub style_cursor: (Color, Color, Attrs),
	/// Optional second endpoint for cursor fade. When `Some`, the cursor lerps
	/// per-component between `style_cursor.{fg,bg}` (A) and these (B), giving
	/// four independent colours. When `None`, falls back to the legacy
	/// fg↔bg swap via [`lerp_inverse`].
	pub style_cursor_alt: Option<(Color, Color)>,
	pub cursor_phase: f32,
}

impl EditArea {
	pub fn new() -> Self {
		EditArea {
			buffer: Buffer::new(),
			cursor: Pos::default(),
			anchor: None,
			history: History::default(),
			wrap: WrapMode::default(),
			goal_col: None,
			view_w: 0,
			view_h: 0,
			scroll: 0,
			hard_cols: 80,
			list: None,
			form: None,
			banner: None,
			style_text: (Color::Default, Color::Default, Attrs::NONE),
			style_selection: (Color::Default, Color::Default, Attrs::INVERSE),
			style_cursor: (Color::Default, Color::Default, Attrs::INVERSE),
			style_cursor_alt: None,
			cursor_phase: 1.0,
		}
	}

	pub fn set_wrap(&mut self, mode: WrapMode) {
		self.wrap = mode;
	}

	pub fn set_hard_cols(&mut self, cols: u16) {
		self.hard_cols = cols.max(1);
	}

	pub fn enter_list(&mut self, mut list: ListState) {
		list.set_filter(&self.buffer.to_string());
		self.list = Some(list);
	}

	pub fn exit_list(&mut self) {
		self.list = None;
	}

	pub fn is_in_list(&self) -> bool {
		matches!(self.list.as_ref(), Some(l) if !l.is_empty())
	}

	pub fn list_state(&self) -> Option<&ListState> {
		self.list.as_ref()
	}

	pub fn list_state_mut(&mut self) -> Option<&mut ListState> {
		self.list.as_mut()
	}

	pub fn preferred_height(&self, cols: u16) -> u16 {
		if let Some(form) = self.form.as_ref() {
			return form.preferred_height().max(1);
		}
		let text_rows = match self.wrap {
			WrapMode::Soft => {
				let cols = cols.max(1);
				let total: usize = self
					.buffer
					.lines()
					.iter()
					.enumerate()
					.map(|(i, line)| wrap_line(i, line, cols).len())
					.sum();
				(total as u16).max(1)
			}
			_ => (self.buffer.line_count() as u16).max(1),
		};
		let list_rows = self.list.as_ref().map(|l| l.preferred_height()).unwrap_or(0);
		let banner_rows = self.banner_height();
		text_rows
			.saturating_add(list_rows)
			.saturating_add(banner_rows)
	}

	fn refilter_list(&mut self) {
		if let Some(list) = self.list.as_mut() {
			list.set_filter(&self.buffer.to_string());
		}
	}

	pub fn enter_form(&mut self, form: FormState) {
		self.list = None;
		self.form = Some(form);
	}

	pub fn exit_form(&mut self) {
		self.form = None;
	}

	pub fn is_in_form(&self) -> bool {
		self.form.is_some()
	}

	pub fn form_state(&self) -> Option<&FormState> {
		self.form.as_ref()
	}

	pub fn form_state_mut(&mut self) -> Option<&mut FormState> {
		self.form.as_mut()
	}

	pub fn enter_banner(&mut self, lines: Vec<String>) {
		if lines.is_empty() {
			self.banner = None;
		} else {
			self.banner = Some(lines);
		}
	}

	pub fn dismiss_banner(&mut self) {
		self.banner = None;
	}

	pub fn has_banner(&self) -> bool {
		self.banner.is_some()
	}

	fn banner_height(&self) -> u16 {
		self
			.banner
			.as_ref()
			.map(|lines| lines.len() as u16)
			.unwrap_or(0)
	}

	pub fn form_handle_paste(&mut self, text: &str) -> bool {
		match self.form.as_mut() {
			Some(f) => {
				f.handle_paste(text);
				true
			}
			None => false,
		}
	}

	pub fn set_text(&mut self, text: &str) {
		self.buffer = Buffer::from_str(text);
		self.cursor = Pos::default();
		self.anchor = None;
		self.history = History::default();
		self.scroll = 0;
		self.refilter_list();
	}

	pub fn set_text_end(&mut self, text: &str) {
		self.set_text(text);
		self.cursor = self.buffer.end();
	}

	pub fn goto_line(&mut self, line: usize) {
		let line = line.saturating_sub(1).min(self.buffer.line_count().saturating_sub(1));
		self.cursor = Pos { line, col: 0 };
		self.anchor = None;
		self.goal_col = None;
	}

	pub fn text(&self) -> String {
		self.buffer.to_string()
	}

	pub fn splice(&mut self, start: Pos, end: Pos, text: &str) {
		let start = self.buffer.clamp(start);
		let end = self.buffer.clamp(end);
		if start != end {
			self.buffer.delete(start, end);
		}
		let cur = self.buffer.insert(start, text);
		self.cursor = cur;
		self.anchor = None;
		self.refilter_list();
	}

	pub fn line_count(&self) -> usize {
		self.buffer.line_count()
	}

	pub fn cursor(&self) -> Pos {
		self.cursor
	}

	pub fn selection(&self) -> Option<(Pos, Pos)> {
		let a = self.anchor?;
		if a == self.cursor {
			return None;
		}
		Some(if a <= self.cursor {
			(a, self.cursor)
		} else {
			(self.cursor, a)
		})
	}

	pub fn handle_key(&mut self, key: &KeyEvent) -> EditOutcome {
		self.handle_input(&Key::from(key))
	}

	pub fn handle_input(&mut self, key: &Key) -> EditOutcome {
		let shift = key.mods.contains(Mods::SHIFT);
		let ctrl = key.mods.contains(Mods::CTRL);
		let alt = key.mods.contains(Mods::ALT);

		if self.banner.is_some() {
			self.banner = None;
		}

		if self.form.is_some() {
			let outcome = self
				.form
				.as_mut()
				.map(|f| f.handle_input(key))
				.unwrap_or(FormOutcome::Continue);
			return match outcome {
				FormOutcome::Continue => EditOutcome::Handled,
				FormOutcome::Cancel => EditOutcome::Cancel,
				FormOutcome::Submit => {
					let values = self
						.form
						.as_ref()
						.map(|f| f.values())
						.unwrap_or_default();
					EditOutcome::FormSubmit(values)
				}
			};
		}

		if self.is_in_list() {
			let action = self
				.list
				.as_ref()
				.and_then(|l| l.bindings().dispatch(key))
				.cloned();
			if let Some(action) = action {
				match action {
					ListAction::MoveUp => {
						if let Some(l) = self.list.as_mut() {
							l.move_up();
						}
						return EditOutcome::Handled;
					}
					ListAction::MoveDown => {
						if let Some(l) = self.list.as_mut() {
							l.move_down();
						}
						return EditOutcome::Handled;
					}
					ListAction::Pick => {
						if let Some(id) = self.list.as_ref().and_then(|l| l.selected_id()) {
							return EditOutcome::Pick(id.to_string());
						}
					}
					ListAction::Cancel => return EditOutcome::Cancel,
					ListAction::Emit(name) => return EditOutcome::Emit(name),
				}
			}
		}

		match key.code {
			IKeyCode::Esc => return EditOutcome::Cancel,
			IKeyCode::Enter => {
				if (ctrl && !shift) || alt {
					return EditOutcome::Submit;
				}
				self.delete_selection_if_any();
				self.insert_text("\n", EditKind::InsertRun);
				return EditOutcome::Handled;
			}
			IKeyCode::Char(c) => {
				if ctrl && !alt {
					match c {
						'a' | 'A' => {
							self.select_all();
							return EditOutcome::Handled;
						}
						'z' | 'Z' => {
							if shift {
								self.redo();
							} else {
								self.undo();
							}
							return EditOutcome::Handled;
						}
						'y' | 'Y' => {
							self.redo();
							return EditOutcome::Handled;
						}
						'j' | 'J' => {
							self.delete_selection_if_any();
							self.insert_text("\n", EditKind::InsertRun);
							return EditOutcome::Handled;
						}
						_ => {}
					}
				}
				self.delete_selection_if_any();
				self.anchor = None;
				let mut buf = [0u8; 4];
				let s = c.encode_utf8(&mut buf);
				self.insert_text(s, EditKind::InsertRun);
				return EditOutcome::Handled;
			}
			IKeyCode::Backspace => {
				if alt {
					self.word_delete_backward();
				} else {
					self.backspace();
				}
				return EditOutcome::Handled;
			}
			IKeyCode::Delete => {
				if shift {
					self.delete_current_line();
				} else if alt {
					self.word_delete_forward();
				} else {
					self.delete_forward();
				}
				return EditOutcome::Handled;
			}
			IKeyCode::Left => {
				self.handle_shift(shift);
				if ctrl {
					self.move_prev_word();
				} else {
					self.move_left();
				}
				return EditOutcome::Handled;
			}
			IKeyCode::Right => {
				self.handle_shift(shift);
				if ctrl {
					self.move_next_word();
				} else {
					self.move_right();
				}
				return EditOutcome::Handled;
			}
			IKeyCode::Up => {
				self.handle_shift(shift);
				self.move_up(1);
				return EditOutcome::Handled;
			}
			IKeyCode::Down => {
				self.handle_shift(shift);
				self.move_down(1);
				return EditOutcome::Handled;
			}
			IKeyCode::Home => {
				self.handle_shift(shift);
				if ctrl {
					self.cursor = Pos::default();
				} else {
					self.cursor.col = 0;
				}
				self.goal_col = None;
				return EditOutcome::Handled;
			}
			IKeyCode::End => {
				self.handle_shift(shift);
				if ctrl {
					self.cursor = self.buffer.end();
				} else {
					self.cursor = self.buffer.end_of_line(self.cursor.line);
				}
				self.goal_col = None;
				return EditOutcome::Handled;
			}
			IKeyCode::PageUp => {
				self.handle_shift(shift);
				let step = self.view_h.max(1) as usize;
				self.move_up(step);
				return EditOutcome::Handled;
			}
			IKeyCode::PageDown => {
				self.handle_shift(shift);
				let step = self.view_h.max(1) as usize;
				self.move_down(step);
				return EditOutcome::Handled;
			}
			IKeyCode::Tab => {
				self.delete_selection_if_any();
				self.insert_text("\t", EditKind::InsertRun);
				return EditOutcome::Handled;
			}
			_ => {}
		}
		EditOutcome::Unhandled
	}

	fn handle_shift(&mut self, shift: bool) {
		if shift {
			if self.anchor.is_none() {
				self.anchor = Some(self.cursor);
			}
		} else {
			self.anchor = None;
		}
	}

	fn move_left(&mut self) {
		self.cursor = self.buffer.prev_grapheme(self.cursor);
		self.goal_col = None;
	}

	fn move_right(&mut self) {
		self.cursor = self.buffer.next_grapheme(self.cursor);
		self.goal_col = None;
	}

	fn move_prev_word(&mut self) {
		let line = self.buffer.line(self.cursor.line).to_string();
		if self.cursor.col == 0 {
			if self.cursor.line > 0 {
				let above = self.cursor.line - 1;
				self.cursor = self.buffer.end_of_line(above);
			}
		} else {
			let col = prev_word_boundary_bytes(&line, self.cursor.col);
			self.cursor.col = col;
		}
		self.goal_col = None;
	}

	fn move_next_word(&mut self) {
		let line = self.buffer.line(self.cursor.line).to_string();
		if self.cursor.col >= line.len() {
			if self.cursor.line + 1 < self.buffer.line_count() {
				self.cursor = Pos {
					line: self.cursor.line + 1,
					col: 0,
				};
			}
		} else {
			let col = next_word_boundary_bytes(&line, self.cursor.col);
			self.cursor.col = col;
		}
		self.goal_col = None;
	}

	fn move_up(&mut self, n: usize) {
		let goal = self
			.goal_col
			.unwrap_or_else(|| self.buffer.display_col(self.cursor.line, self.cursor.col));
		self.goal_col = Some(goal);
		let new_line = self.cursor.line.saturating_sub(n);
		let col = self.buffer.byte_col_for_display(new_line, goal);
		self.cursor = self.buffer.clamp(Pos {
			line: new_line,
			col,
		});
	}

	fn move_down(&mut self, n: usize) {
		let goal = self
			.goal_col
			.unwrap_or_else(|| self.buffer.display_col(self.cursor.line, self.cursor.col));
		self.goal_col = Some(goal);
		let last = self.buffer.line_count().saturating_sub(1);
		let new_line = (self.cursor.line + n).min(last);
		let col = self.buffer.byte_col_for_display(new_line, goal);
		self.cursor = self.buffer.clamp(Pos {
			line: new_line,
			col,
		});
	}

	fn select_all(&mut self) {
		self.anchor = Some(Pos::default());
		self.cursor = self.buffer.end();
	}

	fn backspace(&mut self) {
		if self.selection().is_some() {
			self.delete_selection_if_any();
			return;
		}
		let prev = self.buffer.prev_grapheme(self.cursor);
		if prev == self.cursor {
			return;
		}
		self.delete_range_grouped(prev, self.cursor, EditKind::DeleteRun);
	}

	fn delete_forward(&mut self) {
		if self.selection().is_some() {
			self.delete_selection_if_any();
			return;
		}
		let next = self.buffer.next_grapheme(self.cursor);
		if next == self.cursor {
			return;
		}
		self.delete_range_grouped(self.cursor, next, EditKind::DeleteRun);
	}

	fn word_delete_backward(&mut self) {
		if self.selection().is_some() {
			self.delete_selection_if_any();
			return;
		}
		let line = self.buffer.line(self.cursor.line).to_string();
		let target = if self.cursor.col == 0 {
			if self.cursor.line == 0 {
				return;
			}
			let above = self.cursor.line - 1;
			Pos {
				line: above,
				col: self.buffer.line(above).len(),
			}
		} else {
			Pos {
				line: self.cursor.line,
				col: prev_word_boundary_bytes(&line, self.cursor.col),
			}
		};
		self.delete_range_grouped(target, self.cursor, EditKind::WordDelete);
	}

	fn delete_current_line(&mut self) {
		if self.selection().is_some() {
			self.delete_selection_if_any();
			return;
		}
		let line = self.cursor.line;
		let end = if line + 1 < self.buffer.line_count() {
			Pos {
				line: line + 1,
				col: 0,
			}
		} else {
			self.buffer.end_of_line(line)
		};
		if self.cursor == end {
			return;
		}
		self.delete_range_grouped(self.cursor, end, EditKind::DeleteRun);
	}

	fn word_delete_forward(&mut self) {
		if self.selection().is_some() {
			self.delete_selection_if_any();
			return;
		}
		let line = self.buffer.line(self.cursor.line).to_string();
		let target = if self.cursor.col >= line.len() {
			if self.cursor.line + 1 >= self.buffer.line_count() {
				return;
			}
			Pos {
				line: self.cursor.line + 1,
				col: 0,
			}
		} else {
			Pos {
				line: self.cursor.line,
				col: next_word_boundary_bytes(&line, self.cursor.col),
			}
		};
		self.delete_range_grouped(self.cursor, target, EditKind::WordDelete);
	}

	fn delete_selection_if_any(&mut self) {
		if let Some((a, b)) = self.selection() {
			self.delete_range_grouped(a, b, EditKind::DeleteRun);
		}
	}

	fn delete_range_grouped(&mut self, a: Pos, b: Pos, kind: EditKind) {
		let cursor_before = self.cursor;
		let removed = self.buffer.delete(a, b);
		if removed.is_empty() {
			return;
		}
		self.cursor = a;
		self.anchor = None;
		self.goal_col = None;
		self.history.record(
			kind,
			cursor_before,
			Edit {
				start: a,
				end: b,
				text: removed,
				is_insert: false,
			},
			self.cursor,
		);
		self.apply_hard_wrap_if_enabled();
		self.refilter_list();
	}

	fn insert_text(&mut self, text: &str, kind: EditKind) {
		let cursor_before = self.cursor;
		let start = self.cursor;
		let end = self.buffer.insert(start, text);
		self.cursor = end;
		self.anchor = None;
		self.goal_col = None;
		self.history.record(
			kind.clone(),
			cursor_before,
			Edit {
				start,
				end,
				text: text.to_string(),
				is_insert: true,
			},
			self.cursor,
		);
		self.apply_hard_wrap_if_enabled();
		self.refilter_list();
	}

	fn apply_hard_wrap_if_enabled(&mut self) {
		if self.wrap != WrapMode::Hard {
			return;
		}
		let text = self.buffer.to_string();
		let cursor_byte_offset = byte_offset_of(&text, self.cursor);
		let wrapped = super::wrap::hard_wrap(&text, self.hard_cols);
		if wrapped == text {
			return;
		}
		self.buffer = Buffer::from_str(&wrapped);
		self.cursor = pos_of_byte_offset(&wrapped, cursor_byte_offset);
		self.cursor = self.buffer.clamp(self.cursor);
	}

	pub fn undo(&mut self) {
		let Some(group) = self.history.pop_undo() else {
			return;
		};
		for e in group.edits.iter().rev() {
			if e.is_insert {
				self.buffer.delete(e.start, e.end);
			} else {
				self.buffer.insert(e.start, &e.text);
			}
		}
		self.cursor = self.buffer.clamp(group.cursor_before);
		self.anchor = None;
		self.goal_col = None;
		self.history.push_redo(group);
		self.refilter_list();
	}

	pub fn redo(&mut self) {
		let Some(group) = self.history.pop_redo() else {
			return;
		};
		for e in &group.edits {
			if e.is_insert {
				self.buffer.insert(e.start, &e.text);
			} else {
				self.buffer.delete(e.start, e.end);
			}
		}
		self.cursor = self.buffer.clamp(group.cursor_after);
		self.anchor = None;
		self.goal_col = None;
		self.history.push_undo(group);
		self.refilter_list();
	}

	pub fn render(&mut self, view: &mut FrameView<'_>) {
		let full_w = view.width();
		let full_h = view.height();
		if full_w == 0 || full_h == 0 {
			return;
		}
		if let Some(form) = self.form.as_ref() {
			let (fg, bg, attrs) = self.style_text;
			view.fill(Cell::new(' ').style(fg, bg, attrs));
			form.render(view);
			return;
		}
		let banner_h = self.banner_height().min(full_h.saturating_sub(1));
		if banner_h > 0 {
			let (fg, bg, attrs) = self.style_text;
			let mut bv = view.sub(Region::new(0, 0, full_w, banner_h));
			bv.fill(Cell::new(' ').style(fg, bg, attrs));
			if let Some(lines) = self.banner.as_ref() {
				for (y, line) in lines.iter().take(banner_h as usize).enumerate() {
					bv.put_str(0, y as u16, line, fg, bg, attrs);
				}
			}
		}
		let below_banner_y = banner_h;
		let below_banner_h = full_h.saturating_sub(banner_h);
		let mut remaining_view = view.sub(Region::new(0, below_banner_y, full_w, below_banner_h));
		let view = &mut remaining_view;
		let full_w = view.width();
		let full_h = view.height();
		if full_w == 0 || full_h == 0 {
			return;
		}
		let list_h = match self.list.as_ref() {
			Some(l) => l.preferred_height().min(full_h.saturating_sub(1)),
			None => 0,
		};
		if list_h > 0 {
			let mut lv = view.sub(Region::new(0, 0, full_w, list_h));
			if let Some(l) = self.list.as_ref() {
				l.render(&mut lv);
			}
		}
		let edit_y = list_h;
		let edit_h = full_h.saturating_sub(list_h);
		if edit_h == 0 {
			return;
		}
		let mut view = view.sub(Region::new(0, edit_y, full_w, edit_h));
		let view = &mut view;
		self.view_w = view.width();
		self.view_h = view.height();
		if self.view_w == 0 || self.view_h == 0 {
			return;
		}

		let (fg, bg, attrs) = self.style_text;
		view.fill(Cell::new(' ').style(fg, bg, attrs));

		let rows = self.visual_rows();
		self.ensure_cursor_visible(&rows);

		let sel = self.selection();

		let start = self.scroll.min(rows.len().saturating_sub(1));
		let end = (start + self.view_h as usize).min(rows.len());
		for (screen_y, row) in rows[start..end].iter().enumerate() {
			self.render_row(view, screen_y as u16, row, sel);
		}

		if let Some(screen) = self.cursor_screen(&rows) {
			let (cx, cy) = screen;
			if cy < self.view_h {
				let existing = view.get(cx, cy).copied().unwrap_or_default();
				let mut cc = existing;
				let (ovr_fg, ovr_bg, ovr_attrs) = self.style_cursor;
				let base_fg = if matches!(ovr_fg, Color::Default) {
					existing.fg
				} else {
					ovr_fg
				};
				let base_bg = if matches!(ovr_bg, Color::Default) {
					existing.bg
				} else {
					ovr_bg
				};
				let t = self.cursor_phase.clamp(0.0, 1.0);
				let (fg, bg, smooth) = if let Some((alt_fg, alt_bg)) = self.style_cursor_alt {
					let (fg, fg_smooth) = lerp_pair(base_fg, alt_fg, t);
					let (bg, bg_smooth) = lerp_pair(base_bg, alt_bg, t);
					(fg, bg, fg_smooth && bg_smooth)
				} else {
					let (fg, bg) = lerp_inverse(base_fg, base_bg, t);
					let smooth = matches!(base_fg, Color::Rgb(..)) && matches!(base_bg, Color::Rgb(..));
					(fg, bg, smooth)
				};
				cc.fg = fg;
				cc.bg = bg;
				cc.attrs = if smooth {
					existing.attrs | Attrs(ovr_attrs.0 & !Attrs::INVERSE.0)
				} else if t >= 0.5 {
					existing.attrs | ovr_attrs
				} else {
					existing.attrs
				};
				view.set(cx, cy, cc);
			}
		}
	}

	fn render_row(
		&self,
		view: &mut FrameView<'_>,
		screen_y: u16,
		row: &VisualRow,
		sel: Option<(Pos, Pos)>,
	) {
		use unicode_segmentation::UnicodeSegmentation;
		use unicode_width::UnicodeWidthStr;
		let line = self.buffer.line(row.line);
		let slice = &line[row.start_byte..row.end_byte];
		let (fg, bg, attrs) = self.style_text;
		let (sfg, sbg, sattrs) = self.style_selection;
		let mut cx: u16 = 0;
		for (i, g) in slice.grapheme_indices(true) {
			let w = UnicodeWidthStr::width(g) as u16;
			if w == 0 {
				continue;
			}
			if cx + w > self.view_w {
				break;
			}
			let byte = row.start_byte + i;
			let in_sel = match sel {
				Some((a, b)) => self.pos_in_range(
					Pos {
						line: row.line,
						col: byte,
					},
					a,
					b,
				),
				None => false,
			};
			let (gf, gb, ga) = if in_sel {
				(sfg, sbg, sattrs)
			} else {
				(fg, bg, attrs)
			};
			let ch = g.chars().next().unwrap_or(' ');
			if ch.is_ascii() && w == 1 {
				view.set(cx, screen_y, Cell::new(ch).style(gf, gb, ga));
			} else {
				view.put_str(cx, screen_y, g, gf, gb, ga);
			}
			cx = cx.saturating_add(w);
		}
		if row.trailing_hyphen && cx < self.view_w {
			view.set(cx, screen_y, Cell::new('-').style(fg, bg, attrs));
		}
		// Mark the line break inside a multi-line selection so the user can see
		// that the row terminates with a newline (otherwise empty/short selected
		// lines render as nothing).
		if cx < self.view_w
			&& row.end_byte == line.len()
			&& row.line + 1 < self.buffer.line_count()
		{
			let in_sel = match sel {
				Some((a, b)) => self.pos_in_range(
					Pos {
						line: row.line,
						col: row.end_byte,
					},
					a,
					b,
				),
				None => false,
			};
			if in_sel {
				view.set(cx, screen_y, Cell::new(' ').style(sfg, sbg, sattrs));
			}
		}
	}

	fn pos_in_range(&self, p: Pos, a: Pos, b: Pos) -> bool {
		p >= a && p < b
	}

	fn visual_rows(&self) -> Vec<VisualRow> {
		let mut rows = Vec::new();
		match self.wrap {
			WrapMode::Soft => {
				let cols = self.view_w;
				for (i, line) in self.buffer.lines().iter().enumerate() {
					rows.extend(wrap_line(i, line, cols));
				}
			}
			_ => {
				for (i, line) in self.buffer.lines().iter().enumerate() {
					rows.push(VisualRow {
						line: i,
						start_byte: 0,
						end_byte: line.len(),
						trailing_hyphen: false,
					});
				}
			}
		}
		rows
	}

	fn cursor_screen(&self, rows: &[VisualRow]) -> Option<(u16, u16)> {
		let (row_idx, row) = rows.iter().enumerate().rev().find(|(_, r)| {
			r.line == self.cursor.line && self.cursor.col >= r.start_byte && self.cursor.col <= r.end_byte
		})?;
		let line = self.buffer.line(row.line);
		let col_in_row = self.cursor.col.saturating_sub(row.start_byte);
		let slice = &line[row.start_byte..row.start_byte + col_in_row];
		let display = unicode_width::UnicodeWidthStr::width(slice);
		let sy = row_idx.saturating_sub(self.scroll);
		Some((display.min(self.view_w as usize) as u16, sy as u16))
	}

	fn ensure_cursor_visible(&mut self, rows: &[VisualRow]) {
		let cursor_row = rows.iter().position(|r| {
			r.line == self.cursor.line && self.cursor.col >= r.start_byte && self.cursor.col <= r.end_byte
		});
		let Some(cr) = cursor_row else {
			return;
		};
		let h = self.view_h.max(1) as usize;
		if cr < self.scroll {
			self.scroll = cr;
		} else if cr >= self.scroll + h {
			self.scroll = cr + 1 - h;
		}
	}
}

impl Default for EditArea {
	fn default() -> Self {
		Self::new()
	}
}

/// Lerp two independent colours. RGB pairs blend smoothly; non-RGB does a
/// hard switch at t≥0.5. Bool indicates whether the result is an RGB lerp.
fn lerp_pair(a: Color, b: Color, t: f32) -> (Color, bool) {
	match (a, b) {
		(Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) => {
			let mix = |x: u8, y: u8| {
				(x as f32 * (1.0 - t) + y as f32 * t)
					.round()
					.clamp(0.0, 255.0) as u8
			};
			(Color::Rgb(mix(ar, br), mix(ag, bg), mix(ab, bb)), true)
		}
		_ => (if t >= 0.5 { b } else { a }, false),
	}
}

fn lerp_inverse(fg: Color, bg: Color, t: f32) -> (Color, Color) {
	match (fg, bg) {
		(Color::Rgb(fr, fg_, fb), Color::Rgb(br, bg_, bb)) => {
			let mix = |a: u8, b: u8| {
				(a as f32 * (1.0 - t) + b as f32 * t)
					.round()
					.clamp(0.0, 255.0) as u8
			};
			(
				Color::Rgb(mix(fr, br), mix(fg_, bg_), mix(fb, bb)),
				Color::Rgb(mix(br, fr), mix(bg_, fg_), mix(bb, fb)),
			)
		}
		_ => {
			if t >= 0.5 {
				(bg, fg)
			} else {
				(fg, bg)
			}
		}
	}
}

pub(crate) fn byte_offset_of(text: &str, p: Pos) -> usize {
	let mut acc = 0usize;
	for (i, line) in text.split('\n').enumerate() {
		if i == p.line {
			return acc + p.col.min(line.len());
		}
		acc += line.len() + 1;
	}
	text.len()
}

pub(crate) fn pos_of_byte_offset(text: &str, off: usize) -> Pos {
	let off = off.min(text.len());
	let mut line = 0usize;
	let mut line_start = 0usize;
	for (i, b) in text.bytes().enumerate() {
		if i == off {
			return Pos {
				line,
				col: off - line_start,
			};
		}
		if b == b'\n' {
			line += 1;
			line_start = i + 1;
		}
	}
	Pos {
		line,
		col: off - line_start,
	}
}
