# ADR 0003: WASM components are the only Plugin runtime

## Status

Accepted (2026-10-08). The interface is drafted in `plugin-api/wit/`, with its
rationale in `docs/plugin-api/`.

## Context

Ristle is a UI driven by Plugin data: search, artists, profiles, public and
private playlists, feeds, likes and streams all come from Provider Plugins.
Third-party Plugins are the product, and the bundled ones are placeholders.

Ristle legacy had four Plugin runtimes: native worker processes, a mobile
bundled host, a WASM host, and a Node tier. Each had its own packaging and
supervision rules behind one hand-versioned protocol.

## Decision

- Provider Plugins are WebAssembly components. Their interface is defined in WIT
  and is the single contract between a Plugin and Ristle.
- One host implementation, built on wasmtime, runs on all platforms. It uses the
  Cranelift JIT where the OS allows and the Pulley interpreter where it does not
  (iOS).
- A Plugin has no ambient authority. Network access, storage, caching, sign-in
  flows, time and randomness are host imports, and each is granted against the
  manifest and the user's consent.
- Plugins run in the core's process. A trap or exceeded fuel/epoch budget ends
  that instance only. The host re-instantiates it and fails the call.
- Plugin authors choose any language that compiles to a component (Rust,
  JavaScript through ComponentizeJS, Go, C#…). Ristle provides a Rust SDK and
  templates first.
- API verbosity is the larger risk. Setup friction can be absorbed by templates
  and a step-by-step guide.

## Consequences

- One runtime to secure, profile and document.
- Interpreted WASM on iOS is too slow for heavy work. That is why decoding stays
  in the host (ADR 0004). Plugin work in general must be I/O-bound glue.
- Verified 2026-10-12: a component runs under wasmtime and Pulley on a real
  iPhone 8 (Firebase Test Lab), about 24× slower than native. iOS needs
  `memory_reservation(0)` and no guard region, otherwise instantiation fails
  with ENOMEM. See `docs/research/apple-without-hardware.md`.
