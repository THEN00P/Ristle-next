# ADR 0004: The host owns decoding; Plugins supply bytes

## Status

Accepted (2026-10-08). The codec coverage is to be confirmed by spike B. The
reasoning is in `docs/research/audio-pipeline.md`.

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
- MPEG-TS is not demuxed by Symphonia. HLS over TS needs a small TS demuxer in
  the host, or Plugins have to prefer fMP4/ADTS renditions. Decided in spike B.
