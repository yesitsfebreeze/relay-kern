# Kern `raise_question` + Waiting-Sessions Overlay — Design Spec (Spec B)

**Date:** 2026-06-12
**Status:** Approved (design); pending spec review
**Builds on:** Spec A (mux unification) — branch `feat/mux-unification`. Requires the in-process `Server.mux: Option<Arc<Mutex<PaneRegistry>>>` handle that Spec A added.
**Scope:** `src/mux/registry.rs` (question registry), `src/mcp/tools_mux.rs` (`raise_question` tool), `src/mcp.rs` (dispatch), `src/mux/questions.rs` (new — overlay), `src/mux/tui.rs` (Ctrl+K toggle + draw + status badge), `src/config/mux.rs` (`key_questions`), `src/mux/mod.rs` (wire).

---

## Overview

Agents running in mux panes sometimes need a human decision before they can continue. `raise_question` is a **blocking** MCP tool: the agent calls it, its turn parks, and the call returns once the human types an answer. The human sees every waiting agent in a **dedicated `Ctrl+K` overlay** — a roster of short descriptions ("who is asking what") — picks one, types a reply, and the blocked agent receives it as the tool result. A status-bar badge surfaces the pending count even when the overlay is closed.

Because Spec A made the mux *be* the kern daemon (one process, `Server.mux` reaches the live `PaneRegistry`), the question state is a plain in-process structure shared by the tool handler and the TUI — no IPC, no broker process.

---

## Section 1: Question Registry (shared state)

A new `QuestionRegistry` lives **inside `PaneRegistry`** (reusing the `Server.mux` handle — no new `Server` field, no `run_tui` signature change).

```rust
// src/mux/registry.rs
pub struct PendingQuestion {
    pub id:       String,        // short random id
    pub label:    String,        // agent-supplied "who" (e.g. "worker-1"); may be empty
    pub question: String,        // the ask, shown in the roster
    answer_tx:    std::sync::mpsc::Sender<String>,  // delivers the human's answer to the blocked tool
}

#[derive(Default)]
pub struct QuestionRegistry {
    pending: Vec<PendingQuestion>,   // insertion-ordered; small N
}

impl QuestionRegistry {
    /// Register a question; returns (id, receiver). The tool handler blocks on
    /// the receiver. Caller must NOT hold any lock while blocking.
    pub fn open(&mut self, label: String, question: String) -> (String, std::sync::mpsc::Receiver<String>);
    /// Roster view for the overlay: (id, label, question) per pending entry.
    pub fn list(&self) -> Vec<(String, String, String)>;
    /// Deliver an answer to question `id`: send on its channel, remove it.
    /// Returns false if `id` is unknown (already answered / dismissed).
    pub fn answer(&mut self, id: &str, answer: String) -> bool;
    /// Drop a question without answering (Esc/dismiss or pane reap).
    pub fn dismiss(&mut self, id: &str) -> bool;
    pub fn len(&self) -> usize;
}
```

`PaneRegistry` gains `pub questions: QuestionRegistry` (defaulted in `PaneRegistry::new`). The tool handler reaches it via `Server.mux`; the TUI via its own `registry` lock.

**Locking discipline (critical):** the handler locks the registry **only** to `open()` (insert + get receiver), then **releases the lock** and blocks on the receiver. It never holds the registry mutex while waiting — otherwise the TUI (which locks the registry every frame) would deadlock.

---

## Section 2: `raise_question` Tool

Added to `tools_mux.rs` (the comms-tools module), advertised + dispatched only when `Server.mux.is_some()` — identical gating to `delegate`/`collect`.

```
name: "raise_question"
inputSchema: { required: ["question"], properties: {
    question: string,   // the decision/answer the agent needs
    label:    string,   // optional: who's asking, for the roster (e.g. "worker-3")
}}
```

Handler (`Server::tool_raise_question`):
1. `let Some(reg) = self.mux.as_ref() else { return tool_error("not running under a mux") }`.
2. Lock registry → `questions.open(label, question)` → `(id, rx)` → **unlock**.
3. `let answer = rx.recv()` — **blocks** until the human answers (or the channel drops).
4. On `Ok(answer)` → `tool_result_json({ "answer": answer, "id": id })`.
   On `Err(_)` (sender dropped / dismissed) → `tool_error("question dismissed without an answer")`.

**Blocking semantics (the approved model):** the call returns the answer. The handler runs inside the kern_rpc `call_tool` async task; the synchronous `recv()` parks that task's worker thread for the wait's duration. Acceptable for the intended scale (a handful of concurrent human-gated agents); documented as a known trade-off, not a scalability path.

---

## Section 3: Timeout Risk (must verify in planning)

A human answer can take **minutes** — far longer than any existing call on the `kern mcp` proxy ↔ daemon `kern_rpc` path (the research chat's LLM answer is ~12–21 s). **The plan MUST verify the read timeout on:**
- the agent ↔ `kern mcp` proxy **stdio** MCP channel (Claude waits indefinitely for a tool result — expected fine), and
- the proxy ↔ daemon **`kern_rpc`** channel (`KernRpcClient` / the typed `Channel`).

If `kern_rpc` imposes a finite read timeout, a long human wait would error before the answer arrives. **Resolution if so:** give `raise_question` a bounded internal wait (`recv_timeout`, e.g. 10 min) that returns a structured `tool_error("no answer within 10m — call raise_question again to keep waiting")` so the agent can re-poll, rather than hanging the channel past its timeout. The exact bound is set once the channel timeout is known.

---

## Section 4: Overlay (`Ctrl+K`)

New module `src/mux/questions.rs` — mirrors the `ResearchPanel` (`Ctrl+L`) pattern so the two overlays are structurally consistent.

```rust
pub struct QuestionsOverlay {
    selected: usize,      // index into the roster
    input:    String,     // answer being typed
}
impl QuestionsOverlay {
    pub fn new() -> Self;
    /// Draw the roster + input line. Reads the live roster from `registry`.
    pub fn draw(&self, stdout, registry: &PaneRegistry, cols, rows) -> io::Result<()>;
    /// Handle a key. Returns an action: Close | Answer{id,text} | Dismiss{id} | None.
    pub fn handle_key(&mut self, kev, roster_len: usize) -> QuestionsAction;
}
```

**Layout** (full screen below the status bar, like the research panel):
```
row 0:  [status bar — shows ❓N badge when N>0]
        ┌───────────────────────────────────────────┐
        │ Waiting for you (3)                        │
        │ ▶ worker-1 · Should I force-push to main?  │  ← selected (bold)
        │   worker-3 · Which DB migration tool?      │
        │   audit    · Delete the orphaned shards?   │
        │ ───────────────────────────────────────── │
        │ answer ▸ _                                 │  ← input for the selected
        └───────────────────────────────────────────┘
```

**Keys:** `↑`/`↓` move selection · printable/Backspace edit the answer · `Enter` submit the answer to the selected question (→ `registry.questions.answer(id, text)`, clears input, selection clamps) · `Ctrl+D` dismiss the selected without answering · `Esc` close the overlay (questions stay pending). When the roster is empty the overlay shows "No agents are waiting." and `Enter` does nothing.

**Toggle/dispatch** in `run_tui` follows the existing `research: Option<…>` branch exactly: a `questions: Option<QuestionsOverlay>` field, toggled on `keymap.matches_questions`, drawn when `Some`, all keys routed to it while open (no PTY forwarding), redrawn each frame from the live registry.

---

## Section 5: Status-Bar Badge

`draw_status_bar` (or the inlined status bar in `draw_frame`) gains a `❓N` segment when `registry.questions.len() > 0`, so a human not currently in the overlay still sees that agents are blocked. Rendered in the existing right/mid status region; omitted when N==0.

---

## Section 6: Config

`src/config/mux.rs`: add `key_questions: String` (default `"ctrl+k"`), a `KeyMap::questions: KeyEvent`, and `matches_questions(&self, &KeyEvent) -> bool` — mirroring the `key_research`/`matches_research` pattern.

---

## Section 7: Edge Cases

- **Agent disconnects while waiting** (pane killed): the blocked `recv()` is on a channel whose sender lives in the registry, not the pane — so the question stays in the roster as an orphan. The human can `Ctrl+D` dismiss it. *Refinement (note, not v1-blocking):* reap pending questions whose `label` matches a pane that has exited, during `reap_exited`.
- **Answer to an unknown id** (already answered/dismissed): `answer()` returns false; the overlay just refreshes (no panic).
- **Multiple questions from one agent:** allowed; each is a distinct entry with its own channel. (An agent only blocks on one at a time in practice, since the call is synchronous.)
- **Empty `label`:** roster shows the question alone (no "who" prefix).

---

## What Does Not Change

- Spec A's surface (one process, `kern.sock`, comms tools, `list_tools`).
- `bincode` structs — none touched (`QuestionRegistry` is in-memory only, never persisted).
- The research panel (`Ctrl+L`) — untouched; the questions overlay is a sibling.
- Version stays `1.0.0`.

---

## Repo-Law Notes

- **Shared:** `QuestionRegistry` lives in `registry.rs` beside `PaneRegistry` (its only consumers are the mux tool handler + TUI — not shareable beyond the mux). ✓
- **Duplicates:** the overlay deliberately mirrors `ResearchPanel`'s structure but is a distinct concern (human-gated answers vs. LLM chat) — not a dup to fold. ✓
- **No compat:** `raise_question` is a clean addition; no shims. ✓
