//! The platform output against the machine's real default device: cpal on
//! desktop, AAudio on Android. CI runners have no audio device, so these
//! are ignored there. On desktop:
//!
//! ```text
//! cargo test -p ristle-audio --all-features --test device -- --ignored
//! ```
//!
//! On an emulator or phone (from Git Bash, `MSYS_NO_PATHCONV=1`):
//!
//! ```text
//! cargo ndk -t x86_64 --platform 28 test -p ristle-audio --all-features --test device --no-run
//! adb push target/x86_64-linux-android/debug/deps/device-<hash> /data/local/tmp/device
//! adb shell chmod 755 /data/local/tmp/device
//! adb shell /data/local/tmp/device --ignored --test-threads=1 --nocapture
//! ```
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use ristle_audio::test_support::FakeStream;
use ristle_audio::{Engine, EngineConfig, EngineEvent, EngineObserver, StreamFormat, TrackToken};

#[cfg(not(target_os = "android"))]
fn output() -> Box<dyn ristle_audio::AudioOutput> {
    Box::new(ristle_audio::output::cpal::CpalOutput::new())
}

#[cfg(target_os = "android")]
fn output() -> Box<dyn ristle_audio::AudioOutput> {
    Box::new(ristle_audio::output::aaudio::AAudioOutput::new())
}

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
    let engine = Engine::new(output(), EngineConfig::default(), events.clone()).unwrap();
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
    // The device pulls in real time. The position is what has been heard:
    // frames rendered less the output latency, so it never leads the wall
    // clock (beyond the latency estimate's error) and trails it by start-up.
    println!("position {position} ms after {elapsed} ms");
    assert!(
        position + 300 >= elapsed && position <= elapsed + 30,
        "{position} ms after {elapsed} ms"
    );

    // Ended is reported once the last frame has been heard.
    let deadline = Instant::now() + Duration::from_secs(3);
    while events.0.lock().unwrap().is_empty() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    // The decode thread stops output right after it reports the end.
    thread::sleep(Duration::from_millis(50));
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
        output(),
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

/// ADR 0004: the stream must reach the deep-buffer path, tagged as music.
#[cfg(target_os = "android")]
#[test]
#[ignore = "needs an audio output device"]
fn aaudio_stream_is_power_saving_music() {
    let opened = ristle_audio::output::aaudio::probe().unwrap();
    println!("{opened:?}");
    // PowerSaving is a request. Phones grant it (deep buffer or offload);
    // the emulator's HAL has neither and answers None. What must never
    // happen is the low-latency path cpal's default took on a Pixel 8.
    assert_ne!(opened.performance_mode, "LowLatency");
    assert_eq!(opened.content_type, "Music");
    assert_eq!(opened.usage, "Media");
}
