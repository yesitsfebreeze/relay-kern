# Anchors — replacing single-purpose routing with a multi-anchor root

Date: 2026-06-07
Status: Design (approved sections, pending written-spec review)

## Problem

Today each kern node carries one `purpose_text` / `purpose_vec`. The root
purpose is a single overarching vector; routing (`accept.rs::route_entity`)
descends the tree and, at each node with a purpose, gates an incoming entity by
`acceptance_probability(cosine_distance(e, purpose_vec), inner, outer)`.

A single root vector forces everything through one semantic lens. We want the
root to hold **multiple anchors** — independent overarching vectors — plus a
**generic** catch-all that absorbs whatever matches no anchor.

## Realized mechanism (corrected from the first draft)

The first draft proposed removing `purpose_text`/`purpose_vec` and adding an
`anchors: Vec<Anchor>` to the root. Two facts make that the wrong shape:

1. **`purpose_vec` is the per-node routing vector for the *entire* tree**, not
   just the root. `route_to_child_id` matches an entity against every named
   child's `purpose_vec` at every depth, and `is_named`/`is_dead`/promotion all
   key off `purpose_text`. Deleting it guts tree routing — and "tree-per-anchor
   underneath" (your chosen scope) *needs* per-node vectors in those subtrees.
2. **Persistence is bincode (positional — no field names stored).** Removing two
   fields and adding a differently-shaped one changes the byte layout, so every
   existing `.kern` shard fails to decode → total loss of the live graph.
   Renaming a field *in place* (same position, same type), by contrast, is free.

So the realized design keeps the per-node vector and expresses anchors as the
**named top-level children of the root**:

`types.rs`:

- Rename in place, preserving struct field order (bincode-safe):
  `purpose_text` → `anchor_text`, `purpose_vec` → `anchor_vec`,
  `has_purpose()` → `has_anchor()`. No fields added or removed.
- An **anchor** is a named child of the root: a `Kern` with `anchor_text` = the
  anchor name, `anchor_vec` = embedding of the anchor description, default radii,
  `parent = root.id`.
- **`generic`** is a permanent named child of the root with `anchor_text =
  "generic"` and an **empty** `anchor_vec`. Empty vector ⇒ `route_to_child_id`
  never similarity-matches it (it already skips empty-vec children), so it is
  reachable only as the explicit fallback. Named ⇒ immortal (never GC'd).

## Routing (`accept.rs`)

- New constant `ACCEPT_FLOOR: f64 = 0.5` in `base/constants.rs`.
- `route_to_child_id` returns the best named child **only if its acceptance
  probability ≥ `ACCEPT_FLOOR`**; otherwise `None`.
- In `route_entity`, when `route_to_child_id` is `None` and the current kern has
  no own-anchor gate to apply (the root, post-"drop clean", has no `anchor_vec`),
  the entity descends into the **generic** child via
  `get_or_spawn_generic_child` instead of committing to the dispatcher node.
- Below the chosen anchor (or inside generic) descent is the **existing**
  `route_entity` logic — anchors and generic are ordinary kern subtrees from
  there down. No second anchor layer below the root.

`acceptance_probability` and `cosine_distance` stay shared in `base`; no
duplication. Radii stay on the kern node (each anchor *is* a kern).

## Generic bucket

`generic` is a **normal subtree**: it clusters and descends like any kern. It is
not a flat list. Locality search works inside it before promotion.

## Emergent anchors (`tick/cluster.rs`)

A dense, cohesive cluster inside the `generic` subtree is promoted to a named
anchor:

1. Score generic-subtree clusters with the existing `is_core_cluster`.
2. On a qualifying cluster: name it via the existing `anchor_prompt` path (the
   renamed `purpose_prompt`, the same path that today names unnamed kerns), set
   the cluster centroid as the new kern's `anchor_vec`, inherit default radii.
3. Promote it to **root level**: set the new named kern's `parent = root.id` and
   add it to `root.children`, so it becomes a first-class anchor rather than a
   nested child. Members route under it on subsequent ticks.

This reuses the current cluster → name machinery; the only delta is reparenting
the promoted kern to the root.

## API surface

Rename `purpose` → `anchor` across the stack:

- MCP tool (`mcp/tools_admin.rs`): `purpose` tool becomes `anchor` with
  subcommands `add(name, text)`, `list`, `remove(name)`.
- CLI (`commands/admin.rs`), REPL (`repl.rs`), wire (`wire.rs`).
- RPC: `shared/trnsprt/src/kern_rpc/{dto,svc,mock}.rs` — rename the purpose
  method/DTO to anchor equivalents.
- `retrieval/digest.rs`, `viewer.rs`, resources — update references.

`anchor add` embeds `text` → `anchor_vec` (the same embed path purpose used) and
registers a named child of the root.

## No-compat & data safety

Per `CLAUDE.md` (no compat, clean base) — and without losing the live graph:

- **No serde compat attributes, no dual-read code.** The rename is a pure
  identifier rename in Rust.
- **Bincode is positional** (stores no field names), so renaming `purpose_text`
  → `anchor_text` and `purpose_vec` → `anchor_vec` *in their existing struct
  positions* leaves every stored `.kern` shard byte-identical in meaning. The
  898-thought live graph loads unchanged. **Field order must be preserved.**
- The old single root purpose is dropped at the behavior level: the root stops
  using its own `anchor_vec` for gating (it becomes a pure dispatcher). Its
  stored value is simply ignored; a one-line clear on load is optional.
- A permanent `generic` child is ensured at startup if absent.

## Affected files

- `src/base/types.rs` — rename the two fields in place; `has_anchor()`;
  `generic`/anchor child constructors.
- `src/base/constants.rs` — `ACCEPT_FLOOR`, generic name constant.
- `src/base/accept.rs` — floor in `route_to_child_id`; generic fallback;
  `get_or_spawn_generic_child`.
- `src/base/graph.rs` — ensure-generic-child on root init.
- `src/base/persist.rs` — field-name references only (format byte-compatible).
- `src/tick/cluster.rs` (`purpose_prompt`→`anchor_prompt`, `is_core_cluster`
  arg) + `src/tick/tasks.rs` — promote-to-root.
- `src/mcp/tools_admin.rs`, `src/mcp/tools.rs`, `src/mcp/resources.rs` — anchor tool.
- `src/commands/admin.rs`, `src/commands.rs`, `src/repl.rs`, `src/wire.rs`.
- `src/rpc/kern_rpc_server.rs`, `shared/trnsprt/src/kern_rpc/{dto,svc,mock}.rs`.
- `src/retrieval/digest.rs`, `src/viewer.rs`.
- Docs: `docs/book/src/guides/memory-bank.md`, architecture guide.

## Testing

- Unit: `route_to_child_id` returns a child only at `p ≥ ACCEPT_FLOOR`; below →
  `None` → generic.
- Unit: two anchors, entity nearer one routes there; tie → higher `p`.
- Unit: generic is reachable as fallback and is a normal searchable subtree.
- Unit: `anchor add` creates a named root child; `list`/`remove` behave.
- Promotion: seeded dense generic cluster promotes to a root-level anchor.
- Persist round-trip on a fixture **written with the old field names** decodes
  intact under the new names (guards the bincode-positional assumption).

## Out of scope

- Per-node (non-root) anchor sets.
- Multi-home entities (one home per entity).
- Data-folder relocation of `.kern` files — tracked separately.
