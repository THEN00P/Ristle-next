# ADR 0004: The host owns decoding; Plugins supply bytes

## Status

Accepted (2026-10-08). Amended 2026-10-17 after spike B (see "Amendment"). The
reasoning and measurements are in `docs/research/audio-pipeline.md`.

## Context

iOS does not let third-party apps map writable memory as executable, so WASM can
only be interpreted there. Interpretation is roughly an order of magnitude or
more slower than native code (to be measured). Continuous audio decoding inside
a Plugin would therefore cost a large share of a core on iOS, in the background,
on battery.

Ristle legacy allowed a Plugin to decode to PCM itself (librespot) in addition to
handing over an HTTP source. Third-party streams also vary widely: progressive
files, HLS, chunked CDNs, encrypted chunks, and live streams.

## Decision

Plugins never decode. A Plugin resolves a Track into a **Stream Source**, and the
host fetches, demuxes, decodes, resamples and plays it. Stream Source variants:

1. **Progressive HTTP**: URL, request headers, optional codec/container hint.
   The host seeks with Range requests.
2. **HLS**: playlist URL and headers. The host selects variants, fetches
   segments, and handles standard HLS AES-128 key fetching.
3. **Plugin Byte Source**: the host asks the Plugin for byte ranges on demand.
   This covers chunked or lazily loaded CDNs (the librespot pattern) and
   anything else plain HTTP cannot express.
4. **Host-side decryption**: for a stream encrypted with a standard scheme
   (AES-CTR or AES-CBC), the Plugin supplies the key and IV, and the host
   decrypts while fetching. Prefer this over a Byte Source, which costs Plugin
   CPU.

The host keeps a decoder registry behind a `Decoder` trait. Symphonia is the
default. Platform decoders (MediaCodec, AudioToolbox, Media Foundation) can be
registered for formats where Symphonia is weak, such as HE-AAC if spike B finds
problems.

DRM-protected streams (Widevine, FairPlay, PlayReady) are out of scope. A Plugin
cannot deliver them, and services that only offer DRM streams cannot be played.

## Consequences

- Plugin CPU stays I/O-bound, so even interpreted WASM on iOS is acceptable. A
  Byte Source that decrypts with AES in the interpreter handles a
  320 kbps (~40 KB/s) stream with plenty of headroom (to be measured in spike B).
- Codec coverage is a host responsibility and grows centrally, not per Plugin.
- MPEG-TS is not demuxed by Symphonia. Decided in spike B: the host demuxes TS
  (see the amendment).

## Amendment (2026-10-17, spike B)

Spike B confirmed the split and changed the decoder set. Native decoding costs
0.05–0.3 % of a desktop core per stream and 0.4–2.8 % of a phone's slow core
(Pixel 8 A510, Mi 9T A55).

1. **The decoder registry ships with three providers, not one.** Symphonia
   0.6.1 has no HE-AAC (SBR/PS) and no Opus decoder. With implicit SBR it
   silently outputs the AAC core: half the sample rate, and mono for HE-AAC v2.
   - Symphonia: MP3, AAC-LC, Vorbis, FLAC, ALAC, PCM.
   - **libopus** (BSD-3-Clause) through a Symphonia adapter, on every platform.
   - **Platform AAC decoders for HE-AAC**: MediaCodec, AudioToolbox, Media
     Foundation. AAC goes to them when the HLS `CODECS` or a Plugin hint says
     `mp4a.40.5`/`40.29`, when the AudioSpecificConfig says object type 5 or
     29, or when the AAC core rate is 24 kHz or less. FDK AAC cannot ship: its
     licence is not GPL-compatible. The MediaCodec provider was built in the
     spike: correct HE-AAC v1/v2, timeline-aligned output, 2.5–2.9 % of a
     core (mostly in `media.swcodec`), about 20× Symphonia for AAC-LC, so it is
     used only where Symphonia cannot decode.
2. **Decoder contract.** A `Decoder` gets demuxed packets and returns
   interleaved f32 aligned to the input timeline, so decoder latency is the
   provider's problem (FDK's output trails Symphonia's by 1685 frames).
   Gapless trims are applied in one place per stream.
3. **The host applies MP4 edit lists** (`elst`, `iTunSMPB`). Symphonia parses
   them but does not apply them, so without this AAC albums are not gapless.
   MP3 (LAME), Ogg Vorbis/Opus and FLAC were sample-exact as they are.
4. **The host demuxes MPEG-TS.** Live radio HLS is TS (BBC, Radio France), and
   BBC's is HE-AAC, so a Plugin preference cannot avoid it. The spike demuxer is
   155 lines. A production one is estimated at 300–400 lines. Plugins still
   prefer fMP4 when offered, because it carries the edit list.
5. **HLS decodes segments while they download, and seeks with byte ranges
   inside a segment.** Buffering whole segments puts seeks at 840–890 ms on a
   150 ms / 1.5 Mbit/s link. Streaming decode brings it to 527 ms median.
   Byte-range seeks into fMP4 segments (read the `moof`, fetch from the target
   frame) were built and give 391 ms there, two round trips regardless of
   segment length. `EXT-X-BYTERANGE` playlists are supported.
6. **Android output:** a cpal stream needs a one-call JNI bootstrap that fills
   `ndk-context` (JNA loads the library without `JNI_OnLoad`). AAudio's
   PowerSaving mode and music content type are not reachable through cpal 0.18
   and need the `ndk` stream builder or a cpal patch. This is required, not
   optional: on a Pixel 8 cpal's default landed on the low-latency output
   (about 95 callbacks/s) and `audioserver` used 5.3 % of a core. The core
   resamples to the output rate itself and wakes the decoder on demand.

### Host requirements from the spike B review

Codex's review of the spike found these. Each one is a requirement for the
phase 2 host, whether or not the spike fixed it:

- **Media3 mirrors the core's queue.** The `SimpleBasePlayer` timeline is
  built from core state (queue items, current index, durations) and never from
  a Kotlin copy of the queue. The spike's static `ALBUM` list broke this
  (AGENTS.md ownership).
- **Seeks map logical time through the MP4 edit list.** A seek target is in
  presentation time after the priming trim, so the demuxer target and the
  decode-and-discard count include `elst` media_time.
- **AES-128 covers `EXT-X-MAP` init sections.** When an `EXT-X-KEY` precedes
  the map, the init section is decrypted too (IV from the tag, or the rule the
  spec gives for init sections).
- **Ranged fMP4 parsing filters by track ID.** Muxed audio/video variants put
  several `traf`s in one `moof`. Only the selected audio track's samples (by
  `tfhd` track_ID) are used. A `trun` without `data_offset` continues where
  the previous run ended (ISO/IEC 14496-12), not at the `traf` base. Apple's
  packager writes several runs per fragment.
- **Output follows the decoder's actual output rate.** Implicit SBR doubles
  the rate only after the first frames, and MediaCodec reports it through a
  format change. The core opens the output from the decoder's output, or
  resamples everything to the device rate (the plan anyway).
- **Decoders are flushed at end of stream before the next track starts.**
  MediaCodec holds several packets in flight; dropping it at EOF loses the
  tail and breaks gapless.
