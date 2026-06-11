//! PTY session — one OS PTY pair + spawned process + vt100 screen model.
//!
//! Cross-platform via `portable_pty`: uses ConPTY on Windows and openpty
//! on Unix/WSL with no `#[cfg]` walls in this file.

use std::io::Write;
use std::sync::mpsc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};

pub struct PtySession {
    /// Short random id — 8 lowercase hex chars generated at spawn time.
    pub id: String,
    /// Human label shown in the tab strip (e.g. `"main"`, `"sub-1"`).
    pub label: String,
    /// The command that was spawned (e.g. `"claude"`).
    pub cmd: String,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    /// In-memory VT100 screen model updated by `drain()`.
    pub parser: vt100::Parser,
    rx: mpsc::Receiver<Vec<u8>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    /// Set to `true` once the child process has exited.
    pub exited: bool,
}

impl PtySession {
    pub fn spawn(id: String, label: String, cmd: String, cols: u16, rows: u16) -> anyhow::Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;

        let slave  = pair.slave;
        let master = pair.master;

        let builder = build_command(&cmd)?;
        let child = slave.spawn_command(builder)?;
        drop(slave);

        let writer = master.take_writer()?;
        let mut reader = master.try_clone_reader()?;

        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match std::io::Read::read(&mut reader, &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        let parser = vt100::Parser::new(rows, cols, 0);

        Ok(Self { id, label, cmd, master, writer, parser, rx, child, exited: false })
    }

    pub fn drain(&mut self) {
        while let Ok(bytes) = self.rx.try_recv() {
            self.parser.process(&bytes);
        }
    }

    pub fn write_input(&mut self, bytes: &[u8]) {
        if let Err(e) = self.writer.write_all(bytes) {
            tracing::debug!(target: "kern.mux.pty", error = %e, "write_input: write failed (pane may be dead)");
            return;
        }
        if let Err(e) = self.writer.flush() {
            tracing::debug!(target: "kern.mux.pty", error = %e, "write_input: flush failed");
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        let _ = self.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        // Reset the parser — does not reflow existing content;
        // screen will appear blank until the child process repaints.
        self.parser = vt100::Parser::new(rows, cols, 0);
    }

    pub fn poll_exited(&mut self) -> bool {
        if self.exited { return true; }
        if matches!(self.child.try_wait(), Ok(Some(_))) {
            self.exited = true;
        }
        self.exited
    }

    pub fn screen_text(&self) -> String {
        screen_text_from(self.parser.screen())
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        self.kill();
    }
}

pub(crate) fn screen_text_from(screen: &vt100::Screen) -> String {
    let (rows, cols) = screen.size();
    let mut lines: Vec<String> = Vec::with_capacity(rows as usize);
    for row in 0..rows {
        let mut line = String::with_capacity(cols as usize);
        for col in 0..cols {
            match screen.cell(row, col) {
                Some(cell) => {
                    let c = cell.contents();
                    if c.is_empty() { line.push(' '); } else { line.push_str(&c); }
                }
                None => line.push(' '),
            }
        }
        lines.push(line.trim_end().to_string());
    }
    while lines.last().map(|l: &String| l.is_empty()).unwrap_or(false) {
        lines.pop();
    }
    lines.join("\n")
}

pub fn key_event_to_bytes(ev: &KeyEvent) -> Option<Vec<u8>> {
    let ctrl = ev.modifiers.contains(KeyModifiers::CONTROL);
    match ev.code {
        KeyCode::Char(c) => {
            let alt = ev.modifiers.contains(KeyModifiers::ALT);
            if ctrl {
                let lower = c.to_ascii_lowercase();
                if lower.is_ascii_alphabetic() {
                    return Some(vec![lower as u8 - b'a' + 1]);
                }
                None
            } else if alt {
                // Terminal convention: Alt+char = ESC prefix + char bytes.
                let mut buf = [0u8; 4];
                let s = c.encode_utf8(&mut buf);
                let mut out = vec![0x1b];
                out.extend_from_slice(s.as_bytes());
                Some(out)
            } else {
                let mut buf = [0u8; 4];
                Some(c.encode_utf8(&mut buf).as_bytes().to_vec())
            }
        }
        KeyCode::Enter     => Some(b"\r".to_vec()),
        KeyCode::Backspace => Some(b"\x7f".to_vec()),
        KeyCode::Esc       => Some(b"\x1b".to_vec()),
        KeyCode::Tab       => Some(b"\t".to_vec()),
        KeyCode::Up        => Some(b"\x1b[A".to_vec()),
        KeyCode::Down      => Some(b"\x1b[B".to_vec()),
        KeyCode::Right     => Some(b"\x1b[C".to_vec()),
        KeyCode::Left      => Some(b"\x1b[D".to_vec()),
        KeyCode::Home      => Some(b"\x1b[H".to_vec()),
        KeyCode::End       => Some(b"\x1b[F".to_vec()),
        KeyCode::Delete    => Some(b"\x1b[3~".to_vec()),
        _ => None,
    }
}

/// Build the `CommandBuilder` for a pane child process.
///
/// portable-pty defaults an **unset** cwd to `%USERPROFILE%`/`$HOME` — see
/// `CommandBuilder::current_directory`, which returns `cwd.or(home)`. Unlike
/// `std::process::Command`, it does NOT inherit the parent's working directory.
/// Left unset, `claude` launches in the home dir and loses every project-scoped
/// piece of config — `CLAUDE.md`, `.claude/settings.json` (permissions/hooks),
/// `.mcp.json` (the kern MCP server), and the project's session/memory bucket —
/// so it presents as a different user with nothing configured. Pin cwd to the
/// mux process's own working directory to restore `std::process` semantics.
///
/// (The full environment — USERPROFILE, PATH, etc. — is already inherited:
/// `CommandBuilder::new` seeds from `std::env::vars_os()`.)
fn build_command(cmd: &str) -> anyhow::Result<CommandBuilder> {
    let mut builder = CommandBuilder::new(cmd);
    builder.env("TERM", "xterm-256color");
    builder.cwd(std::env::current_dir()?);
    Ok(builder)
}

pub fn new_session_id() -> String {
    use rand::RngExt;
    let mut rng = rand::rng();
    format!("{:08x}", rng.random::<u32>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_text_from_empty_parser() {
        let parser = vt100::Parser::new(5, 10, 0);
        let text = screen_text_from(parser.screen());
        assert!(text.trim().is_empty(), "fresh parser: got {text:?}");
    }

    #[test]
    fn screen_text_trims_trailing_whitespace_per_row() {
        let mut parser = vt100::Parser::new(3, 10, 0);
        parser.process(b"hi");
        let text = screen_text_from(parser.screen());
        let first = text.lines().next().unwrap_or("");
        assert_eq!(first, "hi", "trailing spaces stripped: got {first:?}");
    }

    #[test]
    fn key_event_to_bytes_char() {
        let ev = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(key_event_to_bytes(&ev), Some(b"a".to_vec()));
    }

    #[test]
    fn key_event_to_bytes_ctrl_c() {
        let ev = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(key_event_to_bytes(&ev), Some(vec![0x03]));
    }

    #[test]
    fn key_event_to_bytes_enter() {
        let ev = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(key_event_to_bytes(&ev), Some(b"\r".to_vec()));
    }

    #[test]
    fn key_event_to_bytes_up_arrow() {
        let ev = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(key_event_to_bytes(&ev), Some(b"\x1b[A".to_vec()));
    }

    #[test]
    fn build_command_pins_cwd_to_current_dir() {
        // Regression: without an explicit cwd, portable-pty spawns children in
        // the home dir, so `claude` loses all project-scoped config.
        let builder = build_command("claude").expect("build");
        let cwd = builder
            .get_cwd()
            .expect("cwd must be pinned so the child inherits the project dir");
        let expected = std::env::current_dir().unwrap();
        assert_eq!(
            std::path::Path::new(cwd),
            expected.as_path(),
            "child cwd pinned to mux process dir, not $HOME"
        );
    }

    #[test]
    fn build_command_sets_term() {
        let builder = build_command("claude").expect("build");
        let term = builder.get_env("TERM").expect("TERM set");
        assert_eq!(term, std::ffi::OsStr::new("xterm-256color"));
    }

    #[test]
    fn new_session_id_is_8_hex_chars() {
        let id = new_session_id();
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()), "not hex: {id}");
    }

    #[test]
    fn key_event_to_bytes_alt_b() {
        let ev = KeyEvent::new(KeyCode::Char('b'), KeyModifiers::ALT);
        assert_eq!(key_event_to_bytes(&ev), Some(vec![0x1b, b'b']));
    }
}
