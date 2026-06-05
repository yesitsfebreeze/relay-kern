use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use crate::error::McpError;

pub trait Transport: Send {
	fn reader(&mut self) -> &mut dyn Read;
	fn writer(&mut self) -> &mut dyn Write;
	fn kill(&mut self) -> std::io::Result<()>;
}

pub struct ChildStdio {
	child: Child,
	reader: BufReader<ChildStdout>,
	writer: ChildStdin,
}

impl ChildStdio {
	pub fn spawn(program: &str, args: &[&str]) -> Result<Self, McpError> {
		let mut child = Command::new(program)
			.args(args)
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.spawn()?;
		let stdout = child
			.stdout
			.take()
			.ok_or_else(|| McpError::Protocol("child stdout missing".into()))?;
		let stdin = child
			.stdin
			.take()
			.ok_or_else(|| McpError::Protocol("child stdin missing".into()))?;
		if let Some(stderr) = child.stderr.take() {
			let source = child_log_source(program);
			std::thread::Builder::new()
				.name(format!("mcp-stderr-{source}"))
				.spawn(move || forward_child_stderr(stderr, &source))
				.ok();
		}
		Ok(Self {
			child,
			reader: BufReader::new(stdout),
			writer: stdin,
		})
	}
}

fn child_log_source(program: &str) -> String {
	let path = std::path::Path::new(program);
	let stem = path
		.file_stem()
		.and_then(|s| s.to_str())
		.unwrap_or(program);
	stem.to_string()
}

fn forward_child_stderr<R: std::io::Read>(stderr: R, source: &str) {
	let reader = BufReader::new(stderr);
	for line in reader.lines().map_while(Result::ok) {
		let trimmed = line.trim_end();
		if trimmed.is_empty() {
			continue;
		}
		let level = classify_child_log_level(trimmed);
		logsink::log(level, source, trimmed);
	}
}

fn classify_child_log_level(line: &str) -> logsink::Level {
	if line.contains("ERROR") || line.contains("error:") {
		logsink::Level::Error
	} else if line.contains("WARN") || line.contains("warning:") {
		logsink::Level::Warn
	} else {
		logsink::Level::Info
	}
}

impl Transport for ChildStdio {
	fn reader(&mut self) -> &mut dyn Read {
		&mut self.reader
	}
	fn writer(&mut self) -> &mut dyn Write {
		&mut self.writer
	}
	fn kill(&mut self) -> std::io::Result<()> {
		match self.child.try_wait()? {
			Some(_) => Ok(()),
			None => {
				let _ = self.child.kill();
				let _ = self.child.wait();
				Ok(())
			}
		}
	}
}
