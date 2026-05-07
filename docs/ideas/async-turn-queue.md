# Async turn queue + transaction tracking

Goal: composer never freezes during LLM turn. Queue messages while a turn
is in flight. Fine-grained `AgentState` transitions (Upstream → Thinking
→ Downstream → Working → Idle) driven by typed turn events. Per-turn
cost recorded in journal; session totals shown in status slot. Prompt
glyph always uses the foreground colour pulsing into the accent colour
— never dims when busy.

## Invariants

- Composer accepts input even while a turn is in flight.
- One turn at a time hits the LLM (session mutex stays single-writer);
  surplus user inputs queue FIFO.
- Slash commands share the same FIFO (no jump-ahead).
- `AgentState` derives only from observed turn events, not from
  `is_pending()`.
- Prompt glyph colour interpolates between `Text` and `Accent` each
  tick. Bold attribute always on for non-Idle. No `DIM`.
- Pricing table lookups are infallible: unknown model → `0` cost.

## Tasks

### T1 — Pulsing prompt-glyph colour (render_helpers.rs)

Touches: `src/bin/relay/chat/src/render_helpers.rs`.

- New helper (in `state.rs` or `render_helpers.rs`): `fn pulse_color(base:
  Color, accent: Color, tick: u64) -> Color`.
  - For both `Color::Rgb(r,g,b)`: lerp channel-wise with `t = (1 -
    cos(2π·tick/period)) / 2` over a ~12-tick period.
  - For `Color::Indexed(_)` either side: alternate between `base` and
    `accent` every ~6 ticks (no blend possible).
  - Mixed (one Indexed, one Rgb): convert Indexed via
    `theme::ansi_to_rgb` if available; else fall back to alternation.
- `render_prompt_row` (the function around line 156 of `render_helpers.rs`):
  - Read `text = theme.get(StyleRole::Text)` and `accent =
    theme.get(StyleRole::Accent)`.
  - Compute `fg = pulse_color(text.fg, accent.fg, tick)`.
  - Drop the `if state == Idle { BOLD } else { DIM }` branch — always
    `Attrs::BOLD`.
  - For `Idle` keep the static `Text` colour (no pulse).
- Glyph cycling stays where it is (`AgentState::glyph(tick)`).

Acceptance:
- `cargo check -p chat` clean.
- Visual: build + run; type a message; prompt glyph pulses while
  pending. Idle prompt is solid `Text` colour.

### T2 — Async queue (submit.rs + lib.rs + chat_view)

Touches: `src/bin/relay/chat/src/submit.rs`,
`src/bin/relay/chat/src/lib.rs`,
`src/bin/relay/chat/src/app_init.rs`,
`src/bin/relay/chat/src/chat_view.rs` (queued-bubble style).

Replace single `pending_rx: Option<Receiver<TurnResult>>` with:

```rust
struct InflightTurn {
    rx: Receiver<TurnResult>,
}
pub(crate) inflight: Option<InflightTurn>,
pub(crate) queue: VecDeque<String>, // raw user inputs awaiting dispatch
```

- `is_pending()` returns `inflight.is_some() || !queue.is_empty()`.
- `submit()` no longer early-returns on pending. Instead:
  - Append user bubble to transcript with new "queued" style if
    `inflight.is_some()`.
  - Push input to `queue`.
  - If `inflight.is_none()`, immediately call `dispatch_next()`.
  - Composer always clears.
- `dispatch_next()`: pop front of queue, spawn worker (existing logic
  factored out of `submit()`), set `inflight`.
- `tick()`: when `inflight.rx` resolves, finalise bubble; if queue
  non-empty, call `dispatch_next()`.
- New `ChatMessage::user_queued(...)` (or a `queued: bool` field) so
  view can render queued bubbles dimmer until they actually dispatch
  (then re-style on dispatch).

Acceptance:
- `cargo check -p chat` clean.
- Existing chat tests pass.
- Manual: send three messages back-to-back while first is in flight;
  all three appear immediately, each one fires when the previous
  finishes.

### T3 — Typed turn events (agent + harness + chat)

Touches: `src/relay/agent/src/types.rs`,
`src/relay/agent/src/session.rs`,
`src/relay/agent/src/recipe_dispatch.rs`,
`src/relay/harness/src/runtime/mod.rs` (or wherever Provider trait lives),
`src/bin/relay/chat/src/state.rs` (`TurnResult`),
`src/bin/relay/chat/src/submit.rs`.

```rust
// agent::types
pub enum TurnEvent {
    Upstream,
    Thinking,
    Downstream,
    ToolCallStart { name: String },
    ToolCallEnd,
    Done { text: String, usage: Option<(u32, u32)>, cost_micro: u64 },
    Err(String),
}
```

- `Session::run_turn` gains `events: &mpsc::Sender<TurnEvent>`.
  Existing `turn_with_sink` keeps its outer signature; internally calls
  `run_turn` with a sender that maps to events.
- Provider trait: optional `before_send(&Sender<TurnEvent>)` and
  `after_first_byte(&Sender<TurnEvent>)` hooks. Default impl fires
  `Upstream` then `Thinking` then `Downstream` synchronously around the
  blocking call (no real chunk streaming yet — symbolic).
- Tool-call dispatch (`recipe_dispatch::dispatch_call` or
  `tools::dispatch_tool_call`) wraps each call in
  `ToolCallStart`/`ToolCallEnd`.
- Worker in chat sends `TurnEvent` over the channel; chat
  `pending_rx: Receiver<TurnEvent>` (rename from `TurnResult`).

Acceptance:
- `cargo check --workspace` clean.
- Existing agent tests still pass.
- New unit test: `Session::run_turn` emits `Upstream → Thinking →
  Downstream → Done` for a tool-less turn; emits
  `... → ToolCallStart → ToolCallEnd → ...` when a tool is invoked.

### T4 — Pricing table (harness)

Touches: new `src/relay/harness/src/pricing.rs`,
`src/relay/harness/src/lib.rs` (re-export).

```rust
pub fn cost_micro(model: &str, in_tok: u32, out_tok: u32) -> u64;
```

- Static slice `&[(model_pattern, in_micro_per_mtok,
  out_micro_per_mtok)]`. Match longest prefix.
- Return `0` for unknown models (silent — chat falls back to "—" in
  the UI).
- Seed entries: `claude-opus-4-7`, `claude-sonnet-4-6`,
  `claude-haiku-4-5`. Numbers: pull from current Anthropic pricing.
  (Plan reviewer fills concrete cents/mtok.)

Acceptance:
- Unit tests for known models, longest-prefix match, unknown returns 0.

### T5 — Transaction journal entry + cost slot

Touches: `src/relay/journal/src/trace.rs` (add `Kind::Transaction`),
`src/bin/relay/chat/src/lib.rs` (new accumulator + slot push).

- `journal::Kind::Transaction` payload: `{model, in_tok, out_tok,
  cost_micro}`.
- Worker emits one entry per `Done`.
- `ChatApp::cost_micro_total: u64`. On `Done`, accumulates and pushes
  `↑in ↓out  $X.YYY` into existing `llm:tokens` slot (rename to
  `llm:usage` if cost ever needs its own slot).

Acceptance:
- Headless test: run a fake turn through `Session` with mock provider
  reporting usage; assert journal has one `Transaction` entry with
  expected cost.

### T6 — Wire AgentState transitions (chat tick)

Touches: `src/bin/relay/chat/src/lib.rs` (or new `tick.rs` if it
splits cleanly).

- Replace single `recv_timeout` drain with `try_iter()` over events.
- For each event:
  - `Upstream` → `set_agent_state(Upstream)`
  - `Thinking` → `Thinking`
  - `Downstream` → `Downstream`
  - `ToolCallStart` → `Working`
  - `ToolCallEnd` → `Downstream`
  - `Done`/`Err` → finalise bubble + record + transition to next queued
    turn (`Upstream`) or `Idle` if queue empty.
- Remove `is_pending()`-based `Idle → Thinking` fallback in
  `agent_state()` (events drive everything now).

Acceptance:
- `cargo check -p chat` clean.
- Existing tests pass; new test asserts state sequence over a fake
  event stream.

## Order

T1 lands first (smallest, no dep). Then T2 → T3 → T4 → T5 → T6 in
sequence; each must compile + tests green before next starts.

## Out of scope

- ~~Real streaming chunk events~~ — implemented in T8 (below). Provider
  adapter trait stays blocking-by-default; streaming is an additional
  trait method with a fall-back impl.
- Per-tool cost attribution.
- Per-day cost rollups in journal (deferred to later analysis tooling).
- UI for cancelling a queued message (Esc-while-queued behaviour
  follows existing rules — separate ticket).

## T8 streaming — real first-byte event via hand-rolled SSE

Builds on T1–T6 (which landed the typed-event channel and symbolic
`Upstream → Thinking → Downstream` transitions) by replacing the
**symbolic** `on_response_received` firing — which T3 placed right
after the blocking POST returns in `agent::session::turn_with_sink` —
with a **real** signal at the moment the first SSE text delta arrives
from Anthropic's Messages API. No new crates; reuses the existing
workspace `reqwest` (blocking + rustls).

### Touches

- `src/relay/harness/src/provider.rs` — extends `HttpTransport` and
  `ProviderAdapter` with a `stream` method (default impls preserve
  blocking semantics for `MockAdapter` / `OpenAiAdapter` /
  `StaticHttpTransport`); adds the hand-rolled `parse_sse` helper and
  `AnthropicAdapter::stream_impl` (translates SSE events into
  `LlmResponse`).
- `src/relay/harness/src/stream_hook.rs` (new) — thread-local
  "first response byte" hook + RAII guard; `install_first_byte_hook`,
  `stream_hook_installed`, `fire_stream_first_byte`. TLS rather than a
  global `Mutex` so concurrent worker threads don't see each other's
  hooks.
- `src/relay/harness/src/lib.rs` — re-exports `parse_sse_public` and
  the stream-hook surface.
- `src/bin/relay/chat/src/plugin_impls.rs` — `ReqwestTransport::stream`
  (drives `parse_sse` over a `BufReader<reqwest::blocking::Response>`);
  `AnthropicLlm::handle` now reads the TLS slot and routes through
  `adapter.stream` when a hook is installed, firing once on the first
  text chunk.
- `src/bin/relay/chat/src/submit.rs` — installs the first-byte hook
  for the duration of each chat-worker turn, with the closure sending
  `TurnEvent::Downstream` over the existing per-turn `etx` channel.

### Wire shape

The streaming SSE events Anthropic emits (well, the subset T8 honours):

| Event                  | Action                                              |
| ---------------------- | --------------------------------------------------- |
| `message_start`        | capture `usage.input_tokens`                        |
| `content_block_delta`  | if `delta.type == text_delta`: chunk + append text  |
| `message_delta`        | capture `usage.output_tokens` and `stop_reason`     |
| `message_stop`         | terminate (also fires on EOF)                       |
| `error`                | surface `error.message` as `PluginError::Failed`    |
| `ping`, others         | ignored                                             |

`AnthropicAdapter::stream` returns the same shape `LlmResponse` as
`complete` for the same inputs (text + usage + `tool_calls: []`), so
streaming and non-streaming code paths are interchangeable for
downstream consumers.

### Surface choice — narrow `complete` path, not new `complete_stream` op

Both paths from the design brief were considered. Picked the **narrow
in-`complete` first-chunk hook** path because the alternative would
require:

1. A new MCP wire format for `complete_stream` (chunks vs the single
   `Response` `Plugin::handle` returns).
2. New `Plugin::handle_stream` API surface on the harness `Plugin`
   trait.
3. Changes to `RegistryLlm`, the agent loop, and the recipe engine to
   propagate per-chunk events.
4. Per-chunk re-emit through the TUI render path (not just the
   first-byte boundary).

T8's brief asks only for the **first** byte to drive
`on_response_received`; subsequent chunks "do not re-fire (state stays
`Downstream`)". Pulling the whole stream surface through MCP for one
state transition is a bad trade.

The narrow path uses harness's TLS slot + the chat worker's existing
`etx` channel, which already routes through the per-turn event
plumbing T3 added. The plugin-trait surface is unchanged; the streaming
path is opt-in per turn via a guard.

### Acceptance

- `cargo check --workspace` clean.
- `cargo test -p relay-harness --lib`: 110 → 120 (10 new — 4 SSE
  parser, 3 `AnthropicAdapter::stream`, 3 `stream_hook`).
- `cargo test -p relay-agent --lib`: 26 (unchanged).
- `cargo test -p chat --lib`: 91 (unchanged).

### Out of scope (T8)

- Re-emitting subsequent chunks as `llm.chunk` observe events — the
  TUI does not yet render incrementally below the bubble level.
- Per-chunk live token rendering in chat history.
- Reconnection / retry on dropped streams. A truncated SSE payload
  surfaces as a clean EOF (the parser flushes any in-flight event)
  followed by whatever `LlmResponse` it managed to assemble; HTTP
  errors propagate as `PluginError::Failed`.
- OpenAI streaming. `OpenAiAdapter` keeps `complete` semantics via the
  trait's default `stream` impl (one chunk = whole reply).
