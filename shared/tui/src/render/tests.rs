mod grapheme {
	use crate::render::grapheme::*;

	#[test]
	fn blank_is_reserved_at_zero() {
		let a = GraphemeArena::new();
		assert_eq!(a.get(BLANK), " ");
	}

	#[test]
	fn interning_dedups() {
		let mut a = GraphemeArena::new();
		let a1 = a.intern("é");
		let a2 = a.intern("é");
		assert_eq!(a1, a2);
		assert_eq!(a.len(), 2);
	}

	#[test]
	fn distinct_clusters_get_distinct_ids() {
		let mut a = GraphemeArena::new();
		let x = a.intern("a");
		let y = a.intern("b");
		assert_ne!(x, y);
		assert_eq!(a.get(x), "a");
		assert_eq!(a.get(y), "b");
	}

	#[test]
	fn combining_mark_cluster_roundtrips() {
		let mut a = GraphemeArena::new();
		let id = a.intern("e\u{0301}");
		assert_eq!(a.get(id), "e\u{0301}");
	}

	#[test]
	fn continuation_returns_empty() {
		let a = GraphemeArena::new();
		assert_eq!(a.get(CONTINUATION), "");
	}

	#[test]
	fn empty_string_maps_to_blank() {
		let mut a = GraphemeArena::new();
		assert_eq!(a.intern(""), BLANK);
	}
}

mod sync {
	use crate::render::sync::*;
	use std::env;
	use std::sync::Mutex;

	static ENV_LOCK: Mutex<()> = Mutex::new(());

	struct EnvGuard {
		_lock: std::sync::MutexGuard<'static, ()>,
		wt: Option<std::ffi::OsString>,
		tp: Option<std::ffi::OsString>,
		term: Option<std::ffi::OsString>,
	}

	impl EnvGuard {
		fn new() -> Self {
			let lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
			let g = EnvGuard {
				_lock: lock,
				wt: env::var_os("WT_SESSION"),
				tp: env::var_os("TERM_PROGRAM"),
				term: env::var_os("TERM"),
			};
			env::remove_var("WT_SESSION");
			env::remove_var("TERM_PROGRAM");
			env::remove_var("TERM");
			g
		}
	}

	impl Drop for EnvGuard {
		fn drop(&mut self) {
			match &self.wt {
				Some(v) => env::set_var("WT_SESSION", v),
				None => env::remove_var("WT_SESSION"),
			}
			match &self.tp {
				Some(v) => env::set_var("TERM_PROGRAM", v),
				None => env::remove_var("TERM_PROGRAM"),
			}
			match &self.term {
				Some(v) => env::set_var("TERM", v),
				None => env::remove_var("TERM"),
			}
		}
	}

	#[test]
	fn detects_windows_terminal() {
		let _g = EnvGuard::new();
		env::set_var("WT_SESSION", "abc");
		assert!(detect_sync_update_support());
	}

	#[test]
	fn detects_iterm() {
		let _g = EnvGuard::new();
		env::set_var("TERM_PROGRAM", "iTerm.app");
		assert!(detect_sync_update_support());
	}

	#[test]
	fn detects_wezterm() {
		let _g = EnvGuard::new();
		env::set_var("TERM_PROGRAM", "WezTerm");
		assert!(detect_sync_update_support());
	}

	#[test]
	fn detects_kitty() {
		let _g = EnvGuard::new();
		env::set_var("TERM", "xterm-kitty");
		assert!(detect_sync_update_support());
	}

	#[test]
	fn detects_foot() {
		let _g = EnvGuard::new();
		env::set_var("TERM", "foot");
		assert!(detect_sync_update_support());
	}

	#[test]
	fn unknown_terminal_returns_false() {
		let _g = EnvGuard::new();
		env::set_var("TERM", "xterm-256color");
		assert!(!detect_sync_update_support());
	}
}

mod theme {
	use crate::render::cell::Color;
	use crate::render::theme::*;

	#[test]
	fn default_uses_ansi_indices_only() {
		let s = StyleSet::default();
		for role in StyleRole::ALL {
			let style = s.get(role);
			for c in [style.fg, style.bg] {
				match c {
					Color::Default | Color::Indexed(_) => {}
					Color::Rgb(_, _, _) => panic!("default_ansi must not use RGB for {:?}", role),
				}
			}
		}
	}

	#[test]
	fn default_ansi_indices_in_range() {
		let s = StyleSet::default_ansi();
		for role in StyleRole::ALL {
			let style = s.get(role);
			for c in [style.fg, style.bg] {
				if let Color::Indexed(n) = c {
					assert!(n < 16, "role {:?} uses index {} outside 0-15", role, n);
				}
			}
		}
	}

	#[test]
	fn dark_rgb_uses_rgb_for_every_role() {
		let s = StyleSet::dark_rgb();
		for role in StyleRole::ALL {
			let style = s.get(role);
			assert!(matches!(style.fg, Color::Rgb(_, _, _)), "fg for {:?}", role);
		}
	}

	#[test]
	fn set_overrides_role() {
		let mut s = StyleSet::default();
		let replacement = Style::fg(Color::Indexed(5));
		s.set(StyleRole::Accent, replacement);
		assert_eq!(s.get(StyleRole::Accent), replacement);
	}

	#[test]
	fn roles_are_distinct() {
		let all = StyleRole::ALL;
		for (i, a) in all.iter().enumerate() {
			for b in &all[i + 1..] {
				assert_ne!(a, b);
			}
		}
	}
}

mod region {
	use crate::render::cell::{Attrs, Cell, Color};
	use crate::render::frame::Frame;
	use crate::render::grapheme::GraphemeArena;
	use crate::render::region::*;

	fn frame(w: u16, h: u16) -> Frame {
		Frame::new(w, h)
	}

	#[test]
	fn split_h_divides_widths() {
		let r = Region::new(0, 0, 10, 4);
		let (l, right) = r.split_h(3);
		assert_eq!(l, Region::new(0, 0, 3, 4));
		assert_eq!(right, Region::new(3, 0, 7, 4));
	}

	#[test]
	fn split_h_clamps_when_over_width() {
		let r = Region::new(2, 1, 5, 3);
		let (l, right) = r.split_h(99);
		assert_eq!(l, Region::new(2, 1, 5, 3));
		assert_eq!(right.w, 0);
	}

	#[test]
	fn split_v_divides_heights() {
		let r = Region::new(0, 0, 6, 10);
		let (top, bot) = r.split_v(4);
		assert_eq!(top, Region::new(0, 0, 6, 4));
		assert_eq!(bot, Region::new(0, 4, 6, 6));
	}

	#[test]
	fn center_places_sub_region() {
		let r = Region::new(0, 0, 10, 10);
		assert_eq!(r.center(4, 4), Region::new(3, 3, 4, 4));
	}

	#[test]
	fn center_clamps_oversized() {
		let r = Region::new(0, 0, 4, 4);
		assert_eq!(r.center(10, 10), Region::new(0, 0, 4, 4));
	}

	#[test]
	fn pad_shrinks_all_sides() {
		let r = Region::new(0, 0, 10, 10);
		assert_eq!(r.pad(1, 2, 3, 4), Region::new(1, 2, 6, 4));
	}

	#[test]
	fn pad_saturates_at_empty() {
		let r = Region::new(0, 0, 4, 4);
		let p = r.pad(10, 10, 10, 10);
		assert!(p.is_empty());
	}

	#[test]
	fn view_write_inside_region_reaches_frame() {
		let mut f = frame(10, 4);
		let mut a = GraphemeArena::new();
		let mut v = FrameView::new(&mut f, &mut a, Region::new(2, 1, 4, 2));
		v.set(0, 0, Cell::new('A'));
		v.set(3, 1, Cell::new('B'));
		assert_eq!(f.get(2, 1).copied(), Some(Cell::new('A')));
		assert_eq!(f.get(5, 2).copied(), Some(Cell::new('B')));
	}

	#[test]
	fn view_write_outside_region_is_noop() {
		let mut f = frame(10, 4);
		let mut a = GraphemeArena::new();
		f.set(6, 1, Cell::new('!'));
		f.set(2, 3, Cell::new('!'));
		{
			let mut v = FrameView::new(&mut f, &mut a, Region::new(2, 1, 4, 2));
			v.set(4, 0, Cell::new('X'));
			v.set(10, 0, Cell::new('X'));
			v.set(0, 2, Cell::new('X'));
			v.set(0, 99, Cell::new('X'));
			v.put_str(0, 5, "clipped", Color::Default, Color::Default, Attrs::NONE);
		}
		assert_eq!(f.get(6, 1).copied(), Some(Cell::new('!')));
		assert_eq!(f.get(2, 3).copied(), Some(Cell::new('!')));
	}

	#[test]
	fn view_put_str_clips_at_right_edge() {
		let mut f = frame(10, 2);
		let mut a = GraphemeArena::new();
		f.set(6, 0, Cell::new('!'));
		{
			let mut v = FrameView::new(&mut f, &mut a, Region::new(2, 0, 4, 1));
			v.put_str(
				0,
				0,
				"ABCDEFGH",
				Color::Default,
				Color::Default,
				Attrs::NONE,
			);
		}
		let c2 = f.get(2, 0).copied().unwrap();
		let c5 = f.get(5, 0).copied().unwrap();
		assert_eq!(a.get(c2.cluster), "A");
		assert_eq!(a.get(c5.cluster), "D");
		assert_eq!(f.get(6, 0).copied(), Some(Cell::new('!')));
	}

	#[test]
	fn view_region_clamped_to_frame() {
		let mut f = frame(4, 4);
		let mut a = GraphemeArena::new();
		let v = FrameView::new(&mut f, &mut a, Region::new(2, 2, 99, 99));
		assert_eq!(v.region(), Region::new(2, 2, 2, 2));
	}

	#[test]
	fn sub_view_intersects_parent() {
		let mut f = frame(20, 10);
		let mut a = GraphemeArena::new();
		let mut v = FrameView::new(&mut f, &mut a, Region::new(4, 2, 8, 4));
		let inner = v.sub(Region::new(2, 1, 20, 20));
		assert_eq!(inner.region(), Region::new(6, 3, 6, 3));
	}

	#[test]
	fn fill_only_touches_region() {
		let mut f = frame(6, 4);
		let mut a = GraphemeArena::new();
		f.set(5, 3, Cell::new('!'));
		{
			let mut v = FrameView::new(&mut f, &mut a, Region::new(1, 1, 3, 2));
			v.fill(Cell::new('#'));
		}
		assert_eq!(f.get(1, 1).copied(), Some(Cell::new('#')));
		assert_eq!(f.get(3, 2).copied(), Some(Cell::new('#')));
		assert_eq!(f.get(0, 0).copied(), Some(Cell::default()));
		assert_eq!(f.get(4, 1).copied(), Some(Cell::default()));
		assert_eq!(f.get(5, 3).copied(), Some(Cell::new('!')));
	}

	#[test]
	fn view_put_str_handles_unicode() {
		let mut f = frame(10, 1);
		let mut a = GraphemeArena::new();
		{
			let mut v = FrameView::new(&mut f, &mut a, Region::new(0, 0, 10, 1));
			v.put_str(0, 0, "a—b", Color::Default, Color::Default, Attrs::NONE);
		}
		let dash = f.get(1, 0).copied().unwrap();
		assert_eq!(a.get(dash.cluster), "—");
	}
}

mod pass {
	use crate::render::cell::{Attrs, Cell, Color};
	use crate::render::diff::Strategy;
	use crate::render::frame::Frame;
	use crate::render::grapheme::GraphemeArena;
	use crate::render::pass::*;

	fn mk_ctx() -> PassCtx {
		PassCtx {
			frame: 42,
			elapsed_secs: 1.0,
			fps: 60.0,
			last_strategy: Strategy::Cells,
		}
	}

	#[test]
	fn debug_overlay_writes_to_top_right_when_enabled() {
		let mut f = Frame::new(40, 3);
		let mut a = GraphemeArena::new();
		let mut pass = DebugOverlay::new();
		pass.apply(&mut f, &mut a, &mk_ctx());
		let any_written =
			(0..f.w).any(|x| f.get(x, 0).copied().unwrap_or(Cell::default()) != Cell::default());
		assert!(any_written, "debug overlay should mutate row 0");
	}

	#[test]
	fn debug_overlay_noop_when_disabled() {
		let mut f = Frame::new(40, 3);
		let mut a = GraphemeArena::new();
		let mut pass = DebugOverlay::new();
		pass.set_enabled(false);
		pass.apply(&mut f, &mut a, &mk_ctx());
		for c in &f.cells {
			assert_eq!(*c, Cell::default());
		}
	}

	#[test]
	fn debug_overlay_skips_when_frame_too_narrow() {
		let mut f = Frame::new(5, 1);
		let mut a = GraphemeArena::new();
		let mut pass = DebugOverlay::new();
		pass.apply(&mut f, &mut a, &mk_ctx());
		for c in &f.cells {
			assert_eq!(*c, Cell::default());
		}
	}

	#[test]
	fn custom_pass_composes_after_earlier_passes() {
		struct Red;
		impl FramePass for Red {
			fn apply(&mut self, f: &mut Frame, _a: &mut GraphemeArena, _c: &PassCtx) {
				let cell = Cell::new(' ').style(Color::Default, Color::Rgb(255, 0, 0), Attrs::NONE);
				f.fill(cell);
			}
		}
		struct Marker;
		impl FramePass for Marker {
			fn apply(&mut self, f: &mut Frame, a: &mut GraphemeArena, _c: &PassCtx) {
				f.put_str(a, 0, 0, "Z", Color::Default, Color::Default, Attrs::NONE);
			}
		}

		let mut f = Frame::new(4, 2);
		let mut a = GraphemeArena::new();
		let ctx = mk_ctx();
		let mut r = Red;
		r.apply(&mut f, &mut a, &ctx);
		let mut m = Marker;
		m.apply(&mut f, &mut a, &ctx);

		let top = *f.get(0, 0).unwrap();
		assert_ne!(top.cluster, crate::render::grapheme::BLANK);
		let next = *f.get(1, 0).unwrap();
		assert_eq!(next.bg, Color::Rgb(255, 0, 0));
	}
}

mod surface {
	use crate::render::surface::*;

	#[test]
	fn buffer_surface_captures_bytes() {
		let mut s = BufferSurface::new(80, 24, Capabilities::MODERN);
		s.write_frame(b"hello").unwrap();
		s.write_frame(b" world").unwrap();
		assert_eq!(s.bytes(), b"hello world");
	}

	#[test]
	fn buffer_surface_take_empties() {
		let mut s = BufferSurface::new(80, 24, Capabilities::MINIMAL);
		s.write_frame(b"abc").unwrap();
		let taken = s.take();
		assert_eq!(taken, b"abc");
		assert!(s.bytes().is_empty());
	}

	#[test]
	fn buffer_surface_reports_size_and_caps() {
		let s = BufferSurface::new(120, 40, Capabilities::MODERN);
		assert_eq!(s.size(), (120, 40));
		assert!(s.capabilities().truecolor);
		assert!(s.capabilities().sync_update);
	}

	#[test]
	fn buffer_surface_resize_updates_size() {
		let mut s = BufferSurface::new(10, 5, Capabilities::MINIMAL);
		s.resize(40, 20);
		assert_eq!(s.size(), (40, 20));
	}

	#[test]
	#[allow(clippy::assertions_on_constants)]
	fn minimal_capabilities_are_conservative() {
		assert!(!Capabilities::MINIMAL.truecolor);
		assert!(!Capabilities::MINIMAL.sync_update);
	}

	#[test]
	#[allow(clippy::assertions_on_constants)]
	fn modern_capabilities_enable_everything() {
		assert!(Capabilities::MODERN.truecolor);
		assert!(Capabilities::MODERN.sync_update);
	}

	#[test]
	fn stdout_surface_round_trips_size() {
		let mut s = StdoutSurface::new(80, 24);
		assert_eq!(s.size(), (80, 24));
		s.refresh_size(100, 30);
		assert_eq!(s.size(), (100, 30));
	}

	#[test]
	fn stdout_surface_set_capabilities_overrides() {
		let mut s = StdoutSurface::new(80, 24);
		s.set_capabilities(Capabilities::MINIMAL);
		assert_eq!(s.capabilities(), Capabilities::MINIMAL);
	}

	#[test]
	fn buffer_surface_empty_write_is_noop() {
		let mut s = BufferSurface::new(10, 3, Capabilities::MINIMAL);
		s.write_frame(b"").unwrap();
		assert!(s.bytes().is_empty());
	}

	#[test]
	fn surface_is_object_safe() {
		let mut s: Box<dyn Surface> = Box::new(BufferSurface::new(4, 2, Capabilities::MINIMAL));
		s.write_frame(b"x").unwrap();
		assert_eq!(s.size(), (4, 2));
	}
}

mod ws_surface {
	use crate::render::surface::{Capabilities, Surface};
	use crate::render::ws_surface::*;
	use std::io;
	use std::io::Read;
	use std::net::{TcpListener, TcpStream};
	use std::thread;

	#[test]
	#[allow(clippy::assertions_on_constants)] // regression guard on the XTERM_JS const profile
	fn xtermjs_capabilities_advertises_truecolor_no_sync() {
		assert!(Capabilities::XTERM_JS.truecolor);
		assert!(!Capabilities::XTERM_JS.sync_update);
	}

	#[test]
	fn ws_surface_reports_size_and_caps() {
		let s = WsSurface::new(Vec::<u8>::new(), 80, 24);
		assert_eq!(s.size(), (80, 24));
		assert_eq!(s.capabilities(), Capabilities::XTERM_JS);
	}

	#[test]
	fn ws_surface_resize_updates_size() {
		let mut s = WsSurface::new(Vec::<u8>::new(), 10, 5);
		s.resize(120, 40);
		assert_eq!(s.size(), (120, 40));
	}

	#[test]
	fn ws_surface_with_custom_capabilities() {
		let s = WsSurface::with_capabilities(Vec::<u8>::new(), 4, 2, Capabilities::MINIMAL);
		assert_eq!(s.capabilities(), Capabilities::MINIMAL);
	}

	#[test]
	fn empty_write_is_noop() {
		let mut s = WsSurface::new(Vec::<u8>::new(), 4, 2);
		s.write_frame(b"").unwrap();
		assert!(s.into_inner().is_empty());
	}

	#[test]
	fn small_payload_uses_7bit_length() {
		let mut s = WsSurface::new(Vec::<u8>::new(), 4, 2);
		s.write_frame(b"hello").unwrap();
		let out = s.into_inner();
		assert_eq!(out[0], 0x82);
		assert_eq!(out[1], 5);
		assert_eq!(&out[2..], b"hello");
	}

	#[test]
	fn medium_payload_uses_16bit_length() {
		let payload = vec![0xAA_u8; 200];
		let mut s = WsSurface::new(Vec::<u8>::new(), 4, 2);
		s.write_frame(&payload).unwrap();
		let out = s.into_inner();
		assert_eq!(out[0], 0x82);
		assert_eq!(out[1], 126);
		assert_eq!(u16::from_be_bytes([out[2], out[3]]) as usize, 200);
		assert_eq!(&out[4..], &payload[..]);
	}

	#[test]
	fn large_payload_uses_64bit_length() {
		let payload = vec![0x55_u8; 70_000];
		let mut s = WsSurface::new(Vec::<u8>::new(), 4, 2);
		s.write_frame(&payload).unwrap();
		let out = s.into_inner();
		assert_eq!(out[0], 0x82);
		assert_eq!(out[1], 127);
		let len = u64::from_be_bytes([
			out[2], out[3], out[4], out[5], out[6], out[7], out[8], out[9],
		]);
		assert_eq!(len as usize, 70_000);
		assert_eq!(&out[10..], &payload[..]);
	}

	#[test]
	fn oversized_payload_is_rejected() {
		let mut s = WsSurface::new(Vec::<u8>::new(), 4, 2);
		let big = vec![0u8; MAX_FRAME_BYTES + 1];
		let err = s.write_frame(&big).unwrap_err();
		assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
	}

	#[test]
	fn smoke_frame_round_trips_over_tcp_loopback() {
		use crate::render::Renderer;

		let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
		let addr = listener.local_addr().expect("local addr");

		let accept = thread::spawn(move || {
			let (stream, _) = listener.accept().expect("accept");
			stream
		});

		let client = TcpStream::connect(addr).expect("connect");
		let mut server = accept.join().expect("accept join");

		let mut r = Renderer::new(4, 2);
		r.put_str(
			0,
			0,
			"hi",
			crate::render::Color::Default,
			crate::render::Color::Default,
			crate::render::Attrs::NONE,
		);
		let mut surface = WsSurface::new(client, 4, 2);
		let strat = r.present(&mut surface).expect("present");
		assert_eq!(strat, crate::render::Strategy::Full);
		drop(surface.into_inner());

		let mut raw = Vec::new();
		server.read_to_end(&mut raw).expect("read frame");

		assert!(raw.len() >= 2, "need at least header bytes");
		assert_eq!(raw[0], 0x82, "FIN=1, opcode=binary");
		let (payload_len, header_len) = match raw[1] {
			n if n < 126 => (n as usize, 2),
			126 => (u16::from_be_bytes([raw[2], raw[3]]) as usize, 4),
			127 => (
				u64::from_be_bytes([
					raw[2], raw[3], raw[4], raw[5], raw[6], raw[7], raw[8], raw[9],
				]) as usize,
				10,
			),
			_ => unreachable!(),
		};
		assert_eq!(raw.len() - header_len, payload_len);
		let payload = &raw[header_len..];
		let txt = String::from_utf8_lossy(payload);
		assert!(txt.contains("hi"), "payload missing glyphs: {txt:?}");
		assert!(txt.contains("\x1b[H"), "payload missing cursor-home");
	}
}

mod snapshot {
	use crate::render::cell::{Attrs, Cell, Color};
	use crate::render::diff::Strategy;
	use crate::render::frame::Frame;
	use crate::render::snapshot::*;
	use crate::render::surface::{BufferSurface, Capabilities};
	use crate::render::Renderer;

	struct Lcg(u64);

	impl Lcg {
		fn new(seed: u64) -> Self {
			Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1))
		}
		fn next_u32(&mut self) -> u32 {
			self.0 = self
				.0
				.wrapping_mul(6364136223846793005)
				.wrapping_add(1442695040888963407);
			(self.0 >> 32) as u32
		}
		fn random_range(&mut self, n: u32) -> u32 {
			if n == 0 {
				0
			} else {
				self.next_u32() % n
			}
		}
	}

	#[test]
	fn snapshot_round_trip_restores_cells() {
		let mut r = Renderer::new(8, 3);
		let mut s = BufferSurface::new(8, 3, Capabilities::MINIMAL);
		r.present(&mut s).unwrap();
		r.put_str(0, 0, "hello", Color::Default, Color::Default, Attrs::NONE);
		r.present(&mut s).unwrap();

		let snap = r.snapshot();
		r.frame().set(0, 0, Cell::new('Z'));
		r.present(&mut s).unwrap();

		r.restore(&snap);
		let after = r.snapshot();
		assert_eq!(after.current, snap.current);
		assert_eq!(after.next, snap.next);
		assert_eq!(after.size, snap.size);
		assert_eq!(after.frame_counter, snap.frame_counter);
	}

	#[test]
	fn snapshot_captures_arena_clusters() {
		let mut r = Renderer::new(4, 1);
		r.put_str(0, 0, "é", Color::Default, Color::Default, Attrs::NONE);
		let snap = r.snapshot();
		assert!(snap.arena_clusters.iter().any(|c| c == "é"));
	}

	#[test]
	fn snapshot_properties_validate() {
		let r = Renderer::new(6, 2);
		let snap = r.snapshot();
		assert_eq!(snap.size, (6, 2));
		assert_eq!(snap.current.len(), 12);
		assert_eq!(snap.next.len(), 12);
		assert!(snap.force_full_next);
		assert_eq!(snap.frame_counter, 0);
	}

	#[test]
	fn restore_clears_forced_full_state() {
		let mut r = Renderer::new(4, 2);
		let mut s = BufferSurface::new(4, 2, Capabilities::MINIMAL);
		r.present(&mut s).unwrap();
		let snap = r.snapshot();
		assert!(!snap.force_full_next);
		r.resize(10, 10);
		r.restore(&snap);
		assert_eq!(r.size(), (4, 2));
	}

	#[test]
	fn invariant_flush_converges_when_edits_reapplied() {
		let mut r = Renderer::new(6, 3);
		let mut s = BufferSurface::new(6, 3, Capabilities::MINIMAL);
		r.put_str(0, 0, "ab", Color::Default, Color::Default, Attrs::NONE);
		r.present(&mut s).unwrap();
		r.put_str(0, 0, "ab", Color::Default, Color::Default, Attrs::NONE);
		s.clear();
		let strat = r.present(&mut s).unwrap();
		assert_eq!(strat, Strategy::Noop);
		assert!(s.bytes().is_empty());
	}

	#[test]
	fn vt_replay_reconstructs_ascii_frame() {
		let mut r = Renderer::new(10, 2);
		let mut s = BufferSurface::new(10, 2, Capabilities::MINIMAL);
		r.put_str(
			0,
			0,
			"hi there",
			Color::Default,
			Color::Default,
			Attrs::NONE,
		);
		r.present(&mut s).unwrap();

		let mut vt = VtReplay::new(10, 2);
		vt.feed(s.bytes()).unwrap();
		assert!(vt.matches_frame(r.current_frame_ref(), r.arena()));
	}

	#[test]
	fn vt_replay_reconstructs_styled_frame() {
		let mut r = Renderer::new(6, 1);
		let mut s = BufferSurface::new(6, 1, Capabilities::MINIMAL);
		r.put_str(
			0,
			0,
			"red",
			Color::Rgb(255, 0, 0),
			Color::Default,
			Attrs::BOLD,
		);
		r.present(&mut s).unwrap();

		let mut vt = VtReplay::new(6, 1);
		vt.feed(s.bytes()).unwrap();
		assert!(vt.matches_frame(r.current_frame_ref(), r.arena()));
	}

	#[test]
	fn vt_replay_reconstructs_wide_glyph_frame() {
		let mut r = Renderer::new(6, 1);
		let mut s = BufferSurface::new(6, 1, Capabilities::MINIMAL);
		r.put_str(0, 0, "a漢b", Color::Default, Color::Default, Attrs::NONE);
		r.present(&mut s).unwrap();

		let mut vt = VtReplay::new(6, 1);
		vt.feed(s.bytes()).unwrap();
		assert!(vt.matches_frame(r.current_frame_ref(), r.arena()));
	}

	#[test]
	fn vt_replay_survives_sync_update_wrap() {
		let mut r = Renderer::new(4, 1);
		let mut s = BufferSurface::new(4, 1, Capabilities::MODERN);
		r.put_str(0, 0, "xyz", Color::Default, Color::Default, Attrs::NONE);
		r.present(&mut s).unwrap();

		let mut vt = VtReplay::new(4, 1);
		vt.feed(s.bytes()).unwrap();
		assert!(vt.matches_frame(r.current_frame_ref(), r.arena()));
	}

	#[test]
	fn invariant_no_invalid_csi_in_emission() {
		let mut r = Renderer::new(12, 4);
		let mut s = BufferSurface::new(12, 4, Capabilities::MODERN);
		r.put_str(
			0,
			0,
			"line one",
			Color::Indexed(2),
			Color::Default,
			Attrs::BOLD,
		);
		r.put_str(
			0,
			1,
			"line two",
			Color::Default,
			Color::Default,
			Attrs::UNDERLINE,
		);
		r.present(&mut s).unwrap();
		let mut vt = VtReplay::new(12, 4);
		let res = vt.feed(s.bytes());
		assert!(res.is_ok(), "unexpected replay error: {:?}", res);
	}

	#[test]
	fn replay_flags_unknown_csi() {
		let mut vt = VtReplay::new(4, 1);
		let err = vt.feed(b"\x1b[9999Z").unwrap_err();
		assert_eq!(err, ReplayError::UnknownCsi('Z'));
	}

	#[test]
	fn invariant_bytes_grow_with_changed_cells() {
		fn emit_with_n_changes(n: u16) -> usize {
			let mut r = Renderer::new(20, 2);
			let mut s = BufferSurface::new(20, 2, Capabilities::MINIMAL);
			r.present(&mut s).unwrap();
			s.clear();
			for x in 0..n {
				r.frame().set(x, 0, Cell::new('#'));
			}
			r.present(&mut s).unwrap();
			s.bytes().len()
		}
		let a = emit_with_n_changes(0);
		let b = emit_with_n_changes(1);
		let c = emit_with_n_changes(5);
		let d = emit_with_n_changes(20);
		assert!(a <= b, "0 <= 1 changes: {} <= {}", a, b);
		assert!(b <= c, "1 <= 5 changes: {} <= {}", b, c);
		assert!(c <= d, "5 <= 20 changes: {} <= {}", c, d);
	}

	#[test]
	fn property_replay_matches_next_under_random_edits() {
		for seed in 0..32u64 {
			let mut rng = Lcg::new(seed);
			let w = 4 + (rng.random_range(6) as u16);
			let h = 2 + (rng.random_range(3) as u16);
			let mut r = Renderer::new(w, h);
			let mut s = BufferSurface::new(w, h, Capabilities::MINIMAL);
			r.present(&mut s).unwrap();
			s.clear();

			let edits = rng.random_range(10);
			for _ in 0..edits {
				let x = rng.random_range(w as u32) as u16;
				let y = rng.random_range(h as u32) as u16;
				let ch = (b'A' + (rng.random_range(26) as u8)) as char;
				r.frame().set(x, y, Cell::new(ch));
			}
			r.present(&mut s).unwrap();

			let mut vt = VtReplay::new(w, h);
			vt.feed(s.bytes())
				.unwrap_or_else(|e| panic!("seed {seed}: replay failed: {e}"));

			let snap = r.snapshot();
			let mut expected = Frame::new(w, h);
			expected.cells.clone_from(&snap.current);
			assert!(
				vt.matches_frame(&expected, r.arena()),
				"seed {seed}: VT replay disagrees with next"
			);
		}
	}

	#[test]
	fn property_bytes_nondecreasing_with_more_edits() {
		for seed in 0..16u64 {
			let mut rng = Lcg::new(seed);
			let w = 10u16;
			let h = 3u16;
			let total = (w * h) as u32;

			let mut last = 0usize;
			for step in 0..5 {
				let mut r = Renderer::new(w, h);
				let mut s = BufferSurface::new(w, h, Capabilities::MINIMAL);
				r.present(&mut s).unwrap();
				s.clear();
				let n = ((step + 1) * 4).min(total);
				let mut used = vec![false; total as usize];
				let mut placed = 0u32;
				while placed < n {
					let i = rng.random_range(total) as usize;
					if !used[i] {
						used[i] = true;
						let x = (i as u16) % w;
						let y = (i as u16) / w;
						r.frame().set(x, y, Cell::new('*'));
						placed += 1;
					}
				}
				r.present(&mut s).unwrap();
				let now = s.bytes().len();
				assert!(
					now >= last,
					"seed {seed} step {step}: bytes regressed {} -> {}",
					last,
					now
				);
				last = now;
			}
		}
	}

	#[test]
	fn replay_empty_bytes_is_noop() {
		let mut vt = VtReplay::new(4, 2);
		vt.feed(b"").unwrap();
		assert!(vt.cells.iter().all(|c| *c == Cell::default()));
	}

	#[test]
	fn replay_rejects_truncated_csi() {
		let mut vt = VtReplay::new(4, 1);
		assert_eq!(vt.feed(b"\x1b[12;").unwrap_err(), ReplayError::TruncatedCsi);
	}

	#[test]
	fn snapshot_after_resize_reflects_new_size() {
		let mut r = Renderer::new(4, 2);
		r.resize(12, 6);
		let snap = r.snapshot();
		assert_eq!(snap.size, (12, 6));
		assert_eq!(snap.current.len(), 72);
	}
}

mod renderer {
	use crate::render::cell::{Attrs, Cell, Color};
	use crate::render::diff::Strategy;
	use crate::render::frame::Frame;
	use crate::render::grapheme::GraphemeArena;
	use crate::render::pass::{DebugOverlay, FramePass, PassCtx};
	use crate::render::surface::{BufferSurface, Capabilities};
	use crate::render::Renderer;

	fn fill(r: &mut Renderer, ch: char) {
		let f = r.frame();
		f.fill(Cell::new(ch));
	}

	#[test]
	fn noop_when_identical() {
		let mut r = Renderer::new(4, 2);
		let mut out = Vec::new();
		r.flush(&mut out).unwrap();
		out.clear();
		let s = r.flush(&mut out).unwrap();
		assert_eq!(s, Strategy::Noop);
		assert!(out.is_empty());
	}

	#[test]
	fn single_cell_change_picks_cells() {
		let mut r = Renderer::new(10, 5);
		let mut out = Vec::new();
		r.flush(&mut out).unwrap();
		r.frame().set(3, 2, Cell::new('X'));
		out.clear();
		let s = r.flush(&mut out).unwrap();
		assert_eq!(s, Strategy::Cells);
		assert!(!out.is_empty());
	}

	#[test]
	fn full_clear_picks_full() {
		let mut r = Renderer::new(8, 4);
		let mut out = Vec::new();
		r.flush(&mut out).unwrap();
		fill(&mut r, '#');
		out.clear();
		let s = r.flush(&mut out).unwrap();
		assert_eq!(s, Strategy::Full);
		let txt = String::from_utf8_lossy(&out);
		assert!(txt.contains("\x1b[H"));
	}

	#[test]
	fn resize_forces_full() {
		let mut r = Renderer::new(4, 2);
		let mut out = Vec::new();
		r.flush(&mut out).unwrap();
		r.resize(6, 3);
		out.clear();
		let s = r.flush(&mut out).unwrap();
		assert_eq!(s, Strategy::Full);
	}

	#[test]
	fn sgr_dedup_contiguous() {
		let mut r = Renderer::new(20, 1);
		let mut out = Vec::new();
		r.flush(&mut out).unwrap();
		let red = Color::Rgb(255, 0, 0);
		let f = r.frame();
		for x in 0..10u16 {
			f.set(x, 0, Cell::new('r').style(red, Color::Default, Attrs::NONE));
		}
		out.clear();
		r.flush(&mut out).unwrap();
		let txt = String::from_utf8_lossy(&out);
		let count = txt.matches("38;2;255;0;0").count();
		assert_eq!(count, 1, "fg SGR should emit once for contiguous run");
	}

	#[test]
	fn wide_glyph_occupies_two_cells() {
		let mut r = Renderer::new(10, 1);
		let mut out = Vec::new();
		r.flush(&mut out).unwrap();
		r.put_str(0, 0, "漢", Color::Default, Color::Default, Attrs::NONE);
		let lead = r.frame().get(0, 0).copied().unwrap();
		let trail = r.frame().get(1, 0).copied().unwrap();
		assert_eq!(lead.width, 2);
		assert!(trail.is_continuation());
		out.clear();
		r.flush(&mut out).unwrap();
		let txt = String::from_utf8_lossy(&out);
		assert_eq!(txt.matches("漢").count(), 1);
	}

	#[test]
	fn combining_mark_is_single_cluster() {
		let mut r = Renderer::new(10, 1);
		let mut out = Vec::new();
		r.flush(&mut out).unwrap();
		r.put_str(
			0,
			0,
			"e\u{0301}",
			Color::Default,
			Color::Default,
			Attrs::NONE,
		);
		let lead = r.frame().get(0, 0).copied().unwrap();
		let next_cell = r.frame().get(1, 0).copied().unwrap();
		assert_eq!(lead.width, 1);
		assert_eq!(next_cell, Cell::default());
		out.clear();
		r.flush(&mut out).unwrap();
		let txt = String::from_utf8_lossy(&out);
		assert!(txt.contains("e\u{0301}"));
	}

	#[test]
	fn zwj_emoji_is_single_cluster() {
		let mut r = Renderer::new(10, 1);
		let mut out = Vec::new();
		r.flush(&mut out).unwrap();
		let fam = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
		r.put_str(0, 0, fam, Color::Default, Color::Default, Attrs::NONE);
		let lead = r.frame().get(0, 0).copied().unwrap();
		assert!(lead.width >= 1);
		assert_eq!(r.arena().get(lead.cluster), fam);
	}

	#[test]
	fn emit_skips_continuation_cells() {
		let mut r = Renderer::new(10, 1);
		let mut out = Vec::new();
		r.flush(&mut out).unwrap();
		r.put_str(0, 0, "漢字", Color::Default, Color::Default, Attrs::NONE);
		out.clear();
		r.flush(&mut out).unwrap();
		let txt = String::from_utf8_lossy(&out);
		assert_eq!(txt.matches("漢").count(), 1);
		assert_eq!(txt.matches("字").count(), 1);
	}

	#[test]
	fn flush_wraps_payload_in_sync_update_when_supported() {
		let mut r = Renderer::new(4, 2);
		r.set_supports_sync_update(true);
		let mut out = Vec::new();
		r.flush(&mut out).unwrap();
		let txt = String::from_utf8_lossy(&out);
		assert!(txt.starts_with("\x1b[?2026h"));
		assert!(txt.ends_with("\x1b[?2026l"));
	}

	#[test]
	fn flush_omits_sync_wrap_when_unsupported() {
		let mut r = Renderer::new(4, 2);
		r.set_supports_sync_update(false);
		let mut out = Vec::new();
		r.flush(&mut out).unwrap();
		let txt = String::from_utf8_lossy(&out);
		assert!(!txt.contains("\x1b[?2026"));
	}

	#[test]
	fn flush_emits_single_sync_pair_per_frame() {
		let mut r = Renderer::new(10, 3);
		r.set_supports_sync_update(true);
		let mut out = Vec::new();
		r.flush(&mut out).unwrap();
		out.clear();
		r.frame().set(1, 1, Cell::new('X'));
		r.frame().set(5, 2, Cell::new('Y'));
		r.flush(&mut out).unwrap();
		let txt = String::from_utf8_lossy(&out);
		assert_eq!(txt.matches("\x1b[?2026h").count(), 1);
		assert_eq!(txt.matches("\x1b[?2026l").count(), 1);
	}

	#[test]
	fn flush_skips_sync_wrap_on_noop() {
		let mut r = Renderer::new(4, 2);
		r.set_supports_sync_update(true);
		let mut out = Vec::new();
		r.flush(&mut out).unwrap();
		out.clear();
		let s = r.flush(&mut out).unwrap();
		assert_eq!(s, Strategy::Noop);
		assert!(out.is_empty());
	}

	#[test]
	fn empty_pipeline_allocates_nothing() {
		let r = Renderer::new(10, 3);
		assert_eq!(r.pass_count(), 0);
		assert!(r.passes.is_none(), "no allocation until add_pass");
	}

	#[test]
	fn add_pass_installs_and_runs() {
		use std::cell::Cell as StdCell;
		use std::rc::Rc;

		struct Counter(Rc<StdCell<u32>>);
		impl FramePass for Counter {
			fn apply(&mut self, _f: &mut Frame, _a: &mut GraphemeArena, _c: &PassCtx) {
				self.0.set(self.0.get() + 1);
			}
		}

		let hits = Rc::new(StdCell::new(0));
		let mut r = Renderer::new(8, 2);
		r.add_pass(Counter(hits.clone()));
		assert_eq!(r.pass_count(), 1);

		let mut out = Vec::new();
		r.flush(&mut out).unwrap();
		r.flush(&mut out).unwrap();
		assert_eq!(hits.get(), 2, "pass runs once per flush");
	}

	#[test]
	fn debug_overlay_pass_mutates_frame() {
		let mut r = Renderer::new(40, 3);
		r.add_pass(DebugOverlay::new());
		let mut out = Vec::new();
		r.flush(&mut out).unwrap();
		let row0_changed = (0..r.size().0).any(|x| {
			r.next.get(x, 0).copied().unwrap_or(Cell::default()) != Cell::default()
				|| r.current.get(x, 0).copied().unwrap_or(Cell::default()) != Cell::default()
		});
		assert!(row0_changed);
	}

	#[test]
	fn present_to_buffer_surface_writes_bytes() {
		let mut r = Renderer::new(4, 2);
		let mut s = BufferSurface::new(4, 2, Capabilities::MINIMAL);
		let strat = r.present(&mut s).unwrap();
		assert_eq!(strat, Strategy::Full);
		assert!(!s.bytes().is_empty());
	}

	#[test]
	fn present_honours_surface_sync_capability() {
		let mut r = Renderer::new(4, 2);
		r.set_supports_sync_update(false);
		let mut s = BufferSurface::new(4, 2, Capabilities::MODERN);
		r.present(&mut s).unwrap();
		let txt = String::from_utf8_lossy(s.bytes());
		assert!(txt.contains("\x1b[?2026h"));
		assert!(txt.contains("\x1b[?2026l"));
	}

	#[test]
	fn present_skips_sync_wrap_on_minimal_surface() {
		let mut r = Renderer::new(4, 2);
		r.set_supports_sync_update(true);
		let mut s = BufferSurface::new(4, 2, Capabilities::MINIMAL);
		r.present(&mut s).unwrap();
		let txt = String::from_utf8_lossy(s.bytes());
		assert!(!txt.contains("\x1b[?2026"));
	}

	#[test]
	fn present_resizes_to_match_surface() {
		let mut r = Renderer::new(4, 2);
		let mut s = BufferSurface::new(10, 5, Capabilities::MINIMAL);
		r.present(&mut s).unwrap();
		assert_eq!(r.size(), (10, 5));
	}

	#[test]
	fn present_noop_writes_nothing_to_surface() {
		let mut r = Renderer::new(4, 2);
		let mut s = BufferSurface::new(4, 2, Capabilities::MINIMAL);
		r.present(&mut s).unwrap();
		s.clear();
		let strat = r.present(&mut s).unwrap();
		assert_eq!(strat, Strategy::Noop);
		assert!(s.bytes().is_empty());
	}

	#[test]
	fn present_runs_frame_passes() {
		let mut r = Renderer::new(40, 3);
		r.add_pass(DebugOverlay::new());
		let mut s = BufferSurface::new(40, 3, Capabilities::MINIMAL);
		r.present(&mut s).unwrap();
		let txt = String::from_utf8_lossy(s.bytes());
		assert!(txt.contains("fps"));
	}

	#[test]
	fn wide_glyph_at_row_edge_does_not_half_write() {
		let mut r = Renderer::new(3, 1);
		let mut out = Vec::new();
		r.flush(&mut out).unwrap();
		r.put_str(0, 0, "ab漢", Color::Default, Color::Default, Attrs::NONE);
		let edge = r.frame().get(2, 0).copied().unwrap();
		assert!(!edge.is_continuation());
		assert_eq!(edge.width, 1);
	}
}
