# Semantic search in the kern viewer

**Date:** 2026-06-05
**Status:** Design — awaiting review

## Goal

Replace the viewer's substring-only search with semantic ranking, reusing the
bge-m3 vectors and HNSW index kern already maintains. One embed call per query.
No new ranking algorithm — this is wiring, not invention.

## Non-goals (YAGNI)

- No grid highlight / camera fly. Results stay in the existing bottom list.
- No shipping vectors to the browser (~234k × 1024 × 8B ≈ 2 GB). Server ranks.
- No per-peer embedding. The hub embeds once; peers do pure cosine/HNSW.
- No reranker, no HyDE, no query expansion in v1.

## Current state

- `Llm::embed(text)` → one bge-m3 embedding call (`src/llm.rs:103`).
- `search_all_unlocked(g, vec, k)` → HNSW ANN over the entity index, returns
  `{entity_id, score}` (`src/base/search.rs:23`).
- `search_reasons_all_unlocked(g, vec, k)` → same for reasons (`search.rs:58`).
- Viewer (`src/viewer.rs`): each daemon runs a local `/graph` server holding the
  `Graph`; one daemon wins the well-known aggregator address and fans `/graph`
  out across all live peers, namespacing ids by peer tag (`merge_peer`).
- Browser (`viewer/src/App.vue`): `runSearch` (line 160) does a substring filter
  over `raw.nodes` labels + `raw.links` text, top-10, click → `setAnchor`.
  Placeholder reads "semantic soon".
- Call site: `viewer::run(vg, &vaddr)` at `src/commands.rs:527`, where
  `llm_client` is already in scope (`commands.rs:510`).

## Architecture

Server ranks, browser displays. Three wiring changes in kern + a browser swap.

### 1. Thread `Llm` into the viewer

`viewer::run(graph, agg_addr)` → `viewer::run(graph, llm, agg_addr)`. Pass
`llm_client.clone()` (or an embed-only handle) from `commands.rs:527`. The local
server's axum `State` becomes `(Graph, Llm)`; the hub's state gains the `Llm`
alongside its `reqwest::Client`.

### 2. Peer endpoint — `POST /search` (per-daemon local server)

- Body: `{ "vec": [f64], "k": usize }`. **Vector supplied — no embed here.**
- Runs `search_all_unlocked(&g, &vec, k)` for entities and
  `search_reasons_all_unlocked(&g, &vec, k)` for reasons (keeps list parity with
  today's mixed thoughts+reasons list).
- Resolves each hit id to its display fields via the loaded graph (same shape as
  `graph_json`: `id, label` (truncated 60), `kind, kern, heat, conf, score`).
- Returns `{ "hits": [...], "reasons": [...] }`.
- Empty/length-mismatched vec → empty result (existing guards in
  `search_all_unlocked` and `cold.rs` already cover this).

### 3. Hub endpoint — `GET /search?q=&k=` (aggregator)

1. `let vec = llm.embed(q).await` — the single embed call. On error → HTTP 503.
2. `live_peers()` → for each, `POST /search { vec, k }` in parallel, with the
   existing `FANOUT_TIMEOUT`. Unreachable peer → skip (existing pattern).
3. Namespace each peer's hit ids by its tag (reuse the `pre`/`merge_peer`
   id-prefixing logic so ids match what `/graph` already shipped to the browser).
4. Merge all peers' hits, sort by `score` desc, truncate to `k`.
5. Return `{ "results": [...] }` (entities + reasons interleaved or in two
   arrays — browser flattens either way).

### 4. Browser (`App.vue`)

- Keep `runSearch` (substring) firing on every keystroke for instant feedback.
- Add a 250ms debounce that calls `fetch('/search?q=' + encodeURIComponent(q) +
  '&k=10')`. On success, replace `results.value` with semantic hits, score shown
  in the existing `sub` slot (e.g. `Fact · 0.81`).
- **Stale-response guard:** stamp each request with an incrementing `seq`; ignore
  any response whose `seq` is not the latest. Prevents a slow embed from
  clobbering a newer query's results.
- On 503 / fetch error: keep the substring results already on screen, no throw.
- `pick(res)` unchanged — ids are namespaced and present in `raw.nodes`, so
  `setAnchor` resolves.
- Update the placeholder: drop "semantic soon".

## Data flow

```
keystroke ──► substring list (0 ms, in-browser)
   │
   └─ pause 250ms ─► GET /search?q ─► hub embeds q (1 bge-m3 call)
                                       └─► POST /search{vec} ─► peer ANN  ┐
                                       └─► POST /search{vec} ─► peer ANN  ┤ parallel
                                       └─► …                              ┘
                                   merge + sort by score + truncate k
                                       └─► results list reorders
click ──► setAnchor (unchanged)
```

## Error handling

| Failure              | Behavior                                                |
|----------------------|---------------------------------------------------------|
| ollama / embed down  | hub → 503; browser keeps substring list; no crash       |
| peer unreachable     | hub skips it (existing fan-out pattern)                  |
| empty query          | no fetch; results cleared                                |
| stale response       | dropped by `seq` guard                                   |
| dim mismatch         | empty hits (guards in `search_all_unlocked` / `cold.rs`) |

## Testing

**Rust (unit):**
- Hub merge+rerank sorts by score across tagged peers and truncates to `k`
  (mirrors the existing `merge_peer_namespaces_ids` test).
- `POST /search` returns ranked hits for a small in-memory graph given a vec.
- Embed error path yields 503.

**Browser (manual):**
- Type → substring list appears instantly.
- Pause → list reorders by cosine; scores visible.
- Click a result → anchors and the reason panel updates.
- Kill ollama → typing still yields substring results, no console throw.

## Dimension safety

Query embeds as bge-m3 (1024) against an entity index built from the same model.
`search_all_unlocked` guards empty vec; `cold.rs` filters on length mismatch.
Model swaps are already code-safe per existing kern design notes.
