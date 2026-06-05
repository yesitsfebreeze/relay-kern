use std::io::{Read, Write};

use serde_json::Value;

use crate::server::{dispatch, McpServer};
use crate::transport::Transport;

pub struct InProcTransport {
	server: Box<dyn McpServer>,
	req_buf: Vec<u8>,
	resp_buf: std::collections::VecDeque<u8>,
	killed: bool,
}

impl InProcTransport {
	pub fn new(server: Box<dyn McpServer>) -> Self {
		Self {
			server,
			req_buf: Vec::new(),
			resp_buf: std::collections::VecDeque::new(),
			killed: false,
		}
	}

	fn try_handle_one(&mut self) -> std::io::Result<()> {
		let pos = match self.req_buf.iter().position(|&b| b == b'\n') {
			Some(p) => p,
			None => return Ok(()),
		};
		let line: Vec<u8> = self.req_buf.drain(..=pos).collect();
		let line_str = match std::str::from_utf8(&line[..pos]) {
			Ok(s) => s.trim_end_matches('\r'),
			Err(_) => return Ok(()),
		};
		let req: Value = match serde_json::from_str(line_str) {
			Ok(v) => v,
			Err(_) => return Ok(()),
		};
		if let Some(frame) = dispatch(&*self.server, &req) {
			let mut s = serde_json::to_string(&frame).unwrap_or_default();
			s.push('\n');
			self.resp_buf.extend(s.as_bytes());
		}
		Ok(())
	}
}

impl Read for InProcTransport {
	fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
		if self.killed {
			return Ok(0);
		}
		let mut n = 0;
		while n < buf.len() {
			match self.resp_buf.pop_front() {
				Some(b) => {
					buf[n] = b;
					n += 1;
				}
				None => break,
			}
		}
		Ok(n)
	}
}

impl Write for InProcTransport {
	fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
		if self.killed {
			return Err(std::io::Error::new(
				std::io::ErrorKind::BrokenPipe,
				"inproc transport killed",
			));
		}
		self.req_buf.extend_from_slice(buf);
		while self.req_buf.contains(&b'\n') {
			self.try_handle_one()?;
		}
		Ok(buf.len())
	}

	fn flush(&mut self) -> std::io::Result<()> {
		Ok(())
	}
}

impl Transport for InProcTransport {
	fn reader(&mut self) -> &mut dyn Read {
		self
	}
	fn writer(&mut self) -> &mut dyn Write {
		self
	}
	fn kill(&mut self) -> std::io::Result<()> {
		self.killed = true;
		Ok(())
	}
}
