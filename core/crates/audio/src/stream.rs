//! The `MediaStream` contract the engine pulls PCM through.
//!
//! These are the shapes fixed in section 3 of `docs/plan/phase-2.md` and
//! agreed with workstream D (`ristle-media`) on 2026-10-19. `ristle-media`
//! will export the same items; when it does, this module becomes
//! `pub use ristle_media::{MediaError, MediaStream, Read, StreamFormat};`
//! and nothing else in the crate changes.

use std::error::Error as StdError;
use std::fmt;

/// The PCM layout a stream delivers: interleaved `f32` frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamFormat {
    /// Frames per second.
    pub sample_rate: u32,
    /// Samples per frame.
    pub channels: u16,
}

/// The outcome of one [`MediaStream::read`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Read {
    /// `n` frames (never 0) were written to the start of the buffer, in the
    /// format [`MediaStream::format`] reported before the read.
    Frames(usize),
    /// No data yet, and the stream has not ended (waiting on the network).
    Pending,
    /// The stream has ended and its decoder was flushed. Sticky until a seek.
    End,
}

/// Why a stream failed. `ristle-media` replaces this with its own `Error`;
/// the engine only uses `Display`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaError(String);

impl MediaError {
    /// An error with this message.
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for MediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl StdError for MediaError {}

/// Decoded, timeline-aligned PCM for one Track, pulled by the engine's
/// decode thread.
pub trait MediaStream: Send {
    /// The format of the frames the next read returns. It may change once
    /// (implicit SBR); a single read never mixes formats.
    fn format(&self) -> StreamFormat;
    /// Fills the start of `out` (a whole number of frames) with PCM.
    fn read(&mut self, out: &mut [f32]) -> Result<Read, MediaError>;
    /// Moves to a presentation time after priming; the next read starts there.
    fn seek(&mut self, position_ms: u64) -> Result<(), MediaError>;
}
