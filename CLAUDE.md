# CLAUDE.md

See `AGENTS.md` for conventions, `CONTEXT.md` for the domain model and `HANDOFF.md`
for the current phase.

Ristle legacy, the previous app, is at `../Ristle`. Its code and tests are a
reference for *behavior*: the C++ tests under `packages/core/tests` and
`packages/app-core/tests` are the spec for the Rust port. Its architecture is not
a template, and nothing from it is to be treated as disposable or "good enough
for now". Its `CLAUDE.md` describes React Native, SpacetimeDB and bridge problems
that do not apply here.

## Words

Only call something a *prototype*, *spike* or *throwaway* when it really will be
deleted. Spikes live in `spikes/` and are deleted once their findings are written
up. Everything else is production code and is built to that standard from the
first commit.

## Volatile tooling: check `docs/agent-tooling.md`

UI driving should be delegated to a cheap GPT "sol" model through `cua-driver` or
computer-use. Details and the last-verified date are in `docs/agent-tooling.md`.
That page goes stale quickly. If its date is more than a month old, ask the user
before relying on it, and update it when you learn something new.

## Next-run orientation

- **Git Bash rewrites leading-slash API paths.** Use `gh api users/...`, not
  `gh api /users/...`. Reading GitHub billing needs the `user` scope, which the
  current `gh` login lacks.
- **wasmtime ≥ 49 needs Rust ≥ 1.96.** The global toolchain is older, so pin it
  per workspace with `rust-toolchain.toml` rather than updating it globally.
  wasmtime's own `Result`/`Error` do not convert from or to `anyhow`, so use
  `wasmtime::Result`.
- **The iOS Simulator hides address-space limits.** Anything that reserves large
  virtual memory (wasmtime memories) passes there and fails on a device. Real
  device runs go through Firebase Test Lab (`spikes/c-wasm-ios/README.md`).
- **Android emulator timings need `-memory 6144`.** The default 2 GB AVD swaps
  while the Play Store installs after boot, which voids measurements. Discard the
  first run after a boot. Other sessions install apps on the same AVD, and adb
  can list a stale `emulator-5562`, so pass `-s emulator-5554`. Results can come
  back through logcat or `run-as`, so no UI agent is needed.
- cmake and ninja for any native build ship inside Visual Studio and are not on
  PATH. Resolve them with `vswhere` (see `../Ristle/CLAUDE.local.md`).
- **`workflow_dispatch` only works for workflow files already on `main`.** New
  Apple jobs go into `spike-c-ios.yml` behind its `spike` input
  (`gh workflow run spike-c-ios.yml --ref <branch> -f spike=d`).
- **The host has 16 GB RAM and the emulator takes about 4.6 GB.** With Gradle
  daemons and other sessions running, the host pages and the emulator stalls
  for minutes, which voids any timing. Run `./gradlew --stop` and check
  `\Memory\Available MBytes` before timing on the emulator. Use the AGP 9.2 line
  with Gradle ≥ 9.4.1 and the Android Studio JBR (Java 21) as `JAVA_HOME`,
  not the global Java 25.
- **Git Bash rewrites `/data/...` and `/sdcard/...` in adb arguments** into
  Windows paths, so pushes and shell commands fail or land in the wrong place.
  Prefix adb calls with `MSYS_NO_PATHCONV=1` (the `spikes/b-audio` scripts do).
  Files adb pushes into a directory it created belong to `shell`, so the app
  can't read them: push into the app's own `files/` directory.
- **C code in Android Rust builds:** cargo-ndk sets the hyphenated
  `CFLAGS_<triple>` variables, which `cc` prefers over the underscore form, so
  pass extra flags as `env "CFLAGS_aarch64-linux-android=..."`. Crates that
  build with cmake (libopus) need the VS-bundled cmake on PATH.
