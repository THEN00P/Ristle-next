# Plugin host spike (spike C remainder)

Research from 2026-10-12. This covers the parts of spike C that do not need Apple
hardware: Android, a real Plugin with network access, package signing, and many
Plugins at once. The iOS half is in `apple-without-hardware.md`. The code is in
`spikes/c-wasm-android/` (raw results in `results/`) and reuses the host crate
from `spikes/c-wasm-ios/`.

## Setup

- wasmtime 49 with the component model. Four engine modes: Cranelift or Pulley,
  each with wasmtime's default memory settings or the iOS settings ("small
  memory": `memory_reservation(0)`, `memory_guard_size(0)`,
  `memory_reservation_for_growth(1 MiB)`, `memory_may_move(true)`).
- Windows: Ryzen 7 7700X, Windows 11, idle machine.
- Android: emulator `Medium_Phone_API_36.1` (Android 16, x86_64, 6 vCPUs on the
  same Ryzen, booted with 6 GB RAM). Runs as a CLI over `adb shell` and inside an
  APK, where the process runs in the `untrusted_app` SELinux domain like any app.
- **No physical Android phone was connected**, so there are no arm64 numbers.
  The arm64 build target is set up (`cargo ndk -t arm64-v8a`) but was not built
  or run.
- Instantiation uses `InstancePre`, which a production host would also use.

Emulator timings are x86_64 code on a desktop CPU. They show that the stack works
on Android and how the modes compare. They do not predict phone speed.

## 1. Android: same component, same report

The synthetic workload from the iOS run: parse 4 pages (200 tracks) of
SoundCloud-shaped JSON, and AES-128-CTR over 1 MiB. Medians in ms. The emulator
column is the range over two CLI runs, and the app column is one run inside the APK.

| Mode | Windows search / AES | Emulator CLI search / AES | Emulator app search / AES |
|---|---|---|---|
| native | 0.80 / 0.11 | 0.43–0.53 / 0.12–0.21 | 0.42 / 0.12 |
| Cranelift, default memory | 0.49 / 7.8 | 0.48–0.74 / 7.8–11.3 | 0.73 / 10.9 |
| Cranelift, small memory | 0.61 / 9.7 | 0.65–1.07 / 10.2–15.4 | 0.93 / 13.6 |
| Pulley, default memory | 9.9 / 215 | 10.1–12.2 / 240–279 | 14.1 / 252 |
| Pulley, small memory | 9.8 / 202 | 10.7–12.4 / 238–275 | 12.1 / 217 |

Compiling this component takes 80–106 ms on Windows and 150–220 ms on the
emulator. Instantiation takes under 0.1 ms everywhere except the app's first
instance (0.4 ms). Linear memory per instance is 1.06 MiB after instantiation
and 2.4 MiB after the workload.

**Cranelift JIT works on Android, in the app process too.** Nothing was needed
beyond the defaults. The `untrusted_app` domain allows the anonymous
writable-then-executable mappings that Cranelift and wasmtime's compiled code
use.

**Android tolerates the default memory settings, but they cost 4 GiB of virtual
address space per instance.** Ten Plugins with defaults added 40.6 GiB of VA, on
the emulator and on Windows alike. With the iOS settings they added under
0.2 GiB.
That fits the emulator's 47-bit address space easily. arm64 Android kernels are
commonly built with 39-bit user address spaces (512 GiB), which would cap a
process at roughly a hundred Plugin instances before anything else. That was
not tested on a phone.

The iOS settings cost Cranelift 20–40 % on tight loops, because bounds checks
become explicit (search 0.49 → 0.61 ms, AES 7.8 → 9.7 ms on Windows). They also
make the compiled artifact about 25 % larger. Pulley is unaffected, since it
checks bounds either way. For real Plugin calls the difference is 0.1–0.3 ms per
search, which is noise next to the network (below).

## 2. Real Plugin: SoundCloud search and resolve

A trimmed copy of Ristle legacy's `soundcloud-provider-plugin` (`v2.rs`: search
across tracks, users and playlists, the transcoding ranking, playability rules,
and stream resolution) compiled to a component against a throwaway WIT
(`spikes/c-wasm-android/wit/soundcloud.wit`). HTTPS is a host import with
`get` and `get-batch`. The host runs a batch concurrently, allows only HTTPS to
the hosts in the signed manifest, and follows no redirects. The package is 256 KB.

**Client id.** Ristle legacy gets it by web capture from a signed-in
soundcloud.com session, or from a registered app id baked in at build time.
Neither was available. The anonymous soundcloud.com web player embeds its own
client id in a public script bundle, so the Plugin fetches the home page, then
the bundles from the last one backwards, and reads `client_id:"…"`. No account
or sign-in is involved. This is the same id the legacy web capture records, and
anonymous `api-v2` search accepts it.

The query was "daft punk" with a limit of 30, giving 10 tracks, 10 artists and
10 playlists. Resolve takes the first playable track (`soundcloud:tracks:88335161`),
fetches it, picks the AAC-HLS transcoding and resolves it to a signed URL on
`playback.media-streaming.soundcloud.cloud`. The host then fetched that playlist
and confirmed it starts with `#EXTM3U`. Every run, on both platforms and in
every mode, returned real results and a working stream.

"Network" is wall time inside the host HTTP import. "Plugin" is the rest of the
call, so it includes copying arguments and results across the component
boundary. Medians in ms, warm instance:

| Mode | Windows search: wall / network / Plugin | Emulator search Plugin | Windows resolve Plugin | Emulator resolve Plugin |
|---|---|---|---|---|
| Cranelift, default memory | 215 / 214 / 0.72 | 0.76–1.15 | 0.06 | 0.09–0.10 |
| Cranelift, small memory | 217 / 216 / 0.91 | 0.94–1.55 | 0.07 | 0.11–0.16 |
| Pulley, default memory | 216 / 205 / 11.5 | 14.2–14.5 | 0.30 | 0.34–0.43 |
| Pulley, small memory | 221 / 208 / 13.8 | 12.1–15.9 | 0.37 | 0.35–0.38 |
| native (same code, recorded responses) | 0.51–0.82 | 0.52–0.92 | 0.01 | 0.01–0.02 |

A resolve is two sequential requests, 98–164 ms of wall time in all modes.

The first search on a new instance also discovers the client id: 5 requests and
3.3 MB of responses, including the 2.9 MB bundle. Its Plugin share is 1.6–4 ms
under Cranelift and 14–19 ms under Pulley. On the emulator one run hit 54 ms.
Emulator wall time for this call was 3–5 s, because the emulator's network
manages about 1 MB/s. Windows took 400–500 ms.

Takeaways:

- **The network dominates.** A Pulley search spends about 5–6 % of its wall time
  in the Plugin. The iPhone 8 ran the synthetic search 2.4× slower than Windows
  Pulley, which would put a Pulley SoundCloud search at about 30 ms of Plugin CPU
  on that phone. That is acceptable.
- **Large bodies through the Plugin are the expensive case.** Scanning the
  2.9 MB bundle is the only call where Pulley costs noticeable CPU. It also grows
  the instance's linear memory from 1.06 to 4.06 MiB, and linear memory never
  shrinks.
- The trimmed Plugin ported with its logic unchanged. Only the HTTP calls and
  the output types changed. That says little about verbosity, because the WIT
  here is a throwaway, but porting cost was not an issue.

## 3. Signing

`spikes/c-wasm-android/pkg/` puts the manifest (id, name, version, author,
hosts, capabilities as JSON) in a `ristle-manifest` custom section, then appends
a `ristle-signature` section holding the publisher's Ed25519 public key and a
signature over SHA-256 of every byte before it. The signature section must be
the last top-level section, so nothing can be added after signing. Wasmtime
accepts the signed package unchanged.

| Check | Result |
|---|---|
| Untampered package | accepted |
| One byte flipped in the core module's code | rejected (bad signature) |
| Manifest host changed | rejected (bad signature) |
| A custom section appended after the signature | rejected (signature not last) |
| Same id, same key | Update |
| Same id, different key | Takeover, rejected unless the user accepts |
| Verify time, 256 KB package | 0.126 ms Windows, 0.127 ms emulator |
| Size overhead | 317 bytes (manifest JSON plus 97-byte signature) |

The unit tests also flip every byte before the signature one at a time, and
every flip is rejected.

**wasmsign2 works on components.** Version 0.2.8 accepts the component header,
puts its signature section first and hashes all sections after it, so an
embedded manifest is covered. On the same package it rejected the tampered copy
and the other key. Verification took 0.6 ms (its SHA-256 is pure Rust), the
overhead was 119 bytes, and wasmtime accepts the signed component. Its signature
does not carry the public key, only an optional key id. A takeover check
therefore needs the publisher key or key id from the manifest. It supports
several signatures on one module, which fits a handover signed by both the old
and the new key.

## 4. Ten Plugins at once

Ten distinct packages (the same code with different manifest ids, so different
bytes), each verified, compiled, given its own `Store` and instantiated, then
called once. Each mode ran in a fresh process. Per Plugin, medians:

| | Windows | Emulator CLI | Emulator app |
|---|---|---|---|
| Verify | 0.14 ms | 0.15–0.16 ms | 0.15–0.16 ms |
| Compile | 118–164 ms | 263–330 ms | 263–330 ms |
| Instantiate | 0.022–0.033 ms | 0.039–0.062 ms | 0.04–0.18 ms |
| Load the compiled artifact instead (`Component::deserialize`) | 0.15–0.5 ms | 0.4–1.6 ms | 1.0–1.7 ms |
| Compiled artifact size | 554 KB Pulley, 616 KB Cranelift, 780 KB Cranelift small memory | same | same |
| Process memory for compiled code | 0.8–1.2 MB | 0.8–1.0 MB | 0.9–1.4 MB |
| Idle instance | 1.1–1.7 MB committed | 26–64 KB resident | 26–64 KB resident |
| After the first search | +3.1 MB | +2.9–4.0 MB | +2.9–4.0 MB |
| Linear memory | 1.06 MiB, then 4.06 MiB after the first search | same | same |
| Virtual address space | 4.1 GiB default, 2–3 MiB small memory | same | same |

Windows reports committed memory, Android reports touched pages, which explains
the idle-instance gap. A deserialized artifact was also instantiated and called
successfully on Windows and inside the Android app, so a compiled-code cache
works on Android without file-backed executable mappings.

Ten idle Plugins cost about 10–30 MB: about 1 MB of compiled code each, plus
the committed initial linear memory. Memory
grows with what a Plugin once held in its linear memory, because that memory is
never returned.

## What this changes

1. **Use the small-memory settings on every platform.** Same config everywhere,
   bounded address space on arm64 Android, and a cost of 0.1–0.3 ms per real
   call under Cranelift. Recorded in ADR 0003.
2. **Cache compiled artifacts on every platform.** Loading one takes 0.15–1.7 ms
   against 120–330 ms to compile, and it works in the Android app process.
   Key the cache by package hash and engine config.
3. **Instantiation is cheap enough to recycle instances.** At 0.02–0.2 ms, the
   host can drop an instance after a trap, or when its linear memory has grown,
   and start a fresh one. That works only if Plugin state lives in host storage,
   which the requirements already demand.
4. **Keep large bodies out of Plugin memory.** The client-id scan is the worst
   call measured. For the WIT, either the web-capture rules extract values
   host-side (the requirements already list web capture for this id), or the
   HTTP import gets a host-side extract, such as a regex over the body that
   returns only the match. Bodies should also have a size cap per request.
5. **Signing works with custom sections, as the requirements planned.** Both
   formats verify in under 1 ms and catch tampering and takeovers.

## Open questions

1. **wasmsign2 or our own format?** I lean towards wasmsign2: it is existing
   prior art, handles components, and supports multiple signatures for
   handovers. The publisher key or key id then belongs in the manifest. Our
   format is smaller and 5× faster to verify, but both costs are negligible.
2. **arm64 Android was not run.** A phone run should confirm the JIT in the app
   process, Pulley speed, and the address-space limit with default settings.
   Firebase Test Lab also offers Android devices, but this brief excluded Test
   Lab runs.
3. **Anonymous client id.** Taking the id from the public web player bundle
   works without sign-in. Whether the shipped Plugin should do this, or only use
   web capture after sign-in as Ristle legacy did, is a product and policy
   decision for the SoundCloud Plugin's repository (ADR 0010).
4. `Component::deserialize_file` (a file-backed mapping) was not tried inside
   the app. Only deserializing from bytes was. Executing from app data files is
   restricted by SELinux on recent Android, so stay with bytes unless measured.
