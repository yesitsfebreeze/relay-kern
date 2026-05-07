# UI Slots: Declarative Chrome Around the Composer

Status: **implemented** (crate: `src/bin/repl/ui_slots/`). For the
short how-to-add-a-segment path, see [`status-bar.md`](./status-bar.md).

Relay's TUI renders two stacked slot regions around the repl composer (see `ReplApp::render` in `src/bin/repl/src/lib.rs`). This doc specifies the **slot** model — the same idea Vim's `statusline` / `tabline` exposes, generalised to multiple rows and two positions.

The harness owns the layout grid and the render. Plugins own the content. Plugins **push** their fragments asynchronously on whatever cadence they choose; the harness caches the latest push per `(slot, key)` and re-composes on every frame.

This document specifies:

1. The slot model (positions, rows, zones).
2. The `config/ui-slots.toml` format and hot-reload semantics.
3. The built-in `ui` MCP tool surface plugins use to push content.
4. Render rules: ordering, collisions, empty-row elision, styling.
5. A worked example: a `clock` plugin pushing `HH:MM` every minute.

No Rust code ships with this document. Spec only.

---

## 1. Slots, rows, zones

Two slots wrap the repl composer:

```
+-------------------------------------------+
|  above_input  (0..N rows)                 |
+-------------------------------------------+
|  > composer                               |
+-------------------------------------------+
|  below_input  (0..N rows)                 |
+-------------------------------------------+
```

Each slot holds **N rows**, indexed from 0. Each row has three **zones**:

- `left` — flush to column 0, grows right.
- `center` — centred on `width/2`.
- `right` — flush to `width-1`, grows left.

N is not fixed. The renderer computes slot height from the config (see §5.3).

Rationale: one-row statuslines cover 90% of cases; two or three rows cover the rest (e.g. git branch on row 0, build status on row 1). Three zones per row matches terminal intuition and Vim's `%=` separator convention.

---

## 2. Config file

Paths (merged, user first then project). Slots live inside the unified
config under `[ui_slots]`:

- user-scope — XDG-resolved (`auth::config_path()`)
- `<project>/.relay/config.toml`

User-editable. Hot-reloaded on mtime change (see §2.3). Loader:
`config::Config::load_merged`.

### 2.1 Grammar

```toml
# Optional. Separator between entries inside the same zone.
separator = "  "

[[below_input]]
row = 0
align = "left"
plugin = "statusbar"
tool = "state"
priority = 0

[[below_input]]
row = 0
align = "right"
plugin = "clock"
tool = "now"

[[above_input]]
row = 0
align = "center"
plugin = "notify"
tool = "toast"
priority = 100
```

### 2.2 Entry fields

| Field | Type | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| `row` | `u16` | yes | — | 0-based row index within the slot. |
| `align` | `"left" \| "center" \| "right"` | yes | — | Zone within the row. |
| `plugin` | `string` | yes | — | MCP plugin (server) name. |
| `tool` | `string` | yes | — | Tool/op identifier. Forms `<plugin>:<tool>` cache key. |
| `priority` | `i32` | no | `0` | Higher first within its zone. |

Top-level keys:

- `separator` — string, default `"  "` (two spaces). Used when multiple entries share a zone.

Unknown fields → hard error on load (matches event-file strictness, §2.3 of `plugin-model.md`).

### 2.3 Hot reload

The harness watches `config/ui-slots.toml`. On change:

1. Re-parse.
2. On success: swap the config, preserve the push cache (cache keys are `(slot, key)` and survive).
3. On failure: log the parse error, keep the previous config, continue.

Entries removed from the config drop their cache slot on the next swap. Entries added start empty until their plugin pushes.

---

## 3. Push contract

The harness exposes a built-in MCP server named `ui`. Plugins call its tools to write into the slot cache. The harness never pulls.

### 3.1 `ui.push`

```
ui.push {
  slot:   "above_input" | "below_input",
  key:    "<plugin>:<tool>",          // matches config entry identity
  lines:  [string],                    // one row's worth; N lines span N rows
  style?: "muted" | "accent" | "ok" | "warn" | "error",
  ttl_ms?: u64,                        // auto-expire after this many ms
}
```

Semantics:

- Writes the latest payload into `cache[(slot, key)]`, replacing any previous push for the same key.
- `lines` is typically one entry; length > 1 means the fragment occupies consecutive rows starting at the config entry's `row`.
- Each string in `lines` may embed ANSI SGR escape sequences (`ESC[...m`). The ui_slots layer parses them into styled runs with concrete `(fg, bg, attrs)` per run. Supported: reset, bold, dim, italic, underline, inverse, strikethrough, basic + bright fg/bg, 256-colour (`38;5;N` / `48;5;N`), and truecolour (`38;2;r;g;b` / `48;2;r;g;b`).
- `style` remains as a semantic-role fallback: runs with no concrete SGR inherit this hint and the renderer maps it to the existing `StyleSet`.
- `ttl_ms`, if set, schedules automatic eviction. "Flash a toast for 3 s then vanish" without writing an explicit clear.

### 3.2 `ui.clear`

```
ui.clear { slot, key }
```

Drops `cache[(slot, key)]` immediately. Equivalent to pushing empty lines, but semantically explicit.

### 3.3 Cache identity

The `key` in a push **must** equal `"<plugin>:<tool>"` from the config entry. If a push arrives with a key that no config entry references, it is stored but never rendered — harmless, logged at debug.

---

## 4. Plugin cadence

The harness does not schedule or poll UI plugins. The plugin decides when to push:

- **On lifecycle events.** A `statusbar` plugin hooks `pre_turn` and `post_turn` (see `plugin-model.md` §3) and pushes updated state each time.
- **On timer.** A `clock` plugin spawns a tokio task that ticks every 60 s and calls `ui.push`.
- **On subprocess output.** A `build` plugin reads `cargo watch` stdout and pushes `ok` / `warn` / `error` styles as lines arrive.
- **On file change.** A `git` plugin hooks `on_file_change`, reruns `git status --porcelain`, pushes branch + dirty count.

This keeps the harness simple and the latency budget honest: rendering a frame is a cache read, never an IPC round-trip.

---

## 5. Render rules

### 5.1 Composition per zone

For each `(slot, row, zone)`:

1. Collect every config entry matching that triple.
2. For each entry, look up `cache[(slot, key)]`. Missing or expired → skip.
3. Take `lines[row - entry.row]` (row-relative). Missing index → skip.
4. Order surviving strings by `priority` descending, then by config order.
5. Join with `separator`.

### 5.2 Zone layout and collisions

Each row is laid out into a buffer of terminal width `w`:

- `left` starts at column 0.
- `right` ends at column `w-1`.
- `center` is centred on column `w/2`.

Collision policy, when zones would overlap:

- `right` wins over `center`.
- `center` wins over `left`.

The loser is truncated with a trailing single-char ellipsis `…` (UTF-8, one display column). If even the truncated form would not fit, it is dropped entirely for this frame.

### 5.3 Empty-row elision

A row is **empty** if all three zones resolve to no visible content after cache lookup and TTL expiry. Empty rows render **zero lines** — no reserved whitespace.

Row indices compact: if row 0 is empty and row 1 has content, the slot occupies one physical terminal line, with the row-1 content on it. `row` values are logical identifiers for authors; they are not a guarantee of vertical position.

Slot height equals the count of non-empty rows. The composer height shrinks by `above.rows + below.rows`.

### 5.4 TTL

A cache entry with `ttl_ms` set expires `ttl_ms` milliseconds after its push arrived. Expiry is lazy: checked at render time, no background sweeper. A subsequent push resets the TTL.

### 5.5 Style mapping

The optional `style` field names a semantic role:

| Role | Typical use |
| --- | --- |
| `muted` | Secondary info (turn counter, path). |
| `accent` | Foregrounded but neutral (model name, branch). |
| `ok` | Success (build green, tests pass). |
| `warn` | Soft failure (dirty tree, slow response). |
| `error` | Hard failure (build red, tool crash). |

Unknown roles fall back to the default foreground. Per-line styling within one push is uniform — if you need mixed styles, push from separate config entries and let them share a zone.

---

## 6. Authoring a slot plugin — `clock`

A minimal plugin that writes the current wall-clock time to the bottom-right every minute.

### 6.1 Config

`config/ui-slots.toml`:

```toml
[[below_input]]
row = 0
align = "right"
plugin = "clock"
tool = "now"
```

### 6.2 Plugin behaviour (sketch)

On plugin startup, spawn a tokio task:

```
loop {
    let now = chrono::Local::now().format("%H:%M").to_string();
    ui.push {
        slot: "below_input",
        key: "clock:now",
        lines: [now],
        style: "muted",
    };
    sleep_until_next_minute_boundary().await;
}
```

No lifecycle hooks. No event files. The plugin's only UI surface is the timer-driven push.

### 6.3 Result

The bottom-right corner of the composer area reads `14:07`, updates on the minute, and costs nothing to render between ticks. If the config entry is deleted, the plugin keeps pushing harmlessly into a dead cache slot; if the plugin dies, the cache entry ages out (or stays stale forever if no `ttl_ms` was set — authors who care should set one).

---

## 7. Out of scope (v1)

- Click / hover interactions. Slots are read-only chrome.
- Per-user theme files. Roles map through the existing `StyleSet`; theming happens there.
- Animation primitives. Plugins can push rapidly, but the harness does not coalesce or interpolate.
- Reserved slots for harness-internal content (progress bars, spinners). Everything goes through the same push contract; harness-internal sources register like any plugin.

## 8. Change log

- v0.1: initial spec. Two slots, three zones, TOML config, `ui.push` / `ui.clear`, TTL, empty-row elision, semantic styles.

---

See also: [`plugin-model.md`](./plugin-model.md) — the event-file + MCP plugin spec this layer rides on top of.
