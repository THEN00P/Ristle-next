//! The crate's one error type.

/// Everything that can go wrong between a Stream Source and PCM.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The bytes are not a container the host reads.
    #[error("unrecognized container{}", hint_suffix(.hint))]
    UnrecognizedContainer {
        /// The MIME type or extension the source declared, if any.
        hint: Option<String>,
    },
    /// The container holds no audio track the host can play.
    #[error("no playable audio track")]
    NoAudioTrack,
    /// No registered decoder supports the codec.
    #[error("no decoder for {0}")]
    UnsupportedCodec(String),
    /// The container or bitstream is malformed beyond recovery.
    #[error("malformed {what}: {detail}")]
    Malformed {
        /// What was being read, such as "mp4 edit list".
        what: &'static str,
        /// What was wrong with it.
        detail: String,
    },
    /// A decoder failed in a way that ends the stream.
    #[error("{decoder} decoder failed: {detail}")]
    Decoder {
        /// The decoder provider's name.
        decoder: &'static str,
        /// The provider's description of the failure.
        detail: String,
    },
    /// A decoder could not decode one packet. Returned by
    /// [`Decoder::decode`](crate::Decoder::decode) only: the stream drops the
    /// packet and continues.
    #[error("{decoder} dropped a corrupt packet: {detail}")]
    CorruptPacket {
        /// The decoder provider's name.
        decoder: &'static str,
        /// The provider's description of the problem.
        detail: String,
    },
    /// The stream cannot seek (a live stream, or a source without random access).
    #[error("the stream cannot seek")]
    NotSeekable,
    /// [`MediaStream::read`](crate::MediaStream::read) was given a buffer
    /// shorter than one frame.
    #[error("the output buffer holds less than one frame of {channels} channels")]
    OutputTooSmall {
        /// The stream's channel count.
        channels: u16,
    },
    /// Reading the source failed.
    #[error("reading the source failed: {0}")]
    Io(#[from] std::io::Error),
}

/// The error type of [`MediaStream`](crate::MediaStream), as section 3 of the
/// phase 2 plan names it.
pub type MediaError = Error;

fn hint_suffix(hint: &Option<String>) -> String {
    hint.as_deref()
        .map(|h| format!(" ({h})"))
        .unwrap_or_default()
}

impl Error {
    pub(crate) fn malformed(what: &'static str, detail: impl Into<String>) -> Self {
        Self::Malformed {
            what,
            detail: detail.into(),
        }
    }
}
