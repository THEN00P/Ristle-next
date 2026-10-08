# ADR 0001: One Rust core with generated bindings

## Status

Accepted (2026-10-08)

## Context

Ristle legacy split its native core across C++ (`core`, `app-core`,
`audio-engine`, ~23k lines) and Rust (Plugins, Plugin hosts, Party client,
~20k lines), joined by a C ABI. Each UI platform had a handwritten bridge of
2–3k lines that mirrored one TypeScript interface through JSON. Those bridges
were among the most frequently changed files. One known bug stayed unfixed
because a single identifier would have had to be threaded through four of them.
Playback state also leaked into JavaScript, so shuffle and Liked Songs behavior
spanned three layers.

## Decision

- The core is a single Rust workspace. It owns domain, queue, sequences,
  storage (SQLite through `rusqlite`), the Plugin host, the audio pipeline,
  caching and Party logic.
- UIs reach it only through generated bindings: UniFFI for Kotlin and Swift,
  cxx-qt for the Qt desktop app. No handwritten marshalling or JSON-as-ABI.
- Ristle legacy's C++ tests are the specification for the port. Port them
  alongside the behavior they cover. Do not port C++ structure.
- Client↔server protocol types live in one Rust crate shared by the app core and
  `ristle-server`.

## Consequences

- An API change is made once and regenerated for every platform.
- UniFFI's async functions and callback interfaces replace the polling and event
  JSON of Ristle legacy. The spike must confirm that async and callbacks behave
  well on Android and iOS (see `HANDOFF.md`, spike D).
- Porting ~23k lines of C++ is the largest single cost of the rebuild. Ristle
  legacy's tests bound the risk.
