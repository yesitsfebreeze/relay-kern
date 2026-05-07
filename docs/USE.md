# Use

Relay is built to help people share understanding with each other and
with agents. The design assumes **good-faith use**. The code does not,
and cannot, enforce that.

This document is the line: what Relay is for, what it is not for, and
what the project will not help anyone do.

## Intended use

- Local knowledge work: drafts, notes, memory, agent loops, graph
  construction on your own device.
- Voluntary federation: sharing understanding with peers who chose the
  same trust sets, banks, and aggregators.
- Research and tooling around retrieval, embeddings, GNNs, and
  agent-driven reasoning.
- Honest authorship: artifacts you produced, attested under your own
  identity (or a pseudonymous one you stand behind).

Local-first is the default. Nothing leaves your device without an
explicit shared-write step.

## Do not use Relay to

- **Harass, dox, stalk, or target individuals or groups.** Reputation,
  flags, and governance mechanics are not weapons.
- **Operate coerced identities.** Running a keypair under duress, or
  coercing someone else into doing so, breaks the human-in-loop gate
  (see [SECURITY.md](./SECURITY.md) §Duress).
- **Poison the graph.** Mass-generated low-quality writes,
  reputation-laundering rings, flag brigades, vote-stacking on appeals,
  or coordinated bank-capture attempts are explicitly in the threat
  model (SECURITY.md §Threat model) and explicitly unwelcome here.
- **Deanonymize flaggers.** Attempted aggregator collusion to unmask
  flag authorship is prohibited regardless of whether the crypto
  currently prevents it.
- **Impersonate.** Signing as someone you are not, laundering
  attestations, or selling/renting keypairs.
- **Automate personhood.** Running Phase-4 attestation challenges
  through a pipeline that bypasses a real human in the loop.
- **Surveil users.** Building clients, aggregators, or bank publishers
  whose purpose is to correlate identities, timing, or content against
  users' stated privacy expectations.
- **Enable illegal content.** CSAM, credible threats, incitement,
  doxxing, or material that exists only to harm a specific person —
  regardless of what a bank or aggregator set accepts.
- **Silence dissent by capture.** Running a "default" client,
  aggregator set, or bank publisher that hides parameters, refuses
  derivation logs, or falsifies `DeploymentCapabilityManifest` claims.
  This is what the protocol is designed to refuse; don't try to be the
  thing it refuses.
- **Military targeting, autonomous weapons, lethal force decisions.**
  Relay is not a targeting system. Do not embed it in one.

## Values the design encodes

- **No single verdict authority.** No aggregator, bank, or reviewer
  pool is protocol-canonical. If you disagree, fork; don't capture.
- **Inspection with consequence.** Power that cannot be inspected or
  refused is not legitimate power. Every authority surface is a signed,
  expiring, forkable manifest.
- **Local autonomy.** Reading, drafting, and local reasoning never
  require audit, reputation, or identity.
- **Honesty over marketing.** If a capability is not implemented, the
  `DeploymentCapabilityManifest` must say so. Aspirational security
  presented as real security is a bug.
- **Minority protection.** Voting alone is mob rule. Appeal, meta-
  review, fork semantics, and diversity sampling are load-bearing.

## If you see abuse

- Flag the artifact in-client. Flags feed reputation; reputation gates
  propagation, not read access.
- File an appeal if you were flagged unjustly
  (SECURITY.md §Governance and appeal).
- For out-of-band harm (real-world threats, criminal conduct),
  protocol is not the right layer. Contact the appropriate authorities
  in your jurisdiction.
- For protocol-level issues (hidden scoring, manifest fraud,
  capability-manifest lies, coordinated capture), file a
  `ClientRefusalRecord` and gossip it; repeated records feed
  `DeploymentPosture` and let other clients refuse the deployment.

## The project will not help you

We will not accept contributions, issues, or feature requests whose
purpose is any of the prohibited uses above. Clear dual-use work —
security research, red-team simulation, adversarial testing, abuse-case
expansion per SECURITY.md §Abuse case template — is welcome and in
fact needed. State the intent.

Demonstrations of attacks against Relay itself, disclosed responsibly,
are welcome. Demonstrations against real users are not.

## Final note

The protocol is explicitly **not a verdict system**, **not a content
policy**, **not a law**, and **not a moral authority**. It is a set of
mechanics designed so that shared understanding can scale without
collapsing into either a gatekept monopoly or an unaccountable mob.
Whether the resulting network earns legitimacy is a human question,
not a protocol one.

Use Relay to build understanding with people, not against them.
