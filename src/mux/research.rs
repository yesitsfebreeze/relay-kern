//! Kern research panel — live journal log + kern LLM chat.
//!
//! Toggled by `Ctrl+L` in the mux TUI. Provides two regions:
//! - Left: conversation history + input line (chat backed by kern's LLM answer)
//! - Right: live tail of `.relay/journal/today.jsonl`

use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::Duration;

use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
};
use journal::entry::{Entry, Kind};

// ── Journal formatting (ported from relay/src/journal_tail.rs) ────────────────

/// One-line representation of a journal entry.
pub fn format_entry(e: &Entry) -> String {
    let tag = match e.kind {
        Kind::User          => "user",
        Kind::Assistant     => "asst",
        Kind::Final         => "final",
        Kind::TurnStart     => "turn>",
        Kind::TurnEnd       => "turn<",
        Kind::Usage         => "usage",
        Kind::ToolCall      => "tool",
        Kind::RecipeInvoke  => "recipe",
        Kind::PluginCall    => "plug",
        Kind::Error         => "err",
        Kind::Ask           => "ask",
        Kind::Answer        => "ans",
        Kind::Goal          => "goal",
        Kind::GoalSnapshot  => "gsnap",
        Kind::Milestone     => "ms",
        Kind::Edit { .. }   => "edit",
        Kind::Fork { .. }   => "fork",
        Kind::RpcSend       => "rpc>",
        Kind::RpcRecv       => "rpc<",
        Kind::RpcError      => "rpc!",
        Kind::Log           => "log",
        Kind::PlanStep      => "plan",
        Kind::PlanProposal  => "prop",
        Kind::EntityTouched => "touch",
        Kind::ForkOpen { .. }   => "fork>",
        Kind::ForkResume { .. } => "fork~",
        Kind::ForkClose { .. }  => "fork<",
    };
    let payload_summary = match &e.payload {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    };
    if payload_summary.is_empty() {
        format!("{tag} {}", e.key)
    } else {
        format!("{tag} {} {payload_summary}", e.key)
    }
}

/// Overview filter: returns `None` for noisy sub-block events.
pub fn format_entry_overview(e: &Entry) -> Option<String> {
    match e.kind {
        Kind::ToolCall
        | Kind::PluginCall
        | Kind::Edit { .. }
        | Kind::ForkOpen { .. }
        | Kind::ForkResume { .. }
        | Kind::ForkClose { .. }
        | Kind::RpcSend
        | Kind::RpcRecv
        | Kind::RpcError
        | Kind::Log
        | Kind::Usage
        | Kind::EntityTouched
        | Kind::GoalSnapshot => None,
        _ => Some(format_entry(e)),
    }
}

// ── JournalTailer ─────────────────────────────────────────────────────────────

const POLL_INTERVAL: Duration = Duration::from_millis(150);
const CHANNEL_CAP:   usize    = 256;
const RING_CAP:      usize    = 512;

pub struct JournalTailer {
    pub rx:   Receiver<String>,
    pub ring: VecDeque<String>,
}

impl JournalTailer {
    /// Spawn the background tailer thread and return a `JournalTailer`.
    ///
    /// Tails `<cwd>/.relay/journal/today.jsonl`. If the file does not yet
    /// exist the thread sleeps and retries — the journal pane stays blank.
    pub fn new() -> Self {
        let (tx, rx) = mpsc::sync_channel::<String>(CHANNEL_CAP);
        let path: PathBuf = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".relay")
            .join("journal")
            .join("today.jsonl");
        std::thread::Builder::new()
            .name("kern-research-journal-tail".into())
            .spawn(move || tail_loop(path, tx))
            .expect("spawn kern-research-journal-tail");
        Self { rx, ring: VecDeque::new() }
    }

    /// Drain all pending lines from the channel into the ring buffer.
    /// Called once per TUI frame. Never blocks.
    pub fn drain(&mut self) {
        while let Ok(line) = self.rx.try_recv() {
            if self.ring.len() >= RING_CAP {
                self.ring.pop_front();
            }
            self.ring.push_back(line);
        }
    }
}

fn tail_loop(path: PathBuf, tx: SyncSender<String>) {
    let mut pos: u64 = 0;
    loop {
        let Ok(file) = File::open(&path) else {
            std::thread::sleep(POLL_INTERVAL);
            continue;
        };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        // File shrank → day rollover. Reset to start (skip header).
        if len < pos {
            pos = 0;
        }
        if len <= pos {
            drop(file);
            std::thread::sleep(POLL_INTERVAL);
            continue;
        }
        let mut reader = BufReader::new(file);
        if reader.seek(SeekFrom::Start(pos)).is_err() {
            std::thread::sleep(POLL_INTERVAL);
            continue;
        }
        let start_pos = pos;
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(n) => {
                    pos += n as u64;
                    // Skip header line (first line of file).
                    if start_pos == 0 && pos == n as u64 {
                        continue;
                    }
                    let trimmed = line.trim();
                    if trimmed.is_empty() { continue; }
                    if let Ok(entry) = serde_json::from_str::<Entry>(trimmed) {
                        if let Some(formatted) = format_entry_overview(&entry) {
                            // Drop on backpressure — never stall the tailer.
                            let _ = tx.try_send(formatted);
                        }
                    }
                }
                Err(_) => break,
            }
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

// ── Chat session ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatRole {
    User,
    Assistant,
}

#[derive(Debug, Clone)]
pub struct ChatEntry {
    pub role: ChatRole,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    /// Fresh open with no history — cursor ready.
    Fresh,
    /// Re-opened with existing history — input shows placeholder.
    WelcomeBack,
    /// User is typing.
    Typing,
    /// LLM call in flight.
    Thinking,
}

pub struct ChatSession {
    pub history: Vec<ChatEntry>,
    pub input:   String,
    pub state:   SessionState,
    pub pending: Option<Receiver<anyhow::Result<String>>>,
}

impl ChatSession {
    pub fn new() -> Self {
        Self {
            history: Vec::new(),
            input:   String::new(),
            state:   SessionState::Fresh,
            pending: None,
        }
    }

    /// Called when the panel is toggled open.
    pub fn on_panel_open(&mut self) {
        self.state = if self.history.is_empty() {
            SessionState::Fresh
        } else {
            SessionState::WelcomeBack
        };
    }

    /// Reset history and input (WelcomeBack + Enter).
    pub fn handle_reset(&mut self) {
        self.history.clear();
        self.input.clear();
        self.pending = None;
        self.state = SessionState::Fresh;
    }

    /// Append a printable char to the input buffer.
    pub fn push_char(&mut self, c: char) {
        self.input.push(c);
        if self.state != SessionState::Thinking {
            self.state = SessionState::Typing;
        }
    }

    /// Remove the last char from the input buffer (multi-byte safe).
    pub fn backspace(&mut self) {
        self.input.pop();
    }
}

// ── ResearchPanel ─────────────────────────────────────────────────────────────

pub struct ResearchPanel {
    pub journal: JournalTailer,
    pub session: ChatSession,
}

impl Default for ResearchPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl ResearchPanel {
    /// Construct a new panel. Spawns the journal tailer thread immediately.
    pub fn new() -> Self {
        Self {
            journal: JournalTailer::new(),
            session: ChatSession::new(),
        }
    }

    /// Called each frame when the panel is open: drain journal + poll answer.
    pub fn tick(&mut self) {
        self.journal.drain();
        self.poll_answer();
    }

    /// Check whether an in-flight answer has arrived.
    fn poll_answer(&mut self) {
        let result = match &self.session.pending {
            Some(rx) => rx.try_recv().ok(),
            None     => return,
        };
        if let Some(outcome) = result {
            self.session.pending = None;
            let text = match outcome {
                Ok(t)  => t,
                Err(e) => format!("[kern error: {e}]"),
            };
            self.session.history.push(ChatEntry { role: ChatRole::Assistant, text });
            self.session.state = SessionState::Typing;
        }
    }

    /// Render the research panel into `stdout`.
    ///
    /// `cols` and `rows` are the full terminal dimensions.
    /// Row 0 is reserved for the status bar (not drawn here).
    /// The panel occupies rows 1..(rows-1) inclusive.
    pub fn draw(&self, stdout: &mut impl Write, cols: u16, rows: u16) -> io::Result<()> {
        let left_cols  = cols / 2;
        let right_cols = cols - left_cols;
        let pane_rows  = rows.saturating_sub(1); // exclude status bar
        let row_offset: u16 = 1;

        // ── Divider ───────────────────────────────────────────────────────
        queue!(stdout, SetForegroundColor(Color::DarkGrey))?;
        for r in 0..pane_rows {
            queue!(stdout, MoveTo(left_cols, r + row_offset), Print("│"))?;
        }
        queue!(stdout, ResetColor)?;

        // ── Journal pane (right) ──────────────────────────────────────────
        let journal_width = right_cols.saturating_sub(1) as usize;
        let visible_lines: Vec<&str> = {
            let skip = self.journal.ring.len().saturating_sub(pane_rows as usize);
            self.journal.ring.iter().skip(skip).map(|s| s.as_str()).collect()
        };

        let blank_rows = (pane_rows as usize).saturating_sub(visible_lines.len());
        queue!(stdout, SetForegroundColor(Color::DarkGrey))?;
        for r in 0..(blank_rows as u16) {
            queue!(stdout,
                MoveTo(left_cols + 1, r + row_offset),
                Print(" ".repeat(journal_width))
            )?;
        }
        for (i, line) in visible_lines.iter().enumerate() {
            let row = row_offset + blank_rows as u16 + i as u16;
            let truncated: String = line.chars().take(journal_width).collect();
            let padding = journal_width.saturating_sub(truncated.chars().count());
            queue!(stdout,
                MoveTo(left_cols + 1, row),
                Print(&truncated),
                Print(" ".repeat(padding))
            )?;
        }
        queue!(stdout, ResetColor)?;

        // ── Chat pane (left) ──────────────────────────────────────────────
        let chat_width  = left_cols as usize;
        let warn_row    = row_offset + pane_rows - 1;
        let input_row   = warn_row.saturating_sub(1);
        let divider_row = input_row.saturating_sub(1);
        let history_rows = pane_rows.saturating_sub(3) as usize;

        // Build all rendered lines from history.
        let mut all_lines: Vec<(bool, String)> = Vec::new(); // (is_user, line)
        for entry in &self.session.history {
            let is_user = entry.role == ChatRole::User;
            if is_user {
                let prefix = "▶ ";
                let prefix_len = prefix.chars().count();
                let max_text = chat_width.saturating_sub(prefix_len);
                let text: String = entry.text.chars().take(max_text).collect();
                let full = format!("{prefix}{text}");
                let pad = chat_width.saturating_sub(full.chars().count());
                all_lines.push((true, format!("{}{full}", " ".repeat(pad))));
            } else {
                for chunk in wrap_text_preserving(&entry.text, chat_width) {
                    all_lines.push((false, chunk));
                }
            }
        }

        // Take last history_rows lines.
        let start = all_lines.len().saturating_sub(history_rows);
        let visible_history = &all_lines[start..];
        let blank_history = history_rows.saturating_sub(visible_history.len());

        for r in 0..(blank_history as u16) {
            queue!(stdout, MoveTo(0, row_offset + r), Print(" ".repeat(chat_width)))?;
        }
        for (i, (is_user, line)) in visible_history.iter().enumerate() {
            let row = row_offset + blank_history as u16 + i as u16;
            let padding = chat_width.saturating_sub(line.chars().count());
            if *is_user {
                queue!(stdout,
                    MoveTo(0, row),
                    SetAttribute(Attribute::Bold),
                    Print(line),
                    Print(" ".repeat(padding)),
                    SetAttribute(Attribute::Reset)
                )?;
            } else {
                queue!(stdout,
                    MoveTo(0, row),
                    Print(line),
                    Print(" ".repeat(padding))
                )?;
            }
        }

        // Divider line.
        queue!(stdout,
            MoveTo(0, divider_row),
            SetForegroundColor(Color::DarkGrey),
            Print("─".repeat(chat_width)),
            ResetColor
        )?;

        // Input line.
        match self.session.state {
            SessionState::WelcomeBack => {
                let placeholder = "type to continue, enter to reset";
                let padding = chat_width.saturating_sub(placeholder.len());
                queue!(stdout,
                    MoveTo(0, input_row),
                    SetForegroundColor(Color::DarkGrey),
                    Print(placeholder),
                    Print(" ".repeat(padding)),
                    ResetColor
                )?;
            }
            SessionState::Thinking => {
                let thinking = "▸ thinking…";
                let thinking_len = thinking.chars().count();
                let padding = chat_width.saturating_sub(thinking_len);
                queue!(stdout,
                    MoveTo(0, input_row),
                    Print(thinking),
                    Print(" ".repeat(padding))
                )?;
            }
            _ => {
                let cursor = "█";
                let max_input = chat_width.saturating_sub(cursor.chars().count());
                let visible: String = self.session.input
                    .chars()
                    .rev()
                    .take(max_input)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                let combined = format!("{visible}{cursor}");
                let padding = chat_width.saturating_sub(combined.chars().count());
                queue!(stdout,
                    MoveTo(0, input_row),
                    Print(&combined),
                    Print(" ".repeat(padding))
                )?;
            }
        }

        // Persistent dim warning: every query sends the whole buffer.
        {
            let warn = "whole conversation is sent to kern for accuracy";
            let shown: String = warn.chars().take(chat_width).collect();
            let pad = chat_width.saturating_sub(shown.chars().count());
            queue!(stdout,
                MoveTo(0, warn_row),
                SetForegroundColor(Color::DarkGrey),
                Print(&shown),
                Print(" ".repeat(pad)),
                ResetColor
            )?;
        }

        Ok(())
    }

    /// Handle a key event when the panel is open.
    ///
    /// Returns `true` if the panel should be closed.
    pub fn handle_key(&mut self, kev: &crossterm::event::KeyEvent) -> bool {
        use crossterm::event::{KeyCode, KeyModifiers};

        match self.session.state {
            SessionState::WelcomeBack => match kev.code {
                KeyCode::Esc => return true,
                // Enter on the (empty) placeholder is the ONLY clear-memory gesture.
                KeyCode::Enter => self.session.handle_reset(),
                // Any editing gesture CONTINUES the conversation: keep history, start typing.
                KeyCode::Backspace => self.session.state = SessionState::Typing,
                KeyCode::Char(c)
                    if kev.modifiers == KeyModifiers::NONE
                    || kev.modifiers == KeyModifiers::SHIFT =>
                {
                    // push_char appends and flips state to Typing; history untouched.
                    self.session.push_char(c);
                }
                _ => {}
            },

            SessionState::Thinking => {
                if kev.code == KeyCode::Esc {
                    // Cancel: drop pending receiver; thread gets broken-pipe.
                    self.session.pending = None;
                    self.session.state   = SessionState::Typing;
                }
                // All other keys ignored while thinking.
            }

            _ => match kev.code {
                KeyCode::Esc => return true,
                KeyCode::Enter => {
                    if !self.session.input.is_empty() {
                        let text = std::mem::take(&mut self.session.input);
                        self.session.history.push(ChatEntry {
                            role: ChatRole::User,
                            text,
                        });
                        // Multi-turn: send the WHOLE conversation buffer to kern.
                        let query = build_kern_query(&self.session.history);
                        let (tx, rx) = mpsc::sync_channel(1);
                        // Capture the runtime handle from the spawn_blocking context
                        // so the answer thread (a plain OS thread with no async context)
                        // can block on the async kern RPC call.
                        match tokio::runtime::Handle::try_current() {
                            Ok(handle) => {
                                std::thread::Builder::new()
                                    .name("kern-research-answer".into())
                                    .spawn(move || {
                                        let _ = tx.send(handle.block_on(kern_answer(query)));
                                    })
                                    .expect("spawn kern-research-answer");
                            }
                            Err(e) => {
                                // No runtime available (e.g. unit test context).
                                let _ = tx.send(Err(anyhow::anyhow!("no tokio runtime: {e}")));
                            }
                        }
                        self.session.pending = Some(rx);
                        self.session.state   = SessionState::Thinking;
                    }
                }
                KeyCode::Backspace => self.session.backspace(),
                KeyCode::Char(c)
                    if kev.modifiers == KeyModifiers::NONE
                    || kev.modifiers == KeyModifiers::SHIFT =>
                {
                    self.session.push_char(c);
                }
                _ => {}
            },
        }

        false // panel stays open
    }
}

// ── Kern answer (internal RPC) ────────────────────────────────────────────────

/// Serialize the full chat buffer into one kern query string.
///
/// The WHOLE conversation is sent for answer accuracy (the UI warns the user of
/// this). The current question is the last `User` entry already pushed onto
/// `history`; the trailing instruction anchors kern to it.
fn build_kern_query(history: &[ChatEntry]) -> String {
    let mut s = String::new();
    for e in history {
        let who = match e.role {
            ChatRole::User      => "User",
            ChatRole::Assistant => "Assistant",
        };
        s.push_str(who);
        s.push_str(": ");
        s.push_str(&e.text);
        s.push_str("\n\n");
    }
    s.push_str("Answer the most recent User question above, using the prior turns as context.");
    s
}

/// Call the kern daemon's `query` tool (with `answer: true`) via the internal
/// typed RPC channel, bypassing the MCP JSON-over-TCP layer entirely.
async fn kern_answer(query: String) -> anyhow::Result<String> {
    use trnsprt::kern_rpc::{CallToolReq, KernRpcClient};
    use trnsprt::typed::JsonEnvelopeCodec;

    let client = KernRpcClient::<JsonEnvelopeCodec>::connect_local()
        .await
        .map_err(|e| anyhow::anyhow!("kern connect: {e}"))?;
    let res = client
        .call_tool(CallToolReq {
            name: "query".into(),
            args: serde_json::json!({"text": query, "k": 5, "answer": true}),
        })
        .await
        .map_err(|e| anyhow::anyhow!("kern query: {e}"))?;

    let env = &res.envelope;
    if env.get("isError").and_then(|v| v.as_bool()).unwrap_or(false) {
        anyhow::bail!("kern: {}", extract_rpc_text(env));
    }
    let raw = extract_rpc_text(env);
    answer_from_envelope_text(&raw)
}

/// Concatenate all `type: text` items from an MCP content envelope.
fn extract_rpc_text(envelope: &serde_json::Value) -> String {
    envelope
        .get("content")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    if item.get("type")?.as_str()? == "text" {
                        item.get("text")?.as_str()
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// Pull kern's synthesized prose answer out of the `query` tool's JSON text.
///
/// kern returns `{"entities":[…],"answer":"<prose>"}`. We surface the `answer`
/// field; if it is empty/absent we degrade to a terse entity summary. We never
/// return the raw JSON object (that is the "JSON list" bug this fixes).
fn answer_from_envelope_text(raw: &str) -> anyhow::Result<String> {
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(v) => {
            if let Some(ans) = v.get("answer").and_then(|a| a.as_str()) {
                if !ans.trim().is_empty() {
                    return Ok(ans.to_string());
                }
            }
            let summary = v
                .get("entities")
                .and_then(|e| e.as_array())
                .map(|arr| summarize_entities(arr))
                .unwrap_or_default();
            if summary.trim().is_empty() {
                anyhow::bail!("kern returned no answer");
            }
            Ok(summary)
        }
        // Not JSON (shouldn't happen) — return raw so output is never silently dropped.
        Err(_) => Ok(raw.to_string()),
    }
}

/// One-line, human-readable summary of entity `text` (falling back to `id`).
/// Graceful degrade when `answer:true` produced retrieval but no synthesis.
fn summarize_entities(arr: &[serde_json::Value]) -> String {
    let names: Vec<String> = arr
        .iter()
        .take(5)
        .filter_map(|e| {
            e.get("text")
                .and_then(|t| t.as_str())
                .or_else(|| e.get("id").and_then(|i| i.as_str()))
                .map(|s| {
                    let t: String = s.chars().take(60).collect();
                    t.trim().to_string()
                })
                .filter(|s| !s.is_empty())
        })
        .collect();
    if names.is_empty() {
        String::new()
    } else {
        format!("Related: {}", names.join("; "))
    }
}

/// Wrap one logical line into rows of at most `width` chars (greedy word-wrap).
fn wrap_one_line(text: &str, width: usize) -> Vec<String> {
    if width == 0 { return vec![text.to_string()]; }
    let mut lines   = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
        } else if current.chars().count() + 1 + word.chars().count() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(current.clone());
            current = word.to_string();
        }
    }
    if !current.is_empty() { lines.push(current); }
    if lines.is_empty()    { lines.push(String::new()); }
    lines
}

/// Word-wrap `text` to `width`, preserving the source line structure: each
/// '\n'-delimited line wraps independently and blank lines are kept as blank
/// rows. Multi-paragraph / bulleted kern answers then render readably instead
/// of collapsing into one run-on block.
fn wrap_text_preserving(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    for src_line in text.split('\n') {
        if src_line.trim().is_empty() {
            out.push(String::new());
            continue;
        }
        out.extend(wrap_one_line(src_line, width));
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── format_entry / JournalTailer ─────────────────────────────────────────

    #[test]
    fn format_entry_overview_filters_tool_call() {
        let e = Entry::new(Kind::ToolCall, "tool.name", serde_json::json!("some payload"));
        assert!(format_entry_overview(&e).is_none(),
            "ToolCall should be filtered by format_entry_overview");
    }

    #[test]
    fn format_entry_overview_passes_goal() {
        let e = Entry::new(Kind::Goal, "kern.goal", serde_json::json!("become smarter"));
        let line = format_entry_overview(&e);
        assert!(line.is_some(), "Goal should pass the overview filter");
        let s = line.unwrap();
        assert!(s.contains("goal"),           "should contain tag 'goal': got {s:?}");
        assert!(s.contains("become smarter"), "should contain payload: got {s:?}");
    }

    #[test]
    fn format_entry_plain_key_no_payload() {
        let e = Entry::new(Kind::TurnStart, "kern.turn", serde_json::Value::Null);
        let s = format_entry(&e);
        assert!(s.contains("turn>"),     "expected tag 'turn>': got {s:?}");
        assert!(s.contains("kern.turn"), "expected key: got {s:?}");
    }

    #[test]
    fn journal_tailer_ring_caps_at_512() {
        let (tx, rx) = mpsc::sync_channel(600);
        let mut tailer = JournalTailer { rx, ring: VecDeque::new() };
        for i in 0..600usize {
            let _ = tx.try_send(format!("line {i}"));
        }
        tailer.drain();
        assert_eq!(tailer.ring.len(), 512, "ring should be capped at 512");
        assert!(tailer.ring.back().unwrap().contains("line 599"), "newest line should be last");
    }

    // ── ChatSession ──────────────────────────────────────────────────────────

    #[test]
    fn chat_session_starts_fresh() {
        let s = ChatSession::new();
        assert!(matches!(s.state, SessionState::Fresh));
        assert!(s.history.is_empty());
        assert!(s.input.is_empty());
        assert!(s.pending.is_none());
    }

    #[test]
    fn chat_session_push_user_entry() {
        let mut s = ChatSession::new();
        s.history.push(ChatEntry { role: ChatRole::User, text: "hello kern".to_string() });
        assert_eq!(s.history.len(), 1);
        assert!(matches!(s.history[0].role, ChatRole::User));
    }

    #[test]
    fn chat_session_welcome_back_on_reopen_with_history() {
        let mut s = ChatSession::new();
        s.history.push(ChatEntry { role: ChatRole::User, text: "hi".to_string() });
        s.on_panel_open();
        assert!(matches!(s.state, SessionState::WelcomeBack));
    }

    #[test]
    fn chat_session_fresh_on_reopen_without_history() {
        let mut s = ChatSession::new();
        s.on_panel_open();
        assert!(matches!(s.state, SessionState::Fresh));
    }

    #[test]
    fn chat_session_welcome_back_enter_resets() {
        let mut s = ChatSession::new();
        s.history.push(ChatEntry { role: ChatRole::User, text: "old".to_string() });
        s.state = SessionState::WelcomeBack;
        s.handle_reset();
        assert!(s.history.is_empty());
        assert!(matches!(s.state, SessionState::Fresh));
    }

    #[test]
    fn chat_session_push_char_typing() {
        let mut s = ChatSession::new();
        s.push_char('h');
        s.push_char('i');
        assert_eq!(s.input, "hi");
        assert!(matches!(s.state, SessionState::Typing));
    }

    #[test]
    fn chat_session_backspace_pops_char() {
        let mut s = ChatSession::new();
        s.input = "hé".to_string();
        s.backspace();
        assert_eq!(s.input, "h");
    }

    #[test]
    fn chat_session_backspace_empty_noop() {
        let mut s = ChatSession::new();
        s.backspace(); // should not panic
        assert!(s.input.is_empty());
    }

    // ── ResearchPanel ────────────────────────────────────────────────────────

    #[test]
    fn research_panel_new_constructs() {
        let panel = ResearchPanel::new();
        assert!(matches!(panel.session.state, SessionState::Fresh));
    }

    #[test]
    fn research_panel_typing_on_printable_key() {
        let mut panel = ResearchPanel::new();
        panel.session.push_char('a');
        assert_eq!(panel.session.input, "a");
        assert!(matches!(panel.session.state, SessionState::Typing));
    }

    #[test]
    fn research_panel_welcome_back_reset() {
        let mut panel = ResearchPanel::new();
        panel.session.history.push(ChatEntry { role: ChatRole::User, text: "old".to_string() });
        panel.session.state = SessionState::WelcomeBack;
        panel.session.handle_reset();
        assert!(panel.session.history.is_empty());
        assert!(matches!(panel.session.state, SessionState::Fresh));
    }

    #[test]
    fn wrap_text_single_line() {
        let lines = wrap_one_line("hello world", 20);
        assert_eq!(lines, vec!["hello world"]);
    }

    #[test]
    fn wrap_text_wraps_at_width() {
        let lines = wrap_one_line("hello world foo bar", 11);
        assert_eq!(lines[0], "hello world");
        assert_eq!(lines[1], "foo bar");
    }

    #[test]
    fn wrap_text_empty_string() {
        let lines = wrap_one_line("", 20);
        assert_eq!(lines, vec![""]);
    }

    #[test]
    fn wrap_text_zero_width_returns_original() {
        let lines = wrap_one_line("hello", 0);
        assert_eq!(lines, vec!["hello"]);
    }

    // ── WelcomeBack: typing continues, Enter resets ──────────────────────────

    #[test]
    fn welcome_back_typing_continues() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut panel = ResearchPanel::new();
        panel.session.history.push(ChatEntry { role: ChatRole::User, text: "old".into() });
        panel.session.state = SessionState::WelcomeBack;
        let close = panel.handle_key(&KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
        assert!(!close, "typing must not close the panel");
        assert_eq!(panel.session.history.len(), 1, "history preserved on typing (continue, not reset)");
        assert_eq!(panel.session.input, "h");
        assert!(matches!(panel.session.state, SessionState::Typing));
    }

    #[test]
    fn welcome_back_enter_resets() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut panel = ResearchPanel::new();
        panel.session.history.push(ChatEntry { role: ChatRole::User, text: "old".into() });
        panel.session.state = SessionState::WelcomeBack;
        let close = panel.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!close, "reset must not close the panel");
        assert!(panel.session.history.is_empty(), "Enter on the placeholder clears memory");
        assert!(matches!(panel.session.state, SessionState::Fresh));
    }

    #[test]
    fn welcome_back_backspace_continues_keeps_history() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut panel = ResearchPanel::new();
        panel.session.history.push(ChatEntry { role: ChatRole::User, text: "old".into() });
        panel.session.state = SessionState::WelcomeBack;
        panel.handle_key(&KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(panel.session.history.len(), 1, "backspace continues, does not reset");
        assert!(matches!(panel.session.state, SessionState::Typing));
    }

    // ── Whole-buffer multi-turn query ────────────────────────────────────────

    #[test]
    fn build_kern_query_includes_all_turns() {
        let hist = vec![
            ChatEntry { role: ChatRole::User,      text: "first q".into() },
            ChatEntry { role: ChatRole::Assistant, text: "first a".into() },
            ChatEntry { role: ChatRole::User,      text: "second q".into() },
        ];
        let q = build_kern_query(&hist);
        assert!(q.contains("first q"),  "includes earliest user turn: {q}");
        assert!(q.contains("first a"),  "includes prior assistant turn: {q}");
        assert!(q.contains("second q"), "includes latest user turn: {q}");
        assert!(q.contains("most recent"), "anchors kern to the latest question: {q}");
    }

    // ── Answer extraction (no JSON dump) ─────────────────────────────────────

    #[test]
    fn answer_from_envelope_extracts_answer_field() {
        let raw = r#"{"entities":[{"id":"a","text":"x"}],"answer":"hello there"}"#;
        assert_eq!(answer_from_envelope_text(raw).unwrap(), "hello there");
    }

    #[test]
    fn answer_from_envelope_falls_back_without_answer() {
        let raw = r#"{"entities":[{"id":"kern.goal","text":"become smarter"},{"id":"kern.viewer","text":"viewer design"}]}"#;
        let out = answer_from_envelope_text(raw).unwrap();
        assert!(out.contains("become smarter"), "summary uses entity text: {out}");
        assert!(!out.contains('{'), "never a raw JSON dump: {out}");
    }

    #[test]
    fn answer_from_envelope_empty_answer_uses_fallback() {
        let raw = r#"{"entities":[{"id":"e1","text":"alpha"}],"answer":"   "}"#;
        let out = answer_from_envelope_text(raw).unwrap();
        assert!(out.contains("alpha"), "blank answer falls back to entities: {out}");
    }

    // ── Line-preserving wrap ─────────────────────────────────────────────────

    #[test]
    fn wrap_text_preserving_keeps_paragraph_breaks() {
        let text = "Para one is short.\n\n- bullet a\n- bullet b";
        let lines = wrap_text_preserving(text, 40);
        assert!(lines.iter().any(|l| l.contains("Para one")), "first paragraph present");
        assert!(lines.iter().any(|l| l.is_empty()), "blank line between paragraphs preserved");
        assert!(lines.iter().any(|l| l.contains("- bullet a")), "bullet a on its own line");
        assert!(lines.iter().any(|l| l.contains("- bullet b")), "bullet b on its own line");
    }
}
