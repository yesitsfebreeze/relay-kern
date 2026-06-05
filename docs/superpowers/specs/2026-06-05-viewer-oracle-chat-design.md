# kern viewer → RAG oracle chat

**Date:** 2026-06-05
**Status:** Design — awaiting review

## Goal

Replace the viewer's explorer UI with an **oracle**: ask a question in natural
language, get a generated answer grounded in kern memory, streamed token-by-token,
with the **source thoughts and provenance chains** it drew from shown in a single
bento box. Multi-turn (the oracle remembers the conversation). Retrieval fans out
across every running daemon and the answer is generated once over the merged union.

## What this supersedes (churn warning)

This replaces the current explorer. The following become dead and are removed from
`viewer/src/App.vue`: the hero (anchor), the structure + reasons bento panels, the
sphere/reason d3 walk logic, the fuzzy search, and the semantic `/search` call
added earlier this session. The server `POST /search` peer endpoint and the
`hub_search`/`rank_peers`/`merge_search_hits` helpers in `src/viewer.rs` are no
longer used by the UI — **they are removed** to keep a clean base (no dead code;
the workspace is `warnings = "deny"` and would reject them anyway). The
`rank_peers` *pattern* (namespace + merge + rank by score) is reused for merging
`/ask` sources, so the logic is not lost, just refocused.

## Current state (relevant facts)

- `retrieval::answer::query(g, cfg, qvec, qtext, mode, llm, embedder, opts) -> QueryResult`
  (`src/retrieval/answer.rs:19`) runs seed → expand → merge → diversify → rerank,
  and (only if `llm` is `Some`) generates an answer. Returns
  `QueryResult { answer: String, entities: Vec<ScoredEntity>, path_chains: Vec<PathChain> }`.
  With `llm = None` it does retrieval only (no answer, no llm rerank) — this is the
  peer path.
- `answer::build_answer_prompt(g, chains, scored, qtext)` and
  `answer::format_chains(g, chains)` are `pub` (`answer.rs:127`, `:153`). `format_chains`
  needs the local graph to resolve chain node texts — so it runs on the peer.
- `Client::complete(prompt)` (`src/llm.rs:171`) POSTs `/v1/chat/completions` with a
  `messages` array (OpenAI/ollama compatible). `Client` is `Clone` (`Arc<Inner>`).
- `Client::complete_func()` (`llm.rs:202`) returns a sync `Fn(&str)->String` usable
  as the `LlmFunc` that `query`'s rerank expects.
- Viewer (`src/viewer.rs`): per-daemon local server + a hub that fans out across
  `live_peers()`, namespacing ids by `format!("{addr}|")`. The hub already holds the
  `Client` (threaded earlier as `HubState.llm`). `viewer::run(graph, llm, agg_addr)`.
- Chat model default is `qwen2.5`; embed model `bge-m3` (must be pulled in ollama).

## Architecture

```
Browser (App.vue)                Hub (one daemon)                 Peers (every daemon)
─────────────────                ────────────────                 ────────────────────
ask question  ──POST /ask──►  embed(q) once
{question, history}           fan ──POST /ask_retrieve──►  answer::query(llm=None)
                              {vec, question, k}           → entities + chains
                                                           format_chains(own g)
                              ◄── {sources[], chain_text} ── (per peer)
                              merge+namespace+rank sources by score, top-N
   ◄── SSE event: sources ──  (numbered, namespaced ids)
                              build prompt from merged texts + chain_texts
                              messages = history + {user: prompt}
                              complete_stream(messages)
   ◄── SSE event: token ────  (per delta)  ◄── stream ── chat model
   ◄── SSE event: done ─────
```

Peers retrieve only. The hub embeds once, merges sources, and generates once over
the union. Generation happens purely from serialized peer data (texts + chain
strings) — the hub never needs a merged `GraphGnn`.

## Backend

### 1. `src/llm.rs` — streaming + multi-turn

- Add `pub struct ChatTurn { pub role: String, pub content: String }` (or reuse an
  existing message type) for multi-turn input.
- Add `pub async fn complete_stream(&self, messages: Vec<ChatMessage<'_>>) -> impl Stream<Item = Result<String, LlmError>>`
  (or returns a boxed stream): POST `/v1/chat/completions` with `stream: true`,
  read the response as a byte stream, parse SSE `data: {json}` lines, extract
  `choices[0].delta.content`, yield each non-empty delta. Terminate on `data: [DONE]`.
- Generalize message construction so both `complete` and `complete_stream` accept a
  `messages` vec (system/user/assistant), not just a single user string. `complete`
  keeps a convenience wrapper for the single-prompt case (used elsewhere — keep its
  existing public signature intact so other callers are unaffected).

### 2. `src/viewer.rs` — peer `POST /ask_retrieve`

- Local-server route. Body `{ vec: Vec<f64>, question: String, k: usize }`
  (`k` capped by existing `MAX_SEARCH_K`).
- Handler: `read_recovered(&g)`, then
  `answer::query(&g, &cfg.retrieval, &vec, &question, Mode::Hybrid, None, None, None)`
  → `QueryResult` (retrieval only; `llm=None` so no generation, no llm rerank).
- Build response:
  - `sources`: for each `ScoredEntity` in `entities` (take top `k`), emit
    `{id, label (truncate text 80), text (truncate 300), kind, kern, heat, conf, score}`.
    (`text` is needed by the hub to build the prompt; `label` for the bento tile.)
  - `chain_text`: `answer::format_chains(&g, &result.path_chains)` — a pre-formatted
    provenance string built against this peer's own graph.
- Returns `{ "sources": [...], "chain_text": "..." }`.
- Requires `cfg.retrieval` (RetrievalConfig) in the local server state → thread it
  into `viewer::run` and the local `State` (becomes `(Graph, RetrievalConfig)` or a
  small struct).

### 3. `src/viewer.rs` — hub `POST /ask` (SSE)

- Hub route. Body `{ question: String, history: Vec<ChatTurn>, k?: usize }`.
- Returns `Sse<impl Stream<Item = Result<Event, Infallible>>>` (axum `axum::response::sse`).
- Steps:
  1. If `question` empty → stream a single `done` event, end.
  2. `let vec = st.llm.embed(&question).await` — one embed. On error → emit
     `event: error` `{message}`, end.
  3. `let k = k.unwrap_or(8).min(MAX_SEARCH_K);`
  4. Fan out to `live_peers()`: `POST http://{addr}/ask_retrieve {vec, question, k}`.
     Collect `(tag = format!("{addr}|"), {sources, chain_text})`. Skip unreachable.
  5. Merge: namespace each source's `id`/`kern` by tag (reuse the `merge_search_hits`
     pattern), pool all peers' sources, sort by `score` desc, take top `k`. This is
     the **sources** payload (with `n` = 1-based index assigned after ranking).
  6. Emit `event: sources` `{ entities: [...top-k with n...], chains: [{addr, text}] }`
     (chains = each peer's non-empty `chain_text`, tagged by daemon).
  7. Build the generation prompt from the merged top-k texts + chain_texts:
     ```
     Context from knowledge graph:
     <chain_text blocks>
     Relevant facts:
     1. <source[0].text>
     2. <source[1].text>
     ...
     Question: <question>
     Answer concisely using only the context above. Cite sources inline as [n]
     where n is the fact number. Do not restate the context. Be direct.
     ```
     (Mirrors `build_answer_prompt`'s shape but built from serialized peer data, and
     adds the `[n]` citation instruction so the answer references the bento tiles.)
  8. `messages = history.map(to ChatMessage) ++ [user: prompt]`.
  9. `complete_stream(messages)` → for each delta yield `event: token` `{t: delta}`.
     On stream error → `event: error`. On completion → `event: done`.
- The hub needs `cfg.retrieval` only for the peer path (peers own it); the hub path
  needs the `Client` (have it) + `live_peers` + the merge helper.

### Threading config

`viewer::run(graph, llm, agg_addr)` → `viewer::run(graph, llm, retrieval_cfg, agg_addr)`.
Pass `cfg.retrieval.clone()` from `commands.rs` (RetrievalConfig must be `Clone`;
verify — if not, wrap in `Arc`). Local state carries `(Graph, RetrievalConfig)`;
hub state (`HubState`) carries `client` + `llm` (already) — unchanged for `/ask`.

## Frontend — `viewer/src/App.vue` (rewrite)

Remove the explorer machinery (hero, sphere/reason bentos, d3 walk, fuzzy,
`/search`). Keep the rail (brand + live pulse + heat legend) and the warm color
ramp / tile styling (reused for source tiles).

Layout:
```
┌ rail: kern · living memory · N thoughts · pulse ───────────────┐
│ CHAT (left)                    │ SOURCES (right, one bento)     │
│  Q: how sure are we?           │  ┌─[1]──┬─[2]──┐               │
│  ◈ oracle: We use max-join     │  │conf  │join  │  numbered     │
│    confidence [1], which is    │  │=max… │peers…│  source tiles │
│    monotone [3]…               │  ├─[3]──┴──────┤  (heat-tinted)│
│                                │  │monotone…    │               │
│  Q: why?                       │  └─────────────┘               │
│  ◈ oracle: …                   │  provenance trail:             │
│                                │   conf-join ─supports→ max-rule│
│ [ ask the oracle…           ]  │   ─ratifies→ monotone          │
└────────────────────────────────────────────────────────────────┘
```

- **Transcript:** array of turns `{role:'user'|'oracle', text, sources?, chains?}`.
  Render user turns plainly; oracle turns render text with `[n]` chips.
- **Citations:** a tiny renderer splits the streaming oracle text on `[n]` and
  renders each as a chip. Hover/click a chip → highlight source tile `n` (and
  vice-versa). Tiles use the existing heat color (`fillOf`) + glyph (`MARK[kind]`).
- **Input:** the former search bar, placeholder "ask the oracle…". Enter sends.
- **Send flow:** `POST /ask {question, history}` via `fetch` with a streamed body
  (`res.body.getReader()`), parse SSE frames:
  - `sources` → set the current turn's `sources` + `chains`, render the right bento.
  - `token` → append `t` to the current oracle turn's `text` (reactive).
  - `done` → finalize; push `{user}` and `{oracle}` turns into `history` for the
    next request.
  - `error` → show "oracle unavailable" inline; keep transcript.
- **Abort:** an `AbortController`; sending a new question aborts the in-flight stream.
- **History sent to server:** the prior `{role, content}` turns (cap to last ~6 to
  bound prompt size).

## Data flow

```
type → Enter → POST /ask {question, history}
   → SSE sources  (fast: fan-out retrieval, merged top-k)   → right bento fills
   → SSE token×N  (chat model streams)                      → answer bubble grows
   → SSE done                                               → turn pushed to history
new question mid-stream → AbortController cancels the fetch
```

## Error handling

| Failure                | Behavior                                                       |
|------------------------|----------------------------------------------------------------|
| embed down (hub)       | `event: error` → "oracle unavailable"; transcript kept         |
| chat model down        | `event: error` during/before tokens → same                     |
| peer unreachable       | skipped in fan-out (existing pattern)                          |
| empty retrieval        | sources event with `entities: []`; prompt notes "no context"; oracle answers it has no memory |
| empty question         | immediate `done`, no fetch on the client side anyway           |
| new question mid-stream| client aborts; server stream drop is tolerated                 |
| oversized `k`/history  | `k` capped by `MAX_SEARCH_K`; history capped client-side       |

## Testing

**Rust (unit):**
- `complete_stream` SSE parsing: feed a synthetic body
  (`data: {"choices":[{"delta":{"content":"He"}}]}\n\n` … `data: [DONE]`) through the
  delta parser; assert it yields `["He","llo"]` and stops at `[DONE]`. (Extract the
  line→delta parsing into a pure helper to test without a live HTTP server.)
- Source merge/rank for `/ask`: reuse/extend the `rank_peers`-style test — two peers'
  `sources` arrays merge, namespace, sort by score, truncate to `k`, assign `n`.
- Prompt builder: a pure `build_ask_prompt(merged_sources, chain_texts, question)`
  unit test asserting fact numbering + the `[n]` citation instruction is present.

**Frontend (manual — needs a daemon + ollama with qwen2.5 + bge-m3):**
- Ask a question → sources bento fills first, then the answer streams in.
- Answer contains `[n]` chips; clicking one highlights tile `n`.
- Ask a follow-up ("why?") → answer reflects prior turn (multi-turn).
- Stop ollama → "oracle unavailable", transcript intact.
- `npm run build` succeeds.

## Out of scope (YAGNI)

- Persisting conversations across reloads.
- Editing/retrying a turn.
- Per-daemon source attribution UI beyond the existing kern tag.
- Re-ranking sources with the chat model (peers' retrieval ranking is used as-is).
- Keeping the old explorer behind a toggle (explicitly replaced).

## Open implementation checks (implementer verifies against code)

- Exact `ScoredEntity` fields (`.entity`, `.score`) and `Entity::text()`.
- `Mode::Hybrid` import path (`retrieval::seed::Mode`).
- `RetrievalConfig: Clone` (else `Arc`).
- `LlmFunc` / `EmbedFunc` type aliases if the peer ever needs `Some(...)` (v1 passes
  `None`, so not needed for `/ask_retrieve`).
- axum SSE API (`axum::response::sse::{Sse, Event, KeepAlive}`) and that the installed
  axum version supports it.
