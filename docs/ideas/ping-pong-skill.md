# Ping-Pong Skill Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Author the `ping-pong` skill at `.claude/skills/ping-pong/SKILL.md` so future sessions can apply the in-file ping-pong rolling implementation pattern with a `@score` header rubric.

**Architecture:** Single skill file (~150 lines) under `.claude/skills/ping-pong/`. No code changes, no macro crate, no tooling — those are out of scope per the spec. Verification is grep-based: each required section is present and non-empty.

**Tech Stack:** Markdown only. Verification via `grep` and manual skill invocation.

**Spec:** [`docs/superpowers/specs/2026-04-25-ping-pong-skill-design.md`](../specs/2026-04-25-ping-pong-skill-design.md)

---

### Task 1: Scaffold skill directory and frontmatter

**Files:**
- Create: `.claude/skills/ping-pong/SKILL.md`

- [ ] **Step 1: Create directory and stub file with frontmatter**

```bash
mkdir -p .claude/skills/ping-pong
```

Write `.claude/skills/ping-pong/SKILL.md` with exactly this content (sections will be appended in later tasks):

```markdown
---
name: ping-pong
description: Use when changing the behavior of an existing function, when promoting a tested improvement, or when adding/updating the @score header on a Rust file. Enforces the in-file ping-pong pattern (frozen previous arm + standalone current arm) and the 7-category score rubric. Refuses fallback chains and unscored files.
---

# Ping-Pong Rolling Implementation

Every functional change to an existing function lands as an in-file ping-pong pattern: a frozen `previous` arm and a standalone `current` arm. Promotion compacts the two into a fresh `previous` and drafts a new `current`. Each `.rs` file carries a `@score` header so repo health is measurable and bad files surface to grep.
```

- [ ] **Step 2: Verify frontmatter present**

Run: `grep -c '^name: ping-pong$' .claude/skills/ping-pong/SKILL.md`
Expected: `1`

- [ ] **Step 3: Commit**

```bash
git add .claude/skills/ping-pong/SKILL.md
git commit -m "skill(ping-pong): scaffold skill file with frontmatter"
```

---

### Task 2: Add invariants section

**Files:**
- Modify: `.claude/skills/ping-pong/SKILL.md` (append)

- [ ] **Step 1: Append invariants section**

Append to `.claude/skills/ping-pong/SKILL.md`:

```markdown

## Invariants

1. **At most ping pongs** — a function has at most two implementations at any time, both inside the same `.rs` file.
2. **Current is standalone** — the `current` arm must not call the `previous` arm. Needing a fallback is a failure signal; reject the change.
3. **Deprecated is frozen** — no edits to the `previous` arm during normal improvement. Edited only during a promotion compaction step.
4. **Every `.rs` file has a `@score` header** — missing header blocks merge.
```

- [ ] **Step 2: Verify section present**

Run: `grep -c '^## Invariants$' .claude/skills/ping-pong/SKILL.md`
Expected: `1`

- [ ] **Step 3: Commit**

```bash
git add .claude/skills/ping-pong/SKILL.md
git commit -m "skill(ping-pong): add invariants section"
```

---

### Task 3: Add score header rubric

**Files:**
- Modify: `.claude/skills/ping-pong/SKILL.md` (append)

- [ ] **Step 1: Append score header section**

Append to `.claude/skills/ping-pong/SKILL.md`:

````markdown

## Score Header

The first non-shebang lines of every `.rs` file are:

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

- `@score N` is `round(cosine_similarity(scores, [100; 7]) * 100)`. Single grep target.
- Subcategories are the seven below, each integer 1–100.
- Repo health = arithmetic mean of all `@score` values across `.rs` files.
- Worst-first triage: `grep -rn '@score' src/ | sort -k3 -n`.

### Categories

| Category      | Asks                                                                 |
|---------------|----------------------------------------------------------------------|
| clarity       | Can a new reader understand intent without scrolling or guessing?    |
| correctness   | Does it produce right answers on inputs it claims to handle?         |
| performance   | Does it avoid wasted work, allocations, and quadratic shapes?        |
| isolation     | Are dependencies narrow and explicit? Testable in isolation?         |
| testability   | Are seams present? Can behavior be exercised without the world?      |
| simplicity    | Is it the smallest design that satisfies the contract?               |
| safety        | Bounds, overflow, panics, unsafe, secrets, injection — all guarded?  |

A 100 means "no concrete improvement comes to mind." A 30 means "I can name several things wrong." Be honest; the number drives triage.
````

- [ ] **Step 2: Verify all seven categories listed**

Run: `grep -E '^\| (clarity|correctness|performance|isolation|testability|simplicity|safety)' .claude/skills/ping-pong/SKILL.md | wc -l`
Expected: `7`

- [ ] **Step 3: Commit**

```bash
git add .claude/skills/ping-pong/SKILL.md
git commit -m "skill(ping-pong): add score header rubric with 7 categories"
```

---

### Task 4: Add ping-pong pattern usage

**Files:**
- Modify: `.claude/skills/ping-pong/SKILL.md` (append)

- [ ] **Step 1: Append pattern section**

Append to `.claude/skills/ping-pong/SKILL.md`:

````markdown

## Ping-Pong Pattern

Preferred form (when the `ping_pong!` macro crate is available):

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

Expansion:
- `parse_v1` — frozen baseline, `#[deprecated(note = "ping_pong baseline")]`, public for eval.
- `parse` — current implementation, standalone.
- A registry entry so the eval harness can find the pair.

Fallback form (when the macro is not available — same file, plain Rust):

```rust
#[deprecated(note = "ping_pong baseline")]
pub fn parse_v1(s: &str) -> Result<Ast> {
    legacy_parser(s)
}

pub fn parse(s: &str) -> Result<Ast> {
    new_parser(s)
}
```

The skill enforces the **shape**, not the macro. Either form is acceptable.
````

- [ ] **Step 2: Verify both forms present**

Run: `grep -c 'ping_pong!' .claude/skills/ping-pong/SKILL.md`
Expected: `>=2`

Run: `grep -c '#\[deprecated' .claude/skills/ping-pong/SKILL.md`
Expected: `>=1`

- [ ] **Step 3: Commit**

```bash
git add .claude/skills/ping-pong/SKILL.md
git commit -m "skill(ping-pong): add macro and fallback pattern usage"
```

---

### Task 5: Add promotion procedure

**Files:**
- Modify: `.claude/skills/ping-pong/SKILL.md` (append)

- [ ] **Step 1: Append promotion section**

Append to `.claude/skills/ping-pong/SKILL.md`:

```markdown

## Promotion Procedure

Triggered when the user requests promotion, or when current passes all gates and beats previous on `@score`.

1. Run the eval harness: previous vs current, on all cases.
2. Confirm hard gates: build passes, tests pass, typecheck passes, lint passes, no behavioral regression.
3. Confirm `current.@score >= previous.@score`.
4. Delete the `previous` arm. Rename the `current` arm to `previous`. Apply `#[deprecated]`.
5. Draft a fresh `current` arm — initially identical to the new `previous`. The next improvement edits this arm.
6. Update the file `@score` header to reflect the new state.

Promotion is the **only** time the `previous` arm may be touched.
```

- [ ] **Step 2: Verify all 6 steps present**

Run: `grep -cE '^[1-6]\. ' .claude/skills/ping-pong/SKILL.md`
Expected: `>=6`

- [ ] **Step 3: Commit**

```bash
git add .claude/skills/ping-pong/SKILL.md
git commit -m "skill(ping-pong): add promotion procedure"
```

---

### Task 6: Add refusal conditions

**Files:**
- Modify: `.claude/skills/ping-pong/SKILL.md` (append)

- [ ] **Step 1: Append refusal section**

Append to `.claude/skills/ping-pong/SKILL.md`:

```markdown

## Refusal Conditions

Refuse to accept a change as an improvement, and refuse to promote, when any of:

- `current` calls `previous` (chain detected).
- More than ping pongs present for one logical function.
- The file is missing the `@score` header.
- Build, tests, typecheck, or lint fails.
- `current.@score < previous.@score`.
- Behavioral regression in eval.
- The user asks to edit `previous` outside of a promotion step.

When refusing, name the rule and the exact location. Do not silently fix.
```

- [ ] **Step 2: Verify section present**

Run: `grep -c '^## Refusal Conditions$' .claude/skills/ping-pong/SKILL.md`
Expected: `1`

- [ ] **Step 3: Commit**

```bash
git add .claude/skills/ping-pong/SKILL.md
git commit -m "skill(ping-pong): add refusal conditions"
```

---

### Task 7: Add report format

**Files:**
- Modify: `.claude/skills/ping-pong/SKILL.md` (append)

- [ ] **Step 1: Append report section**

Append to `.claude/skills/ping-pong/SKILL.md`:

````markdown

## Report Format

After running the skill on a feature, report in this shape:

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
````

- [ ] **Step 2: Verify section present**

Run: `grep -c '^## Report Format$' .claude/skills/ping-pong/SKILL.md`
Expected: `1`

- [ ] **Step 3: Commit**

```bash
git add .claude/skills/ping-pong/SKILL.md
git commit -m "skill(ping-pong): add report format"
```

---

### Task 8: Final structural verification

**Files:** none (verification only)

- [ ] **Step 1: Verify all required sections present**

Run:
```bash
grep -E '^## (Invariants|Score Header|Ping-Pong Pattern|Promotion Procedure|Refusal Conditions|Report Format)$' .claude/skills/ping-pong/SKILL.md | wc -l
```
Expected: `6`

- [ ] **Step 2: Verify file length is in target range**

Run: `wc -l .claude/skills/ping-pong/SKILL.md`
Expected: between 100 and 200 lines.

If outside range, trim or expand the offending section, then re-run.

- [ ] **Step 3: Verify no placeholders**

Run: `grep -nE 'TBD|TODO|FIXME|fill in|tbd' .claude/skills/ping-pong/SKILL.md`
Expected: no output.

- [ ] **Step 4: Smoke test — invoke skill on a real file**

Pick any `.rs` file in `src/bin/kern/src/` that does not yet have a `@score` header. In a fresh session, ask: "Use the ping-pong skill to add a `@score` header to <path>." The skill should produce a header with the seven categories and a `@score` line, and refuse to touch the rest of the file unless asked.

If the smoke test fails (skill misreads its own rules), edit the offending section and re-run.

- [ ] **Step 5: Final commit (only if any fixes were made in steps 2–4)**

```bash
git add .claude/skills/ping-pong/SKILL.md
git commit -m "skill(ping-pong): final structural fixes after verification"
```

---

## Self-Review Notes

- **Spec coverage:** purpose, invariants, score header (with 7 categories), macro + fallback, promotion procedure, refusal conditions, report format, out-of-scope — all mapped to tasks 1–7. Verification in task 8.
- **Placeholders:** none — every step has its full markdown content inline.
- **Type consistency:** all section headings used in verification (`grep -c '^## …$'`) match the headings written in append steps.
- **Out of scope honored:** no tasks for the `ping_pong!` macro crate, eval harness, or score tooling. The skill explicitly notes the macro is optional and the fallback form is acceptable.
