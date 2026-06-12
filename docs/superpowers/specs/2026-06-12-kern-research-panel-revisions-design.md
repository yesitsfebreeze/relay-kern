# Kern Research Panel — Revisions Design Spec

**Date:** 2026-06-12
**Status:** Draft (for review)
**Supersedes:** Sections 2, 4, 5 of `2026-06-12-kern-research-panel-design.md` (the panel is already implemented; this spec revises its behavior).
**Scope:** `src/mux/research.rs`, `src/mux/tui.rs` (no config / schema / bincode changes)

---

## Motivation

The `Ctrl+L` research panel is built and wired. Three problems with the shipped behavior:

1. **The panel is destroyed on hide.** `tui.rs` toggles with `research.take()` → on toggle-off the panel is dropped, killing the journal-tailer thread and discarding chat history. It must instead **stay alive when hidden** — show/hide a persistent panel.
2. **The chat answer renders as a JSON list.** The kern `query` tool returns `{"entities":[...], "answer":"<prose>"}` as a JSON object (`tools_query.rs`). The panel concatenates the whole content blob, so it surfaces the entire object — entities array and all. kern **is** synthesizing a natural-language `answer`; the panel just shows the wrong field.
3. **No conversational continuity.** Each query is single-shot; re-opening offers only a destructive "reset". The chat should be multi-turn and continuable.

This spec is **kern-only**. There is no `claude -p` / haiku path; kern's onboard answer pipeline (`query(answer=true)`) is the sole LLM.

---

## Section 1: Persistent panel (show / hide, never dropped)

The panel is constructed lazily on the first `Ctrl+L` and **never dropped** for the life of the mux. Visibility is a separate flag.

**`tui.rs` loop state:**

```rust
let mut research: Option<crate::mux::ResearchPanel> = None; // construct-once
let mut research_visible: bool = false;
```

**`Ctrl+L` toggle:**

```rust
} else if keymap.matches_research(&kev) {
    match research {
        None => {
            // First open: construct (spawns the journal tailer once).
            let mut panel = crate::mux::ResearchPanel::new();
            panel.session.on_panel_open();
            research = Some(panel);
            research_visible = true;
        }
        Some(ref mut panel) => {
            research_visible = !research_visible;
            if research_visible {
                panel.session.on_panel_open(); // re-entering → WelcomeBack if history exists
            }
        }
    }
    queue!(stdout, Clear(ClearType::All))?;
    snapshots.clear();
}
```

**Key dispatch / draw / tick** gate on `research_visible && research.is_some()` instead of on `research.is_some()`.

**`Esc` inside the panel** hides it (does **not** drop): `handle_key` returning "close" sets `research_visible = false` (panel + history + journal ring preserved). The journal tailer thread now lives until the mux process exits — matching the original tailer design intent.

**Resize:** unchanged — `if !research_visible { reg.resize_all(...) }` so PTY panes only resize when the panel is hidden.

---

## Section 2: Session state & continuity (supersedes original Section 2)

The chat is **multi-turn**. The full conversation buffer is sent to kern on every query (see Section 3).

`SessionState` is unchanged (`Fresh | WelcomeBack | Typing | Thinking`).

**Show / hide:**
- First `Ctrl+L`: construct panel; `on_panel_open()` → `Fresh` (empty history).
- Hide (`Esc` or `Ctrl+L`): `research_visible = false`; history + ring **preserved**.
- Re-show (`Ctrl+L`): `on_panel_open()` → `WelcomeBack` if history non-empty, else `Fresh`.

**`WelcomeBack` behavior — REVISED (typing continues, does NOT reset):**
- Input row renders dim placeholder `type to continue, enter to reset`.
- **Any printable char → keep history, → `Typing`, append the char.** (No `handle_reset()`.)
- **`Backspace` → keep history, → `Typing`** (no-op on the empty buffer otherwise).
- **`Enter` (empty input) → `handle_reset()` — clears history + input → `Fresh`.** This is the only "clear the memory" gesture.
- `Esc` → hide panel, history preserved.

> This reverses the shipped `WelcomeBack` handler, which called `handle_reset()` on the first keystroke. The corresponding `research.rs` tests (`chat_session_welcome_back_*`) are updated to assert the continue-not-reset behavior.

**Normal input (`Fresh` / `Typing`):**
- Printable chars → append, → `Typing`.
- `Backspace` → `String::pop()` (multi-byte safe).
- `Enter` with non-empty input → push `User` entry to history, spawn answer thread with the **whole buffer**, clear `input`, → `Thinking`.
- `Enter` empty → no-op.
- `Esc` → hide panel.

**`Thinking`:** printable / `Enter` ignored; `Esc` drops the pending receiver and → `Typing` (cancel), unchanged.

**Answer arrival** (polled per frame): `Ok(text)` → push `Assistant` entry, → `Typing`; `Err(e)` → push `Assistant` entry `[kern error: {e}]`, → `Typing`. Unchanged.

---

## Section 3: Answer path — whole-buffer multi-turn over kern (supersedes original Section 4)

### 3a. Send the whole conversation buffer

On `Enter`, the user turn is pushed to `history`, then the answer thread is spawned with a **kern query text built from the entire buffer**, not just the latest input:

```rust
/// Serialize the full chat buffer into a single kern query string.
/// The whole conversation is sent for answer accuracy (the user is warned
/// of this in the UI — see Section 4). The current question is the last
/// `User` entry already pushed onto `history`.
fn build_kern_query(history: &[ChatEntry]) -> String {
    let mut s = String::new();
    for e in history {
        let who = match e.role { ChatRole::User => "User", ChatRole::Assistant => "Assistant" };
        s.push_str(who);
        s.push_str(": ");
        s.push_str(&e.text);
        s.push_str("\n\n");
    }
    s.push_str("Answer the most recent User question above, using the prior turns as context.");
    s
}
```

The transcript ends on an instruction anchoring kern to the latest question, so the answer LLM sees full context while the query still resolves to "the current question."

> **Known tradeoff (documented, accepted):** embedding the full transcript dilutes the retrieval query vector as the conversation grows — kern's HyDE/embed stage sees the whole buffer, not just the question. This is the explicit cost of "send everything for accuracy." If retrieval quality degrades on long sessions, a future revision can split retrieval (embed only the latest question) from synthesis (answer over the full buffer); out of scope here.

### 3b. Extract kern's prose `answer`, not the JSON blob

`kern_answer` parses the tool result as JSON and returns the `answer` field. The entities array is never surfaced in the chat.

```rust
async fn kern_answer(query: String) -> anyhow::Result<String> {
    // ... connect + call_tool("query", {"text": query, "k": 5, "answer": true}) ...
    let raw = extract_rpc_text(env); // concatenated text content = the JSON object
    // Parse and pull the synthesized prose answer.
    match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(v) => {
            if let Some(ans) = v.get("answer").and_then(|a| a.as_str()) {
                if !ans.trim().is_empty() {
                    return Ok(ans.to_string());
                }
            }
            // answer empty/absent → terse fallback from entity keys, NOT the raw JSON.
            let summary = v.get("entities")
                .and_then(|e| e.as_array())
                .map(|arr| summarize_entities(arr))
                .unwrap_or_default();
            if summary.is_empty() {
                anyhow::bail!("kern returned no answer");
            }
            Ok(summary)
        }
        // Not JSON (shouldn't happen) → return raw so we never silently swallow output.
        Err(_) => Ok(raw),
    }
}
```

`summarize_entities` builds a short human line (e.g. the top entity keys/labels) — a graceful degrade for `answer:true` calls that produced retrieval but no synthesis, never a JSON dump.

> **Evidence the `answer` is already prose:** `answer_prompt_from` (answer.rs) instructs the model *"Answer the question concisely using only the context above. Do not restate the context. Be direct,"* and `synthesize` returns the LLM output verbatim. The JSON the user sees is the **tool envelope** (`{"entities":…,"answer":…}`), not the answer. No change to kern's answer model/prompt is needed — this is purely a panel-side read/render fix.

### 3b-bis. Render prose with its structure intact

Extracting `answer` is necessary but not sufficient to "format it correctly": the shipped `wrap_text` runs `split_whitespace` over the entire string, collapsing newlines, paragraph breaks, and bullet lines into one run-on block. Replace it with a **line-preserving wrap**:

```rust
/// Word-wrap `text` to `width`, preserving the source line structure:
/// each '\n'-delimited line wraps independently; blank lines are kept as
/// blank rows. So multi-paragraph / bulleted kern answers render readably.
fn wrap_text_preserving(text: &str, width: usize) -> Vec<String> {
    if width == 0 { return vec![text.to_string()]; }
    let mut out = Vec::new();
    for src_line in text.split('\n') {
        if src_line.trim().is_empty() {
            out.push(String::new());          // keep paragraph break
            continue;
        }
        out.extend(wrap_one_line(src_line, width)); // existing greedy word-wrap, per line
    }
    if out.is_empty() { out.push(String::new()); }
    out
}
```

The existing greedy word-wrap becomes the per-line helper (`wrap_one_line`); `wrap_text_preserving` is the entry point used by `Assistant` rendering in `draw`. `User` entries stay single-line (right-aligned, truncated) as today.

### 3c. No haiku / `claude -p`

Explicitly out of scope. kern's `query(answer=true)` is the only LLM in this path.

---

## Section 4: Warning line — "sends whole convo" (revises original Section 5 layout)

A persistent **dim warning line sits at the very bottom row of the chat pane, below the input line.** It is always visible while the panel is shown.

```
row 0:   [status bar]
rows 1…(h-1):
┌─────────────────────┬──────────────────────┐
│  Chat history       │  Journal log         │
│  ─────────────────  │  (tail-follows)      │
│  ▶ user message     │  turn> …             │
│  assistant reply…   │  …                   │
│  ──────────────────  │                      │  divider
│  > input█           │                      │  input
│  sends whole convo ⚡│                      │  warning (dim)
└─────────────────────┴──────────────────────┘
```

**Layout change (left/chat pane):**
- `warn_row   = row_offset + pane_rows - 1`  (bottom row)
- `input_row  = warn_row - 1`
- `divider_row = input_row - 1`
- `history_rows = pane_rows - 3` (was `- 2`; the warning costs one row)

**Warning text:** dim (`Color::DarkGrey`), truncated to chat width:
`whole conversation is sent to kern for accuracy`
(or a `⚡`-prefixed compact form when width is tight). Static — same in every state.

The journal (right) pane is unchanged.

---

## Section 5: Tests (research.rs)

Updated / added unit tests:

- `chat_session_welcome_back_typing_continues` — printable char in `WelcomeBack` keeps history, → `Typing` (replaces the old reset-on-keystroke assertion).
- `chat_session_welcome_back_enter_resets` — `Enter` on empty input in `WelcomeBack` clears history → `Fresh`.
- `build_kern_query_includes_all_turns` — output contains every history entry and ends with the anchor instruction.
- `kern_answer_extracts_answer_field` — given `{"entities":[…],"answer":"hello"}`, returns `"hello"` (not the JSON).
- `kern_answer_falls_back_without_answer` — given entities but empty/no `answer`, returns a non-JSON summary, never the raw object.
- `wrap_text_preserving_keeps_paragraph_breaks` — input with `\n\n` and a bullet line wraps each line independently and retains the blank row (no run-on collapse).

`tui.rs` remains integration-level (no new unit tests); the correctness gates are `cargo test -p kern` + manual verification.

---

## What does NOT change

- `run_server()` / daemon code — untouched.
- kern MCP tools and the `query` tool's output schema — untouched (we change only how the panel *reads* it).
- `src/config/mux.rs` (`key_research` etc.) — untouched.
- `bincode`-derived structs — none touched.
- Journal tailer formatting + ring buffer — untouched.
- Version stays `1.0.0`.

---

## Manual verification

1. `Ctrl+L` opens the panel; ask a question → a **prose** answer appears (no JSON, no entities array).
2. Ask a follow-up ("what about X?") → answer reflects the earlier turn (whole buffer sent).
3. `Ctrl+L` to hide, `Ctrl+L` to show → history is still there; input shows `type to continue, enter to reset`.
4. Type a char on re-show → history preserved, cursor active (does NOT clear).
5. `Enter` on the empty re-show placeholder → history cleared (memory reset).
6. The dim `whole conversation is sent…` line is visible at the bottom whenever the panel is shown.
7. Hide the panel and confirm PTY panes resize/behave normally; the journal keeps tailing across hide/show.
