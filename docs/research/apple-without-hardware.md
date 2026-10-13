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

## Spike C results so far (2026-10-12)

| Where | Pulley search (200 tracks) | Pulley AES-CTR 1 MiB | Native search / AES | Outcome |
|---|---|---|---|---|
| Windows desktop | 12.2 ms | 281 ms | 0.8 ms / 0.1 ms | Pass, identical results in all modes |
| macOS CI runner | 12.1 ms | 198 ms | 0.4 ms / 3.2 ms | Pass |
| iOS Simulator (iPhone 16 Pro, CI) | 12.5 ms | 202 ms | 0.5 ms / 4.3 ms | Pass |
| iPhone 8, iOS 16.6, Test Lab, run 1 | — | — | 1.0 ms / 5.8 ms | Fail: `mmap failed to reserve 0x100000000 bytes: Cannot allocate memory` |
| **iPhone 8, iOS 16.6, Test Lab, run 2 (memory fix)** | **23.5 ms** | **364 ms** | 1.0 ms / 5.7 ms | **Pass**, results identical to native |

What is proven on the real device:
- An ad-hoc-signed XCTest build without an Apple Developer account installs and
  runs in Test Lab (Spark plan, no billing account linked).
- The Rust core runs natively on the device.

What failed: wasmtime reserves 4 GiB of virtual address space per linear memory
by default. iOS caps an app's address space, so instantiation fails. The Simulator
does not reproduce this, because it runs inside the macOS address space.

Fix, verified on the iPhone 8 in run 2:
`memory_reservation(0)`, `memory_guard_size(0)`, `memory_reservation_for_growth(1 MiB)`
and `memory_may_move(true)` for the Pulley engine. Pulley timings are unchanged
with it (12.7 ms / 287 ms on Windows). This becomes a required engine setting on
iOS in the production Plugin host.

### Verdict (2026-10-12)

**WASM components under Pulley work on a real iPhone.** The test used the
oldest device Test Lab offers (iPhone 8, A11), so newer phones will be faster.

- Plugin glue is ~24× slower than native: parsing a 200-track search page takes
  23.5 ms. That is fine for I/O-bound work and confirms that heavy loops do not
  belong in Plugins.
- A Plugin Byte Source decrypting a 320 kbps stream would use ~1.4 % of one core
  on the iPhone 8 (364 ms per MiB, one MiB per 26 s of audio). That is acceptable
  as a fallback. Host-side decryption (ADR 0004) remains the default.
- Compiling the component to Pulley bytecode takes 182 ms on the device. The
  production host should cache compiled Pulley bytecode
  (`Component::serialize`). That is interpreted bytecode, not native code, so
  iOS allows it. Verify this when the host is built.
- Required iOS engine settings: Pulley target, no signals-based traps, and no
  up-front memory reservation (see above).

The Android side of spike C, manifest signing in custom sections and a real
Plugin with network access are in `plugin-host-spike.md`.
