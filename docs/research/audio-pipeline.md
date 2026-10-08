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
| HE-AAC, HE-AACv2, Opus, WavPack | Not rated (present, maturity unknown) |

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
