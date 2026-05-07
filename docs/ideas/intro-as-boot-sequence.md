# Intro-as-Boot-Sequence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the `intro` plugin and the `EditArea` "banner" mechanism with a boot sequence that streams `kern_log` entries and the static intro lines into repl scrollback, then unlocks the composer when the intro has finished printing.

**Architecture:** The TUI startup order is unchanged (`session_factory` constructs the registry, `app_init.rs` builds the `ReplApp`). We add a `boot_ready: bool` gate on `ReplApp` (defaults false). After `ReplApp` construction, a new `boot::run(&mut app)` routine drains the `kern_log` global `Sink` ring, pushes each entry as a `Role::System` `SurfMessage`, then pushes the static intro lines as additional `Role::System` messages, and flips `boot_ready = true`. `handle_key` short-circuits while `boot_ready == false`. The `intro` crate, the `EditArea::banner` field, the `enter_banner` / `dismiss_banner` / `has_banner` / `banner_height` methods, the `IntroHandle` type alias, and the `ReplApp::intro` field are deleted.

**Tech Stack:** Rust 2021, the workspace already in `C:\Users\sayhe\dev\relay`, existing `kern_log::Sink` ring (1024 entries), `SurfView::push_message`, `SurfMessage::system`.

---

## File Structure

**New:**
- `src/bin/repl/src/boot.rs` — boot routine (`run`) and `intro_lines()` (relocated from the deleted `intro` crate). Single file, ~50 LOC.

**Modified:**
- `src/bin/repl/src/lib.rs` — declare `mod boot;`, drop `intro` re-export and `IntroHandle` re-export, drop `intro` field from `ReplApp`, add `boot_ready: bool`.
- `src/bin/repl/src/app_init.rs` — drop `composer.enter_banner(...)` line, drop the local `IntroPlugin::new(...)` block, drop `intro` from struct literal, initialize `boot_ready: false`, call `boot::run(&mut app)` at the end of `with_default_session` (only there — `ReplApp::new` is the test-only entry point and must stay headless).
- `src/bin/repl/src/session_factory.rs` — remove `IntroHandle` type alias, drop `IntroHandle` from every tuple return type, drop the `IntroPlugin::new` + `intro.set_visible(0)` + `reg.register(intro)` block, fix every caller (downstream tuple destructures).
- `src/textarea/src/edit_area.rs` — delete `banner` field, `enter_banner`, `dismiss_banner`, `has_banner`, `banner_height` methods. Delete every call site of those methods inside the file (rendering hook + key dismiss path).
- `src/textarea/src/edit_area.rs` tests — delete any test that drives `enter_banner` / `has_banner` / `dismiss_banner`.
- `Cargo.toml` (workspace root) — drop `"src/plugins/intro"` from `members`.
- `src/bin/repl/Cargo.toml` — drop `intro = { path = "../../../plugins/intro" }`.

**Deleted:**
- `src/plugins/intro/` (entire crate directory).

---

### Task 1: Add `boot_ready` flag and `boot.rs` skeleton with relocated `intro_lines`

**Files:**
- Create: `src/bin/repl/src/boot.rs`
- Modify: `src/bin/repl/src/lib.rs` (add `mod boot;`, add `boot_ready` field, drop `intro` field, drop `IntroHandle` re-export)
- Test: `src/bin/repl/src/boot.rs` (inline `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing test for `intro_lines`**

Add this to `src/bin/repl/src/boot.rs` (creating the file):

```rust
//! @score 80
//! clarity: 82
//! correctness: 80
//! performance: 82
//! isolation: 78
//! testability: 80
//! simplicity: 85
//! safety: 82

//! Boot sequence. Drains the `kern_log` sink into repl scrollback as
//! `Role::System` bubbles, prints the static intro lines, and flips
//! `ReplApp::boot_ready` so the composer accepts input.

use crate::{ReplApp, SurfMessage};

/// Static intro banner lines. Relocated from the deleted `intro` crate.
/// Pure function; the version is sourced from the repl crate's
/// `CARGO_PKG_VERSION` so the printed line tracks the binary, not the
/// (now-defunct) intro crate.
pub fn intro_lines() -> Vec<String> {
	let version = env!("CARGO_PKG_VERSION");
	vec![
		format!("\x1b[7mRELAY\x1b[0m v{version}"),
		String::new(),
		"Relay is a registered trademark of SSH, Inc.".to_string(),
		String::new(),
		"(c) Copyright 2026-20?? SSH, Incorporated".to_string(),
		String::new(),
		"/help to get started".to_string(),
		"ESC+ESC+ESC quits.".to_string(),
		"Type then enter to query.".to_string(),
		String::new(),
		"Be save.".to_string(),
	]
}

/// Format one `kern_log::Entry` as a single-line system bubble.
fn format_entry(e: &kern_log::Entry) -> String {
	format!("[{}][{}] {}", e.level.tag(), e.source, e.message)
}

/// Drain the global `kern_log::Sink` (if installed), push each entry
/// as a `Role::System` bubble, push the intro banner, then flip
/// `boot_ready`. Subsequent log lines (e.g. async MCP bridges that
/// finish after boot) keep accumulating in the ring — boot does not
/// re-drain.
pub fn run(app: &mut ReplApp) {
	if let Some(sink) = kern_log::sink() {
		for entry in sink.snapshot() {
			app.view.push_message(SurfMessage::system(format_entry(&entry)));
		}
	}
	for line in intro_lines() {
		app.view.push_message(SurfMessage::system(line));
	}
	app.boot_ready = true;
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn intro_lines_starts_with_inverse_kern() {
		let lines = intro_lines();
		assert!(lines[0].starts_with("\x1b[7m"));
		assert!(lines[0].contains("RELAY"));
	}

	#[test]
	fn intro_lines_contains_typo_be_save() {
		assert!(intro_lines().iter().any(|l| l == "Be save."));
	}

	#[test]
	fn run_flips_boot_ready_and_pushes_intro_messages() {
		let session = crate::default_session();
		let mut app = ReplApp::new(session);
		assert!(!app.boot_ready);
		let before = app.view.messages.len();
		run(&mut app);
		assert!(app.boot_ready);
		// At minimum, the intro lines are pushed (kern_log sink may or
		// may not be installed in the test harness — boot tolerates
		// either).
		let intro_count = intro_lines().len();
		assert!(app.view.messages.len() >= before + intro_count);
	}

	#[test]
	fn format_entry_renders_tag_source_message() {
		let e = kern_log::Entry {
			level: kern_log::Level::Warn,
			source: "repl".into(),
			message: "hello".into(),
			when_ms: 0,
		};
		assert_eq!(format_entry(&e), "[WRN][repl] hello");
	}
}
```

- [ ] **Step 2: Make `SurfView::messages` accessible to the test**

Open `src/bin/repl/src/surf_view/mod.rs`, find the `messages: Vec<SurfMessage>` field around line 42, and confirm it is `pub` (or `pub(crate)`). If it is private, change it to `pub(crate)` so `boot::tests` can read its length. Do not widen visibility further.

```rust
pub(crate) messages: Vec<SurfMessage>,
```

- [ ] **Step 3: Wire `mod boot;` and add `boot_ready` to `ReplApp`**

In `src/bin/repl/src/lib.rs`:

1. Add `mod boot;` next to the other `mod` declarations.
2. Re-export `boot::intro_lines` and `boot::run` for callers and tests:
   ```rust
   pub use boot::{intro_lines, run as run_boot};
   ```
3. Locate the `pub use session_factory::{... IntroHandle};` line (around line 62) and **drop `IntroHandle`** from that re-export list.
4. Locate the `ReplApp` struct definition and:
   - **Delete** the `intro: IntroHandle,` field.
   - **Add** `pub(crate) boot_ready: bool,` immediately after `pub(crate) should_quit: bool,`.

- [ ] **Step 4: Run boot tests to verify they fail (no `boot::run` callers yet, but the unit tests must compile and pass)**

Run: `cargo test -p kern-repl boot::tests`
Expected: `intro_lines_starts_with_inverse_relay` and `intro_lines_contains_typo_be_save` and `format_entry_renders_tag_source_message` PASS. `run_flips_boot_ready_and_pushes_intro_messages` PASS once Task 2 + Task 3 also land — but the file must at least compile here. If `ReplApp::new` does not currently compile because the `intro` field is gone but still in the struct literal, that is expected; Task 2 fixes it.

- [ ] **Step 5: Commit**

```bash
git add src/bin/repl/src/boot.rs src/bin/repl/src/lib.rs src/bin/repl/src/surf_view/mod.rs
git commit -m "feat(repl): add boot module with relocated intro_lines + boot_ready flag"
```

---

### Task 2: Drop the local `IntroPlugin` from `ReplApp::new` and stop calling `enter_banner`

**Files:**
- Modify: `src/bin/repl/src/app_init.rs`

- [ ] **Step 1: Open `app_init.rs` and remove the banner + intro plumbing**

Delete line 43 (the banner call):

```rust
// REMOVE:
composer.enter_banner(intro::intro_lines());
```

Delete lines 109–120 (the `IntroPlugin` block) and the comment above it. Replace with nothing.

```rust
// REMOVE:
// The `default_session_*` builders construct the canonical intro
// handle and register it; `ReplApp::new` is the generic entry
// point (e.g. tests) that only has a `Session`, so build a local
// handle bound to our freshly allocated `slot_cache`. Production
// callers replace this via `with_default_session` so the TUI and
// session share the same plugin instance.
let local_cache = SlotCache::new();
let intro = Arc::new(intro::IntroPlugin::new(
	Arc::clone(&local_cache),
	Slot::AboveInput,
	"intro:banner",
));
```

Replace it with just the cache allocation:

```rust
let local_cache = SlotCache::new();
```

- [ ] **Step 2: Remove `intro` from the `ReplApp` struct literal and add `boot_ready: false`**

In the `ReplApp { ... }` literal (currently lines 150–192), delete the `intro,` line and add `boot_ready: false,` adjacent to `should_quit: false,`:

```rust
should_quit: false,
boot_ready: false,
trace_handle: TraceRingHandle::new(64),
```

- [ ] **Step 3: Strip the `intro` field assignment in `with_default_session`**

In the same file, locate `with_default_session` (around line 215). The line `app.intro = intro;` must be deleted. Update the destructure of `default_session_with_cache_plugins_mcp_and_tee` to drop the `intro` element — the tuple becomes `(session, cache, edit_area_bus)` after Task 3 lands; for now, accept the binding-but-unused warning by binding to `_intro`:

```rust
let (session, cache, _intro, edit_area_bus) =
	default_session_with_cache_plugins_mcp_and_tee(cache, extra, mcp_shared, Some(tee));
```

Then **call boot at the end** of `with_default_session`, immediately before `app`:

```rust
crate::boot::run(&mut app);
app
```

- [ ] **Step 4: Run the repl crate tests to verify it still compiles**

Run: `cargo build -p kern-repl`
Expected: builds clean (warnings about unused `intro` import are fine, fixed in Task 3).

- [ ] **Step 5: Commit**

```bash
git add src/bin/repl/src/app_init.rs
git commit -m "refactor(repl): drop intro+banner from ReplApp::new, call boot::run from with_default_session"
```

---

### Task 3: Strip `IntroHandle` and `IntroPlugin` from `session_factory.rs`

**Files:**
- Modify: `src/bin/repl/src/session_factory.rs`

- [ ] **Step 1: Delete the `IntroHandle` type alias**

Remove lines 31–35:

```rust
// REMOVE:
/// Handle to the intro banner plugin, shared between the session registry
/// (as `Arc<dyn Plugin>`) and [`crate::ReplApp`] (as `Arc<IntroPlugin>`
/// so it can call the concrete `set_visible` method as the composer
/// text changes).
pub type IntroHandle = Arc<intro::IntroPlugin>;
```

- [ ] **Step 2: Update every public function signature to drop `IntroHandle`**

The signatures to change (each currently returns `(Session, Arc<SlotCache>, IntroHandle, Arc<EditAreaBus>)`):

- `default_session_with_cache`
- `default_session_with_cache_and_plugins`
- `default_session_with_cache_plugins_and_mcp`
- `default_session_with_cache_plugins_mcp_and_tee`

After:

```rust
pub fn default_session_with_cache(
	cache: Arc<SlotCache>,
) -> (Session, Arc<SlotCache>, Arc<EditAreaBus>) { ... }
```

(Repeat the pattern for the other three — drop the third tuple element everywhere.)

- [ ] **Step 3: Delete the `IntroPlugin::new` + `set_visible` + register block**

Inside `default_session_with_cache_plugins_mcp_and_tee`, delete lines 163–175:

```rust
// REMOVE:
// The intro plugin is still registered so recipes / hook runners
// that reference it by name don't 404, but the banner itself is
// rendered inside the edit area now (see `ReplApp::new`), not
// pushed into the above-input slot. Immediately clear any slot
// entry the plugin created on construction so the old rendering
// path shows nothing.
let intro = Arc::new(intro::IntroPlugin::new(
	Arc::clone(&cache),
	Slot::AboveInput,
	"intro:banner",
));
intro.set_visible(0);
reg.register(wrap(Arc::clone(&intro) as Arc<dyn Plugin>));
```

- [ ] **Step 4: Update the function body's final `(session, cache, intro, edit_area_bus)` return**

Change to `(session, cache, edit_area_bus)`. Mirror in the other three wrappers that currently delegate (`default_session_with_cache`, `default_session_with_cache_and_plugins`, `default_session_with_cache_plugins_and_mcp`) — each must drop the now-removed third tuple element.

- [ ] **Step 5: Update callers in `app_init.rs`**

Change the destructure in `with_default_session` from `let (session, cache, _intro, edit_area_bus) = ...` to:

```rust
let (session, cache, edit_area_bus) =
	default_session_with_cache_plugins_mcp_and_tee(cache, extra, mcp_shared, Some(tee));
```

- [ ] **Step 6: Drop the `intro` import from `session_factory.rs`**

The line `use intro::...` (if any) and any reference to `intro::` inside the file must be gone. Confirm by:

Run: `cargo build -p kern-repl 2>&1 | grep -i intro`
Expected: no compilation errors mentioning `intro`. Warnings about unused `Slot` imports are fine.

- [ ] **Step 7: Search the rest of the repo for callers of these tuple returns**

Run: `grep -rn "default_session_with_cache" src/ tools/ tests/ --include="*.rs"`
For every match, update the destructure pattern. Tests that previously bound `(_, _, _intro, _)` become `(_, _, _)`. Do not leave any `IntroHandle` reference behind.

Run: `grep -rn "IntroHandle" src/ tools/ tests/ --include="*.rs"`
Expected: zero matches.

- [ ] **Step 8: Run the repl crate test suite**

Run: `cargo test -p kern-repl`
Expected: PASS (pre-existing tests that didn't drive `intro` directly).

- [ ] **Step 9: Commit**

```bash
git add src/bin/repl/src/session_factory.rs src/bin/repl/src/app_init.rs
git commit -m "refactor(repl): remove IntroHandle + IntroPlugin from session_factory"
```

---

### Task 4: Gate `handle_key` on `boot_ready`

**Files:**
- Modify: `src/bin/repl/src/key_handling.rs`

- [ ] **Step 1: Add the gate at the top of `handle_key`**

Open `src/bin/repl/src/key_handling.rs`. The function starts at line 32 (`pub fn handle_key(&mut self, key: &Key) -> bool {`). Insert **immediately after the opening brace, before any other logic**:

```rust
pub fn handle_key(&mut self, key: &Key) -> bool {
	// Boot gate: while the boot sequence has not finished printing the
	// intro lines, the composer is locked. Returning `false` keeps the
	// app alive without dispatching the key anywhere.
	if !self.boot_ready {
		return false;
	}
	// ... existing form-mode short-circuit ...
```

- [ ] **Step 2: Write a test that confirms keys are dropped while `!boot_ready`**

Add to `src/bin/repl/src/tests.rs` (or wherever existing key tests live — search with `grep -rn "fn handle_key" src/bin/repl/src/`). Add:

```rust
#[test]
fn handle_key_drops_input_while_boot_not_ready() {
	use input::{Key, KeyCode, Modifiers};
	let session = crate::default_session();
	let mut app = ReplApp::new(session);
	assert!(!app.boot_ready);
	let key = Key {
		code: KeyCode::Char('x'),
		mods: Modifiers::empty(),
	};
	let quit = app.handle_key(&key);
	assert!(!quit);
	// Composer must not have received the keystroke.
	assert_eq!(app.composer.text(), "");
}
```

- [ ] **Step 3: Run the test**

Run: `cargo test -p kern-repl handle_key_drops_input_while_boot_not_ready`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/bin/repl/src/key_handling.rs src/bin/repl/src/tests.rs
git commit -m "feat(repl): gate handle_key on boot_ready until intro finishes"
```

---

### Task 5: Delete `EditArea::banner` field and all `enter_banner` / dismiss plumbing

**Files:**
- Modify: `src/textarea/src/edit_area.rs`

- [ ] **Step 1: Delete the field declaration**

Remove lines 89–92:

```rust
// REMOVE:
/// Optional passive banner lines rendered above the edit buffer.
/// Dismissed by the first keystroke the widget receives, so a
/// startup greeting melts away as soon as the user starts typing.
banner: Option<Vec<String>>,
```

- [ ] **Step 2: Delete `enter_banner`, `dismiss_banner`, `has_banner`, `banner_height`**

Remove lines 225–252 (the four banner methods).

- [ ] **Step 3: Delete every internal call site of those methods inside `edit_area.rs`**

Within the same file, search for `self.banner` and `dismiss_banner(`. Each match must be either:

- A render path that paints the banner above the buffer — delete the entire `if let Some(lines) = &self.banner { ... }` block plus any `banner_height()` offset that adjusts the cursor row.
- A keystroke dismiss (`if self.banner.is_some() { self.banner = None; ... }`) — delete the entire conditional including the early-return path it gates.
- The `Default::default()` initializer for the struct — delete the `banner: None,` line.

After: zero references to `banner` or `dismiss_banner` remain in the file.

Run: `grep -n "banner" src/textarea/src/edit_area.rs`
Expected: zero matches.

- [ ] **Step 4: Delete every test in this file that drives the banner**

Within the same file (or the textarea crate's `tests/` directory), find any `#[test] fn ...` exercising `enter_banner`, `has_banner`, `dismiss_banner`. Delete those test functions wholesale.

Run: `grep -rn "enter_banner\|has_banner\|dismiss_banner\|banner_height" src/textarea/`
Expected: zero matches.

- [ ] **Step 5: Build the textarea crate**

Run: `cargo build -p kern-textarea`
Expected: builds clean, possibly with warnings about unused imports (which you should also clean up).

- [ ] **Step 6: Search the rest of the workspace for `enter_banner` callers**

Run: `grep -rn "enter_banner\|has_banner\|dismiss_banner" src/ tools/ tests/ --include="*.rs"`
Expected: zero matches (Task 2 already removed the only repl caller). If something else still references them, delete the call site — the methods are gone.

- [ ] **Step 7: Commit**

```bash
git add src/textarea/src/edit_area.rs
git commit -m "refactor(textarea): remove EditArea banner field and methods"
```

---

### Task 6: Delete the `intro` crate

**Files:**
- Delete: `src/plugins/intro/`
- Modify: `Cargo.toml` (workspace root)
- Modify: `src/bin/repl/Cargo.toml`

- [ ] **Step 1: Confirm zero remaining users**

Run: `grep -rn "intro::\|intro =\|IntroPlugin\|plugin-intro" src/ tools/ tests/ --include="*.rs" --include="*.toml"`
Expected: only the soon-to-be-deleted lines in `Cargo.toml` files and the crate itself. If any other reference exists, fix or delete it first.

- [ ] **Step 2: Remove the workspace member**

Open `Cargo.toml` (workspace root). Delete the line `"src/plugins/intro",` from the `members` array (currently in the `# plugins` group).

- [ ] **Step 3: Drop the dep from repl's `Cargo.toml`**

Open `src/bin/repl/Cargo.toml`. Delete the line:

```toml
intro = { path = "../../../plugins/intro" }
```

- [ ] **Step 4: Delete the crate directory**

```bash
rm -rf src/plugins/intro
```

- [ ] **Step 5: Verify the workspace still builds**

Run: `cargo build --workspace`
Expected: clean build. If anything still reaches for `intro`, the message will name the file — fix it and rerun.

- [ ] **Step 6: Run the full test suite**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml src/bin/repl/Cargo.toml
git rm -r src/plugins/intro
git commit -m "chore: delete intro crate, replaced by repl::boot"
```

---

### Task 7: End-to-end smoke verification

**Files:** none (manual run).

- [ ] **Step 1: Build the binary in release mode**

Run: `cargo build --release --bin kern`
Expected: clean build.

- [ ] **Step 2: Run the binary and visually confirm the boot sequence**

Run: `cargo run --bin kern`

Expected on screen, in order:
1. A series of `[INF]` / `[WRN]` system bubbles describing plugin registration, MCP bridge spawns, hook loads (whatever lines `kern_log` already emits during startup).
2. The static intro bubble: inverse-video `RELAY vX.Y.Z`, blank, trademark line, blank, copyright line, blank, `/help`, `ESC+ESC+ESC quits.`, `Type then enter to query.`, blank, `Be save.`.
3. Cursor blinking in the composer; typing accepts input.

- [ ] **Step 3: Verify pre-boot keystrokes are dropped**

This requires a small race: launch the binary and immediately spam a key. If `kern_log` is essentially empty (no `.mcp.json` in the cwd), boot completes too fast to test by hand — that is acceptable; the unit test in Task 4 already covers this contract. If `.mcp.json` declares a slow bridge, you should see keystrokes ignored until the boot bubbles finish printing.

- [ ] **Step 4: Final commit (only if any cleanup landed during smoke)**

```bash
git status
# If clean, no commit needed.
# Otherwise:
git add -A
git commit -m "chore: post-smoke cleanup"
```

---

## Notes / Non-Goals

- **Async log entries that arrive after boot.** MCP bridges spawn their own threads (see `session_factory.rs:258–283`) and may emit `kern_log::info!("mcp '{name}' ready")` after `boot::run` has finished snapshotting. Those entries stay in the `kern_log` ring and surface in the existing `trace_view`; they are intentionally **not** retro-fitted into repl scrollback. If a user wants to see the late entry, they open `trace_view`. This keeps `boot::run` synchronous and predictable.
- **`kern_log` sink installation.** `kern_log::install_sink` is idempotent and presumed to have been called by `main.rs` before `ReplApp::with_default_session`. If the sink is absent, `boot::run` silently skips the trace step and only prints the intro lines — `kern_log::sink()` returning `None` is a tolerated degraded mode.
- **`ReplApp::new` (test entry point) does not call `boot::run`.** Tests that want to exercise the boot path do so explicitly. This keeps every existing repl test that constructs a bare `ReplApp` headless and `boot_ready: false`.
- **No retry / no live tail.** `boot::run` is a one-shot drain. The boot phase is a finite event, not an ongoing subscription.
