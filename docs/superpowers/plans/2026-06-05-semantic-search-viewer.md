# Semantic Search in the kern Viewer — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the viewer's substring search with semantic ranking over kern's existing bge-m3 vectors, embedding each query exactly once at the aggregator hub.

**Architecture:** Server ranks, browser displays. Each daemon's local server gains a `POST /search` route that runs the existing HNSW ANN over a *supplied* query vector. The aggregator hub gains `GET /search?q=`: it embeds the query once, fans the vector out to every live peer, namespaces ids, and merges by score. The browser debounces typing (250ms) and swaps its substring list for the server's ranking, with a sequence guard against stale responses.

**Tech Stack:** Rust (axum, serde_json, reqwest, tokio), Vue 3 + d3, Ollama bge-m3.

---

## File Structure

- `kern/src/viewer.rs` — **modify.** Add `rank_peers`/`merge_search_hits` (pure), `peer_search` handler + route on the local server, `HubState` struct, `hub_search` handler + route on the hub. Change `aggregate` to read its client from `HubState`. Change `run` signature to accept an `Llm`.
- `kern/src/commands.rs` — **modify (1 line, ~527).** Pass `llm_client.clone()` into `viewer::run`.
- `kern/viewer/src/App.vue` — **modify.** Debounced `semanticSearch`, sequence guard, placeholder text.

No new files. Ranking reuses `search_all_unlocked` / `search_reasons_all_unlocked` / `find_entity` / `find_reason` from `src/base/search.rs`.

**Testing note:** Pure functions (`rank_peers`) are unit-tested TDD-style — these mirror the existing `merge_peer` test in `viewer.rs`. The I/O wiring (endpoints, embed call, browser fetch) is verified manually, because the ANN path itself is already covered by `hnsw.rs`/`cold.rs` tests and building a populated `GraphGnn` + HNSW index in a unit test is disproportionate. Manual steps are explicit and checkable.

---

## Task 1: `rank_peers` — pure merge + namespace + sort + truncate (TDD)

**Files:**
- Modify: `kern/src/viewer.rs` (add functions + test near the existing `merge_peer` test, ~line 211)

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `kern/src/viewer.rs`:

```rust
#[test]
fn rank_peers_namespaces_pools_sorts_and_truncates() {
    // Two peers. Each returns entity hits + reason hits with scores.
    let peer_a = json!({
        "hits":    [{ "id": "e1", "kern": "k1", "label": "a", "score": 0.40 }],
        "reasons": [{ "id": "e9", "kern": "k1", "label": "ra", "score": 0.95 }],
    });
    let peer_b = json!({
        "hits":    [{ "id": "e2", "kern": "k2", "label": "b", "score": 0.70 }],
        "reasons": [],
    });
    let tagged = vec![
        ("A|".to_string(), peer_a),
        ("B|".to_string(), peer_b),
    ];
    let out = rank_peers(&tagged, 2);

    // Truncated to k=2, sorted by score desc across BOTH peers and BOTH arrays.
    assert_eq!(out.len(), 2);
    assert_eq!(out[0]["score"], 0.95);
    assert_eq!(out[1]["score"], 0.70);
    // ids + kern are namespaced by peer tag so they match what /graph shipped.
    assert_eq!(out[0]["id"], "A|e9");
    assert_eq!(out[0]["kern"], "A|k1");
    assert_eq!(out[1]["id"], "B|e2");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib viewer::tests::rank_peers_namespaces_pools_sorts_and_truncates`
Expected: FAIL — `cannot find function rank_peers in this scope`.

- [ ] **Step 3: Write minimal implementation**

Add to `kern/src/viewer.rs` (non-test code, e.g. just below `merge_peer`, ~line 211):

```rust
/// Tag one peer's search payload (`{hits, reasons}`) and append every hit to
/// `out`, prefixing `id`/`kern` so they match the namespaced ids `/graph`
/// already shipped to the browser. Both arrays are pooled into one list.
fn merge_search_hits(tag: &str, v: &Value, out: &mut Vec<Value>) {
    let pre = |id: &Value| -> Value {
        id.as_str().map(|s| Value::String(format!("{tag}{s}"))).unwrap_or(Value::Null)
    };
    for arr in ["hits", "reasons"] {
        for h in v.get(arr).and_then(Value::as_array).into_iter().flatten() {
            let mut h = h.clone();
            if let Some(o) = h.as_object_mut() {
                if let Some(id) = o.get("id") { let p = pre(id); o.insert("id".into(), p); }
                if let Some(k) = o.get("kern") { let p = pre(k); o.insert("kern".into(), p); }
            }
            out.push(h);
        }
    }
}

/// Merge every peer's tagged payload, sort by `score` descending, truncate to k.
fn rank_peers(peers: &[(String, Value)], k: usize) -> Vec<Value> {
    let mut out = Vec::new();
    for (tag, v) in peers {
        merge_search_hits(tag, v, &mut out);
    }
    out.sort_by(|a, b| {
        let sa = a.get("score").and_then(Value::as_f64).unwrap_or(0.0);
        let sb = b.get("score").and_then(Value::as_f64).unwrap_or(0.0);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(k);
    out
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib viewer::tests::rank_peers_namespaces_pools_sorts_and_truncates`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add kern/src/viewer.rs
git commit -m "feat(viewer): rank_peers — merge+namespace+sort search hits"
```

---

## Task 2: Peer `POST /search` endpoint (vector in → ranked hits)

**Files:**
- Modify: `kern/src/viewer.rs` (imports; local server router ~line 64; new handler)

- [ ] **Step 1: Add imports**

At the top of `kern/src/viewer.rs`, change the axum routing import and add extractors:

```rust
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
```

(Replace the existing `use axum::extract::State;` and `use axum::routing::get;` lines.)

- [ ] **Step 2: Add the shared request types + handler**

Add to `kern/src/viewer.rs` (non-test code):

```rust
fn default_k() -> usize { 10 }

#[derive(serde::Deserialize)]
struct SearchBody {
    vec: Vec<f64>,
    #[serde(default = "default_k")]
    k: usize,
}

/// Peer endpoint: rank this daemon's graph against a *supplied* query vector.
/// No embedding happens here — the hub already embedded once and passes the
/// vector down, so N daemons cost one embed call total.
async fn peer_search(State(g): State<Graph>, Json(body): Json<SearchBody>) -> Json<Value> {
    use crate::base::search::{
        find_entity, find_reason, search_all_unlocked, search_reasons_all_unlocked,
    };
    let g = read_recovered(&g);

    let mut hits = Vec::new();
    for h in search_all_unlocked(&g, &body.vec, body.k) {
        if let Some((e, kern)) = find_entity(&g, &h.entity_id) {
            hits.push(json!({
                "id": e.id,
                "label": truncate(&e.text(), 60),
                "kind": format!("{:?}", e.kind),
                "kern": kern,
                "heat": e.heat,
                "conf": e.conf_mean(),
                "score": h.score,
            }));
        }
    }

    let mut reasons = Vec::new();
    for h in search_reasons_all_unlocked(&g, &body.vec, body.k) {
        if let Some((r, kern)) = find_reason(&g, &h.reason_id) {
            // id is the edge's target entity so a click anchors a real node,
            // matching today's substring behavior (results used l.target).
            reasons.push(json!({
                "id": r.to,
                "label": truncate(&r.text, 80),
                "kind": format!("{:?}", r.kind),
                "kern": kern,
                "score": h.score,
            }));
        }
    }

    Json(json!({ "hits": hits, "reasons": reasons }))
}
```

- [ ] **Step 3: Register the route on the local server**

In `run`, change the local app router (~line 64) from:

```rust
let local_app = Router::new().route("/graph", get(graph_json)).with_state(graph);
```

to:

```rust
let local_app = Router::new()
    .route("/graph", get(graph_json))
    .route("/search", post(peer_search))
    .with_state(graph);
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build -p kern`
Expected: builds (warnings about `hub_search`/`HubState` not yet existing are fine only if you have them; at this point expect a clean build — `peer_search` is wired, `Query`/`StatusCode`/`Response`/`IntoResponse` may be unused → that's a warning, not an error). If `find_entity`/`find_reason`/`search_reasons_all_unlocked` paths differ, confirm against `src/base/search.rs`.

- [ ] **Step 5: Commit**

```bash
git add kern/src/viewer.rs
git commit -m "feat(viewer): POST /search peer endpoint (vector in, ranked hits out)"
```

---

## Task 3: Hub `GET /search?q=` — embed once, fan out, merge

**Files:**
- Modify: `kern/src/viewer.rs` (`HubState`, `aggregate` signature, `hub_search`, `run` signature + hub router)
- Modify: `kern/src/commands.rs` (~527, pass the `Llm`)

- [ ] **Step 1: Add `HubState` and the hub handler**

Add to `kern/src/viewer.rs` (non-test code):

```rust
#[derive(Clone)]
struct HubState {
    client: reqwest::Client,
    llm: crate::llm::Llm,
}

#[derive(serde::Deserialize)]
struct SearchQuery {
    q: String,
    #[serde(default = "default_k")]
    k: usize,
}

/// Hub endpoint: embed the query ONCE, fan the vector out to every live peer's
/// `POST /search`, namespace + merge + rank. Embed failure (e.g. ollama down)
/// returns 503 so the browser can fall back to its in-page substring list.
async fn hub_search(State(st): State<HubState>, Query(p): Query<SearchQuery>) -> Response {
    let q = p.q.trim();
    if q.is_empty() {
        return Json(json!({ "results": [] })).into_response();
    }
    let vec = match st.llm.embed(q).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(target: "kern.viewer", error = %e, "search embed failed");
            return (StatusCode::SERVICE_UNAVAILABLE, "embed unavailable").into_response();
        }
    };

    let peers = live_peers();
    let body = json!({ "vec": vec, "k": p.k });
    let mut tagged = Vec::new();
    for addr in &peers {
        let url = format!("http://{addr}/search");
        let resp = match st.client.post(&url).json(&body).send().await {
            Ok(r) => r,
            Err(_) => continue, // unreachable peer (race with shutdown) — skip
        };
        if let Ok(v) = resp.json::<Value>().await {
            tagged.push((format!("{addr}|"), v));
        }
    }

    Json(json!({ "results": rank_peers(&tagged, p.k) })).into_response()
}
```

- [ ] **Step 2: Switch `aggregate` to read its client from `HubState`**

Change `aggregate`'s signature and its first client use:

```rust
async fn aggregate(State(st): State<HubState>) -> Json<Value> {
    let client = &st.client;
    let peers = live_peers();
    // ... rest unchanged (it already uses `client.get(...)`) ...
```

(Only the signature line and the added `let client = &st.client;` change; the body's `client.get(&url)` calls now resolve to the borrowed field.)

- [ ] **Step 3: Change `run` to accept an `Llm` and build the hub state**

Change the signature:

```rust
pub async fn run(graph: Graph, llm: crate::llm::Llm, agg_addr: &str) -> std::io::Result<()> {
```

In the aggregator bind loop, replace the hub app construction:

```rust
let app = Router::new()
    .route("/", get(index))
    .route("/graph", get(aggregate))
    .with_state(client.clone());
```

with:

```rust
let hub = HubState { client: client.clone(), llm: llm.clone() };
let app = Router::new()
    .route("/", get(index))
    .route("/graph", get(aggregate))
    .route("/search", get(hub_search))
    .with_state(hub);
```

- [ ] **Step 4: Pass the `Llm` from the call site**

In `kern/src/commands.rs` (~527), change:

```rust
if let Err(e) = crate::viewer::run(vg, &vaddr).await {
```

to:

```rust
if let Err(e) = crate::viewer::run(vg, llm_client.clone(), &vaddr).await {
```

(`llm_client` is already in scope at `commands.rs:510`; it is already `.clone()`d elsewhere, so `Llm: Clone` holds. If the build complains that `Llm` is not `Clone`, add `#[derive(Clone)]` to the `Llm` struct in `src/llm.rs` — its inner is an `Arc`, so the clone is cheap.)

- [ ] **Step 5: Verify it compiles + run the full viewer test module**

Run: `cargo build -p kern && cargo test --lib viewer::`
Expected: builds clean; `rank_peers_*`, `merge_peer_*` tests PASS.

- [ ] **Step 6: Commit**

```bash
git add kern/src/viewer.rs kern/src/commands.rs
git commit -m "feat(viewer): GET /search hub — embed once, fan out vector, merge"
```

---

## Task 4: Browser — debounced semantic re-rank with stale guard

**Files:**
- Modify: `kern/viewer/src/App.vue` (`runSearch` ~160; add `semanticSearch`; placeholder ~259)

- [ ] **Step 1: Add debounce + sequence state**

In `App.vue`'s `<script setup>`, near the other `let` module state (~line 30), add:

```js
let searchTimer = null
let searchSeq = 0
```

- [ ] **Step 2: Schedule the semantic call from `runSearch`**

Replace the existing `runSearch` (lines ~160-168) with — the substring block is unchanged, the debounce scheduling is new:

```js
function runSearch() {
  const q = searchQ.value.trim().toLowerCase()
  if (searchTimer) clearTimeout(searchTimer)
  if (!q) { results.value = []; return }
  // instant in-page substring feedback (unchanged)
  const t = raw.nodes.filter(n => n.label.toLowerCase().includes(q)).sort((a, b) => (b.heat || 0) - (a.heat || 0)).slice(0, 10)
    .map(n => ({ kind: 't', id: n.id, label: n.label, sub: `${n.kind} · ${(+n.heat).toFixed(2)}` }))
  const r = raw.links.filter(l => (l.text || '').toLowerCase().includes(q)).slice(0, 6)
    .map(l => ({ kind: 'r', id: l.target, label: l.text || '(reason)', sub: l.kind }))
  results.value = [...t, ...r]
  // semantic re-rank after a 250ms pause
  searchTimer = setTimeout(() => semanticSearch(searchQ.value.trim()), 250)
}

async function semanticSearch(q) {
  if (!q) return
  const seq = ++searchSeq
  try {
    const res = await fetch('/search?q=' + encodeURIComponent(q) + '&k=10')
    if (!res.ok) return                      // 503 (embed down) → keep substring list
    const data = await res.json()
    if (seq !== searchSeq) return            // a newer query started — drop this
    if (searchQ.value.trim() !== q) return   // input changed under us
    results.value = (data.results || []).map(h => ({
      kind: h.heat === undefined ? 'r' : 't', // reason hits carry no heat
      id: h.id,
      label: h.label,
      sub: `${h.kind} · ${(+h.score).toFixed(2)}`,
    }))
  } catch (_) { /* network error → keep substring results */ }
}
```

- [ ] **Step 3: Drop the "semantic soon" placeholder**

Change the input placeholder (~line 259) from:

```
placeholder="search thoughts + reasons to anchor…  ( / to focus · semantic soon )"
```

to:

```
placeholder="search thoughts + reasons to anchor…  ( / to focus · semantic )"
```

- [ ] **Step 4: Manual verification**

With a kern daemon running (so the aggregator serves on `127.0.0.1:7700`) and the viewer served against it:

Run: `cd kern/viewer && npm run dev`

Check:
- Type a word that appears verbatim → list fills instantly (substring).
- Pause ~250ms → list reorders; each row's right-hand `sub` shows `Kind · 0.xx` cosine scores.
- Type a synonym that is NOT a substring of any label (e.g. "trust" when entities say "confidence") → after the pause, semantically related rows appear. This proves the embed→ANN path, not substring.
- Click a result → it anchors and the reason panel updates (unchanged `pick`/`setAnchor`).
- Stop ollama, type again → list still shows substring matches, no uncaught error in the console (503 path).

- [ ] **Step 5: Commit**

```bash
git add kern/viewer/src/App.vue
git commit -m "feat(viewer): debounced semantic search with stale-response guard"
```

---

## Self-Review

**Spec coverage:**
- Thread `Llm` into viewer → Task 3 (steps 3-4). ✓
- Peer `POST /search` (vector in, no embed) → Task 2. ✓
- Hub `GET /search?q=` embed-once + fan-out + merge → Task 3. ✓
- Browser debounce + swap + stale guard → Task 4. ✓
- Reason search for list parity → Task 2 (`search_reasons_all_unlocked`). ✓
- Error handling: embed down → 503 + substring fallback (Task 3 step 1, Task 4 step 2); peer unreachable → skip (Task 3 step 1); empty query → no fetch (Task 3 step 1, Task 4 step 2); stale response → `seq` guard (Task 4 step 2). ✓
- Testing: pure `rank_peers` unit-tested (Task 1); endpoints/browser manual (Tasks 2-4). ✓
- Dimension safety: relies on existing `search_all_unlocked`/`cold.rs` guards; no new code path. ✓

**Placeholder scan:** No TBD/TODO/"handle edge cases" — every code step shows full code. ✓

**Type consistency:** `SearchBody { vec, k }` (peer) and `SearchQuery { q, k }` (hub) both use `default_k`. Peer returns `{hits, reasons}`; `merge_search_hits` reads exactly `"hits"`/`"reasons"`; hub wraps as `{results}`; browser reads `data.results`. Hit fields `id/label/kind/kern/heat/conf/score` are produced in Task 2 and consumed in Task 4 (`h.heat === undefined` distinguishes reasons, which omit `heat`). `rank_peers(&[(String, Value)], usize)` signature matches its test and its hub call site. ✓
