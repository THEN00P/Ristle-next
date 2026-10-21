//! From a Stream Source to decoded, timeline-aligned PCM (ADR 0004).
//!
//! - **Sources:** local files and other seekable byte sources
//!   ([`ContainerStream`]). Progressive HTTP with Range seeks, HLS, host-side
//!   AES-CTR/CBC decryption and Plugin Byte Sources follow.
//! - **Demux:** the containers Symphonia reads (MP4, Ogg, MKV/WebM, FLAC,
//!   WAV, AIFF, CAF, ADTS, MP3), with the MP4 edit list of the selected audio
//!   track applied by the host; and MPEG-TS with ADTS or MPEG audio, which
//!   the host demuxes and frames itself.
//! - **Decode:** the [`Decoder`] contract and the [`DecoderRegistry`].
//!   Providers: Symphonia (default), libopus, and platform AAC decoders for
//!   HE-AAC, which the platform glue registers. Decoders return interleaved
//!   f32 aligned to the input timeline, never trim, and are flushed at end of
//!   stream.
//! - **Gapless:** encoder delay, padding, edit lists and Opus pre-skip are
//!   applied in one place per stream, which also makes seeks exact in
//!   presentation time.
//!
//! The output is a [`MediaStream`] that `ristle-audio` pulls from. Milestone
//! M6a (`docs/plan/phase-2.md`).

mod codec;
mod container;
mod decode;
mod demux;
mod error;
mod stream;

pub use codec::{Codec, CodecDescription, HeAac};
pub use container::ContainerStream;
pub use decode::{
    Decoder, DecoderProvider, DecoderRegistry, LibopusProvider, OpenedDecoder, Packet, Support,
    SymphoniaProvider,
};
pub use demux::MediaHint;
pub use error::{Error, MediaError};
pub use stream::{MediaStream, Read, StreamFormat};
