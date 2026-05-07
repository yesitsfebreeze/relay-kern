# Relay — Project Overview

A single dense document covering the whole project: what it is, why it exists, how the pieces fit, and where it's going. For surface-level orientation see `ARCHITECTURE.md`; for ethics and abuse model see `USE.md` and `SECURITY.md`. This doc is the connective tissue.

## Thesis

Most "AI memory" systems are shaped by the LLM vendor's product cycle: a vector store bolted onto a chat window. Relay starts from the opposite end. The unit is **shared understanding between people and agents**, persisted as a typed graph, federated by gossip, and reasoned over by a long-running process whose loop is deterministic where possible and LLM-driven only where reasoning is required. The model is a participant, not the conductor.

The long bet: as models get cheaper and graphs get richer, the centre of gravity moves *out of the weights*. Knowledge lives in a verifiable, forkable, locally-owned substrate. Reasoning runs on top — sometimes by a frontier model, sometimes by a small one trained on the substrate itself.

## Architecture — three binaries, star topology

Mnemonic: **surface -> agent -> kernel**. Two long-running services and one thin client, talking over `tarpc` on per-cwd Unix sockets / Windows named pipes. Spawn chain: `repl` launches -> spawns `agnt` if missing -> `agnt` spawns `kern` if missing. Last `repl` detaches -> `agnt` warns -> drains -> exits -> `kern` exits.

```
repl -> tarpc -> agnt -> tarpc -> kern
(thin TUI)        (long-running       (graph DB daemon,
                   reasoner)           journal owner)
```

External surface to other tools is MCP. Internal hops are tarpc. Service definitions live in `src/shared/protocol/`.

### kern — knowledge graph daemon

Stores **thoughts** (factual chunks, extracted claims) and **reasons** (justified edges between thoughts: knowledge↔knowledge, thought↔thought, thought↔knowledge). Retrieval is vector + lexical fused with a GNN layer for learned embeddings. Hand-rolled HNSW, GNN, beam search, gossip, MCP. Persistence is `bincode`. Dependencies deliberately minimal.

Pipeline: chunk -> extract thought -> propose reason edges -> query by either dimension -> assemble context.

Condensation: vector clusters periodically consolidate into sub-child databases that stay detached until a query needs them, then lazy-link. Hot graph stays small; long tail stays cheap.

Federation: TCP gossip carries `Sphere`, `Question`, `Pulse`, `PeerExchange`, `Fetch` payloads. No coordinator. CRDT design pending. Authority via damped, typed, scoped PageRank-style scoring. Belief is moving from scalar `confidence` to `(belief, uncertainty)` tuples.

Self-improvement is stigmergic: access leaves a `heat` trace, decayed on tick, reinforced on traversal. `qbst` (Query-Biased Structural Traction) consumes the signal during retrieval. Ant-colony shape, not metaphor — the math is the same.

**The journal lives here.** Append-only JSONL event log. Source of truth across sessions. No "session" entity in the schema — forks are journal pointers. `/continue` and `/resume` attach a new working tip to a prior fork tail.

### agnt — long-running reasoner

Owns the ReAct loop, the recipes, the sub-agents, the LLM clients, and the plugin host. Long-running: outlives any one `repl` session. Deterministic steps dispatch near-inline; the LLM only spends a turn when reasoning is required. The model is one participant in the loop, not the conductor.

**Plugins are MCP servers.** Every plugin — stdio subprocess or in-proc Rust `impl harness::Plugin` — exposes its tools via Model Context Protocol. `agnt` is an MCP client.

**Event-hook layer on top of MCP.** Declarative YAML files bind lifecycle events (`pre_turn`, `post_tool`, `on_error`, …) to MCP tool calls with priority, size caps, cycle guard. `pre_turn` pulls context from `kern` before the LLM call; `post_tool` journals output; `on_error` attaches detail the model sees.

**Recipes** are reusable declarative workflows bundled with prompts (TOML + `pre.md` / `post.md`). Authoring entry point for new tool-surface work. Sub-agents are recipe-bound specialists with small contexts. Each sub-agent emits a structured **receipt** — what changed, what was learned, anchors touched, proposed edges — not prose. The orchestrator reads the receipt; the prose, if any, goes into `kern` as thought content.

State model: `agnt`'s "current goal" pins to a thought-node in `kern` so the conversation thread can't balloon turn-over-turn. Working context is re-derived per turn from graph walk + a slim rolling summary. Crashes don't lose the goal.

### repl — terminal UI

Thin TUI client. Raw ANSI rendered, single self-contained release binary feel. The composer sits at the bottom; UI slots above it are declarative chrome populated by plugins. `repl` holds **no harness state** — it's an RPC client to `agnt`. Detaching `repl` doesn't kill the loop.

**Two plugin surfaces, separate.** Compute plugins use `harness::Plugin` (RPC-like, `Send + Sync`, name-keyed registry) and live in `agnt`. UI plugins use `plugin_ui::View` (paints into a clipped `FrameView`, consumes keys for its region, addressed by `ViewId`) and live in `repl`. Don't bolt one onto the other.

**Repl grammar is hybrid.** LLM parses natural-language intent; slash commands bypass the LLM for determinism. Matches the near-inline-vs-LLM dispatch doctrine.

## Repo layout

```
src/
  bin/
    kern/        # graph DB daemon
    agnt/        # reasoner: harness, recipes, sub-agents, LLM, plugin spawning
    repl/        # terminal UI
  shared/
    protocol/    # tarpc service defs + shared types (agent, memory)
    config-io/   # TOML config loader
    journal/     # append-only event journal (entry, day_journal, history, tracing_layer)
    log/         # tracing setup (logsink)
    search/      # shared search primitives
    trnsprt/     # MCP transport + client + registry
  plugins/
    kern-relay/  # in-tree MCP plugin
  tools/
    xtask/       # doc pipeline + dev tasks
  test_utils/    # test fixtures shared across crates
```

All three binaries are scaffolded and compile. Port is mid-flight — modules land incrementally; some functionality is stubbed (see *Current state* below).

## Locality, federation, and consent

Local-first is the default. Nothing leaves the device without an explicit shared-write step. Each node carries a visibility flag: `local`, `mesh-public`, `mesh-trusted-group`. The local tier is folder-as-memory: notes, codebases under NDA, drafts, journals — ingested into `kern`, never published. The shared tier publishes signed nodes over gossip. The trusted-group tier sits between, encrypted to a peer set.

Federation is opt-in and asymmetric. A laptop peer contributes what it can; a datacenter peer can run merges. No aggregator, bank, or reviewer pool is protocol-canonical. If you disagree, fork; don't capture. Every authority surface is a signed, expiring, forkable manifest. Power that cannot be inspected or refused is not legitimate power.

## Mesh-trained reasoner — forward concept

Beyond retrieval. The federated graph becomes a training substrate.

- Graph topology drives a curriculum: triples -> examples, walks -> multi-hop chains, conflicts -> contrastive pairs, centrality weights sampling.
- Base LM frozen (small, 1–3B). Each peer trains a LoRA adapter on its local shard. Periodic merge (TIES / model-soup) produces the epoch's reasoner.
- Re-train on graph delta only — incremental updates, not full retrain.
- **Local stays local; deltas travel.** The shard never leaves; the gradient signal — DP-noised, signed — does. Knowledge propagates as behavior change, not text.
- **Local specialist forge.** A peer trains LoRAs for its own tasks with no network needed. Useful immediately. Federation is a deliberate "contribute" action when the delta has earned its keep.
- Properties: censorship-resistant, verifiable (cite-back to node IDs), pluralist (disagreement preserved as multi-modal output), specialist-friendly.
- Hard problems: sybil reosistance, convergence vs drift, privacy of gradients, sample bandwidth, catastrophic specialization, attestation that a running model was actually trained on the graph it claims.

Full design: `docs/planned/mesh-reasoner.md`. Adjacent research: `docs/kern/fl-vs-knids-federation.md`.

## Engineering posture

- Hand-rolled where it matters. HNSW, GNN, gossip, MCP client, harness, TUI renderer, event-hook engine.
- Dependencies kept minimal. `bincode` over JSON for persistence. Single-binary feel per service.
- KISS / DRY / YAGNI as a first-class skill, invoked at session start. Every keep/drop/simplify verdict measured against it.
- Rust idioms enforced via the `rust-best-practices` skill. Borrowing over cloning, errors via `Result`, ownership clarity.
- Wizard-mode development for non-trivial work: phased planning, TDD, adversarial self-review.
- Caveman mode for narrative output to keep tokens cheap; code, commits, security messages stay normal.

## Current state — April 2026

The repo is mid-port from a prior monorepo (`relay -> kern`, `kern -> agnt + repl`, board dropped). Each binary is one crate — per-binary internal sub-crates were dissolved into modules.

**Built — `kern`.** `base/`, `commands/`, `config/`, `crdt`, `gnn/*`, `gossip/`, `ingest/`, `llm`, `mcp/`, `quant`, `retrieval/`, `tick/`, `types`, `wire`. CLI dispatch lands; MCP server lands.

**Built — `agnt`.** `agent/` (session, recipe_dispatch, resolver, respawn, tool_specs), `harness/` (plugin host, registry, tee, hooks, runtime, provider, pricing, metrics), `recipe/` (engine, loader, schema, template), `auth/` (file, login, models, providers, secret, paths, interactive), `providers/` (anthropic, openai, local), `commands/`, `context/`, `journal/`, `search/`, `ask/`, `kern_plugin`, `fs_plugin`, `fs_inproc`, `dev_plugin`, `mcp_server`, `lease`, `session_factory`, `dispatch_tees`, `headless`, `rpc`.

**Built — `repl`.** `agnt_client/` + `agnt_spawn`, `app_init`, `chat_view/`, `command_mode`, `commands/`, `input/`, `journal_tail`, `key_handling`, `layout`, `list_nav/`, `login_wizard`, `logo`, `mentions`, `plugins/`, `render/` (cell, diff, emit, frame, grapheme, pass, region, snapshot, surface, sync, theme, theme_config, ws_surface), `slash_lists`, `state`, `status_bar`, `submit`, `textarea/` (binding, buffer, edit_area, form, history, list), `trace_view/`, `tui_sink`, `http_server`, `mcp_server`, `auth_store`, `selection`, `anim`, `log`.

**Built — shared.** `protocol` (agent, memory), `config-io`, `journal` (entry, day_journal, history, tracing_layer, state), `log` (logsink), `search`, `trnsprt` (client, registry, transport, inproc, error, types). `src/test_utils`, `src/tools/xtask` (doc pipeline). One in-tree plugin: `src/plugins/kern-relay`.

**Not built yet.** Spawn chain end-to-end (surf detach → drain → exit). Event-hook engine wiring across full lifecycle. Federation / gossip wire-up beyond stub (`cmd_peers` returns "not yet implemented"). Stigmergy formalisation. PageRank authority. (belief, uncertainty) tuples. Full hybrid `repl` grammar. UI slots / modal stack final form. Recipe receipt contract (structured, not prose). Orchestrator goal pinned to a thought-node.

**Next.** Recipe return contract → orchestrator state model → spawn-chain glue → federation. See `docs/cleanup-audit.md` for current cleanup backlog.

**Dropped.** Board (kanban coordination UI). Out of scope for the clean repo.

## What this project is not

Not a verdict system. Not a content policy. Not a moral authority. Not a targeting system. Reputation, flags, governance mechanics are not weapons. The protocol is designed to refuse capture: hidden scoring, manifest fraud, capability-manifest lies, or coordinated bank-capture attempts are explicitly in the threat model and explicitly unwelcome.

## Why the shape

Three forces converging:

1. **Models commoditize, knowledge doesn't.** The durable asset is the typed, verified, locally-owned graph — not the weights of whatever model was current last quarter.
2. **Local-first is finally tractable.** Cheap inference, cheap LoRA fine-tunes, cheap vector indices. The user doesn't need to rent their own memory back from a vendor.
3. **Reasoning over substrate beats memorization.** A small reasoner over a rich graph outperforms a large model with stale weights on tasks where verifiability matters. The mesh-reasoner concept is the long-tail payoff of the substrate the rest of the project builds.

Build the substrate (`kern`). Build the long-running reasoner that uses it (`agnt`). Make the surface thin (`repl`). Federate when the user asks. Train on the graph when the protocol is ready. That's the whole project.
