# ADR 0009: Open source from the start

## Status

Accepted (2026-10-08)

## Decision

| Part | Licence | Why |
|---|---|---|
| App and core (`core/`, `apps/`) | GPL-3.0-or-later | Keeps forks of the player open. Compatible with LGPL Qt and with alternative iOS distribution |
| `ristle-server` (`server/`) | AGPL-3.0-or-later | A hosted fork of the server must publish its changes |
| Plugin API, WIT, SDK, templates and example Plugins (`plugin-api/`) | MIT | Plugin authors may use any licence, including proprietary. A Plugin talks to Ristle only through WIT, so it is not a derivative of the GPL app |

The root `LICENSE` is GPL-3.0. `server/LICENSE` and `plugin-api/LICENSE`
override it for their directories. Every crate's `Cargo.toml` states its SPDX
identifier.

## Notes

- Contributions follow inbound=outbound with a DCO sign-off and no CLA. Relicensing
  later would need every contributor's consent. That is accepted.
- Third-party crates must be licence-compatible. `cargo deny` enforces it in CI.
- Qt is used under LGPLv3 and linked dynamically. GPL-only Qt add-ons are
  allowed under GPL-3.0, but they would block any later move to a more
  permissive licence for the desktop app.
