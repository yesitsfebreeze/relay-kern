# Plan — wire DiskANN into the live search path

**Goal:** relieve the unbounded resident-set / load-time ceiling by serving entity
vector search from a disk-resident Vamana (`DiskIndex`) once the resident set is
large, while keeping the fast in-memory HNSW for the common small case.

**Verdict basis (settled):** DiskANN is the architecture's designated huge-corpus
path (`src/config/graph.rs`: "the DiskANN index's job, not this cap";
`src/base/constants.rs`: no entity-eviction cap ships, so the in-RAM indices grow
unbounded). User confirmed the db grows unbounded in practice. KEEP + WIRE.

## Ground truth (verified 2026-06-12)

- Live search: `src/base/search.rs::search_all_unlocked` / `search_all_filtered`
  call `g.entity_idx.search(..)` and `g.gnn_entity_idx.search(..)`, then
  `merge_hits`. Reasons via `search_reasons_all_unlocked`.
- Indices are **graph-global**, not per-kern: `GraphGnn { entity_idx,
  gnn_entity_idx, reason_idx: HnswIndex }` (`src/base/graph.rs:65`). Populated for
  ALL loaded kerns by `rebuild_index` (graph.rs:177) via the shared
  index-population loop (skips Superseded entities — must preserve).
- DiskANN today (`src/base/diskann.rs`): `build_and_save(dir, &[(String, Vec<f32>)],
  Params)` + mmap `DiskIndex::open(dir)` / `.search(query: &[f32], k, search_l)
  -> Vec<(String, f32 distance)>`. Full f32 vectors on disk, no PQ, no filtered
  search, batch-build only (no incremental insert/delete).
- Quant: HNSW uses `quant_mode` (int8 wired). DiskIndex stores raw f32.

## Gaps DiskANN must close before it can serve the live path

1. **Score convention.** `HnswHit { id, score }` where score = cosine similarity
   (`1 - distance`), higher=better. `DiskIndex::search` returns distance,
   lower=better. Need a thin adapter to `Vec<HnswHit>` with `score = 1.0 - dist`.
2. **Filtered search.** Hot path has `search_filtered(vec,k,ef,keep)`; DiskIndex
   has none. Need a `keep` predicate applied during/after the beam walk.
3. **Mutation consistency.** DiskIndex is batch-built and immutable; the graph
   ingests continuously. Need a delta for inserts-since-snapshot + tombstones for
   supersede/forget, merged at search time, rebuilt periodically.
4. **Vector type.** Entities store `Vec<f64>`; DiskIndex takes `f32`. Convert at
   the build/query boundary (lossy but acceptable for ANN; matches int8 posture).

## Design — one clean implementation (no compat shim)

Introduce a backend seam so `search.rs` is unchanged at the call site:

```
enum VectorBackend {
    Resident(HnswIndex),                  // small: today's path
    Disk { snapshot: DiskIndex, delta: HnswIndex, tombstones: HashSet<String> },
}
impl VectorBackend {
    fn search(&self, vec, k, ef) -> Vec<HnswHit>;
    fn search_filtered(&self, vec, k, ef, keep) -> Vec<HnswHit>;
    fn insert(&mut self, id, vec);        // Resident -> hnsw; Disk -> delta
    fn is_empty(&self) -> bool;
}
```

`GraphGnn::{entity_idx, gnn_entity_idx, reason_idx}` become `VectorBackend`.
`rebuild_index` chooses: resident-entity-count ≤ `graph.disk_threshold` →
`Resident`; above → build a `DiskIndex` snapshot under `<data_dir>/diskann/<which>/`
and start an empty `delta`. Disk search = union(snapshot.search, delta.search)
minus tombstones, then merge/rank (reuse `merge_hits`). The tick worker rebuilds
the snapshot (fold delta in, drop tombstones) on a cadence — slots next to
stigmergy GC. Config: `[graph] disk_threshold` (default = effectively off / very
high so small deployments are byte-for-byte unchanged → "no compat" but safe
default).

## Execution — TDD increments (each: RED→GREEN→/gate; one commit)

- **I0 — research lock (no code).** Read `hnsw.rs` (HnswIndex API: insert,
  search, search_filtered, is_empty, with_mode), `config/graph.rs` (add knob),
  the tick worker, and where `data_dir` is reachable from `GraphGnn`. Confirm the
  seam compiles conceptually. Update this plan's "Design" if reality differs.
- **I1 — score+filter adapter on DiskIndex.** Add `DiskIndex::search_hits` →
  `Vec<HnswHit>` (cosine score) and `search_hits_filtered(.., keep)`. Tests:
  ordering matches brute force; filtered ⊆ unfiltered; reject-all empty.
- **I2 — build a DiskIndex from a `GraphGnn`'s resident entities.** Helper that
  snapshots `(id, vec as f32)` (skipping Superseded, mirroring the index loop)
  into a dir. Test: snapshot then search returns the same top-k ids as the in-RAM
  HNSW within recall tolerance.
- **I3 — `VectorBackend` enum + delegate `search.rs` through it.** No behavior
  change yet (always `Resident`). Full existing search test suite stays green.
- **I4 — Disk backend with delta+tombstones.** `insert` routes to delta;
  supersede/forget adds a tombstone; search unions+filters. Tests: insert after
  snapshot is found; superseded id is excluded; union ranking == single-index
  ranking on a merged corpus.
- **I5 — `rebuild_index` threshold selection + `[graph] disk_threshold` config.**
  Test: below threshold → Resident; above → Disk with a real on-disk snapshot;
  search parity across the boundary.
- **I6 — tick rebuild/consolidate job.** Fold delta into snapshot, clear
  tombstones, atomic swap. Test: post-consolidate search == pre-consolidate.
- **I7 — bench + recall regression.** Extend `bench_support`/`benches` to assert
  recall@10 of the disk path vs in-RAM HNSW within tolerance; capture latency.
- **I8 — `/personas` review + docs.** Run the panel (storage/durability + IR
  perspectives), update `diskann-disk-index.md` status to "wired", correct kern
  memory (`retrieval-stack-state`) to reflect actual wiring.

## Risks / guardrails

- Hot-path integrity: I3 is a pure pass-through; no routing until I5. Never edit
  the live path without the existing search suite green.
- Crash consistency: snapshot writes via existing `atomic_write` + tmp-rename;
  tick swap must be atomic (write new dir, rename).
- Determinism: preserve the id-ascending tiebreak in `merge_hits` for reproducible
  top-k truncation across disk+delta union.
- Windows mmap: `memmap2` + single-writer daemon (already relied on by the store).
- Default off: ship `disk_threshold` high so existing small deployments are
  unchanged until a kern actually crosses the ceiling.
