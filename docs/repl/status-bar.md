# Status Bar How-To

Recipe for adding info to the rows above and below the repl composer.

Full spec lives in [`ui-slots.md`](./ui-slots.md). This doc is the short
path from "I want to show X" to working code.

---

## TL;DR

Two things exist:

1. A **slot cache** (`ui_slots::SlotCache`) that holds key → lines + style.
2. A **slot config** (`ui-slots.toml`) that decides where each key paints.

To add info to the status bar:

1. Push into the cache (from a plugin, a hook, or directly from the app).
2. Add a matching entry in `ui-slots.toml` so the layout engine reserves
   a zone for it.

No renderer changes. No TUI surgery.

---

## Anatomy

```
+--------------------------------+
|  above_input rows              |   ← SlotConfig.above_input entries
+--------------------------------+
|  > composer                    |
+--------------------------------+
|  below_input rows              |   ← SlotConfig.below_input entries
+--------------------------------+
```

Each row has three zones: **left**, **center**, **right**. Multiple
entries in the same `(row, align)` cell are joined by `separator`
(default `"  "`), ordered by `priority` desc then config order.

Empty rows collapse to zero height — unused slots cost nothing.

---

## Where things live

| Thing | Path |
| --- | --- |
| `ui_slots` crate | `src/bin/repl/ui_slots/src/lib.rs` |
| `clock` reference plugin | `src/plugins/clock/src/lib.rs` |
| Repl TUI (renderer + wire-up) | `src/bin/repl/src/lib.rs` |
| User-wide config | XDG-resolved (`auth::config_path()`), typically `~/.config/relay/config.toml` or `%APPDATA%\relay\config.toml` |
| Project config | `<project>/.relay/config.toml` under `[ui_slots]` |

User config loads first, project config appends and can override
`separator` (`config::Config::load_merged`). Config hot-reloads on mtime
change (`reload_slot_config_if_changed`).

---

## Push API

```rust
cache.push(
    slot:  Slot,            // AboveInput | BelowInput
    key:   impl Into<String>, // must match `"<plugin>:<tool>"` in the config
    lines: Vec<String>,     // 1 line = 1 row; N lines span N rows
    style: Option<String>,  // "muted" | "accent" | "ok" | "warn" | "error" | "focus"
    ttl_ms: Option<u64>,    // auto-expire; None = sticky
);

cache.clear(slot, key);
```

MCP surface (for out-of-process plugins):

```json
// tool: ui.push
{
  "slot": "above_input",
  "key":  "git:branch",
  "lines": ["kern ●"],
  "style": "accent",
  "ttl_ms": 60000
}

// tool: ui.clear
{ "slot": "above_input", "key": "git:branch" }
```

Registered by `ui_slots::register_builtin_tools(&mut registry, cache)`
— already called from `default_session_with_cache_and_plugins`.

---

## Three ways to push

Pick the one that matches your source of data.

### 1. Background thread — data on a timer

For clocks, polling, file watchers. Pattern from
`src/plugins/clock/src/lib.rs`:

```rust
pub struct MyPlugin {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl MyPlugin {
    pub fn new(cache: Arc<SlotCache>, slot: Slot, key: String) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_c = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            while !stop_c.load(Ordering::Acquire) {
                let text = compute_something();
                cache.push(slot, key.clone(), vec![text], Some("muted".into()), None);
                // Short slices → responsive shutdown.
                for _ in 0..20 {
                    if stop_c.load(Ordering::Acquire) { return; }
                    thread::sleep(Duration::from_millis(50));
                }
            }
        });
        Self { stop, thread: Some(thread) }
    }
}

impl Drop for MyPlugin {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.thread.take() { let _ = h.join(); }
    }
}
```

Wire it up in `default_session_with_cache_and_plugins`
(`src/bin/repl/src/lib.rs`) next to `ClockPlugin`:

```rust
reg.register(Arc::new(my_plugin::MyPlugin::new(
    Arc::clone(&cache),
    Slot::AboveInput,
    "my:thing".into(),
)));
```

### 2. Lifecycle hooks — data per turn

For token counts, last-latency, turn index. Use YAML hooks in
`.relay/*.yaml` so `HookRunner` fires on `PreTurn` / `PostTurn` /
`Startup` and pushes via the `ui` plugin. Lower ceiling than option 1
but no Rust recompile.

If `HookRunner` is wired, it is the sole authority for status pushes —
the built-in fallback (`push_statusbar_state`) disables itself. See
the `hook_runner.is_none()` branches in `SurfApp::render`.

### 3. Direct app push — data the TUI already has

For "ready / thinking" pills, recipe name, focus mode. Call the cache
from inside `SurfApp` itself. See `push_statusbar_state` and
`push_statusbar_hint` in `src/bin/repl/src/lib.rs`.

Only use this when the data lives in `SurfApp` and nothing else. Prefer
(1) or (2) for anything a plugin could own.

---

## Config

`<project>/.relay/config.toml` (or user-scoped):

```toml
[ui_slots]
separator = "  "

# Top-left: provider/model
[[ui_slots.above_input]]
row = 0
align = "left"
plugin = "llm"
tool = "model"

# Top-right: clock
[[ui_slots.above_input]]
row = 0
align = "right"
plugin = "clock"
tool = "now"

# Bottom-left: ready/thinking pill
[[ui_slots.below_input]]
row = 0
align = "left"
plugin = "statusbar"
tool = "state"

# Bottom-right: session token usage
[[ui_slots.below_input]]
row = 0
align = "right"
plugin = "llm"
tool = "tokens"
```

The `plugin:tool` pair **must** equal the `key` you push. A key with no
matching entry is stored but never painted.

---

## Style hints

| Hint | Role | Use for |
| --- | --- | --- |
| `muted` | dim | time, path, counters |
| `accent` | foreground neutral | model name, branch |
| `focus` | primary | active recipe |
| `ok` | green | build green, tests pass |
| `warn` | yellow | dirty tree, slow |
| `error` | red | crash, hard fail |

Unknown hint → muted. One hint per push; for mixed styles, use two
entries sharing a zone.

---

## Collisions & limits

- `right` wins over `center`, `center` wins over `left` on overlap —
  loser zone dropped that frame.
- Final run truncates with `…` if it overflows its span.
- Cap ~3 segments per bar before it becomes noise.
- Every push sets a dirty flag; renderer repaints next frame. Heavy
  pushers (e.g. sub-second) are fine — reads are snapshot clones.

---

## Checklist for a new segment

1. Decide data source → pick route 1, 2, or 3 above.
2. Choose a stable `plugin:tool` key.
3. Call `cache.push(...)` from that source.
4. Add a `[[above_input]]` or `[[below_input]]` entry keyed to it.
5. Restart repl (config reload picks it up on next mtime bump).
6. Verify with `cargo test -p ui-slots` (layout) and a visual check.

---

See also: [`ui-slots.md`](./ui-slots.md) (full spec),
[`plugin-model.md`](./plugin-model.md) (plugin lifecycle),
[`plugins.md`](./plugins.md) (built-in plugin inventory).
