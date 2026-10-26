//! Probing real files: one tagged file per format the host plays, files
//! without tags, and long files whose audio must not be read.

// clippy.toml exempts `#[test]` functions only; the helpers are test code.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::cell::RefCell;
use std::path::Path;

use ristle_local_files::{Date, Format, Metadata, ProbeError, RandomAccess, SourceError, probe};

/// A fixture read through `RandomAccess`, counting the bytes fetched.
struct Fixture {
    bytes: Vec<u8>,
    /// Every `(offset, length)` returned.
    reads: RefCell<Vec<(u64, u64)>>,
}

impl Fixture {
    fn open(name: &str) -> Self {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        Self {
            bytes: std::fs::read(path).unwrap(),
            reads: RefCell::default(),
        }
    }
}

impl RandomAccess for Fixture {
    fn size(&self) -> u64 {
        self.bytes.len() as u64
    }
    fn read_at(&self, offset: u64, length: u32) -> Result<Vec<u8>, SourceError> {
        let bytes = self.bytes.as_slice().read_at(offset, length)?;
        self.reads.borrow_mut().push((offset, bytes.len() as u64));
        Ok(bytes)
    }
}

fn read(name: &str) -> Metadata {
    probe(&Fixture::open(name), name).unwrap_or_else(|error| panic!("{name}: {error}"))
}

fn assert_tagged(name: &str, format: Format, isrc: bool) {
    let metadata = read(name);
    assert_eq!(metadata.format, format, "{name}");
    assert_eq!(metadata.title.as_deref(), Some("First Light"), "{name}");
    assert_eq!(metadata.artists, ["Ada Example"], "{name}");
    assert_eq!(
        metadata.album.as_deref(),
        Some("Synthetic Mornings"),
        "{name}"
    );
    assert_eq!(metadata.album_artists, ["The Examples"], "{name}");
    assert_eq!(metadata.track_number, Some(3), "{name}");
    assert_eq!(metadata.disc_number, Some(2), "{name}");
    assert_eq!(
        metadata.released,
        Some(Date {
            year: 2019,
            month: Some(3),
            day: Some(14)
        }),
        "{name}"
    );
    assert_eq!(metadata.genres, ["Ambient"], "{name}");
    assert_eq!(
        metadata.isrc.as_deref(),
        isrc.then_some("GBAYE0601498"),
        "{name}"
    );
    let duration = metadata.duration_ms.unwrap();
    assert!((990..=1060).contains(&duration), "{name}: {duration} ms");
    assert!(metadata.bitrate_kbps.is_some_and(|kbps| kbps > 0), "{name}");
}

#[test]
fn reads_tags_and_properties_of_every_format() {
    assert_tagged("tagged.flac", Format::Flac, true);
    assert_tagged("tagged.mp3", Format::Mp3, true);
    assert_tagged("tagged.m4a", Format::Aac, false);
    assert_tagged("tagged-alac.m4a", Format::Alac, false);
    assert_tagged("tagged.ogg", Format::Vorbis, true);
    assert_tagged("tagged.opus", Format::Opus, true);

    let wav = read("tagged.wav");
    assert_eq!(wav.format, Format::Wav);
    assert_eq!(wav.title.as_deref(), Some("Plain Wave"));
    assert_eq!(wav.duration_ms, Some(500));
    // 44.1 kHz, 16-bit mono.
    assert_eq!(wav.bitrate_kbps, Some(706));
}

#[test]
fn keeps_the_precision_of_a_date() {
    let metadata = read("year-only.flac");
    assert_eq!(
        metadata.released,
        Some(Date {
            year: 1997,
            month: None,
            day: None
        })
    );
    assert_eq!(metadata.track_number, Some(7));
    assert_eq!(metadata.disc_number, None);
}

#[test]
fn a_file_without_tags_has_only_properties() {
    let metadata = read("untagged.mp3");
    assert_eq!(metadata.format, Format::Mp3);
    assert_eq!(metadata.title, None);
    assert!(metadata.artists.is_empty());
    assert!(metadata.duration_ms.is_some());
}

/// Walkthrough: tags and headers come from near the start and the end; the
/// audio in between is never read through the Plugin.
#[test]
fn does_not_read_the_audio_of_long_files() {
    for name in ["long.mp3", "long.m4a"] {
        let fixture = Fixture::open(name);
        let metadata = probe(&fixture, name).unwrap();
        assert_eq!(metadata.title.as_deref(), Some("Long Walk"));
        assert!(
            metadata
                .duration_ms
                .is_some_and(|ms| (39_900..=40_200).contains(&ms))
        );
        // The middle of the file is audio, and tags never sit there.
        let middle = fixture.size() / 2;
        let reads = fixture.reads.borrow();
        assert!(
            reads
                .iter()
                .all(|(offset, length)| !(*offset..offset + length).contains(&middle)),
            "{name}: read the middle of the file: {reads:?}"
        );
    }
}

#[test]
fn text_with_an_audio_extension_is_not_audio() {
    assert_eq!(
        probe(&Fixture::open("fake.mp3"), "fake.mp3"),
        Err(ProbeError::Unsupported)
    );
}

struct Revoked;

impl RandomAccess for Revoked {
    fn size(&self) -> u64 {
        1000
    }
    fn read_at(&self, _: u64, _: u32) -> Result<Vec<u8>, SourceError> {
        Err(SourceError::Revoked)
    }
}

#[test]
fn a_revoked_folder_is_reported_as_such() {
    assert_eq!(
        probe(&Revoked, "a.flac"),
        Err(ProbeError::Source(SourceError::Revoked))
    );
}
