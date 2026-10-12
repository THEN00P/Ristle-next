# UniFFI between the Rust core and Kotlin/Swift

Spike D, 2026-10-12. Code: `spikes/d-uniffi/` (deleted after review). Question
from ADR 0001: do UniFFI async functions and callbacks behave well on Android and
iOS?

**Verdict: go.** Async, callbacks, typed errors and cancellation all work on both
platforms. The service kept streaming with the app backgrounded, and a 10-minute
churn soak leaked nothing. There are six sharp edges, listed below. Each one has
a known workaround, and the core's API conventions have to absorb them before
phase 2 starts.

## Setup

- UniFFI 0.32.2, library mode (`uniffi-bindgen generate --library`), Rust 1.97.
- Core stub: a `Core` object with async `load_track` (typed `CoreError`), a
  `StateObserver` callback interface, an async foreign trait `TokenProvider`, a
  100 ms fake playhead delivered by push (`subscribe`) and by pull (`next_state`
  long-poll), and counters for leak checks.
- Android: Compose screen plus a Media3 `MediaSessionService` whose
  `SimpleBasePlayer` only mirrors core state. Both subscribe to one `Core`.
  StrictMode `detectAll` and LeakCanary 2.14 on. Debug build on the
  API 36.1 x86_64 emulator.
- iOS: an XCTest suite hosted by a SwiftUI app, run once in the iPhone 16 Pro
  Simulator (Xcode 16.4) by the `spike-d` job of `spike-c-ios.yml`.

## Results

| Check | Android (Kotlin) | iOS (Swift) |
|---|---|---|
| Async call, resumes on caller | Yes, on `Dispatchers.Main` | Yes, on `@MainActor` |
| Typed error, Rust to platform | `CoreException.NotFound(id)` | `CoreError.NotFound(id:)` |
| Typed error, platform callback to Rust | Kotlin `throw CoreException.Provider` arrives as `Err` | Same |
| Undeclared exception in a callback | Maps to `CoreError::Provider` via `UnexpectedUniFFICallbackError` | Same |
| Platform cancels a Rust future | 200 of 200 Rust tasks aborted, no handle left behind | **Not forwarded** (see below) |
| Rust drops a future awaiting the platform | Kotlin coroutine cancelled | Swift `Task` cancelled |
| Panic | `InternalException`, core still usable | Throwing fn: `UniffiInternalError`. Non-throwing fn: **process trap** |
| Push stream at 100 ms | 10 updates per 1.09 s to screen and service | 18 updates in 2 s |
| Callback cost (Rust side, per `on_state`) | 0.34–0.46 ms avg, 1–4.5 ms max (debug, emulator) | 0.08 ms avg, 0.23 ms max |
| Unclosed subscription | Lives until the GC `Cleaner` runs | Freed at once by ARC |
| Subscribe + cancel churn | 70 activity recreations, no growth | 1.6 µs per cycle, observer released |

The fake ticker ran at about 9.2 Hz rather than 10 on both platforms. That is
the stub's `tokio::time::interval` with `MissedTickBehavior::Delay`, not the
binding.

### Android checks

- **Main thread.** After warm-up, main-thread lag (a probe posting every 50 ms)
  stayed at 1–30 ms in the foreground and 1–17 ms in the background. Synchronous
  FFI calls on main (`stats`, `subscribe`, `play`) took 0.3–2.6 ms each in the
  debug build. StrictMode reported violations only from JNA's first-use
  initialisation (below).
- **Leaks.** During the soak (4 min of rotation every 5 s, 4 min backgrounded,
  2 min of rotation again) `dumpsys meminfo` ended at 1 Activity and 7 app
  contexts. LeakCanary saw each destroyed `MainActivity` collected and never
  reached its dump threshold. Live subscriptions returned to 2 in the foreground
  and 1 in the background. UniFFI's continuation and foreign-future handle maps
  stayed at 0–1. The Java heap stayed at 5–10 MB and the native heap at
  19.6–20.6 MB. Only the two core worker threads stayed attached to the JVM.
- **Background.** With the app on the home screen the service received 100
  updates every 10.9 s, the same rate as in the foreground. `dumpsys` shows it
  as a `mediaPlayback` foreground service with a Media3 transport notification
  and an active media session.
- **UI.** A delegated GPT-6.1-Sol agent drove the screen. Push and pull times
  advance together, Play/Pause stops and resumes both, and "Load missing",
  "Panic" and "Load" show `NotFound(id=missing-7)`, the `InternalException` and
  the new track without a crash. It could not open the notification shade, so
  the notification check above rests on `dumpsys`.
- The first soak ran while the host was paging hard (32 GB committed on 16 GB
  RAM, 0.4 GB free). The emulator then stalled for up to 100 s and every thread
  slowed down, Rust ones included. I took the lag and timing numbers above from a
  second, clean 6.5-minute run. The leak and background results come from the
  first soak and do not depend on timing.

## Sharp edges

1. **JNA attaches a new JVM thread for every callback.** By default a callback
   from a Rust thread attaches, runs and detaches, so each `on_state` makes a new
   `java.lang.Thread` (`Thread-317`, `Thread-617`, ...). Calling
   `Native.setCallbackThreadInitializer(cb, CallbackThreadInitializer(true, false, "ristle-core-cb"))`
   on the generated callback objects keeps core threads attached. It cut the
   average callback cost from 0.60–0.69 ms to 0.37–0.43 ms and stopped the
   thread churn. The objects are `internal` in the generated file, so the app
   module that compiles the bindings has to install this. Swift has no such cost.
2. **JNA's first use touches disk on the calling thread.** `Native.<clinit>`
   creates and cleans a temp directory. On the main thread that took 59 ms warm
   and 0.5–1.1 s cold, with StrictMode disk read and write violations. Construct
   the core off the main thread.
3. **Swift `Task` cancellation does not reach Rust.** The generated
   `uniffiRustCallAsync` awaits `withUnsafeContinuation` with no cancellation
   handler. A cancelled task waited out the full 1.5 s Rust sleep, and the Rust
   future was never dropped. Kotlin uses `suspendCancellableCoroutine` and frees
   the Rust future on cancel. Workaround, verified: long-running core calls take
   a `CancelToken` object, and the Swift side wraps the call in
   `withTaskCancellationHandler { ... } onCancel: { token.cancel() }`. That
   returned `CoreError.Cancelled` after 0.11 s.
4. **A Rust panic in a non-throwing function kills the iOS process.** Swift
   wraps such calls in `try! rustCall`. Kotlin throws `InternalException` for the
   same panic. The core must not panic across the boundary. Exported functions
   that can fail return `Result`, and a panic there arrives as an untyped
   `UniffiInternalError`, a `fileprivate` type.
5. **The Swift bindings are not Swift 6 clean.** With an async foreign trait
   present, the generated `uniffiTraitInterfaceCallAsync` fails in Swift 6
   language mode with 2 errors ("passing closure as a 'sending' parameter"). In
   Swift 5 mode with complete strict-concurrency checking they are 2 warnings.
   Compile the bindings in their own Swift 5-mode module until upstream fixes
   it.
6. **The generated Swift error enum is `internal`.** `enum CoreError` has no
   `public`, while records and objects do. Code in another module cannot catch
   typed errors. In this spike the bindings compile into the app module. I did
   not check whether a `uniffi.toml` option changes this.

Smaller things:

- Library-mode bindgen reads metadata from symbols, so `strip = "symbols"` in the
  Cargo profile breaks generation ("No UniFFI metadata found"). Keep Rust output
  unstripped and let AGP and Xcode strip when packaging.
- tokio timers need a runtime context, and the platform polls UniFFI futures from
  its own threads. The stub runs each async body on the core's own runtime, with
  a guard that aborts the task when the platform drops the future. Dropping the
  runtime from a core thread panics, so `Core::drop` uses
  `shutdown_background`.
- Push callbacks from async work (loads, the playhead ticker) arrive on core
  threads. **The stub's synchronous commands (`play`, `pause`, `seek`) notify
  observers inline, on the caller's thread**, which is usually the platform's
  main thread. The threading tests only covered the async path. The production
  core must dispatch every observer notification through its own runtime, so a
  slow observer can never block a UI transport action. Either way, Media3 needs
  its application looper and Compose state wants main, so every platform
  observer hops threads itself. (Found in review on PR #3.)
- Kotlin's `CoreException.message` is `id=missing-1`, not the Rust `Display`
  string. Swift's `localizedDescription` is the Swift type description.
- Kotlin async foreign-trait calls run on `GlobalScope` and
  `Dispatchers.Default`. Swift ones run in an unstructured `Task`.

## Size

| | Android (release, R8) | iOS (Release, arm64, stripped) |
|---|---|---|
| Rust core stub incl. tokio and scaffolding | 720 KB `.so` (arm64), 752 KB (x86_64) | +839 KB executable (911 KB versus a 71 KB baseline app). Includes the Swift bindings |
| JNA native (`libjnidispatch.so`) | 166 KB (arm64), 117 KB (x86_64) | n/a |
| Bindings and JNA in dex | 66 KB `uniffi.ristle_core` + 105 KB JNA | in the line above |
| Generated source | 3,579 lines Kotlin (139 KB) | 2,262 lines Swift (75 KB) + 836-line C header |
| Whole APK, 2 ABIs | 3.56 MB (mostly Compose and Media3) | app bundle 900 KB versus 80 KB |

The binding layer itself costs about 340 KB per Android ABI (JNA plus generated
dex) and is negligible on iOS. Most of the rest is the core's own code, which
will grow with the real core regardless of the binding.

## Build time

| Step | Windows (Android) | macOS runner, M2 Pro VM (iOS) |
|---|---|---|
| `uniffi-bindgen` CLI, cold (once per cache) | 162 s | 102 s |
| Core, cold, release + LTO | 26 s for 2 ABIs | 9 s sim, 8 s device |
| Core, incremental after a core edit | 8 s for 2 ABIs | 3 s |
| Generate bindings | 0.8 s Kotlin | under 1 s Swift |
| App build | Gradle no-op 3 s, after a core edit 11 s, cold release 64 s | Xcode Release 8 s with bindings versus 10 s baseline (noise) |

Per-edit overhead is the incremental Rust build, under 10 s. Cache the bindgen
binary in CI.
