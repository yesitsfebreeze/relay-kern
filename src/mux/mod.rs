pub mod mcp;
pub mod pty;
pub mod registry;
pub mod tui;

pub use pty::{new_session_id, PtySession};
pub use registry::{PaneRegistry, SharedRegistry};

/// Launch the mux PTY-multiplexer TUI. Called when `kern` is run with no
/// subcommand and `--daemon` is not set. Placeholder until Task 6.
pub async fn run_mux(_cfg: &crate::config::Config) {
    eprintln!("kern: mux TUI not yet implemented — use kern --daemon for the knowledge substrate");
}
