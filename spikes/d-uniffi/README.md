# Spike D: UniFFI between the Rust core and Kotlin/Swift (disposable)

Results: `docs/research/uniffi.md` and ADR 0001 (Consequences).

- `core/`: core stub. Async functions, a `StateObserver` callback interface, an
  async foreign trait (`TokenProvider`), a 100 ms fake playhead (push via
  `subscribe`, pull via `next_state`) and `stats` counters for leak checks.
- `uniffi-bindgen/`: the bindgen CLI, pinned to the same UniFFI version.
- `android/`: Compose screen + Media3 `MediaSessionService`, both subscribed to
  one core. Gradle runs `cargo ndk` and `uniffi-bindgen` (`app/build.gradle.kts`).
- `ios/`: XcodeGen project: the app with bindings, a baseline app without them
  (size delta), an XCTest suite and a Swift 6 language-mode check of the bindings.

## Android (Windows)

Needs `cargo-ndk`, NDK 27.1 and the Android Studio JBR as `JAVA_HOME`.

```sh
cd android
./gradlew :app:assembleDebug            # -PrustAbis=x86_64 for an emulator-only build
adb install -r app/build/outputs/apk/debug/app-debug.apk
adb shell pm grant dev.ristle.spike.d android.permission.POST_NOTIFICATIONS
adb shell am start -n dev.ristle.spike.d/.MainActivity
adb logcat | grep SPIKE_                # SELFTEST, HEALTH every 30 s, SERVICE every 100 updates
bash scripts/soak.sh ../build/soak       # 10 min: rotation churn, background, foreground
```

`adb shell run-as dev.ristle.spike.d touch files/jna-default` turns off the JNA
`CallbackThreadInitializer` (see the research note) for comparison.

## iOS (GitHub Actions, macOS runner)

`gh workflow run spike-c-ios.yml --ref spike/d-uniffi -f spike=d` runs the
`spike-d` job: Simulator tests, cold build timings, Release device sizes and a
Swift 6 compile of the bindings. The metrics are in the run summary and the
`spike-d-ios-logs` artifact. Run it rarely: macOS minutes count 10×.
