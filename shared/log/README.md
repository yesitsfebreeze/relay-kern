# `logsink`

Bounded in-memory log buffer shared across the workspace. Used by
binaries and plugins that need to surface recent log lines in a UI
(status bar, debug pane) without pulling in a tracing subscriber on
the read path.

## Surface

- `Sink` — `Clone`, holds a ring buffer of `Entry` (cap
  `MAX_ENTRIES = 1024`).
- `Entry { level, source, message, when_ms }`.
- `Level::{Info, Warn, Error}` with three-letter `tag()` (`INF`,
  `WRN`, `ERR`).

The crate name is `logsink` (the `log` directory predates the rename
and stays for path stability).

## Build

```
cargo check -p logsink
cargo test  -p logsink
```
