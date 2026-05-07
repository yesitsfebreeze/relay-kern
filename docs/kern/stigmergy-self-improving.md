# Stigmergy Models for Self-Improving kern Memory

**Ticket:** N98TSKFZ
**Status:** Research / design
**Decision:** **Adopt-modified** — formalise the existing `access_count` +
`accessed_at` + `pulse` machinery as an explicit stigmergic pheromone field.
Introduce a single continuous `heat` scalar (reinforced on access, decayed on
tick) with a tunable half-life. Retrieval already consumes this signal via
`qbst`; this proposal *names* the model, tightens the formula, and adds a
convergence metric so we can answer "is the bell curve actually converging?".

---

## 1. Problem

kern's retrieval relies on `qbst` (Query-Biased Structural Traction) in
`crates/retrieval/src/score.rs`:

```rust
let access = (access_count as f64 + 1.0).ln() * cfg.qbst_access_weight;
let recency = cfg.qbst_recency_weight * (-age / half_life).exp();
(access + recency).min(cfg.qbst_cap)
```

This mixes two loosely-coupled signals (cumulative count, exponential
recency) and caps the sum at `QBST_CAP=0.1`. The pulse mechanism
(`crates/tick/src/pulse.rs`) walks the Kern tree with geometric decay
(`PULSE_DECAY=0.5`, `PULSE_THRESHOLD=0.05`) but only enqueues clustering
tasks — **it never writes to `access_count` or `last_access`**. So pulse
does not currently feed retrieval's freshness signal.

We have the *shape* of a stigmergic system (trace + decay) without the
discipline of one. Consequences:

- No principled way to tune the decay rate. `QBST_RECENCY_HALF_LIFE` is
  literally hard-coded to 24h.
- Two redundant freshness knobs (`access_count` log-growth and
  `accessed_at` exponential recency) interact in non-obvious ways.
- No observability on whether the corpus converges on "hot paths" — we
  cannot answer whether the access distribution is bell-curved,
  Pareto, or flat.
- Pulse is pure overhead right now (kicks clustering) and does not
  reinforce the trails it walks.

## 2. Stigmergy primer

Stigmergy: agents modify a shared environment; other agents respond to
the modification. Classic case — ant colonies laying pheromones on paths
they traverse; pheromones evaporate; short paths accumulate more
pheromone per unit time; colony converges on short paths without central
coordination. Ant Colony Optimisation (ACO) formalises this:

```
τ_ij(t+1) = (1 − ρ) · τ_ij(t) + Σ_k Δτ_ij^k
```

- `τ_ij` — pheromone on edge `ij`
- `ρ ∈ (0,1]` — evaporation rate per tick
- `Δτ_ij^k` — deposit from agent `k` that used edge `ij`

Three knobs only: reinforcement magnitude, evaporation rate, threshold
floor below which a trail is pruned. Everything else is emergent.

**Mapping to kern:**

| ACO concept          | kern analogue                                 |
|----------------------|-----------------------------------------------|
| Edge `ij`            | `Thought` (node-level pheromone, phase 1) or `Reason` (phase 2) |
| Pheromone `τ`        | `heat: f32` derived from `access_count` + `accessed_at` |
| Deposit `Δτ`         | `+1` on retrieval hit, `+w` on pulse crossing |
| Evaporation `ρ`      | Per-tick multiplicative decay                 |
| Threshold floor      | `MIN_DELIVER_SCORE` / cold-thought demotion   |
| Ant trail            | Query → retrieval path                        |

## 3. Formula proposal

### 3.1 Unified heat scalar

Replace the two-term `qbst` with a single continuous field `heat ∈ [0, ∞)`
stored per thought. Compute lazily (no extra storage):

```
heat(t) = access_count · exp(−λ · (t − accessed_at))
```

where `λ = ln(2) / half_life`. This is the **exponentially-weighted access
count** — the unique functional form that (a) reinforces linearly on hit
and (b) decays exponentially between hits, with one parameter.

Update rule (on retrieval hit at time `t`):

```
heat'     = heat(t) + 1
access_count  := round(heat')            // keep the existing i32 field
accessed_at   := t
```

Equivalent closed form — no tick required to compute, just to *observe*.

### 3.2 Scoring integration

Replace `qbst` body with:

```rust
pub fn heat(cfg: &RetrievalConfig, access_count: i32, accessed_at: Option<SystemTime>) -> f64 {
    let Some(at) = accessed_at else { return 0.0 };
    let age = now().duration_since(at).unwrap_or_default().as_secs_f64();
    let lambda = std::f64::consts::LN_2 / cfg.heat_half_life_secs;
    let h = (access_count as f64) * (-lambda * age).exp();
    (cfg.heat_weight * (h + 1.0).ln()).min(cfg.heat_cap)
}
```

- `ln_1p` compresses so one thousand hits doesn't dominate one hit 10×.
- `heat_cap` prevents single super-hot thoughts from drowning vector signal.
- Single half-life param — orthogonal to everything else.

### 3.3 Pulse as pheromone deposit

Today `pulse` walks the tree but drops no trail. Modify
`crates/tick/src/pulse.rs` to reinforce Kern-level heat when it traverses:

```rust
if let Some(kern) = g.kern_loaded(child_id) {
    // Stigmergic deposit proportional to surviving strength.
    kern_heat_deposit(g, &kern.id, strength * PULSE_DEPOSIT_SCALE);
    pulse(q, g, kern, reduced);
}
```

`kern_heat_deposit` increments `Kern.access_count` by `strength` (writes
to existing field, touches `last_access`). Kerns that are upstream of
frequent retrievals accumulate pheromone → crystallise preferentially →
short paths win.

## 4. Tuning the decay rate

Two free parameters: `heat_half_life_secs` (λ) and `PULSE_DEPOSIT_SCALE`.

### 4.1 Half-life derivation

Pick half-life so a thought that was hot a week ago but never touched
since loses to a thought that just got one hit:

```
access_count · 2^(−age_days / T_half)  <  1
```

Solving for `T_half` given `access_count = 32, age_days = 7`:
`T_half < 7 / log2(32) = 1.4 days`. So a half-life around **1–2 days**
matches intuition that a week of silence overrides a month of heat. Make
this env-configurable:

```
KERN_HEAT_HALF_LIFE_SECS   (default 129_600  = 36h)
KERN_HEAT_WEIGHT           (default 0.08)
KERN_HEAT_CAP              (default 0.15)
KERN_PULSE_DEPOSIT_SCALE   (default 0.1)
```

### 4.2 Tuning procedure

1. **Offline replay.** Record a week of retrieval traces (query, returned
   ids, feedback). For each candidate `(half_life, weight)` pair,
   recompute rankings and measure NDCG@10 vs. observed clicks / forget
   events.
2. **Sweep.** Grid over `half_life ∈ {6h, 12h, 1d, 2d, 1w}` and
   `weight ∈ {0.02, 0.05, 0.1, 0.2}`.
3. **Select** the pair maximising NDCG subject to the entropy
   constraint from §5.1 (don't let heat flatten the ranking).
4. **Monitor** in prod via the metric in §5 and adjust quarterly.

## 5. Convergence metric — "efficient path"

The question we owe an answer to: *is the access distribution converging
on a bell curve / Pareto, or is it diffuse?* Add a single periodic
metric, computed at the same cadence as `tick` (~hourly):

### 5.1 Heat Gini coefficient

```
G = Σ_i Σ_j |h_i − h_j| / (2 · n² · mean(h))
```

- `G = 0` → uniform heat (no convergence, every thought equally hot).
- `G → 1` → power-law (a few trails dominate, ACO-style convergence).

Record `G` to `prometheus`/metrics each tick. Plot over time.

**Convergence criterion:** a kern subgraph has *converged on an efficient
path* if:

1. `G ≥ 0.6` sustained over ≥ 24h (distribution is concentrated), **and**
2. Top-10 thoughts by heat have `stability ≥ 0.8` — i.e. 8 of the top
   10 from one hour ago are still in the top 10 now (rank churn is low),
   **and**
3. Median retrieval path length (thoughts touched per query before hit)
   is **decreasing** week-over-week.

### 5.2 Implementation sketch

New crate module `crates/tick/src/stigmergy.rs`:

```rust
pub struct HeatStats {
    pub gini: f64,
    pub top10_stability: f64,
    pub median_path_len: f64,
    pub cold_fraction: f64,     // heat < floor
}

pub fn compute_heat_stats(g: &GraphGnn, prev_top10: &[ThoughtId]) -> HeatStats { ... }
```

Expose via `/metrics` endpoint and MCP `health` resource.

### 5.3 Cold-path pruning

Once `G ≥ 0.6` and a thought's `heat < heat_floor` for `≥ forget_ttl`
seconds, it becomes a candidate for `forget()` — the existing
`mcp__kern__forget` path. Stigmergy closes the loop: unused pheromone
evaporates → thought cools → automatic garbage collection.

## 6. Integration plan

### 6.1 `crates/tick/src/pulse.rs`

- Add `PULSE_DEPOSIT_SCALE` to `base::constants`.
- In the child-recursion branch, call `kern.record_access_deposit(strength)`
  (new method on `Kern` that increments `access_count` by `strength.round()`
  and sets `last_access`).
- **No API break** — `pulse` signature unchanged.

### 6.2 `crates/retrieval/src/score.rs`

- Rename `qbst` → `heat_boost` (keep a compat alias for one release).
- Collapse `qbst_access_weight + qbst_recency_weight + qbst_recency_half_life`
  into `heat_weight + heat_half_life_secs + heat_cap`.
- `commit_access` stays identical.

### 6.3 `crates/tick/src/stigmergy.rs` (new, ~80 LOC)

- `compute_heat_stats` (see §5.2).
- Unit tests for Gini on synthetic distributions (uniform → 0, dirac → 1).

### 6.4 `crates/env/src/lib.rs`

- New vars: `KERN_HEAT_HALF_LIFE_SECS`, `KERN_HEAT_WEIGHT`, `KERN_HEAT_CAP`,
  `KERN_PULSE_DEPOSIT_SCALE`.
- Keep old `KERN_RETRIEVAL_*` vars reading through with deprecation log.

### 6.5 `crates/server/src/lib.rs`, `crates/mcp/src/resources.rs`

- Export `HeatStats` in `/health` and MCP `kern://health` resource.

## 7. Failure modes & mitigations

| Mode                                      | Mitigation                                           |
|-------------------------------------------|------------------------------------------------------|
| Hot-spot lock-in (rich-get-richer)        | `heat_cap` + `ln_1p` compression                     |
| Query adversary pumping one thought       | Rate-limit `commit_access` per (producer, thought)   |
| Clock skew between nodes                  | Use local `SystemTime` only; never compare across peers |
| Half-life mis-tuned → thrash              | Entropy floor: if Gini < 0.2 for 72h, alert          |
| Cold-path false-positive forgets          | Require `heat < floor` AND `age > min_age_days`      |

## 8. What we explicitly do **not** adopt

- Per-reason pheromone (phase 2 only — storage cost, weak signal until
  reason graph density grows).
- Multiple pheromone types (food/danger in real ACO). Single heat scalar
  suffices; we already have `ThoughtKind` for orthogonal typing.
- Mass-action / fluid stigmergy (Bonabeau). Discrete decay matches
  tick-based architecture better.
- Cross-peer heat gossip. Heat is *local* provenance; authority is the
  federation-level signal (see `pagerank-authority.md`).

## 9. Decision

**Adopt.** Rename `qbst` → `heat`, collapse to a single half-life param,
make `pulse` deposit heat on traversal, and ship the Gini + top-10
stability metric so we can finally answer the bell-curve question with
data. No schema changes — reuse existing `access_count` + `accessed_at`.

## 10. Acceptance checklist

- [x] Decay formula proposed (§3.1, closed-form, one parameter)
- [x] Integration plan touching `crates/tick/src/pulse.rs` (§6.1)
- [x] Path-efficiency metric defined + convergence criterion (§5)
- [ ] Implementation ticket filed (future work)
- [ ] Offline NDCG sweep to pick `heat_half_life_secs` default (future work)

## References

- Dorigo & Stützle, *Ant Colony Optimization*, MIT Press 2004.
- Theraulaz & Bonabeau, "A brief history of stigmergy", *Artificial Life* 1999.
- `crates/retrieval/src/score.rs` — current `qbst`
- `crates/tick/src/pulse.rs` — current pulse
- `docs/pagerank-authority.md` — federation-level authority (complementary)
