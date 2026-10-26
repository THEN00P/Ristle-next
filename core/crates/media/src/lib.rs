//! From a Stream Source to decoded, timeline-aligned PCM (ADR 0004).
//!
//! - **Sources:** local files and other seekable byte sources
//!   ([`ContainerStream`]); HLS ([`HlsStream`]); progressive HTTP with Range
//!   seeks and Plugin Byte Sources ([`ProgressiveStream`]), with host-side
//!   AES-CTR/CBC decryption ([`Decryption`]). HTTP goes through the
//!   [`Fetch`] port, Byte Sources through [`ByteSourceReader`].
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
mod fetch;
mod hls;
mod progressive;
mod source;
mod stream;

pub use codec::{Codec, CodecDescription, HeAac};
pub use container::ContainerStream;
#[cfg(target_os = "android")]
pub use decode::MediaCodecProvider;
pub use decode::{
    Decoder, DecoderProvider, DecoderRegistry, LibopusProvider, OpenedDecoder, Packet, Support,
    SymphoniaProvider,
};
pub use demux::MediaHint;
pub use error::{Error, MediaError};
pub use fetch::{ByteRange, Fetch, FetchBody, FetchError, FetchRequest};
pub use hls::{HlsSource, HlsStream};
pub use progressive::{Location, ProgressiveSource, ProgressiveStream};
pub use source::decrypt::{AesMode, Decryption};
pub use source::{ByteSourceHandle, ByteSourceReader, SourceError};
pub use stream::{MediaStream, Read, StreamFormat};
