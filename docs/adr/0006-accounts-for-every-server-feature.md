# ADR 0006: Accounts for every server feature

## Status

Accepted (2026-10-08). The sign-in methods are proposed.

## Context

Bandwidth on the Relay and TURN must be limited per person. Anonymous
per-install keys cost nothing to mint, so limits attached to them can be evaded.
Shared and public playlists, which are planned, need durable identity across
devices anyway.

## Decision

- Local listening needs no Account. Every server feature that *consumes quota*
  requires one: hosting a Party, TURN relaying, and later shared playlists.
- **Party guests do not need an Account.** Redeeming a Party Invite mints a
  **Guest Token**: scoped to that one Party, expiring with it, and billed to the
  host's quota. A guest who is signed in joins with their Account instead, and
  the host's quota still pays for the Party's traffic.
- `ristle-server` implements sign-in itself, with no separate identity server.
  Proposed methods: OpenID Connect with Google, Apple, GitHub and Discord, plus
  email magic link. Passkeys can follow.
- Each installation creates a **Device Key** pair in the platform credential
  store and registers its public key to the Account. Connections authenticate
  with a short-lived access token plus a signature over a server nonce, so a
  leaked token alone cannot impersonate a device.
- Guest devices still create a Device Key, so a Guest Token is bound to one
  device and cannot be passed around.
- Usage limits and the ledger attach to the Account. TURN usernames embed the
  Account id so coturn usage maps back to it.

## Consequences

- PostgreSQL holds Accounts, Device Keys and sessions from the first server
  release.
- Sign in with Apple is not required for AltStore distribution, but it is cheap
  to include and expected by iOS users.
