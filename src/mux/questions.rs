//! Ctrl+K overlay: roster of agents blocked on a human answer.
//!
//! Sibling to [`crate::mux::ResearchPanel`] (Ctrl+L). The overlay holds only
//! its own UI state (selection + the answer being typed); it reads the live
//! roster from [`PaneRegistry::questions`] each frame and reports the human's
//! intent back to `run_tui` as a [`QuestionsAction`] keyed by roster index —
//! `run_tui` resolves the index to a question id and calls
//! `QuestionRegistry::answer` / `dismiss`, which unblocks the parked
//! `raise_question` tool call.

use std::io::{self, Write};

use crossterm::{
    cursor::MoveTo,
    event::{KeyCode, KeyEvent, KeyModifiers},
    queue,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
};

use crate::mux::registry::PaneRegistry;

/// Result of a key press while the overlay is open. `run_tui` applies
/// `Answer`/`Dismiss` against the live registry by `index` into the current
/// roster — so the overlay holds no registry reference itself.
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
    pub fn new() -> Self {
        Self { selected: 0, input: String::new() }
    }

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
                if roster_len > 0 && self.selected + 1 < roster_len {
                    self.selected += 1;
                }
                QuestionsAction::None
            }
            KeyCode::Char('d') if kev.modifiers == KeyModifiers::CONTROL => {
                if roster_len == 0 {
                    return QuestionsAction::None;
                }
                QuestionsAction::Dismiss { index: self.selected.min(roster_len - 1) }
            }
            KeyCode::Enter => {
                if roster_len == 0 || self.input.is_empty() {
                    return QuestionsAction::None;
                }
                let text = std::mem::take(&mut self.input);
                QuestionsAction::Answer { index: self.selected.min(roster_len - 1), text }
            }
            KeyCode::Backspace => {
                self.input.pop();
                QuestionsAction::None
            }
            KeyCode::Char(c)
                if kev.modifiers == KeyModifiers::NONE || kev.modifiers == KeyModifiers::SHIFT =>
            {
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

        // Title row.
        let title = format!("Waiting for you ({})", roster.len());
        let title_pad = width.saturating_sub(title.chars().count());
        queue!(
            stdout,
            MoveTo(0, 1),
            SetAttribute(Attribute::Bold),
            Print(&title),
            SetAttribute(Attribute::Reset),
            Print(" ".repeat(title_pad))
        )?;

        // Roster rows (start at row 3).
        let first_row: u16 = 3;
        let max_rows = input_row.saturating_sub(first_row + 1) as usize;
        let sel = self.selected.min(roster.len().saturating_sub(1));
        if roster.is_empty() {
            queue!(
                stdout,
                MoveTo(0, first_row),
                SetForegroundColor(Color::DarkGrey),
                Print("No agents are waiting."),
                ResetColor
            )?;
        }
        for (i, (_id, label, question)) in roster.iter().take(max_rows).enumerate() {
            let row = first_row + i as u16;
            let is_sel = i == sel;
            let marker = if is_sel { "▶ " } else { "  " };
            let who = if label.is_empty() { String::new() } else { format!("{label} · ") };
            let line: String = format!("{marker}{who}{question}").chars().take(width).collect();
            let pad = width.saturating_sub(line.chars().count());
            if is_sel {
                queue!(
                    stdout,
                    MoveTo(0, row),
                    SetAttribute(Attribute::Bold),
                    Print(&line),
                    Print(" ".repeat(pad)),
                    SetAttribute(Attribute::Reset)
                )?;
            } else {
                queue!(stdout, MoveTo(0, row), Print(&line), Print(" ".repeat(pad)))?;
            }
        }

        // Divider + input line.
        let divider_row = input_row.saturating_sub(1);
        queue!(
            stdout,
            MoveTo(0, divider_row),
            SetForegroundColor(Color::DarkGrey),
            Print("─".repeat(width)),
            ResetColor
        )?;
        let prompt = format!("answer ▸ {}█", self.input);
        let pad = width.saturating_sub(prompt.chars().count());
        queue!(stdout, MoveTo(0, input_row), Print(&prompt), Print(" ".repeat(pad)))?;
        Ok(())
    }
}

impl Default for QuestionsOverlay {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_builds_answer_and_enter_emits_answer_action() {
        let mut o = QuestionsOverlay::new();
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
        assert!(matches!(
            o.handle_key(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), 1),
            QuestionsAction::Close
        ));
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
