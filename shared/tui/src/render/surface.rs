use std::io::{self, Write};

use super::sync::detect_sync_update_support;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
	pub truecolor: bool,
	pub sync_update: bool,
}

impl Capabilities {
	pub const MINIMAL: Self = Capabilities {
		truecolor: false,
		sync_update: false,
	};

	pub const MODERN: Self = Capabilities {
		truecolor: true,
		sync_update: true,
	};
}

pub trait Surface {
	fn size(&self) -> (u16, u16);

	fn capabilities(&self) -> Capabilities;

	fn write_frame(&mut self, bytes: &[u8]) -> io::Result<()>;
}

pub struct StdoutSurface {
	stdout: io::Stdout,
	size: (u16, u16),
	caps: Capabilities,
}

impl StdoutSurface {
	pub fn new(cols: u16, rows: u16) -> Self {
		StdoutSurface {
			stdout: io::stdout(),
			size: (cols, rows),
			caps: Capabilities {
				truecolor: detect_truecolor(),
				sync_update: detect_sync_update_support(),
			},
		}
	}

	pub fn refresh_size(&mut self, cols: u16, rows: u16) {
		self.size = (cols, rows);
	}

	pub fn set_capabilities(&mut self, caps: Capabilities) {
		self.caps = caps;
	}
}

impl Surface for StdoutSurface {
	fn size(&self) -> (u16, u16) {
		self.size
	}

	fn capabilities(&self) -> Capabilities {
		self.caps
	}

	fn write_frame(&mut self, bytes: &[u8]) -> io::Result<()> {
		if bytes.is_empty() {
			return Ok(());
		}
		self.stdout.write_all(bytes)?;
		self.stdout.flush()
	}
}

pub struct BufferSurface {
	buf: Vec<u8>,
	size: (u16, u16),
	caps: Capabilities,
}

impl BufferSurface {
	pub fn new(cols: u16, rows: u16, caps: Capabilities) -> Self {
		BufferSurface {
			buf: Vec::new(),
			size: (cols, rows),
			caps,
		}
	}

	pub fn bytes(&self) -> &[u8] {
		&self.buf
	}

	pub fn take(&mut self) -> Vec<u8> {
		std::mem::take(&mut self.buf)
	}

	pub fn clear(&mut self) {
		self.buf.clear();
	}

	pub fn resize(&mut self, cols: u16, rows: u16) {
		self.size = (cols, rows);
	}
}

impl Surface for BufferSurface {
	fn size(&self) -> (u16, u16) {
		self.size
	}

	fn capabilities(&self) -> Capabilities {
		self.caps
	}

	fn write_frame(&mut self, bytes: &[u8]) -> io::Result<()> {
		self.buf.extend_from_slice(bytes);
		Ok(())
	}
}

fn detect_truecolor() -> bool {
	if let Ok(ct) = std::env::var("COLORTERM") {
		if ct.eq_ignore_ascii_case("truecolor") || ct.eq_ignore_ascii_case("24bit") {
			return true;
		}
	}
	if let Ok(term) = std::env::var("TERM") {
		if term.contains("direct") || term == "xterm-kitty" || term == "foot" {
			return true;
		}
	}
	if std::env::var_os("WT_SESSION").is_some() {
		return true;
	}
	false
}
