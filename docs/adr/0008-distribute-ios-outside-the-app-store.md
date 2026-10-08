# ADR 0008: Distribute iOS outside the App Store

## Status

Accepted (2026-10-08)

## Context

App Store Review Guideline 2.5.2 forbids downloading code that adds or changes
features. Installable Provider Plugins are exactly that. Apps with downloadable
sources, such as Aidoku, ship through AltStore or TestFlight instead.

## Decision

- The iOS app ships through alternative distribution: AltStore PAL and other EU
  alternative marketplaces, plus Japan's when available, and AltStore Classic or
  SideStore sideloading elsewhere.
- There is no App Store build for now. A store build without Plugin installation
  would not be Ristle.
- iOS is the last platform to be built (see `HANDOFF.md`).

## Consequences

- Users in the US and most other regions can only sideload, with weekly re-signing
  on a free Apple ID. This is accepted.
- No JIT on iOS (ADR 0003, ADR 0004).
- No App Store review constraints, so licensing options such as GPL stay open
  (ADR 0009).
