mod binding {
	use crate::input::{Key, KeyCode, Mods};
	use crate::textarea::binding::*;

	#[test]
	fn parses_bare_key() {
		let c = KeyChord::parse("up").unwrap();
		assert_eq!(c.code, KeyCode::Up);
		assert_eq!(c.mods, Mods::NONE);
	}

	#[test]
	fn parses_modifier_chord() {
		let c = KeyChord::parse("ctrl+shift+enter").unwrap();
		assert_eq!(c.code, KeyCode::Enter);
		assert!(c.mods.contains(Mods::CTRL));
		assert!(c.mods.contains(Mods::SHIFT));
	}

	#[test]
	fn case_insensitive_char_match() {
		let chord = KeyChord::parse("ctrl+x").unwrap();
		let hit = Key::with(KeyCode::Char('X'), Mods::CTRL);
		assert!(chord.matches(&hit));
	}

	#[test]
	fn default_list_bindings_cover_basics() {
		let b = ListBindings::default();
		let up = Key::with(KeyCode::Up, Mods::NONE);
		assert_eq!(b.dispatch(&up), Some(&ListAction::MoveUp));
	}

	#[test]
	fn custom_binding_wins_first() {
		let mut b = ListBindings::default();
		b.push(
			KeyChord::parse("ctrl+o").unwrap(),
			ListAction::Emit("open".into()),
		);
		let k = Key::with(KeyCode::Char('o'), Mods::CTRL);
		match b.dispatch(&k) {
			Some(ListAction::Emit(s)) => assert_eq!(s, "open"),
			other => panic!("unexpected: {:?}", other),
		}
	}
}

mod buffer {
	use crate::textarea::buffer::*;

	#[test]
	fn empty_buffer_has_one_line() {
		let b = Buffer::new();
		assert_eq!(b.line_count(), 1);
		assert_eq!(b.line(0), "");
	}

	#[test]
	fn from_str_splits_on_newline() {
		let b = Buffer::from_str("ab\ncd\nef");
		assert_eq!(b.line_count(), 3);
		assert_eq!(b.line(1), "cd");
	}

	#[test]
	fn insert_inline_returns_post_position() {
		let mut b = Buffer::from_str("hello");
		let p = b.insert(Pos { line: 0, col: 2 }, "XY");
		assert_eq!(b.line(0), "heXYllo");
		assert_eq!(p, Pos { line: 0, col: 4 });
	}

	#[test]
	fn insert_with_newline_splits_line() {
		let mut b = Buffer::from_str("hello");
		let p = b.insert(Pos { line: 0, col: 2 }, "X\nY");
		assert_eq!(b.line_count(), 2);
		assert_eq!(b.line(0), "heX");
		assert_eq!(b.line(1), "Yllo");
		assert_eq!(p, Pos { line: 1, col: 1 });
	}

	#[test]
	fn delete_inline_removes_chars() {
		let mut b = Buffer::from_str("abcdef");
		let removed = b.delete(Pos { line: 0, col: 1 }, Pos { line: 0, col: 4 });
		assert_eq!(removed, "bcd");
		assert_eq!(b.line(0), "aef");
	}

	#[test]
	fn delete_multiline_merges_lines() {
		let mut b = Buffer::from_str("one\ntwo\nthree");
		let removed = b.delete(Pos { line: 0, col: 1 }, Pos { line: 2, col: 2 });
		assert_eq!(removed, "ne\ntwo\nth");
		assert_eq!(b.line_count(), 1);
		assert_eq!(b.line(0), "oree");
	}

	#[test]
	fn slice_across_lines() {
		let b = Buffer::from_str("one\ntwo\nthree");
		let s = b.slice(Pos { line: 0, col: 1 }, Pos { line: 2, col: 2 });
		assert_eq!(s, "ne\ntwo\nth");
	}

	#[test]
	fn next_grapheme_moves_over_combining_mark() {
		let b = Buffer::from_str("e\u{0301}x");
		let p = b.next_grapheme(Pos { line: 0, col: 0 });
		assert_eq!(p.col, 3);
	}

	#[test]
	fn prev_grapheme_steps_back_over_cluster() {
		let b = Buffer::from_str("e\u{0301}x");
		let p = b.prev_grapheme(Pos { line: 0, col: 3 });
		assert_eq!(p.col, 0);
	}

	#[test]
	fn next_grapheme_at_eol_wraps_to_next_line() {
		let b = Buffer::from_str("ab\ncd");
		let p = b.next_grapheme(Pos { line: 0, col: 2 });
		assert_eq!(p, Pos { line: 1, col: 0 });
	}

	#[test]
	fn prev_grapheme_at_bol_wraps_to_prev_line() {
		let b = Buffer::from_str("ab\ncd");
		let p = b.prev_grapheme(Pos { line: 1, col: 0 });
		assert_eq!(p, Pos { line: 0, col: 2 });
	}

	#[test]
	fn display_col_counts_visible_width() {
		let b = Buffer::from_str("a漢b");
		assert_eq!(b.display_col(0, 4), 3);
	}

	#[test]
	fn byte_col_for_display_rounds_past_wide_glyphs() {
		let b = Buffer::from_str("a漢b");
		assert_eq!(b.byte_col_for_display(0, 2), 1);
		assert_eq!(b.byte_col_for_display(0, 3), 4);
	}

	#[test]
	fn clamp_snaps_invalid_col_to_grapheme() {
		let b = Buffer::from_str("e\u{0301}x");
		let p = b.clamp(Pos { line: 0, col: 1 });
		assert_eq!(p.col, 0);
	}

	#[test]
	fn to_string_roundtrip() {
		let s = "one\ntwo\nthree";
		let b = Buffer::from_str(s);
		assert_eq!(b.to_string(), s);
	}
}

mod history {
	use crate::textarea::buffer::Pos;
	use crate::textarea::history::*;

	fn mk_edit(is_insert: bool, text: &str) -> Edit {
		Edit {
			start: Pos::default(),
			end: Pos::default(),
			text: text.into(),
			is_insert,
		}
	}

	#[test]
	fn same_kind_coalesces_into_one_group() {
		let mut h = History::default();
		h.record(
			EditKind::InsertRun,
			Pos::default(),
			mk_edit(true, "a"),
			Pos::default(),
		);
		h.record(
			EditKind::InsertRun,
			Pos::default(),
			mk_edit(true, "b"),
			Pos::default(),
		);
		h.commit();
		assert_eq!(h.undo.len(), 1);
		assert_eq!(h.undo[0].edits.len(), 2);
	}

	#[test]
	fn different_kinds_close_prior_group() {
		let mut h = History::default();
		h.record(
			EditKind::InsertRun,
			Pos::default(),
			mk_edit(true, "a"),
			Pos::default(),
		);
		h.record(
			EditKind::DeleteRun,
			Pos::default(),
			mk_edit(false, "a"),
			Pos::default(),
		);
		h.commit();
		assert_eq!(h.undo.len(), 2);
	}

	#[test]
	fn new_edit_invalidates_redo() {
		let mut h = History::default();
		h.record(
			EditKind::InsertRun,
			Pos::default(),
			mk_edit(true, "a"),
			Pos::default(),
		);
		h.commit();
		let g = h.pop_undo().unwrap();
		h.push_redo(g);
		assert!(h.can_redo());
		h.record(
			EditKind::InsertRun,
			Pos::default(),
			mk_edit(true, "b"),
			Pos::default(),
		);
		assert!(!h.can_redo());
	}

	#[test]
	fn capacity_drops_oldest() {
		let mut h = History::new(2);
		for _ in 0..3 {
			h.record(
				EditKind::Other,
				Pos::default(),
				mk_edit(true, "x"),
				Pos::default(),
			);
			h.commit();
		}
		assert_eq!(h.undo.len(), 2);
	}
}

mod words {
	use crate::textarea::words::*;

	#[test]
	fn next_word_skips_spaces() {
		let s = "foo  bar  baz";
		assert_eq!(next_word_boundary_bytes(s, 0), 5);
		assert_eq!(next_word_boundary_bytes(s, 5), 10);
	}

	#[test]
	fn next_word_at_end_returns_len() {
		let s = "foo";
		assert_eq!(next_word_boundary_bytes(s, 1), 3);
		assert_eq!(next_word_boundary_bytes(s, 3), 3);
	}

	#[test]
	fn prev_word_lands_on_word_start() {
		let s = "foo bar baz";
		assert_eq!(prev_word_boundary_bytes(s, 11), 8);
		assert_eq!(prev_word_boundary_bytes(s, 8), 4);
		assert_eq!(prev_word_boundary_bytes(s, 4), 0);
	}

	#[test]
	fn prev_word_from_zero_stays_zero() {
		assert_eq!(prev_word_boundary_bytes("abc", 0), 0);
	}

	#[test]
	fn punctuation_is_own_word() {
		let s = "a,b";
		assert_eq!(next_word_boundary_bytes(s, 0), 1);
	}

	#[test]
	fn handles_unicode_word() {
		let s = "hé llo";
		assert_eq!(next_word_boundary_bytes(s, 0), 4);
	}
}

mod wrap {
	use crate::textarea::wrap::*;

	#[test]
	fn empty_line_yields_one_empty_row() {
		let rows = wrap_line(0, "", 10);
		assert_eq!(rows.len(), 1);
		assert_eq!(rows[0].end_byte, 0);
		assert!(!rows[0].trailing_hyphen);
	}

	#[test]
	fn short_line_fits_on_one_row() {
		let rows = wrap_line(0, "hello", 10);
		assert_eq!(rows.len(), 1);
		assert_eq!(rows[0].end_byte, 5);
		assert!(!rows[0].trailing_hyphen);
	}

	#[test]
	fn prefers_whitespace_break() {
		let rows = wrap_line(0, "alpha beta gamma", 8);
		let first = &"alpha beta gamma"[rows[0].start_byte..rows[0].end_byte];
		assert!(first == "alpha" || first.ends_with(' '));
		assert!(!rows[0].trailing_hyphen);
	}

	#[test]
	fn does_not_split_wide_glyph() {
		let rows = wrap_line(0, "a漢b", 2);
		let first = &"a漢b"[rows[0].start_byte..rows[0].end_byte];
		assert_eq!(first, "a");
	}

	#[test]
	fn hard_wrap_preserves_existing_newlines() {
		let out = hard_wrap("a\nb", 10);
		assert_eq!(out, "a\nb");
	}

	#[test]
	fn cols_zero_treated_as_one() {
		let rows = wrap_line(0, "ab", 0);
		assert_eq!(rows.len(), 2);
	}

	#[test]
	fn long_word_gets_soft_hyphen() {
		let text = "antidisestablishmentarianism";
		let rows = wrap_line(0, text, 10);
		assert!(rows.len() >= 2);
		assert!(rows[0].trailing_hyphen, "expected soft hyphen on first row");
		let first = &text[rows[0].start_byte..rows[0].end_byte];
		assert_eq!(first.chars().count(), 9);
	}

	#[test]
	fn wrap_display_inlines_soft_hyphen() {
		let out = wrap_display("antidisestablishmentarianism", 10);
		assert!(out[0].ends_with('-'));
		assert!(out[0].chars().count() <= 10);
	}

	#[test]
	fn breaks_after_existing_hyphen() {
		let text = "well-documented code";
		let rows = wrap_line(0, text, 6);
		let first = &text[rows[0].start_byte..rows[0].end_byte];
		assert!(
			first.ends_with('-') || first.ends_with(' ') || first == "well",
			"unexpected first row `{first}`"
		);
		assert!(!rows[0].trailing_hyphen);
	}

	#[test]
	fn short_word_after_hyphen_does_not_break_there() {
		let text = "co-op thing";
		let rows = wrap_line(0, text, 6);
		let first = &text[rows[0].start_byte..rows[0].end_byte];
		assert!(first == "co-op" || first == "co-op ");
	}

	#[test]
	fn respects_three_letter_minimum_on_synthetic_break() {
		let rows = wrap_line(0, "abcdefghij", 4);
		let first = &"abcdefghij"[rows[0].start_byte..rows[0].end_byte];
		assert_eq!(first, "abc");
		assert!(rows[0].trailing_hyphen);
		let second = &"abcdefghij"[rows[1].start_byte..rows[1].end_byte];
		assert!(second.chars().count() >= 3);
	}

	#[test]
	fn narrow_cols_fall_back_to_hard_break() {
		let rows = wrap_line(0, "abcdef", 2);
		for r in &rows {
			assert!(!r.trailing_hyphen);
		}
	}

	#[test]
	fn hard_wrap_emits_hyphen_on_synthetic_break() {
		let out = hard_wrap("antidisestablishmentarianism", 10);
		assert!(out.split('\n').next().unwrap().ends_with('-'));
	}
}

mod list {
	use crate::textarea::list::*;

	fn sample() -> Vec<ListItem> {
		vec![
			ListItem::new("/auth", "/auth").with_hint("login"),
			ListItem::new("/help", "/help"),
			ListItem::new("/recipes", "/recipes"),
		]
	}

	#[test]
	fn prefix_match_wins() {
		let mut s = ListState::new(sample());
		s.set_filter("/au");
		let first = s.filtered.first().copied().unwrap();
		assert_eq!(s.items[first].id, "/auth");
		assert_eq!(s.cursor.sel(), 0);
	}

	#[test]
	fn empty_filter_keeps_all() {
		let mut s = ListState::new(sample());
		s.set_filter("");
		assert_eq!(s.filtered.len(), 3);
	}

	#[test]
	fn no_match_empties() {
		let mut s = ListState::new(sample());
		s.set_filter("zzzz");
		assert!(s.is_empty());
		assert_eq!(s.preferred_height(), 0);
	}

	#[test]
	fn move_clamps() {
		let mut s = ListState::new(sample());
		s.set_filter("");
		s.move_up();
		assert_eq!(s.cursor.sel(), 0);
		for _ in 0..10 {
			s.move_down();
		}
		assert_eq!(s.cursor.sel(), s.filtered.len() - 1);
	}
}

mod form {
	use crate::input::{Key, KeyCode, Mods};
	use crate::textarea::form::*;

	fn key(code: KeyCode) -> Key {
		Key::with(code, Mods::NONE)
	}

	#[test]
	fn tab_cycles_fields() {
		let mut f = FormState::new(
			"t",
			vec![
				FormField::input("a", "A"),
				FormField::input("b", "B"),
				FormField::input("c", "C"),
			],
		);
		assert_eq!(f.active(), 0);
		f.handle_input(&key(KeyCode::Tab));
		assert_eq!(f.active(), 1);
		f.handle_input(&key(KeyCode::Tab));
		assert_eq!(f.active(), 2);
		f.handle_input(&key(KeyCode::Tab));
		assert_eq!(f.active(), 0);
	}

	#[test]
	fn typing_edits_active_field() {
		let mut f = FormState::new(
			"t",
			vec![FormField::input("a", "A"), FormField::input("b", "B")],
		);
		f.handle_input(&key(KeyCode::Char('h')));
		f.handle_input(&key(KeyCode::Char('i')));
		assert_eq!(
			f.values(),
			vec![("a".into(), "hi".into()), ("b".into(), "".into())]
		);
	}

	#[test]
	fn enter_submits_on_last_field() {
		let mut f = FormState::new(
			"t",
			vec![FormField::input("a", "A"), FormField::input("b", "B")],
		);
		assert_eq!(f.handle_input(&key(KeyCode::Enter)), FormOutcome::Continue);
		assert_eq!(f.active(), 1);
		assert_eq!(f.handle_input(&key(KeyCode::Enter)), FormOutcome::Submit);
	}

	#[test]
	fn ctrl_enter_always_submits() {
		let mut f = FormState::new(
			"t",
			vec![FormField::input("a", "A"), FormField::input("b", "B")],
		);
		let k = Key::with(KeyCode::Enter, Mods::CTRL);
		assert_eq!(f.handle_input(&k), FormOutcome::Submit);
	}

	#[test]
	fn esc_cancels() {
		let mut f = FormState::new("t", vec![FormField::input("a", "A")]);
		assert_eq!(f.handle_input(&key(KeyCode::Esc)), FormOutcome::Cancel);
	}

	#[test]
	fn masked_field_keeps_raw_value() {
		let mut f = FormState::new("t", vec![FormField::input("k", "key").masked()]);
		for c in "abc".chars() {
			f.handle_input(&key(KeyCode::Char(c)));
		}
		assert_eq!(f.values()[0].1, "abc");
	}
}

mod edit_area {
	use crate::render::{Cell, Frame, FrameView, Region};
	use crate::textarea::buffer::Pos;
	use crate::textarea::edit_area::{
		byte_offset_of, pos_of_byte_offset, EditArea, EditOutcome, WrapMode,
	};
	use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

	fn key(code: KeyCode) -> KeyEvent {
		KeyEvent::new(code, KeyModifiers::NONE)
	}
	fn shift(code: KeyCode) -> KeyEvent {
		KeyEvent::new(code, KeyModifiers::SHIFT)
	}
	fn ctrl(code: KeyCode) -> KeyEvent {
		KeyEvent::new(code, KeyModifiers::CONTROL)
	}
	fn alt(code: KeyCode) -> KeyEvent {
		KeyEvent::new(code, KeyModifiers::ALT)
	}

	#[test]
	fn type_insert_then_backspace() {
		let mut e = EditArea::new();
		e.handle_key(&key(KeyCode::Char('a')));
		e.handle_key(&key(KeyCode::Char('b')));
		assert_eq!(e.text(), "ab");
		e.handle_key(&key(KeyCode::Backspace));
		assert_eq!(e.text(), "a");
	}

	#[test]
	fn enter_inserts_newline() {
		let mut e = EditArea::new();
		e.handle_key(&key(KeyCode::Char('a')));
		e.handle_key(&key(KeyCode::Enter));
		e.handle_key(&key(KeyCode::Char('b')));
		assert_eq!(e.text(), "a\nb");
	}

	#[test]
	fn ctrl_enter_submits() {
		let mut e = EditArea::new();
		assert_eq!(e.handle_key(&ctrl(KeyCode::Enter)), EditOutcome::Submit);
	}

	#[test]
	fn ctrl_shift_enter_inserts_newline() {
		let mut e = EditArea::new();
		e.set_text("ab");
		e.handle_key(&key(KeyCode::End));
		let k = KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
		assert_eq!(e.handle_key(&k), EditOutcome::Handled);
		assert_eq!(e.text(), "ab\n");
	}

	#[test]
	fn ctrl_j_inserts_newline() {
		let mut e = EditArea::new();
		e.set_text("ab");
		e.handle_key(&key(KeyCode::End));
		assert_eq!(
			e.handle_key(&ctrl(KeyCode::Char('j'))),
			EditOutcome::Handled
		);
		assert_eq!(e.text(), "ab\n");
	}

	#[test]
	fn esc_cancels() {
		let mut e = EditArea::new();
		assert_eq!(e.handle_key(&key(KeyCode::Esc)), EditOutcome::Cancel);
	}

	#[test]
	fn arrow_navigation_moves_cursor() {
		let mut e = EditArea::new();
		e.set_text("abc\ndef");
		e.handle_key(&key(KeyCode::Right));
		assert_eq!(e.cursor(), Pos { line: 0, col: 1 });
		e.handle_key(&key(KeyCode::Down));
		assert_eq!(e.cursor().line, 1);
	}

	#[test]
	fn shift_right_selects_char() {
		let mut e = EditArea::new();
		e.set_text("hello");
		e.handle_key(&shift(KeyCode::Right));
		e.handle_key(&shift(KeyCode::Right));
		let sel = e.selection().unwrap();
		assert_eq!(sel.0, Pos { line: 0, col: 0 });
		assert_eq!(sel.1, Pos { line: 0, col: 2 });
	}

	#[test]
	fn ctrl_right_jumps_by_word() {
		let mut e = EditArea::new();
		e.set_text("foo bar baz");
		e.handle_key(&ctrl(KeyCode::Right));
		assert_eq!(e.cursor().col, 4);
	}

	#[test]
	fn alt_backspace_deletes_word() {
		let mut e = EditArea::new();
		e.set_text("foo bar");
		e.handle_key(&key(KeyCode::End));
		e.handle_key(&alt(KeyCode::Backspace));
		assert_eq!(e.text(), "foo ");
	}

	#[test]
	fn alt_delete_deletes_forward_word() {
		let mut e = EditArea::new();
		e.set_text("foo bar");
		e.handle_key(&alt(KeyCode::Delete));
		assert_eq!(e.text(), "bar");
	}

	fn shift_del(code: KeyCode) -> KeyEvent {
		KeyEvent::new(code, KeyModifiers::SHIFT)
	}

	#[test]
	fn shift_delete_kills_to_end_of_line_and_joins() {
		let mut e = EditArea::new();
		e.set_text("alpha\nbeta\ngamma");
		e.handle_key(&key(KeyCode::Down));
		e.handle_key(&shift_del(KeyCode::Delete));
		assert_eq!(e.text(), "alpha\ngamma");
		assert_eq!(e.cursor(), Pos { line: 1, col: 0 });
	}

	#[test]
	fn shift_delete_preserves_front_offset() {
		let mut e = EditArea::new();
		e.set_text("  hello world\nnext");
		for _ in 0..7 {
			e.handle_key(&key(KeyCode::Right));
		}
		e.handle_key(&shift_del(KeyCode::Delete));
		assert_eq!(e.text(), "  hellonext");
		assert_eq!(e.cursor(), Pos { line: 0, col: 7 });
	}

	#[test]
	fn shift_delete_on_last_line_drops_tail_only() {
		let mut e = EditArea::new();
		e.set_text("alpha\nbeta");
		e.handle_key(&key(KeyCode::Down));
		e.handle_key(&shift_del(KeyCode::Delete));
		assert_eq!(e.text(), "alpha\n");
	}

	#[test]
	fn shift_delete_clears_only_line_in_place() {
		let mut e = EditArea::new();
		e.set_text("alpha");
		e.handle_key(&shift_del(KeyCode::Delete));
		assert_eq!(e.text(), "");
		assert_eq!(e.cursor(), Pos { line: 0, col: 0 });
	}

	#[test]
	fn undo_redo_roundtrips_simple_insert() {
		let mut e = EditArea::new();
		e.handle_key(&key(KeyCode::Char('a')));
		e.handle_key(&key(KeyCode::Char('b')));
		e.history.commit();
		e.handle_key(&ctrl(KeyCode::Char('z')));
		assert_eq!(e.text(), "");
		e.handle_key(&ctrl(KeyCode::Char('y')));
		assert_eq!(e.text(), "ab");
	}

	#[test]
	fn home_end_jump() {
		let mut e = EditArea::new();
		e.set_text("abcdef");
		e.handle_key(&key(KeyCode::End));
		assert_eq!(e.cursor().col, 6);
		e.handle_key(&key(KeyCode::Home));
		assert_eq!(e.cursor().col, 0);
	}

	#[test]
	fn pgdn_moves_by_viewport() {
		let mut e = EditArea::new();
		e.set_text("a\nb\nc\nd\ne");
		e.view_h = 2;
		e.handle_key(&key(KeyCode::PageDown));
		assert_eq!(e.cursor().line, 2);
	}

	#[test]
	fn goal_col_preserved_through_vertical_motion() {
		let mut e = EditArea::new();
		e.set_text("long line here\nshort\nlong line here");
		e.handle_key(&key(KeyCode::End));
		e.handle_key(&key(KeyCode::Down));
		assert_eq!(e.cursor().line, 1);
		e.handle_key(&key(KeyCode::Down));
		assert_eq!(e.cursor().line, 2);
		assert_eq!(e.cursor().col, "long line here".len());
	}

	#[test]
	fn hard_wrap_breaks_long_line() {
		let mut e = EditArea::new();
		e.set_wrap(WrapMode::Hard);
		e.set_hard_cols(5);
		for c in "alpha beta gamma".chars() {
			e.handle_key(&key(KeyCode::Char(c)));
		}
		let txt = e.text();
		for line in txt.split('\n') {
			assert!(
				unicode_width::UnicodeWidthStr::width(line) <= 5,
				"line too wide: {line:?}"
			);
		}
	}

	#[test]
	fn render_clips_to_region_bounds() {
		let mut f = Frame::new(20, 5);
		let mut a = crate::render::GraphemeArena::new();
		f.set(15, 0, Cell::new('!'));
		f.set(0, 4, Cell::new('!'));
		let mut e = EditArea::new();
		e.set_text("hello world");
		{
			let mut v = FrameView::new(&mut f, &mut a, Region::new(2, 1, 6, 2));
			e.render(&mut v);
		}
		assert_eq!(
			f.get(15, 0).copied().unwrap().cluster,
			Cell::new('!').cluster
		);
		assert_eq!(
			f.get(0, 4).copied().unwrap().cluster,
			Cell::new('!').cluster
		);
	}

	#[test]
	fn render_writes_expected_glyphs_in_region() {
		let mut f = Frame::new(20, 3);
		let mut a = crate::render::GraphemeArena::new();
		let mut e = EditArea::new();
		e.set_text("hi");
		{
			let mut v = FrameView::new(&mut f, &mut a, Region::new(1, 0, 8, 2));
			e.render(&mut v);
		}
		assert_eq!(f.get(1, 0).unwrap().cluster, Cell::new('h').cluster);
		assert_eq!(f.get(2, 0).unwrap().cluster, Cell::new('i').cluster);
	}

	#[test]
	fn select_all_then_delete_clears_buffer() {
		let mut e = EditArea::new();
		e.set_text("hello\nworld");
		e.handle_key(&ctrl(KeyCode::Char('a')));
		e.handle_key(&key(KeyCode::Backspace));
		assert_eq!(e.text(), "");
	}

	#[test]
	fn typing_into_selection_replaces_it() {
		let mut e = EditArea::new();
		e.set_text("hello");
		e.handle_key(&ctrl(KeyCode::Char('a')));
		e.handle_key(&key(KeyCode::Char('x')));
		assert_eq!(e.text(), "x");
	}

	#[test]
	fn byte_offset_roundtrip() {
		let s = "one\ntwo\nthree";
		let p = Pos { line: 2, col: 3 };
		let off = byte_offset_of(s, p);
		let r = pos_of_byte_offset(s, off);
		assert_eq!(r, p);
	}
}
