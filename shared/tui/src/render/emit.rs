use std::fmt::Write as _;

use super::cell::{ascii_from_cluster, Attrs, Cell, Color};
use super::frame::Frame;
use super::grapheme::GraphemeArena;

#[derive(Default)]
pub struct SgrCache {
	last: Option<(Color, Color, Attrs)>,
}

impl SgrCache {
	pub fn reset(&mut self) {
		self.last = None;
	}

	pub fn apply(&mut self, out: &mut String, cell: &Cell) {
		let next = (cell.fg, cell.bg, cell.attrs);
		match self.last {
			Some(prev) if prev == next => return,
			Some((pfg, pbg, pattrs)) => {
				emit_delta_attrs(out, pattrs, cell.attrs);
				if pfg != cell.fg {
					emit_fg(out, cell.fg);
				}
				if pbg != cell.bg {
					emit_bg(out, cell.bg);
				}
			}
			None => {
				emit_set_attrs(out, cell.attrs);
				emit_fg(out, cell.fg);
				emit_bg(out, cell.bg);
			}
		}
		self.last = Some(next);
	}
}

fn emit_set_attrs(out: &mut String, new: Attrs) {
	let mut started = false;
	macro_rules! p {
		($s:literal) => {
			if !started { out.push_str("\x1b["); started = true; } else { out.push(';'); }
			out.push_str($s);
		}
	}
	if new.contains(Attrs::BOLD)          { p!("1"); }
	if new.contains(Attrs::DIM)           { p!("2"); }
	if new.contains(Attrs::ITALIC)        { p!("3"); }
	if new.contains(Attrs::UNDERLINE)     { p!("4"); }
	if new.contains(Attrs::INVERSE)       { p!("7"); }
	if new.contains(Attrs::STRIKETHROUGH) { p!("9"); }
	if started { out.push('m'); }
}

fn emit_delta_attrs(out: &mut String, prev: Attrs, new: Attrs) {
	let removed = Attrs(prev.0 & !new.0);
	let added = Attrs(new.0 & !prev.0);
	let mut started = false;
	macro_rules! p {
		($s:literal) => {
			if !started { out.push_str("\x1b["); started = true; } else { out.push(';'); }
			out.push_str($s);
		}
	}
	let mut clear_bold_dim = false;
	if removed.contains(Attrs::BOLD) || removed.contains(Attrs::DIM) {
		p!("22");
		clear_bold_dim = true;
	}
	if removed.contains(Attrs::ITALIC)        { p!("23"); }
	if removed.contains(Attrs::UNDERLINE)     { p!("24"); }
	if removed.contains(Attrs::INVERSE)       { p!("27"); }
	if removed.contains(Attrs::STRIKETHROUGH) { p!("29"); }
	if added.contains(Attrs::BOLD) || (clear_bold_dim && new.contains(Attrs::BOLD)) { p!("1"); }
	if added.contains(Attrs::DIM)  || (clear_bold_dim && new.contains(Attrs::DIM))  { p!("2"); }
	if added.contains(Attrs::ITALIC)        { p!("3"); }
	if added.contains(Attrs::UNDERLINE)     { p!("4"); }
	if added.contains(Attrs::INVERSE)       { p!("7"); }
	if added.contains(Attrs::STRIKETHROUGH) { p!("9"); }
	if started { out.push('m'); }
}

fn emit_fg(out: &mut String, c: Color) {
	match c {
		Color::Default => out.push_str("\x1b[39m"),
		Color::Rgb(r, g, b) => {
			let _ = write!(out, "\x1b[38;2;{};{};{}m", r, g, b);
		}
		Color::Indexed(n) => {
			let _ = write!(out, "\x1b[38;5;{}m", n);
		}
	}
}

fn emit_bg(out: &mut String, c: Color) {
	match c {
		Color::Default => out.push_str("\x1b[49m"),
		Color::Rgb(r, g, b) => {
			let _ = write!(out, "\x1b[48;2;{};{};{}m", r, g, b);
		}
		Color::Indexed(n) => {
			let _ = write!(out, "\x1b[48;5;{}m", n);
		}
	}
}

fn cup(out: &mut String, row: u16, col: u16) {
	let _ = write!(out, "\x1b[{};{}H", row + 1, col + 1);
}

fn write_glyph(out: &mut String, cell: &Cell, arena: &GraphemeArena) {
	if cell.is_continuation() {
		return;
	}
	if let Some(b) = ascii_from_cluster(cell.cluster) {
		out.push(b as char);
		return;
	}
	out.push_str(arena.get(cell.cluster));
}

pub fn full(out: &mut String, next: &Frame, arena: &GraphemeArena) {
	out.push_str("\x1b[H");
	let mut sgr = SgrCache::default();
	for y in 0..next.h {
		cup(out, y, 0);
		for x in 0..next.w {
			let i = (y as usize) * (next.w as usize) + (x as usize);
			let c = &next.cells[i];
			if c.is_continuation() {
				continue;
			}
			sgr.apply(out, c);
			write_glyph(out, c, arena);
		}
	}
	out.push_str("\x1b[0m");
}

pub fn lines(out: &mut String, cur: &Frame, next: &Frame, arena: &GraphemeArena) {
	let mut sgr = SgrCache::default();
	let w = next.w as usize;
	for y in 0..next.h as usize {
		let start = y * w;
		let end = start + w;
		if cur.cells[start..end] == next.cells[start..end] {
			continue;
		}
		cup(out, y as u16, 0);
		sgr.reset();
		for x in 0..w {
			let c = &next.cells[start + x];
			if c.is_continuation() {
				continue;
			}
			sgr.apply(out, c);
			write_glyph(out, c, arena);
		}
	}
	out.push_str("\x1b[0m");
}

pub fn cells(out: &mut String, cur: &Frame, next: &Frame, arena: &GraphemeArena) {
	let mut sgr = SgrCache::default();
	let w = next.w as usize;
	let mut cursor: Option<(u16, u16)> = None;
	for y in 0..next.h as usize {
		for x in 0..w {
			let i = y * w + x;
			if cur.cells[i] == next.cells[i] {
				continue;
			}
			let c = &next.cells[i];
			if c.is_continuation() {
				continue;
			}
			let here = (x as u16, y as u16);
			match cursor {
				Some((cx, cy)) if cy == here.1 && cx == here.0 => {}
				_ => cup(out, here.1, here.0),
			}
			sgr.apply(out, c);
			write_glyph(out, c, arena);
			let advance = c.width.max(1) as u16;
			cursor = Some((here.0.saturating_add(advance), here.1));
		}
	}
	out.push_str("\x1b[0m");
}
