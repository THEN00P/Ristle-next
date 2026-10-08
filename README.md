# Ristle

Ristle is one listening app for music from many sources. It is a player and a
UI. Search results, libraries, playlists, feeds and streams come from Provider
Plugins: sandboxed WebAssembly components that anyone can write.

This repository is a ground-up rebuild of Ristle. The previous app, **Ristle
legacy**, stays at `../Ristle` as a read-only reference.

## Layout (planned)

| Path | Contents |
|---|---|
| `core/` | Rust workspace: domain, queue, storage, plugin host, audio pipeline |
| `plugin-api/` | WIT interface and SDK for Provider Plugins |
| `apps/android/` | Kotlin, Jetpack Compose, Media3 |
| `apps/ios/` | Swift: UIKit shell and SwiftUI screens |
| `apps/desktop/` | Qt Quick UI for Windows and macOS, with the Rust core through cxx-qt |
| `server/` | `ristle-server`: accounts, Party relay, TURN credential broker |
| `docs/` | Domain model, ADRs, research, UI rules, design spec |

## Start here

- [`HANDOFF.md`](HANDOFF.md): current phase, next steps, spike briefs
- [`CONTEXT.md`](CONTEXT.md): domain language
- [`docs/adr/`](docs/adr): decisions
- [`docs/agent-tooling.md`](docs/agent-tooling.md): volatile agent tooling notes (check the date)
