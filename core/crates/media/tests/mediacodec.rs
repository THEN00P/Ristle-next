//! MediaCodec against the decoder contract, on an Android device (ADR 0004:
//! "each platform decoder needs a gapless test"). No HE-AAC encoder is
//! GPL-usable, so the provider is forced onto the AAC-LC album: lengths,
//! start alignment and seeks must match the source exactly, as they do
//! through Symphonia.
//!
//! Run on a device or emulator: build with
//! `cargo ndk -t x86_64 --platform 28 test -p ristle-media --test mediacodec --no-run`,
//! push the binary and `tests/fixtures` to `/data/local/tmp/`, and run it with
//! `RISTLE_MEDIA_FIXTURES=/data/local/tmp/fixtures`.

#![cfg(target_os = "android")]
// Test helpers outside #[test] functions are not covered by clippy.toml.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::sync::Arc;

use common::{CUTS_44, best_lag, fixture, left, read_to_end, reference, snr};
use ristle_media::{ContainerStream, DecoderRegistry, MediaCodecProvider, MediaHint, MediaStream};

fn open(name: &str) -> ContainerStream {
    let mut registry = DecoderRegistry::with_defaults();
    registry.register(Arc::new(MediaCodecProvider::for_all_aac()));
    let stream =
        ContainerStream::open_file(&fixture(name), &MediaHint::default(), &registry).unwrap();
    assert_eq!(stream.decoder(), "mediacodec", "{name}");
    stream
}

#[test]
fn mediacodec_aac_is_sample_exact_with_the_edit_list() {
    let mut album = Vec::new();
    for track in 0..3 {
        let name = format!("album-{track}.m4a");
        let (format, samples) = read_to_end(&mut open(&name));
        assert_eq!((format.sample_rate, format.channels), (44_100, 2), "{name}");
        let expected = CUTS_44[track + 1] - CUTS_44[track];
        assert_eq!(samples.len() / 2, expected, "{name}: decoded length");
        let source = left(&reference(44_100, CUTS_44[track]..CUTS_44[track + 1]), 2);
        assert_eq!(
            best_lag(&left(&samples, 2), &source, 2048, 8192, 3000),
            0,
            "{name}: start"
        );
        album.extend(samples);
    }
    let ratio = snr(&reference(44_100, 0..CUTS_44[3]), &album);
    assert!(ratio > 20.0, "album SNR {ratio:.1} dB");
}

#[test]
fn mediacodec_seeks_start_exactly_at_the_target() {
    let mut stream = open("album-1.m4a");
    for position_ms in [700, 250, 1000] {
        stream.seek(position_ms).unwrap();
        let (_, samples) = read_to_end(&mut stream);
        let target = position_ms as usize * 44_100 / 1000;
        assert_eq!(
            samples.len() / 2,
            CUTS_44[2] - CUTS_44[1] - target,
            "at {position_ms} ms"
        );
        let source = left(&reference(44_100, CUTS_44[1] + target..CUTS_44[2]), 2);
        assert_eq!(
            best_lag(&left(&samples, 2), &source, 1024, 4096, 2000),
            0,
            "at {position_ms} ms"
        );
    }
}

#[test]
fn mediacodec_decodes_adts_in_mpeg_ts() {
    // TS signals no encoder delay: the 1024 priming frames stay, as through
    // Symphonia.
    let (_, samples) = read_to_end(&mut open("track-aac.ts"));
    let source = left(&reference(44_100, CUTS_44[1]..CUTS_44[2]), 2);
    assert_eq!(
        best_lag(&left(&samples, 2), &source, 2048, 8192, 3000),
        1024
    );
}
