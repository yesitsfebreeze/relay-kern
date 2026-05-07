# Relay Search TUI — PRD v1

Status: drafted 2026-05-05. Phase 3 of `/coder` flow.

## Vision

Relay = fast-search-first text editor TUI over kern's connected index. Search palette is the front door. Every entity (file, ticket, session, agent output, thought, fact, document, question, answer, conclusion) is a single connected graph traversable by typed `Reason` edges. Viewer-first; edit later.

## Non-goals (v1)

- Editing files (read-only preview pane only).
- Configurable EntityKinds per database.
- Mouse input.
- Cross-kern federation in search.
- LLM answer synthesis in palette (kern's existing `answer` mode stays available, palette doesn't auto-trigger it).

## Canonical EntityKinds (fixed enum, identical across all kern instances)

| Kind | Role |
|------|------|
| `Fact` | verified claim, high-conf, immutable |
| `Claim` | default unverified statement |
| `Document` | source artifact (file, ticket body, session slice, agent output blob) |
| `Question` | open inquiry |
| `Answer` | resolution to Question |
| `Conclusion` | synthesized stance over many Claims |
| `Superseded` | deprecated, kept for history |

`EntityStatus` is a separate orthogonal flag (Active / Superseded). `Superseded` above is both a kind-level status and lifecycle marker — collapses cleanly: lifecycle = `Active | Superseded`.

Receipts are NOT a kind — they live in the journal (action log, not knowledge).

## Decisions (locked)

1. Viewer first, edit later.
2. tree-sitter for highlighting (semantic search reusable downstream).
3. Fixed canonical EntityKind enum (no config).
4. Search RPC built on `trnsprt` typed channel.
5. Latency budget: P50 ≤16 ms keystroke→first frame, ≤80 ms full ranked result.
6. Filesystem indexing: notify-rs background watcher, no on-demand fallback.
7. `Thought` renamed to `Entity` across kern crate.
8. Hardcoded enum, no TOML registry.

## Glossary

**Domain**

- **Entity** — kern's storage unit. Fields: `id`, `vector`, `gnn_vector`, `kind: EntityKind`, `status: EntityStatus`, `source: Source` (uri scheme: `file://`, `ticket://`, `session://`, `agent://`, `inline://`), `conf`, `heat`, `access_count`, edges via Reasons.
- **EntityKind** — fixed enum above.
- **EntityStatus** — `Active | Superseded`.
- **Reason** — typed directed edge (Answers / Supports / Contradicts / Extends / Requires / References / Derives / Instances / PartOf / Consolidates).
- **Index** — fused HNSW + BM25 + reason graph + heat. Single, cross-kind.
- **Watcher** — notify-rs filesystem hook → ingest queue → kern as `kind=Document`, `source=file://...`.

**TUI**

- **Palette** — top-level search overlay. Default `Ctrl-P`. Always-on incremental.
- **Facet** — sigil-prefixed source/kind filter.
  - Source-scheme facets: `>file`, `#ticket`, `:session`, `~agent`.
  - Kind facets: `!fact`, `?question`, `=answer`, `*conclusion`, `^claim`, `§document`.
  - Stackable: `>file !fact rust borrow` = files filtered by Fact kind matching "rust borrow".
- **Card** — result row: kind icon, source-scheme glyph, label, snippet, score, lifecycle dim.
- **Chain** — breadcrumb stack of `(facet_set, query, selected_id)` frames. Drill pushes; Backspace at empty query pops.
- **Drill** — Enter or `→` on Card calls `neighbors(entity_id, edge_kinds, depth=1)` and replaces results with edge-typed neighbors.
- **Preview** — right pane (default 50% width, configurable). File→tree-sitter highlight; Entity→text + metadata; Reason edge→sentence + endpoints.
- **Highlighter** — per-language tree-sitter session. Tree maps to `Cell` via theme `StyleRole`.

**Transport**

- **service!** — `trnsprt_macros::service` proc macro.
- **SearchSvc** — typed: `search(SearchReq) -> SearchRes`, `neighbors(id, edge_kinds, depth) -> Vec<Entity>`, `preview(id) -> PreviewBlob`, `kinds() -> &'static [EntityKind]`. Streamed where keystroke-driven.

## User flows

### F1 — Cold open → search
1. User starts repl. `Ctrl-P` opens palette.
2. Types `borrow checker`. Each keystroke triggers `SearchSvc::search` (debounced 8 ms).
3. First frame ≤16 ms: existing top-K results from prior cache + spinner for refresh.
4. Full ranked frame ≤80 ms: HNSW + BM25 + PageRank + heat fused.

### F2 — Drill ticket → files
1. User types `#ticket auth`. Cards = ticket Entities (`source=ticket://...`).
2. Selects one, presses `→`.
3. New frame: neighbors filtered by Reason kind `References` ∪ `Instances`. Files with `source=file://...` rise to top.
4. Breadcrumb: `[#ticket auth] → [refs of T-123]`.

### F3 — File preview with highlighting
1. Card selected = `source=file://src/foo.rs`.
2. Preview pane loads file content (cached). Tree-sitter parses Rust grammar.
3. Highlight spans → `Cell` styles via `theme.rs` (`Keyword`, `String`, `Comment`, `Function`...).
4. Scroll with `J/K`. Search-in-file `/`.

### F4 — Entity → graph walk
1. Card = a Conclusion. Drill (`→`) shows supporting Claims and Facts.
2. Drill again on a Fact: shows Documents that ground it.
3. Each level cached in chain stack. `Esc` or Backspace at empty query pops to prior frame.

## Architecture

```
+---------------------+         +---------------------+
|  repl (TUI)         |         |  agnt (reasoner)    |
|                     |         |                     |
|  palette/  ──────┐  |         +---------------------+
|  preview/        │  |
|  highlight/ ─────┤  |             ▲
|  textarea (RO)   │  |             │ trnsprt typed
|                  ▼  |             │
|       SearchSvcClient ───────────►│
+---------------------+             │
                                    │
                              +---------------------+
                              |  kern (graph DB)    |
                              |                     |
                              |  Entity / Reason    |
                              |  HNSW + BM25 + GNN  |
                              |  RRF fuse           |
                              |  expand (depth-N)   |
                              |                     |
                              |  ◄─── notify-rs     |
                              |       watcher       |
                              +---------------------+
```

## Module map (planned)

- `src/bin/kern/src/base/` — `Entity` (was `Thought`), `EntityKind`, `EntityStatus`, `Source`. Adjust `Reason` to reference Entity ids.
- `src/bin/kern/src/retrieval/` — touch `search.rs` to filter by `kind` / `source.scheme`. `expand.rs` already does depth-1; lift to typed RPC.
- `src/shared/trnsprt/` — `SearchSvc` `service!` definition + types.
- `src/shared/watcher/` — new crate or module. notify-rs → ingest queue → kern `ingest` calls.
- `src/shared/tui/highlight/` — tree-sitter session manager + grammar registry + span→cell mapper.
- `src/bin/repl/src/palette/` — overlay state, facet parser, chain stack, render.
- `src/bin/repl/src/preview/` — preview pane: file/text/edge variants.

## Test plan (TDD scaffolds, phase 4)

| Slice | Test |
|-------|------|
| EntityKind | enum exhaustive match coverage; serde roundtrip |
| Source URI | parse `file://`, `ticket://`, `session://`, `agent://`, `inline://`; reject unknown |
| Facet parser | `#ticket auth` → `(scheme=ticket, q="auth")`; `>file !fact rust` → multi-facet |
| Chain stack | push/pop/clear; persist across resize |
| SearchSvc | mock impl; client-server roundtrip; cancellation under rapid keystrokes |
| Highlight | sample Rust file → expected span styles for `fn`, `let`, comments |
| Watcher | tmp dir create/modify/delete → ingest queue events |
| Latency | bench: P50 ≤16 ms first frame, ≤80 ms full (golden set 1000 entities) |

## Risks

- **Tree-sitter binary size + C deps**: each grammar adds weight. Mitigation: lazy-load grammars; ship rust + ts/js + python core; rest on demand.
- **notify-rs platform quirks** (Windows): debounce + coalesce.
- **Kern rename ripple**: large diff. Mitigation: solo agent, single PR. CLAUDE.md says no compat — clean rename.
- **Latency budget tight under cold cache**: warm cache on TUI start with last 200 accessed Entities (heat-ranked).

## Out of scope

- Mouse, drag-to-resize panes, multi-window — deferred.
- Edit mode + LSP — deferred.
- Cross-kern federated search — deferred.
- Custom user-defined kinds — explicitly rejected (canonical only).
