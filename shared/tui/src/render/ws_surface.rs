use std::io::{self, Write};

use super::surface::{Capabilities, Surface};

impl Capabilities {
	pub const XTERM_JS: Self = Capabilities {
		truecolor: true,
		sync_update: false,
	};
}

pub(crate) const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;

const OPCODE_BINARY: u8 = 0x2;

pub struct WsSurface<W: Write> {
	inner: W,
	size: (u16, u16),
	caps: Capabilities,
}

impl<W: Write> WsSurface<W> {
	pub fn new(inner: W, cols: u16, rows: u16) -> Self {
		WsSurface {
			inner,
			size: (cols, rows),
			caps: Capabilities::XTERM_JS,
		}
	}

	pub fn with_capabilities(inner: W, cols: u16, rows: u16, caps: Capabilities) -> Self {
		WsSurface {
			inner,
			size: (cols, rows),
			caps,
		}
	}

	pub fn resize(&mut self, cols: u16, rows: u16) {
		self.size = (cols, rows);
	}

	pub fn into_inner(self) -> W {
		self.inner
	}
}

impl<W: Write> Surface for WsSurface<W> {
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
		if bytes.len() > MAX_FRAME_BYTES {
			return Err(io::Error::new(
				io::ErrorKind::InvalidInput,
				"frame exceeds WsSurface maximum payload size",
			));
		}
		let mut header = [0u8; 10];
		header[0] = 0x80 | OPCODE_BINARY;
		let header_len = encode_payload_length(bytes.len(), &mut header[1..]);
		self.inner.write_all(&header[..1 + header_len])?;
		self.inner.write_all(bytes)?;
		self.inner.flush()
	}
}

fn encode_payload_length(len: usize, out: &mut [u8]) -> usize {
	if len < 126 {
		out[0] = len as u8;
		1
	} else if len <= u16::MAX as usize {
		out[0] = 126;
		out[1..3].copy_from_slice(&(len as u16).to_be_bytes());
		3
	} else {
		out[0] = 127;
		out[1..9].copy_from_slice(&(len as u64).to_be_bytes());
		9
	}
}

