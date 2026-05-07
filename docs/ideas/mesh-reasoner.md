# Mesh-Trained Reasoner

**Status:** concept
**Date:** 2026-04-25

## One-liner

A small reasoning model whose training corpus is a knowledge graph distributed across the public internet. The graph *is* the model — weights are a compressed echo of mesh state at a given epoch.

## Premise

Today's LLMs separate three things that should be one:

1. **Knowledge** — what is true, stored in weights or retrieved from a vector store.
2. **Reasoning** — how to combine truths, baked into weights at pretraining.
3. **Provenance** — who said what, usually lost.

A federated knowledge graph collapses these. Nodes carry claims. Edges carry relations and provenance. A model trained on this graph internalizes reasoning *over* the structure rather than memorizing facts. The graph remains the ground truth; the model is a fast approximator with audit trail back to source nodes.

## Architecture

### Layer 1 — The mesh

- Peers hold shards of a content-addressed graph. Each node has a stable ID (hash of content + signer).
- Discovery: DHT or gossip. No central registry.
- Edges carry: relation type, provenance signature, confidence, timestamp.
- Anyone can publish a node. Trust is computed, not granted — web-of-trust over signers, stake-weighted edges, or reputation accrued from past correctness.

### Layer 2 — Curriculum extraction

- Graph topology drives training data, not raw text dumps.
- Each `(subject, relation, object)` triple → synthetic training example.
- Subgraph walks → multi-hop reasoning chains.
- Conflicting nodes → contrastive examples ("A asserts X with confidence 0.8; B asserts ¬X with confidence 0.6").
- Sampling weighted by PageRank-like centrality so dense regions train harder.

### Layer 3 — Federated training

- Base language model frozen (small, 1–3B, open weights).
- Each peer trains a LoRA adapter on its local shard.
- Periodic merge: TIES-merge or model-soup across peer adapters. Result is the epoch's reasoner.
- Re-train on graph delta only — cheap incremental updates, not full retrain.

### Layer 4 — Inference

- Tiny model runs locally. Reasons fast over internalized structure.
- On low confidence: queries the live mesh for the relevant subgraph. Treats retrieval as fact-check, not primary recall.
- Every emission can cite back to node IDs — verifiable answers.

## Why this is new

| Existing | Difference |
|----------|------------|
| RAG | Keeps model and data separate. Here, graph *trains* the model; retrieval is a backstop. |
| KG embeddings (TransE, ComplEx) | Embed nodes into a vector space. Here, full LM weights absorb reasoning *over* the graph. |
| Federated learning (FedAvg) | Trains on private text shards. Here, the shard is a typed graph slice with provenance. |
| Bittensor / Petals / Hivemind | Distribute compute. Here, the corpus itself is distributed, typed, and signed. |
| KGPT, JAKET | Pretrain on KG + text once. Here, training is a continuous loop tied to live graph deltas. |

The gap: **no system today treats a federated, signed knowledge graph as the primary training substrate with a live delta-driven re-training loop.**

## Locality and sharing

Not all knowledge wants to leave the machine. The mesh distinguishes two tiers:

### Local tier — folders as memory

- Any folder on disk can be ingested as a private graph shard.
- Nodes from local folders are tagged `local` and never published.
- The reasoner trains on them through the local LoRA pass — they shape the model's behavior without leaking content.
- Use cases: personal notes, codebases under NDA, drafts, journals, customer data.

### Shared tier — model deltas as currency

- What *does* leave the machine is the LoRA delta trained on the local shard, not the shard itself.
- Deltas are gradient-compressed, optionally DP-noised, signed by the contributor.
- Other peers merge the delta into their reasoner without ever seeing the source nodes.
- Effect: knowledge propagates as *behavior change*, not as text. The graph stays home; the model travels.

### Boundary control

- Each node carries a visibility flag: `local`, `mesh-public`, `mesh-trusted-group`.
- The curriculum extractor respects flags. Local nodes only feed the local LoRA pass.
- A peer may publish a public sub-shard while keeping the rest dark — same machine, two scopes.
- Group-scoped sharing (encrypted gossip among trusted peers) sits between fully private and fully public.

This is the ergonomic core of the system: **mark a folder local and it shapes only your model; mark a node public and it shapes the mesh.** No middle bureaucracy.

## Local-first training, opt-in federation

Compute is asymmetric. Each peer trains on what it cares about, with whatever hardware it has. Federation is a choice, not a requirement.

### Local specialist forge

- A peer trains LoRAs on its local shards for its own tasks. No network needed.
- Output: a personal reasoner specialized to that user's domain — their codebase, their notes, their workflow.
- Hardware floor is low: a laptop GPU or even CPU can train small adapters overnight.
- The model is useful *immediately* without ever touching the mesh.

### Opt-in feedback to federation

- When a local LoRA proves useful and is trained on shareable nodes, the peer can publish the delta.
- Publication is explicit: a deliberate "contribute" action, not a default.
- The peer chooses what to share: full delta, distilled subset, or only the gradient signal over public nodes.
- The mesh merges the contribution into the global reasoner on the next epoch.

### Why this matters

- **Sovereignty.** No one is forced to upload anything. Privacy is the default.
- **Practical incentive.** A peer gets value (a working specialist) before contributing — the reverse of most federated schemes.
- **Quality filter.** Only deltas that earned their keep locally tend to get published. Junk training stays junk-local.
- **Asymmetric compute, symmetric voice.** A laptop's contribution is structurally the same as a datacenter's — just smaller. Merge weights by demonstrated correctness, not by training FLOPs.

The model becomes a layered artifact: a stable mesh-trained base, plus your private specialist layer on top. You can run pure-local, pure-mesh, or any blend.

## Properties

- **Censorship-resistant.** Kill a peer, graph and model survive.
- **Verifiable.** Every claim traces to a signed node.
- **Pluralist.** Disagreement is preserved as multi-modal output ("source A: X; source B: Y") rather than averaged away.
- **Cheap to contribute.** A laptop peer trains a LoRA on its slice; a rich peer runs the merge.
- **Specialist by default.** A peer can train its own reasoner on its own subgraph — domain models fall out for free.

## Hard problems

1. **Sybil resistance.** Fake peers poison the graph and through it the model. Mitigation: stake, web-of-trust, or proof-of-correctness over time.
2. **Convergence vs drift.** Shards mutate faster than merges propagate. Bound: epoch length < global drift rate. Open question: what's the right epoch?
3. **Privacy.** Federated gradients leak shard content. Differential privacy noise or secure aggregation required if shards are sensitive.
4. **Bandwidth.** Full graph traversal is infeasible. Each peer samples by local centrality and remote demand.
5. **Catastrophic specialization.** Tiny model + dense graph training = loses general language. Mitigation: freeze base LM, treat graph training as adapter layer only.
6. **Verifiability of the model itself.** How does a user know the running model was trained on the graph it claims? Open problem — possibly attestation over training logs, or zk-proofs of training.

## Relation to this project

The `knids` knowledge graph in this repo is a candidate substrate. It already has:

- Content-addressed node IDs.
- Typed edges with provenance.
- Local-first storage with sync hooks.

What's missing for the mesh-reasoner concept:

- Federation protocol (DHT or gossip layer over knids nodes).
- Signed publication and verification.
- Curriculum extractor: graph → training examples.
- Federated LoRA training pipeline.
- Merge cadence and epoch protocol.

## Minimum viable demonstration

1. Two knids instances, gossip sync between them.
2. Curriculum extractor that turns a subgraph into training pairs.
3. LoRA fine-tune of a 1B base model on each instance's slice.
4. TIES-merge the two adapters.
5. Show the merged model answers a question that requires nodes from both shards — and cites them.

If that works end-to-end, the rest is scale and protocol hardening.

## Open questions

- Is the graph schema rich enough to drive reasoning, or does it need a separate "reasoning trace" node type?
- Does the model need to learn the graph schema explicitly, or can it infer it from edge patterns?
- What stops the merged model from regressing on general language? Continual learning literature suggests rehearsal — but rehearsal on what corpus?
- Who pays for the merge compute? Is there a market structure (Bittensor-like) that falls out naturally?

## Name

Working title: **mesh-reasoner**. The mesh is the graph; the reasoner is its echo.
