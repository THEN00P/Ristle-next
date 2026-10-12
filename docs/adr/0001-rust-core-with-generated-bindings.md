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
  JSON of Ristle legacy. Spike D (2026-10-12, `docs/research/uniffi.md`) confirmed
  that they work on Android and iOS: typed errors in both directions, async
  foreign traits, a 100 ms push stream to a Compose screen and a backgrounded
  Media3 service, and no leaks over a 10-minute churn soak. The binding layer
  adds about 340 KB per Android ABI (JNA plus generated code) and little on iOS.
  An incremental Rust build adds under 10 s per edit.
- The spike fixes these conventions for the core's UniFFI API and the platform
  glue:
  - Long-running async calls take an explicit cancellation token. Generated
    Swift does not forward `Task` cancellation to the Rust future. Kotlin does.
  - The core never panics across the boundary. A panic in a non-throwing
    function traps the iOS process. Fallible functions return `Result`.
  - Callbacks arrive on core threads. Platform observers hop to their UI or
    Media3 thread themselves.
  - Android keeps core threads attached to the JVM with a JNA
    `CallbackThreadInitializer` and creates the core off the main thread.
  - The Swift bindings build in their own Swift 5 language-mode module until
    the generated code compiles under Swift 6, and that module must make the
    generated error enums reachable, since they are generated `internal`.
  - Bindings are generated in library mode from an unstripped build. Packaging
    strips.
- Porting ~23k lines of C++ is the largest single cost of the rebuild. Ristle
  legacy's tests bound the risk.
