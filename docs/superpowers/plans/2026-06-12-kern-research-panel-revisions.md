# Kern Research Panel Revisions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Revise the shipped `Ctrl+L` research panel so it persists across hide/show, treats typing as "continue" (Enter resets), sends the whole conversation buffer to kern, renders kern's prose `answer` (not the JSON envelope) with structure intact, and shows a "whole conversation is sent" warning.

**Architecture:** All chat/render/answer logic stays in `src/mux/research.rs`; the panel-lifecycle (persistent show/hide) change is in `src/mux/tui.rs`. kern-only — no `claude -p`/haiku. No config, schema, or bincode changes.

**Tech Stack:** Rust, `crossterm` (workspace), `serde_json` (workspace), `tokio` runtime handle (already wired), `trnsprt::kern_rpc` (local). `anyhow` (workspace).

**Spec:** `docs/superpowers/specs/2026-06-12-kern-research-panel-revisions-design.md`

---

## File Map

| Action | Path | Responsibility |
|--------|------|----------------|
| Modify | `src/mux/research.rs` | WelcomeBack continue-not-reset + placeholder; `build_kern_query` (whole buffer); `answer_from_envelope_text` + `summarize_entities`; line-preserving wrap; warning line + layout |
| Modify | `src/mux/tui.rs` | Persistent panel: `research_visible` flag, toggle, Esc-hides, dispatch/draw/tick/resize gating |

All `cargo` commands run from the repo root (`C:\Users\sayhe\dev\relay\kern`). The crate is `kern`.

---

## Task 1: WelcomeBack = continue (typing), Enter = reset; placeholder text

**Files:**
- Modify: `src/mux/research.rs` (the `SessionState::WelcomeBack` arm of `ResearchPanel::handle_key`; the `WelcomeBack` arm of `ResearchPanel::draw`'s input line)
- Test: `src/mux/research.rs` `#[cfg(test)] mod tests`

- [ ] **Step 1: Write the failing tests**

Append to the `mod tests` block in `src/mux/research.rs`:

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kern mux::research::tests::welcome_back 2>&1`
Expected: FAIL — `welcome_back_typing_continues` (history cleared to 0 by the current `handle_reset()` path) and `welcome_back_backspace_continues_keeps_history`.

- [ ] **Step 3: Rewrite the `WelcomeBack` arm in `handle_key`**

In `src/mux/research.rs`, replace the existing `SessionState::WelcomeBack => match kev.code { ... }` arm with:

```rust
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
```

- [ ] **Step 4: Update the placeholder text in `draw`**

In `ResearchPanel::draw`, in the `SessionState::WelcomeBack` input-line arm, change the placeholder string:

```rust
            SessionState::WelcomeBack => {
                let placeholder = "type to continue, enter to reset";
```

(Only the string literal changes; the surrounding dim-render block stays.)

- [ ] **Step 5: Update the now-stale existing test**

The shipped test `research_panel_welcome_back_reset` calls `handle_reset()` directly and still holds — leave it. No other edits needed.

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p kern mux::research 2>&1`
Expected: PASS — all research tests including the three new `welcome_back_*`.

- [ ] **Step 7: Commit**

```bash
git add kern/src/mux/research.rs
git commit -m "feat(mux/research): WelcomeBack typing continues, Enter resets; new placeholder"
```

---

## Task 2: Send the whole conversation buffer to kern (`build_kern_query`)

**Files:**
- Modify: `src/mux/research.rs` (add `build_kern_query`; wire it into the `Enter` submit in `handle_key`)
- Test: `src/mux/research.rs` `mod tests`

- [ ] **Step 1: Write the failing test**

Append to `mod tests`:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kern mux::research::tests::build_kern_query 2>&1`
Expected: FAIL — `build_kern_query` not defined (compile error).

- [ ] **Step 3: Implement `build_kern_query`**

Add this free function to `src/mux/research.rs` (next to `kern_answer`, before `#[cfg(test)]`):

```rust
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
```

- [ ] **Step 4: Wire it into the `Enter` submit**

In `handle_key`, in the default (`Fresh`/`Typing`) arm's `KeyCode::Enter` branch, replace the body so the query is built from the whole buffer (after the user turn is pushed):

```rust
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
                                let _ = tx.send(Err(anyhow::anyhow!("no tokio runtime: {e}")));
                            }
                        }
                        self.session.pending = Some(rx);
                        self.session.state   = SessionState::Thinking;
                    }
                }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p kern mux::research 2>&1`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add kern/src/mux/research.rs
git commit -m "feat(mux/research): send whole conversation buffer to kern (multi-turn)"
```

---

## Task 3: Render kern's prose `answer`, not the JSON envelope

**Files:**
- Modify: `src/mux/research.rs` (add `answer_from_envelope_text` + `summarize_entities`; call from `kern_answer`)
- Test: `src/mux/research.rs` `mod tests`

- [ ] **Step 1: Write the failing tests**

Append to `mod tests`:

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kern mux::research::tests::answer_from_envelope 2>&1`
Expected: FAIL — `answer_from_envelope_text` not defined (compile error).

- [ ] **Step 3: Implement the extractor + fallback**

Add to `src/mux/research.rs` (next to `extract_rpc_text`, before `#[cfg(test)]`):

```rust
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
```

- [ ] **Step 4: Call the extractor from `kern_answer`**

In `kern_answer`, replace the final `Ok(extract_rpc_text(env))` with a parse step. The function tail becomes:

```rust
    let env = &res.envelope;
    if env.get("isError").and_then(|v| v.as_bool()).unwrap_or(false) {
        anyhow::bail!("kern: {}", extract_rpc_text(env));
    }
    let raw = extract_rpc_text(env);
    answer_from_envelope_text(&raw)
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p kern mux::research 2>&1`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add kern/src/mux/research.rs
git commit -m "feat(mux/research): render kern's prose answer field, not the JSON envelope"
```

---

## Task 4: Line-preserving prose wrap

**Files:**
- Modify: `src/mux/research.rs` (rename `wrap_text` → `wrap_one_line`; add `wrap_text_preserving`; swap the `Assistant` render call in `draw`; update the 4 existing `wrap_text_*` tests)
- Test: `src/mux/research.rs` `mod tests`

- [ ] **Step 1: Write the failing test**

Append to `mod tests`:

```rust
#[test]
fn wrap_text_preserving_keeps_paragraph_breaks() {
    let text = "Para one is short.\n\n- bullet a\n- bullet b";
    let lines = wrap_text_preserving(text, 40);
    assert!(lines.iter().any(|l| l.contains("Para one")), "first paragraph present");
    assert!(lines.iter().any(|l| l.is_empty()), "blank line between paragraphs preserved");
    assert!(lines.iter().any(|l| l.contains("- bullet a")), "bullet a on its own line");
    assert!(lines.iter().any(|l| l.contains("- bullet b")), "bullet b on its own line");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kern mux::research::tests::wrap_text_preserving 2>&1`
Expected: FAIL — `wrap_text_preserving` not defined (compile error).

- [ ] **Step 3: Rename `wrap_text` to `wrap_one_line`**

In `src/mux/research.rs`, rename the existing function `fn wrap_text(text: &str, width: usize) -> Vec<String>` to `fn wrap_one_line(text: &str, width: usize) -> Vec<String>` (body unchanged).

- [ ] **Step 4: Add `wrap_text_preserving`**

Add directly after `wrap_one_line`:

```rust
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
```

- [ ] **Step 5: Swap the `Assistant` render call in `draw`**

In `ResearchPanel::draw`, the chat-history build loop has an `else` branch (the `Assistant` case) that calls `wrap_text`. Change it:

```rust
            } else {
                for chunk in wrap_text_preserving(&entry.text, chat_width) {
                    all_lines.push((false, chunk));
                }
            }
```

- [ ] **Step 6: Update the 4 existing wrap tests to call `wrap_one_line`**

In `mod tests`, rename the calls (the test fn names can stay):

```rust
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
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test -p kern mux::research 2>&1`
Expected: PASS — `wrap_text_preserving_keeps_paragraph_breaks` plus the renamed four.

- [ ] **Step 8: Commit**

```bash
git add kern/src/mux/research.rs
git commit -m "feat(mux/research): line-preserving wrap so prose answers render with structure"
```

---

## Task 5: Warning line + chat-pane layout shift

**Files:**
- Modify: `src/mux/research.rs` (`ResearchPanel::draw` left-pane layout rows + warning render)

This task is visual; correctness is verified by `cargo build` + manual check (no unit test — `draw` writes to a `Write` sink and is integration-level).

- [ ] **Step 1: Shift the chat-pane layout rows**

In `ResearchPanel::draw`, find the left-pane layout block:

```rust
        let chat_width  = left_cols as usize;
        let input_row   = row_offset + pane_rows - 1;
        let divider_row = input_row - 1;
        let history_rows = pane_rows.saturating_sub(2) as usize;
```

Replace with (reserve the bottom row for the warning):

```rust
        let chat_width  = left_cols as usize;
        let warn_row    = row_offset + pane_rows - 1;
        let input_row   = warn_row.saturating_sub(1);
        let divider_row = input_row.saturating_sub(1);
        let history_rows = pane_rows.saturating_sub(3) as usize;
```

- [ ] **Step 2: Render the warning line**

In `ResearchPanel::draw`, immediately after the `match self.session.state { ... }` block that renders the input line (and before the final `Ok(())`), add:

```rust
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
```

- [ ] **Step 3: Build to verify it compiles**

Run: `cargo build -p kern 2>&1`
Expected: compiles with 0 errors (`warn_row` is now used; no unused-variable warning).

- [ ] **Step 4: Run the full research test module**

Run: `cargo test -p kern mux::research 2>&1`
Expected: PASS (layout change does not affect unit tests).

- [ ] **Step 5: Commit**

```bash
git add kern/src/mux/research.rs
git commit -m "feat(mux/research): dim 'whole conversation is sent' warning + layout row"
```

---

## Task 6: Persistent panel — show/hide without dropping (`tui.rs`)

**Files:**
- Modify: `src/mux/tui.rs` (`run_tui`: add `research_visible`; toggle; Esc-hides; gate draw/tick/resize/dispatch)

Integration-level; verified by `cargo build` + the manual checklist at the end.

- [ ] **Step 1: Add the visibility flag**

In `run_tui`, after the line `let mut research: Option<crate::mux::ResearchPanel> = None;` add:

```rust
    let mut research_visible: bool = false;
```

- [ ] **Step 2: Gate the draw block on visibility (don't drop on hide)**

Replace the draw block (currently `if let Some(ref mut panel) = research { ... } else { ... }`) with:

```rust
        // Draw: research panel takes over when visible; otherwise normal pane draw.
        if research_visible {
            if let Some(ref mut panel) = research {
                panel.tick();
                {
                    let reg = registry.lock().unwrap_or_else(|p| p.into_inner());
                    draw_status_bar(&reg, &mut stdout, cols, &cwd)?;
                }
                panel.draw(&mut stdout, cols, rows)?;
            }
        } else {
            let reg = registry.lock().unwrap_or_else(|p| p.into_inner());
            draw_frame(&reg, &mut stdout, cols, rows, &cwd, &mut snapshots)?;
        }
        stdout.flush()?;
```

- [ ] **Step 3: Gate the resize branch on visibility**

In the `Event::Resize(w, h)` arm, change `if research.is_none()` to `if !research_visible`:

```rust
                Event::Resize(w, h) => {
                    cols = w;
                    rows = h;
                    if !research_visible {
                        let mut reg = registry.lock().unwrap_or_else(|p| p.into_inner());
                        reg.resize_all(cols, rows.saturating_sub(1));
                    }
                    queue!(stdout, Clear(ClearType::All))?;
                    snapshots.clear();
                }
```

- [ ] **Step 4: Rewrite the toggle + key dispatch**

Replace the `else if keymap.matches_research(&kev) { ... } else if let Some(ref mut panel) = research { ... }` blocks with:

```rust
                    } else if keymap.matches_research(&kev) {
                        // Toggle visibility. Construct once on first open; NEVER drop —
                        // history + journal tailer persist while hidden.
                        match research {
                            None => {
                                let mut panel = crate::mux::ResearchPanel::new();
                                panel.session.on_panel_open();
                                research = Some(panel);
                                research_visible = true;
                            }
                            Some(ref mut panel) => {
                                research_visible = !research_visible;
                                if research_visible {
                                    // Re-entering: WelcomeBack if history exists, else Fresh.
                                    panel.session.on_panel_open();
                                }
                            }
                        }
                        queue!(stdout, Clear(ClearType::All))?;
                        snapshots.clear();
                    } else if research_visible {
                        // Delegate keys to the panel while it is shown.
                        if let Some(ref mut panel) = research {
                            let close = panel.handle_key(&kev);
                            if close {
                                // Esc HIDES (panel + history preserved), never drops.
                                research_visible = false;
                                queue!(stdout, Clear(ClearType::All))?;
                                snapshots.clear();
                            }
                        }
                    } else {
```

(The `} else {` opens the existing normal-pane-routing block, which is unchanged below it. Confirm the brace count still balances — one `else if` arm was replaced by two, and the trailing `else {` is the same one that already wrapped normal pane routing.)

- [ ] **Step 5: Build to verify it compiles**

Run: `cargo build -p kern 2>&1`
Expected: compiles with 0 errors.

- [ ] **Step 6: Commit**

```bash
git add kern/src/mux/tui.rs
git commit -m "feat(mux): persistent research panel — Ctrl+L show/hide, Esc hides, never dropped"
```

---

## Task 7: Full verification

**Files:** none (verification only)

- [ ] **Step 1: Full test suite**

Run: `cargo test -p kern 2>&1`
Expected: all tests pass.

- [ ] **Step 2: Clippy clean**

Run: `cargo clippy -p kern -- -D warnings 2>&1`
Expected: 0 warnings.

- [ ] **Step 3: Manual verification (mux mode)**

1. Launch `kern` (mux). `Ctrl+L` opens the panel.
2. Ask a question → a **prose** answer appears (no JSON object, no entities array).
3. Ask a follow-up ("what about X?") → answer reflects the earlier turn (whole buffer sent).
4. `Ctrl+L` to hide, `Ctrl+L` to show → history still present; input shows `type to continue, enter to reset`.
5. Type a char on re-show → history preserved, cursor active (does NOT clear).
6. `Enter` on the empty re-show placeholder → history cleared (memory reset).
7. The dim `whole conversation is sent to kern for accuracy` line is at the bottom whenever the panel is shown.
8. Hide the panel → PTY panes resize/behave normally; the journal keeps tailing across hide/show.

- [ ] **Step 4: Final commit (if any manual fixes were needed)**

```bash
git add kern/src/mux/
git commit -m "fix(mux/research): post-verification adjustments"
```

(Skip if nothing changed.)

---

## Done

After Task 7, the panel persists across hide/show, continues-on-type / resets-on-Enter, sends the whole buffer to kern, renders kern's prose answer with structure intact, and warns that the whole conversation is sent. No haiku, no config/schema/bincode changes, version stays `1.0.0`.
