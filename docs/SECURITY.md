# SECURITY

Relay ships knowledge across a shared vector space. Shared knowledge =
shared power. Shared power needs audit. This doc states the model.

## Status

Draft v4. Pre-implementation. Governance-security design, not a full
security specification. Readers should expect some claims to be
intentionally weakened from absolutist phrasing in favor of
measurable properties. Invariants are load-bearing; parameter
defaults are versioned and tunable across protocol versions with
published migration paths.

## Implementation status

**Nothing in this document is implemented yet.** The repo is mid-port
of `kern` (graph daemon, partial); `agnt` and `repl` binaries are not
scaffolded; federation, gossip, manifests, audit-loop, and reputation
do not exist in code. By Invariant 17 (Deployment honesty), every
invariant 1–20 is `unmet` for any current build, and Invariant 18
(Authority-before-gating) bars any deployment from enforcing
shared-write audit gating until the authority ledger
(`TrustSetManifest`, `ParameterManifest`, `DeploymentCapabilityManifest`,
replayable `AggregatorDerivationLog`) lands.

Treat this document as the design target the implementation must
converge toward — and as the specification any future
`DeploymentCapabilityManifest` will be measured against. Companion
documents referenced here (`docs/kern/security/{validity-and-refusal,
objects,state-machines,abuse-cases,parameters}.md`) do not yet
exist; they are required before implementation per §Meta.

## Thesis

AI usage scales past individual control. Centralized gatekeeping
concentrates power; unbounded federation concentrates noise and
abuse. Relay's answer: **human audit loop** at the edge, **flag-based
reputation** at the network, **no single verdict authority**, and
measured properties rather than absolutist guarantees.

Goal: broad-base legitimacy of shared understanding. Not perfect
democracy — voting alone is mob rule. Process, appeal, minority
protection, transparent mechanics, and explicit fork semantics are
load-bearing.

**Defaults are governance objects, not neutral configuration.** Every
default trust set, parameter set, bank set, aggregator set, and
reviewer pool must be a signed, expiring, forkable, inspectable
protocol object. "The official app" must not be allowed to become
the de-facto authority by virtue of being the default.

Weakest parts of this design are **not cryptographic**. They are
social bootstrapping, coercion semantics, identity scalability,
reviewer capture, and whether write influence is sufficient to
prevent graph poisoning. This document distinguishes four layers:

1. **Protocol-enforced** — cryptographically or structurally
   guaranteed.
2. **Client policy** — configurable per client / fork.
3. **Aggregator policy** — chosen by clients selecting aggregator
   sets.
4. **Social process** — human-layer norms that protocol supports but
   does not enforce.

Each §claim should be read in the layer it belongs to.

## Invariants

Protocol-enforced unless otherwise noted.

1. **Human-in-loop gate.** Shared-space writes require a valid
   audit-answer signature within scope. No bypass flag. (Protocol.)
2. **Replay-proof answer scope.** Every audit answer binds
   `{keypair, session-id, nonce, counter, minute, question-id,
   bank-id, trust-set-id, parameter-manifest-id}`. Replay outside
   scope is rejected at the gate. (Protocol.)
3. **Flags are signals, not verdicts.** Flags aggregate into
   reputation; reputation weights write propagation, merge, ranking,
   and sampling — never read access, never ban authority.
   (Protocol.)
4. **Reputation is bounded and decaying.** Asymptotic curve + decay
   + influence cap. No runaway accumulation. (Protocol.)
5. **Local-first non-dependence.** Local reads, local drafts, local
   memory, local agent loops, and local-only graph construction
   require no audit, no reputation, no identity, and no aggregator
   contact. Audit gate applies only at the shared-write boundary.
   (Protocol.)
6. **Keypair rotation is supported.** Reputation portability across
   rotation is explicit; emergency revocation exists. (Protocol.)
7. **No aggregator set is protocol-canonical.** No aggregator,
   aggregator set, or default aggregator manifest is
   protocol-canonical. Canonicality exists only relative to a
   client-selected `TrustSetManifest`. Aggregation is federated,
   reproducible, M-of-N; divergence surfaces to clients. (Protocol.)
8. **No question bank is protocol-canonical.** No question bank,
   bank publisher, or default bank set is protocol-canonical. Audit
   validity is scoped to the bank manifest and aggregator trust set
   that accepted it. Banks must meet the public-bank definition
   (§Public bank). (Protocol.)
9. **Phase 4 is an attestation, not a bit.** Human verification
   produces structured `Phase4Attestation` objects with provenance,
   decay, reviewer diversity, and expiration — not a binary
   "verified human" flag. (Protocol.)
10. **Anonymous flag eligibility (target).** Aggregators must not
    require plaintext flagger identity to compute flag eligibility,
    flag weight, or reputation deltas. Until anonymous eligibility
    proofs are implemented, deployments must mark this invariant
    as unmet per Invariant 17. (Protocol target; residual risk
    acknowledged.)
11. **Duress is local policy.** Duress response is chosen by the
    user at onboarding from explicit policy options. Protocol
    supports duress signaling; protocol does not mandate a universal
    duress behavior. (Client policy, protocol-supported.)
12. **Absolutist claims softened.** Measured properties are *cost of
    forgery*, *cost of influence capture*, *residual inference
    risk*, and *recovery cost*. Nothing is "impossible."
13. **Explicit trust manifests.** Every aggregator set, question-bank
    set, identity-provider set, appeal-review pool, and bank-publisher
    set used by a client is represented by a signed, expiring,
    forkable `TrustSetManifest`. No implicit default authority is
    protocol-valid. (Protocol.)
14. **Parameter transparency.** Every reputation, merge, ranking,
    propagation, decay, appeal, and sampling decision references a
    signed `ParameterManifest`. Hidden parameters are invalid.
    (Protocol.)
15. **Reproducible influence.** Any client-visible influence score,
    reputation delta, merge decision, or propagation weight must be
    reproducible from signed protocol objects, public manifests, and
    published derivation logs. (Protocol.)
16. **Bounded verification cost.** Aggregator derivation logs must
    support deterministic replay, bounded verification cost,
    checkpoint hashes, inclusion proofs, and (where feasible)
    exclusion proofs. Technically-reproducible-but-practically-
    unauditable logs are non-compliant. (Protocol.)
17. **Deployment honesty.** If a deployment does not implement a
    protocol target (e.g. anonymous flag eligibility), the unmet
    property must be machine-readable and client-visible. Aspirational
    security must never be presented as implemented security.
    (Protocol.)
18. **Authority-before-gating.** No deployment may enforce shared-write
    audit gating until `TrustSetManifest`, `ParameterManifest`,
    `DeploymentCapabilityManifest`, and minimally replayable
    `AggregatorDerivationLog` objects are implemented and
    client-visible. Otherwise the audit gate becomes an unaccountable
    authority surface — the first real power center in the network.
    (Protocol.)
19. **Consequence semantics.** Every "visible" or "inspectable"
    property has a corresponding automated client response: refuse,
    downgrade, quarantine, warn, or fork. Visibility without
    automated consequence is governance theater. (Protocol.)
20. **Validity layering.** A thing may be *protocol-valid* (signatures
    and hashes check), *deployment-valid* (honestly declared
    capability status meets profile), and *socially-legitimate*
    (community accepts the process). These are distinct. Protocol
    enforces the first two; the third is explicitly out of scope.
    (Protocol boundary.)

## Protocol objects (canonical signed objects)

Shapes defined here; wire format + canonical encoding live in
`docs/kern/security/objects.md` (TBD).

- `TrustSetManifest` — load-bearing, §TrustSetManifest
- `ParameterManifest` — load-bearing, §ParameterManifest
- `AuditAnswer`
- `SharedWriteProposal`
- `SharedWriteMerged`
- `Flag` (threshold-encrypted flagger identity)
- `ReputationDelta`
- `AggregatorDerivationLog`
- `BankHealthReport`
- `AppealRequest`
- `AppealDecision`
- `KeyRotationRecord`
- `RevocationRecord`
- `Phase4Attestation`
- `ClusterStateReport`
- `OfflineBatchDeclaration`
- `DuressPolicyManifest` (client-side, never leaves device by
  default)
- `RecoveryQuorumManifest` (private membership commitment)
- `DeploymentCapabilityManifest` (per Invariant 17)
- `DeploymentPosture` — derived state, green/yellow/red/black
- `TrustSetManifestDiff` — semantic diff for update consent
- `AdversarialBankProbeReport` — cross-bank differential bias testing
- `ClientRefusalRecord` — automated rejection evidence
- `SafetyPresentationPolicy` — client-local warning/collapse spec

Each object has: version, issuer key, timestamps, content hash,
signature. State machines for lifecycle are defined in §State
machines.

## TrustSetManifest

Every authority surface (aggregators, banks, identity providers,
appeal pools, bank publishers) is a signed manifest, not an
implicit default. Clients may *use* defaults; defaults are
themselves manifests.

```
TrustSetManifest = {
  version,
  purpose,                        # aggregator | bank | identity-provider
                                  # | appeal-pool | bank-publisher
  publisher_key,                  # who issues this manifest
  members[],                      # keys or manifest-ids
  selection_algorithm,            # how members are drawn at query time
  quorum_rule,                    # M-of-N spec
  expiration,
  update_policy,                  # how members change
  conflict_policy,                # how to handle disagreement
  human_readable_summary_hash,    # references auditor-readable summary
  fork_lineage,                   # prior manifest(s) this forks from
  signatures[],
}
```

**Invariant binding**: by Invariant 13, no aggregator,
bank, identity provider, or appeal pool may be used without
referencing a `TrustSetManifest`. "Default" clients ship with a
manifest; that manifest is the governance surface, not the client
binary.

Manifests expire. Expiration forces periodic re-consent, preventing
indefinite default capture.

## ParameterManifest

Every tunable parameter is a signed manifest. No hidden parameters.

```
ParameterManifest = {
  protocol_version,
  issuer_key,
  effective_from,
  expires_at,
  audit_cadence,                  # N, M, ceilings, new-keypair policy
  reputation_curve,               # k, β, α, o_baseline, R_max/R_min
  decay_rates,                    # split decay per category
  influence_cap,                  # K× median
  bounded_delta_window,           # ±ΔR / window
  appeal_quorums,                 # initial + meta
  phase4_cadence,                 # re-attestation schedule
  phase4_confidence_classes,      # enum definitions
  dp_budget,                      # ε per release, daily composition
  partition_policy,               # safety-vs-liveness spec
  vocabulary_version,             # flag category set
  migration_notes_hash,
  simulation_report_hashes[],     # adversarial simulation evidence
  signatures[],
}
```

**Invariant binding**: by Invariant 14, no reputation, merge,
appeal, or propagation decision is valid without referencing a
`ParameterManifest`. By Invariant 2, audit answers bind the
manifest-id; by Invariant 15, decisions derived from these
parameters must be reproducible.

## BankHealthReport

Bank capture is slow and quiet. Health reports make it auditable.

```
BankHealthReport = {
  bank_id,
  version,
  publisher_key,
  question_count,
  category_distribution,
  reviewer_distribution,
  acceptance_rate,
  rejection_rate,
  fork_count,
  challenge_count,
  appeal_count,
  entropy_score,                  # question diversity
  topic_coverage_score,
  adversarial_review_score,       # red-team review outcome
  signatures[],                   # issued by bank auditors, not bank itself
}
```

**Requirement**: aggregators must publish which `BankHealthReport`s
they considered when accepting bank IDs. Clients may refuse banks
with stale, missing, or adversarial-review-failing reports.

## Public bank definition

A question bank is **public** (and therefore usable for
protocol-valid writes) if:

- its canonical hash is published
- question objects are available to clients before use, except
  sealed/randomized challenge material
- admission rules are published
- version history is append-only
- removed questions remain hash-auditable (tombstoned, not erased)
- publisher keys are known and bound to a `TrustSetManifest`

**Sealed challenge material** (anti-gaming exception): sealed
questions may hide answer content before prompt time, but their
commitment hash, category, difficulty metadata, and admission proof
must be public before use. Private challenge banks cannot sneak
back in under anti-gaming arguments.

## DeploymentCapabilityManifest (Invariant 17)

Deployments must publish which protocol targets they implement and
which they have not yet implemented.

```
DeploymentCapabilityManifest = {
  deployment_name,
  issuer_key,
  protocol_version,
  invariant_status[],             # one entry per invariant, with:
                                  #   invariant_id
                                  #   status: met | unmet | partial
                                  #   residual_risk_summary_hash
                                  #   mitigation_hash_if_partial
  signatures[],
}
```

Clients display unmet invariants prominently. No "marketing page
says secure, protocol says not." The machine-readable manifest is
load-bearing.

## DeploymentPosture

Most users will not read 14 manifests. Derive a bounded posture
class from `DeploymentCapabilityManifest` + aggregator compliance +
manifest freshness.

```
DeploymentPosture = green | yellow | red | black
```

| Posture | Meaning | Automated client response |
|---------|---------|--------------------------|
| green   | all protocol-required invariants met | normal participation |
| yellow  | partial anonymity, partial exclusion proofs, stale but present health reports | participate with visible warning, elevated flag-weight sensitivity |
| red     | hidden parameters, expired trust set, missing derivation logs | refuse shared writes by default; reads permitted locally |
| black   | unverifiable authority surface; structural violation | refuse participation; warn user; offer fork path |

Posture is computed client-side from manifests, not declared by the
deployment itself. Deployments cannot grade themselves green.

## Client refusal rules

Three validity classes. Consequence is automated (Invariant 19).

| Class | Examples | Consequence |
|-------|----------|-------------|
| **fatal-invalid** | hidden parameters, expired trust set, missing signature, invalid derivation hash, unsigned bank acceptance | protocol-level refusal; object discarded; refusal logged as `ClientRefusalRecord` |
| **degraded-valid** | no anonymous flag eligibility, stale but present `BankHealthReport`, partial exclusion proofs, missing `AdversarialBankProbeReport` | participate with degraded flags; display warning; reduced trust weighting |
| **policy-valid** | client chose a controversial bank, appeal pool, or identity provider | participate normally; choice is the user's |

`ClientRefusalRecord` is gossipable; repeated refusals against a
deployment surface to other clients and feed `DeploymentPosture`.

## Validity layering

Three distinct concepts. Do not conflate.

- **Protocol-valid**: signatures, hashes, scope bindings, manifest
  references all check out. Mechanical.
- **Deployment-valid**: `DeploymentCapabilityManifest` honestly
  declares status and meets the required deployment profile for
  the action being taken.
- **Socially-legitimate**: users, forks, reviewers, and communities
  accept the authority process.

Protocol enforces the first two. The third is explicitly out of
scope (Invariant 20). A thing can be protocol-valid and socially
illegitimate; Relay does not adjudicate that. Forks do.

## Staged flagger anonymity

Invariant 10 is a target. There are four distinct privacy
properties, not one.

| Stage | Property | Status |
|-------|---------|--------|
| A | Public cannot see flagger identity; aggregator set can | Implementable with standard crypto |
| B | No individual aggregator can see identity; quorum can | Threshold encryption |
| C | Quorum can verify eligibility and weight without plaintext identity | Anonymous credentials / ZK eligibility proofs |
| D | Correlation-resistant timing / batching reduces metadata leakage | Mix networks, cover traffic |

Deployments declare stage per `DeploymentCapabilityManifest`.
Collapsing all four into "anonymous flag eligibility" lets
deployments overclaim. Separate them.

## AdversarialBankProbeReport

`BankHealthReport` measures visible symptoms. Slow ideological or
epistemic bias passes entropy, topic coverage, and acceptance-rate
checks.

Complement with cross-bank differential probing.

```
AdversarialBankProbeReport = {
  bank_id,
  compared_bank_ids[],
  probe_set_hash,
  disagreement_matrix_hash,
  bias_axis_claims[],             # e.g. political, cultural, epistemic
  reviewer_panel_manifest_id,
  result_summary_hash,
  signatures[],                   # red-team panel, not bank itself
}
```

The metric is not "is this bank healthy in isolation" but "how does
this bank systematically differ from peer banks under adversarial
probes." Missing or stale probe reports drop bank acceptance to
degraded-valid.

## Aggregator verification modes (Invariant 16 concrete)

Minimum required verification modes. "Bounded verification cost"
must be hand-waved no longer.

1. **Light client mode** — verifies checkpoint signatures, manifest
   IDs, inclusion proofs. Low cost; runs on any device.
2. **Audit client mode** — replays sampled derivations within
   bounded cost. Catches targeted manipulation.
3. **Full auditor mode** — replays entire derivation windows from
   signed inputs. High cost; for dedicated auditors.
4. **Challenge mode** — submits discrepancy proof against
   checkpoint. Forces aggregator to answer or fork.

Aggregators must support all four. Clients declare which they use.
Missing any mode = red posture.

## TrustSetManifestDiff

Expiration forces re-consent. Re-consent without semantic diff is
click-through theater.

```
TrustSetManifestDiff = {
  old_manifest_id,
  new_manifest_id,
  added_members[],
  removed_members[],
  quorum_rule_change,
  selection_algorithm_change,
  conflict_policy_change,
  risk_delta_summary_hash,        # references human-readable analysis
  signatures[],
}
```

Clients present the diff, not the full new manifest. Risk delta is
signed by an independent diff-auditor role (not the manifest
publisher). Deployments without diff auditors = degraded-valid.

## Safety presentation (read-access edge case)

Invariant 3 says reputation never affects read access. Clients
still need safety interstitials for harmful artifacts. Distinguish:

- **Read availability** — protocol guarantees the artifact is
  addressable (Invariant 3).
- **Default visibility** — client may collapse, warn, or opt-in
  gate, provided the artifact remains addressable.
- **Ranking** — client may downrank.
- **Warning presentation** — client may interstitial.
- **Local hiding** — user may hide; protocol does not.

Rule (codified):

> Reputation never removes protocol-level read availability, but
> clients may apply local presentation policy — including warnings,
> collapses, and opt-in display — provided the underlying artifact
> remains addressable and the presentation policy itself is
> inspectable.

Client presentation policy is declared as a `SafetyPresentationPolicy`
object; users can see what their client is hiding and why.

## RecoveryQuorumManifest

Duress-policy option 4 (recovery quorum) requires privately
committed membership.

```
RecoveryQuorumManifest = {
  subject_key,
  quorum_members_commitment,      # hash/commitment, not plaintext
  threshold,                      # M-of-N
  policy_scope,                   # which duress policy triggers quorum
  contact_rotation_policy,
  expiration,
  emergency_replace_policy,
  signatures[],
}
```

**Privacy requirement**: recovery quorum membership is private by
default. Public membership creates targeting risk. Aggregators see
the manifest hash; members are revealed only during quorum
invocation.

## Cluster state machine (Phase 4 capture boundary)

Phase 4 regional capture is a real threat. Turn "acknowledged risk"
into an operational state machine.

```
ClusterState = healthy | watch | degraded | quarantined | forked
```

### Transition triggers

```
healthy → watch:
  - abnormal internal vouch density
  - repeated reviewer conflicts
  - low external ambassador diversity
  - appeal reversal rate above threshold

watch → degraded:
  - sustained watch triggers over 2 reporting windows
  - failed ambassador rotation
  - evidence of coordinated false attestations

degraded → quarantined:
  - meta-review confirms capture or severe process fault

any → forked:
  - cluster split invoked; history preserved, trust computation
    diverges
```

### Effects

```
watch        → increased external review sampling
degraded     → reduced outgoing attestation weight
quarantined  → no new high-influence attestations accepted until
               reconciliation via cross-cluster meta-review
forked       → history remains visible; trust computation diverges;
               separate TrustSetManifest lineage
```

Cluster state transitions are published as `ClusterStateReport`
objects and must be signed by cross-cluster reviewers, not the
cluster itself.

## Audit loop

```
every N shared writes OR M minutes OR topic-cluster entry:
  prompt user with audit question drawn from client-selected bank
  user picks graded answer
  client signs AuditAnswer with full replay-proof scope
  signature gossiped to client-selected aggregator set
  gate releases queued shared writes bound to this signature
```

Skipping = queued writes stay queued. Session continues locally.
Shared participation resumes on next answered prompt.

### Question banks (federated)

Banks are published by bank-publisher keypairs; hashes are
reproducible. Clients select M-of-N bank publishers. Writes must
reference a bank-id the aggregator set accepts; accepted bank-ids
are themselves a client/aggregator choice, not a protocol monopoly.

Peer-authored questions enter a bank only via that bank's own
admission process — which the bank publisher owns. Users who distrust
a bank fork it.

Categories:
- **Attestation** — state-of-self.
- **Intent** — session goal in own words; hashed for drift, never
  content-analyzed centrally.
- **Consistency probe** — prior signed claim shown. Answers:
  *still true* / *changed my mind (why)* / *was wrong* / *don't
  remember*. Changed-my-mind is first-class.
- **Peer sample** — anonymized peer write, flag/no-flag with
  category.
- **Duress canary** — per §Duress.

### Graded answers (4 options)

- **bad-a**, **bad-b** — two distinct wrong framings (defeats
  "pick the non-bad" shortcut).
- **okayish** — honest partial signal.
- **good** — high-confidence signal.

Okayish is honest, never punished morally, but carries **calibration
cost** (§Reputation curve). Pure-okayish-always decays toward near-zero
influence.

## Drift detection

Client-side computation against local history. Only bounded,
differentially private scores leave the device.

Dimensions (6): latency shift, entropy shift, explicit-contradiction
count (excluding changed-my-mind), write-burst score, topic-jump
score, edge-density score.

Drift raises audit frequency and peer-review sample weight. Never
auto-bans.

### Drift privacy (adjacency + residual risk)

**Adjacency definition**: two histories are adjacent if they differ
in exactly one audited session (one session, one set of writes and
one set of answers). DP guarantees protect against inference about
the presence or absence of a single session's contribution.

**ε budget**: 0.2 per release, 1.0 per day, composed via advanced
composition. 5 releases / day, pseudo-random jittered intervals.

**Residual risk acknowledged**: DP on drift alone does not prevent
behavioral inference when combined with write timestamps, topic
transitions, flag timing, and offline-period metadata. The
protocol minimizes telemetry and documents the residual risk rather
than claiming strong privacy. Clients intolerant of this risk may
operate in local-only mode indefinitely.

## Flag-based reputation

- Any authenticated user may flag any shared-space artifact.
- Flag fields: target, category (protocol-fixed vocabulary),
  severity, optional justification, threshold-encrypted flagger
  identity.
- **Vocabulary is protocol-fixed**: *malicious*, *low-quality*,
  *distorted*, *off-topic*, *unsafe-harmful*, *unsafe-illegal*,
  *unsafe-manipulative*, *spam*. "Unsafe" is split into three
  narrow sub-categories to prevent it becoming an everything-flag.
  Clients may display subsets.
- **Boundary-gaming mitigation**: attackers who learn which
  category has lower penalty variance coordinate around it.
  Mitigations in `ParameterManifest`:
  - published cross-category severity normalization
  - category-confusion matrix updated per window
  - reviewer calibration audits
  - flag-category appeal correction (misfiled category is itself
    appealable and correctable)
- **Flagger anonymity target** (Invariant 10): aggregators should
  verify flag eligibility and reputation weight without learning
  identity. Primitives: threshold-encrypted flagger identity, blind
  signatures for eligibility proof, anonymous credentials for
  reputation-tier proof. Full zero-knowledge protocol definition
  is implementation work, not in this doc. Residual risk: until the
  crypto is deployed, flagger identity is known to the aggregator
  set collectively and some deanonymization is possible by collusion.
- Reputation score inputs:
  - flags received, weighted by flagger reputation × **diversity
    bonus**
  - flags given that others later concurred with
  - audit compliance rate
  - peer-review calibration on sampled artifacts
  - okayish calibration credit

## Reputation curve (asymptotic, asymmetric, decaying)

Three combined properties make perfect score unreachable and
high scores non-permanent.

### 1. Concave saturation

Raw score `S` accumulates from signed contributions. Effective
reputation `R` is:

```
R = sign(S) · (1 − exp(−|S| / k))
```

- `R ∈ (−1, 1)`, never reaching endpoints.
- `k = 50`: ≈63% of asymptote at |S|=50; ≈95% at |S|=150.

Each additional good action gives diminishing return. No one is
"perfect."

### 2. Asymmetric weighting

```
good action: ΔS = +g · quality
bad action:  ΔS = −β · g · severity
```

`β = 3`. Trust slow to build, fast to lose. `severity` is calibrated
relative to category baselines per protocol version, preventing
double-counting with `β`.

### 3. Decay (split)

Decay is split to avoid suppressing intermittent high-quality
contributors:

- **Identity-confidence** (Phase 4 freshness): λ tied to Phase 4
  expiration, not continuous decay.
- **Calibration** (audit+peer accuracy): λ = 0.02/day, ~35-day
  half-life. Current-style calibration must be maintained.
- **Topic-specific reputation**: decays by topic-specific activity,
  not wall clock. Intermittent topic experts preserved.
- **Review authority**: decays fastest (λ = 0.05/day, ~14-day
  half-life). Reviewer activity must be continuous.

### 4. Display vs. weight

Protocol uses raw `R`. No leaderboards at protocol level. Clients
may display; clients may fork if they want to gamify, but that fork
is not the canonical protocol.

### 5. Okayish calibration cost

Let `o` = rolling okayish rate over last 50 answered prompts,
`o_baseline` = 0.30.

```
influence_multiplier = 1 − α · max(0, o − o_baseline)²
```

`α = 2.0`. Peer-consensus matches on peer-sample okayish answers
credit `o` back by 1/50 per match.

Combined effect: asymptotic ceiling (can't perfect), asymmetry
(bad hurts triple), split decay (no permanent laurels, but not
suppressive), calibration cost (no free middle).

## Anti-oligarchy mechanisms

- **Diversity weighting**: flags from socially/graph-distant
  flaggers count more than clustered flaggers.
- **Influence cap**: no keypair exceeds `K×` median influence. Hard
  ceiling independent of reputation.
- **New-user floor**: fresh Phase-4-verified keypairs carry non-zero
  weight from day one.
- **Bounded delta**: reputation updates capped per window; sudden
  cratering floored.
- **Asymptotic curve + split decay**: per §Reputation curve.
- **No leaderboards in protocol**: per §Display vs. weight.

## Substrate split: identity vs human governance

Phase 4 is structurally too powerful to sit inside Identity. It
controls reviewer pools, appeals, cluster reports, ambassadors, and
high-influence trust. Split:

- **Identity substrate** (this section): keypairs, rotation,
  revocation, M-of-N providers, biometric local attestations.
- **Human governance substrate** (§Phase 4 attestations and beyond):
  Phase 4, clusters, reviewers, appeals, ambassadors.

Protocol should not let identity mechanisms quietly become
governance mechanisms by default. Separate trust sets cover each
(both scoped by `TrustSetManifest`).

## Identity

**Goal**: bounded Sybil cost and bounded influence per human
identity cluster. **Not**: enforced one-human-one-keypair, which is
not technically achievable without heavy infrastructure.

Measured properties:
- minimum cost per durable identity
- maximum influence per correlated identity cluster
- decay on unmaintained identities
- correlation-resistant but auditable uniqueness claims
- explicit regional / provider failure modes

Layered mechanisms:

1. **Web of trust** — existing users vouch; staked with published
   slashing curve.
2. **Biometric fingerprint** — device-held, never network-transmitted,
   attestation only.
3. **Global ID providers** — external PoP. **M-of-N required**
   across multiple providers. Client-configurable; protocol
   publishes a recommended baseline but does not mandate a single
   provider. Explicit regional failure: in jurisdictions that
   permit only one provider, users operate at reduced influence
   ceiling.
4. **Phase 4 attestations** — per §Phase 4.

No phase is sufficient alone. Stacking raises forgery cost. The
measured security property is *cost*, not *impossibility*.

## Phase 4 attestations

Phase 4 does **not** produce a binary verified-human bit. It
produces `Phase4Attestation` objects:

```
Phase4Attestation = {
  subject_key,
  reviewer_keys[],
  challenge_type,            # video-novel-reasoning | gathering | audio | ...
  locality,                  # cluster-id + region
  timestamp,
  confidence_class,          # bounded enum, e.g. A/B/C
  expiration,
  transcript_hash_or_null,   # private by default
  appeal_pointer,
  signatures[]
}
```

Clients and aggregators compute derived trust from attestation sets
using published algorithms. Different clients may weight attestations
differently; protocol sets no single blessed computation.

### Challenge types

- **Video novel-reasoning**: live human-to-human video with
  contextual reasoning question unknown to both parties until the
  moment. Deepfake-hard in proportion to novelty of reasoning
  required.
- **Gathering attendance**: in-person regional gathering with
  multiple cross-checking attendees.
- **Audio with regional/temporal context**: live audio challenge
  referencing current-region current-time context.

### Cadence

Reputation-scaled: median users re-attest annually; highest-influence
tier re-attests quarterly. Attestations carry expiration; expired
attestations decay out of derived trust, not retroactively nullify
prior shared writes.

### Gathering logistics

- Clusters cap at ~150 (Dunbar). Automatic split on sustained
  growth.
- Cross-cluster **rotating ambassadors**: max 2 consecutive terms,
  mandatory cooldown. No permanent ambassador position.
- Remote-only users: satellite verification via live video bridge
  with 3 independent in-person attendee signatures. Soft influence
  cap (≤ median) until first in-person attendance.
- Bootstrap: per §Genesis.

**Honest statement of the Phase-4 risk**: even with rotation and
diversity requirements, in-person verification produces a
high-influence class. Regional capture is a real threat. Protocol
mitigates with rotation, diversity weighting, influence cap, and
fork semantics — but cannot eliminate it.

## Rotation and revocation

- **Standard rotation**: user signs `KeyRotationRecord` old→new,
  witnessed by M Phase-4-verified peers. Cooldown window (default
  72h) during which new key's flag weight is elevated; catches
  stolen-then-transitioned keys.
- **Emergency revocation** (key believed compromised, user cannot
  sign): requires M-of-N Phase-4 peer attestation. Slow by design.
- **Rotation portability**: reputation transfers across rotation.
  Cooldown windowing and elevated flag weight mitigate rotation-as-
  escape-from-reputation.

## Duress (local policy module)

Duress is **not a universal protocol behavior**. Protocol supports
a duress signal; the response is chosen by the user at onboarding
from explicit policy options. Aggregators do **not** by default see
"duress observed" in real time.

### User policy options

At onboarding, user picks one or more:

1. **Notify contacts only** — safety contacts notified; no
   protocol-level change to writes or visibility.
2. **Freeze shared writes invisibly** — shared writes queued
   locally but not emitted; user sees normal UI.
3. **Allow writes but mark for delayed merge** — writes emitted but
   flagged locally (encrypted marker), held for post-duress review
   before merge. Marker is decryptable only by the user's recovery
   quorum.
4. **Route through trusted recovery quorum** — writes require
   additional signature from recovery quorum before merge.

Users may also choose **no duress policy** (accept risk, simpler
model).

### Duress signaling

- **Rotating canary bank**: 20 phrases pre-selected at onboarding,
  salted-hashed. Auto-advance on each trigger.
- **Manual bank rotation**: requires live video with pre-designated
  safety contact answering pre-arranged wellness question. Raises
  coercion cost; not unforgeable. Contact changes require 2-of-3
  existing contacts or gathering ceremony.

### Duress privacy principle

> Duress signals must not create new centralized visibility into
> vulnerable users.

Specifically:
- Watermarks on duress-tagged writes are **locally encrypted** until
  post-duress recovery invocation.
- Aggregators receive duress-observed status only if user's policy
  explicitly authorizes. Default: no.
- Safety contacts may themselves be compromised or abusive. Policy
  option 4 (recovery quorum) exists for users who do not trust a
  single contact.

## Gate principle (softened)

Shared-space writes require a replay-proof signed audit answer
within scope. Machine-only pipelines cannot produce protocol-valid
shared writes unless they control a valid human-bound signing
context. Malware, remote control, and key/session theft **can**
operate through a valid context; the gate does not prevent that.
Drift detection, peer flagging, and rotation are the mitigations
for compromised contexts, not the gate itself.

## Write lifecycle (formal)

Reputation applies at specific stages only. Explicit mapping:

```
1. local draft                      (no reputation, no gate)
2. signed shared-write proposal     (gate: audit signature required)
3. pending pool                     (awaits aggregator acceptance)
4. reputation-weighted merge        (reputation → merge confidence,
                                     edge weight, propagation prob)
5. accepted graph edge              (stored in shared graph)
6. propagated index                 (reputation → default ranking
                                     weight, visibility scoring)
7. client-visible result            (clients may re-rank per policy)
8. disputed / flagged               (reputation → flag weight)
9. archived / decayed               (time + reputation → persistence)
```

**Where reputation does NOT apply**: local reads, local drafts, any
stage before (2).

**Invariant**: reputation never determines whether a person may
speak locally, but it determines default propagation, merge
confidence, ranking weight, and persistence in shared views.

**Poisoning mitigation**: low-rep writes that still enter the graph
carry low edge weight and low propagation probability. Downstream
clients that ingest everything (rather than respecting aggregator
weights) are making a client-policy choice and accept the
consequences. Protocol does not prevent naive clients from
self-poisoning.

## Aggregator trust

- **M-of-N federated**, scoped to a client-selected
  `TrustSetManifest`. No protocol-canonical aggregator.
- **Reproducible derivation logs** per Invariant 16. Required
  properties: deterministic replay, bounded verification cost,
  checkpoint hashes, inclusion proofs, exclusion proofs where
  feasible, `ParameterManifest` reference, `TrustSetManifest`
  reference, appeal/fork lineage reference.
- **Divergence surfaced** as first-class client event.
- **Flagger identity**: threshold/blind crypto target (Invariant 10);
  deployment status reported via `DeploymentCapabilityManifest`
  (Invariant 17). Until deployed, aggregator-set collusion is a
  residual risk and must be client-visible.
- **Drift scores**: DP-noised client-side; aggregators see bounded
  scores, not raw timelines.

### Hidden-scoring prohibition

By Invariant 15, any client-visible reputation or merge decision
must be derivable from published signed inputs, parameter
manifests, trust-set manifests, and aggregator derivation logs.
Hidden scoring is not protocol-valid. Aggregators that refuse to
publish the required inputs are non-compliant and clients must
flag them via `DeploymentCapabilityManifest` comparison.

### Partition behavior (open)

Network partitions (half the aggregators unreachable) require
defined client behavior:
- queue writes against last-known aggregator state
- prefer safety (delay merge) over liveness
- resume with explicit reconciliation

Exact algorithm TBD in reference implementation.

## Governance and appeal

### Appeal structure

- **Initial appeal**: 7 reviewers, 4-of-7 quorum. Diversity-sampled.
- **Meta-review**: 11 reviewers, 7-of-11 quorum. Invoked if
  cryptographic or process fault alleged against initial appeal.
- **Finality**: after meta-review, decision is final unless new
  evidence with hash predating the original decision surfaces.
- **Cooldown** before re-opening: default 90 days.
- **Reviewer slashing**: ordinary reversal slashes initial
  reviewers' reputation mildly; meta-review upholding fault
  finding slashes them severely.

Recursion terminates at meta-review + new-evidence exception.

### Reviewer sampling algorithm

7 reviewers drawn from Phase-4-verified pool. Stratified:

1. **Graph distance from target**: 2 at ≥3 hops, 2 at ≥5 hops, 3
   at maximum available distance.
2. **Reputation tier**: 2 high, 3 mid, 2 low-but-verified.
3. **Topic cluster**: 3 in-cluster, 4 out-of-cluster.
4. **Activity cap**: no reviewer with >5 reviews in trailing 30
   days.
5. **Geographic diversity**: ≤2 from same regional cluster.

Conflicts resolved by relaxing strata in published priority order
(activity cap > tier > cluster > geography > distance). All
relaxations logged.

### Governance principle

No authority above peer review within a protocol version.
Protocol-version changes are their own governance surface (fork or
accept).

## Genesis

The launch cohort is a distinct threat surface. Bootstrap requires:

1. **Public founding charter** — hash-published, immutable.
2. **Named launch cohort** — public keys, real-name or pseudonymous
   with attestations, regional and community diversity required.
3. **External audit** of initial client, aggregator, and bank code.
4. **Temporary influence cap** — all genesis users capped at
   median influence regardless of contributions.
5. **Mandatory sunset** — genesis privileges expire on schedule
   (default: 180 days from first N non-genesis user onboarding).
6. **Non-genesis cohort selection** — first non-genesis users
   selected across regions and communities; not hand-picked by
   genesis cohort.
7. **90-day genesis review** — public report on genesis cohort
   behavior; appeal against capture allowed.
8. **Fork procedure** — documented before launch, usable from day
   one.

Principle:

> Genesis trust must decay faster than earned network trust.

Otherwise the launch cohort becomes a permanent priesthood. Decay
schedule is protocol-parameter and public.

## Trust boundaries

Explicit separation:

| Component | Trust level | Failure mode |
|-----------|-------------|-------------|
| Client trusted code | Fully trusted (user's own device) | Malware → compromised context |
| Local device secure storage | Trusted | Theft, OS compromise |
| Aggregator set | M-of-N trusted | Collusion → metadata leak |
| Bank publishers | Client-selected, M-of-N | Biased questions → forks |
| Phase 4 reviewers | Diverse pool, slashable | Local capture → cluster fork |
| Safety contacts | User-chosen | Compromised contact → policy 4 |
| Identity providers | M-of-N external | Single-provider mandate → regional fork |
| Shared-graph display clients | Client policy | Naive ingest → self-poisoning |

## Fork semantics

"No single authority" implies forks are normal and supported.

- **Aggregator fork**: client switches to a different M-of-N set.
  Reputation history is visible across aggregator sets unless an
  aggregator refuses to publish; protocol requires publication for
  reproducibility.
- **Bank fork**: client switches question-bank publishers. Past
  audit answers remain valid against the banks they referenced.
- **Protocol-version fork**: migration path documented per version
  increment.
- **Appeals across forks**: appeal decisions are scoped to the
  aggregator set that issued them. Cross-fork appeals are not
  valid; each fork owns its own governance.
- **Reputation history migration**: rotation-style transition
  supported across aggregator forks with cooldown.

## Threat model

| Threat | Mitigation |
|--------|-----------|
| User lies on attestation | Drift, peer flag, calibration cost |
| Coerced user | Duress canary, user-chosen policy, safety contacts |
| Cognitive distortion | Drift, peer flag, appeal path |
| Sybil flood | Layered identity, M-of-N providers, staked vouching, influence cap |
| Flag brigading | Diversity weighting, influence cap, new-user floor |
| Reputation laundering | Diversity weighting, bounded delta, meta-review slashing |
| Graph poisoning | Gate + reputation-weighted merge + low-rep edge weight |
| Naive client self-poisoning | Client-policy responsibility; protocol publishes weights |
| Doxxing flagger | Threshold/blind crypto target; residual risk stated |
| Aggregator collusion | M-of-N, reproducible logs, divergence surfaced, DP drift |
| Aggregator surveillance | Client-side drift + DP; residual inference risk stated |
| Audit fatigue | Rotating bank, okayish allowed, calibration cost on overuse |
| Automated personhood pipeline | Phase 4 attestations with novel reasoning + gatherings |
| Real-time deepfake | Novel-reasoning challenges + gatherings + behavioral continuity |
| Binary honest/dishonest forcing lies | Graded answers, changed-my-mind first-class |
| Okayish-always strategy | Calibration cost curve |
| Score explosion / gamification | Asymptotic curve, no protocol leaderboards |
| Trust wrong-direction speed | Asymmetric weighting β=3 |
| Permanent laurels | Split decay |
| Keypair compromise | Rotation with witnessed transition + cooldown |
| Keypair rental | Drift + Phase 4 re-attestation cadence |
| Replay of audit answer | Replay-proof scope binding (Invariant 2) |
| Offline burst false positive | Pre-declared offline batch declarations |
| State-level single-provider capture | M-of-N providers; regional reduced-ceiling fallback |
| Vocabulary fragmentation | Protocol-fixed flag vocabulary |
| Early-user oligarchy | Diversity, cap, decay, floor, curve |
| Appeal absent / centralized | Peer review with meta-review termination |
| Appeal infinite recursion | Finality after meta-review + new-evidence exception |
| Question-bank capture | Federated banks, client-selected, forkable (Invariant 8) |
| Phase-4 local capture | Rotation, diversity, cap, fork semantics; residual risk stated |
| Flagger deanonymization by aggregator collusion | Threshold/blind crypto; residual risk until deployed |
| Duress universal policy leaking victim | User-chosen local policy; no default centralized visibility |
| Safety contact compromise | Recovery quorum policy option |
| Genesis cohort priesthood | Mandatory sunset, external audit, faster-than-earned decay |
| Fork of aggregators leaving users stranded | Documented reputation migration across forks |
| Network partition | Safety > liveness policy; explicit reconciliation (TBD) |
| Compromised signing context (malware, RAT) | Drift + peer flag + rotation; gate does not prevent |
| Default-client becomes de-facto authority | TrustSetManifest required for all defaults; expiration forces re-consent |
| Hidden aggregator scoring | Invariant 15 + derivation-log requirements (Invariant 16) |
| Technically-reproducible-but-unauditable logs | Invariant 16 bounded verification cost |
| Aspirational security presented as implemented | Invariant 17 + DeploymentCapabilityManifest |
| Slow bank bias before fork | BankHealthReport with adversarial-review score |
| Regional Phase-4 capture | Cluster state machine with cross-cluster signing |
| Recovery quorum coercion | Private membership commitment, threshold >1, delayed merge review |
| Public recovery-quorum targeting | RecoveryQuorumManifest commitment hides membership |
| Parameter-manifest substitution | Audit answers bind parameter-manifest-id (Invariant 2) |
| Private challenge bank reintroduction | Public-bank definition requires commitment hash + admission proof even for sealed questions |
| Audit gate becomes unaccountable authority | Invariant 18 authority-before-gating; MVP profile required |
| Visibility without consequence (theater) | Invariant 19 consequence semantics; DeploymentPosture + ClientRefusalRecord |
| Click-through manifest updates | TrustSetManifestDiff with independent diff auditor |
| User ignores 14 manifests | DeploymentPosture derived class; automated refusal |
| Deployment grades itself green | Posture computed client-side from manifests, not declared |
| Slow epistemic bank bias passing symptom checks | AdversarialBankProbeReport cross-bank differential |
| Flag anonymity overclaim | Staged A/B/C/D declaration in DeploymentCapabilityManifest |
| Unsafe-flag-everything collapse | Vocabulary split: unsafe-harmful / unsafe-illegal / unsafe-manipulative |
| Category-boundary gaming | Cross-category severity normalization in ParameterManifest |
| Read-vs-presentation conflation | SafetyPresentationPolicy; protocol availability vs client display |
| Identity substrate creeping into governance | Substrate split with separate TrustSetManifests |
| Recovery quorum non-invocation (manipulation) | Dead-man trigger, silent availability check, delayed self-recovery |

## Abuse case template

Each threat should be expanded in `docs/kern/security/abuse-cases.md`
with:

- attacker goal
- attacker capability
- expected cost (in identity setup, time, reputation stake)
- detection signal
- recovery path
- residual risk

Top-priority abuse cases for pre-implementation analysis:
- question-bank capture (slow biasing before fork threshold)
- Phase-4 regional capture (cluster state machine stress test)
- flagger deanonymization via aggregator collusion
- graph poisoning through naive-client ingest
- coerced user with policy-3 watermarked writes
- recovery-quorum coercion (attacker coerces user + contacts up
  to threshold)
- default-client captures by silent TrustSetManifest update
- parameter-manifest substitution attacks
- hidden-scoring aggregator that appears compliant

### Recovery quorum coercion (worked example)

```
attacker_goal:      compromise recovery quorum, defeat duress policy 4
attacker_capability: coerces user + N-1 quorum contacts
expected_cost:       proportional to threshold M and contact diversity
detection_signal:    drift on user + drift on contacts + abnormal
                     quorum invocation pattern
recovery_path:       independent quorum path; delayed merge review
                     surfaces coerced writes post-crisis
residual_risk:       coercion of quorum majority defeats policy;
                     protocol cannot exceed the weakest-link contact
mitigation:          threshold > 1, private membership commitment,
                     geographic/social contact diversity, rotation
```

### Recovery quorum non-invocation (worked example)

Common abuse dynamic: attacker does not coerce quorum, just
convinces user that invoking recovery would endanger quorum
members. User never invokes.

```
attacker_goal:      prevent recovery without compromising quorum
attacker_capability: manipulates user's threat perception
expected_cost:       social; no technical attack
detection_signal:    prolonged absence of recovery invocation
                     combined with sustained drift
recovery_path:       dead-man local trigger (pre-configured); delayed
                     self-recovery window; silent quorum availability
                     check that does not reveal duress to aggregators
residual_risk:       mostly non-technical; protocol supports but
                     cannot guarantee victim safety
mitigation:          pre-configured dead-man, post-crisis private
                     evidence bundle, opt-in silent availability check
```

Protocol-level honest statement:

> Recovery quorum protects merge integrity better than it protects
> real-time victim safety. Real-time safety is a social and legal
> problem, not primarily a protocol problem.

## State machines

Define lifecycle state machines for:
- session
- shared write (per §Write lifecycle)
- flag
- appeal
- identity
- key rotation
- duress
- offline batch

Specifics live in `docs/kern/security/state-machines.md` (TBD).

## Parameters (defaults, version-bound)

Derivations and simulation results live in
`docs/kern/security/parameters.md` (TBD).

### Audit cadence

- Base: every **10 shared writes** OR **30 minutes active** OR
  **new topic cluster**, whichever first.
- High-reputation: interval ×1.5 (max 15 writes / 45 min).
- Drift-elevated: interval ×0.5 (floor 3 writes / 10 min).
- Hard ceiling: **50 shared writes** or **2 hours**.
- New-keypair policy: first 100 writes use **tiered sampling**, not
  universal audit:
  - every write touching new topic cluster: audit
  - every write above propagation threshold: audit
  - every write after drift spike: audit
  - low-risk replies: sampled at 1-in-5
- Time floor caveat: write-count trigger takes priority when active.

### Reputation curve

- `k = 50`, `β = 3`, `o_baseline = 0.30`, `α = 2.0`.
- Split decay: identity (tied to Phase 4 expiration), calibration
  (λ=0.02/day, ~35d), topic-specific (by topic activity), review
  authority (λ=0.05/day, ~14d).
- Influence cap: `K = 5` × median influence.
- Bounded delta per window: ±0.1 R per 24h.

### Phase 4

- Cluster size cap ~150.
- Median user re-attestation: 12 months. Top tier: 3 months.
- Gathering cadence: min 2/year per cluster.
- Ambassador terms: max 2 consecutive, mandatory cooldown.
- Bootstrap cluster threshold: 20 Phase-4 users before full
  participation; below that, Phase-3 ceiling.

### Appeal

- Initial: 7 reviewers, 4-of-7.
- Meta: 11 reviewers, 7-of-11.
- Cooldown: 90 days.

### DP

- ε = 0.2 per release, 1.0 per day, 5 jittered releases.
- 6 drift dimensions, Laplace per-dim, client-side clipping.
- Adjacency: one audited session.

### Duress canary

- 20 phrases pre-selected at onboarding.
- Auto-advance on trigger.
- Manual rotation: live safety-contact wellness question.

## Non-goals

- Not a verdict system.
- Not a content policy (vocabulary is fixed; judgments are not).
- Not a KYC system; personhood ≠ state identity.
- Not a replacement for law.
- Not perfect. Measured properties, not absolutes.
- Not a complete security specification. Governance-security draft
  for pre-implementation review.

## Open questions (genuinely unresolved)

- **Threshold/blind crypto for flagger anonymity**: protocol choice
  + implementation cost + latency budget.
- **Partition behavior**: exact reconciliation algorithm.
- **Economic layer stance**: if a third-party token/payment layer
  emerges, how does protocol respond? Publish stance before
  emergence.
- **Jurisdictional fragmentation acceptance**: is reduced-ceiling
  fallback sufficient, or does protocol need stronger regional
  guarantees?
- **Abuse-case coverage**: §Abuse case template requires full
  expansion before implementation freeze.
- **Wire-format specifications**: protocol objects need canonical
  encoding before interop.

## Implementation order

Do **not** implement the audit loop first (Invariant 18). Build the
authority ledger first. Honesty layer before capability layer.

1. Canonical object encoding (wire format, deterministic hashing).
2. Signature, hash, expiration, fork-lineage library.
3. `DeploymentCapabilityManifest` — honesty layer first, so every
   later step declares its own status.
4. `ParameterManifest`.
5. `TrustSetManifest`.
6. `TrustSetManifestDiff` + manifest diffing + client refusal
   rules + `DeploymentPosture` derivation.
7. `AggregatorDerivationLog` — minimal replay, checkpoint hashes,
   inclusion proofs (Invariant 16 modes 1–2).
8. `SharedWriteProposal` / `SharedWriteMerged` lifecycle.
9. Basic reputation simulation (curve + decay + cap).
10. Bank manifests + public-bank hashing + `BankHealthReport` +
    `AdversarialBankProbeReport`.
11. `AuditAnswer` with full replay-proof scope binding (Invariant 2).
    **Shared-write gating enabled only from this step** (Invariant
    18 satisfied once steps 3–7 are deployed and client-visible).
12. `Flag` object without anonymity guarantees — explicitly
    Stage-A per `DeploymentCapabilityManifest`.
13. Appeal state machine + reviewer sampling.
14. `Phase4Attestation` + `ClusterStateReport` (human governance
    substrate).
15. Key rotation + revocation.
16. Duress module + `RecoveryQuorumManifest` + dead-man triggers.
17. DP drift reporting with adjacency-bound budget.
18. `AggregatorDerivationLog` modes 3–4 (full auditor + challenge).
19. Anonymous flag eligibility — advance through Stages B, C, D;
    update `DeploymentCapabilityManifest` as each lands.
20. Partition reconciliation.

Each step requires:
- abuse-case expansion per §Abuse case template
- state-machine spec per §State machines
- simulation report referenced from `ParameterManifest`
- `DeploymentCapabilityManifest` update declaring new status

## Minimum viable protocol validity profile (MVP)

For a deployment to accept shared writes at all.

**MVP shared-write deployment requires**:
- canonical encoding
- signed protocol objects
- `TrustSetManifest`
- `ParameterManifest`
- `DeploymentCapabilityManifest`
- `TrustSetManifestDiff` handling
- replay-scoped `AuditAnswer` (Invariant 2)
- basic `SharedWriteProposal` / `SharedWriteMerged`
- append-only aggregator log with checkpoint hashes + inclusion
  proofs (modes 1–2)
- explicit Stage-A flagger-anonymity declaration
- client refusal rules implemented
- `DeploymentPosture` computation

**MVP must not claim**:
- anonymous flag eligibility beyond Stage A
- mature Phase 4 identity
- DP drift privacy
- coercion resistance
- robust appeal legitimacy
- graph-poisoning resistance against naive clients
- socially-legitimate authority (out of protocol scope always)

Deployments that make any of the forbidden claims in their
`DeploymentCapabilityManifest` without the implementation are
fatal-invalid (§Client refusal rules).

## Meta

Governance-security draft, not an implementable specification.

### Required companion documents

Before implementation, the following must exist:

1. **`docs/kern/security/validity-and-refusal.md`** — *top priority.*
   Defines: what makes an object invalid, what makes a deployment
   degraded, what clients must refuse, what clients may warn about,
   what can be forked without invalidating history, how manifest
   updates are semantically diffed, what claims a deployment is
   forbidden to make at each capability level.
2. `docs/kern/security/objects.md` — wire format + canonical
   encoding.
3. `docs/kern/security/state-machines.md` — lifecycle specs.
4. `docs/kern/security/abuse-cases.md` — expanded per template.
5. `docs/kern/security/parameters.md` — derivations + simulation
   reports referenced from `ParameterManifest`.

The first is the highest-leverage. Without refusal rules with
automated consequence, the rest of the protocol is governance
theater.

### Residual risks (honest list)

Not cryptographic. Social/structural:

- **question-bank capture** — federated, health-reported, probe-
  tested, forkable; governance remains social
- **Phase-4 regional capture** — state machine makes it operational,
  not absolute
- **aggregator metadata correlation** — threshold crypto target;
  stage tracked in `DeploymentCapabilityManifest`
- **graph poisoning via naive clients** — client-policy layer; not
  protocol-preventable
- **genesis cohort permanence** — mandatory sunset, but enforcement
  is social
- **default-client authority** — `TrustSetManifest` + diffs
  required, but most users still click through
- **recovery-quorum majority coercion** — protocol cannot exceed
  weakest-link contact trust
- **recovery-quorum non-invocation under manipulation** — real-time
  victim safety is not primarily a protocol problem
- **compromised signing context** — malware through valid context
  bypasses the gate by definition
- **social legitimacy** — explicitly out of scope (Invariant 20);
  forks exist for disagreement

### Fix pattern

Across all of these: make defaults, parameters, trust sets,
aggregation derivations, bank acceptance, cluster states, and
deployment gaps all signed, inspectable, expiring protocol
objects — each with automated client consequences (refuse,
downgrade, quarantine, warn, fork). Not social-language claims.
Not visibility without teeth.

### Thesis restated

Relay prevents hidden power centers by making power inspectable
**and automatically refusable**. Inspection without consequence is
theater. The whole point of this document is to refuse that outcome.

Any implementation that ships shared-write gating before the
authority ledger (Invariant 18) or makes claims its
`DeploymentCapabilityManifest` does not back (Invariant 17) is a
legitimacy machine with hidden power centers — exactly what this
design refuses.
