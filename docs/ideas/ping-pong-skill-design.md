# Ping-Pong Rolling Implementation — Skill Design

**Date:** 2026-04-25
**Status:** approved
**Skill path (target):** `.claude/skills/ping-pong/SKILL.md`

## Purpose

Force every functional change to land as an in-file ping-pong pattern: a frozen `previous` arm and a standalone `current` arm. Promotion compacts the two into a fresh `previous`, and a new `current` is drafted on top. No version chains. No silent fallbacks. Each file carries a score header so repo health is measurable and bad files surface to grep.

## Invariants

1. At most **two** implementations of any one logical function exist at a time, both inside the same `.rs` file.
2. `current` is **standalone**. It must not call `previous`. Needing a fallback is a failure signal — reject the change.
3. `previous` is **frozen**. No edits during normal improvement work. Edited only during promotion compaction.
4. Every `.rs` file has a `@score` header. Missing header blocks merge.

## Score Header

Top of every `.rs` file:

```rust
//! @score 28
//! clarity: 30
//! correctness: 40
//! performance: 20
//! isolation: 15
//! testability: 25
//! simplicity: 35
//! safety: 30
```

- `@score N` = `round(cosine_similarity(scores, [100; 7]) * 100)`. Single-token grep target.
- Subcategories are fixed at 7. Each 1–100.
- Repo health = arithmetic mean of all `@score` values across `.rs` files.
- Triage: `grep -rn '@score' src/ | sort -k3 -n` lists worst files first.

### Category definitions

| Category      | Asks                                                                 |
|---------------|----------------------------------------------------------------------|
| clarity       | Can a new reader understand intent without scrolling or guessing?    |
| correctness   | Does it produce right answers on inputs it claims to handle?         |
| performance   | Does it avoid wasted work, allocations, and quadratic shapes?        |
| isolation     | Are dependencies narrow and explicit? Can it be tested in isolation? |
| testability   | Are seams present? Can behavior be exercised without the world?      |
| simplicity    | Is it the smallest design that satisfies the contract?               |
| safety        | Bounds, overflow, panics, unsafe, secrets, injection — all guarded?  |

Scoring is honest self-assessment. A 100 means "no concrete improvement comes to mind." A 30 means "I can name several things wrong with this."

## Ping-Pong Macro

```rust
ping_pong! {
    previous fn parse(s: &str) -> Result<Ast> {
        legacy_parser(s)
    }
    current fn parse(s: &str) -> Result<Ast> {
        new_parser(s)
    }
}
```

Expands to:
- `parse_v1` — frozen baseline, `#[deprecated(note = "ping_pong baseline")]`, public for eval.
- `parse` — current implementation, standalone.
- A registry entry (via `inventory` or similar) so the eval harness finds the pair.

When the macro crate is unavailable, the fallback convention is plain naming: `fn parse_v1(...)` + `fn parse(...)` in the same file, with `#[deprecated]` on the baseline. Skill enforces shape, not the macro per se.

## Promotion Procedure

Trigger: user requests promotion, or current passes all gates and beats previous on `@score`.

1. Run the eval harness: previous vs current, on all cases.
2. Confirm hard gates: build, tests, typecheck, lint, no behavioral regression.
3. Confirm `current.@score >= previous.@score`.
4. Delete the `previous` arm. Rename `current` arm to `previous`. Apply `#[deprecated]`.
5. Draft a fresh empty `current` arm (initially identical to the new previous — agent then proposes the next improvement).
6. Update file header `@score` to reflect the new state.

## Refusal Conditions

The skill refuses to promote, and refuses to accept a change as "improvement," when any of:
- `current` calls `previous` (chain detected).
- More than ping pongs present for one logical function.
- File missing `@score` header.
- Build, test, typecheck, or lint fails.
- `current.@score < previous.@score`.
- Behavioral regression in eval.
- User asks to edit `previous` outside of a promotion step.

## Report Format

After running the skill on a feature:

```
## Ping-Pong Report

Invariant: ok | violated (reason)
File: <path>
Header score: <N>  (was: <M>)

Eval:
  previous_score: …
  current_score:    …
  delta:            …
  hard failures:    …
  regressions:      …

Decision: promote | reject | manual_review
Compaction: not needed | done | failed
Notes: …
```

## Skill File Shape

`.claude/skills/ping-pong/SKILL.md` — target length ~150 lines:

1. Frontmatter (`name`, `description`, triggers).
2. Purpose + invariants.
3. Score header rubric (7 categories with one-line definitions).
4. `ping_pong!` macro usage + naming-convention fallback.
5. Promotion procedure.
6. Refusal conditions.
7. Report format.

Macro crate implementation is **out of scope** for this skill. Skill assumes the macro exists at a known path or that the project uses the naming-convention fallback.

## Out of Scope

- Building the `ping_pong!` macro crate.
- Building the eval harness.
- Cross-file or cross-crate ping-pong patterns.
- Automated score computation tooling. Scores are written by the agent on save and reviewed by humans.
