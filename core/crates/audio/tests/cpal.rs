//! The cpal output against the machine's real default device. CI runners
//! have no audio device, so these are ignored there; run them with
//! `cargo test -p ristle-audio --all-features --test cpal -- --ignored`.
#![cfg(not(target_os = "android"))]
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use ristle_audio::output::cpal::CpalOutput;
use ristle_audio::test_support::FakeStream;
use ristle_audio::{Engine, EngineConfig, EngineEvent, EngineObserver, StreamFormat, TrackToken};

#[derive(Default)]
struct Events(Mutex<Vec<EngineEvent>>);

impl EngineObserver for Events {
    fn on_event(&self, event: EngineEvent) {
        self.0.lock().unwrap().push(event);
    }
}

#[test]
#[ignore = "needs an audio output device"]
fn plays_at_the_device_rate_in_real_time_and_ends() {
    let events = Arc::new(Events::default());
    let engine = Engine::new(
        Box::new(CpalOutput::new()),
        EngineConfig::default(),
        events.clone(),
    )
    .unwrap();
    let info = engine.output_info();
    assert!(info.sample_rate >= 8_000 && info.channels >= 1, "{info:?}");

    // 1.5 s of silence at 44.1 kHz, resampled to whatever the device runs at.
    let silence = FakeStream::constant(
        0.0,
        66_150,
        StreamFormat {
            sample_rate: 44_100,
            channels: 2,
        },
    );
    engine.set_current(TrackToken(1), Box::new(silence), 0);
    engine.play().unwrap();
    let started = Instant::now();
    thread::sleep(Duration::from_millis(1000));
    let position = engine.state().position_ms;
    let elapsed = started.elapsed().as_millis() as u64;
    // The device pulls in real time; allow for its buffer and start-up.
    assert!(
        position + 300 >= elapsed && position <= elapsed + 50,
        "{position} ms after {elapsed} ms"
    );

    let deadline = Instant::now() + Duration::from_secs(3);
    while !engine.state().ended && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    let state = engine.state();
    assert!(state.ended && !state.playing, "{state:?}");
    assert_eq!(state.position_ms, 1500);
    let events = events.0.lock().unwrap().clone();
    assert_eq!(
        events,
        [EngineEvent::Ended {
            track: TrackToken(1)
        }]
    );
}

#[test]
#[ignore = "needs an audio output device"]
fn pause_and_resume_drive_the_device() {
    let engine = Engine::new(
        Box::new(CpalOutput::new()),
        EngineConfig::default(),
        Arc::new(Events::default()),
    )
    .unwrap();
    let silence = FakeStream::constant(
        0.0,
        441_000,
        StreamFormat {
            sample_rate: 44_100,
            channels: 2,
        },
    );
    engine.set_current(TrackToken(1), Box::new(silence), 0);
    engine.play().unwrap();
    thread::sleep(Duration::from_millis(300));
    engine.pause();
    thread::sleep(Duration::from_millis(100));
    let paused_at = engine.state().position_ms;
    thread::sleep(Duration::from_millis(300));
    assert_eq!(engine.state().position_ms, paused_at);
    engine.play().unwrap();
    thread::sleep(Duration::from_millis(300));
    assert!(engine.state().position_ms > paused_at + 100);
}
