//! The engine against real `ristle-media` streams: workstream D's album
//! fixtures (three tracks cut from one signal at non-frame-aligned points)
//! played back to back through Gapless Transitions. The engine's output must
//! be exactly what the streams decode, frame for frame: the engine adds,
//! drops and moves nothing.

// Test helpers outside #[test] functions are not covered by clippy.toml.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ristle_audio::test_support::{CaptureHandle, CaptureOutput};
use ristle_audio::{
    Engine, EngineConfig, EngineEvent, EngineObserver, MediaStream, Read, StreamFormat, TrackToken,
};
use ristle_media::{ContainerStream, DecoderRegistry, MediaHint};

const WAIT: Duration = Duration::from_secs(10);
/// Frames per simulated device callback.
const CALLBACK: usize = 1024;

#[derive(Default)]
struct Events(Mutex<Vec<EngineEvent>>);

impl EngineObserver for Events {
    fn on_event(&self, event: EngineEvent) {
        self.0.lock().unwrap().push(event);
    }
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../media/tests/fixtures")
        .join(name)
}

fn open(name: &str) -> ContainerStream {
    ContainerStream::open_file(
        &fixture(name),
        &MediaHint::default(),
        &DecoderRegistry::with_defaults(),
    )
    .unwrap_or_else(|error| panic!("opening {name}: {error}"))
}

/// What the stream decodes on its own, to its end.
fn decode(name: &str) -> (StreamFormat, Vec<f32>) {
    let mut stream = open(name);
    let format = stream.format();
    let mut buffer = vec![0.0; 4096 * usize::from(format.channels)];
    let mut out = Vec::new();
    loop {
        match stream.read(&mut buffer).unwrap() {
            Read::Frames(frames) => {
                out.extend_from_slice(&buffer[..frames * usize::from(format.channels)]);
            }
            Read::Pending => panic!("a file stream never pends"),
            Read::End => return (format, out),
        }
    }
}

/// Plays `names` back to back through the engine, preparing each next Track
/// while the previous one plays, and returns the output up to the end of
/// the last Track.
fn play_album(names: &[String], output_rate: u32) -> Vec<f32> {
    let (output, handle) = CaptureOutput::new(output_rate, 2);
    let events = Arc::new(Events::default());
    let config = EngineConfig {
        buffer_ms: 2000,
        preferred_rate: None,
        channels: 2,
    };
    let engine = Engine::new(Box::new(output), config, events.clone()).unwrap();
    let token = |index: usize| TrackToken(index as u64);
    engine.set_current(token(0), Box::new(open(&names[0])), 0);
    assert!(engine.wait_until_buffered(WAIT));
    let mut prepared = 0;
    engine.play().unwrap();
    let mut out = Vec::new();
    let ended = || {
        events.0.lock().unwrap().contains(&EngineEvent::Ended {
            track: token(names.len() - 1),
        })
    };
    while !ended() {
        let state = engine.state();
        // Prepare the next Track as soon as the previous one is current,
        // and have it decoded before the current one runs out.
        if state.next.is_none()
            && prepared + 1 < names.len()
            && state.current == Some(token(prepared))
        {
            prepared += 1;
            engine
                .set_next(token(prepared), Box::new(open(&names[prepared])))
                .unwrap();
            assert!(engine.wait_until_next_buffered(WAIT));
        }
        // Keep the decoder a buffer ahead (or at the end), as a real
        // device's pace would; a test runs faster than real time.
        assert!(engine.wait_until_buffered(WAIT) || engine.state().ended);
        out.extend(render(&handle, CALLBACK));
    }
    out
}

fn render(handle: &CaptureHandle, frames: usize) -> Vec<f32> {
    handle.render(frames)
}

fn album(extension: &str) -> Vec<String> {
    (0..3)
        .map(|track| format!("album-{track}.{extension}"))
        .collect()
}

/// Plays the album at the stream's own rate: the engine passes samples
/// through, so its output must equal the streams' decode exactly.
fn assert_engine_is_transparent(extension: &str) {
    let names = album(extension);
    let mut expected = Vec::new();
    let mut rate = 0;
    for name in &names {
        let (format, samples) = decode(name);
        assert_eq!(format.channels, 2, "{name}");
        rate = format.sample_rate;
        expected.extend(samples);
    }
    let played = play_album(&names, rate);
    assert!(
        played.len() >= expected.len(),
        "{extension}: output too short"
    );
    let first_difference = expected
        .iter()
        .zip(&played)
        .position(|(a, b)| a != b)
        .map(|sample| sample / 2);
    assert_eq!(first_difference, None, "{extension}: first differing frame");
    assert!(
        played[expected.len()..].iter().all(|s| *s == 0.0),
        "{extension}: audio after the last Track"
    );
}

#[test]
fn mp3_album_plays_through_the_engine_sample_exact() {
    assert_engine_is_transparent("mp3");
}

#[test]
fn aac_album_with_edit_lists_plays_through_the_engine_sample_exact() {
    assert_engine_is_transparent("m4a");
}

#[test]
fn vorbis_album_plays_through_the_engine_sample_exact() {
    assert_engine_is_transparent("ogg");
}

#[test]
fn opus_album_plays_through_the_engine_sample_exact() {
    assert_engine_is_transparent("opus");
}

#[test]
fn flac_album_plays_through_the_engine_sample_exact() {
    assert_engine_is_transparent("flac");
}

#[test]
fn a_resampled_album_keeps_every_track_length_exact() {
    // 44.1 kHz FLAC into a 48 kHz output: each Track is resampled on its
    // own, and its length must be exactly round(n * 48 000 / 44 100).
    let names = album("flac");
    let mut expected_frames = 0;
    for name in &names {
        let (format, samples) = decode(name);
        let frames = (samples.len() / 2) as u64;
        let rate = u64::from(format.sample_rate);
        expected_frames += (frames * 48_000 + rate / 2) / rate;
    }
    let played = play_album(&names, 48_000);
    let frames = played.as_chunks::<2>().0;
    let last_audio = frames
        .iter()
        .rposition(|frame| *frame != [0.0, 0.0])
        .unwrap();
    // No gap anywhere: the signal has no silent stretch.
    let longest_silence = frames[..=last_audio]
        .split(|frame| *frame != [0.0, 0.0])
        .map(<[_]>::len)
        .max()
        .unwrap_or(0);
    assert!(
        longest_silence <= 2,
        "a silent run of {longest_silence} frames"
    );
    // The signal is never silent for long, so the last non-zero frame is
    // the last frame, give or take the band-limited tail's zero crossings.
    let expected = expected_frames as usize;
    assert!(
        last_audio < expected && last_audio + 1 + 8 >= expected,
        "{} frames of audio, expected {expected}",
        last_audio + 1
    );
    assert!(played[expected * 2..].iter().all(|s| *s == 0.0));
}

#[test]
fn a_seek_in_a_real_stream_plays_from_the_exact_frame() {
    let name = "album-0.m4a";
    let mut direct = open(name);
    direct.seek(700).unwrap();
    let mut expected = vec![0.0; 256 * 2];
    let mut filled = 0;
    while filled < expected.len() {
        match direct.read(&mut expected[filled..]).unwrap() {
            Read::Frames(frames) => filled += frames * 2,
            other => panic!("{other:?}"),
        }
    }

    let rate = direct.format().sample_rate;
    let (output, handle) = CaptureOutput::new(rate, 2);
    let engine = Engine::new(
        Box::new(output),
        EngineConfig::default(),
        Arc::new(Events::default()),
    )
    .unwrap();
    engine.set_current(TrackToken(1), Box::new(open(name)), 0);
    assert!(engine.wait_until_buffered(WAIT));
    engine.play().unwrap();
    render(&handle, 100);
    engine.seek(700).unwrap();
    assert!(engine.wait_until_buffered(WAIT));
    assert_eq!(render(&handle, 256), expected);
    assert_eq!(
        engine.state().position_ms,
        700 + 256 * 1000 / u64::from(rate)
    );
}
