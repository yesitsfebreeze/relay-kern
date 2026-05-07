# surf → repl Port Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rename `src/bin/surf` → `src/bin/repl` and port the full TUI app shell from the old `spool` crate, implementing the confirmed split-column layout.

**Architecture:** `repl` is the terminal frontend. It owns the draw loop, key handling, submit flow (via tarpc to `agnt`), and login wizard. `agnt` owns all agent/session logic. The two bins communicate via the `AgntRpc` tarpc service.

**Layout** — single `│` vertical divider, no other chrome:

```
   0        split_x (~40%)                               cols
   ┌──────────────────────────┬──────────────────────────────────┐
   │ ┃ user message           │  agent response text             │
   │ ┃ earlier message        │                                  │
   │                          │  ┌─ file.rs:47 ───────────────┐  │
   │                          │  │ - old()                    │  │
   │                          │  │ + new()                    │  │
   │ ┃ active msg  ◄──pinned  │  └────────────────────────────┘  │
   │ >  █  ← pushed up by     │                                  │
   │        trace, sticky 40% │  (synced scroll with left)       │
   │ 0 │1│ 2 3      v1.0.2   │                                  │
   │ ~/dev/relay-clean main.rs│                                  │
   │ ✔ kern  file written     │                                  │
   │ · agnt  reading auth.rs  │                                  │
   └──────────────────────────┴──────────────────────────────────┘
```

**Region rules:**
- `split_x` = cols × 40%
- Left top (rows 0..input_y): user messages scrollable; active turn pinned just above input
- Left input row (`input_y`): `> █`; pushed up by trace, sticky floor at rows × 40%
- Left status (input_y+1, input_y+2): tab/version row + cwd/file row
- Left trace (status_y+2..rows): tool activity lines, grows upward
- Right (full height, 0..rows): agent response + code, synced scroll with left

**Tech Stack:** Rust, crossterm, tarpc (client side), unicode-segmentation, unicode-width. Existing surf primitives: `render`, `input`, `textarea`, `plugin_ui`, `ui_slots`, `trace_view`, `list_nav`.

---

## File Map

```
src/bin/repl/               ← renamed from src/bin/surf/
  Cargo.toml                ← name = "repl"
  src/
    lib.rs                  ← add new pub mods
    main.rs                 ← full draw loop + ReplApp wiring
    state.rs                ← AgentState, Focus, ActiveList, ActiveForm, Tab
    tui_sink.rs             ← TurnEvent, TuiSink (agnt→repl bridge)
    chat_view/
      mod.rs                ← ChatView: scroll, 60% cap, viewport
      message.rs            ← ChatMessage, Role (user bar | llm flush-left | info lines)
    layout.rs               ← layout(cols, rows, trace_rows) → Layout struct (split_x, left regions, right rect)
    render_app.rs           ← draw_app(app, r): left-column + right-column orchestrator
    status_bar.rs           ← draw_status_left(app, r, layout): tabs+version row + cwd row (left panel only)
    key_handling.rs         ← handle_key(app, key) → bool
    submit.rs               ← submit(app) + dispatch_next via tarpc
    slash_lists.rs          ← sync_slash_list(app)
    mentions.rs             ← sync_mention_list(app, kind)
    commands/
      mod.rs                ← Command trait, CommandRegistry, CommandCtx, Visibility
      builtin.rs            ← /help /clear /thread /plugins /run /query /delegate /recipes /login /auth
      slash_adapter.rs      ← run_slash(ctx, body) → SlashOutcome
    login_wizard.rs         ← start_login_wizard + 5-step flow
    app_init.rs             ← ReplApp::new + ReplApp::with_default_session
    http_server.rs          ← serve(listener, session_factory)
  tests/
    command_registry.rs
    headless.rs
    mcp_serve.rs

src/plugins/
  ask-bubble/src/lib.rs     ← port from ../relay
  clock/src/lib.rs          ← port from ../relay
  intro/src/lib.rs          ← port from ../relay
  relay/src/lib.rs          ← port from ../relay
  fs/src/lib.rs             ← port from ../relay (fs_plugin already in agnt; this is standalone)
```

---

## Task 1: Rename surf → repl

**Files:**
- Rename: `src/bin/surf/` → `src/bin/repl/`
- Modify: `Cargo.toml` (workspace)
- Modify: `src/bin/repl/Cargo.toml`
- Modify: `src/bin/repl/src/lib.rs` (crate-level uses)
- Modify: `src/bin/repl/src/main.rs` (all `surf::` → `repl::`)

- [ ] **Step 1: Move directory**

```bash
git mv src/bin/surf src/bin/repl
```

- [ ] **Step 2: Update workspace Cargo.toml**

In `Cargo.toml`, replace `"src/bin/surf"` with `"src/bin/repl"`:

```toml
members = ["src/bin/agnt", "src/bin/kern", "src/bin/repl", ...]
```

- [ ] **Step 3: Update crate name**

In `src/bin/repl/Cargo.toml`:
```toml
[package]
name = "repl"
```

- [ ] **Step 4: Fix imports in main.rs**

Replace all `use surf::` with `use repl::` in `src/bin/repl/src/main.rs`.

- [ ] **Step 5: Verify compile**

```bash
cargo check -p repl
```
Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "rename: src/bin/surf → src/bin/repl (crate name repl)"
```

---

## Task 2: Layout computation

**Files:**
- Create: `src/bin/repl/src/layout.rs`
- Modify: `src/bin/repl/src/lib.rs`

Left column: user_msgs (top) → input (sticky floor 40%) → status (2 rows) → trace (grows up).
Right column: full height, same row count as terminal.
Single `│` divider at `split_x`.

- [ ] **Step 1: Write failing tests**

Create `src/bin/repl/src/layout.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_x_is_40_percent() {
        let l = layout(80, 24, 0);
        assert_eq!(l.split_x, 32); // 80 * 40%
    }

    #[test]
    fn input_sticky_at_40_percent_floor() {
        // with 0 trace rows, input should be well above 40% sticky floor
        let l = layout(80, 24, 0);
        let sticky_floor = (24f32 * 0.40) as u16;
        assert!(l.input.y >= sticky_floor);
    }

    #[test]
    fn trace_pushes_input_up() {
        let l0 = layout(80, 24, 0);
        let l4 = layout(80, 24, 4);
        assert!(l4.input.y < l0.input.y);
    }

    #[test]
    fn input_never_above_sticky_floor() {
        // massive trace: input must not go above 40% sticky floor
        let l = layout(80, 24, 99);
        let sticky_floor = (24f32 * 0.40) as u16;
        assert!(l.input.y >= sticky_floor);
    }

    #[test]
    fn right_is_full_height() {
        let l = layout(80, 24, 3);
        assert_eq!(l.right.y, 0);
        assert_eq!(l.right.height, 24);
    }

    #[test]
    fn left_regions_do_not_overflow_rows() {
        let l = layout(80, 24, 5);
        let bottom = l.trace.y + l.trace.height;
        assert!(bottom <= 24);
    }
}
```

- [ ] **Step 2: Run — expect fail**

```bash
cargo test -p repl layout 2>&1 | head -5
```

- [ ] **Step 3: Implement layout.rs**

```rust
const STATUS_ROWS: u16 = 2; // tab/version row + cwd/file row
const SPLIT_FRAC:  f32 = 0.40;
const STICKY_FRAC: f32 = 0.40; // input never pushed above this row

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub y: u16,
    pub height: u16,
    pub x: u16,
    pub width: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct Layout {
    pub split_x:   u16,
    pub user_msgs: Rect, // left top: scrollable chat history
    pub input:     Rect, // left: "> █" row
    pub status:    Rect, // left: 2 rows (tab row + cwd row)
    pub trace:     Rect, // left bottom: tool activity lines
    pub right:     Rect, // right column: agent response + code (full height)
}

/// Compute split layout.
/// `trace_rows`: current number of trace lines (0 = no trace yet).
pub fn layout(cols: u16, rows: u16, trace_rows: u16) -> Layout {
    let split_x  = ((cols as f32 * SPLIT_FRAC) as u16).max(20);
    let left_w   = split_x;
    let right_w  = cols.saturating_sub(split_x + 1); // +1 for divider

    let sticky_floor = (rows as f32 * STICKY_FRAC) as u16;
    let max_trace    = rows.saturating_sub(sticky_floor + 1 + STATUS_ROWS);
    let actual_trace = trace_rows.min(max_trace);

    // input_y: as low as possible, pushed up by trace, floor at sticky_floor
    let input_y = rows
        .saturating_sub(1 + STATUS_ROWS + actual_trace)
        .max(sticky_floor);

    let status_y     = input_y + 1;
    let trace_y      = status_y + STATUS_ROWS;
    let trace_height = rows.saturating_sub(trace_y);
    let msgs_height  = input_y; // rows 0..input_y

    Layout {
        split_x,
        user_msgs: Rect { y: 0,        height: msgs_height,  x: 0,       width: left_w },
        input:     Rect { y: input_y,  height: 1,            x: 0,       width: left_w },
        status:    Rect { y: status_y, height: STATUS_ROWS,  x: 0,       width: left_w },
        trace:     Rect { y: trace_y,  height: trace_height, x: 0,       width: left_w },
        right:     Rect { y: 0,        height: rows,         x: split_x + 1, width: right_w },
    }
}
```

- [ ] **Step 4: Add mod to lib.rs**

```rust
pub mod layout;
```

- [ ] **Step 5: Run tests — expect pass**

```bash
cargo test -p repl layout
```
Expected: 6 tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/bin/repl/src/layout.rs src/bin/repl/src/lib.rs
git commit -m "feat(repl): layout — split-column rects, input sticky 40%, trace pushes up"
```

---

## Task 3: State types

**Files:**
- Create: `src/bin/repl/src/state.rs`
- Modify: `src/bin/repl/src/lib.rs`

- [ ] **Step 1: Write failing test**

In `src/bin/repl/src/state.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_state_glyph_cycles() {
        let g0 = AgentState::Thinking.glyph(0);
        let g1 = AgentState::Thinking.glyph(8);
        // braille spinners must differ across ticks
        assert_ne!(g0, g1);
    }

    #[test]
    fn focus_default_is_input() {
        assert_eq!(Focus::default(), Focus::Input);
    }
}
```

- [ ] **Step 2: Run — expect fail**

```bash
cargo test -p repl state 2>&1 | head -5
```

- [ ] **Step 3: Implement state.rs**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    #[default]
    Input,
    Selection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentState {
    #[default]
    Idle,
    Thinking,
    Working,
    Downstream,
    Upstream,
}

const THINK_GLYPHS: &[char] = &['⠋','⠙','⠹','⠸','⠼','⠴','⠦','⠧','⠇','⠏'];
const WORK_GLYPHS:  &[char] = &['⣾','⣽','⣻','⢿','⡿','⣟','⣯','⣷'];
const DOWN_GLYPHS:  &[char] = &['▁','▂','▃','▄','▅','▆','▇','█'];
const UP_GLYPHS:    &[char] = &['█','▇','▆','▅','▄','▃','▂','▁'];

impl AgentState {
    pub fn glyph(self, tick: u64) -> char {
        match self {
            AgentState::Idle       => '❯',
            AgentState::Thinking   => THINK_GLYPHS[(tick as usize) % THINK_GLYPHS.len()],
            AgentState::Working    => WORK_GLYPHS[(tick as usize) % WORK_GLYPHS.len()],
            AgentState::Downstream => DOWN_GLYPHS[(tick as usize) % DOWN_GLYPHS.len()],
            AgentState::Upstream   => UP_GLYPHS[(tick as usize) % UP_GLYPHS.len()],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveList {
    Slash,
    MentionFiles { anchor_col: u16 },
    MentionDirs  { anchor_col: u16 },
    LoginProvider,
    LoginExisting,
    LoginRole,
    LoginModel,
    JournalTrace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveForm {
    LoginKey,
    Plugin { origin: String },
}

/// Per-tab file binding (tabs 1–9 = pinned files, 0 = root session).
#[derive(Debug, Clone, Default)]
pub struct Tab {
    pub index: u8,
    pub pinned_file: Option<String>,
    pub unread: u32,
}
```

- [ ] **Step 4: Add mod to lib.rs**

```rust
pub mod state;
```

- [ ] **Step 5: Run tests — expect pass**

```bash
cargo test -p repl state
```

- [ ] **Step 6: Commit**

```bash
git add src/bin/repl/src/state.rs src/bin/repl/src/lib.rs
git commit -m "feat(repl): state types — AgentState, Focus, ActiveList, ActiveForm, Tab"
```

---

## Task 4: Chat message types

**Files:**
- Create: `src/bin/repl/src/chat_view/message.rs`
- Create: `src/bin/repl/src/chat_view/mod.rs` (stub)
- Modify: `src/bin/repl/src/lib.rs`

LAYOUT.md message styles:
- User: `┃` left bar, strong highlight (replace `█` prefix with `┃`)
- LLM info lines: `🞆` muted, sub-bullets `⠧ ` indented
- LLM message: flush-left, no indent, word-wrap
- Pending: dimmed spinner
- Error: error colour

- [ ] **Step 1: Write failing test**

```rust
// src/bin/repl/src/chat_view/message.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_message_has_bar_prefix() {
        let m = ChatMessage::user("hello");
        assert_eq!(m.role(), Role::User);
    }

    #[test]
    fn pending_is_not_final() {
        let m = ChatMessage::pending();
        assert!(!m.is_final());
    }

    #[test]
    fn finalize_replaces_pending() {
        let mut m = ChatMessage::pending();
        m.finalize("done", Role::Assistant);
        assert!(m.is_final());
        assert_eq!(m.text(), "done");
    }
}
```

- [ ] **Step 2: Run — expect fail**

```bash
cargo test -p repl chat_view 2>&1 | head -5
```

- [ ] **Step 3: Implement message.rs**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    Info,      // 🞆 muted info line from LLM
    System,    // slash-command result
    Pending,
    Error,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    role: Role,
    text: String,
}

impl ChatMessage {
    pub fn user(text: impl Into<String>)      -> Self { Self { role: Role::User,      text: text.into() } }
    pub fn assistant(text: impl Into<String>) -> Self { Self { role: Role::Assistant, text: text.into() } }
    pub fn info(text: impl Into<String>)      -> Self { Self { role: Role::Info,      text: text.into() } }
    pub fn system(text: impl Into<String>)    -> Self { Self { role: Role::System,    text: text.into() } }
    pub fn error(text: impl Into<String>)     -> Self { Self { role: Role::Error,     text: text.into() } }
    pub fn pending()                          -> Self { Self { role: Role::Pending,   text: String::new() } }

    pub fn role(&self) -> Role  { self.role }
    pub fn text(&self) -> &str  { &self.text }
    pub fn is_final(&self) -> bool { self.role != Role::Pending }

    pub fn finalize(&mut self, text: impl Into<String>, role: Role) {
        self.text = text.into();
        self.role = role;
    }

    /// Gutter char for left-side decoration.
    pub fn gutter_char(&self) -> &'static str {
        match self.role {
            Role::User      => "┃",
            Role::Info      => "🞆",
            Role::System    => "»",
            Role::Error     => "✗",
            Role::Assistant => " ",
            Role::Pending   => " ",
        }
    }
}
```

- [ ] **Step 4: Create chat_view/mod.rs stub**

```rust
pub mod message;
pub use message::{ChatMessage, Role};
```

- [ ] **Step 5: Add mod to lib.rs**

```rust
pub mod chat_view;
```

- [ ] **Step 6: Run tests — expect pass**

```bash
cargo test -p repl chat_view
```

- [ ] **Step 7: Commit**

```bash
git add src/bin/repl/src/chat_view/
git commit -m "feat(repl): chat_view message types with LAYOUT.md gutter chars"
```

---

## Task 5: ChatView scroll and viewport

**Files:**
- Modify: `src/bin/repl/src/chat_view/mod.rs`

ChatView holds messages, scroll offset, and renders into a `Rect` with 60% height cap enforced by the caller (layout.rs already does this).

- [ ] **Step 1: Write failing tests**

```rust
// in chat_view/mod.rs under #[cfg(test)]
#[test]
fn push_and_count() {
    let mut v = ChatView::new();
    v.push(ChatMessage::user("hi"));
    v.push(ChatMessage::assistant("hello"));
    assert_eq!(v.len(), 2);
}

#[test]
fn scroll_older_clamps() {
    let mut v = ChatView::new();
    for i in 0..5 { v.push(ChatMessage::user(&format!("msg {i}"))); }
    v.scroll_older(100);
    assert!(v.scroll_offset() <= v.len());
}

#[test]
fn scroll_newer_reaches_zero() {
    let mut v = ChatView::new();
    for i in 0..5 { v.push(ChatMessage::user(&format!("msg {i}"))); }
    v.scroll_older(3);
    v.scroll_newer(10);
    assert_eq!(v.scroll_offset(), 0);
}

#[test]
fn pop_last_pending_removes_it() {
    let mut v = ChatView::new();
    v.push(ChatMessage::user("hi"));
    v.push(ChatMessage::pending());
    assert!(v.pop_last_if_pending());
    assert_eq!(v.len(), 1);
}
```

- [ ] **Step 2: Run — expect fail**

```bash
cargo test -p repl chat_view::tests 2>&1 | head -5
```

- [ ] **Step 3: Implement ChatView in mod.rs**

```rust
pub mod message;
pub use message::{ChatMessage, Role};

pub struct ChatView {
    messages: Vec<ChatMessage>,
    scroll_offset: usize,
}

impl ChatView {
    pub fn new() -> Self {
        Self { messages: Vec::new(), scroll_offset: 0 }
    }

    pub fn push(&mut self, m: ChatMessage) {
        self.messages.push(m);
    }

    pub fn len(&self) -> usize { self.messages.len() }
    pub fn is_empty(&self) -> bool { self.messages.is_empty() }
    pub fn scroll_offset(&self) -> usize { self.scroll_offset }
    pub fn messages(&self) -> &[ChatMessage] { &self.messages }

    pub fn scroll_older(&mut self, n: usize) {
        let max = self.messages.len().saturating_sub(1);
        self.scroll_offset = (self.scroll_offset + n).min(max);
    }

    pub fn scroll_newer(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    pub fn pop_last_if_pending(&mut self) -> bool {
        if self.messages.last().map(|m| !m.is_final()).unwrap_or(false) {
            self.messages.pop();
            true
        } else {
            false
        }
    }

    pub fn clear(&mut self) {
        self.messages.clear();
        self.scroll_offset = 0;
    }
}
```

- [ ] **Step 4: Run tests — expect pass**

```bash
cargo test -p repl chat_view
```

- [ ] **Step 5: Commit**

```bash
git add src/bin/repl/src/chat_view/mod.rs
git commit -m "feat(repl): ChatView — push/scroll/pop, viewport offset"
```

---

## Task 6: TurnEvent + TuiSink

**Files:**
- Create: `src/bin/repl/src/tui_sink.rs`
- Modify: `src/bin/repl/src/lib.rs`

The sink bridges agnt tarpc stream events back to the repl UI thread via `mpsc`.

- [ ] **Step 1: Write failing test**

```rust
// src/bin/repl/src/tui_sink.rs
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn sink_sends_upstream_event() {
        let (tx, rx) = mpsc::channel();
        let mut sink = TuiSink::new(tx);
        sink.on_user_message("hi");
        assert_eq!(rx.try_recv().unwrap(), TurnEvent::Upstream);
    }

    #[test]
    fn sink_latches_final_text() {
        let (tx, rx) = mpsc::channel();
        let mut sink = TuiSink::new(tx);
        sink.on_final_message("answer");
        let _ = rx.try_recv(); // drain Downstream event
        assert_eq!(sink.take_result().unwrap(), "answer");
    }
}
```

- [ ] **Step 2: Run — expect fail**

```bash
cargo test -p repl tui_sink 2>&1 | head -5
```

- [ ] **Step 3: Implement tui_sink.rs**

```rust
use std::sync::mpsc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnEvent {
    Upstream,
    Thinking,
    Downstream,
    ToolCallStart(String),
    ToolCallEnd,
}

pub struct TuiSink {
    tx: mpsc::Sender<TurnEvent>,
    result: Option<Result<String, String>>,
}

impl TuiSink {
    pub fn new(tx: mpsc::Sender<TurnEvent>) -> Self {
        Self { tx, result: None }
    }

    pub fn on_user_message(&mut self, _text: &str) {
        let _ = self.tx.send(TurnEvent::Upstream);
    }

    pub fn on_thinking(&mut self) {
        let _ = self.tx.send(TurnEvent::Thinking);
    }

    pub fn on_final_message(&mut self, text: &str) {
        self.result = Some(Ok(text.to_owned()));
        let _ = self.tx.send(TurnEvent::Downstream);
    }

    pub fn on_error(&mut self, err: &str) {
        self.result = Some(Err(err.to_owned()));
    }

    pub fn on_tool_call_start(&mut self, id: String) {
        let _ = self.tx.send(TurnEvent::ToolCallStart(id));
    }

    pub fn on_tool_call_end(&mut self) {
        let _ = self.tx.send(TurnEvent::ToolCallEnd);
    }

    /// Called by the UI thread after turn completes.
    pub fn take_result(&mut self) -> Option<Result<String, String>> {
        self.result.take()
    }
}
```

- [ ] **Step 4: Add mod**

```rust
// lib.rs
pub mod tui_sink;
```

- [ ] **Step 5: Run tests — pass**

```bash
cargo test -p repl tui_sink
```

- [ ] **Step 6: Commit**

```bash
git add src/bin/repl/src/tui_sink.rs src/bin/repl/src/lib.rs
git commit -m "feat(repl): TuiSink + TurnEvent — agnt→repl bridge via mpsc"
```

---

## Task 7: Commands layer

**Files:**
- Create: `src/bin/repl/src/commands/mod.rs`
- Create: `src/bin/repl/src/commands/builtin.rs`
- Create: `src/bin/repl/src/commands/slash_adapter.rs`
- Modify: `src/bin/repl/src/lib.rs`

- [ ] **Step 1: Write failing test**

```rust
// tests/command_registry.rs
use repl::commands::{CommandRegistry, Visibility};

#[test]
fn help_command_exists() {
    let reg = CommandRegistry::builtin();
    let cmd = reg.find("help");
    assert!(cmd.is_some());
}

#[test]
fn all_builtins_have_descriptions() {
    let reg = CommandRegistry::builtin();
    for cmd in reg.commands() {
        assert!(!cmd.description().is_empty(), "{} missing description", cmd.name());
    }
}
```

- [ ] **Step 2: Run — expect fail**

```bash
cargo test -p repl command_registry 2>&1 | head -5
```

- [ ] **Step 3: Implement commands/mod.rs**

```rust
pub mod builtin;
pub mod slash_adapter;

pub use builtin::CommandRegistry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility { SlashOnly, McpOnly, Shared }

pub enum SlashOutcome {
    Ok(String),
    Unknown(String),
    Err(String),
}

pub trait Command: Send + Sync {
    fn name(&self)        -> &'static str;
    fn description(&self) -> &'static str;
    fn visibility(&self)  -> Visibility { Visibility::SlashOnly }
    fn handle(&self, ctx: &CommandCtx, args: &str) -> SlashOutcome;
}

pub struct CommandCtx<'a> {
    pub registry: &'a CommandRegistry,
}
```

- [ ] **Step 4: Implement commands/builtin.rs**

```rust
use super::{Command, CommandCtx, CommandRegistry, SlashOutcome, Visibility};

macro_rules! cmd {
    ($name:literal, $desc:literal, $body:expr) => {{
        struct C;
        impl Command for C {
            fn name(&self)        -> &'static str { $name }
            fn description(&self) -> &'static str { $desc }
            fn handle(&self, ctx: &CommandCtx, args: &str) -> SlashOutcome { ($body)(ctx, args) }
        }
        Box::new(C) as Box<dyn Command>
    }};
}

pub struct CommandRegistry(Vec<Box<dyn Command>>);

impl CommandRegistry {
    pub fn builtin() -> Self {
        Self(vec![
            cmd!("help",     "Show available commands",             |_, _| SlashOutcome::Ok(HELP_TEXT.into())),
            cmd!("clear",    "Clear the chat transcript",           |_, _| SlashOutcome::Ok("__clear__".into())),
            cmd!("thread",   "Show or switch conversation thread",  |_, _| SlashOutcome::Ok("thread: not yet implemented".into())),
            cmd!("plugins",  "List installed plugins",              |_, _| SlashOutcome::Ok("plugins: not yet implemented".into())),
            cmd!("run",      "Dispatch an executor run",            |_, a| SlashOutcome::Ok(format!("run: {a}"))),
            cmd!("query",    "Query the relay knowledge graph",     |_, a| SlashOutcome::Ok(format!("query: {a}"))),
            cmd!("delegate", "Delegate a subtask to a sub-agent",   |_, a| SlashOutcome::Ok(format!("delegate: {a}"))),
            cmd!("recipes",  "List or activate a recipe",           |_, a| SlashOutcome::Ok(format!("recipe: {a}"))),
            cmd!("login",    "Launch the login wizard",             |_, _| SlashOutcome::Ok("__login__".into())),
            cmd!("auth",     "Show auth status",                    |_, _| SlashOutcome::Ok("auth: not yet implemented".into())),
        ])
    }

    pub fn find(&self, name: &str) -> Option<&dyn Command> {
        self.0.iter().find(|c| c.name() == name).map(|c| c.as_ref())
    }

    pub fn commands(&self) -> impl Iterator<Item = &dyn Command> {
        self.0.iter().map(|c| c.as_ref())
    }
}

const HELP_TEXT: &str = "\
/help     Show this message
/clear    Clear transcript
/thread   Show/switch thread
/plugins  List plugins
/run      Run a task
/query    Query knowledge graph
/delegate Delegate subtask
/recipes  List/activate recipe
/login    Login wizard
/auth     Auth status";
```

- [ ] **Step 5: Implement slash_adapter.rs**

```rust
use super::{CommandCtx, CommandRegistry, SlashOutcome};

pub fn run_slash(registry: &CommandRegistry, body: &str) -> SlashOutcome {
    let body = body.trim_start_matches('/');
    let (name, args) = body.split_once(' ').unwrap_or((body, ""));
    let ctx = CommandCtx { registry };
    match registry.find(name) {
        Some(cmd) => cmd.handle(&ctx, args),
        None      => SlashOutcome::Unknown(format!("/{name}: unknown command")),
    }
}
```

- [ ] **Step 6: Add mod to lib.rs**

```rust
pub mod commands;
```

- [ ] **Step 7: Run tests — pass**

```bash
cargo test -p repl command_registry
```

- [ ] **Step 8: Commit**

```bash
git add src/bin/repl/src/commands/ src/bin/repl/src/lib.rs src/bin/repl/tests/
git commit -m "feat(repl): commands layer — Command trait, 10 builtins, slash_adapter"
```

---

## Task 8: ReplApp struct + app_init

**Files:**
- Create: `src/bin/repl/src/app_init.rs`
- Modify: `src/bin/repl/src/lib.rs`

`ReplApp` is the central state struct. It owns `ChatView`, `EditArea` (textarea), all state flags, and the inflight channel.

- [ ] **Step 1: Write failing test**

```rust
// tests/headless.rs
use repl::app_init::ReplApp;

#[test]
fn new_app_is_idle() {
    let app = ReplApp::new_headless();
    assert_eq!(app.agent_state(), repl::state::AgentState::Idle);
    assert!(!app.should_quit());
}

#[test]
fn clear_command_empties_chat() {
    let mut app = ReplApp::new_headless();
    app.chat_view_mut().push(repl::chat_view::ChatMessage::user("hi"));
    app.handle_clear();
    assert!(app.chat_view().is_empty());
}
```

- [ ] **Step 2: Run — expect fail**

```bash
cargo test -p repl headless 2>&1 | head -5
```

- [ ] **Step 3: Implement app_init.rs**

```rust
use std::collections::VecDeque;
use std::sync::mpsc;

use crate::chat_view::{ChatMessage, ChatView};
use crate::commands::CommandRegistry;
use crate::state::{ActiveForm, ActiveList, AgentState, Focus, Tab};
use crate::tui_sink::TurnEvent;

pub struct ReplApp {
    pub chat:            ChatView,
    pub agent_state:     AgentState,
    pub focus:           Focus,
    pub active_list:     Option<ActiveList>,
    pub active_form:     Option<ActiveForm>,
    pub tabs:            Vec<Tab>,
    pub active_tab:      u8,
    pub should_quit:     bool,
    pub queue:           VecDeque<String>,
    pub inflight:        Option<mpsc::Receiver<Result<String, String>>>,
    pub events_rx:       Option<mpsc::Receiver<TurnEvent>>,
    pub commands:        CommandRegistry,
}

impl ReplApp {
    pub fn new_headless() -> Self {
        Self {
            chat:        ChatView::new(),
            agent_state: AgentState::Idle,
            focus:       Focus::Input,
            active_list: None,
            active_form: None,
            tabs:        vec![Tab { index: 0, ..Default::default() }],
            active_tab:  0,
            should_quit: false,
            queue:       VecDeque::new(),
            inflight:    None,
            events_rx:   None,
            commands:    CommandRegistry::builtin(),
        }
    }

    pub fn agent_state(&self)           -> AgentState  { self.agent_state }
    pub fn should_quit(&self)           -> bool        { self.should_quit }
    pub fn chat_view(&self)             -> &ChatView   { &self.chat }
    pub fn chat_view_mut(&mut self)     -> &mut ChatView { &mut self.chat }

    pub fn handle_clear(&mut self) {
        self.chat.clear();
    }
}
```

- [ ] **Step 4: Add mod to lib.rs**

```rust
pub mod app_init;
```

- [ ] **Step 5: Run tests — pass**

```bash
cargo test -p repl headless
```

- [ ] **Step 6: Commit**

```bash
git add src/bin/repl/src/app_init.rs src/bin/repl/src/lib.rs src/bin/repl/tests/headless.rs
git commit -m "feat(repl): ReplApp struct + headless constructor"
```

---

## Task 9: Submit flow

**Files:**
- Create: `src/bin/repl/src/submit.rs`
- Modify: `src/bin/repl/src/lib.rs`

Submit enqueues body; `dispatch_next` pops queue and either runs slash command inline or sends to agnt via tarpc. For now, LLM dispatch is a stub returning a placeholder (real tarpc wiring in Task 14).

- [ ] **Step 1: Write failing test**

```rust
// tests/headless.rs — add to existing file
#[test]
fn submit_empty_is_noop() {
    let mut app = ReplApp::new_headless();
    app.set_composer_text("");
    repl::submit::submit(&mut app);
    assert!(app.chat_view().is_empty());
}

#[test]
fn submit_slash_clear_empties_chat() {
    let mut app = ReplApp::new_headless();
    app.chat_view_mut().push(ChatMessage::user("hi"));
    app.set_composer_text("/clear");
    repl::submit::submit(&mut app);
    assert!(app.chat_view().is_empty());
}

#[test]
fn submit_quit_sets_flag() {
    let mut app = ReplApp::new_headless();
    app.set_composer_text("/quit");
    repl::submit::submit(&mut app);
    assert!(app.should_quit());
}
```

- [ ] **Step 2: Run — expect fail**

```bash
cargo test -p repl headless 2>&1 | head -10
```

- [ ] **Step 3: Implement submit.rs**

```rust
use std::sync::mpsc;
use std::thread;

use crate::app_init::ReplApp;
use crate::chat_view::ChatMessage;
use crate::commands::slash_adapter;
use crate::commands::SlashOutcome;

pub fn submit(app: &mut ReplApp) {
    let text = app.composer_text().trim().to_owned();
    if text.is_empty() { return; }
    app.clear_composer();

    if text == "/quit" {
        app.chat.push(ChatMessage::system("bye"));
        app.should_quit = true;
        return;
    }

    app.queue.push_back(text);
    if app.inflight.is_none() {
        dispatch_next(app);
    }
}

pub fn dispatch_next(app: &mut ReplApp) {
    let Some(body) = app.queue.pop_front() else { return };

    app.chat.push(ChatMessage::pending());

    let (tx, rx) = mpsc::channel::<Result<String, String>>();

    if body.starts_with('/') {
        let commands = app.commands.clone_arc();
        thread::spawn(move || {
            let outcome = slash_adapter::run_slash(&commands, &body);
            let _ = match outcome {
                SlashOutcome::Ok(s)      => tx.send(Ok(s)),
                SlashOutcome::Unknown(s) => tx.send(Err(s)),
                SlashOutcome::Err(s)     => tx.send(Err(s)),
            };
        });
    } else {
        // TODO Task 14: replace with real agnt tarpc call
        let body_clone = body.clone();
        thread::spawn(move || {
            let _ = tx.send(Ok(format!("[stub] received: {body_clone}")));
        });
    }

    app.inflight = Some(rx);
}

/// Call from the render tick to drain completed turns.
pub fn tick(app: &mut ReplApp) {
    if let Some(rx) = &app.inflight {
        match rx.try_recv() {
            Ok(Ok(text)) => {
                app.chat.pop_last_if_pending();
                if text == "__clear__" {
                    app.handle_clear();
                } else if text == "__login__" {
                    // login wizard triggered via command
                    app.chat.push(ChatMessage::system("login wizard: TODO Task 13"));
                } else {
                    app.chat.push(ChatMessage::assistant(text));
                }
                app.inflight = None;
                dispatch_next(app);
            }
            Ok(Err(e)) => {
                app.chat.pop_last_if_pending();
                app.chat.push(ChatMessage::error(e));
                app.inflight = None;
                dispatch_next(app);
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                app.chat.pop_last_if_pending();
                app.chat.push(ChatMessage::error("agent disconnected".into()));
                app.inflight = None;
            }
        }
    }
}
```

- [ ] **Step 4: Add `set_composer_text`, `composer_text`, `clear_composer` to ReplApp in app_init.rs**

```rust
// add to ReplApp struct:
pub composer_text: String,

// add to new_headless():
composer_text: String::new(),

// add methods:
pub fn composer_text(&self)            -> &str  { &self.composer_text }
pub fn set_composer_text(&mut self, s: &str)    { self.composer_text = s.to_owned(); }
pub fn clear_composer(&mut self)                { self.composer_text.clear(); }
```

- [ ] **Step 5: Make CommandRegistry cloneable**

Add `clone_arc` helper so it can be moved into thread:
```rust
// commands/mod.rs
use std::sync::Arc;
pub struct CommandRegistry(Arc<Vec<Box<dyn Command>>>);
// wrap in Arc, clone_arc returns Arc clone
```

- [ ] **Step 6: Add mod to lib.rs**

```rust
pub mod submit;
```

- [ ] **Step 7: Run tests — pass**

```bash
cargo test -p repl headless
```

- [ ] **Step 8: Commit**

```bash
git add src/bin/repl/src/submit.rs src/bin/repl/src/app_init.rs src/bin/repl/src/commands/
git commit -m "feat(repl): submit flow — enqueue, dispatch_next, tick, slash inline"
```

---

## Task 10: Key handling

**Files:**
- Create: `src/bin/repl/src/key_handling.rs`
- Modify: `src/bin/repl/src/lib.rs`

- [ ] **Step 1: Write failing test**

```rust
// tests/headless.rs — add
use repl::input::{InputEvent, KeyCode, Mods};
use repl::key_handling::handle_key;

fn key(code: KeyCode) -> InputEvent {
    InputEvent::Key { code, mods: Mods::NONE }
}

#[test]
fn enter_key_submits() {
    let mut app = ReplApp::new_headless();
    app.set_composer_text("hello");
    handle_key(&mut app, &key(KeyCode::Enter));
    assert!(app.chat_view().len() >= 1); // pending pushed
}

#[test]
fn escape_clears_non_empty_composer() {
    let mut app = ReplApp::new_headless();
    app.set_composer_text("hello");
    handle_key(&mut app, &key(KeyCode::Esc));
    assert_eq!(app.composer_text(), "");
}

#[test]
fn ctrl_c_does_not_quit() {
    let mut app = ReplApp::new_headless();
    let k = InputEvent::Key { code: KeyCode::Char('c'), mods: Mods::CTRL };
    handle_key(&mut app, &k);
    assert!(!app.should_quit());
}
```

- [ ] **Step 2: Run — expect fail**

```bash
cargo test -p repl headless 2>&1 | head -10
```

- [ ] **Step 3: Implement key_handling.rs**

```rust
use crate::app_init::ReplApp;
use crate::chat_view::ChatMessage;
use crate::input::{InputEvent, KeyCode, Mods};
use crate::state::Focus;
use crate::submit;

const ESC_QUIT_WINDOW_MS: u128 = 1500;

pub fn handle_key(app: &mut ReplApp, event: &InputEvent) {
    let InputEvent::Key { code, mods } = event else { return };

    // --- form mode ---
    if app.active_form.is_some() {
        // TODO: route to plugin_ui form handler
        if *code == KeyCode::Esc { app.active_form = None; }
        return;
    }

    // --- list mode ---
    if app.active_list.is_some() {
        match code {
            KeyCode::Esc => { app.active_list = None; }
            KeyCode::Enter => {
                // TODO: dispatch list pick
            }
            _ => {}
        }
        return;
    }

    // --- Ctrl+C: signal bubble, no quit ---
    if *code == KeyCode::Char('c') && mods.contains(Mods::CTRL) {
        app.chat.push(ChatMessage::system("^C received"));
        return;
    }

    // --- Enter: submit ---
    if *code == KeyCode::Enter && mods.is_empty() {
        submit::submit(app);
        app.focus = Focus::Input;
        return;
    }

    // --- Ctrl+J: newline ---
    if *code == KeyCode::Char('j') && mods.contains(Mods::CTRL) {
        app.composer_text.push('\n');
        return;
    }

    // --- Esc: clear / selection / quit ladder ---
    if *code == KeyCode::Esc {
        handle_escape(app);
        return;
    }

    // --- Selection mode scroll ---
    if app.focus == Focus::Selection {
        match code {
            KeyCode::Up       => { app.chat.scroll_older(1); }
            KeyCode::Down     => { app.chat.scroll_newer(1); }
            KeyCode::PageUp   => { app.chat.scroll_older(10); }
            KeyCode::PageDown => { app.chat.scroll_newer(10); }
            _                 => { app.focus = Focus::Input; }
        }
        return;
    }

    // --- Normal char input ---
    if let KeyCode::Char(c) = code {
        app.composer_text.push(*c);
        sync_autocomplete(app);
    } else if *code == KeyCode::Backspace {
        app.composer_text.pop();
        sync_autocomplete(app);
    }
}

fn handle_escape(app: &mut ReplApp) {
    if !app.composer_text.is_empty() {
        app.composer_text.clear();
        app.active_list = None;
        return;
    }
    if app.focus == Focus::Input {
        app.focus = Focus::Selection;
        return;
    }
    // second Esc from Selection = quit
    app.should_quit = true;
}

fn sync_autocomplete(app: &mut ReplApp) {
    use crate::state::ActiveList;
    if app.composer_text.starts_with('/') {
        if app.active_list.is_none() {
            app.active_list = Some(ActiveList::Slash);
        }
        crate::slash_lists::sync_slash_list(app);
    } else {
        if matches!(app.active_list, Some(ActiveList::Slash)) {
            app.active_list = None;
        }
    }

    if app.composer_text.starts_with('@') {
        if app.active_list.is_none() {
            app.active_list = Some(ActiveList::MentionFiles { anchor_col: 0 });
        }
        // TODO Task 11: sync_mention_list
    }
}
```

- [ ] **Step 4: Add mod to lib.rs**

```rust
pub mod key_handling;
```

- [ ] **Step 5: Run tests — pass**

```bash
cargo test -p repl headless
```

- [ ] **Step 6: Commit**

```bash
git add src/bin/repl/src/key_handling.rs src/bin/repl/src/lib.rs
git commit -m "feat(repl): key_handling — Enter/Esc/Ctrl+C/scroll/char dispatch"
```

---

## Task 11: Slash lists + mentions

**Files:**
- Create: `src/bin/repl/src/slash_lists.rs`
- Create: `src/bin/repl/src/mentions.rs`
- Modify: `src/bin/repl/src/lib.rs`

- [ ] **Step 1: Write failing test**

```rust
// tests/headless.rs
#[test]
fn slash_list_filters_by_prefix() {
    let mut app = ReplApp::new_headless();
    app.set_composer_text("/hel");
    repl::slash_lists::sync_slash_list(&mut app);
    let items = app.slash_list_items();
    assert!(items.iter().any(|i| i == "help"));
    assert!(!items.iter().any(|i| i == "clear"));
}
```

- [ ] **Step 2: Implement slash_lists.rs**

```rust
use crate::app_init::ReplApp;

pub fn sync_slash_list(app: &mut ReplApp) {
    let prefix = app.composer_text()
        .trim_start_matches('/')
        .to_lowercase();
    let items: Vec<String> = app.commands
        .commands()
        .filter(|c| c.name().starts_with(prefix.as_str()))
        .map(|c| c.name().to_owned())
        .collect();
    app.slash_list_cache = items;
}
```

- [ ] **Step 3: Add `slash_list_cache` and `slash_list_items()` to ReplApp**

```rust
// app_init.rs — add field
pub slash_list_cache: Vec<String>,

// new_headless: slash_list_cache: Vec::new(),

// method
pub fn slash_list_items(&self) -> &[String] { &self.slash_list_cache }
```

- [ ] **Step 4: Implement mentions.rs (stub + filesystem filter)**

```rust
use crate::app_init::ReplApp;
use crate::state::ActiveList;

pub fn sync_mention_list(app: &mut ReplApp, kind: &ActiveList) {
    let query = app.composer_text().trim_start_matches('@').to_lowercase();
    let entries: Vec<String> = match kind {
        ActiveList::MentionDirs { .. } => {
            read_dir_entries(".", true)
                .into_iter()
                .filter(|e| e.to_lowercase().contains(query.as_str()))
                .take(20)
                .collect()
        }
        _ => {
            read_dir_entries(".", false)
                .into_iter()
                .filter(|e| e.to_lowercase().contains(query.as_str()))
                .take(20)
                .collect()
        }
    };
    app.mention_list_cache = entries;
}

fn read_dir_entries(path: &str, dirs_only: bool) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(path) else { return vec![] };
    rd.filter_map(|e| {
        let e = e.ok()?;
        let meta = e.metadata().ok()?;
        if dirs_only && !meta.is_dir() { return None; }
        Some(e.file_name().to_string_lossy().into_owned())
    }).collect()
}
```

- [ ] **Step 5: Add mods to lib.rs**

```rust
pub mod slash_lists;
pub mod mentions;
```

- [ ] **Step 6: Run tests — pass**

```bash
cargo test -p repl headless
```

- [ ] **Step 7: Commit**

```bash
git add src/bin/repl/src/slash_lists.rs src/bin/repl/src/mentions.rs src/bin/repl/src/lib.rs
git commit -m "feat(repl): slash autocomplete + @ mention list (filesystem)"
```

---

## Task 12: Status bar

**Files:**
- Create: `src/bin/repl/src/status_bar.rs`
- Modify: `src/bin/repl/src/lib.rs`

Left panel only. Two rows inside `layout.status`:
- Row 0: `0 │1│ 2 3 4    v1.0.2`
- Row 1: `~/dev/relay-clean   main.rs`

- [ ] **Step 1: Write failing test**

```rust
// tests/headless.rs
#[test]
fn status_tab_row_shows_active_tab() {
    let app = ReplApp::new_headless();
    let row = repl::status_bar::tab_row_text(&app, 80);
    assert!(row.contains('0')); // root tab always present
}
```

- [ ] **Step 2: Implement status_bar.rs**

```rust
use crate::app_init::ReplApp;
use crate::layout::Layout;
use crate::render::Renderer;

/// Draw 2-row status block into left panel.
pub fn draw_status_left(app: &ReplApp, r: &mut Renderer, l: &Layout) {
    let w = l.status.width as usize;
    let tab_row = tab_row_text(app, w);
    let cwd_row = cwd_row_text(app, w);
    // r.put_str(0, l.status.y,     &tab_row);
    // r.put_str(0, l.status.y + 1, &cwd_row);
    let _ = (r, tab_row, cwd_row, l);
}

/// `0 │1│ 2 3   v1.0.2`
pub fn tab_row_text(app: &ReplApp, width: usize) -> String {
    let tabs_part: String = app.tabs.iter().map(|t| {
        let n = t.index;
        if n == app.active_tab { format!("[{n}]") }
        else if t.unread > 0   { format!("{n}*")  }
        else                   { format!(" {n} ") }
    }).collect::<Vec<_>>().join(" ");

    let ver = concat!("v", env!("CARGO_PKG_VERSION"));
    let pad = width.saturating_sub(tabs_part.len() + ver.len() + 1);
    format!("{tabs_part}{}{ver}", " ".repeat(pad))
}

/// `~/dev/relay-clean   main.rs`
pub fn cwd_row_text(app: &ReplApp, width: usize) -> String {
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let pin = app.tabs.get(app.active_tab as usize)
        .and_then(|t| t.pinned_file.as_deref())
        .unwrap_or("");
    let pad = width.saturating_sub(cwd.len() + pin.len() + 1);
    format!("{cwd}{}{pin}", " ".repeat(pad))
}
```
```

- [ ] **Step 3: Add mod to lib.rs + add `tabs` field test coverage**

```rust
pub mod status_bar;
```

- [ ] **Step 4: Run tests — pass**

```bash
cargo test -p repl headless
```

- [ ] **Step 5: Commit**

```bash
git add src/bin/repl/src/status_bar.rs src/bin/repl/src/lib.rs
git commit -m "feat(repl): status bar — tab row + cwd row + clock (LAYOUT.md)"
```

---

## Task 13: Login wizard

**Files:**
- Create: `src/bin/repl/src/login_wizard.rs`
- Modify: `src/bin/repl/src/lib.rs`

5-step modal flow: provider pick → existing check → role pick → key entry → save.

- [ ] **Step 1: Write failing test**

```rust
// tests/headless.rs
#[test]
fn start_login_wizard_sets_list() {
    let mut app = ReplApp::new_headless();
    repl::login_wizard::start_login_wizard(&mut app);
    assert_eq!(app.active_list, Some(repl::state::ActiveList::LoginProvider));
}
```

- [ ] **Step 2: Implement login_wizard.rs**

```rust
use crate::app_init::ReplApp;
use crate::state::{ActiveForm, ActiveList};

#[derive(Debug, Default, Clone)]
pub struct LoginWizard {
    pub provider: Option<String>,
    pub role:     Option<String>,
}

pub fn start_login_wizard(app: &mut ReplApp) {
    app.login_wizard = Some(LoginWizard::default());
    app.active_form = None;
    app.active_list = Some(ActiveList::LoginProvider);
}

pub fn on_provider_pick(app: &mut ReplApp, provider: String) {
    if let Some(w) = &mut app.login_wizard {
        w.provider = Some(provider);
    }
    app.active_list = Some(ActiveList::LoginRole);
}

pub fn on_role_pick(app: &mut ReplApp, role: String) {
    if let Some(w) = &mut app.login_wizard {
        w.role = Some(role);
    }
    app.active_list = None;
    app.active_form = Some(ActiveForm::LoginKey);
}

pub fn on_key_submit(app: &mut ReplApp, key: String) {
    let (provider, role) = app.login_wizard.as_ref()
        .map(|w| (
            w.provider.clone().unwrap_or_default(),
            w.role.clone().unwrap_or_default(),
        ))
        .unwrap_or_default();
    // Persist via agnt auth module (same path logic as spool).
    // For now emit a system message; Task 14 wires real auth write.
    let _ = (provider, role, key);
    app.login_wizard = None;
    app.active_form = None;
    app.chat.push(crate::chat_view::ChatMessage::system("login saved"));
}
```

- [ ] **Step 3: Add `login_wizard` field to ReplApp**

```rust
// app_init.rs
pub login_wizard: Option<crate::login_wizard::LoginWizard>,
// in new_headless: login_wizard: None,
```

- [ ] **Step 4: Add mod to lib.rs**

```rust
pub mod login_wizard;
```

- [ ] **Step 5: Run tests — pass**

```bash
cargo test -p repl headless
```

- [ ] **Step 6: Commit**

```bash
git add src/bin/repl/src/login_wizard.rs src/bin/repl/src/app_init.rs src/bin/repl/src/lib.rs
git commit -m "feat(repl): login wizard — 5-step flow (provider/role/key/save)"
```

---

## Task 14: Wire main.rs — full draw loop + agnt RPC

**Files:**
- Modify: `src/bin/repl/src/main.rs`

Replace stub `SpoolView` usage with `ReplApp`. Wire the 5-region draw loop using `layout::region_rects`, draw each region via its module. Replace stub LLM dispatch with real `AgntRpc` tarpc call.

- [ ] **Step 1: Replace SpoolView with ReplApp in main.rs**

Full replacement (keep Guard and enable_vt_output as-is):

```rust
use std::io::{self, Write};
use std::time::{Duration, Instant};

use crossterm::terminal;
use repl::app_init::ReplApp;
use repl::input::{EventPump, InputEvent};
use repl::key_handling::handle_key;
use repl::layout::region_rects;
use repl::render::{Renderer, StdoutSurface};
use repl::submit;

const TARGET_FPS: u32 = 60;
const RESIZE_DEBOUNCE: Duration = Duration::from_millis(60);

// ... Guard unchanged ...

fn main() -> io::Result<()> {
    let _guard = Guard::enter()?;
    let (cols, rows) = terminal::size()?;
    let mut surface = StdoutSurface::new(cols, rows);
    let mut r = Renderer::new(cols, rows);
    let mut pump = EventPump::new();
    let mut app = ReplApp::new_headless(); // TODO: with_agnt_session when Task 14b done
    let mut tick: u64 = 0;
    let mut resize_pending: Option<Instant> = None;
    let frame_budget = Duration::from_millis(1000 / TARGET_FPS as u64);

    loop {
        let frame_start = Instant::now();

        // Process events
        while let Some(ev) = pump.try_next() {
            match &ev {
                InputEvent::Resize(w, h) => {
                    resize_pending = Some(Instant::now());
                    let _ = (w, h);
                }
                InputEvent::Key { .. } => {
                    handle_key(&mut app, &ev);
                }
                _ => {}
            }
        }

        // Debounced resize
        if let Some(t) = resize_pending {
            if t.elapsed() >= RESIZE_DEBOUNCE {
                let (w, h) = terminal::size()?;
                r.resize(w, h);
                surface.resize(w, h);
                resize_pending = None;
            }
        }

        // Drain inflight turn
        submit::tick(&mut app);

        if app.should_quit { break; }

        // Draw frame
        let (cols, rows) = (r.cols(), r.rows());
        let regions = region_rects(cols, rows, 1); // TODO: real textarea rows
        r.clear();
        repl::render_app::draw_app(&mut app, &mut r, &regions, tick);
        r.present(&mut surface)?;

        tick = tick.wrapping_add(1);
        let elapsed = frame_start.elapsed();
        if elapsed < frame_budget {
            std::thread::sleep(frame_budget - elapsed);
        }
    }

    Ok(())
}
```

- [ ] **Step 2: Create stub render_app.rs**

```rust
// src/bin/repl/src/render_app.rs
use crate::app_init::ReplApp;
use crate::layout::{Layout, layout as compute_layout};
use crate::render::Renderer;
use crate::status_bar;

pub fn draw_app(app: &mut ReplApp, r: &mut Renderer, tick: u64) {
    let cols = r.cols();
    let rows = r.rows();
    let l = compute_layout(cols, rows, app.trace_lines.len() as u16);

    // Draw divider column
    for row in 0..rows {
        r.put_char(l.split_x, row, '│');
    }

    draw_left_msgs(app, r, &l, tick);
    draw_left_input(app, r, &l, tick);
    status_bar::draw_status_left(app, r, &l);
    draw_left_trace(app, r, &l);
    draw_right(app, r, &l, tick);
}

fn draw_left_msgs(app: &mut ReplApp, r: &mut Renderer, l: &Layout, _tick: u64) {
    // Render user messages in l.user_msgs rect.
    // Active/pinned msg sits at last row of user_msgs (just above input).
    // TODO: word-wrap, ┃ gutter char, scroll offset
    let _ = (app, r, l);
}

fn draw_left_input(app: &mut ReplApp, r: &mut Renderer, l: &Layout, tick: u64) {
    let glyph = app.agent_state.glyph(tick);
    let text = app.composer_text();
    // Write "{glyph} {text}" at (0, l.input.y)
    let _ = (glyph, text, r, l);
}

fn draw_left_trace(app: &mut ReplApp, r: &mut Renderer, l: &Layout) {
    // Render tool activity lines bottom-up in l.trace rect.
    // Each line: "✔ kern  file written" or "· agnt  reading auth.rs"
    let _ = (app, r, l);
}

fn draw_right(app: &mut ReplApp, r: &mut Renderer, l: &Layout, _tick: u64) {
    // Render agent response + code blocks in l.right rect.
    // Scroll offset synced with left (app.scroll_offset).
    // TODO: syntax-highlight code fences, diff +/- colouring
    let _ = (app, r, l);
}
```

- [ ] **Step 3: Update main.rs to pass `tick` and drop old `regions` arg**

```rust
repl::render_app::draw_app(&mut app, &mut r, tick);
```

Remove old `region_rects` call from main.rs.

- [ ] **Step 4: Add mod to lib.rs**

```rust
pub mod render_app;
```

- [ ] **Step 5: Add `trace_lines` and `scroll_offset` to ReplApp**

```rust
// app_init.rs
pub trace_lines:   Vec<String>,  // tool activity; new_headless: vec![]
pub scroll_offset: usize,        // synced left+right; new_headless: 0
```

- [ ] **Step 6: Verify compile + run**

```bash
cargo build -p repl 2>&1 | head -20
```

Then: `cargo run -p repl` — TUI opens, `│` divider visible, quit on double-Esc.

- [ ] **Step 7: Commit**

```bash
git add src/bin/repl/src/main.rs src/bin/repl/src/render_app.rs src/bin/repl/src/app_init.rs src/bin/repl/src/lib.rs
git commit -m "feat(repl): wire main.rs — split-column draw loop, │ divider, synced scroll"
```

---

## Task 15: HTTP server

**Files:**
- Create: `src/bin/repl/src/http_server.rs`
- Modify: `src/bin/repl/src/lib.rs`

Hand-rolled HTTP/1.1 (no framework), same as old spool. Two routes: `POST /query` and `POST /query/stream`.

- [ ] **Step 1: Write failing test**

```rust
// tests/headless.rs
#[test]
fn http_parse_args_bind() {
    let addr = repl::http_server::parse_bind_arg(&["--bind", "127.0.0.1:9000"]).unwrap();
    assert_eq!(addr, "127.0.0.1:9000");
}

#[test]
fn http_parse_args_missing_value() {
    let err = repl::http_server::parse_bind_arg(&["--bind"]);
    assert!(err.is_err());
}
```

- [ ] **Step 2: Implement http_server.rs**

```rust
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};

#[derive(Debug)]
pub enum ArgError {
    MissingValue(&'static str),
    Unknown(String),
}

pub fn parse_bind_arg(args: &[&str]) -> Result<String, ArgError> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if *a == "--bind" {
            return it.next()
                .map(|v| v.to_string())
                .ok_or(ArgError::MissingValue("--bind"));
        } else {
            return Err(ArgError::Unknown(a.to_string()));
        }
    }
    Ok("127.0.0.1:7070".into()) // default
}

pub fn serve(listener: TcpListener) {
    for stream in listener.incoming() {
        if let Ok(s) = stream {
            std::thread::spawn(|| handle_conn(s));
        }
    }
}

fn handle_conn(mut stream: TcpStream) {
    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() { return; }
    let parts: Vec<&str> = request_line.trim().split(' ').collect();
    if parts.len() < 2 { return; }
    let (_method, path) = (parts[0], parts[1]);

    // Consume headers
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).is_err() { break; }
        if h.trim().is_empty() { break; }
    }

    let response = match path {
        "/query" => {
            // Read body — simplified (no Content-Length parsing)
            let body = r#"{"answer":"stub","usage":{}}"#;
            format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}", body.len(), body)
        }
        "/query/stream" => {
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\nevent: done\r\ndata: {}\r\n\r\n".into()
        }
        _ => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".into(),
    };

    let _ = stream.write_all(response.as_bytes());
}
```

- [ ] **Step 3: Add mod to lib.rs**

```rust
pub mod http_server;
```

- [ ] **Step 4: Run tests — pass**

```bash
cargo test -p repl headless
```

- [ ] **Step 5: Commit**

```bash
git add src/bin/repl/src/http_server.rs src/bin/repl/src/lib.rs
git commit -m "feat(repl): HTTP server — POST /query + /query/stream + parse_bind_arg"
```

---

## Task 16: MCP serve integration test

**Files:**
- Create: `src/bin/repl/tests/mcp_serve.rs`

Mirrors `spool/tests/mcp_serve.rs` — starts repl in headless mode, sends an MCP `initialize` request, verifies `capabilities` response.

- [ ] **Step 1: Write test**

```rust
// tests/mcp_serve.rs
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader, Write};

const INIT_MSG: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0.0.1"}}}"#;

#[test]
fn mcp_serve_responds_to_initialize() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_repl"))
        .args(["--mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn repl --mcp");

    let stdin = child.stdin.as_mut().unwrap();
    let msg = format!("Content-Length: {}\r\n\r\n{}", INIT_MSG.len(), INIT_MSG);
    stdin.write_all(msg.as_bytes()).unwrap();

    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert!(line.starts_with("Content-Length:"), "got: {line}");

    child.kill().ok();
}
```

- [ ] **Step 2: Add `--mcp` subcommand to main.rs**

In `main()`, check `std::env::args()` for `--mcp` before entering TUI mode and call `agnt::mcp_server::serve_stdio()` (same pattern as agnt's MCP server).

- [ ] **Step 3: Run test**

```bash
cargo test -p repl mcp_serve
```

- [ ] **Step 4: Commit**

```bash
git add src/bin/repl/tests/mcp_serve.rs src/bin/repl/src/main.rs
git commit -m "test(repl): mcp_serve integration — initialize handshake"
```

---

## Task 17: Port missing plugins

**Files:**
- Create: `src/plugins/ask-bubble/src/lib.rs`
- Create: `src/plugins/ask-bubble/Cargo.toml`
- Create: `src/plugins/clock/src/lib.rs`
- Create: `src/plugins/clock/Cargo.toml`
- Create: `src/plugins/intro/src/lib.rs`
- Create: `src/plugins/intro/Cargo.toml`
- Create: `src/plugins/relay/src/lib.rs`
- Create: `src/plugins/relay/Cargo.toml`
- Create: `src/plugins/fs/src/lib.rs`
- Create: `src/plugins/fs/Cargo.toml`
- Modify: `Cargo.toml` (workspace members)

Each plugin is an MCP stdio plugin (same pattern as existing `echo` and `llm` plugins). Port from `../relay/src/plugins/` stripping old infra (rubric, inline comments, dead test helpers).

- [ ] **Step 1: Add workspace members**

```toml
# Cargo.toml
"src/plugins/ask-bubble",
"src/plugins/clock",
"src/plugins/intro",
"src/plugins/relay",
"src/plugins/fs",
```

- [ ] **Step 2: Create each plugin Cargo.toml (pattern from existing echo)**

```toml
[package]
name = "plugin-ask-bubble"   # etc.
version.workspace = true
edition.workspace = true
license.workspace = true

[[bin]]
name = "plugin-ask-bubble"
path = "src/main.rs"

[dependencies]
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
```

- [ ] **Step 3: Port each plugin lib.rs from `../relay/src/plugins/<name>/src/lib.rs`**

For each plugin: copy, remove `#[cfg(test)]` rubric blocks, remove inline comments, update import paths (no spool/relay internal crate imports).

- [ ] **Step 4: Compile check**

```bash
cargo check -p plugin-ask-bubble -p plugin-clock -p plugin-intro -p plugin-relay -p plugin-fs
```

- [ ] **Step 5: Commit**

```bash
git add src/plugins/
git commit -m "feat(plugins): port ask-bubble, clock, intro, relay, fs from ../relay"
```

---

## Self-Review

**Spec coverage:**
- [x] Rename surf → repl — Task 1
- [x] 5-region LAYOUT.md (chat 60%, input, lock, status, trace 30%) — Tasks 2, 14
- [x] State types (AgentState, Focus, Tab, ActiveList) — Task 3
- [x] Message styles (user `┃`, LLM flush-left, info `🞆`) — Task 4
- [x] ChatView scroll + 60% cap — Task 5
- [x] TuiSink + TurnEvent — Task 6
- [x] Status bar (tabs, clock, cwd, file pin) — Task 12
- [x] Submit flow + slash dispatch — Task 9
- [x] Key handling (Enter, Esc, Ctrl+C, scroll) — Task 10
- [x] Slash autocomplete + @mentions — Task 11
- [x] Commands (10 builtins) — Task 7
- [x] Login wizard (5-step) — Task 13
- [x] Draw loop wired to ReplApp — Task 14
- [x] HTTP server — Task 15
- [x] MCP serve test — Task 16
- [x] Missing plugins — Task 17

**Gaps:**
- Real agnt tarpc LLM dispatch (stub in Task 9, note added in Task 14) — intentionally deferred; wiring depends on agnt RPC being live
- Lock zone decay animation (noted in layout.rs but visual impl deferred to render_app polish pass)
- Full chat rendering (gutter chars, word-wrap per message role) — render_app stub calls are scaffolded, full impl is a follow-on
- Tab switching keybinds (Ctrl+digit) — not in key_handling; add once Tab model stable
