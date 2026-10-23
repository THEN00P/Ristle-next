//! What a file holds: its format, its tags and its stream properties, read
//! from the bytes near its start and end (walkthrough, "Local files").
//!
//! The Plugin never decodes audio (ADR 0004). It reports what the container
//! says; the host fingerprints, groups and plays.

use std::io::{Seek, SeekFrom};

use lofty::config::ParseOptions;
use lofty::file::{AudioFile, FileType, TaggedFile, TaggedFileExt};
use lofty::mp4::{Mp4Codec, Mp4File};
use lofty::mpeg::{Layer, MpegFile};
use lofty::probe::Probe;
use lofty::tag::{Accessor, ItemKey, Tag};

use crate::source::{RandomAccess, Reader, SourceError};

/// The most a probe reads from one file. Tags, stream headers and an MP4
/// `moov` box fit easily; a file needing more is reported as malformed
/// rather than read whole.
const READ_BUDGET: u64 = 16 << 20;

/// An audio format the host decodes. The string is `local-file.format`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Format {
    /// FLAC.
    Flac,
    /// Apple Lossless in MP4.
    Alac,
    /// Uncompressed PCM in WAV.
    Wav,
    /// MPEG-1/2 Layer III.
    Mp3,
    /// AAC, in MP4 or ADTS.
    Aac,
    /// Opus in Ogg.
    Opus,
    /// Vorbis in Ogg.
    Vorbis,
}

impl Format {
    /// The name the WIT's `local-file.format` uses.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Flac => "flac",
            Self::Alac => "alac",
            Self::Wav => "wav",
            Self::Mp3 => "mp3",
            Self::Aac => "aac",
            Self::Opus => "opus",
            Self::Vorbis => "vorbis",
        }
    }

    /// Lossless formats, which the core prefers among duplicates.
    pub fn is_lossless(self) -> bool {
        matches!(self, Self::Flac | Self::Alac | Self::Wav)
    }
}

/// A Media Date with the precision the tag gave.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Date {
    /// The year.
    pub year: u16,
    /// The month, 1 to 12.
    pub month: Option<u8>,
    /// The day, only with a month.
    pub day: Option<u8>,
}

/// What one audio file says about itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Metadata {
    /// The format.
    pub format: Format,
    /// The title tag.
    pub title: Option<String>,
    /// Track artists, in tag order, without duplicates.
    pub artists: Vec<String>,
    /// The album tag.
    pub album: Option<String>,
    /// Album artists, in tag order, without duplicates.
    pub album_artists: Vec<String>,
    /// The track number within its disc.
    pub track_number: Option<u32>,
    /// The disc number.
    pub disc_number: Option<u32>,
    /// The release or recording date.
    pub released: Option<Date>,
    /// The ISRC, upper-case without dashes.
    pub isrc: Option<String>,
    /// Genres, in tag order.
    pub genres: Vec<String>,
    /// The duration, when the container says.
    pub duration_ms: Option<u64>,
    /// The audio bitrate in kbit/s.
    pub bitrate_kbps: Option<u32>,
}

/// Why a file was not read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProbeError {
    /// Not audio, or a format the host does not play.
    #[error("not a supported audio file")]
    Unsupported,
    /// Audio whose container could not be parsed.
    #[error("malformed: {0}")]
    Malformed(String),
    /// The bytes could not be read.
    #[error(transparent)]
    Source(#[from] SourceError),
}

/// Reads a file's format, tags and properties. `name` is its file name; the
/// extension is used only when the bytes do not identify the format.
pub fn probe<S: RandomAccess + ?Sized>(source: &S, name: &str) -> Result<Metadata, ProbeError> {
    let mut reader = Reader::new(source, READ_BUDGET);
    let result = read(&mut reader, name);
    // A failed read surfaces as a parse error; report the cause instead.
    match reader.take_error() {
        Some(error) if result.is_err() => Err(ProbeError::Source(error)),
        _ => result,
    }
}

fn options() -> ParseOptions {
    ParseOptions::new().read_cover_art(false)
}

fn malformed(error: impl std::fmt::Display) -> ProbeError {
    ProbeError::Malformed(error.to_string())
}

fn read<S: RandomAccess + ?Sized>(
    reader: &mut Reader<'_, S>,
    name: &str,
) -> Result<Metadata, ProbeError> {
    let guessed = Probe::new(&mut *reader)
        .guess_file_type()
        .map_err(malformed)?
        .file_type();
    let file_type = guessed
        .or_else(|| file_type_from_extension(name))
        .ok_or(ProbeError::Unsupported)?;
    // Bytes that only the extension calls audio and that do not parse as
    // it are not audio, rather than broken audio.
    let unrecognized = |error: ProbeError| match (guessed, error) {
        (None, ProbeError::Malformed(_)) => ProbeError::Unsupported,
        (_, error) => error,
    };
    reader.seek(SeekFrom::Start(0)).map_err(malformed)?;
    // MPEG and MP4 are read as their own types, whose properties name the
    // layer and the codec; the rest through the generic probe.
    let (format, file): (Format, TaggedFile) = match file_type {
        FileType::Mpeg => {
            let file = MpegFile::read_from(reader, options())
                .map_err(malformed)
                .map_err(unrecognized)?;
            if *file.properties().layer() != Layer::Layer3 {
                return Err(ProbeError::Unsupported);
            }
            (Format::Mp3, file.into())
        }
        FileType::Mp4 => {
            let file = Mp4File::read_from(reader, options()).map_err(malformed)?;
            let format = match file.properties().codec() {
                Some(Mp4Codec::AAC) => Format::Aac,
                Some(Mp4Codec::ALAC) => Format::Alac,
                Some(Mp4Codec::MP3) => Format::Mp3,
                Some(Mp4Codec::FLAC) => Format::Flac,
                // Mp4Codec is non-exhaustive.
                _ => return Err(ProbeError::Unsupported),
            };
            (format, file.into())
        }
        FileType::Flac | FileType::Wav | FileType::Aac | FileType::Opus | FileType::Vorbis => {
            let format = match file_type {
                FileType::Flac => Format::Flac,
                FileType::Wav => Format::Wav,
                FileType::Aac => Format::Aac,
                FileType::Opus => Format::Opus,
                _ => Format::Vorbis,
            };
            let file = Probe::new(&mut *reader)
                .options(options())
                .set_file_type(file_type)
                .read()
                .map_err(malformed)
                .map_err(unrecognized)?;
            (format, file)
        }
        _ => return Err(ProbeError::Unsupported),
    };
    Ok(metadata(format, &file))
}

/// Only for files whose first bytes do not say what they are.
fn file_type_from_extension(name: &str) -> Option<FileType> {
    let (_, extension) = name.rsplit_once('.')?;
    match extension.to_ascii_lowercase().as_str() {
        "mp3" => Some(FileType::Mpeg),
        "aac" => Some(FileType::Aac),
        _ => None,
    }
}

fn metadata(format: Format, file: &TaggedFile) -> Metadata {
    // The primary tag first (ID3v2 before ID3v1, for example); later tags
    // only fill what earlier ones lack.
    let mut tags: Vec<&Tag> = file.primary_tag().into_iter().collect();
    for tag in file.tags() {
        if !tags.iter().any(|seen| std::ptr::eq(*seen, tag)) {
            tags.push(tag);
        }
    }

    let first = |get: &dyn Fn(&Tag) -> Option<String>| tags.iter().find_map(|tag| get(tag));
    let all = |key: ItemKey| -> Vec<String> {
        tags.iter()
            .map(|tag| distinct(tag.get_strings(key)))
            .find(|values| !values.is_empty())
            .unwrap_or_default()
    };

    let mut artists = all(ItemKey::TrackArtists);
    if artists.is_empty() {
        artists = all(ItemKey::TrackArtist);
    }
    let properties = file.properties();
    let duration_ms = u64::try_from(properties.duration().as_millis()).unwrap_or(u64::MAX);

    Metadata {
        format,
        title: first(&|tag| text(tag.title().as_deref())),
        artists,
        album: first(&|tag| text(tag.album().as_deref())),
        album_artists: all(ItemKey::AlbumArtist),
        track_number: tags.iter().find_map(|tag| tag.track()).filter(|n| *n > 0),
        disc_number: tags.iter().find_map(|tag| tag.disk()).filter(|n| *n > 0),
        released: [
            ItemKey::ReleaseDate,
            ItemKey::RecordingDate,
            ItemKey::Year,
            ItemKey::OriginalReleaseDate,
        ]
        .into_iter()
        .find_map(|key| tags.iter().find_map(|tag| parse_date(tag.get_string(key)?))),
        isrc: tags
            .iter()
            .find_map(|tag| normalize_isrc(tag.get_string(ItemKey::Isrc)?)),
        genres: all(ItemKey::Genre),
        duration_ms: (duration_ms > 0).then_some(duration_ms),
        bitrate_kbps: properties
            .audio_bitrate()
            .or(properties.overall_bitrate())
            .filter(|kbps| *kbps > 0),
    }
}

fn text(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn distinct<'a>(values: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for value in values {
        let value = value.trim();
        if !value.is_empty() && !out.iter().any(|seen| seen == value) {
            out.push(value.to_owned());
        }
    }
    out
}

/// `YYYY`, `YYYY-MM` or `YYYY-MM-DD`, optionally followed by a time.
fn parse_date(value: &str) -> Option<Date> {
    let date = value.trim().split(['T', ' ']).next()?;
    let mut parts = date.split('-');
    let year = parts.next().filter(|y| y.len() == 4)?.parse::<u16>().ok()?;
    if year == 0 {
        return None;
    }
    let month = match parts.next() {
        Some(m) => Some(m.parse::<u8>().ok().filter(|m| (1..=12).contains(m))?),
        None => None,
    };
    let day = match parts.next() {
        Some(d) if month.is_some() => Some(d.parse::<u8>().ok().filter(|d| (1..=31).contains(d))?),
        Some(_) => return None,
        None => None,
    };
    if parts.next().is_some() {
        return None;
    }
    Some(Date { year, month, day })
}

/// An ISRC is two letters, three alphanumerics and seven digits.
fn normalize_isrc(value: &str) -> Option<String> {
    let isrc: String = value
        .chars()
        .filter(|c| *c != '-' && !c.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase();
    let bytes = isrc.as_bytes();
    let valid = bytes.len() == 12
        && bytes[..2].iter().all(u8::is_ascii_alphabetic)
        && bytes[2..5].iter().all(u8::is_ascii_alphanumeric)
        && bytes[5..].iter().all(u8::is_ascii_digit);
    valid.then_some(isrc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dates_with_their_precision() {
        let date = |year, month, day| Some(Date { year, month, day });
        assert_eq!(parse_date("1997"), date(1997, None, None));
        assert_eq!(parse_date("1997-08"), date(1997, Some(8), None));
        assert_eq!(parse_date("1997-08-31"), date(1997, Some(8), Some(31)));
        assert_eq!(
            parse_date("1997-08-31T12:00:00"),
            date(1997, Some(8), Some(31))
        );
        assert_eq!(
            parse_date(" 2001-02-03 10:00 "),
            date(2001, Some(2), Some(3))
        );
        for bad in [
            "",
            "97",
            "1997-13",
            "1997-08-32",
            "1997-8-1-1",
            "abcd",
            "0000",
        ] {
            assert_eq!(parse_date(bad), None, "{bad}");
        }
    }

    #[test]
    fn normalizes_isrcs() {
        assert_eq!(
            normalize_isrc("us-rc1-76-07839"),
            Some("USRC17607839".into())
        );
        assert_eq!(normalize_isrc("GBAYE0601498"), Some("GBAYE0601498".into()));
        assert_eq!(normalize_isrc("USRC1760783"), None);
        assert_eq!(normalize_isrc("12RC17607839"), None);
    }

    #[test]
    fn keeps_distinct_values_in_order() {
        let values = ["B", " A ", "B", "", "C"];
        assert_eq!(distinct(values.into_iter()), ["B", "A", "C"]);
    }

    #[test]
    fn rejects_bytes_that_are_not_audio() {
        let bytes = b"not audio at all, just some text".repeat(100);
        assert_eq!(
            probe(bytes.as_slice(), "notes.txt"),
            Err(ProbeError::Unsupported)
        );
    }
}
