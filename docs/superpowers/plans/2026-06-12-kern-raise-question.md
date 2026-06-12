# Kern `raise_question` + Waiting-Sessions Overlay — Implementation Plan (Spec B)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.
>
> **Environment note:** subagent fan-out is unavailable here (mux delegation + built-in Agent tool are blocked), so execute **inline** via superpowers:executing-plans.

**Goal:** A blocking `raise_question` MCP tool that parks an agent until a human answers via a `Ctrl+K` overlay rostering all waiting agents, plus a status-bar pending badge.

**Architecture:** A `QuestionRegistry` inside `PaneRegistry` (reached by the tool handler through `Server.mux`, and by the TUI through its own registry lock). The handler registers a question + blocks on an `mpsc::Receiver`; the overlay delivers the answer via the matching `Sender`. No IPC — one process (Spec A). The wire path has **no read timeout** (verified), so the block is indefinite and safe.

**Tech Stack:** Rust, `std::sync::mpsc`, `crossterm`, `serde_json`. Builds on branch `feat/mux-unification`.

**Spec:** `docs/superpowers/specs/2026-06-12-kern-raise-question-design.md`

---

## File Map

| Action | Path | Responsibility |
|--------|------|----------------|
| Modify | `src/mux/registry.rs` | `PendingQuestion`, `QuestionRegistry` (open/list/answer/dismiss/len); add `questions` field to `PaneRegistry` |
| Modify | `src/config/mux.rs` | `key_questions` config + `KeyMap::questions` + `matches_questions` |
| Modify | `src/mcp/tools_mux.rs` | `raise_question` schema + `tool_raise_question` handler |
| Modify | `src/mcp.rs` | dispatch `"raise_question"` |
| Create | `src/mux/questions.rs` | `QuestionsOverlay` + `QuestionsAction` (new/draw/handle_key) |
| Modify | `src/mux/mod.rs` | `mod questions;` + re-export |
| Modify | `src/mux/tui.rs` | `Ctrl+K` toggle, overlay draw dispatch, status-bar `❓N` badge |

---

## Task 1: `QuestionRegistry` in `registry.rs`

**Files:** Modify `src/mux/registry.rs`

- [ ] **Step 1: Write failing tests**

Append to the `#[cfg(test)] mod tests` in `src/mux/registry.rs`:

```rust
#[test]
fn question_open_then_answer_delivers_text() {
    let mut q = QuestionRegistry::default();
    let (id, rx) = q.open("worker-1".into(), "ship it?".into());
    assert_eq!(q.len(), 1);
    assert!(q.answer(&id, "yes".into()), "answer should find the id");
    assert_eq!(rx.recv().unwrap(), "yes", "blocked caller receives the answer");
    assert_eq!(q.len(), 0, "answered question is removed");
}

#[test]
fn question_list_reports_label_and_text() {
    let mut q = QuestionRegistry::default();
    let _ = q.open("audit".into(), "delete shards?".into());
    let roster = q.list();
    assert_eq!(roster.len(), 1);
    assert_eq!(roster[0].1, "audit");
    assert_eq!(roster[0].2, "delete shards?");
}

#[test]
fn question_dismiss_drops_sender_so_recv_errors() {
    let mut q = QuestionRegistry::default();
    let (id, rx) = q.open(String::new(), "x?".into());
    assert!(q.dismiss(&id));
    assert!(rx.recv().is_err(), "dismiss drops the sender; recv errors");
    assert_eq!(q.len(), 0);
}

#[test]
fn question_answer_unknown_id_returns_false() {
    let mut q = QuestionRegistry::default();
    assert!(!q.answer("nope", "x".into()));
}
```

- [ ] **Step 2: Run — verify fail**

```
cargo test -p kern mux::registry::tests::question 2>&1 | tail -8
```
Expected: compile error — `QuestionRegistry` undefined.

- [ ] **Step 3: Implement**

Add near the top of `src/mux/registry.rs` (after the `use` lines):

```rust
use crate::mux::pty::new_session_id;

/// One in-flight human question raised by an agent via `raise_question`.
pub struct PendingQuestion {
    pub id:       String,
    pub label:    String,
    pub question: String,
    answer_tx:    std::sync::mpsc::Sender<String>,
}

/// In-memory roster of questions awaiting a human answer. Lives inside
/// [`PaneRegistry`]; the `raise_question` tool handler registers + blocks,
/// the TUI overlay lists + answers. Never persisted.
#[derive(Default)]
pub struct QuestionRegistry {
    pending: Vec<PendingQuestion>,
}

impl QuestionRegistry {
    /// Register a question and return `(id, receiver)`. The caller blocks on
    /// the receiver AFTER releasing the registry lock.
    pub fn open(&mut self, label: String, question: String) -> (String, std::sync::mpsc::Receiver<String>) {
        let id = new_session_id();
        let (tx, rx) = std::sync::mpsc::channel();
        self.pending.push(PendingQuestion { id: id.clone(), label, question, answer_tx: tx });
        (id, rx)
    }

    /// Roster view: `(id, label, question)` per pending entry, insertion order.
    pub fn list(&self) -> Vec<(String, String, String)> {
        self.pending.iter().map(|p| (p.id.clone(), p.label.clone(), p.question.clone())).collect()
    }

    /// Deliver `answer` to question `id`; remove it. False if `id` is unknown.
    pub fn answer(&mut self, id: &str, answer: String) -> bool {
        let Some(pos) = self.pending.iter().position(|p| p.id == id) else { return false };
        let p = self.pending.remove(pos);
        let _ = p.answer_tx.send(answer); // recv side may have hung up; ignore.
        true
    }

    /// Drop question `id` without answering (its sender drops → caller's recv errors).
    pub fn dismiss(&mut self, id: &str) -> bool {
        let Some(pos) = self.pending.iter().position(|p| p.id == id) else { return false };
        self.pending.remove(pos);
        true
    }

    pub fn len(&self) -> usize { self.pending.len() }
    pub fn is_empty(&self) -> bool { self.pending.is_empty() }
}
```

- [ ] **Step 4: Add `questions` field to `PaneRegistry`**

In the `PaneRegistry` struct add:
```rust
    pub questions: QuestionRegistry,
```
In `PaneRegistry::new(...)`, add `questions: QuestionRegistry::default(),` to the returned `Self { ... }`.

- [ ] **Step 5: Run — verify pass**

```
cargo test -p kern mux::registry 2>&1 | tail -8
```
Expected: all pass.

- [ ] **Step 6: Commit**

```
git add src/mux/registry.rs
git commit -m "feat(mux): QuestionRegistry inside PaneRegistry (open/list/answer/dismiss)"
```

---

## Task 2: `key_questions` config + `matches_questions`

**Files:** Modify `src/config/mux.rs`

- [ ] **Step 1: Write failing tests**

Append to `#[cfg(test)] mod tests` in `src/config/mux.rs`:

```rust
#[test]
fn mux_config_key_questions_default() {
    assert_eq!(MuxConfig::default().key_questions, "ctrl+k");
}

#[test]
fn keymap_matches_questions_ctrl_k() {
    let km = KeyMap::from_config(&MuxConfig::default());
    assert!(km.matches_questions(&KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL)));
}
```

- [ ] **Step 2: Run — verify fail**

```
cargo test -p kern config::mux 2>&1 | tail -6
```
Expected: compile error — `key_questions` / `matches_questions` undefined.

- [ ] **Step 3: Implement**

In `MuxConfig` add after `key_research`:
```rust
    /// Key binding to toggle the waiting-questions overlay. Default `ctrl+k`.
    pub key_questions: String,
```
In `MuxConfig::default()` after `key_research`:
```rust
            key_questions:  "ctrl+k".into(),
```
In `KeyMap` add after `research`:
```rust
    pub questions: KeyEvent,
```
In `KeyMap::from_config` after `research`:
```rust
            questions: parse_key_event(&cfg.key_questions)
                .unwrap_or_else(|| KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL)),
```
Add the method after `matches_research`:
```rust
    pub fn matches_questions(&self, ev: &KeyEvent) -> bool { self.questions == *ev }
```

> **Engineer note:** confirm the exact `KeyMap` field list + `from_config` shape by reading `src/config/mux.rs` first — mirror the `research` field exactly (it was added the same way).

- [ ] **Step 4: Run — verify pass**

```
cargo test -p kern config::mux 2>&1 | tail -8
```

- [ ] **Step 5: Commit**

```
git add src/config/mux.rs
git commit -m "feat(mux): key_questions config + KeyMap::matches_questions (ctrl+k)"
```

---

## Task 3: `raise_question` tool

**Files:** Modify `src/mcp/tools_mux.rs`, `src/mcp.rs`

- [ ] **Step 1: Write failing schema test**

Append to `#[cfg(test)] mod tests` in `src/mcp/tools_mux.rs`:

```rust
#[test]
fn raise_question_schema_present_and_requires_question() {
    let defs = tool_schemas();
    let d = defs.iter().find(|d| d["name"] == "raise_question").expect("raise_question present");
    let req: Vec<&str> = d["inputSchema"]["required"].as_array().unwrap()
        .iter().filter_map(|v| v.as_str()).collect();
    assert!(req.contains(&"question"), "must require question, got {req:?}");
}
```
Also update `schema_names_are_kern_native` to expect the new name:
```rust
assert_eq!(names, ["delegate", "collect", "spawn", "send", "panes", "status", "raise_question"]);
```

- [ ] **Step 2: Run — verify fail**

```
cargo test -p kern mcp::tools_mux 2>&1 | tail -8
```

- [ ] **Step 3: Add the schema**

Append to the `vec![...]` in `tool_schemas()` (after the `status` entry):

```rust
json!({
    "name": "raise_question",
    "description": "Ask the human operator a question and BLOCK until they answer in the mux. Returns their answer as the tool result. Use when you need a human decision to proceed.",
    "inputSchema": {
        "type": "object",
        "required": ["question"],
        "properties": {
            "question": { "type": "string", "description": "The decision or answer you need from the human." },
            "label":    { "type": "string", "description": "Optional: who is asking (e.g. 'worker-3'), shown in the operator's roster." },
        },
    },
}),
```

- [ ] **Step 4: Add the handler**

Add the arg struct alongside the others in `tools_mux.rs`:
```rust
#[derive(Deserialize)]
struct RaiseQuestionArgs { question: String, #[serde(default)] label: String }
```
Add the method in the `impl Server` block:
```rust
/// `raise_question` — register a question and BLOCK until the human answers
/// via the Ctrl+K overlay. The wire path has no read timeout, so the block is
/// indefinite (the parked worker thread is freed the moment the answer lands).
pub(crate) fn tool_raise_question(&self, args: &serde_json::Value) -> serde_json::Value {
    let Some(reg) = self.mux.as_ref() else { return tool_error("not running under a mux") };
    let p: RaiseQuestionArgs = match serde_json::from_value(args.clone()) {
        Ok(v) => v,
        Err(e) => return tool_error(&format!("invalid args: {e}")),
    };
    // Lock ONLY to register; release before blocking so the TUI can deliver.
    let rx = {
        let mut r = match reg.lock() { Ok(g) => g, Err(_) => return tool_error("registry lock poisoned") };
        let (_id, rx) = r.questions.open(p.label, p.question);
        rx
    };
    match rx.recv() {
        Ok(answer) => tool_result_json(&json!({ "answer": answer })),
        Err(_)     => tool_error("question dismissed without an answer"),
    }
}
```

- [ ] **Step 5: Dispatch in `mcp.rs`**

In `src/mcp.rs` `call_tool`, add an arm beside the other comms tools:
```rust
"raise_question" => self.tool_raise_question(args),
```

- [ ] **Step 6: Run — verify pass**

```
cargo test -p kern mcp::tools_mux 2>&1 | tail -8
cargo build -p kern 2>&1 | tail -5
```

- [ ] **Step 7: Commit**

```
git add src/mcp/tools_mux.rs src/mcp.rs
git commit -m "feat(mcp): blocking raise_question tool (gated on mux)"
```

---

## Task 4: `QuestionsOverlay` in `questions.rs`

**Files:** Create `src/mux/questions.rs`

- [ ] **Step 1: Write failing tests** (create the file with only tests + the action enum stub)

```rust
//! Ctrl+K overlay: roster of agents waiting on a human answer.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_builds_answer_and_enter_emits_answer_action() {
        let mut o = QuestionsOverlay::new();
        // roster has 1 item; selected = 0
        o.handle_key(&KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE), 1);
        o.handle_key(&KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE), 1);
        assert_eq!(o.input(), "hi");
        let action = o.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), 1);
        assert!(matches!(action, QuestionsAction::Answer { ref text, index: 0 } if text == "hi"));
    }

    #[test]
    fn enter_with_empty_input_is_noop() {
        let mut o = QuestionsOverlay::new();
        let action = o.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), 1);
        assert!(matches!(action, QuestionsAction::None));
    }

    #[test]
    fn esc_closes() {
        let mut o = QuestionsOverlay::new();
        assert!(matches!(o.handle_key(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), 1), QuestionsAction::Close));
    }

    #[test]
    fn down_up_moves_selection_within_bounds() {
        let mut o = QuestionsOverlay::new();
        o.handle_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), 3);
        assert_eq!(o.selected(), 1);
        o.handle_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), 3);
        o.handle_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), 3); // clamps at 2
        assert_eq!(o.selected(), 2);
        o.handle_key(&KeyEvent::new(KeyCode::Up, KeyModifiers::NONE), 3);
        assert_eq!(o.selected(), 1);
    }

    #[test]
    fn ctrl_d_dismisses_selected() {
        let mut o = QuestionsOverlay::new();
        let action = o.handle_key(&KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL), 2);
        assert!(matches!(action, QuestionsAction::Dismiss { index: 0 }));
    }

    #[test]
    fn enter_on_empty_roster_is_noop() {
        let mut o = QuestionsOverlay::new();
        o.handle_key(&KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE), 0);
        let action = o.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), 0);
        assert!(matches!(action, QuestionsAction::None));
    }
}
```

- [ ] **Step 2: Run — verify fail**

```
cargo test -p kern mux::questions 2>&1 | tail -8
```

- [ ] **Step 3: Implement** (add above the test module)

```rust
use std::io::{self, Write};
use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
};

use crate::mux::registry::PaneRegistry;

/// Result of a key press while the overlay is open. The caller (run_tui)
/// applies `Answer`/`Dismiss` against the live registry by `index` into the
/// current roster — so the overlay holds no registry reference itself.
pub enum QuestionsAction {
    None,
    Close,
    Answer { index: usize, text: String },
    Dismiss { index: usize },
}

pub struct QuestionsOverlay {
    selected: usize,
    input:    String,
}

impl QuestionsOverlay {
    pub fn new() -> Self { Self { selected: 0, input: String::new() } }

    pub fn input(&self) -> &str { &self.input }
    pub fn selected(&self) -> usize { self.selected }

    /// Handle a key given the current roster length. Pure (no I/O).
    pub fn handle_key(&mut self, kev: &KeyEvent, roster_len: usize) -> QuestionsAction {
        match kev.code {
            KeyCode::Esc => QuestionsAction::Close,
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                QuestionsAction::None
            }
            KeyCode::Down => {
                if roster_len > 0 && self.selected + 1 < roster_len { self.selected += 1; }
                QuestionsAction::None
            }
            KeyCode::Char('d') if kev.modifiers == KeyModifiers::CONTROL => {
                if roster_len == 0 { return QuestionsAction::None; }
                QuestionsAction::Dismiss { index: self.selected.min(roster_len - 1) }
            }
            KeyCode::Enter => {
                if roster_len == 0 || self.input.is_empty() { return QuestionsAction::None; }
                let text = std::mem::take(&mut self.input);
                QuestionsAction::Answer { index: self.selected.min(roster_len - 1), text }
            }
            KeyCode::Backspace => { self.input.pop(); QuestionsAction::None }
            KeyCode::Char(c) if kev.modifiers == KeyModifiers::NONE || kev.modifiers == KeyModifiers::SHIFT => {
                self.input.push(c);
                QuestionsAction::None
            }
            _ => QuestionsAction::None,
        }
    }

    /// Render the roster + input line. Rows 1..(rows-1); row 0 is the status bar.
    pub fn draw(&self, stdout: &mut impl Write, registry: &PaneRegistry, cols: u16, rows: u16) -> io::Result<()> {
        let roster = registry.questions.list();
        let width = cols as usize;
        let input_row = rows.saturating_sub(1);

        // Title.
        queue!(stdout, MoveTo(0, 1), SetAttribute(Attribute::Bold),
            Print(format!("Waiting for you ({})", roster.len())), SetAttribute(Attribute::Reset))?;
        queue!(stdout, Print(" ".repeat(width.saturating_sub(20))))?;

        // Roster rows (start at row 3).
        let first_row: u16 = 3;
        let max_rows = input_row.saturating_sub(first_row + 1) as usize;
        if roster.is_empty() {
            queue!(stdout, MoveTo(0, first_row), SetForegroundColor(Color::DarkGrey),
                Print("No agents are waiting."), ResetColor)?;
        }
        for (i, (_id, label, question)) in roster.iter().take(max_rows).enumerate() {
            let row = first_row + i as u16;
            let sel = i == self.selected.min(roster.len().saturating_sub(1));
            let marker = if sel { "▶ " } else { "  " };
            let who = if label.is_empty() { String::new() } else { format!("{label} · ") };
            let line: String = format!("{marker}{who}{question}").chars().take(width).collect();
            let pad = width.saturating_sub(line.chars().count());
            if sel {
                queue!(stdout, MoveTo(0, row), SetAttribute(Attribute::Bold), Print(&line),
                    Print(" ".repeat(pad)), SetAttribute(Attribute::Reset))?;
            } else {
                queue!(stdout, MoveTo(0, row), Print(&line), Print(" ".repeat(pad)))?;
            }
        }

        // Divider + input line.
        let divider_row = input_row.saturating_sub(1);
        queue!(stdout, MoveTo(0, divider_row), SetForegroundColor(Color::DarkGrey),
            Print("─".repeat(width)), ResetColor)?;
        let prompt = format!("answer ▸ {}█", self.input);
        let pad = width.saturating_sub(prompt.chars().count());
        queue!(stdout, MoveTo(0, input_row), Print(&prompt), Print(" ".repeat(pad)))?;
        Ok(())
    }
}

impl Default for QuestionsOverlay {
    fn default() -> Self { Self::new() }
}
```

- [ ] **Step 4: Run — verify pass**

```
cargo test -p kern mux::questions 2>&1 | tail -8
```

- [ ] **Step 5: Register module in `src/mux/mod.rs`**

Add `mod questions;` and `pub use questions::{QuestionsOverlay, QuestionsAction};`.

- [ ] **Step 6: Commit**

```
git add src/mux/questions.rs src/mux/mod.rs
git commit -m "feat(mux): QuestionsOverlay (Ctrl+K roster + answer input)"
```

---

## Task 5: Wire overlay into `tui.rs` + status badge

**Files:** Modify `src/mux/tui.rs`

- [ ] **Step 1: Read the research-panel integration**

Read `src/mux/tui.rs` and locate: (a) the `research: Option<ResearchPanel>` loop state, (b) the `keymap.matches_research` toggle branch, (c) the draw dispatch (`if let Some(ref mut panel) = research`), and (d) the status-bar rendering (`draw_status_bar` or inlined). The questions overlay mirrors (a)–(c) exactly; the badge extends (d).

- [ ] **Step 2: Add overlay loop state**

Beside `let mut research: Option<...>`, add:
```rust
let mut questions: Option<crate::mux::QuestionsOverlay> = None;
```

- [ ] **Step 3: Toggle on Ctrl+K**

In the key-event block, add a branch mirroring the research toggle (place it so an open research panel and an open questions overlay are mutually exclusive — toggling one closes the other):
```rust
} else if keymap.matches_questions(&kev) {
    research = None;
    questions = match questions { Some(_) => None, None => Some(crate::mux::QuestionsOverlay::new()) };
    queue!(stdout, crossterm::terminal::Clear(crossterm::terminal::ClearType::All))?;
    snapshots.clear();
```

- [ ] **Step 4: Dispatch keys when open**

Add, beside the research key-dispatch branch (and before normal PTY routing):
```rust
} else if let Some(ref mut overlay) = questions {
    let roster_len = {
        let reg = registry.lock().unwrap_or_else(|p| p.into_inner());
        reg.questions.len()
    };
    match overlay.handle_key(&kev, roster_len) {
        crate::mux::QuestionsAction::Close => {
            questions = None;
            queue!(stdout, crossterm::terminal::Clear(crossterm::terminal::ClearType::All))?;
            snapshots.clear();
        }
        crate::mux::QuestionsAction::Answer { index, text } => {
            let mut reg = registry.lock().unwrap_or_else(|p| p.into_inner());
            if let Some((id, _, _)) = reg.questions.list().get(index).cloned() {
                reg.questions.answer(&id, text);
            }
        }
        crate::mux::QuestionsAction::Dismiss { index } => {
            let mut reg = registry.lock().unwrap_or_else(|p| p.into_inner());
            if let Some((id, _, _)) = reg.questions.list().get(index).cloned() {
                reg.questions.dismiss(&id);
            }
        }
        crate::mux::QuestionsAction::None => {}
    }
```

> **Engineer note:** match the exact `else if` chain shape and the `MutexGuard` handling already used by the research branch (drop the guard before re-locking). Adjust to whatever lock-recovery helper `tui.rs` uses.

- [ ] **Step 5: Draw dispatch**

In the draw section, add a branch beside the research draw:
```rust
if let Some(ref overlay) = questions {
    {
        let reg = registry.lock().unwrap_or_else(|p| p.into_inner());
        draw_status_bar(&reg, &mut stdout, cols, &cwd)?; // or the inlined bar
        overlay.draw(&mut stdout, &reg, cols, rows)?;
    }
} else if let Some(ref mut panel) = research {
    /* existing research draw */
} else {
    /* existing normal draw */
}
stdout.flush()?;
```

- [ ] **Step 6: Status-bar badge**

In the status-bar rendering, after the existing left/cwd segment, append a pending-questions badge when non-zero. Where the bar reads `registry`:
```rust
let pending = registry.questions.len();
// include in the bar text only when pending > 0:
let badge = if pending > 0 { format!(" ❓{pending} ") } else { String::new() };
```
Insert `badge` into the assembled status line (e.g. into the mid/right region) so it shows even with the overlay closed. Match the bar's existing `format_status_*` composition.

- [ ] **Step 7: Build + full test**

```
cargo build -p kern 2>&1 | tail -8
cargo test -p kern 2>&1 | tail -12
```
Expected: builds; all pass.

- [ ] **Step 8: Commit**

```
git add src/mux/tui.rs
git commit -m "feat(mux): wire Ctrl+K questions overlay + status-bar pending badge"
```

---

## Task 6: Verify

- [ ] **Step 1: Build + tests + (my-files) clippy**

```
cargo build -p kern 2>&1 | tail -5
cargo test -p kern 2>&1 | tail -12
cargo clippy -p kern 2>&1 | grep -E "questions|tools_mux|registry" || echo "no clippy hits in new files"
```

- [ ] **Step 2: Manual smoke (needs an interactive terminal — operator)**

1. Launch `kern`; in a pane have the agent call `mcp__kern__raise_question({question:"ship it?", label:"w1"})`.
2. The agent's turn parks; the status bar shows `❓1`.
3. `Ctrl+K` → roster shows `w1 · ship it?`; type `yes`, Enter.
4. The agent's tool call returns `{"answer":"yes"}` and it continues.
5. `Ctrl+D` on a question dismisses it → that agent's call returns the dismissed error.

---

## Self-Review

- **Spec §1 QuestionRegistry** → Task 1. ✓
- **Spec §2 raise_question (blocking, mux-gated)** → Task 3 (handler releases lock before `recv()`). ✓
- **Spec §3 timeout** → resolved in planning: no wire read-timeout exists, so indefinite `recv()` is safe (no `recv_timeout` needed). ✓
- **Spec §4 overlay (Ctrl+K)** → Tasks 4–5. ✓
- **Spec §5 status badge** → Task 5 Step 6. ✓
- **Spec §6 config** → Task 2. ✓
- **Spec §7 edge cases** → dismiss (Task 1/4/5), unknown-id false (Task 1), empty label (Task 4 draw). Pane-exit reap is a noted refinement, not in this plan. ✓
- **Type consistency:** `QuestionsAction::{Answer,Dismiss}` carry `index`; run_tui resolves index→id via `list()` before `answer`/`dismiss`. `open/list/answer/dismiss/len` names match across tasks. ✓
- **No placeholders;** every code step is complete. Investigation steps (Task 2.3, 5.1) read named files before mirroring an established pattern. ✓
- **Version `1.0.0`, no bincode structs touched.** ✓
