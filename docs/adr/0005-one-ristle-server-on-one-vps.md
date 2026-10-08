# ADR 0005: One `ristle-server` deployment on one VPS

## Status

Accepted (2026-10-08). The VPS has not been provisioned yet.

## Context

Server-side needs are Party relaying, TURN relaying for File Shares, per-Account
bandwidth limits, and later shared and public playlists. Ristle legacy used
SpacetimeDB for Party and an unconnected coturn configuration, and it had no
credential broker.

## Decision

One VPS runs:

| Process | Role |
|---|---|
| `ristle-server` (Rust, `axum` + `tokio`) | Accounts and sign-in, Party Relay (WebSocket), TURN credential broker, usage accounting, and later the playlist API |
| PostgreSQL | Accounts, Device Keys, Parties, usage ledger, and later playlists |
| coturn | TURN for File Shares, with credentials minted by `ristle-server` (TURN REST scheme, HMAC over `expiry:accountId:partyId`) |
| Reverse proxy (e.g. Caddy) | TLS on 443 for the API and WebSocket |

`ristle-server` and the Party Relay are one binary and one deployment. Protocol
types come from the shared crate (ADR 0001). SpacetimeDB is not used.

## Consequences

- Party control traffic is small: roughly a few MB per Party-hour, under 1 % of a
  500 Mbps link at expected scale. TURN dominates bandwidth, so coturn gets hard
  limits (`max-bps`, `bps-capacity`) and cannot starve the API.
- A single VPS is a single point of failure. That is acceptable for the beta.
  Keep the server stateless apart from PostgreSQL so it can be replaced later.
- When TURN outgrows the box, the broker can issue credentials for a second
  coturn host or a hosted TURN service without any client change.
- Confirm whether the VPS's 500 Mbps is per direction or combined. TURN traffic
  counts in both directions.
