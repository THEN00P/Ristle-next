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

- **Git Bash rewrites leading-slash arguments.** Use `gh api users/...`, not
  `gh api /users/...`, and `powercfg -list`, not `powercfg /list`. Reading GitHub billing needs the `user` scope, which the
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
- **WIT tooling** is installed with `cargo install --locked wasm-tools wit-bindgen-cli`.
  Validate with `wasm-tools component wit plugin-api/wit` and
  `wit-bindgen rust --world full-provider --out-dir <tmp> plugin-api/wit` (the
  WIT path goes last). In WIT, `from` is a keyword (write `%from`), and a
  function cannot share a name with a type in the same interface.
- **Bash heredocs break on `'''`.** A quoted heredoc containing Python
  triple-single-quotes fails with "unexpected EOF". Write the script to a file
  with the Write tool and run it instead.
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
- **Babysitting Codex reviews: T3's PR watch wakes late.** It reacts
  promptly to the edited "Codex Review Summary" comment, but delivers the
  inline review comments about ten minutes late. Poll in the background
  instead: wait until `pulls/1/reviews` has a review by
  `chatgpt-codex-connector[bot]` whose `commit_id` is the head, or a `+1`
  reaction on the PR. Use `gh api --paginate` for both: PR #1 has more
  than 100 reviews, and a single page silently misses new ones. The same
  goes for `reviewThreads` in GraphQL: page with `after:$endCursor` and
  `gh api graphql --paginate`, or the newest threads stay unseen and
  unresolved. Resolve threads with the GraphQL `resolveReviewThread`
  mutation after replying.
- **Qt without an account:** aqtinstall 3.3.0 from PyPI cannot read the Qt
  6.11+ repository layout. Install it from git main
  (`pip install git+https://github.com/miurahr/aqtinstall`). cxx-qt 0.10:
  declare `QAbstractListModel` in one bridge only (two declarations give
  duplicate cast symbols at link time), and a QML property named `on...` is
  parsed as a signal handler. Check a `qproperty`'s type before QML compares
  it: a `bool` compared with `=== 1` is silently false (this voided a day of
  spike A variant runs).
- **Frame timing on Windows** (spike A harness, `spikes/a-qt-quick/`):
  PresentMon works without elevation (the account is in Performance Log
  Users) but cannot target `dwm.exe` by name (use `--process_id`), and it
  loses a process's presents while any Desktop Duplication session runs, so
  never record both at once. A touch-injecting process must outlive the
  measurement: when it exits, Qt rebuilds its device list on the GUI thread
  (~15 ms). Measure Qt with render-thread Animators off (see
  `docs/research/qt-quick-spike.md`).
- **Unattended multi-hour runs:** background Bash tasks die after 2 h and with
  the session, so start long drivers detached (`Start-Process`) and wait on
  their log. `wmic` is gone on this Windows build; find processes by command
  line with `Get-CimInstance Win32_Process`.
- **`../Ristle` does not resolve from a T3 worktree** (they live under
  `~/.t3/worktrees/`). Ristle legacy is at `~/Documents/Ristle`; pass it to
  `docs/plan/legacy_tests.py` as `RISTLE_LEGACY`. Some legacy tests wrap
  their name onto the line after `RISTLE_TEST(`, so a one-line grep misses
  them.
