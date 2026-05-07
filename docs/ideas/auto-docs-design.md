# Auto-generated integration docs

**Status:** design
**Date:** 2026-04-24
**Scope:** repo-wide documentation pipeline

## Problem

The repo has ~30 crates. Each has a README. What it lacks: **integration
docs** — narratives that cross crate boundaries and answer "if I touch X,
what else do I have to look at?". Rustdoc answers "what does this function
do"; it does not answer "how does a plugin flow from load to teardown."

API-heavy docs are low-value here. The user wants a two-stage experience:

1. **How to use it** — integration guide, narrative, cross-crate.
2. **Where the code lives** — rustdoc, reachable via anchors from the guide.

Output: GitHub Pages site with fuzzy search.

## Non-goals

- Replacing rustdoc. Rustdoc is the API layer, linked from guides.
- Regenerating on every merge. LLM calls are committed artifacts, reviewed
  in PRs.
- Covering private items. Only `pub` surface.

## Architecture

```
┌─────────────────────────────────────────────────────┐
│  Source of truth (repo)                             │
│  ├─ crate READMEs        (exist, hand-written)      │
│  ├─ rustdoc JSON         (cargo +nightly, per crate)│
│  └─ relay graph          (cross-crate linkage)      │
└──────────────────┬──────────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────────┐
│  xtask: `cargo xtask docs`                          │
│  1. emit rustdoc JSON for all crates                │
│  2. collect READMEs                                 │
│  3. query relay for seeded flows                    │
│  4. LLM pass → integration guides (markdown)        │
│  5. write to docs/book/src/guides/*.md              │
└──────────────────┬──────────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────────┐
│  docs/book/  (mdBook source, committed)             │
│  ├─ SUMMARY.md                                      │
│  ├─ guides/      ← AI-generated integration         │
│  ├─ crates/      ← copied from crate READMEs        │
│  └─ api/         ← rustdoc HTML (built, not commit) │
└──────────────────┬──────────────────────────────────┘
                   │ mdbook build + cargo doc
                   ▼
┌─────────────────────────────────────────────────────┐
│  GitHub Pages (gh-pages branch, CI publish)         │
│  fuzzy search via mdBook Elasticlunr                │
└─────────────────────────────────────────────────────┘
```

## Decisions

| Question | Choice | Reason |
|---|---|---|
| Static site generator | **mdBook** | Built-in fuzzy search (Elasticlunr), Rust-ecosystem standard, plain markdown in. |
| Granularity | **Per-crate README + cross-cutting integration guides** | READMEs already exist; integration layer is the missing value. |
| Topic discovery | **Hybrid: hand-seeded flows + relay-derived neighbors** | User controls taxonomy; graph fills cross-references. |
| Regeneration | **Committed + manual (`cargo xtask docs gen`)** | Diffs reviewable in PR; no LLM cost on merge; matches worktree cadence. |
| API layer | **rustdoc JSON for structure, rustdoc HTML for link targets** | Line-move-stable anchors. |

## Components

### `cargo xtask docs`

Single CLI entry. Subcommands:

- `gen` — regenerate integration guides via LLM (uses cache).
- `build` — assemble mdBook tree (READMEs + guides + rustdoc), run `mdbook build`.
- `serve` — local preview (`mdbook serve`).
- `check` — lint: dead anchors, missing crates in SUMMARY, stale flow list.
  Runs in CI.

### Flow registry: `docs/book/flows.toml`

Hand-seeded list of integration flows. Committing this file = user controls
taxonomy.

```toml
[[flow]]
name = "plugin-lifecycle"
title = "Using a plugin"
seeds = ["relay-plugin", "relay-dispatch"]
question = "What touches a plugin from load to teardown?"
```

Fields:

- `name` — slug, becomes `guides/<name>.md`.
- `title` — chapter title in SUMMARY.
- `seeds` — seed crates or relay node IDs.
- `question` — the narrative question the guide must answer.

### LLM generator

Rust bin. Per flow entry:

1. Collect rustdoc JSON for seed crates (pub API, signatures, doc strings).
2. Read seed crate READMEs.
3. Query relay: neighbors of seed nodes, depth 2, edges with justifications.
4. Compress: drop private items, dedupe, cap ~30k tokens.
5. LLM call: template + flow question + compressed context. Model
   `claude-opus-4-7`. Output markdown structured as:
   - Overview
   - Step-by-step
   - Pitfalls
   - See also (links)
6. Post-process:
   - Resolve `crate::Path::Item` → rustdoc URL (lookup from step 1).
   - Verify every symbol link points at a real rustdoc page; fail otherwise.
   - Inject footer: `Generated from flows.toml:<name>, <date>`.
7. Write `docs/book/src/guides/<name>.md`.

### Anchors

Two kinds:

- **Symbol anchors** (preferred):
  `` [`relay_textarea::Textarea`](../api/relay_textarea/struct.Textarea.html) ``
  resolved from rustdoc JSON, stable across line moves.
- **Source anchors** (rare, for flow examples):
  `[src](https://github.com/.../src/foo.rs#L42)` — acknowledged to rot; used
  only when symbol-level pointer is insufficient.

### mdBook layout

```
docs/book/
  book.toml
  flows.toml
  src/
    SUMMARY.md          (generated by xtask)
    introduction.md     (hand-written)
    guides/             (AI-generated, committed)
      plugin-lifecycle.md
      recipe-flow.md
      ...
    crates/             (copied from crate READMEs at build)
      relay-textarea.md
      ...
```

`SUMMARY.md` is fully derived from `flows.toml` + `crates/*`. Never
hand-edited.

### Caching

Hash = `(flow entry + seed READMEs + seed rustdoc JSON signatures)`. Skip
LLM call if hash unchanged. Cache at `target/xtask-docs-cache/`.

## Data flow (single flow)

```
flows.toml entry
    │
    ▼
[collect context]
    ├── rustdoc JSON   → pub API of seed crates
    ├── READMEs        → existing narrative
    └── relay query    → depth-2 neighbors + edge justifications
    │
    ▼
[compress]    drop private, dedupe, cap ~30k tokens
    │
    ▼
[LLM call]    template + question + context → markdown
    │
    ▼
[post-process]
    ├── rewrite `crate::Item` → rustdoc link
    ├── verify every link resolves (else fail)
    └── inject provenance footer
    │
    ▼
docs/book/src/guides/<name>.md
```

## Error handling

- Missing rustdoc JSON (nightly not installed) → fail with install hint.
- Missing crate README → warn, skip that crate in `crates/`, continue.
- relay unavailable → warn, generate without graph context (degrades,
  does not block).
- LLM call fails → retry 2×, then fail (no partial write).
- Symbol link resolves to nothing → fail generation (prevents dead
  anchors shipping).
- `flows.toml` references unknown crate → fail with list of valid crates.

## Testing

- **Unit:** rustdoc JSON parser, symbol resolver, anchor rewriter, cache
  hasher.
- **Integration:** `xtask docs check` as CI job — rebuilds SUMMARY, lints
  dead anchors, fails if committed guides drift from `flows.toml` schema.
- **Snapshot:** one small fixture crate + fixture flow, stub LLM, assert
  generated markdown structure stable.
- **No LLM calls in CI.** Generation runs on dev machine only.

## Publishing

GitHub Action `docs.yml` on push to `relay`:

1. `cargo xtask docs check` — fail if stale.
2. `cargo +nightly doc --no-deps --workspace` → `docs/book/src/api/`.
3. `cargo xtask docs build` → `docs/book/book/`.
4. Publish `docs/book/book/` to `gh-pages` branch.
5. GitHub Pages serves from `gh-pages`.

## Rollout (worktree sessions)

Session 1 — **bulk**: xtask skeleton + rustdoc JSON + README collector +
mdBook scaffold + CI publish. No LLM yet. Output: GitHub Pages site with
crate READMEs and rustdoc. `flows.toml` empty.

Session 2 — **LLM generator**: flow registry, generator, anchor resolver,
cache. Seed `flows.toml` with 3 flows, generate, review.

Session 3 — **polish**: lint (`docs check`), error paths, expand flow list
from relay suggestions, snapshot tests.

## Open items

- Exact relay query shape (which edge kinds count as "related" for a flow).
  Resolve in Session 2 when wiring generator.
- Whether `crates/` pages link into rustdoc via a banner. Resolve in
  Session 1 when assembling mdBook.
