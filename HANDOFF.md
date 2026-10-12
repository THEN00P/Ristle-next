# Handoff

Current phase: **1, design and spikes.** Decisions are recorded in ADRs
0001–0011. Next come the Plugin API design and four spikes (A–D).

Spikes are the only disposable work in this repository. Each lives in
`spikes/<letter>-<name>/` and is deleted once its findings are written into the
matching ADR or research document. Everything outside `spikes/` is built to
production standard from the first commit. Ristle legacy (`../Ristle`) is not
disposable either: it is the behavioral reference.

## Phases

| Phase | Content | Exit |
|---|---|---|
| 0. Decisions | ADRs, `CONTEXT.md`, UI rules, Plugin API requirements, licences | Done |
| 1. Design and spikes | WIT draft, and spikes A–D below | Go/no-go per spike, ADRs updated |
| 2. Core | Rust workspace: domain, queue, sequences, storage, Plugin host, audio. Port Ristle legacy's C++ tests | CI green, every legacy test ported or explicitly retired |
| 3. Android | Complete app: browse, search, play, background, sign-in | Daily-drivable on a phone |
| 4. Desktop | Qt Quick app for Windows and macOS | Daily-drivable, passes the frame-timing method |
| 5. Server and Party | `ristle-server`, accounts, Guest Tokens, Relay, TURN broker, Party UI | Party with 3 devices across networks |
| 6. iOS | UIKit/SwiftUI app, AltStore build | Installs through AltStore |

## Design task: WIT draft

Input: `docs/plugin-api/requirements.md`, and from Ristle legacy:
`packages/provider-protocol`, `docs/plugin-api.md` and
`WEB_CAPTURE_FRAMEWORK.md`.

Output: `plugin-api/wit/*.wit`, plus a walkthrough showing how the SoundCloud
and Spotify Plugins from Ristle legacy (`packages/*-provider-plugin`) map onto
it, covering every data domain. The walkthrough is the test of whether the API is
verbose. Review it with the user before host code is written. The WIT is not
final until spike C has passed on a real iPhone.

**Status 2026-10-12:** draft `ristle:plugin@0.1.0` is in `plugin-api/wit/`
(validated with `wasm-tools component wit` and `wit-bindgen rust`). The
walkthrough, with the decisions and open questions for the user at its end, is
`docs/plugin-api/walkthrough.md`; packaging is `docs/plugin-api/packaging.md`.
Waiting for the user's review. Before it is final, a short Spotify spike should
confirm the access-point key and spclient paths, which decide whether the
`socket` import stays.

## Spike A: Qt Quick desktop

Goal: certify Qt Quick for the desktop app, or trigger an evaluation of GPUI.
Rust models through cxx-qt, logic-free QML, Lumen styling.

**Measurement follows `docs/research/frame-timing-method.md` with no
shortcuts.** Its positive control (injected stalls must be detected) runs first.
Any result without a load ramp that reached a breaking point is void.

Scenarios, on the user's Windows machine at 60 Hz and the maximum refresh rate,
and at 100 %, 125 % and 150 % scaling:

1. A queue with animated insert, remove and drag reorder. Every desktop frame is
   captured to check for corruption. Transitions stay enabled; Elisa disables
   them because of QTBUG-49868.
2. A track table with artwork, ramped to 1M rows. Flings, wheel and touchpad.
   Scrollbar behavior is checked.
3. Toggling the sidebar while a large list is visible.
4. Switching between two heavy pages, both with cached pages and with fresh ones.
5. Text sharpness compared against native Windows apps at each scaling, using
   pixel crops from duplicated frames.
6. One Lumen glow or blur element through `MultiEffect`, measured for its
   frame-time cost.
7. Idle memory with the 1,000-row page open, and the 30-minute soak.

Baselines: Ristle legacy and a native Windows list, measured with the same
harness.

macOS: CI builds and smoke-tests the app on a macOS runner. Timing on macOS
waits for the user's Mac (see `docs/research/apple-without-hardware.md`).

## Spike B: audio pipeline

Criteria are in `docs/research/audio-pipeline.md`: Symphonia codec CPU, the cost
of an interpreted Byte Source, gapless playback, HLS (including how common TS
is), and Android background playback through Media3.

## Spike C: WASM Plugin host

**Status 2026-10-12:** item 5 (real iPhone) and the iOS half of 1 and 3 have
passed. Results are in `docs/research/apple-without-hardware.md` and the code is
in `spikes/c-wasm-ios/`. Still open: Android, a real Plugin (2) and signing (4).

1. Build wasmtime with Cranelift for Windows and Android, and with Pulley and the
   component model for `aarch64-apple-ios` and `aarch64-apple-ios-sim`.
2. Compile Ristle legacy's SoundCloud Plugin as a component against a minimal WIT
   (search + resolve) and run it in the host on Windows and Android.
3. Measure instantiation time, the cost of one search call, and memory per
   instance, both with JIT and with Pulley forced.
4. Embed a manifest and signature in custom sections, verify them on load, and
   reject a tampered module.
5. **On a real iPhone through Firebase Test Lab:** an XCTest that runs the
   component under Pulley (search, resolve, a Byte Source AES workload), timed
   in-test with `mach_absolute_time`. This is a pass criterion, because only a
   device enforces the JIT ban. If it cannot be run, stop and report. Waiting for
   the user's iPhone is then the fallback.

## Spike D: UniFFI

1. A Rust core stub with an async function, a callback interface (state
   observer) and a streaming update every 100 ms.
2. Kotlin: call it from a Compose screen and a Media3 service, with no leaks
   after 10 minutes and no main-thread blocking.
3. Swift: the same from a minimal iOS app, run in the Simulator on a macOS CI
   runner.
4. Record the binding size and build-time overhead in both apps.

## Cost guardrails

- **Firebase project `ristle-spike` stays on the Spark plan.** Never upgrade it,
  link a billing account or start a trial. Spark allows 5 physical-device runs
  per day, and each device in a matrix counts as one run. Test Lab shuts down on
  2027-09-30.
- **GitHub Actions on this private repo:** macOS minutes count 10×. Apple jobs
  are `workflow_dispatch` only until the repository is public. Keep runs few and
  reuse the cache.
- Anything else that could bill (VPS, paid services) needs the user's approval
  first.

## Running the work

- This repository's coordinating thread launches the design task and each spike
  as separate threads, each with its brief from this file. Those threads read
  this repository and Ristle legacy, not the coordinating conversation.
- UI driving for functional checks is delegated to a cheap model. See
  `docs/agent-tooling.md`, and check its last-verified date first. Timing
  evidence never comes from UI agents.
- Results come back as edits to the ADRs and research documents and are reviewed
  with the user before phase 2 starts.

## Ristle legacy

`../Ristle` is the behavioral reference:

- Behavior spec: `packages/core/tests`, `packages/app-core/tests`.
- Domain: `CONTEXT.md` (ported here) and ADRs 0001–0024.
- Plugins to port into their own repositories: `packages/spotify-provider-plugin`,
  `packages/soundcloud-provider-plugin`.
- Known service quirks: `../Ristle/CLAUDE.md` (the SoundCloud mobile app versus
  desktop app, Spotify rate limits, positional Liked Songs) and
  `docs/soundcloud-favorites-web-observations.md`.
