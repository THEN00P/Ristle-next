# ADR 0007: Host-authoritative Party over a dumb Relay

## Status

Accepted (2026-10-08)

## Context

In Ristle legacy, SpacetimeDB acted as a mailbox between two clients. The
database held part of the authority (command order, host epoch), and the host
device held the rest. The result was outcome tables, host claims, two-stage
results, transaction-batch delivery and reconnect reconciliation, about 8k lines
across four languages. It took five UI retest rounds to stabilize.

## Decision

The Party Host is the only authority. The Relay checks identity and membership
and forwards messages. It does not order, apply or interpret queue changes.

Protocol (types in the shared protocol crate):

- **Guest → Host:** `Command { seq, … }`. `seq` increases per member and is
  persisted on the guest until acknowledged.
- **Host → all:** `State { rev, transport, queue, acks: { member → appliedThrough }, rejections: { member → [(seq, reason)] } }`.
  The outcome of a command is part of the state it produced, so a result can
  never arrive before its state.
- The host persists `appliedThrough` per member in the same SQLite transaction
  as the queue mutation, and ignores any `seq ≤ appliedThrough`.
- On reconnect, the Relay sends the latest stored State. The guest resends every
  pending command above `acks[me]`.
- The Relay keeps the latest State per Party, a bounded inbox of commands while
  the host is offline, and membership. A newer host connection replaces the
  older one, which is closed, so no epoch is needed.
- State is sent whole and compressed once per broadcast. Transport-only updates
  are a separate small message. Deltas against an acknowledged `rev` are an
  optimization for later, only if measurements justify them.
- File Share signaling (offer, answer, candidates, cancel) is a Relay message
  type. Audio goes peer to peer over a DTLS data channel, through coturn only
  when no direct path exists.

Prior art: game-netcode input sequence plus snapshot acknowledgement, and
Replicache's `lastMutationID`.

## Consequences

- Indeterminate results are resolved by resending, which is idempotent by
  construction.
- An unresolvable Track or a bad argument is a *rejection* in State. Only
  transient host failures leave a command pending.
- Search requests and replies are ephemeral Relay messages with request ids and
  are never stored.
- Playback leases (guest credentials for host playback) are dropped until some
  Plugin can issue a narrow, expiring per-Track credential.
