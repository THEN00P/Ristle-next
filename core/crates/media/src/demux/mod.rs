//! Demuxers: from container bytes to the selected audio track's packets.
//!
//! Symphonia's readers cover the progressive containers (MP4, Ogg, MKV/WebM,
//! FLAC, WAV, AIFF, CAF, ADTS, MP3), with the host applying MP4 edit lists
//! (`mp4`). The stream (`crate::stream`) pulls packets through [`Demuxer`]
//! and does not care which demuxer produced them.

pub(crate) mod es;
pub(crate) mod mp4;
mod symphonia;
pub(crate) mod ts;

pub(crate) use self::symphonia::SymphoniaDemuxer;
pub(crate) use self::ts::{TsDemuxer, TsFileDemuxer};

use crate::Error;
use crate::codec::CodecDescription;

/// A track's time base: one unit is `numer / denom` seconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TimeBase {
    pub(crate) numer: u32,
    pub(crate) denom: u32,
}

impl TimeBase {
    /// `ts` in frames at `rate`, rounded to the nearest frame.
    pub(crate) fn to_frames(self, ts: i64, rate: u32) -> i64 {
        let scaled = i128::from(ts) * i128::from(self.numer) * i128::from(rate);
        let denom = i128::from(self.denom);
        let rounded = if scaled >= 0 {
            (scaled + denom / 2) / denom
        } else {
            (scaled - denom / 2) / denom
        };
        i64::try_from(rounded).unwrap_or(if scaled >= 0 { i64::MAX } else { i64::MIN })
    }

    /// `frames` at `rate` in this time base, rounded down, so that a seek to
    /// the result lands at or before the frame.
    pub(crate) fn ts_at_or_before(self, frames: i64, rate: u32) -> i64 {
        let scaled = i128::from(frames) * i128::from(self.denom);
        let divisor = i128::from(self.numer) * i128::from(rate);
        i64::try_from(scaled.div_euclid(divisor)).unwrap_or(i64::MAX)
    }
}

/// One packet of the selected track, as the container describes it.
#[derive(Debug)]
pub(crate) struct DemuxedPacket {
    pub(crate) data: Box<[u8]>,
    /// Time of the first decoded frame, in the track's time base. Negative
    /// for encoder delay.
    pub(crate) pts: i64,
    /// The container's duration for the packet. Readers disagree on whether
    /// it includes the trims, so only decoders that want a hint read it.
    pub(crate) dur: u64,
    /// Decoded frames to drop from the start of this packet's output, in the
    /// track's time base.
    pub(crate) trim_start: u64,
    /// Decoded frames to drop from the end of this packet's output.
    pub(crate) trim_end: u64,
}

pub(crate) enum Demuxed {
    Packet(DemuxedPacket),
    /// The next packet's bytes have not arrived yet.
    Pending,
    End,
}

pub(crate) enum Seeked {
    /// Positioned at or before the requested time.
    At,
    /// The requested time is past the end of the track.
    PastEnd,
}

/// A presentation window the container declares (an MP4 edit list), in the
/// track's time base.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Window {
    /// Time of presentation zero: everything before it is encoder delay.
    pub(crate) start: i64,
    /// How long the presentation is, if the container says.
    pub(crate) length: Option<u64>,
}

pub(crate) trait Demuxer: Send {
    /// The selected track's codec.
    fn codec(&self) -> &CodecDescription;

    fn time_base(&self) -> TimeBase;

    /// The container's presentation window, when it has one.
    fn window(&self) -> Option<Window>;

    fn next_packet(&mut self) -> Result<Demuxed, Error>;

    /// Positions the demuxer at or before `ts` (track time base) so that the
    /// next packet's `pts` is at or before it.
    fn seek(&mut self, ts: i64) -> Result<Seeked, Error>;
}

impl Demuxer for Box<dyn Demuxer> {
    fn codec(&self) -> &CodecDescription {
        (**self).codec()
    }

    fn time_base(&self) -> TimeBase {
        (**self).time_base()
    }

    fn window(&self) -> Option<Window> {
        (**self).window()
    }

    fn next_packet(&mut self) -> Result<Demuxed, Error> {
        (**self).next_packet()
    }

    fn seek(&mut self, ts: i64) -> Result<Seeked, Error> {
        (**self).seek(ts)
    }
}

/// What a Stream Source says about its bytes before anyone reads them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaHint {
    /// A MIME type, possibly with a `codecs` parameter, as the WIT
    /// `stream-source.mime` carries it: `audio/ogg; codecs=vorbis`,
    /// `audio/mp4; codecs="mp4a.40.5"`.
    pub mime: Option<String>,
    /// A file extension without the dot, such as `m4a`.
    pub extension: Option<String>,
}

impl MediaHint {
    /// A hint from a MIME type.
    pub fn mime(mime: impl Into<String>) -> Self {
        Self {
            mime: Some(mime.into()),
            extension: None,
        }
    }

    /// A hint from a file extension.
    pub fn extension(extension: impl Into<String>) -> Self {
        Self {
            mime: None,
            extension: Some(extension.into()),
        }
    }

    /// The MIME type without parameters, lower case.
    pub(crate) fn essence(&self) -> Option<String> {
        let mime = self.mime.as_deref()?;
        let essence = mime.split(';').next()?.trim().to_ascii_lowercase();
        (!essence.is_empty()).then_some(essence)
    }

    /// The `codecs` parameter, unquoted.
    pub(crate) fn codecs(&self) -> Option<String> {
        let mime = self.mime.as_deref()?;
        mime.split(';').skip(1).find_map(|parameter| {
            let (name, value) = parameter.split_once('=')?;
            name.trim()
                .eq_ignore_ascii_case("codecs")
                .then(|| value.trim().trim_matches('"').to_owned())
                .filter(|codecs| !codecs.is_empty())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_base_conversions_round_to_the_nearest_frame_and_seek_down() {
        let millis = TimeBase {
            numer: 1,
            denom: 1000,
        };
        assert_eq!(millis.to_frames(-6, 48_000), -288);
        assert_eq!(millis.to_frames(1, 44_100), 44);
        assert_eq!(millis.ts_at_or_before(47, 48_000), 0);
        assert_eq!(millis.ts_at_or_before(-1, 48_000), -1);
        let samples = TimeBase {
            numer: 1,
            denom: 44_100,
        };
        assert_eq!(samples.to_frames(1024, 88_200), 2048);
        assert_eq!(samples.ts_at_or_before(2049, 88_200), 1024);
    }

    #[test]
    fn mime_hints_split_into_essence_and_codecs() {
        let hint = MediaHint::mime("Audio/MP4; codecs=\"mp4a.40.5\"");
        assert_eq!(hint.essence().as_deref(), Some("audio/mp4"));
        assert_eq!(hint.codecs().as_deref(), Some("mp4a.40.5"));
        let bare = MediaHint::mime("audio/ogg");
        assert_eq!(bare.codecs(), None);
        assert_eq!(MediaHint::extension("flac").essence(), None);
    }
}
