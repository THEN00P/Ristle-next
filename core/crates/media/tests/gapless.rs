//! Sample-exact gapless playback (M6a exit): three album tracks cut from one
//! signal at non-frame-aligned points, encoded separately, decoded back to
//! back and compared with the signal they were cut from.

// Test helpers outside #[test] functions are not covered by clippy.toml.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{CUTS_44, CUTS_48, best_lag, left, open, read_to_end, reference, snr};

/// Decodes `album-{0,1,2}.{extension}` and checks each track's length and
/// start against the source, then the whole album's error, including 10 ms
/// windows straddling each track boundary.
fn assert_gapless(extension: &str, rate: u32, cuts: &[usize; 4], min_snr_db: f64) -> Vec<String> {
    let mut album = Vec::new();
    let mut decoders = Vec::new();
    for track in 0..3 {
        let name = format!("album-{track}.{extension}");
        let mut stream = open(&name);
        decoders.push(stream.decoder().to_owned());
        let (format, samples) = read_to_end(&mut stream);
        assert_eq!(format.sample_rate, rate, "{name}");
        assert_eq!(format.channels, 2, "{name}");
        let expected = cuts[track + 1] - cuts[track];
        assert_eq!(
            samples.len() / 2,
            expected,
            "{name}: decoded length in frames"
        );
        let source = left(&reference(rate, cuts[track]..cuts[track + 1]), 2);
        let lag = best_lag(&left(&samples, 2), &source, 2048, 8192, 3000);
        assert_eq!(lag, 0, "{name}: start offset in frames");
        album.extend(samples);
    }

    let source = reference(rate, 0..cuts[3]);
    let whole = snr(&source, &album);
    assert!(whole >= min_snr_db, "{extension}: album SNR {whole:.1} dB");
    let window = rate as usize / 100;
    for boundary in &cuts[1..3] {
        let range = (boundary - window / 2) * 2..(boundary + window / 2) * 2;
        let at_boundary = snr(&source[range.clone()], &album[range]);
        assert!(
            at_boundary >= min_snr_db - 6.0,
            "{extension}: {at_boundary:.1} dB around the boundary at frame {boundary}"
        );
    }
    decoders
}

#[test]
fn mp3_with_a_lame_tag_is_sample_exact() {
    assert_gapless("mp3", 44_100, &CUTS_44, 20.0);
}

#[test]
fn aac_in_mp4_is_sample_exact_with_the_edit_list_applied() {
    assert_gapless("m4a", 44_100, &CUTS_44, 20.0);
}

#[test]
fn ogg_vorbis_is_sample_exact() {
    assert_gapless("ogg", 44_100, &CUTS_44, 20.0);
}

#[test]
fn ogg_opus_is_sample_exact_through_libopus() {
    let decoders = assert_gapless("opus", 48_000, &CUTS_48, 20.0);
    assert!(decoders.iter().all(|d| d == "libopus"), "{decoders:?}");
}

#[test]
fn flac_is_bit_exact() {
    // 16-bit quantization of the reference is the only difference.
    assert_gapless("flac", 44_100, &CUTS_44, 85.0);
}
