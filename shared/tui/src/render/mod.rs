pub mod cell;
pub mod diff;
mod emit;
pub mod frame;
pub mod grapheme;
pub mod pass;
pub mod region;
pub mod snapshot;
pub mod surface;
pub mod sync;
pub mod theme;
pub mod theme_config;
pub mod ws_surface;

#[cfg(test)]
mod tests;

use std::io::{self, Write};
use std::time::Instant;

pub use cell::{Attrs, Cell, Color};
pub use diff::Strategy;
pub use frame::Frame;
pub use grapheme::{ClusterId, GraphemeArena};
pub use pass::{DebugOverlay, FramePass, PassCtx};
pub use region::{FrameView, Region};
pub use snapshot::{ReplayError, Snapshot, VtReplay};
pub use surface::{BufferSurface, Capabilities, StdoutSurface, Surface};
pub use sync::detect_sync_update_support;
pub use theme::{Style, StyleRole, StyleSet};
pub use ws_surface::WsSurface;

pub const SYNC_BEGIN: &str = "\x1b[?2026h";
pub const SYNC_END: &str = "\x1b[?2026l";

pub struct Renderer {
	pub(crate) current: Frame,
	pub(crate) next: Frame,
	arena: GraphemeArena,
	buf: String,
	write_buf: Vec<u8>,
	force_full_next: bool,
	last: Strategy,
	supports_sync_update: bool,
	pub(crate) passes: Option<Vec<Box<dyn FramePass>>>,
	start: Instant,
	frame_counter: u64,
	fps_ema: f32,
	last_flush: Option<Instant>,
}

impl Renderer {
	pub fn new(w: u16, h: u16) -> Self {
		Renderer {
			current: Frame::new(w, h),
			next: Frame::new(w, h),
			arena: GraphemeArena::new(),
			buf: String::with_capacity(64 * 1024),
			write_buf: Vec::with_capacity(64 * 1024),
			force_full_next: true,
			last: Strategy::Full,
			supports_sync_update: detect_sync_update_support(),
			passes: None,
			start: Instant::now(),
			frame_counter: 0,
			fps_ema: 0.0,
			last_flush: None,
		}
	}

	pub fn add_pass<P: FramePass + 'static>(&mut self, pass: P) {
		self
			.passes
			.get_or_insert_with(|| Vec::with_capacity(2))
			.push(Box::new(pass));
	}

	pub fn clear_passes(&mut self) {
		if let Some(v) = self.passes.as_mut() {
			v.clear();
		}
	}

	pub fn pass_count(&self) -> usize {
		self.passes.as_ref().map(|v| v.len()).unwrap_or(0)
	}

	#[inline]
	fn run_passes(&mut self) {
		let Some(passes) = self.passes.as_mut() else {
			return;
		};
		if passes.is_empty() {
			return;
		}
		let ctx = PassCtx {
			frame: self.frame_counter,
			elapsed_secs: self.start.elapsed().as_secs_f32(),
			fps: self.fps_ema,
			last_strategy: self.last,
		};
		for p in passes.iter_mut() {
			p.apply(&mut self.next, &mut self.arena, &ctx);
		}
	}

	pub fn set_supports_sync_update(&mut self, on: bool) {
		self.supports_sync_update = on;
	}

	pub fn supports_sync_update(&self) -> bool {
		self.supports_sync_update
	}

	pub fn size(&self) -> (u16, u16) {
		(self.next.w, self.next.h)
	}

	pub fn resize(&mut self, w: u16, h: u16) {
		self.current.resize(w, h);
		self.next.resize(w, h);
		self.force_full_next = true;
	}

	pub fn frame(&mut self) -> &mut Frame {
		&mut self.next
	}

	pub fn put_str(&mut self, x: u16, y: u16, s: &str, fg: Color, bg: Color, attrs: Attrs) {
		self.next.put_str(&mut self.arena, x, y, s, fg, bg, attrs);
	}

	pub fn arena(&self) -> &GraphemeArena {
		&self.arena
	}

	pub fn arena_mut(&mut self) -> &mut GraphemeArena {
		&mut self.arena
	}

	pub fn frame_and_arena(&mut self) -> (&mut Frame, &mut GraphemeArena) {
		(&mut self.next, &mut self.arena)
	}

	pub fn frame_view(&mut self, region: self::region::Region) -> self::region::FrameView<'_> {
		let (f, a) = self.frame_and_arena();
		self::region::FrameView::new(f, a, region)
	}

	pub fn last_strategy(&self) -> Strategy {
		self.last
	}

	pub fn current_frame_ref(&self) -> &Frame {
		&self.current
	}

	pub fn next_frame_ref(&self) -> &Frame {
		&self.next
	}

	pub fn snapshot(&self) -> snapshot::Snapshot {
		snapshot::Snapshot::capture(
			(self.next.w, self.next.h),
			&self.current,
			&self.next,
			&self.arena,
			self.force_full_next,
			self.last,
			self.frame_counter,
		)
	}

	pub fn restore(&mut self, snap: &snapshot::Snapshot) {
		let (w, h) = snap.size;
		if (self.next.w, self.next.h) != (w, h) {
			self.current.resize(w, h);
			self.next.resize(w, h);
		}
		self.current.cells.clone_from(&snap.current);
		self.next.cells.clone_from(&snap.next);
		self.arena = GraphemeArena::new();
		for (i, cluster) in snap.arena_clusters.iter().enumerate() {
			if i == 0 {
				continue;
			}
			self.arena.intern(cluster);
		}
		self.force_full_next = true;
		self.last = snap.last_strategy;
		self.frame_counter = snap.frame_counter;
	}

	pub fn present<S: Surface + ?Sized>(&mut self, surface: &mut S) -> io::Result<Strategy> {
		let (w, h) = surface.size();
		if (w, h) != (self.next.w, self.next.h) {
			self.resize(w, h);
		}
		let caps = surface.capabilities();
		let prev_sync = self.supports_sync_update;
		self.supports_sync_update = caps.sync_update;
		let mut buf = std::mem::take(&mut self.write_buf);
		buf.clear();
		let strat = self.flush(&mut buf);
		self.supports_sync_update = prev_sync;
		let strat = strat?;
		surface.write_frame(&buf)?;
		self.write_buf = buf;
		Ok(strat)
	}

	pub fn flush<W: Write>(&mut self, w: &mut W) -> io::Result<Strategy> {
		self.run_passes();
		self.buf.clear();
		let strat = if self.force_full_next {
			self.force_full_next = false;
			emit::full(&mut self.buf, &self.next, &self.arena);
			Strategy::Full
		} else {
			let s = diff::stats(&self.current, &self.next);
			let pick = diff::pick(&s);
			match pick {
				Strategy::Full => emit::full(&mut self.buf, &self.next, &self.arena),
				Strategy::Lines => emit::lines(&mut self.buf, &self.current, &self.next, &self.arena),
				Strategy::Cells => emit::cells(&mut self.buf, &self.current, &self.next, &self.arena),
				Strategy::Noop => {}
			}
			pick
		};
		if !self.buf.is_empty() {
			if self.supports_sync_update {
				w.write_all(SYNC_BEGIN.as_bytes())?;
			}
			w.write_all(self.buf.as_bytes())?;
			if self.supports_sync_update {
				w.write_all(SYNC_END.as_bytes())?;
			}
			w.flush()?;
		}
		std::mem::swap(&mut self.current, &mut self.next);
		if matches!(strat, Strategy::Full) && self.arena.over_capacity() {
			self.arena.reset();
			self.current.clear();
			self.next.clear();
			self.force_full_next = true;
		}
		self.last = strat;
		let now = Instant::now();
		if let Some(prev) = self.last_flush {
			let dt = now.duration_since(prev).as_secs_f32();
			if dt > 0.0 {
				let inst = 1.0 / dt;
				self.fps_ema = if self.fps_ema == 0.0 {
					inst
				} else {
					self.fps_ema * 0.8 + inst * 0.2
				};
			}
		}
		self.last_flush = Some(now);
		self.frame_counter = self.frame_counter.saturating_add(1);
		Ok(strat)
	}
}
