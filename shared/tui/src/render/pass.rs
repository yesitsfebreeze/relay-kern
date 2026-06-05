use super::frame::Frame;
use super::grapheme::GraphemeArena;

#[derive(Clone, Copy, Debug)]
pub struct PassCtx {
	pub frame: u64,
	pub elapsed_secs: f32,
	pub fps: f32,
	pub last_strategy: super::diff::Strategy,
}

pub trait FramePass {
	fn name(&self) -> &str {
		"pass"
	}

	fn apply(&mut self, frame: &mut Frame, arena: &mut GraphemeArena, ctx: &PassCtx);
}

pub struct DebugOverlay {
	enabled: bool,
}

impl DebugOverlay {
	pub fn new() -> Self {
		DebugOverlay { enabled: true }
	}

	pub fn set_enabled(&mut self, on: bool) {
		self.enabled = on;
	}

	pub fn enabled(&self) -> bool {
		self.enabled
	}

	pub fn toggle(&mut self) {
		self.enabled = !self.enabled;
	}
}

impl Default for DebugOverlay {
	fn default() -> Self {
		Self::new()
	}
}

impl FramePass for DebugOverlay {
	fn name(&self) -> &str {
		"debug-overlay"
	}

	fn apply(&mut self, frame: &mut Frame, arena: &mut GraphemeArena, ctx: &PassCtx) {
		if !self.enabled {
			return;
		}
		use super::cell::{Attrs, Color};
		use super::diff::Strategy;

		let strat = match ctx.last_strategy {
			Strategy::Full => "full ",
			Strategy::Lines => "lines",
			Strategy::Cells => "cells",
			Strategy::Noop => "noop ",
		};
		let line = format!(" fps {:>5.1} | {} | f {:>6} ", ctx.fps, strat, ctx.frame);
		let w = line.chars().count() as u16;
		if frame.w < w + 1 || frame.h == 0 {
			return;
		}
		let x = frame.w - w - 1;
		let y = 0;
		let fg = Color::Default;
		let bg = Color::Default;
		let attrs = Attrs::BOLD | Attrs::INVERSE;
		frame.put_str(arena, x, y, &line, fg, bg, attrs);
	}
}
