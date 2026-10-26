# Audio pipeline: decoding, iOS and codec coverage

Research for ADR 0004 (2026-10-08). Statements marked *(spike B)* are estimates
to be measured, not facts.

## Is the iOS concern real?

Yes. iOS does not let third-party apps create executable memory at runtime. Only
specially entitled browser engines and debugger-attached processes get JIT. So
on iOS WebAssembly can only be **interpreted**:

- wasmtime's **Pulley** is a portable bytecode interpreter. It is the default
  where Cranelift cannot generate code, and it can be forced with the `pulley`
  feature and a `pulley64` target. Official support for iOS as a target has not
  been confirmed. *(spike C)*
- **wasmi** is a mature pure-Rust interpreter and the fallback.
- Precompiling modules to native code (`.cwasm`) does not help. Mapping
  downloaded native code as executable is exactly what iOS forbids.

An interpreter is roughly 10–50× slower than native code. *(spike B: measure
Pulley and wasmi on a real device)* Natively, decoding a 320 kbps MP3 or Vorbis
stream costs about 1 % of one mobile core. Interpreted, that becomes a large
share of a core, continuously, in the background, on battery. **So decoding
cannot happen in a Plugin.** Plugin work on iOS must stay I/O-bound glue: JSON,
paging, signing requests, and small crypto.

## The split

```
Plugin (WASM, possibly interpreted)          Host (native Rust)
──────────────────────────────────          ────────────────────────────────────
resolve(track) → StreamSource        ──▶     fetch: HTTP Range / HLS / Byte Source
  · Progressive { url, headers, hint }        decrypt: AES-CTR/CBC with a Plugin key
  · Hls { url, headers }                      demux: MP4, MKV/WebM, OGG, WAV, AIFF,
  · ByteSource { id, length?, hint }                 CAF, ADTS, (TS: spike B)
  · …+ Decrypt { scheme, key, iv }            decode: Decoder trait → Symphonia /
read(id, offset, len) → bytes  ◀──(only for          platform decoders
                                  ByteSource)  resample → gapless queue → output
```

- **Byte Source** covers lazily loaded, chunked or otherwise non-HTTP media (the
  librespot case). The host drives it: it requests ranges ahead of the playhead
  and on seek, and the Plugin fetches through host HTTP. If the Plugin also
  decrypts, the cost is a few hundred KB per minute of AES in the interpreter,
  which is negligible. *(spike B: confirm)*
- **Host-side decryption** is preferred over a Byte Source whenever the scheme is
  standard, because then the Plugin spends no CPU on the stream at all.
- **DRM** (Widevine, FairPlay, PlayReady) is out of scope by design.

## Codec and container coverage

Symphonia (pure Rust) according to its README:

| Codec | Status |
|---|---|
| MP3, FLAC, Vorbis, PCM | Excellent |
| AAC-LC, ALAC, MP1/MP2 | Great |
| ADPCM | Good |
| HE-AAC, HE-AACv2, Opus, WavPack | Not rated (present, maturity unknown). **Corrected by spike B: Symphonia 0.6.1 has no SBR/PS and no Opus decoder at all** (see the results below) |

| Container | Status |
|---|---|
| WAV | Excellent |
| OGG, ISO/MP4, AIFF | Great |
| MKV/WebM, CAF | Good |
| MPEG-TS | **not supported** |

Gaps and plan:

- **MPEG-TS**: common in HLS. Add a small TS demuxer in the host (PES → ADTS
  AAC/MP3 frames), or require fMP4/ADTS renditions from Plugins. *(spike B: how
  much real HLS uses TS)*
- **HE-AAC and Opus**: verify quality and gapless behavior. If they fall short,
  register platform decoders through the `Decoder` trait (Android MediaCodec,
  Apple AudioToolbox, Windows Media Foundation), or libopus for Opus.
- **FFmpeg** is a last resort only. It is large, its licensing depends on the
  build, and it should not be in the default build.

## Output and background playback

| Platform | Output | Session and background |
|---|---|---|
| Android | `cpal` → AAudio (Oboe) | Media3 `MediaSessionService` as a foreground service. The `Player` is a thin `SimpleBasePlayer` facade over the core, so the core stays the authority |
| iOS | `cpal` → CoreAudio (RemoteIO) | `AVAudioSession` `.playback`, `audio` background mode, `MPNowPlayingInfoCenter`, `MPRemoteCommandCenter`, interruption and route-change handling |
| Windows | `cpal` → WASAPI | SMTC through a small shim |
| macOS | `cpal` → CoreAudio | `MPNowPlayingInfoCenter` and `MPRemoteCommandCenter` |

Gapless playback needs encoder delay and padding from the container (MP4
`edts`/`iTunSMPB`, LAME/Xing headers) plus pre-decoding the next Track.
Symphonia notes that gapless playback needs support from both the demuxer and
the decoder. *(spike B: verify per format)*

## Spike B acceptance criteria

1. Decode CPU per codec (MP3 320, AAC-LC 256, Vorbis 320, Opus 160, FLAC) on a
   low-end Android phone and on Windows. Target: under 3 % of one core.
2. Interpreted Byte Source: a Pulley/wasmi Plugin feeding 320 kbps through AES-CTR
   keeps under 5 % of one core on an iPhone or an interpreter-forced Android
   build.
3. Gapless transitions between MP3 → MP3 and AAC → AAC album tracks, with no
   audible gap (sample-accurate check against the source).
4. SoundCloud-style AAC HLS plays and seeks within 500 ms.
5. Android: playback continues for 30 minutes with the screen off and the app
   backgrounded, controlled from the lock screen.

## Spike B results (2026-10-13 to 2026-10-17)

Code: `spikes/b-audio/`, deleted at M6a; the paths below are in commit `818eac8`
(`git show 818eac8:spikes/b-audio/<path>`). Criterion 2 was measured in spike
C (about 1.4 % of one core on an iPhone 8) and was not repeated.

**Verdict: go, with five design changes** (summarized at the end and carried
into ADR 0004). Symphonia is cheap enough everywhere measured and is gapless for
MP3, Vorbis, Opus and FLAC. It cannot be the only decoder: it has **no HE-AAC
(SBR/PS) and no Opus**, and it does not apply MP4 edit lists. Live radio HLS is
mostly MPEG-TS, so the host needs a TS demuxer. One was written in the spike in
155 lines. It ran on Windows, the Android emulator and a **Pixel 8**: every
codec decodes in under 3 % of a Cortex-A510 little core, and 30 minutes of
screen-off, Doze playback had no underruns. On the phone the player costs about
10 % of a core in total (app plus `audioserver`), mostly from avoidable
wakeups (see section 4).

### Setup

- Symphonia 0.6.1 (`all` features), libopus through
  `symphonia-adapter-libopus` 0.3, FDK AAC through `symphonia-adapter-fdk-aac`
  0.2 (comparison only, see licensing), rubato 0.16, cpal 0.18.2, UniFFI 0.32.2,
  Media3 1.8, Rust 1.97.
- Windows: AMD Ryzen 7 7700X, Windows 11. Android: the API 36.1 x86_64 emulator
  (`-memory 6144`) on the same host. During the emulator runs the host had about
  2 GB free and paged at 100–180 pages/s, with one spike to 16k pages/s. That is
  acceptable for counter-based CPU shares but makes them indicative only.
- Test audio is synthetic and generated by `gen-media.sh`: a chord with vibrato,
  kick and noise hi-hat transients, a chirp and pink noise. No third-party
  audio is used. Encoders: libmp3lame 320k CBR, ffmpeg AAC 256k, libvorbis 320k,
  libopus 160k, FLAC, and FDK for HE-AAC v1 (VBR 3, about 110 kbps) and v2
  (VBR 2, about 46 kbps).
- CPU comes from the OS's per-thread counters (`GetThreadTimes`,
  `CLOCK_THREAD_CPUTIME_ID`). Each file is decoded from memory repeatedly until
  at least 3 s of thread CPU time has accumulated, after a discarded warm-up
  pass. The figure covers demux, decode and conversion to interleaved f32, but
  not file I/O. In the soak, CPU comes from `/proc/<pid>/stat` and
  `/proc/<pid>/task/*/stat`.

### 1. Decode CPU per stream

Share of one core while decoding one real-time stream.

| Stream | Decoder | Windows 7700X | Emulator x86_64 | Pixel 8, unpinned | Pixel 8, A510 core | Mi 9T, unpinned | Mi 9T, A55 core |
|---|---|---|---|---|---|---|---|
| MP3 320 CBR | Symphonia | 0.080 % | 0.112 % | 0.135 % | 0.72 % | 0.214 % | 0.69 % |
| AAC-LC 256 (MP4) | Symphonia | 0.054 % | 0.087 % | 0.111 % | 0.74 % | 0.173 % | 0.62 % |
| AAC-LC 256 (ADTS) | Symphonia | 0.053 % | 0.086 % | 0.107 % | 0.72 % | 0.166 % | 0.61 % |
| Vorbis 320 | Symphonia | 0.076 % | 0.114 % | 0.143 % | 0.93 % | 0.256 % | 0.89 % |
| Opus 160 (Ogg) | libopus | 0.152 % | 0.174 % | 0.240 % | 1.36 % | 0.388 % | 1.09 % |
| Opus 160 (WebM) | libopus | 0.151 % | 0.173 % | 0.241 % | 1.39 % | 0.487 % | 1.08 % |
| FLAC 16/44.1 | Symphonia | 0.050 % | 0.088 % | 0.093 % | 0.50 % | 0.136 % | 0.41 % |
| FLAC 24/96 | Symphonia | 0.130 % | 0.232 % | 0.238 % | 1.22 % | 0.386 % | 1.15 % |
| HE-AAC v1 (ADTS / MP4) | FDK | 0.276 / 0.281 % | 0.263 / 0.263 % | 0.338 / 0.338 % | 2.28 / 2.29 % | 0.675 / 0.659 % | 2.48 / 2.46 % |
| HE-AAC v2 (ADTS) | FDK | 0.309 % | 0.298 % | 0.354 % | 2.55 % | 0.720 % | 2.78 % |
| AAC-LC 256 (MP4), for comparison | FDK | 0.214 % | 0.220 % | 0.263 % | — | 0.504 % | — |
| HE-AAC v1 / v2 (ADTS) | MediaCodec (`c2.android.aac.decoder`) | — | — | — | — | 2.55 / 2.93 % (0.71 / 0.72 % own thread + 1.84 / 2.21 % in `media.swcodec`) | — |
| AAC-LC 256 (MP4 / ADTS), for comparison | MediaCodec | — | — | — | — | 3.53 / 3.76 % | — |
| Resampling 44.1 → 48 kHz | rubato FFT | 0.031 % | 0.033 % | 0.11–0.13 % | — | 0.23–0.26 % | — |

Phones: a Google Pixel 8 (Tensor G3, Android 17) and a Xiaomi Mi 9T
(Snapdragon 730, Android 11, 2019, six Cortex-A55 at 1.8 GHz plus two A76),
both on USB power. "Unpinned" lets the scheduler choose. The core columns pin
the process with `taskset 0f` to CPUs 0–3: the Pixel's Cortex-A510 little
cores at 1.7 GHz, and the Mi 9T's Cortex-A55 cores, the core of most low-end
phones. **Every codec stays under the 3 % target on the slow cores of both
phones**: 0.4–0.9 % for the Symphonia codecs at 16/44.1, 1.1–1.4 % for Opus,
and 2.3–2.8 % for HE-AAC through FDK, the most expensive case.

MediaCodec (implemented in the spike, `lab/src/mediacodec.rs`, measured on the
Mi 9T unpinned) decodes HE-AAC v1 and v2 correctly: 44.1 kHz stereo, so SBR and
Parametric Stereo both work. It costs 2.5–2.9 % of a core in total, most of
it in the `media.swcodec` process (read from `/proc`) and about 0.7 % on our
thread for the buffer queueing, IPC and copies. For AAC-LC it costs 3.5–3.8 %
against Symphonia's 0.17 %, about 20× more, so MediaCodec is only for what
Symphonia cannot decode. Pinning the emulator run changed nothing, as expected
for x86 cores of one type. Raw data: `spikes/b-audio/results/windows-7700x.jsonl`,
`android-emu-x86_64-bench.jsonl`, `android-pixel8-bench.jsonl`,
`android-mi9t-bench.jsonl` and `android-mi9t-mediacodec.jsonl`.

### 2. Gapless

Three tracks (9.93 s, 10.28 s and 9.80 s) were cut at non-frame-aligned sample
positions from one continuous 30 s signal, then encoded separately as an album
encoder would, decoded back to back and compared with the source. The check
reports each track's length delta and start offset (cross-correlation lag) and
the signal-to-error ratio of 10 ms windows around each boundary. A positive
control that inserts one frame at the start of track 2 is caught as lag +1 and
delta +1. The results were identical on Windows, the emulator, the Pixel 8 and the Mi 9T.

| Container | Gapless info | Who trims | Result |
|---|---|---|---|
| MP3 (LAME) | LAME tag: delay 1105 (576 + 529), padding per track | Symphonia demuxer + decoder | **Sample-exact**: delta 0, lag 0 |
| MP4/AAC (ffmpeg) | `elst`: media_time 1024, duration per track. No `iTunSMPB` | **Nobody**. Symphonia parses `elst` but never applies it | Each track starts 1024 frames late and runs 1265–1565 frames long |
| MP4/AAC, edit list applied by the host | as above | host (`mp4edit.rs`, 100 lines, also reads `iTunSMPB`) | **Sample-exact**: delta 0, lag 0. Boundary windows at 21 and 26 dB, above the 1st percentile of all windows (14 dB), so codec noise, not a gap |
| ADTS AAC | none | — | Not gapless: +1024 frames per track, padding unknown |
| Ogg Vorbis | granule positions | Symphonia | **Sample-exact** |
| Ogg Opus | pre-skip 312 + end granule | Symphonia demuxer + libopus adapter (no double trim) | **Sample-exact** |
| FLAC | lossless | — | Bit-exact (241 dB) |

Two further findings:

- **Decoder latency differs per decoder.** The FDK adapter's output trails
  Symphonia's by a constant 1685 frames on the same AAC files. A platform
  decoder (Android's software AAC decoder is FDK) can therefore shift every
  track. The `Decoder` contract must require output aligned to the input
  timeline (the first sample out is the first sample of the first packet), and
  each platform decoder needs a gapless test like this one.
- In the player (section 4) album transitions are seamless because the decode
  thread writes the next track into the same ring right after the last frame of
  the previous one. This needs one output rate. The production host resamples
  everything to the device rate (0.03 % of a core) and must not reset the
  resampler between tracks.

### 3. HLS

The HLS client is ported from legacy `provider-media` and extended. It handles
master playlists with `EXT-X-MEDIA` audio renditions, `EXT-X-BYTERANGE`,
AES-128 with a sequence-number or explicit IV, keys cached per playlist,
packed-audio ID3 stripping, and the TS demuxer. Fixtures are a 4-minute AAC-LC
160k track (SoundCloud's tier) with 6 s segments in five variants: fMP4, packed
ADTS with Apple's ID3 timestamp tag, MPEG-TS, AES-128 fMP4 and AES-128 TS. All
five play and seek correctly. A seek is "seek call to the first decoded frame
at the target, playlist already loaded". The local server simulates the RTT and
paces the body to the bandwidth. (Re-measured
on 2026-10-17 for all five variants in both modes, after review found the
`fmp4-aes` rows missing from the raw file and the streaming fetch counter
stuck at zero: `results/hls-local-windows.jsonl`. The latencies below are
unchanged. In streaming runs the network wait is practically the whole seek
time: 95–100 % for every variant. A second review round found that fetch time
was read after the accuracy check's extra reads, which had pushed TS above
100 %; it is now snapshotted at the latency boundary and re-measured.)
The AES-128 fMP4 fixture puts `EXT-X-KEY` after `EXT-X-MAP`, so its init
segment is not encrypted. An encrypted init section (key before the map) was
not exercised; the spike client would hand its ciphertext to the demuxer.

Seek latency, median (p95), 12 seeks per row:

| Network | Whole segment, then decode | Decode while downloading |
|---|---|---|
| none (localhost) | 2–8 ms | 2–3 ms |
| 30 ms RTT, 20 Mbit/s | 94–99 ms | 64 ms (88) |
| 80 ms RTT, 5 Mbit/s | 296–312 ms | 199 ms (284) |
| 150 ms RTT, 1.5 Mbit/s | 836–887 ms | **527 ms (792)** |

Start latency on the weakest link drops from about 840 ms to 220 ms with
streaming decode. Container makes no difference. AES adds nothing once the key
is cached.

Public streams (decode while downloading, about 60 ms RTT to the CDN):

| Stream | Container | Codec | Start | Seek p50 |
|---|---|---|---|---|
| Apple `img_bipbop_adv_example_fmp4` (audio rendition) | fMP4, byte ranges | AAC-LC | 26 ms | 80 ms |
| Apple `img_bipbop_adv_example_ts` (audio rendition) | packed ADTS, not TS | AAC-LC | 31 ms | 82 ms |
| Apple `bipbop_16x9` alternate audio | packed ADTS, byte ranges | AAC-LC 22.05 kHz | 23 ms | 63 ms |
| BBC Radio 1 live | **MPEG-TS** | **HE-AAC** (`mp4a.40.5`) | 15 ms | 36 ms |
| Radio France FIP live | **MPEG-TS** | AAC-LC | 67 ms | 123 ms |

Findings:

- **Criterion 4 passes on every tested network with byte-range seeks.** With
  streaming decode alone, a seek waits for the bytes up to the target (on
  average half a segment): 527 ms median and 792 ms p95 on the slow link.
  `lab/src/hlsseek.rs` implements byte-range seeks for fMP4 (SoundCloud's
  container). It fetches the first 4 KB of the segment, reads every `trun`
  (Apple's packager writes several per fragment), then requests only the bytes
  from one frame before the target onwards and feeds those AAC frames straight
  to the decoder, with the extra frame priming the MDCT overlap. Positions stay
  sample-accurate (same constant −1024 priming offset as every other path):

  | Network | Streaming decode, median (p95) | Byte-range seek, median (p95) |
  |---|---|---|
  | localhost | 2 ms | 1 ms (1) |
  | 30 ms RTT, 20 Mbit/s | 64 ms (88) | 69 ms (70) |
  | 80 ms RTT, 5 Mbit/s | 199 ms (284) | 190 ms (190) |
  | 150 ms RTT, 1.5 Mbit/s | 527 ms (792) | **391 ms (391)** |
  | Apple's fMP4 example over the internet, three paired runs | 76–79 ms (88–99) | **66–68 ms (70–80)** |

  An earlier single run against Apple gave 147 ms streaming against 72 ms
  ranged. The paired runs show most of that gap was CDN variance. Over a
  fast connection a ranged seek saves only about 10 ms; the gain is on slow
  links. In the ranged runs the fetch time equals the seek time to within
  1 ms (`results/hls-ranged-windows.jsonl`).

  A byte-range seek costs two round trips and about 5 KB regardless of
  segment length or seek position. Not covered: AES-128 segments (CBC needs
  the preceding cipher block as IV, which is one extra block in the range), and
  TS/ADTS, which have no sample table, so the offset has to be estimated from
  the byte rate, then resynchronised on the sync word and corrected with PTS.
  Live radio (TS) seeks rarely, so streaming decode is enough there.
- **Positions need an encoder-delay correction.** Every variant was off by a
  constant −1024 frames (23 ms), the AAC priming, which HLS carries as ordinary
  audio. For fMP4 the init segment's `elst` provides the value. TS and ADTS have
  no signal, so assume 1024 (2048 + 64 for some encoders) or ignore it.
- Symphonia ignores fMP4 `tfdt` and numbers packets from 0 for every
  container, so the host positions by playlist time (`EXTINF` sums).
- Legacy rejected `EXT-X-BYTERANGE`, which two of three Apple examples use. It
  is cheap and now handled.
- **How common TS is.** No published usage statistics exist. Vendor sources
  agree that VOD moved to fMP4/CMAF after 2016, and TS persists in live
  broadcast and legacy set-top paths. What was checked directly: SoundCloud's
  AAC HLS is `audio/mp4` (fMP4) per legacy's Plugin tests. Apple's examples are
  fMP4 or packed ADTS. **Both live radio services probed (BBC, Radio France)
  serve TS**, and BBC's is HE-AAC.
- **Recommendation: a TS demuxer in the host.** Live radio is a core use case
  and cannot be steered by a Plugin preference: there is no fMP4 alternative.
  The spike's demuxer (PAT → PMT → first audio PES, emitting the ADTS or MPEG
  audio elementary stream to Symphonia, plus first PTS) is 155 lines. A
  production version with sections spanning packets, discontinuities, PTS-based
  positioning and tests is estimated at 300–400 lines. `mpeg2ts-reader` is an
  alternative dependency. Plugins should still prefer fMP4 when offered,
  because it carries `elst` for gapless.

### 4. Output and background playback on Android

`spikes/b-audio/player` is the core: a decode thread fills an SPSC ring
(`rtrb`, 2 s), the cpal callback drains it, and a seek hands the callback a
fresh ring through a one-slot queue, so the real-time side never locks. The app
hosts a Media3 `MediaSessionService` whose `SimpleBasePlayer` only mirrors
`PlayerState` and forwards play/pause/seek/next to the core. The album loops:
three MP3 tracks, then three AAC tracks with the host edit-list trim.

**Emulator soak (30 min, home screen, screen off, forced deep Doze): pass.**
`soak.sh` started playback, pressed Home and `KEYCODE_SLEEP`, faked unplugged
power and forced deep idle (`dumpsys deviceidle force-idle`), then sent pause
and play at minute 10 and next at minute 20 through the media session while the
screen stayed off. No wake lock was held.

| | Result |
|---|---|
| Audio played | 1826 s in 1831 s (the 5 s gap is the deliberate pause) |
| Underruns | 0 starved callbacks, 0 AAudio xruns, in 79,003 callbacks. Worst callback 361 µs |
| Track transitions | 182 gapless transitions (MP3 → MP3, MP3 → AAC, AAC → AAC) |
| Commands with the screen off | pause, play and next all arrived and took effect |
| End state | `mWakefulness=Asleep`, `mDeviceIdleMode=true`, foreground service type `mediaPlayback` with the Media3 transport notification |
| App CPU, `/proc` | 1.10 % of one core over 30 min (0.97–1.18 % per minute): decode thread 0.50 %, cpal callback thread (`AudioTrack`) 0.43 %, state thread and observer callbacks 0.07 % |
| `audioserver` CPU | 0.94 % |

**Notification and lock-screen controls (emulator): pass.** The media card in
the notification shade and on the keyguard (with a temporary PIN set) shows
title, artist, seek bar and transport buttons. Pause, play and next pressed on
both arrived as `SPIKE_B_COMMAND`s and took effect. The card's title followed
the core's queue index when sampled every 7 s across a full album loop.
`uiautomator dump` fails while the card animates ("could not get idle
state"), so the buttons were located from screenshots
(`spikes/b-audio/results/ui/`) and pressed with `adb shell input tap`, not by a
delegated UI agent.

On the emulator this is host-desktop CPU. The phone gives the real figure:

**Pixel 8 soak (30 min, same script and album, USB power, faked unplugged +
forced deep idle): pass.**

| | Result |
|---|---|
| Audio played | 1885 s in 1892 s (5 s scripted pause, about 1 s from the user's touch, startup) |
| Underruns | 0 starved callbacks, 0 AAudio xruns, in 179,788 callbacks. Worst callback 1.3 ms |
| Track transitions | 188 gapless transitions |
| End state | `mWakefulness=Dozing`, `mDeviceIdleMode=true`, foreground `mediaPlayback` service, MediaStyle notification |
| Screen | off throughout, except about 30 s at minute 26 when the user unlocked the phone and opened the app, which paused and resumed playback |
| App CPU | **4.55 %** of one core: decode thread 2.40 %, cpal callback thread (`AudioTrack`) 1.10 %, observer callbacks (`ristle-core-cb`) 0.48 %, main thread (Media3 state refresh) 0.48 % |
| `audioserver` CPU | **5.30 %** |

The phone's log buffer wrapped during the 30 minutes, so the scripted
commands were checked again right after the soak, with the phone dozing and the
screen off: pause, play and next through the media session all took effect.
**The lock-screen card works on the real device**: pause, play and next tapped
with the secure keyguard showing all reached the core.

Where the phone's 10 % goes, and what fixes it:

- **The decode thread (2.40 %) is 18× its benchmark figure.** On a little core
  the benchmark is 0.13–0.74 %. The rest is the spike's 20 ms polling loop
  (50 wakeups/s) and small reads. Production should decode in batches and wake
  when the ring drops below a threshold.
- **The output path costs the most (1.10 % callback + 5.30 % `audioserver`).**
  The phone has a deep-buffer output (`AUDIO_OUTPUT_FLAG_DEEP_BUFFER`,
  1920-frame periods). cpal's performance mode NONE put the stream on the
  primary low-latency output instead (480-frame periods, about 95
  callbacks/s), at 44.1 kHz into a 48 kHz mixer, so `audioserver` also
  resamples. Asking AAudio for PowerSaving routes music to deep buffer or
  offload, and resampling to 48 kHz in the core (0.11–0.13 % on this phone)
  removes the server-side resampler. Neither is reachable through cpal 0.18,
  which makes the `ndk` stream builder (or a cpal patch) a requirement, not an
  option.
- The observer and Media3 refresh (about 1 %) come from pushing full state
  every 500 ms. Push on change and let Media3 extrapolate the position.

**Xiaomi Mi 9T (Android 11, MIUI): 10 clean screen-off minutes, then a Media3
crash.** The soak ran with the screen off and forced deep idle: 0 underruns,
app 4.7–5.1 % of a core per minute (decode thread 3.2 %, callback 1.1 %),
`audioserver` 5.4–5.9 %, the same pattern as the Pixel 8. At minute 10 the
scripted `cmd media_session dispatch pause` **crashed the app**:
`IllegalArgumentException: packageName should be nonempty` inside Media3
1.8.0's legacy `onMediaButtonEvent`. On Android 11 a media button injected from
the shell has an empty caller package, and Media3 fails while building the
controller info (`spikes/b-audio/results/soak-mi9t-partial/`). The Pixel 8
(Android 17) took the same command fine. Real notification and headset
buttons carry a package name, so this is mainly the test tool, but a media
button from a system component without a package would kill playback the same
way. Production should check this against the current Media3 release, or guard
`onMediaButtonEvent`. The following "play" restarted the service and resumed
playback on its own. The run was not repeated to 30 minutes. A rerun without
the shell commands measured 30 minutes of *paused* idle by mistake, and that
cost 1.0 % of a core, from the spike's 20 ms decode polling and the 500 ms
state push. That confirms the wakeup fix below. MIUI also blocks `adb install`
and injected key events even with "Install via USB" on, so the APK went in
through the file manager and the screen was switched off by hand.

None of this threatened playback, but it is battery. The production target
should be the deep-buffer path with a decode wakeup every few hundred
milliseconds.

Findings that change implementation:

- **cpal needs `ndk-context`, which a UniFFI library does not get.** cpal asks
  Java's AudioManager for burst sizes, and JNA loads the library with `dlopen`,
  so no `JNI_OnLoad` runs. The first stream failed with "android context was not
  initialized". Fix, verified: Kotlin calls `System.loadLibrary` and then one
  JNI function that hands over the JavaVM and the application context
  (`player/src/android.rs`, 20 lines). Every crate built on `ndk-context` needs
  this.
- **cpal leaves AAudio's performance mode at NONE** (LowLatency only with its
  `realtime` feature) and offers no PowerSaving or usage/content type. On the
  emulator NONE gave about 43 callbacks/s. On the Pixel 8 it landed on the
  primary low-latency output at about 95 callbacks/s, and `audioserver` cost
  5.3 % (above). PowerSaving (deep buffer, offload) and `CONTENT_TYPE_MUSIC`
  need the `ndk` crate's `AudioStreamBuilder` directly or a cpal patch.
- Media keys and `cmd media_session dispatch` reach the session only after it
  has played once ("Media button session is null"). This is normal Android
  behavior, but playback has to start from the app UI.
- Not covered by the spike: audio focus and becoming-noisy handling. Media3's
  `SimpleBasePlayer` does not do them (ExoPlayer does). They are platform code
  around the core.

### 5. Decoder registry and platform fallback

`spikes/b-audio/lab/src/registry.rs` sketches the registry the host needs:

```rust
pub enum Support { No, Degraded, Full }                      // ordered: higher wins
pub struct StreamInfo { params: AudioCodecParameters,        // from the demuxer
                        profile_hint: Option<String> }       // HLS CODECS, Plugin hint
pub trait Decoder: Send {
    fn decode(&mut self, packet: &Packet, out: &mut Vec<f32>) -> Result<()>;
    fn output(&self) -> (u32, usize);   // rate, channels; may change after packet 1
    fn reset(&mut self);                // after a seek
}
pub trait DecoderProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn support(&self, stream: &StreamInfo) -> Support;
    fn create(&self, stream: &StreamInfo) -> Result<Box<dyn Decoder>>;
}
// DecoderRegistry::select: best Support wins, later registrations win ties,
// so platform decoders register after Symphonia.
```

Demuxing stays with Symphonia's readers and the host's TS demuxer. Providers
only decode packets. The contract also needs **timeline-aligned output** (see
gapless above) and **gapless trims applied by the decoder or the host,
consistently**. Today Symphonia's MP3 and Vorbis decoders apply packet trims,
the libopus adapter applies trims plus pre-skip, and nobody trims MP4.

**Does HE-AAC or Opus force a fallback? Yes, both, and it is not a quality
question: Symphonia has neither.**

- **HE-AAC.** Symphonia 0.6.1's AAC decoder is LC only. With explicit
  signalling (object type 5 or 29 in the AudioSpecificConfig) it refuses with
  "aac too complex". With implicit signalling (ADTS, and MP4s whose ASC says LC
  at 22.05 kHz, as ffmpeg remuxes them) it **silently decodes the core**: half
  the sample rate, no high band, and for HE-AAC v2 **mono**, because Parametric
  Stereo is dropped. No error is raised. This is the common case for live radio
  (BBC above). The registry routes AAC away from Symphonia when the HLS
  `CODECS` says `mp4a.40.5`/`40.29`, when the ASC says 5/29, or when the core
  rate is 24 kHz or less. The last rule also catches Apple's 22.05 kHz LC
  stream: a harmless false positive, because a full decoder handles LC.
  Fallback: Android MediaCodec (`c2.android.aac.decoder`, FDK-based, on every
  device), Apple AudioToolbox, Windows Media Foundation. **The MediaCodec
  provider was built and tested on the Mi 9T**: it decodes HE-AAC v1 and v2
  at the full rate and in stereo, and AAC album tracks through it are
  sample-exact with the host's edit-list trim. **Both were measured in
  isolation**, through `audiolab bench` and `audiolab gapless`, which call
  `finish()` at end of stream and use the decoder's reported output rate.
  The spike player never played a MediaCodec stream. Review found that it
  dropped the decoder's tail at track ends (now fixed: it flushes before
  advancing) and that it opens the output at the demuxer's rate, so
  implicit-SBR ADTS would play at half speed through it (not fixed, a host
  requirement in ADR 0004). Unlike the raw FDK adapter
  (+1685 frames), Codec2 compensates its own decoder delay, so its output lines
  up with Symphonia's frame for frame. Cost: section 1. The trait needed one
  addition for it: `finish()`, because MediaCodec holds several packets in
  flight and only releases them on end-of-stream. FDK itself cannot ship
  in the app: the Fraunhofer licence is not GPL-compatible (ADR 0009), and the
  spike used it only for comparison and to encode test files. Desktop Linux is
  not a target. CPU with FDK is 0.26–0.31 % of a desktop core.
- **Opus.** Symphonia has no Opus decoder (it is absent from `all-codecs`).
  libopus through `symphonia-adapter-libopus` is gapless, costs 0.15–0.17 %,
  and is BSD-3-Clause, so it is GPL-compatible. It builds with cmake (for
  Android, the VS-bundled cmake through `vswhere`). Opus matters for WebM
  sources and for SoundCloud's legacy `hls_opus_64` transcodings. Pure-Rust
  Opus crates exist (`opus-decoder`, `rusty-opus` and others) but were not
  evaluated. Recommendation: libopus as a registered provider on every
  platform. A platform fallback is not needed for Opus.

### Reuse from Ristle legacy

- Reused: the playlist parser and AES-128 handling from `provider-media`
  (extended as above), the decode-and-discard seek to an exact position, and
  the two-slot current/next idea from `audio-engine`, which here became one
  ring written continuously across tracks.
- Not reused: legacy's `LinearResampler` (aliases; rubato costs 0.03 %), the
  `VecDeque<f32>` ring with a mutex and per-sample `pop_front` (replaced by a
  lock-free SPSC ring), and the `LeadingSilenceTrimmer` (it guesses at missing
  priming and eats real silence at track starts; use `elst`/LAME data and a
  known 1024-frame AAC priming instead). Buffering whole HLS segments before
  decoding is the main seek-latency cost and was replaced by streaming decode.
  Re-probing and re-fetching the init segment on every seek were dropped (the
  init is cached).

### Design changes (carried into ADR 0004)

1. The decoder registry ships with **three providers**: Symphonia (default),
   **libopus** for Opus, and **platform AAC decoders for HE-AAC** (MediaCodec,
   AudioToolbox, Media Foundation). The selection rules above route implicit
   SBR away from Symphonia.
2. **The host demuxes MPEG-TS** (about 300–400 lines). A Plugin preference is
   not enough for live radio.
3. **The host applies MP4 edit lists** (`elst`, `iTunSMPB`) and the decoder
   contract requires timeline-aligned output.
4. **HLS decodes while downloading** and seeks with byte ranges into fMP4
   segments (built and measured: 391 ms on a 150 ms / 1.5 Mbit/s link).
5. Android output needs the **`ndk-context` JNI bootstrap** and **must use
   AAudio PowerSaving** (outside cpal's current API), with the core resampling
   to the output rate and waking its decoder on demand. On the Pixel 8 the
   defaults cost about 10 % of a core.

### Open items

- **The full 30-minute soak passed on the Pixel 8 and the emulator.** The Mi 9T
  gave 10 clean screen-off minutes before the Media3 media-button crash
  (section 4). That crash needs a check against current Media3 before phase 3.
- Platform AAC decoders on iOS (AudioToolbox) and Windows (Media Foundation)
  were not built. MediaCodec was.
- Byte-range seeks for AES-128 fMP4 and for TS/ADTS segments are not built.
  AAudio PowerSaving with resampling in the core and event-driven decode
  wakeups are untested. The Pixel 8 soak says they matter for battery
  (section 4).
