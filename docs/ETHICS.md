# Ethics

The non-negotiables. Everything in `relay-clean` answers to this document. If a feature, recipe, plugin, or merge violates an invariant here, it does not ship — regardless of utility.

Citations of the form `knids:<id>` point at thoughts in the project's knowledge graph (`mcp__knids__query` with `id`). The graph is the source of truth; this file is the human-readable mirror.

---

## I. Plurality Invariant

**Statement.** Relay never returns a single authoritative answer. Every output is a set of perspectives with provenance, weight, and dissent preserved. Collapse-to-one is malfunction, not feature. Choice belongs to the human.

**Why — the compound argument.** Single-answer systems compound errors centrally: one bad frame propagates to every downstream consumer with no exit. Plural systems distribute outcomes across human choice — some people pick the wrong answer, some pick the right one, but the AI is not the moral agent. The human picking from the set is. Both system shapes produce wrong answers in aggregate. Only one removes free will from the population. We pick the one that keeps the human in the loop.

**How it is enforced.**

- **`kern` query API.** No scalar-answer return type. Only `perspectives() -> Vec<Perspective>` carrying `belief`, `uncertainty`, `provenance`, `dissent_links`. Single-answer is unrepresentable in the schema. Property test: for any query, `|perspectives| >= 1` with provenance, and no API path returns `Option<Single>`.
- **`agnt` recipe receipts.** Structured, multi-frame. Never prose verdicts. Orchestrator forbidden from selecting one frame before the user sees the set. If a recipe returns prose-as-conclusion, treat the receipt as malformed.
- **`repl` rendering.** Default view shows the spread. Never collapses a perspective set to one row without an explicit user fold action.
- **Mesh-reasoner output type.** Distribution, not mode. Sampling, not argmax. Cite-back to node IDs is mandatory; outputs without provenance are not valid mesh-reasoner outputs.

**The non-rule.** This is not a politeness norm or a UX preference. It is a protocol invariant. A "helpful" mode that flips this off does not exist. A configuration flag to flip this off does not exist. A future deployment target that demands this be flipped off is refused.

**Source nodes.** `knids:4be420f8…` (invariant), `knids:46ae229d…` (origin principle), `knids:1b7ff04b…` (uncertainty as feature), `knids:7899b1c4…` (resist LLM-wrapper).

---

## II. Threat — Aggregation Attack

**The shape.** A larger actor — corporation, state, market aggregator — crawls, scrapes, buys, or subpoenas the N local oracles produced by federation, combines them, and ships the result as One Big Oracle that claims to have synthesized "all perspectives." Distributed substrate becomes centralized product. Plurality is harvested into monoculture and sold back with legitimacy laundering: *we asked everyone*. This is worse than starting centralized — it claims pluralist authority while collapsing it.

**Why it is hard.** Open data is open. Signed nodes are signable by anyone. Once content is published to the `mesh-public` tier, downstream synthesis is uncontrollable. Cryptography stops tampering, not aggregation.

**Defenses already in the design.**

- `mesh-trusted-group` is the default visibility for high-value perspective. `mesh-public` is opt-in per node, not per peer.
- The mesh-reasoner federates *gradient signal*, not text. Aggregator gets behavior, not shards.
- Manifests are signed, expiring, forkable. Any "unified Relay oracle" claim is a forkable manifest, not a protocol fact.

**Defenses to add at the protocol level.**

1. **Plurality as output type, not policy.** Enforced by Invariant I above.
2. **Disagreement preservation as first-class.** `(belief, uncertainty)` tuples replacing scalar `confidence`. Queries return distributions, not modes.
3. **No-synthesis bit on signed nodes.** Norm-level marker. Synthesis without attribution becomes detectable and shameable. Norm beats law in open systems.
4. **Provenance non-strippable.** Every mesh-reasoner output cites back to node IDs. Strippers get caught. "Show your sources or you're lying" becomes testable.
5. **Asymmetric federation rate-limit detection.** A datacenter peer that consumes 1000× what it contributes is a visible asymmetry. Detection in protocol; refusal per-peer.

**The realistic win.** We cannot fully prevent aggregation. We can make it expensive, detectable, and illegitimate. Single-answer aggregation is a *malfunction signature*, not a UX choice.

**Source node.** `knids:8cc7c65d…`.

---

## III. Read / Propose / Commit Separation

The reasoner does not write to the substrate. Three layers, architecturally enforced, not policy-enforced:

1. **READ.** Reasoner has free read access to the graph.
2. **PROPOSE.** Reasoner emits proposals. Queued, unsigned, never live in the graph.
3. **COMMIT.** Only the curator (human or trust policy) holds the ed25519 signing key. Curator signs proposals into the graph.

Three things must never mix in one process: reasoner ↔ graph writes, reasoner ↔ signing key, reasoner ↔ commit authority.

**Why.** A reasoner that can write its own conclusions into the substrate it later reads from is a closed loop with no human checkpoint. Plurality Invariant prevents output collapse; Read/Propose/Commit prevents state collapse. Both are needed. Together they keep the human as the moral agent.

**Source node.** `knids:31b38dba…`.

---

## IV. Local-First, Federation-as-Consent

Nothing leaves the device without an explicit shared-write step. Visibility is per-node, not per-peer:

- `local` — never published. Default for ingested folders, drafts, journals, NDA work.
- `mesh-trusted-group` — encrypted to a peer set. Opt-in.
- `mesh-public` — signed, gossiped. Opt-in per node.

Federation is asymmetric and refusable. A laptop peer contributes what it can; a datacenter peer can run merges. There is no aggregator, bank, or reviewer pool in the protocol. *If you disagree, fork.* Capture is refused at the protocol layer, not relitigated per release.

---

## V. What Relay Is Not

- Not a verdict system.
- Not a content policy engine.
- Not a moral authority.
- Not a targeting system.
- Not a reputation weapon.

Reputation, flags, governance mechanics are not weapons. Hidden scoring, manifest fraud, capability-manifest lies, and coordinated bank-capture attempts are explicitly in the threat model and explicitly unwelcome. Power that cannot be inspected or refused is not legitimate power. The protocol is designed to refuse capture. If you want a tool that decides for the user, build something else.

---

## VI. The Human Is the Moral Agent

The user is the one choosing. Relay's job is to widen the choice space, surface dissent, attach provenance, and stay out of the way. The system is a sail, not a rudder. Wind direction belongs to the world; sail angle belongs to the human.

A tool that tells you what to think is an oracle. We are not building an oracle. We are building the substrate that makes oracles unnecessary.
