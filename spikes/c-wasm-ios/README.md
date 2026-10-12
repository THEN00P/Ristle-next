# Spike C: Plugin component under wasmtime Pulley (disposable)

Results: `docs/research/apple-without-hardware.md`.

## Run locally (Windows/macOS/Linux)

```sh
cargo build -p spike-guest --target wasm32-unknown-unknown --release
cargo run -p spike-host --release        # native, Pulley and Cranelift report
```

`rust-toolchain.toml` pins Rust 1.97: wasmtime 49 needs at least 1.96.

## Run on a real iPhone (Firebase Test Lab)

1. `gh workflow run spike-c-ios.yml -f device_build=true -f run_simulator=false`
   (manual only, about 2 min warm on a macOS runner).
2. `gh run download <run-id> -n ristle-spike-ios -D build/artifact`
3. `gcloud firebase test ios run --type xctest --test build/artifact/ristle-spike-ios.zip --device model=iphone8,version=16.6 --timeout 10m --no-record-video --project ristle-spike`
4. Fetch `xcodebuild_output.log` from the GCS path gcloud prints and
   `grep '^RISTLE_SPIKE_RESULT'`.

The build is ad-hoc signed, and Test Lab re-signs it, so no Apple Developer
account is needed. On iOS the Pulley engine needs `memory_reservation(0)`,
otherwise instantiation fails with ENOMEM.
