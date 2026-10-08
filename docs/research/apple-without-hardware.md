# Apple platforms without Apple hardware

Research from 2026-10-08. The user has no iPhone or Mac until at least
December 2026. The question is whether any Apple-specific unknown could force an
**architecture** rollback, in which case the rebuild should wait for the
hardware.

## Risk by component

| Component | Prior art | Rollback risk |
|---|---|---|
| Rust core via UniFFI in Swift | Element X (matrix-rust-sdk), Mozilla application-services, Bitwarden SDK. Mainstream | Low |
| `cpal` → CoreAudio on iOS and macOS | Standard Rust audio output | Low. Background, interruption and route-change handling are app code, not architecture |
| Qt Quick on macOS | Qt's core desktop target for 15+ years | Low for function. Feel is checked once a Mac exists |
| UIKit and SwiftUI | Apple's own stack | None |
| **wasmtime with Pulley on iOS, with the component model** | `swift-wasmtime` forces Pulley on iOS and ships iOS, simulator and tvOS builds. `wasmtime-kmp` runs on iOS, but with the component model disabled. `rebeckerspecialties/wasm-benchmark` measures Pulley on an iPhone 12 and XS. wasmtime lists `aarch64-apple-ios` as **Tier 3** and marks the component model as supported under Pulley | **Medium. This is the one that matters** |

### Why wasmtime matters

ADR 0003 chose WASM *components* with WIT. On iOS, only an interpreter is
allowed. wasmtime's Pulley is the interpreter that supports the component model.
The usual fallback interpreters (wasmi, WAMR, wasm3) run core modules only. If
Pulley with components failed on a real iPhone, the Plugin ABI would have to drop
components on iOS or everywhere. That would change ADR 0003 and the WIT, and it
could only be discovered after Plugins exist.

Known performance data point: on the same benchmarks WAMR's interpreter is 1.4–1.9×
faster than Pulley on iPhones, and an `audio_dsp` workload takes 537 ms under
Pulley versus 419 ms under WAMR on an iPhone XS. Plugin work is I/O-bound glue
(ADR 0004), so this is acceptable unless spike C shows otherwise.

## Retiring the risk without owning hardware

1. **GitHub Actions macOS runners** build the core for `aarch64-apple-ios` and
   `aarch64-apple-ios-sim`, build the macOS Qt app, and run the test suites in
   the iOS Simulator with Pulley forced. This gives functional coverage. The
   Simulator does not enforce the device's JIT ban and its timing is not
   representative.
2. **Firebase Test Lab on physical iPhones** runs an XCTest bundle on real
   devices for up to 45 minutes per run. Spike C ships a test that instantiates a
   real Plugin component under Pulley, runs search and resolve, a Byte Source
   AES workload and a decode benchmark, and measures them in-test with
   `mach_absolute_time`. A real device enforces the JIT ban, so this test is the
   actual proof.
3. **A rented cloud Mac** by the hour, if a run needs interactive Xcode such as
   signing problems or Instruments traces.

## What still waits for hardware

- The feel of the iOS UI and the macOS desktop app (scrolling, transitions).
- Long background-audio sessions on iOS (lock screen, interruptions, Bluetooth).
- AltStore installation end to end.

None of these is architectural. iOS is phase 6 and macOS ships with phase 4, so
the hardware arrives before they are due.

## Conclusion

Do not defer the rebuild. Do make the **Firebase Test Lab real-device run part of
spike C's pass criteria**, and do not finalize the WIT until it passes. If it
cannot be run (no account, no device availability), that becomes the reason to
wait for the iPhone.

## Sources

- [wasmtime platform support](https://docs.wasmtime.dev/stability-platform-support.html), [tiers of support](https://docs.wasmtime.dev/stability-tiers.html), [Using Pulley](https://docs.wasmtime.dev/examples-pulley.html)
- [swift-wasmtime](https://github.com/OpenCow42/swift-wasmtime), [wasmtime-kmp](https://github.com/brahmkshatriya/wasmtime-kmp), [wasmtime#5754 iOS target](https://github.com/bytecodealliance/wasmtime/issues/5754)
- [wasm-benchmark (iPhone 12/XS results)](https://github.com/rebeckerspecialties/wasm-benchmark)
- [Firebase Test Lab for iOS](https://firebase.google.com/docs/test-lab/ios/get-started)
