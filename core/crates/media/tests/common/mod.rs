//! Shared by the integration tests: the fixtures' source signal, decoding
//! helpers, and the sample-accurate alignment check from spike B.

// Each test crate uses a different subset; helpers outside #[test]
// functions are not covered by clippy.toml.
#![allow(dead_code, unreachable_pub, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use ristle_media::{ContainerStream, DecoderRegistry, MediaHint, MediaStream, Read, StreamFormat};

/// Album cut points at 44.1 kHz and 48 kHz (`fixtures/generate.sh`).
pub const CUTS_44: [usize; 4] = [0, 52_517, 105_011, 158_760];
pub const CUTS_48: [usize; 4] = [0, 57_131, 114_287, 172_800];

pub fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

pub fn open(name: &str) -> ContainerStream {
    ContainerStream::open_file(
        &fixture(name),
        &MediaHint::default(),
        &DecoderRegistry::with_defaults(),
    )
    .unwrap_or_else(|error| panic!("opening {name}: {error}"))
}

/// The fixtures' signal at `frame`, as ffmpeg's `aevalsrc` computes it and
/// `pcm_s16le` stores it. Keep in sync with `fixtures/generate.sh`.
pub fn signal(rate: u32, frame: usize) -> [f32; 2] {
    use std::f64::consts::PI;
    let t = frame as f64 / f64::from(rate);
    let left = 0.25 * (2.0 * PI * 440.0 * t).sin() * (0.75 + 0.25 * (2.0 * PI * 3.0 * t).sin())
        + 0.15 * (2.0 * PI * (150.0 * t + 300.0 * t * t)).sin();
    let right = 0.25 * (2.0 * PI * 554.37 * t).sin() * (0.75 + 0.25 * (2.0 * PI * 2.0 * t).cos())
        + 0.15 * (2.0 * PI * (2000.0 * t - 250.0 * t * t)).sin();
    let quantize = |x: f64| ((x * 32768.0).round().clamp(-32768.0, 32767.0) / 32768.0) as f32;
    [quantize(left), quantize(right)]
}

/// Interleaved stereo reference for frames `range` at `rate`.
pub fn reference(rate: u32, range: std::ops::Range<usize>) -> Vec<f32> {
    range.flat_map(|frame| signal(rate, frame)).collect()
}

/// Reads a stream to its end with the given buffer size (in frames),
/// checking the `MediaStream` contract on the way.
pub fn read_to_end_with(
    stream: &mut dyn MediaStream,
    buffer_frames: usize,
) -> (StreamFormat, Vec<f32>) {
    let format = stream.format();
    let channels = usize::from(format.channels);
    let mut buffer = vec![0f32; buffer_frames * channels];
    let mut out = Vec::new();
    loop {
        match stream.read(&mut buffer).expect("read") {
            Read::Frames(frames) => {
                assert!(frames > 0 && frames <= buffer_frames);
                assert_eq!(stream.format(), format, "file streams keep their format");
                out.extend_from_slice(&buffer[..frames * channels]);
            }
            Read::Pending => panic!("a file stream never pends"),
            Read::End => break,
        }
    }
    // End is sticky.
    assert_eq!(stream.read(&mut buffer).expect("read after end"), Read::End);
    (format, out)
}

pub fn read_to_end(stream: &mut dyn MediaStream) -> (StreamFormat, Vec<f32>) {
    read_to_end_with(stream, 1024)
}

pub fn left(interleaved: &[f32], channels: usize) -> Vec<f32> {
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame[0])
        .collect()
}

/// The lag (in frames) at which `decoded` best matches `source`, searching
/// ±`max` frames. Correlates `window` frames starting `skip` frames in, so a
/// codec's onset smearing does not dominate. 0 means sample-accurate.
pub fn best_lag(decoded: &[f32], source: &[f32], skip: usize, window: usize, max: i64) -> i64 {
    let mut best = (f64::MIN, 0);
    for lag in -max..=max {
        let mut dot = 0f64;
        let mut energy = 0f64;
        for i in skip..skip + window {
            let j = i as i64 + lag;
            if j < 0 || j as usize >= decoded.len() || i >= source.len() {
                continue;
            }
            let a = f64::from(decoded[j as usize]);
            dot += a * f64::from(source[i]);
            energy += a * a;
        }
        let score = dot / energy.sqrt().max(1e-12);
        if score > best.0 {
            best = (score, lag);
        }
    }
    best.1
}

/// Signal-to-error ratio in dB.
pub fn snr(signal: &[f32], decoded: &[f32]) -> f64 {
    let n = signal.len().min(decoded.len());
    let power: f64 = signal[..n].iter().map(|x| f64::from(*x).powi(2)).sum();
    let error: f64 = signal[..n]
        .iter()
        .zip(&decoded[..n])
        .map(|(s, d)| (f64::from(*s) - f64::from(*d)).powi(2))
        .sum();
    10.0 * (power / error.max(1e-20)).log10()
}
