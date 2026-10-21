//! Seeks land exactly on the requested presentation time, after the encoder
//! delay, in every container (ADR 0004: a seek target maps through the MP4
//! edit list, so the demuxer target and the discarded frames include its
//! media time).

// Test helpers outside #[test] functions are not covered by clippy.toml.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{CUTS_44, CUTS_48, best_lag, left, open, read_to_end, reference};
use ristle_media::{MediaStream, Read};

const FILES: [(&str, u32); 6] = [
    ("album-1.mp3", 44_100),
    ("album-1.m4a", 44_100),
    ("album-1.ogg", 44_100),
    ("album-1.opus", 48_000),
    ("album-1.flac", 44_100),
    ("muxed.mp4", 44_100),
];

fn track_1(rate: u32) -> (usize, usize) {
    if rate == 48_000 {
        (CUTS_48[1], CUTS_48[2])
    } else {
        (CUTS_44[1], CUTS_44[2])
    }
}

#[test]
fn a_seek_starts_exactly_at_the_target() {
    for (name, rate) in FILES {
        let (start, end) = track_1(rate);
        let mut stream = open(name);
        for position_ms in [700, 250, 1000] {
            stream.seek(position_ms).unwrap();
            let (_, samples) = read_to_end(&mut stream);
            let target = position_ms as usize * rate as usize / 1000;
            assert_eq!(
                samples.len() / 2,
                end - start - target,
                "{name} at {position_ms} ms: frames left"
            );
            let source = left(&reference(rate, start + target..end), 2);
            let lag = best_lag(&left(&samples, 2), &source, 1024, 4096, 2000);
            assert_eq!(lag, 0, "{name} at {position_ms} ms: offset in frames");
        }
    }
}

#[test]
fn seeking_to_zero_replays_the_start() {
    for (name, _) in FILES {
        let mut stream = open(name);
        let (_, first) = read_to_end(&mut stream);
        stream.seek(0).unwrap();
        let (_, again) = read_to_end(&mut stream);
        assert_eq!(first.len(), again.len(), "{name}");
        // Deterministic decoders give the same output from the same state.
        let differs = first.iter().zip(&again).position(|(a, b)| a != b);
        assert_eq!(differs, None, "{name}: first differing sample");
    }
}

#[test]
fn a_seek_past_the_end_ends_the_stream() {
    for (name, _) in FILES {
        let mut stream = open(name);
        stream.seek(60_000).unwrap();
        let mut buffer = [0f32; 512];
        assert_eq!(stream.read(&mut buffer).unwrap(), Read::End, "{name}");
        // A position from a malformed command must not overflow.
        stream.seek(u64::MAX).unwrap();
        let mut buffer = [0f32; 512];
        assert_eq!(stream.read(&mut buffer).unwrap(), Read::End, "{name}");
        // And a later seek recovers.
        stream.seek(1000).unwrap();
        assert!(
            matches!(stream.read(&mut buffer).unwrap(), Read::Frames(_)),
            "{name}"
        );
    }
}
