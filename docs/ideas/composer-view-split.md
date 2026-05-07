# Composer / Ambient-Repl / View Split — Foundation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One repl process, N tabs. Every tab = view chrome + scoped transcript + composer. Composer is host chrome, always present. View is pluggable content above the transcript. Esc toggles focus composer↔view; triple-Esc closes the tab (home protected). Transcripts are tab-scoped sub-sessions, feed into shared `kern`, deletable. Ticket tabs attach to a live worker if one exists; spawn new otherwise.

**Architecture:** Today `ReplApp` (src/bin/repl/src/lib.rs) owns composer + transcript + repl-specific dispatch as one monolith. This plan splits it:
- `Host` owns composer, slot cache, tab vec, focus FSM, shared `Session` (registry/kern/queue/journal), and a per-tab `SubSession` factory.
- `Tab` owns a `Box<dyn View>`, a `SubSession` (scoped transcript + repl history + agent scope), and a preserved composer buffer.
- `View` trait declares the chrome above the transcript, the addressee a submit routes to, and the layout split (chrome vs transcript rows).
- Ship four view impls this plan: `HomeView` (migration target for current ReplApp body — 0% chrome, all transcript), `InboxView` (stub), plus the trait plumbing that future `BoardView` / `TicketView` will fill. `TicketView` + `BoardView` are deferred to follow-up plans but the hooks (`WorkerRegistry::attach_or_spawn`, `AgentScope`) land here so those plans are pure wiring.

Transcripts are ephemeral. `kern` is the source of truth. When a tab closes, the host offers to promote interesting turns to `kern` (deferred to plan #4); transcript bytes drop by default.

**Tech Stack:** Rust workspace. Re-uses `textarea::EditArea`, `render::{FrameView, Region, Renderer}`, `input::{Key, KeyCode, Mods}`, `ui_slots::{SlotCache, Layout, EditAreaBus}`, `agent::Session`, `journal::{DayJournal, History, StateHandle}`, `harness::Registry`. New modules under `src/bin/repl/src/host/` and `src/bin/repl/src/views/`. One new crate dep boundary: `WorkerRegistry` lives in `src/bin/repl/src/host/workers.rs` (internal; not a new crate).

**Scope (in):**
- `Host` shell that owns composer, slots, tabs, focus FSM, shared `Session`, `WorkerRegistry`.
- `View` trait; `AgentScope` descriptor; `SubmitOutcome`.
- `SubSession` — per-tab transcript + repl history + agent scope + sub-journal lane.
- `HomeView` — migrates current `ReplApp` body. Chrome=0%, transcript=all.
- `InboxView` — stub. Chrome=70%, transcript=30%. Renders "no pending approvals".
- Esc FSM: composer↔view toggle, triple-Esc closes tab (home protected).
- `/inbox`, `/home` switch to singleton tab (reuse if open).
- `/tab new <view_id>` spawns additional tab of same view kind, independent `InstanceId`.
- `WorkerRegistry::attach_or_spawn(ticket_id)` — returns handle; stubbed body (no ticket workers yet) but the API shape lands.
- Prompt glyph + focus indicator driven by active tab's `AgentScope`.

**Scope (out — deferred):**
- BoardView, TicketView, orchestrator integration (plan #2 — board migration).
- Extracting shared primitives (List, Picker, Form, Tabs, Card) into `ui_kit` (plan #3 — primitives).
- Queue integration, trust tiers, cross-tab notifications (plan #4 — approvals).
- Curator recipe, budget ceiling (plan #5 — guardrails).
- Transcript-to-kern promotion on tab close.

---

## File Structure

**Create:**
- `src/bin/repl/src/host/mod.rs` — `Host` struct; top-level render + dispatch.
- `src/bin/repl/src/host/view.rs` — `View` trait, `AgentScope`, `SubmitOutcome`, `ViewEvent`, `LayoutSplit`.
- `src/bin/repl/src/host/tab.rs` — `Tab`, `InstanceId`, per-tab composer buffer.
- `src/bin/repl/src/host/focus.rs` — focus FSM, Esc ladder.
- `src/bin/repl/src/host/subsession.rs` — `SubSession`: transcript + repl history + scope + journal lane.
- `src/bin/repl/src/host/workers.rs` — `WorkerRegistry`: attach-or-spawn by ticket id.
- `src/bin/repl/src/views/mod.rs` — re-exports.
- `src/bin/repl/src/views/home.rs` — `HomeView` (migration target).
- `src/bin/repl/src/views/inbox.rs` — `InboxView` stub.

**Modify:**
- `src/bin/repl/src/lib.rs` — add `pub mod host; pub mod views;`. Keep `ReplApp` as shim over `Host` until dependents migrate.
- `src/bin/repl/src/main.rs` — construct `Host` via `Host::new(session)`.
- `src/bin/repl/src/slash.rs` — add `/inbox`, `/home`, `/tab` variants.

**Test:**
- `src/bin/repl/tests/host_focus.rs` — Esc toggle, triple-Esc close, home protected.
- `src/bin/repl/tests/host_tabs.rs` — composer buffer preservation, singleton-vs-duplicate semantics.
- `src/bin/repl/tests/host_submit.rs` — submit routing by scope, `/inbox` + `/tab` semantics.
- `src/bin/repl/tests/host_workers.rs` — attach returns existing, spawn when absent.

---

## Task 1: View trait, AgentScope, SubmitOutcome, LayoutSplit

**Files:**
- Create: `src/bin/repl/src/host/view.rs`

- [ ] **Step 1: Write the failing test**

Inline in `host/view.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_scope_prompt_hint_comes_from_view_id() {
        let s = AgentScope {
            view_id: "inbox".into(),
            instance: InstanceId(3),
            context_ref: None,
            recipe: "approvals".into(),
        };
        assert_eq!(s.prompt_hint(), "inbox>");
    }

    #[test]
    fn layout_split_ratio_rounds_to_rows() {
        assert_eq!(LayoutSplit::chrome_ratio(0.0).rows(10), (0, 10));
        assert_eq!(LayoutSplit::chrome_ratio(0.7).rows(10), (7, 3));
        assert_eq!(LayoutSplit::chrome_ratio(1.0).rows(10), (10, 0));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kern --lib host::view::tests`
Expected: FAIL — unresolved types.

- [ ] **Step 3: Write minimal implementation**

`src/bin/repl/src/host/view.rs`:

```rust
//! View trait — pluggable chrome above the transcript. The host owns
//! composer + slots + focus + session; a view owns what renders in its
//! chrome region, how submits are interpreted, and the layout split
//! between chrome and transcript.

use render::{FrameView, Region};

/// Stable identifier for a tab instance. Two tabs of the same view
/// kind (two `inbox` tabs, two `ticket:X` tabs) share `view_id` but
/// carry distinct `InstanceId`s so workers and transcripts stay apart.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct InstanceId(pub u64);

/// Describes the agent a tab's composer submits to. Carried by the
/// view; copied into the tab's `SubSession`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentScope {
    /// View kind: "home" | "inbox" | "board" | "ticket" | ...
    pub view_id: String,
    /// Per-tab instance id — distinguishes two tabs of the same kind.
    pub instance: InstanceId,
    /// Optional anchor in the kern graph this conversation is pinned to.
    pub context_ref: Option<String>,
    /// Recipe id the agent runs under for this scope.
    pub recipe: String,
}

impl AgentScope {
    /// Glyph rendered before the composer's `>`. Distinguishes scope
    /// at a glance. Format: `{view_id}>`.
    pub fn prompt_hint(&self) -> String {
        format!("{}>", self.view_id)
    }
}

/// Non-input event a view receives.
#[derive(Debug)]
pub enum ViewEvent<'a> {
    FocusGained,
    FocusLost,
    Key(&'a input::Key),
}

/// What the view wants after a submit.
#[derive(Debug)]
pub enum SubmitOutcome {
    /// View handled the effect itself — do nothing else.
    Consumed,
    /// Route text into this tab's agent loop as a repl turn.
    DispatchTurn { text: String },
    /// Route text to the approvals subsystem (plan #4).
    DispatchApproval { text: String },
    /// Switch to an existing tab by view_id; create if absent.
    SwitchTab { view_id: String },
    /// Open a fresh instance of the named view kind in a new tab.
    NewTab { view_id: String },
    /// View refused the submit — surface `reason` as a system line.
    Reject { reason: String },
}

/// Declarative split of the view's vertical real estate between its
/// chrome region (view-rendered) and the transcript region (host-
/// rendered). Rendered top-to-bottom: chrome, transcript, then the
/// composer slots below.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct LayoutSplit {
    /// Fraction of rows allocated to the chrome. Clamped to [0.0, 1.0].
    chrome: f32,
}

impl LayoutSplit {
    pub fn chrome_ratio(chrome: f32) -> Self {
        Self { chrome: chrome.clamp(0.0, 1.0) }
    }

    /// Turn the ratio into concrete row counts summing to `total`.
    /// Chrome takes `floor(total * chrome)`; transcript gets the rest.
    pub fn rows(self, total: u16) -> (u16, u16) {
        let chrome = ((total as f32) * self.chrome).floor() as u16;
        let chrome = chrome.min(total);
        (chrome, total - chrome)
    }
}

/// Pluggable tab body above the transcript.
pub trait View: Send {
    fn id(&self) -> &str;
    fn title(&self) -> &str;
    /// Agent scope. Host reads this every tick; changes between calls
    /// are honoured (e.g., ticket view swaps `context_ref` when the
    /// selected ticket changes).
    fn scope(&self) -> AgentScope;
    fn layout(&self) -> LayoutSplit;
    /// Paint the chrome region. Transcript is painted separately by
    /// the host.
    fn paint_chrome(&self, frame: &mut FrameView<'_>, region: Region);
    fn on_event(&mut self, event: ViewEvent<'_>) -> bool;
    fn on_submit(&mut self, text: &str) -> SubmitOutcome;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_scope_prompt_hint_comes_from_view_id() {
        let s = AgentScope {
            view_id: "inbox".into(),
            instance: InstanceId(3),
            context_ref: None,
            recipe: "approvals".into(),
        };
        assert_eq!(s.prompt_hint(), "inbox>");
    }

    #[test]
    fn layout_split_ratio_rounds_to_rows() {
        assert_eq!(LayoutSplit::chrome_ratio(0.0).rows(10), (0, 10));
        assert_eq!(LayoutSplit::chrome_ratio(0.7).rows(10), (7, 3));
        assert_eq!(LayoutSplit::chrome_ratio(1.0).rows(10), (10, 0));
    }
}
```

Wire `pub mod host;` into `src/bin/repl/src/lib.rs` and `pub mod view;` into a minimal `host/mod.rs`:

```rust
// host/mod.rs
pub mod view;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p kern --lib host::view::tests`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add src/bin/repl/src/host/ src/bin/repl/src/lib.rs
git commit -m "feat(repl/host): View trait + AgentScope + LayoutSplit"
```

---

## Task 2: Focus FSM — Esc toggle + triple-Esc close

**Files:**
- Create: `src/bin/repl/src/host/focus.rs`

Identical in shape to the previous revision. The FSM is view-agnostic; the host enforces the home-protection rule when applying `CloseView`.

- [ ] **Step 1: Write the failing test**

Inline:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esc_toggles_composer_and_view() {
        let mut fsm = FocusFsm::new();
        assert_eq!(fsm.focus(), Focus::Composer);
        assert_eq!(fsm.press_esc(), EscOutcome::ToggledToView);
        assert_eq!(fsm.focus(), Focus::View);
        assert_eq!(fsm.press_esc(), EscOutcome::ToggledToComposer);
        assert_eq!(fsm.focus(), Focus::Composer);
    }

    #[test]
    fn third_esc_closes_view() {
        let mut fsm = FocusFsm::new();
        fsm.press_esc();
        fsm.press_esc();
        assert_eq!(fsm.press_esc(), EscOutcome::CloseView);
    }

    #[test]
    fn non_esc_key_resets_counter() {
        let mut fsm = FocusFsm::new();
        fsm.press_esc();
        fsm.press_esc();
        fsm.note_non_esc();
        assert_eq!(fsm.press_esc(), EscOutcome::ToggledToView);
    }
}
```

- [ ] **Step 2: Run — expected FAIL**

Run: `cargo test -p kern --lib host::focus::tests`

- [ ] **Step 3: Implement**

`src/bin/repl/src/host/focus.rs`:

```rust
//! Focus FSM — composer ↔ view toggle on Esc; triple-Esc returns
//! CloseView. Non-Esc keys reset the ladder.

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Focus { Composer, View }

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EscOutcome { ToggledToView, ToggledToComposer, CloseView }

#[derive(Debug)]
pub struct FocusFsm {
    focus: Focus,
    esc_run: u8,
}

impl FocusFsm {
    pub fn new() -> Self { Self { focus: Focus::Composer, esc_run: 0 } }
    pub fn focus(&self) -> Focus { self.focus }

    pub fn press_esc(&mut self) -> EscOutcome {
        self.esc_run = self.esc_run.saturating_add(1);
        match (self.focus, self.esc_run) {
            (Focus::Composer, 1) => {
                self.focus = Focus::View;
                EscOutcome::ToggledToView
            }
            (Focus::View, 2) => {
                self.focus = Focus::Composer;
                EscOutcome::ToggledToComposer
            }
            (Focus::Composer, 3) => {
                self.esc_run = 0;
                EscOutcome::CloseView
            }
            _ => {
                self.esc_run = 0;
                EscOutcome::CloseView
            }
        }
    }

    pub fn note_non_esc(&mut self) { self.esc_run = 0; }
    pub fn reset_to_composer(&mut self) {
        self.focus = Focus::Composer;
        self.esc_run = 0;
    }
}

impl Default for FocusFsm {
    fn default() -> Self { Self::new() }
}
```

Add `pub mod focus;` to `host/mod.rs`.

- [ ] **Step 4: Run — expected PASS**

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(repl/host): focus FSM with Esc toggle + triple-Esc close"
```

---

## Task 3: SubSession — per-tab transcript + repl history + scope

**Files:**
- Create: `src/bin/repl/src/host/subsession.rs`

- [ ] **Step 1: Write the failing test**

Inline:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::view::{AgentScope, InstanceId};

    fn scope() -> AgentScope {
        AgentScope {
            view_id: "home".into(),
            instance: InstanceId(1),
            context_ref: None,
            recipe: "repl".into(),
        }
    }

    #[test]
    fn transcript_starts_empty_and_appends() {
        let mut s = SubSession::new(scope());
        assert_eq!(s.transcript().len(), 0);
        s.push_user("hi".into());
        s.push_assistant("hello".into());
        assert_eq!(s.transcript().len(), 2);
    }

    #[test]
    fn surf_history_caps_at_max() {
        let mut s = SubSession::with_cap(scope(), 4);
        for i in 0..10 {
            s.push_user(format!("u{i}"));
            s.push_assistant(format!("a{i}"));
        }
        // Rolling cap evicts oldest pairs first.
        assert!(s.transcript().len() <= 4);
    }
}
```

- [ ] **Step 2: Run — expected FAIL**

Run: `cargo test -p kern --lib host::subsession::tests`

- [ ] **Step 3: Implement**

`src/bin/repl/src/host/subsession.rs`:

```rust
//! SubSession — per-tab transcript + repl history + agent scope.
//! Transcripts are ephemeral; on tab close the host may promote
//! interesting turns to kern (wired in plan #4), then drop bytes.
//!
//! The shared `Session` (registry, kern handle, queue, journal)
//! stays on the `Host`; `SubSession` only holds tab-local state.

use crate::host::view::AgentScope;
use crate::repl_view::{ReplMessage, Role};

pub struct SubSession {
    scope: AgentScope,
    transcript: Vec<ReplMessage>,
    /// Rolling cap on transcript length. Oldest messages are dropped
    /// when the cap is exceeded.
    cap: usize,
}

impl SubSession {
    pub fn new(scope: AgentScope) -> Self {
        Self::with_cap(scope, crate::MAX_HISTORY_MESSAGES)
    }
    pub fn with_cap(scope: AgentScope, cap: usize) -> Self {
        Self { scope, transcript: Vec::new(), cap }
    }
    pub fn scope(&self) -> &AgentScope { &self.scope }
    pub fn transcript(&self) -> &[ReplMessage] { &self.transcript }

    pub fn push_user(&mut self, text: String) {
        self.push(ReplMessage { role: Role::User, body: text });
    }
    pub fn push_assistant(&mut self, text: String) {
        self.push(ReplMessage { role: Role::Assistant, body: text });
    }
    pub fn push_system(&mut self, text: String) {
        self.push(ReplMessage { role: Role::System, body: text });
    }

    fn push(&mut self, msg: ReplMessage) {
        self.transcript.push(msg);
        while self.transcript.len() > self.cap {
            self.transcript.remove(0);
        }
    }
}
```

Make `ReplMessage`/`Role` fields pub where needed, or use existing ctors — verify via `grep -n 'pub struct ReplMessage\|pub enum Role' src/bin/repl/src/repl_view.rs` before using struct-literal syntax. If ctors like `ReplMessage::user(text)` exist, prefer those.

Add `pub mod subsession;` to `host/mod.rs`.

- [ ] **Step 4: Run — expected PASS**

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(repl/host): SubSession with scoped transcript + cap"
```

---

## Task 4: Tab — view + subsession + composer buffer

**Files:**
- Create: `src/bin/repl/src/host/tab.rs`

- [ ] **Step 1: Failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::view::{AgentScope, InstanceId, LayoutSplit, SubmitOutcome, View, ViewEvent};
    use render::{FrameView, Region};

    struct Stub { id: &'static str }
    impl View for Stub {
        fn id(&self) -> &str { self.id }
        fn title(&self) -> &str { self.id }
        fn scope(&self) -> AgentScope {
            AgentScope {
                view_id: self.id.into(),
                instance: InstanceId(0),
                context_ref: None,
                recipe: "repl".into(),
            }
        }
        fn layout(&self) -> LayoutSplit { LayoutSplit::chrome_ratio(0.0) }
        fn paint_chrome(&self, _: &mut FrameView<'_>, _: Region) {}
        fn on_event(&mut self, _: ViewEvent<'_>) -> bool { false }
        fn on_submit(&mut self, _: &str) -> SubmitOutcome { SubmitOutcome::Consumed }
    }

    #[test]
    fn tab_holds_view_subsession_and_buffer() {
        let mut tab = Tab::new(Box::new(Stub { id: "home" }));
        assert_eq!(tab.view().id(), "home");
        tab.set_composer_snapshot("hello".into());
        assert_eq!(tab.composer_snapshot(), "hello");
        tab.subsession_mut().push_user("hi".into());
        assert_eq!(tab.subsession().transcript().len(), 1);
    }
}
```

- [ ] **Step 2: Run — expected FAIL**

- [ ] **Step 3: Implement**

`src/bin/repl/src/host/tab.rs`:

```rust
//! Tab — one view + its per-tab SubSession + stashed composer buffer.

use crate::host::subsession::SubSession;
use crate::host::view::View;

pub struct Tab {
    view: Box<dyn View>,
    subsession: SubSession,
    composer_snapshot: String,
}

impl Tab {
    pub fn new(view: Box<dyn View>) -> Self {
        let scope = view.scope();
        Self {
            subsession: SubSession::new(scope),
            view,
            composer_snapshot: String::new(),
        }
    }
    pub fn view(&self) -> &dyn View { &*self.view }
    pub fn view_mut(&mut self) -> &mut dyn View { &mut *self.view }
    pub fn view_id(&self) -> &str { self.view.id() }
    pub fn subsession(&self) -> &SubSession { &self.subsession }
    pub fn subsession_mut(&mut self) -> &mut SubSession { &mut self.subsession }
    pub fn composer_snapshot(&self) -> &str { &self.composer_snapshot }
    pub fn set_composer_snapshot(&mut self, text: String) { self.composer_snapshot = text; }
}
```

Add `pub mod tab;` to `host/mod.rs`.

- [ ] **Step 4: Run — expected PASS**

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(repl/host): Tab owns view + SubSession + composer buffer"
```

---

## Task 5: WorkerRegistry — attach-or-spawn

**Files:**
- Create: `src/bin/repl/src/host/workers.rs`
- Create: `src/bin/repl/tests/host_workers.rs`

This task lands the API shape the ticket view will call. Body is stubbed — real worker spawn lives in the orchestrator (plan #2). The registry tracks string keys and returns handle ids.

- [ ] **Step 1: Integration test**

`src/bin/repl/tests/host_workers.rs`:

```rust
use repl::host::workers::{WorkerHandle, WorkerRegistry};

#[test]
fn attach_returns_same_handle_for_same_key() {
    let mut reg = WorkerRegistry::new();
    let WorkerHandle { id: a, spawned: spawned_a } = reg.attach_or_spawn("ticket/RELAY-12");
    let WorkerHandle { id: b, spawned: spawned_b } = reg.attach_or_spawn("ticket/RELAY-12");
    assert_eq!(a, b);
    assert!(spawned_a);
    assert!(!spawned_b);
}

#[test]
fn distinct_keys_get_distinct_handles() {
    let mut reg = WorkerRegistry::new();
    let a = reg.attach_or_spawn("ticket/RELAY-12").id;
    let b = reg.attach_or_spawn("ticket/RELAY-13").id;
    assert_ne!(a, b);
}
```

- [ ] **Step 2: Run — expected FAIL**

Run: `cargo test -p kern --test host_workers`

- [ ] **Step 3: Implement**

`src/bin/repl/src/host/workers.rs`:

```rust
//! WorkerRegistry — attach-or-spawn by ticket key.
//!
//! Callers (typically a ticket view) ask the registry for a worker
//! handle given a stable key (e.g. "ticket/RELAY-12"). If a worker is
//! already running under that key, return its handle; otherwise
//! register a new one and mark it spawned. Actual worker bodies are
//! the orchestrator's concern (plan #2); this registry only tracks
//! ownership.

use std::collections::HashMap;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct WorkerId(pub u64);

pub struct WorkerHandle {
    pub id: WorkerId,
    /// True if this call caused the worker to register (i.e. no prior
    /// entry existed). False when an existing worker was returned.
    pub spawned: bool,
}

pub struct WorkerRegistry {
    by_key: HashMap<String, WorkerId>,
    next_id: u64,
}

impl WorkerRegistry {
    pub fn new() -> Self {
        Self { by_key: HashMap::new(), next_id: 1 }
    }

    pub fn attach_or_spawn(&mut self, key: &str) -> WorkerHandle {
        if let Some(&id) = self.by_key.get(key) {
            return WorkerHandle { id, spawned: false };
        }
        let id = WorkerId(self.next_id);
        self.next_id += 1;
        self.by_key.insert(key.to_string(), id);
        WorkerHandle { id, spawned: true }
    }

    pub fn forget(&mut self, key: &str) {
        self.by_key.remove(key);
    }
}

impl Default for WorkerRegistry {
    fn default() -> Self { Self::new() }
}
```

Expose `pub mod workers;` from `host/mod.rs` and `pub use host::workers;` from `lib.rs` so the test can `use repl::host::workers::{...}`.

- [ ] **Step 4: Run — expected PASS**

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(repl/host): WorkerRegistry::attach_or_spawn"
```

---

## Task 6: InboxView stub

**Files:**
- Create: `src/bin/repl/src/views/mod.rs`
- Create: `src/bin/repl/src/views/inbox.rs`

- [ ] **Step 1: Failing test**

Inline in `views/inbox.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::view::{LayoutSplit, View};

    #[test]
    fn inbox_view_identifies_and_splits() {
        let v = InboxView::new();
        assert_eq!(v.id(), "inbox");
        assert_eq!(v.scope().view_id, "inbox");
        assert_eq!(v.layout(), LayoutSplit::chrome_ratio(0.7));
    }
}
```

- [ ] **Step 2: Run — expected FAIL**

Run: `cargo test -p kern --lib views::inbox::tests`

- [ ] **Step 3: Implement**

`src/bin/repl/src/views/mod.rs`:

```rust
pub mod home;
pub mod inbox;
```

`src/bin/repl/src/views/inbox.rs`:

```rust
//! Inbox view — approvals queue stub. Chrome 70% / transcript 30%.
//! Approvals wiring lands in plan #4.

use crate::host::view::{AgentScope, InstanceId, LayoutSplit, SubmitOutcome, View, ViewEvent};
use render::{FrameView, Region};

pub struct InboxView {
    instance: InstanceId,
}

impl InboxView {
    pub fn new() -> Self { Self::with_instance(InstanceId(0)) }
    pub fn with_instance(instance: InstanceId) -> Self {
        Self { instance }
    }
}

impl Default for InboxView {
    fn default() -> Self { Self::new() }
}

impl View for InboxView {
    fn id(&self) -> &str { "inbox" }
    fn title(&self) -> &str { "inbox" }
    fn scope(&self) -> AgentScope {
        AgentScope {
            view_id: "inbox".into(),
            instance: self.instance,
            context_ref: None,
            recipe: "approvals".into(),
        }
    }
    fn layout(&self) -> LayoutSplit { LayoutSplit::chrome_ratio(0.7) }
    fn paint_chrome(&self, frame: &mut FrameView<'_>, region: Region) {
        // One-line placeholder. Real list render lands in plan #4.
        let _ = (frame, region);
    }
    fn on_event(&mut self, _event: ViewEvent<'_>) -> bool { false }
    fn on_submit(&mut self, _text: &str) -> SubmitOutcome {
        SubmitOutcome::Reject { reason: "inbox submits wired in plan #4".into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::view::{LayoutSplit, View};

    #[test]
    fn inbox_view_identifies_and_splits() {
        let v = InboxView::new();
        assert_eq!(v.id(), "inbox");
        assert_eq!(v.scope().view_id, "inbox");
        assert_eq!(v.layout(), LayoutSplit::chrome_ratio(0.7));
    }
}
```

Note the real `paint_chrome` body is intentionally trivial (a single `let _ = ...` line). The render crate's text-draw method differs between versions; use whatever `views::home` ends up using in Task 8 once consistent.

Add `pub mod views;` to `lib.rs`.

- [ ] **Step 4: Run — expected PASS**

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(repl/views): InboxView stub"
```

---

## Task 7: HomeView scaffold (identity only)

Scope limit: this lands the `HomeView` trait impl with an empty body so Host can compile with two views. The ReplApp body migration into HomeView is Task 9 — heavy refactor, separate commit.

**Files:**
- Create: `src/bin/repl/src/views/home.rs`

- [ ] **Step 1: Failing test**

Inline:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::view::{LayoutSplit, View};

    #[test]
    fn home_view_takes_full_transcript() {
        let v = HomeView::new();
        assert_eq!(v.id(), "home");
        assert_eq!(v.scope().view_id, "home");
        assert_eq!(v.layout(), LayoutSplit::chrome_ratio(0.0));
    }
}
```

- [ ] **Step 2: Run — expected FAIL**

Run: `cargo test -p kern --lib views::home::tests`

- [ ] **Step 3: Implement scaffolding only**

`src/bin/repl/src/views/home.rs`:

```rust
//! Home view — project-scoped repl. Chrome 0% / transcript 100%.
//! Task 9 migrates the legacy ReplApp dispatch + scrollback body into
//! this file. For now it's a scaffold that identifies itself and
//! returns `DispatchTurn` for every submit.

use crate::host::view::{AgentScope, InstanceId, LayoutSplit, SubmitOutcome, View, ViewEvent};
use render::{FrameView, Region};

pub struct HomeView {
    instance: InstanceId,
}

impl HomeView {
    pub fn new() -> Self { Self::with_instance(InstanceId(0)) }
    pub fn with_instance(instance: InstanceId) -> Self { Self { instance } }
}

impl Default for HomeView {
    fn default() -> Self { Self::new() }
}

impl View for HomeView {
    fn id(&self) -> &str { "home" }
    fn title(&self) -> &str { "home" }
    fn scope(&self) -> AgentScope {
        AgentScope {
            view_id: "home".into(),
            instance: self.instance,
            context_ref: None,
            recipe: "repl".into(),
        }
    }
    fn layout(&self) -> LayoutSplit { LayoutSplit::chrome_ratio(0.0) }
    fn paint_chrome(&self, _: &mut FrameView<'_>, _: Region) {}
    fn on_event(&mut self, _: ViewEvent<'_>) -> bool { false }
    fn on_submit(&mut self, text: &str) -> SubmitOutcome {
        SubmitOutcome::DispatchTurn { text: text.to_string() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::view::{LayoutSplit, View};

    #[test]
    fn home_view_takes_full_transcript() {
        let v = HomeView::new();
        assert_eq!(v.id(), "home");
        assert_eq!(v.scope().view_id, "home");
        assert_eq!(v.layout(), LayoutSplit::chrome_ratio(0.0));
    }
}
```

- [ ] **Step 4: Run — expected PASS**

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(repl/views): HomeView scaffold"
```

---

## Task 8: Host shell — tabs, composer round-trip, focus dispatch

**Files:**
- Modify: `src/bin/repl/src/host/mod.rs`
- Create: `src/bin/repl/tests/host_tabs.rs`
- Create: `src/bin/repl/tests/host_focus.rs`

- [ ] **Step 1: Failing tests**

`src/bin/repl/tests/host_tabs.rs`:

```rust
use repl::host::Host;
use repl::views::{home::HomeView, inbox::InboxView};

#[test]
fn switch_tab_preserves_composer_buffer() {
    let mut host = Host::headless();
    host.add_tab(Box::new(HomeView::new()));
    host.add_tab(Box::new(InboxView::new()));

    host.set_focus_tab_by_id("home");
    host.set_composer_text("draft reply");
    host.set_focus_tab_by_id("inbox");
    assert_eq!(host.composer_text(), "");

    host.set_composer_text("y 1");
    host.set_focus_tab_by_id("home");
    assert_eq!(host.composer_text(), "draft reply");

    host.set_focus_tab_by_id("inbox");
    assert_eq!(host.composer_text(), "y 1");
}
```

`src/bin/repl/tests/host_focus.rs`:

```rust
use repl::host::focus::Focus;
use repl::host::Host;
use repl::views::{home::HomeView, inbox::InboxView};

fn esc() -> input::Key {
    // Match the real ctor — grep `src/input/src/lib.rs` for the
    // actual constructor. Common options: `Key::press(KeyCode::Esc, Mods::NONE)`
    // or a struct literal. If the field set below does not match, update
    // before running.
    input::Key {
        code: input::KeyCode::Esc,
        mods: input::Mods::NONE,
        kind: input::KeyEventKind::Press,
    }
}

#[test]
fn esc_toggles_focus_in_any_view() {
    let mut host = Host::headless();
    host.add_tab(Box::new(HomeView::new()));
    host.add_tab(Box::new(InboxView::new()));
    host.set_focus_tab_by_id("inbox");
    assert_eq!(host.focus(), Focus::Composer);
    host.handle_key(&esc());
    assert_eq!(host.focus(), Focus::View);
    host.handle_key(&esc());
    assert_eq!(host.focus(), Focus::Composer);
}

#[test]
fn triple_esc_closes_non_home_tab() {
    let mut host = Host::headless();
    host.add_tab(Box::new(HomeView::new()));
    host.add_tab(Box::new(InboxView::new()));
    host.set_focus_tab_by_id("inbox");
    host.handle_key(&esc());
    host.handle_key(&esc());
    host.handle_key(&esc());
    assert_eq!(host.active_view_id(), Some("home"));
}

#[test]
fn home_tab_is_not_closable() {
    let mut host = Host::headless();
    host.add_tab(Box::new(HomeView::new()));
    host.set_focus_tab_by_id("home");
    host.handle_key(&esc());
    host.handle_key(&esc());
    host.handle_key(&esc());
    assert_eq!(host.active_view_id(), Some("home"));
}
```

- [ ] **Step 2: Run — expected FAIL**

Run: `cargo test -p kern --test host_tabs --test host_focus`

- [ ] **Step 3: Implement Host**

`src/bin/repl/src/host/mod.rs`:

```rust
//! Host shell — owns composer, slot cache, tabs vec, focus FSM,
//! shared session handles, and a worker registry.
//!
//! This file ships the headless subset in Task 8. Task 9 widens the
//! constructor to accept a real Session. Task 11 wires the bin entry.

pub mod focus;
pub mod subsession;
pub mod tab;
pub mod view;
pub mod workers;

use focus::{EscOutcome, Focus, FocusFsm};
use tab::Tab;
use view::View;
use workers::WorkerRegistry;

use textarea::EditArea;

/// View id that is never closable by the Esc ladder. The home tab is
/// the user's project-scoped repl; closing it would leave the host
/// tab-less.
pub const HOME_VIEW_ID: &str = "home";

pub struct Host {
    composer: EditArea,
    tabs: Vec<Tab>,
    active: usize,
    fsm: FocusFsm,
    #[allow(dead_code)]
    workers: WorkerRegistry,
}

impl Host {
    pub fn headless() -> Self {
        Self {
            composer: EditArea::new(),
            tabs: Vec::new(),
            active: 0,
            fsm: FocusFsm::new(),
            workers: WorkerRegistry::new(),
        }
    }

    pub fn add_tab(&mut self, view: Box<dyn View>) {
        self.tabs.push(Tab::new(view));
    }

    pub fn set_focus_tab_by_id(&mut self, view_id: &str) -> bool {
        let Some(target) = self.tabs.iter().position(|t| t.view_id() == view_id) else {
            return false;
        };
        self.switch_to(target);
        true
    }

    fn switch_to(&mut self, target: usize) {
        if target >= self.tabs.len() { return; }
        if self.active < self.tabs.len() {
            let outgoing = self.composer.text();
            self.tabs[self.active].set_composer_snapshot(outgoing);
        }
        self.active = target;
        let incoming = self.tabs[self.active].composer_snapshot().to_string();
        self.composer.set_text(&incoming);
        self.fsm.reset_to_composer();
    }

    pub fn set_composer_text(&mut self, text: &str) { self.composer.set_text(text); }
    pub fn composer_text(&self) -> String { self.composer.text() }
    pub fn focus(&self) -> Focus { self.fsm.focus() }
    pub fn active_view_id(&self) -> Option<&str> {
        self.tabs.get(self.active).map(|t| t.view_id())
    }

    pub fn handle_key(&mut self, key: &input::Key) -> bool {
        // Only bare Esc in the composer's plain Edit mode reaches the
        // FSM. List/Form/Banner modes eat Esc themselves.
        if key.code == input::KeyCode::Esc && !self.composer.is_in_mode() {
            match self.fsm.press_esc() {
                EscOutcome::ToggledToView | EscOutcome::ToggledToComposer => return true,
                EscOutcome::CloseView => {
                    self.close_active_tab();
                    return true;
                }
            }
        }
        if key.code != input::KeyCode::Esc {
            self.fsm.note_non_esc();
        }
        match self.fsm.focus() {
            Focus::Composer => {
                self.composer.on_key(key);
                false
            }
            Focus::View => {
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    tab.view_mut().on_event(view::ViewEvent::Key(key));
                }
                false
            }
        }
    }

    pub fn close_active_tab(&mut self) {
        if self.tabs.len() <= 1 { return; }
        if self.tabs[self.active].view_id() == HOME_VIEW_ID { return; }
        self.tabs.remove(self.active);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len().saturating_sub(1);
        }
        let incoming = self.tabs[self.active].composer_snapshot().to_string();
        self.composer.set_text(&incoming);
        self.fsm.reset_to_composer();
    }
}
```

Add `pub use host::Host;` and the `views` re-exports to `lib.rs`:

```rust
pub mod host;
pub mod views;
```

Add `EditArea::is_in_mode` (or equivalent) to `textarea`:

```rust
// src/textarea/src/lib.rs
pub fn is_in_mode(&self) -> bool {
    self.is_in_list() || self.is_in_form() || self.is_in_banner()
}
```

Match the real method names — grep `is_in_` in `textarea` first. Banner may live elsewhere (`intro` plugin owns a banner; the EditArea's banner mode is a separate thing). Only include the ones that exist.

- [ ] **Step 4: Run — expected PASS**

Run: `cargo test -p kern --test host_tabs --test host_focus`

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(repl/host): Host shell with tabs + Esc-driven focus"
```

---

## Task 9: Migrate ReplApp body into HomeView

The heaviest task. Goal: take the current `ReplApp` in `src/bin/repl/src/lib.rs` (≈2500 lines) and move repl-specific dispatch, transcript rendering, and composer-mode glue into `HomeView`, behind a `ReplApp` shim that wraps `Host` + one `HomeView`. The entire existing test suite must still pass at the end.

**Files:**
- Modify: `src/bin/repl/src/lib.rs`
- Expand: `src/bin/repl/src/views/home.rs`

- [ ] **Step 1: Baseline**

Run: `cargo test -p kern`
Capture pass count and output. Save to `docs/superpowers/plans/2026-04-24-composer-view-split.progress.md` alongside commit SHA.

- [ ] **Step 2: Classify ReplApp fields**

Grep `pub struct ReplApp {` (around line 1178) and classify each field:

Stays on `Host`:
- `composer: EditArea`
- `theme: StyleSet`
- `slot_cache`, `slot_config`, `slot_config_path`, `slot_config_mtime`
- `max_composer_rows`, `max_history_messages`
- `hook_runner`
- `edit_area_bus`
- `intro: IntroHandle`

Stays on `Host` but shared behind `Arc<Mutex<_>>`:
- `session: Arc<Mutex<Session>>`
- `day_journal`, `history`, `state` (construct in Host::new, pass references into HomeView on construction)

Moves to `HomeView`:
- `view: SurfView` (the bubble-view sub-component — rename to `transcript_view` or similar to avoid shadowing the new trait)
- `error: Option<String>`
- `pending_rx: Option<Receiver<TurnResult>>`
- `anim_start`, `last_activity`, `phase_at_activity`
- `selection_cursor`, `correction_for`
- `active_recipe`, `active_form`, `active_list`, `login_wizard`
- `tokens_in_total`, `tokens_out_total`, `agent_state`
- `esc_presses`, `esc_last` (selection-scroll ladder — keep name but scope to HomeView)
- `focus: Focus` (old two-state enum — rename to `HomeFocus` and keep for bubble selection vs composer)

Moves to `Tab::subsession`:
- The rolling repl history (`Session`'s `history_messages` equivalent that currently lives inside `session`). Keep the `Session` itself intact — just make `Tab::subsession` the mirror of per-tab transcript with the same cap.

- [ ] **Step 3: Introduce the split without moving behaviour**

First pass: widen `HomeView` to hold all the fields marked "moves to HomeView" above, leaving them uninitialised or defaulted. Add getters that return defaults. Do NOT rewire `ReplApp::handle_key` yet — the goal of this step is a compilable change with the same runtime behaviour.

```rust
pub struct HomeView {
    instance: InstanceId,
    transcript_view: crate::repl_view::SurfView,
    error: Option<String>,
    pending_rx: Option<std::sync::mpsc::Receiver<crate::TurnResult>>,
    anim_start: std::time::Instant,
    last_activity: Option<std::time::Instant>,
    phase_at_activity: f32,
    selection_cursor: usize,
    correction_for: Option<usize>,
    active_recipe: Option<String>,
    active_form: Option<crate::ActiveForm>,
    active_list: Option<crate::ActiveList>,
    login_wizard: Option<crate::LoginWizard>,
    tokens_in_total: u64,
    tokens_out_total: u64,
    agent_state: crate::AgentState,
    home_focus: HomeFocus,
    esc_presses: u8,
    esc_last: Option<std::time::Instant>,
}
```

Note: the types `TurnResult`, `ActiveForm`, `ActiveList`, `LoginWizard`, `AgentState` are private to `lib.rs` today — make them `pub(crate)` so `views::home` can name them.

- [ ] **Step 4: Port `handle_key` into `HomeView::on_event` + `Host::handle_key`**

Split current `ReplApp::handle_key` into three layers:

1. **Host-level**: bare-Esc ladder (already in Task 8), slash commands (Task 10), tab switching. This already lives in `Host::handle_key`.
2. **Composer-level**: List/Form/Banner mode dismissal, text editing — delegated to `EditArea` via `composer.on_key(key)` when focus is composer.
3. **View-level**: everything repl-specific — `HomeFocus` transitions, bubble selection navigation, plugin-installed list/form routing. Port the remaining arms of the current `handle_key` match into `HomeView::on_event(ViewEvent::Key(key))`.

Keep the submit path (`fn submit`) whole — move it to `HomeView::on_submit`. Submit now returns `SubmitOutcome::DispatchTurn { text }` for normal turns and `SubmitOutcome::Consumed` for slash commands handled inline (trace / recipes / auth).

- [ ] **Step 5: Port `render` into `HomeView::paint_chrome` + Host transcript paint**

Current `ReplApp::render` does: (a) compute regions, (b) paint bubbles, (c) paint composer + slots, (d) paint dock. Split:
- Host owns (a) and (c) — takes the bottom rows for slots + composer, passes the top rows to the active view.
- HomeView's `paint_chrome` is a no-op (layout split is 0/100).
- Host owns transcript paint using each tab's `SubSession::transcript()`. Port the bubble-rendering loop from the current `SurfView` bubble wrapper into `Host::paint_transcript(tab, region, frame)`.

Add `Host::render(&mut self, renderer: &mut Renderer)`:

```rust
pub fn render(&mut self, renderer: &mut Renderer) {
    // Pseudocode — replace with concrete region math once the current
    // `ReplApp::render` is split. Preserve ui_slots row counts.
    let area = renderer.area();
    let (composer_rows, slots_above, slots_below) = self.compute_composer_region(area);
    let view_area = area_above(area, composer_rows + slots_above + slots_below);
    let (chrome_rows, transcript_rows) = self.active_layout().rows(view_area.h);
    let chrome_region = top_slice(view_area, chrome_rows);
    let transcript_region = below_slice(view_area, chrome_rows, transcript_rows);

    let mut frame = renderer.begin();
    if let Some(tab) = self.tabs.get(self.active) {
        tab.view().paint_chrome(&mut frame, chrome_region);
        self.paint_transcript(tab, transcript_region, &mut frame);
    }
    self.paint_slots_and_composer(slots_above, slots_below, composer_rows, &mut frame);
    frame.flush();
}
```

The helpers `area_above`, `top_slice`, `below_slice`, `compute_composer_region`, `paint_transcript`, `paint_slots_and_composer` are each small translators over the existing render code. Extract them in-place; do not add abstractions.

- [ ] **Step 6: Rewrite `ReplApp` as a shim**

```rust
pub struct ReplApp {
    host: Host,
}

impl ReplApp {
    pub fn new(session: Session) -> Self {
        let mut host = Host::with_session(session);
        host.add_tab(Box::new(HomeView::new_default()));
        host.set_focus_tab_by_id(HOME_VIEW_ID);
        Self { host }
    }
    pub fn handle_key(&mut self, key: &Key) -> bool { self.host.handle_key(key) }
    pub fn handle_paste(&mut self, text: &str) { self.host.handle_paste(text) }
    pub fn render(&mut self, r: &mut Renderer) { self.host.render(r) }
    pub fn composer_text(&self) -> String { self.host.composer_text() }
    pub fn messages(&self) -> Vec<ReplMessage> {
        self.host
            .active_tab()
            .map(|t| t.subsession().transcript().to_vec())
            .unwrap_or_default()
    }
}
```

Add `Host::with_session`, `Host::handle_paste`, `Host::active_tab()` accessors.

- [ ] **Step 7: Run the full suite**

Run: `cargo test -p kern`
Expected: pass count equals the Step 1 baseline. If any test regresses, do NOT push forward — diagnose via `superpowers:systematic-debugging`. Known risk areas: `sync_slash_list`, plugin-installed list / form routing (ActiveList::Plugin), login wizard flow, trace picker, mention picker.

- [ ] **Step 8: Commit**

```bash
git commit -am "refactor(repl): migrate ReplApp body into HomeView behind Host shim"
```

---

## Task 10: Slash routing — `/inbox`, `/home`, `/tab new <view_id>`

**Files:**
- Modify: `src/bin/repl/src/slash.rs`
- Modify: `src/bin/repl/src/host/mod.rs`
- Create: `src/bin/repl/tests/host_submit.rs`

- [ ] **Step 1: Failing tests**

`src/bin/repl/tests/host_submit.rs`:

```rust
use repl::host::Host;
use repl::views::{home::HomeView, inbox::InboxView};

#[test]
fn slash_inbox_opens_or_focuses_inbox_tab() {
    let mut host = Host::headless();
    host.add_tab(Box::new(HomeView::new()));
    host.set_focus_tab_by_id("home");
    host.set_composer_text("/inbox");
    host.submit();
    assert_eq!(host.active_view_id(), Some("inbox"));
}

#[test]
fn slash_home_returns_to_home() {
    let mut host = Host::headless();
    host.add_tab(Box::new(HomeView::new()));
    host.add_tab(Box::new(InboxView::new()));
    host.set_focus_tab_by_id("inbox");
    host.set_composer_text("/home");
    host.submit();
    assert_eq!(host.active_view_id(), Some("home"));
}

#[test]
fn slash_tab_new_inbox_spawns_second_inbox_tab_with_fresh_instance() {
    let mut host = Host::headless();
    host.add_tab(Box::new(HomeView::new()));
    host.add_tab(Box::new(InboxView::new()));
    host.set_focus_tab_by_id("inbox");

    let first_instance = host.active_tab().unwrap().view().scope().instance;

    host.set_composer_text("/tab new inbox");
    host.submit();

    // Two inbox tabs now; active is the new one with a distinct instance.
    assert_eq!(host.active_view_id(), Some("inbox"));
    let second_instance = host.active_tab().unwrap().view().scope().instance;
    assert_ne!(first_instance, second_instance);
    assert_eq!(host.tabs_by_view_id("inbox").count(), 2);
}
```

- [ ] **Step 2: Run — expected FAIL**

Run: `cargo test -p kern --test host_submit`

- [ ] **Step 3: Add slash entries**

Extend `src/bin/repl/src/slash.rs`:

```rust
// CATALOG additions
SlashEntry { prefix: "/inbox", name: "inbox", hint: "switch to approvals" },
SlashEntry { prefix: "/home",  name: "home",  hint: "switch to home repl"  },
SlashEntry { prefix: "/tab",   name: "tab",   hint: "tab control: new <view_id>" },

// Command additions
pub enum Command {
    // existing ...
    SwitchView { view_id: String },
    TabNew { view_id: String },
}

// parse():
//   "/inbox" -> SwitchView { view_id: "inbox".into() }
//   "/home"  -> SwitchView { view_id: "home".into() }
//   "/tab new <id>" -> TabNew { view_id: id }
//   other    -> Unknown
```

- [ ] **Step 4: Wire `Host::submit`**

```rust
impl Host {
    pub fn submit(&mut self) {
        let text = self.composer.text();
        self.composer.set_text("");

        if slash::is_slash(&text) {
            match slash::parse(&text) {
                slash::Command::SwitchView { view_id } => {
                    self.open_or_switch(&view_id);
                    return;
                }
                slash::Command::TabNew { view_id } => {
                    self.spawn_new_tab(&view_id);
                    return;
                }
                _ => {}  // fall through to view on_submit
            }
        }

        let outcome = {
            let Some(tab) = self.tabs.get_mut(self.active) else { return };
            tab.view_mut().on_submit(&text)
        };
        self.apply_submit_outcome(outcome);
    }

    fn open_or_switch(&mut self, view_id: &str) -> bool {
        if self.set_focus_tab_by_id(view_id) { return true; }
        // No existing tab — construct a default instance of the known view kind.
        let view: Box<dyn View> = match view_id {
            "inbox" => Box::new(crate::views::inbox::InboxView::with_instance(self.next_instance())),
            "home"  => Box::new(crate::views::home::HomeView::with_instance(self.next_instance())),
            _ => return false,
        };
        self.add_tab(view);
        let idx = self.tabs.len() - 1;
        self.switch_to(idx);
        true
    }

    fn spawn_new_tab(&mut self, view_id: &str) -> bool {
        let view: Box<dyn View> = match view_id {
            "inbox" => Box::new(crate::views::inbox::InboxView::with_instance(self.next_instance())),
            "home"  => return false,  // home is singleton
            _ => return false,
        };
        self.add_tab(view);
        let idx = self.tabs.len() - 1;
        self.switch_to(idx);
        true
    }

    fn next_instance(&mut self) -> view::InstanceId {
        // Monotonic per-host counter. Collisions with the 0 instance
        // used by `new()` are fine — only `InstanceId`s within the
        // same `Host` need to be distinct.
        self.instance_counter += 1;
        view::InstanceId(self.instance_counter)
    }

    pub fn tabs_by_view_id<'a>(&'a self, view_id: &'a str) -> impl Iterator<Item = &'a Tab> + 'a {
        self.tabs.iter().filter(move |t| t.view_id() == view_id)
    }

    pub fn active_tab(&self) -> Option<&Tab> { self.tabs.get(self.active) }

    fn apply_submit_outcome(&mut self, outcome: view::SubmitOutcome) {
        use view::SubmitOutcome;
        match outcome {
            SubmitOutcome::Consumed => {}
            SubmitOutcome::SwitchTab { view_id } => { self.open_or_switch(&view_id); }
            SubmitOutcome::NewTab { view_id } => { self.spawn_new_tab(&view_id); }
            SubmitOutcome::DispatchTurn { text } => { self.dispatch_turn_active(text); }
            SubmitOutcome::DispatchApproval { .. } => { /* plan #4 */ }
            SubmitOutcome::Reject { reason: _ } => { /* TODO: surface as toast once ui_slots toast helper lands */ }
        }
    }

    fn dispatch_turn_active(&mut self, text: String) {
        // Calls into HomeView's existing dispatch (the migrated body
        // from Task 9). HomeView owns `pending_rx`, so it dispatches
        // on a worker thread and drains the result on the next tick.
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.subsession_mut().push_user(text.clone());
            tab.view_mut().on_submit(&text);  // HomeView kicks off dispatch
        }
    }
}
```

Add `instance_counter: u64` to `Host`, initialised to `0`.

- [ ] **Step 5: Run — expected PASS**

Run: `cargo test -p kern --test host_submit`

- [ ] **Step 6: Commit**

```bash
git commit -am "feat(repl/host): /inbox, /home, /tab new slash routes"
```

---

## Task 11: Prompt hint + focus indicator

**Files:**
- Modify: `src/bin/repl/src/host/mod.rs`
- Modify: composer prompt render site (grep `prompt_glyph\|composer prompt\|StyleRole::Focus` in `src/bin/repl/src/lib.rs`).

- [ ] **Step 1: Failing tests**

Extend `host_submit.rs`:

```rust
#[test]
fn prompt_hint_tracks_active_view_scope() {
    let mut host = Host::headless();
    host.add_tab(Box::new(HomeView::new()));
    host.add_tab(Box::new(InboxView::new()));
    host.set_focus_tab_by_id("home");
    assert_eq!(host.prompt_hint(), "home>");
    host.set_focus_tab_by_id("inbox");
    assert_eq!(host.prompt_hint(), "inbox>");
}

#[test]
fn focus_indicator_reflects_focus_state() {
    let mut host = Host::headless();
    host.add_tab(Box::new(HomeView::new()));
    host.add_tab(Box::new(InboxView::new()));
    host.set_focus_tab_by_id("inbox");
    assert_eq!(host.focus_indicator_char(), '>');
    host.handle_key(&esc());
    assert_eq!(host.focus_indicator_char(), '·');
}
```

- [ ] **Step 2: Run — expected FAIL**

- [ ] **Step 3: Implement**

```rust
impl Host {
    pub fn prompt_hint(&self) -> String {
        self.active_tab()
            .map(|t| t.view().scope().prompt_hint())
            .unwrap_or_else(|| ">".into())
    }

    pub fn focus_indicator_char(&self) -> char {
        match self.fsm.focus() {
            Focus::Composer => '>',
            Focus::View => '·',
        }
    }
}
```

Replace the hard-coded prompt prefix in the composer render site (found in the ReplApp render migration from Task 9) with `host.focus_indicator_char()` followed by `host.prompt_hint()` minus its trailing `>` (the indicator replaces it). If that turns awkward, just concatenate — the user sees `·home>` when focus is in the view and `>home>` when in the composer. Adjust styling so the duplicate `>` reads clearly.

- [ ] **Step 4: Run — expected PASS**

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(repl/host): prompt hint + focus indicator reflect active scope"
```

---

## Task 12: Wire binary entrypoint

**Files:**
- Modify: `src/bin/repl/src/main.rs`

- [ ] **Step 1: Read main**

Open `src/bin/repl/src/main.rs`. It constructs a `ReplApp`. With the Task 9 shim, no code change is expected. Build to confirm.

- [ ] **Step 2: Build + smoke run**

Run: `cargo build -p kern`
Expected: clean build.

Run: `cargo run -p kern` (interactive).
Manual checks:
1. Type text. Submit (Enter). Home repl responds normally.
2. Type `/inbox`. Enter. Prompt prefix becomes `>inbox>` (composer focused, inbox view active). Chrome shows the inbox stub.
3. Press `Esc`. Prompt prefix becomes `·inbox>`. Arrows no longer move the composer cursor.
4. Press `Esc`. Back to `>inbox>`.
5. Press `Esc` three times consecutively. Tab closes; back at home.
6. `/tab new inbox` — Enter. Two inbox tabs. `/home` switches to home; `/inbox` returns to the most recently focused inbox tab (behaviour follows `open_or_switch` — it finds the first match).

Record outcomes in `docs/superpowers/plans/2026-04-24-composer-view-split.progress.md`.

- [ ] **Step 3: Commit (if any drift)**

```bash
git commit -am "chore(repl): main wires through Host shim"
```

---

## Self-Review Checklist (run before handoff)

- [ ] **Spec coverage.** Each "Scope (in)" bullet has at least one task:
  - Host shell → Tasks 1–5, 8, 10, 11.
  - View trait + AgentScope + SubmitOutcome → Task 1.
  - SubSession → Task 3.
  - HomeView (chrome=0%, full transcript) → Tasks 7, 9.
  - InboxView stub → Task 6.
  - Esc toggle + triple-close + home protected → Tasks 2, 8.
  - `/inbox`, `/home`, `/tab new` → Task 10.
  - WorkerRegistry API → Task 5.
  - Prompt glyph + focus indicator → Task 11.
  - Per-tab composer buffer → Tasks 4, 8.

- [ ] **Placeholder scan.** Grep the plan for: `TBD`, `TODO` in plan text (code comments exempt in narrow cases), "fill in", "implement later", "similar to", "add error handling", "handle edge cases", "write tests for the above" without code. Fix any hits in place.

- [ ] **Type consistency.** `View::scope() -> AgentScope` referenced in Tasks 1, 6, 7, 10. `SubmitOutcome` variants (`Consumed`, `DispatchTurn`, `DispatchApproval`, `SwitchTab`, `NewTab`, `Reject`) all defined in Task 1 and only those referenced in Tasks 6, 7, 9, 10. `InstanceId(u64)` constructor in Tasks 1, 6, 7, 10. `HOME_VIEW_ID` constant referenced in Tasks 8, 9.

- [ ] **API reality check.** Before implementing each task: grep the crate in question for the exact method names the plan names. Known approximations:
  - `EditArea::is_in_mode` (Task 8) — confirm `is_in_list`, `is_in_form`, `is_in_banner` exist in `textarea`. Drop any that don't.
  - `input::Key` constructor (Task 8 test harness) — use whatever `ReplApp` existing tests already call (grep `press(KeyCode::Esc`).
  - `Renderer`, `FrameView`, `Region` paint methods (Tasks 1, 6, 7, 9) — match the real signatures in `src/render/`.
  - `ReplMessage` / `Role` constructors (Task 3) — grep `src/bin/repl/src/repl_view.rs`.

---

## Execution notes

- **Baseline first.** Task 9 requires a full-suite pass count before any migration edit. Drop any task that causes regression.
- **Commit per task.** Every task ends with one commit. No batching.
- **Branch.** Recommend `repl/composer-view-split` off `relay`. Worktree optional.
- **Follow-up plans.**
  - Plan #2 — board migration. `BoardView`, `TicketView`, hook `TicketView` into `WorkerRegistry::attach_or_spawn`, port `planned/board/board_ops` into a `board` plugin.
  - Plan #3 — primitives extraction (`ui_kit`). List / Picker / Form / Tabs / Card lifted out of repl inline code into a shared crate.
  - Plan #4 — approvals loop. Queue sink, trust tiers, InboxView wiring, cross-tab notifications via `ui_slots` toast.
  - Plan #5 — guardrails. Curator recipe, budget ceiling per tab per day, transcript-to-kern promotion on tab close.
