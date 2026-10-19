//! From a Stream Source to decoded, timeline-aligned PCM (ADR 0004).
//!
//! - **Sources:** progressive HTTP with Range seeks, HLS (variant choice,
//!   streaming decode while a segment downloads, byte-range seeks inside fMP4
//!   segments, `EXT-X-BYTERANGE`, AES-128 including `EXT-X-MAP` init
//!   sections), host-side AES-CTR/CBC decryption, Plugin Byte Sources through
//!   the `ByteSourceReader` port, and local files.
//! - **Demux:** MP4 and fMP4 (audio track chosen first, its own edit list
//!   applied, `traf` filtered by track ID, `trun` continuation), MPEG-TS,
//!   ADTS, and the containers Symphonia reads.
//! - **Decode:** the `Decoder` trait and registry. Providers: Symphonia
//!   (default), libopus, and platform AAC for HE-AAC (MediaCodec first).
//!   Decoders return interleaved f32 aligned to the input timeline and are
//!   flushed at end of stream.
//!
//! The output is a `MediaStream` that `ristle-audio` pulls from. Milestone M6
//! (`docs/plan/phase-2.md`).
