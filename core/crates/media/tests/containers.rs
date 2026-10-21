//! Every container the host reads through Symphonia, the MediaStream
//! contract, and the edit list of a muxed MP4.

// Test helpers outside #[test] functions are not covered by clippy.toml.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{
    CUTS_44, CUTS_48, best_lag, left, open, read_to_end, read_to_end_with, reference, snr,
};
use ristle_media::{
    ContainerStream, DecoderRegistry, Error, MediaHint, MediaStream, Read, StreamFormat,
};

const TRACK_1: std::ops::Range<usize> = CUTS_44[1]..CUTS_44[2];
const TRACK_1_FRAMES: usize = CUTS_44[2] - CUTS_44[1];

fn assert_starts_on_time(name: &str, samples: &[f32], rate: u32, range: std::ops::Range<usize>) {
    let source = left(&reference(rate, range), 2);
    assert_eq!(
        best_lag(&left(samples, 2), &source, 2048, 8192, 3000),
        0,
        "{name}: start offset"
    );
}

#[test]
fn wav_decodes_bit_exact() {
    let (format, samples) = read_to_end(&mut open("track.wav"));
    assert_eq!(
        format,
        StreamFormat {
            sample_rate: 44_100,
            channels: 2
        }
    );
    assert_eq!(samples.len() / 2, 44_100);
    let source = reference(44_100, CUTS_44[1]..CUTS_44[1] + 44_100);
    assert!(snr(&source, &samples) > 85.0);
}

#[test]
fn flac_in_matroska_decodes_bit_exact() {
    let (_, samples) = read_to_end(&mut open("track.mka"));
    assert_eq!(samples.len() / 2, TRACK_1_FRAMES);
    assert!(snr(&reference(44_100, TRACK_1), &samples) > 85.0);
}

#[test]
fn vorbis_in_webm_starts_on_time() {
    // Matroska does not signal the Vorbis priming packet; the stream drops
    // it anyway. The end padding is not signalled either, so up to one block
    // of it remains.
    let (_, samples) = read_to_end(&mut open("track-vorbis.webm"));
    assert_starts_on_time("track-vorbis.webm", &samples, 44_100, TRACK_1);
    let frames = samples.len() / 2;
    assert!(
        (TRACK_1_FRAMES..=TRACK_1_FRAMES + 1024).contains(&frames),
        "{frames}"
    );
}

#[test]
fn opus_in_webm_applies_the_pre_skip_once() {
    let mut stream = open("track-opus.webm");
    assert_eq!(stream.decoder(), "libopus");
    let (format, samples) = read_to_end(&mut stream);
    assert_eq!(format.sample_rate, 48_000);
    assert_starts_on_time("track-opus.webm", &samples, 48_000, CUTS_48[1]..CUTS_48[2]);
    // Matroska's DiscardPadding is not read, so the last packet's padding
    // (under 20 ms) remains.
    let frames = samples.len() / 2;
    let expected = CUTS_48[2] - CUTS_48[1];
    assert!((expected..=expected + 960).contains(&frames), "{frames}");
}

#[test]
fn adts_decodes_with_the_priming_it_cannot_signal() {
    // ADTS has no encoder-delay field: the 1024 priming frames stay.
    let (format, samples) = read_to_end(&mut open("track.aac"));
    assert_eq!(
        format,
        StreamFormat {
            sample_rate: 44_100,
            channels: 2
        }
    );
    let source = left(&reference(44_100, TRACK_1), 2);
    assert_eq!(
        best_lag(&left(&samples, 2), &source, 2048, 8192, 3000),
        1024
    );
    assert!(samples.len() / 2 >= TRACK_1_FRAMES + 1024);
}

#[test]
fn a_muxed_mp4_uses_the_edit_list_of_its_audio_track() {
    // Track 1 is video with media_time 100 at 1 kHz; track 2 is the audio.
    let (_, samples) = read_to_end(&mut open("muxed.mp4"));
    assert_eq!(samples.len() / 2, TRACK_1_FRAMES);
    assert_starts_on_time("muxed.mp4", &samples, 44_100, TRACK_1);
}

#[test]
fn any_buffer_size_reads_the_same_frames() {
    let (_, whole) = read_to_end_with(&mut open("album-1.m4a"), 4096);
    let (_, framewise) = read_to_end_with(&mut open("album-1.m4a"), 1);
    assert_eq!(whole, framewise);
}

#[test]
fn a_buffer_shorter_than_a_frame_is_an_error() {
    let mut stream = open("album-1.flac");
    let mut one_sample = [0f32; 1];
    assert!(matches!(
        stream.read(&mut one_sample),
        Err(Error::OutputTooSmall { channels: 2 })
    ));
    // The stream is still usable.
    let mut frame = [0f32; 2];
    assert_eq!(stream.read(&mut frame).unwrap(), Read::Frames(1));
}

#[test]
fn in_memory_sources_decode_like_files() {
    let bytes = std::fs::read(common::fixture("album-1.ogg")).unwrap();
    let registry = DecoderRegistry::with_defaults();
    let mut stream = ContainerStream::open_reader(
        std::io::Cursor::new(bytes),
        &MediaHint::mime("audio/ogg"),
        &registry,
    )
    .unwrap();
    let (_, from_memory) = read_to_end(&mut stream);
    let (_, from_file) = read_to_end(&mut open("album-1.ogg"));
    assert_eq!(from_memory, from_file);
}

#[test]
fn bytes_that_are_no_container_are_rejected() {
    let registry = DecoderRegistry::with_defaults();
    let garbage = std::io::Cursor::new(vec![0x5au8; 4096]);
    let error = ContainerStream::open_reader(garbage, &MediaHint::mime("audio/mpeg"), &registry)
        .unwrap_err();
    assert!(
        matches!(error, Error::UnrecognizedContainer { .. }),
        "{error}"
    );
}

#[test]
fn a_registry_without_opus_cannot_open_opus() {
    let mut registry = DecoderRegistry::empty();
    registry.register(std::sync::Arc::new(ristle_media::SymphoniaProvider::new()));
    let error = ContainerStream::open_file(
        &common::fixture("album-1.opus"),
        &MediaHint::default(),
        &registry,
    )
    .unwrap_err();
    assert!(matches!(error, Error::UnsupportedCodec(_)), "{error}");
}
