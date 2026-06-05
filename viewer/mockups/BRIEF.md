# Design brief — kern viewer UI

## What it is
A live viewer for **kern**, a per-project memory daemon. It renders the daemon's
knowledge graph in the browser (Vue 3 + Vite single-file `App.vue`, polls
`GET /graph` every 5s). One viewer aggregates **many daemons at once** (multiple
projects merged), so the UI must handle a forest of roots, not one tree.

The job of the UI is **reach**: let someone find any thought, and understand why
it matters, in very few strokes. It is an instrument for reading a mind's memory
— not a dashboard, not a social feed.

## Data it shows (the whole model — surface all of it)
- **Thoughts** (nodes): `label` (text), `kind` ∈ {Fact ◆, Document ■, Question ▲, Claim ●},
  `heat` (unbounded float, recency/importance), `conf` (0–1 confidence), `kern` (its group).
- **Groups** (kerns): `label` (purpose), `count` (members), `parent`/`children` (tree), `named`.
- **Reasons** (edges, directed): `kind` ∈ {Supersedes ↟, Ratification ✓, Question ?,
  Similarity ≈, Provenance ⌖, Rephrase ↺, Spawn ✶}, `score` (0–1 strength), `text`.
- Top level: `daemons` (how many projects merged).

## Interaction model — PRESERVE THIS, it's decided
One subject, two views:
- **Anchor (hero)** — the single thought you're looking at, large. Kind, group, heat, confidence, reason count.
- **STRUCTURE (left)** — walk the kern tree (groups → thoughts). Click a thought to anchor it.
- **REASONS (right)** — the anchor's reason edges. Click / press `1–4` to walk to a neighbour (re-anchor).

Find in few strokes:
- type anywhere → fuzzy finder across thoughts · groups · reasons, match-highlighted
- `↑↓` select · `↵` land (anchors + jumps structure to its group) · `1–4` walk · `/` focus · `Esc` up/clear

Show only the top items per view (4), ranked by heat, with an honest "4 / 23" count and a "+N" affordance. Never imply you're showing everything when you're not.

## Aesthetic direction — "instrument, not terminal"
Start from a **monospace, structured, engineering-log** discipline: hairline rules,
a tight grid, everything aligned, data legible at a glance. Think field instrument
/ aircraft readout / serious technical log — calm, exact, trustworthy.

But it must read **serious and restrained**, NOT hacker/sci-fi. Explicitly:

**KILL (too techy):**
- no CRT scanlines, no screen flicker, no blinking cursor
- no neon glow / text-shadow halos / phosphor bloom
- no faux-ASCII chrome (`◢ ANCHOR`, box-drawing borders as decoration)
- no matrix green; no saturated electric accents competing with the data
- no purple gradients, no glassmorphism (the usual AI defaults)

**KEEP / DO:**
- monospace as the structural voice; consider one quiet humanist/grotesque or serif
  face for the anchor headline so the subject feels human, not a log line
- near-black or deep warm-charcoal ground; muted warm-grey text; ONE restrained accent
- let the **heat ramp** (cold→hot, deep ember → warm amber) be the only real colour —
  it carries meaning, so nothing else should shout over it
- hairline 1px rules and generous, disciplined whitespace instead of borders/shadows for separation
- precise alignment, baseline rhythm, small-caps or letterspaced labels used sparingly
- motion: minimal and functional only (a quiet state change, a brief reveal). No looping, no glow pulses.

Mood words: **serious, exact, quiet, legible, engineered.** Like a well-made
measurement tool you trust.

## Layout regions (top → bottom)
1. **Rail** — `kern` wordmark, live counts (thoughts · groups · daemons), a small heat legend.
2. **Anchor** — the focused thought, large, with its metadata line.
3. **Two panels** — STRUCTURE | REASONS, equal, hairline-separated.
4. **Finder** — a single input that becomes a ranked, keyboard-driven result list when typing.

## Constraints
- Single Vue 3 SFC (`<script setup>` + `<template>` + `<style>`); the logic already exists — this is a **reskin**, keep all functions/refs/handlers intact.
- Fonts via Google Fonts `<link>`; pick characterful ones, avoid Inter/Roboto/Arial and avoid Space Grotesk (overused). A distinctive mono + one display face.
- Heat colour comes from a d3 ramp already in code; you may restyle the stops but keep cold→hot legible on the ground colour, and keep readable text contrast on coloured surfaces.
- 60fps, no heavy libraries beyond the existing d3. Works at laptop and wide-desktop widths.

## Deliverable
Three restrained variations on this instrument direction (different type pairings /
ground tones / how the anchor headline contrasts the mono body), each as a working
self-contained mockup. Pick the one that feels most serious and most legible at a
glance — where you can find a thought without thinking about the UI.

## Success criteria
- A stranger can find a named thought in ≤4 keystrokes and explain what it's connected to.
- Nothing on screen reads as "sci-fi" or "demo"; it reads as a serious tool.
- Heat is the only thing using saturated colour, and it's instantly readable.
- Calm: no element competes for attention except the thing you're looking at.
