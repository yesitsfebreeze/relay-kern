# Federation security — operator guide

> **Scope.** This document describes the trust model of the gossip federation
> **as implemented today**, for an operator deciding whether and how to enable
> it. It is deliberately separate from [`SECURITY.md`](SECURITY.md), which is an
> aspirational governance *design target* ("nothing in this document is
> implemented yet"). This file describes the code that actually runs.

## TL;DR

- **Gossip is off by default** (`[gossip] enabled = false`). Single-node kern
  never opens a network port.
- When enabled, federation is **unauthenticated and unencrypted**. Treat the
  gossip network as a **trusted LAN segment only** — equivalent in trust to an
  NFS export or an internal Redis with no auth.
- The blast radius of a malicious peer is **bounded but non-zero**: it can
  inject and confidence-inflate entities within a quarantined remote namespace;
  it cannot overwrite, delete, or downgrade your local thoughts.

## What enabling gossip does

With `enabled = true`, a node:

1. Binds a TCP listener (default `0.0.0.0:7400`) for gossip messages.
2. Broadcasts a UDP discovery announce (default port `7475`) of the form
   `<network_id>:<tcp_addr>`, and listens for peers announcing the same
   `network_id`.
3. Heartbeats peers, broadcasts its root scope and hottest entity bodies, and
   merges inbound entity bodies from same-`network_id` peers into a phantom
   `remote-<network_id>-<kern_id>` kern via the content-addressed CRDT.

## Trust model: what is and isn't protected

### Protected / bounded

- **Default-off.** No attack surface unless you opt in.
- **Network partitioning by `network_id`.** Messages whose `network_id` does
  not match the local node are rejected. This separates co-located deployments.
- **Local data is never overwritten by peers.** Remote entities land in a
  separate `remote-*` phantom kern. Merge is a content-addressed union (ids are
  `sha256` of content), so a peer cannot mutate or delete an id you already
  hold — only contribute to the remote namespace.
- **Per-merge id cap.** A single inbound sync can introduce at most a bounded
  number of new ids into a target kern, limiting flood amplification.
- **Sybil rate-clipping.** A per-peer rate clipper bounds how much one source
  address can push.
- **Seen-set loop suppression** with a TTL and a hard count ceiling, so
  replayed/looping messages are dropped and the set can't grow without bound.
- **Poison-tolerant handlers.** A panic processing one message no longer
  poisons shared locks or crashes the daemon (it degrades to a logged warning).

### NOT protected — assume a peer on the network can do these

- **No encryption.** Transport is raw TCP; discovery is plaintext UDP. Anyone
  who can sniff the segment sees all federated knowledge in cleartext.
- **`network_id` is not a secret.** It is broadcast in the clear in every
  discovery announce. It is a *grouping* key, not an access credential — anyone
  on the segment can read it and join that network.
- **No peer authentication, no payload signatures.** A node cannot prove which
  peer authored an entity. Signed payloads are a known future effort (see the
  comment at `gossip/handler.rs` `handle_entity_sync`); until then the id cap +
  remote-namespace scoping are the accepted bound.
- **Confidence is monotone-increasing under merge.** Entity confidence joins by
  `max`. A malicious peer can publish a claim at maximum confidence, and honest
  replicas cannot lower it. Do not treat a remote entity's confidence as a
  trust signal — it reflects the most optimistic source, not consensus.
- **Content is accepted on cap, not verified.** Entity bodies are accepted up to
  the cap without semantic verification (an intentional, documented decision —
  see the EntitySync content-verification note in the git history).

## Deployment guidance

- **Only enable on a network segment where you trust every host.** Home/lab
  LAN, a private VPC subnet, or a WireGuard mesh — not a coffee-shop Wi-Fi, not
  a shared office VLAN, not the public internet.
- **Do not bind to a public interface.** The default `0.0.0.0:7400` listens on
  all interfaces. On a multi-homed host, set `addr` to a specific private
  interface, and firewall the gossip TCP port and the UDP discovery port to the
  trusted segment.
- **Use a distinct `network_id` per logical deployment** so unrelated kern
  fleets on the same segment do not merge.
- **If you need confidentiality or peer authentication today, provide it at the
  network layer** (run gossip only inside a WireGuard/VPN mesh). The protocol
  itself provides neither.
- **Keep it off if you don't need multi-node memory.** Single-node kern is the
  default and has no network exposure.

## Reporting

Security issues in the federation path: see [`SECURITY.md`](SECURITY.md) for the
broader model and disclosure expectations.
