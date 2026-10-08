# ADR 0010: Provider Plugins live outside the app repository

## Status

Accepted (2026-10-08)

## Context

The repository is public from the start (ADR 0009). Plugins for commercial
services may depend on unofficial APIs or on reverse-engineered protocols such as
librespot, and those carry terms-of-service risk the app itself should not
carry. Ristle also has to prove that third parties can build Plugins with only
the public API.

## Decision

- The app repository contains the Local Files Plugin (built in), an example
  Plugin and a mock Plugin for tests.
- Spotify, SoundCloud and every other service Plugin live in separate
  repositories. They build against the published WIT and SDK like any third
  party would.
- Ristle legacy's Rust Plugins (`packages/spotify-provider-plugin`,
  `packages/soundcloud-provider-plugin`) are ported into those repositories as
  WASM components.

## Consequences

- Plugin API gaps surface immediately, because first-party Plugins have no
  backdoor.
- The app's CI does not depend on any service's availability or credentials.
