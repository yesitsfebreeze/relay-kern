# Ping-Pong Skill — Addendum: closing the autonomy loop

**Date:** 2026-04-25
**Status:** draft
**Extends:** [`2026-04-25-ping-pong-skill-design.md`](./2026-04-25-ping-pong-skill-design.md)

## Why this exists

The base spec defines the *shape* (two slots, `@score` header, promotion procedure) but leaves three load-bearing pieces out of scope: the eval harness, objective scoring inputs, and the autonomous driver that runs the loop. Without those, `@score` is LLM self-rating (Goodhart drift) and "self-sufficient improvement" is aspirational. This addendum specifies the minimal additions needed to make the loop honest and runnable.

## 1. Score: hybrid objective + LLM, gated on min

Two independent raters per category. Tests catch correctness/perf the LLM rationalises past. LLM catches clarity/structure tests cannot see. Averaging alone hides disagreement — keep both visible, gate on the worse one.

### 1a. Objective rater (`obj_score`)

Deterministic mapping from measurable signals to 1–100 per category:

| Category    | Signal                                                                    |
|-------------|---------------------------------------------------------------------------|
| clarity     | `clippy::cognitive_complexity` + line count vs sibling-file median        |
| correctness | eval-harness pass rate + property-test/mutation kill rate                 |
| performance | criterion bench delta vs `previous` (ns/iter, allocs)                     |
| isolation   | `cargo modules` fan-in/fan-out + count of `pub(crate)` leaks              |
| testability | branch coverage on this file (llvm-cov) + test-to-code line ratio         |
| simplicity  | LOC + cyclomatic; penalise above sibling-file median                      |
| safety      | `cargo audit` + `cargo geiger` unsafe count + clippy `correctness` lints  |

Mapping function lives in `ping_pong_eval::scoring`. Pure function of measurements → integer. No LLM. Reproducible.

### 1b. LLM rater (`llm_score`)

Same seven categories, 1–100, scored by an LLM run with fixed prompt template, temperature 0, model ID pinned. Cached on `hash(file_content + prompt_version + model_id)` so identical inputs always return identical scores. Re-rated only when file changes or prompt-version bumps.

**Anti-collusion:** the model that wrote (or last edited) the `current` arm may not rate it. Rater session is separate, no transcript carry-over. Self-graded homework is rejected at the driver level.

### 1c. Combination

Per category:
- `display = (obj + llm) / 2` — for human triage and the `@health` value.
- `gate    = min(obj, llm)` — for promotion gating. Worst rater wins.
- `disagree = |obj - llm| > 20` — flagged in header, surfaces to human review queue.

Aggregation across the seven categories: **min(gate per category)** is `@score`. **mean(display per category)** is `@health`. Cosine dropped — collapses imbalanced files to similar numbers.

### 1d. Header

```rust
//! @score 30                          // min of (gate per category)
//! @health 58                         // mean of (display per category)
//! @rater_model claude-opus-4-7
//! @prompt_version 1
//! clarity:     obj=80 llm=70  (75)
//! correctness: obj=90 llm=85  (88)
//! performance: obj=95 llm=80  (88)
//! isolation:   obj=60 llm=55  (58)
//! testability: obj=40 llm=35  (38)
//! simplicity:  obj=70 llm=30  (50, disagree)
//! safety:      obj=50 llm=50  (50)
```

`@score` stays single grep target. `disagree` flag is the second triage signal — files with disagreement go to human review even if `@score` looks fine.

### 1e. Override

Either rater may override an `obj` measurement by ±10 with written justification (e.g. `safety: obj=80 (override +5: input bounded by caller, see L42)`). Override appears in commit message and decays after 90 days unless re-asserted.

## 2. Eval harness

Minimal shape to make promotion decisions falsifiable.

**Crate:** `src/ping_pong_eval/` (new).

**Inputs:**
- A `#[ping_pong_case]` attribute marking test functions that exercise both arms.
- Each case: `fn(impl Fn(I)->O) -> CaseResult` — given an arm, returns `{passed, latency_ns, allocs, panic, output_hash}`.

**Run:** `cargo run -p ping_pong_eval -- <file>` discovers via `inventory`, runs every case against both `name_v1` (the previous arm) and `name` (the current arm), emits JSON:

```json
{
  "function": "parse",
  "previous": { "pass": 18, "fail": 2, "p50_ns": 412, "allocs": 3 },
  "current":  { "pass": 20, "fail": 0, "p50_ns": 280, "allocs": 1 },
  "delta":    { "pass": +2, "p50_ns": -132, "allocs": -2 },
  "regressions": []
}
```

**Hard-fail conditions** (block promotion regardless of `@score`):
- Any case `panic` in `current` that didn't panic in `previous`.
- Any `output_hash` divergence on a case marked `#[behavioral_invariant]`.
- p99 latency regression >20% with no allocation reduction.

## 3. Global gate

Per-file `@score` improvement is necessary, not sufficient. Promotion still requires:

1. `cargo test --workspace` green.
2. `cargo clippy --workspace -- -D warnings` green.
3. **Repo-health non-regression:** mean `@score` across all `.rs` files must not drop. Touched file may drop iff at least one other rises by ≥ same delta in same commit.
4. Kern-graph health: `mcp__kern__health` `unnamed` count not increased; `reasons/thoughts` ratio ≥ baseline. (Catches refactors that orphan memories.)

## 4. Autonomy driver

New skill: `.claude/skills/ping-pong-driver/SKILL.md`. Triggered by `/loop` or scheduled cron.

**Each tick:**

1. `grep -rn '@score' src/ | sort -k3 -n | head -1` → worst file.
2. Spawn sub-agent with full file content + score breakdown + recent eval JSON.
3. Sub-agent edits *only* the `current` arm. May not touch `previous`. May update `@score` header.
4. Driver runs eval harness + global gate.
5. **Outcomes:**
   - Promotion eligible → invoke `ping-pong` skill promotion procedure → commit.
   - Improvement but not promotion (current better than prior current, not yet better than previous) → keep new current, commit, next tick.
   - No improvement / regression → revert current to prior, log attempt, next tick.

**Budget:** per-cycle token budget (default 100k). Cycle aborts on overrun.

## 5. Stuck-state escape

Two slots = one rollback. Add bounded retries before human handoff.

- Per-file counter `@attempts: N` in header. Increments on each rejected current. Resets on successful promotion.
- At `@attempts >= 5`: file enters `manual_review`. Driver skips it. Surfaced via `grep -rn '@attempts' | awk '$NF>=5'`.
- Optional escape valve: when stuck, driver may *replace* `previous` with a known-good external reference impl (e.g. stdlib equivalent) once, marked `@reset: <date> <reason>` in header. One-shot per file per quarter.

## 6. Plateau / stop condition

Driver stops the loop (not just the file) when:

- Mean repo-health delta over last 20 cycles < 0.5.
- OR cumulative cycle cost since last promotion > $X (configurable).

Stop = pause, not abandon. Resumes on user command or new code merged in.

## Out of scope (still)

- Cross-file ping-pong. Per-function only.
- Multi-arm ensemble. Two slots stays two.
- Distributed eval. Local `cargo` only.
- Replacing the LLM with a learned scoring model. Override audit + min-gate is the discipline; ML stays future work.

## Open questions

- Should `@attempts` live in the file (grep-friendly, churns commits) or in a sidecar `target/ping_pong_state.json` (clean diffs, less visible)? Lean: sidecar; file only carries score.
- Does the eval harness double as bench? Probably yes — criterion integration not separate harness.
- How does the driver pick *which* function in the worst file? Heuristic: function with largest delta between its tests' assertions and the file's measured min-category. Defer to v2.
