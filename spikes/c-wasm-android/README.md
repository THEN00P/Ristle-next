# Spike C remainder: Android, a real Plugin, signing (disposable)

Results: `docs/research/plugin-host-spike.md`. The iOS half is in
`../c-wasm-ios/`, and its host crate is reused here.

- `wit/soundcloud.wit`: throwaway WIT (search + resolve, host `http` import).
- `sc-plugin/`: trimmed copy of Ristle legacy's SoundCloud Plugin (`v2.rs`),
  anonymous `api-v2`, client id read from the public web player bundle.
- `pkg/`: manifest and Ed25519 signature in custom sections, tamper and
  takeover checks (`cargo test -p ristle-pkg`).
- `runner/`: the host. CLI `spike-run` and a JNI entry for the Android app.
- `android/`: a one-Activity APK built from the SDK tools (no Gradle).
- `results/`: raw JSON lines. `python summarize.py results/*.jsonl` prints them.

## Run

```sh
cargo build -p spike-guest --target wasm32-unknown-unknown --release --manifest-path ../c-wasm-ios/Cargo.toml
cargo build -p sc-plugin --target wasm32-unknown-unknown --release
cargo run -p plugin-host-spike --release --bin spike-run -- [sections] [modes]
```

Sections: `synthetic,signing,soundcloud,ten`. Modes:
`cranelift+default-memory,cranelift+small-memory,pulley+default-memory,pulley+small-memory`.
Empty means all. Run `ten` once per mode in its own process for clean memory deltas.

Android (NDK 27 from the SDK, `cargo-ndk`):

```sh
export ANDROID_NDK_HOME="$LOCALAPPDATA/Android/Sdk/ndk/27.1.12297006"
cargo ndk -t x86_64 -P 26 build -p plugin-host-spike --release   # -t arm64-v8a for a phone
adb push target/x86_64-linux-android/release/spike-run /data/local/tmp/
adb shell /data/local/tmp/spike-run synthetic,soundcloud
python android/build_apk.py            # app process, results in logcat tag RISTLE_SPIKE
adb install -r target/apk/ristle-spike-c.apk
adb shell am start -n org.ristle.spike/.SpikeActivity --es sections synthetic,soundcloud
adb shell run-as org.ristle.spike cat files/results.jsonl
```

Boot the AVD with `-memory 6144`. The default 2 GB AVD swaps under the Play
Store's post-boot installs, and that voids the timings. Discard the first run
after a boot.
