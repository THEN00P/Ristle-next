# Handoff

Current phase: **1, design and spikes.** Decisions are recorded in ADRs
0001–0012. Next come the Plugin API design and five spikes (A–E).

Spikes are the only disposable work in this repository. Each lives in
`spikes/<letter>-<name>/` and is deleted once its findings are written into the
matching ADR or research document. Everything outside `spikes/` is built to
production standard from the first commit. Ristle legacy (`../Ristle`) is not
disposable either: it is the behavioral reference.

## Phases

| Phase | Content | Exit |
|---|---|---|
| 0. Decisions | ADRs, `CONTEXT.md`, UI rules, Plugin API requirements, licences | Done |
| 1. Design and spikes | WIT draft, and spikes A–E below | Go/no-go per spike, ADRs updated |
| 2. Core | Rust workspace: domain, queue, sequences, storage, Plugin host, audio. Port Ristle legacy's C++ tests | CI green, every legacy test ported or explicitly retired |
| 3. Android | Complete app: browse, search, play, sign-in, user-triggered refresh, and background calls (lock-screen and notification likes, Android Auto browsing) | Daily-drivable on a phone, including from the lock screen and Android Auto |
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
walkthrough is `docs/plugin-api/walkthrough.md` and packaging is
`docs/plugin-api/packaging.md`. The user's first review is folded in; the
settled decisions and the remaining open questions are at the end of the
walkthrough. The WIT is final once spike E confirms the Spotify playback path.

## Spike A: Qt Quick desktop

Goal: certify Qt Quick for the desktop app, or trigger an evaluation of GPUI.
Rust models through cxx-qt, logic-free QML, Lumen styling.

**Status 2026-10-19: go for Qt Quick** (ADR 0002 amendment).
Results: `docs/research/qt-quick-spike.md`, code and harness in
`spikes/a-qt-quick/`. Passes on the discrete GPU with four non-default
settings (D3D12, no Animators, position-only list transitions, native text).
Both conditions of the 2026-10-18 verdict were met on 2026-10-19: motion-only
queue transitions came out clean under bursts and paced rates (burst captures
not frame-complete, see the report), and memory was flat over a 3-hour soak.
Software rendering is no longer a gate (frame-timing method, rule 4). Ristle
legacy could not be measured (its debug build needs Metro). GPUI is an
optional comparison.

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

**Status 2026-10-17: go, with design changes** (ADR 0004 amendment). Results:
`docs/research/audio-pipeline.md` (Spike B results), code in `spikes/b-audio/`.
Ran on Windows, the Android emulator, a Pixel 8 and a Xiaomi Mi 9T. Decode
CPU, gapless, the 30-minute screen-off soak and the lock-screen controls passed
on the Pixel 8. The Mi 9T found a Media3 crash on shell media-button events
under Android 11, which needs a check against current Media3.

Criteria are in `docs/research/audio-pipeline.md`: Symphonia codec CPU, the cost
of an interpreted Byte Source, gapless playback, HLS (including how common TS
is), and Android background playback through Media3.

## Spike C: WASM Plugin host

**Status 2026-10-12:** all five items have run. iOS (items 1, 3 and 5):
`docs/research/apple-without-hardware.md`, code in `spikes/c-wasm-ios/`.
Android, the SoundCloud Plugin (2), signing (4) and ten Plugins at once:
`docs/research/plugin-host-spike.md`, code in `spikes/c-wasm-android/`.
Android ran on the emulator only. No arm64 phone run yet. Open questions are at
the end of that document.

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

**Status 2026-10-12: go.** Items 1–4 passed on the Android emulator and in the
iOS Simulator. Results and sharp edges are in `docs/research/uniffi.md`, and the
resulting API conventions are in ADR 0001. The code is in `spikes/d-uniffi/` until
the findings are reviewed.

1. A Rust core stub with an async function, a callback interface (state
   observer) and a streaming update every 100 ms.
2. Kotlin: call it from a Compose screen and a Media3 service, with no leaks
   after 10 minutes and no main-thread blocking.
3. Swift: the same from a minimal iOS app, run in the Simulator on a macOS CI
   runner.
4. Record the binding size and build-time overhead in both apps.

## Spike E: Spotify playback without librespot's player

Goal: confirm the Spotify Stream Source and the Spotify Connect Queue
Projection in `docs/plugin-api/walkthrough.md`. It decides whether the
`socket` and `live` imports stay in the WIT as drafted.

1. With the playback client's OAuth token, fetch Track metadata and
   `storage-resolve` from spclient over plain HTTPS, including the client
   token from `clienttoken.spotify.com`.
2. Open an access point connection (TCP, Spotify's handshake and Shannon
   framing), log in with the same token, and request one audio key.
3. Download the CDN file, decrypt it with AES-128-CTR from offset 0, drop 167
   bytes, and decode the Ogg Vorbis with Symphonia. Seek once with a Range
   request and a computed counter.
4. Run step 2's handshake as a component under Pulley and time it.
5. The catalog reads Spotify's February 2026 changes took away from
   Development Mode apps: several Tracks at once, artist top Tracks, user
   profiles and playlists, and items of other users' playlists, all from
   spclient (see the walkthrough's Development Mode section), and removing
   one occurrence of a Track that a playlist holds more than once, and
   whether a playlist reorder is applied against the `snapshot_id` it names
   (positional adds rely on it).
6. Spotify Connect for the Queue Projection: open the dealer WebSocket, get the
   connection id, announce a device through spclient's connect-state, and
   receive pause, skip, seek and add-to-queue commands from the Spotify app.

The playback client is rate-limited hard. On 429 or an access point refusal,
wait for the returned `Retry-After` or back-off and try again; a refusal is
not a failure of the spike. Native Rust is fine for steps 1–3, 5 and 6.

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
