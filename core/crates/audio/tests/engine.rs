//! Engine scenarios against the capture output, with sample-exact checks.
//! The first eight are Ristle legacy's `audio_engine_test.cpp`, under the
//! names the phase 2 inventory gives them.

// clippy.toml allows unwrap in test functions; the helpers here are test
// code too.
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use ristle_audio::test_support::{CaptureHandle, CaptureOutput, FakeStream};
use ristle_audio::{
    Engine, EngineConfig, EngineEvent, EngineObserver, OutputError, StreamFormat, TrackToken,
};

const WAIT: Duration = Duration::from_millis(2000);

#[derive(Default)]
struct Events(Mutex<Vec<EngineEvent>>);

impl EngineObserver for Events {
    fn on_event(&self, event: EngineEvent) {
        self.0.lock().unwrap().push(event);
    }
}

impl Events {
    fn all(&self) -> Vec<EngineEvent> {
        self.0.lock().unwrap().clone()
    }

    fn wait_for(&self, wanted: impl Fn(&EngineEvent) -> bool) -> bool {
        until(|| self.all().iter().any(&wanted))
    }
}

struct Rig {
    engine: Engine,
    output: CaptureHandle,
    events: Arc<Events>,
}

fn rig(sample_rate: u32, channels: u16, buffer_ms: u32) -> Rig {
    let (output, handle) = CaptureOutput::new(sample_rate, channels);
    let events = Arc::new(Events::default());
    let config = EngineConfig {
        buffer_ms,
        preferred_rate: None,
        channels,
    };
    let engine = Engine::new(Box::new(output), config, events.clone()).unwrap();
    Rig {
        engine,
        output: handle,
        events,
    }
}

fn format(sample_rate: u32, channels: u16) -> StreamFormat {
    StreamFormat {
        sample_rate,
        channels,
    }
}

fn until(condition: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + WAIT;
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        thread::sleep(Duration::from_millis(1));
    }
    false
}

const T1: TrackToken = TrackToken(1);
const T2: TrackToken = TrackToken(2);

#[test]
fn audio_engine_joins_prepared_sources_at_the_exact_frame_boundary() {
    let rig = rig(100, 2, 2000);
    rig.engine.set_current(
        T1,
        Box::new(FakeStream::constant(0.25, 40, format(100, 2))),
        0,
    );
    rig.engine
        .set_next(T2, Box::new(FakeStream::constant(0.75, 60, format(100, 2))))
        .unwrap();
    assert!(rig.engine.wait_until_buffered(WAIT));
    assert!(rig.engine.wait_until_next_buffered(WAIT));
    rig.engine.play().unwrap();

    let output = rig.output.render(100);

    for frame in 0..40 {
        assert_eq!(output[frame * 2], 0.25);
        assert_eq!(output[frame * 2 + 1], 0.25);
    }
    for frame in 40..100 {
        assert_eq!(output[frame * 2], 0.75);
        assert_eq!(output[frame * 2 + 1], 0.75);
    }
    assert!(
        rig.events
            .wait_for(|e| matches!(e, EngineEvent::Transitioned { .. }))
    );
    let transitions: Vec<_> = rig
        .events
        .all()
        .into_iter()
        .filter(|e| matches!(e, EngineEvent::Transitioned { .. }))
        .collect();
    assert_eq!(
        transitions,
        [EngineEvent::Transitioned { from: T1, to: T2 }]
    );
}

#[test]
fn audio_engine_seek_sequence_prevents_stale_position_authority() {
    let rig = rig(1000, 2, 1000);
    assert!(rig.engine.state().current.is_none());
    rig.engine.set_current(
        T1,
        Box::new(FakeStream::constant(0.5, 5000, format(1000, 2))),
        0,
    );
    assert!(rig.engine.wait_until_buffered(WAIT));
    rig.engine.play().unwrap();

    rig.output.render(100);
    let before_seek = rig.engine.state();
    rig.engine.seek(2000).unwrap();
    let after_seek = rig.engine.state();

    assert!(after_seek.sequence > before_seek.sequence);
    assert_eq!(after_seek.position_ms, 2000);
    assert!(after_seek.playing);
    assert!(after_seek.current.is_some());
}

#[test]
fn audio_engine_paused_track_stops_buffering_once_decoded() {
    let rig = rig(100, 2, 1000);
    rig.engine.set_current(
        T1,
        Box::new(FakeStream::constant(1.0, 10, format(100, 2))),
        0,
    );
    assert!(rig.engine.wait_until_buffered(WAIT));
    // Nothing has rendered: a paused engine never runs the render callback.
    assert!(!rig.output.is_running());
    assert!(!rig.engine.state().playing);
    assert!(!rig.engine.state().buffering);
}

#[test]
fn audio_engine_applies_one_core_owned_gain() {
    let rig = rig(100, 2, 1000);
    rig.engine.set_current(
        T1,
        Box::new(FakeStream::constant(1.0, 10, format(100, 2))),
        0,
    );
    rig.engine.set_volume(0.25).unwrap();
    assert!(rig.engine.wait_until_buffered(WAIT));
    rig.engine.play().unwrap();

    let output = rig.output.render(10);
    for sample in output {
        assert_eq!(sample, 0.25);
    }
}

#[test]
fn audio_engine_promotes_buffered_next_without_reopening_it() {
    let rig = rig(100, 2, 2000);
    rig.engine.set_current(
        T1,
        Box::new(FakeStream::constant(0.2, 100, format(100, 2))),
        0,
    );
    rig.engine
        .set_next(T2, Box::new(FakeStream::constant(0.8, 100, format(100, 2))))
        .unwrap();
    assert!(until(|| rig.engine.state().next.is_some()));
    rig.engine.play().unwrap();

    assert!(rig.engine.wait_until_next_buffered(WAIT));
    assert!(rig.engine.promote_next());
    assert!(rig.engine.state().next.is_none());
    assert_eq!(rig.engine.state().current, Some(T2));
    let output = rig.output.render(10);
    assert_eq!(output[0], 0.8);
}

#[test]
fn audio_engine_does_not_promote_failed_prepared_next() {
    let rig = rig(100, 2, 2000);
    rig.engine.set_current(
        T1,
        Box::new(FakeStream::constant(0.2, 100, format(100, 2))),
        0,
    );
    rig.engine
        .set_next(
            T2,
            Box::new(
                FakeStream::constant(0.8, 100, format(100, 2))
                    .failing_at(0, "deterministic source failure"),
            ),
        )
        .unwrap();
    assert!(until(|| rig.engine.state().next.is_none()));

    assert!(
        !rig.engine
            .wait_until_next_buffered(Duration::from_millis(20))
    );
    assert!(!rig.engine.promote_next());
    assert!(rig.engine.state().error.is_none());
}

#[test]
fn audio_engine_keeps_polling_sources_that_are_not_exhausted() {
    let rig = rig(100, 2, 1000);
    // 20 empty reads must not be mistaken for the end of the Track.
    rig.engine.set_current(
        T1,
        Box::new(FakeStream::constant(0.5, 50, format(100, 2)).with_pending_reads(20)),
        0,
    );
    assert!(rig.engine.wait_until_buffered(WAIT));
    rig.engine.play().unwrap();

    let output = rig.output.render(50);
    for frame in 0..50 {
        assert_eq!(output[frame * 2], 0.5);
        assert_eq!(output[frame * 2 + 1], 0.5);
    }
    assert!(rig.engine.state().error.is_none());
}

#[test]
fn audio_engine_retains_source_failures_for_diagnostics() {
    let rig = rig(100, 2, 1000);
    rig.engine.set_current(
        T1,
        Box::new(
            FakeStream::constant(0.5, 100, format(100, 2))
                .failing_at(0, "deterministic source failure"),
        ),
        0,
    );

    assert!(until(|| rig.engine.state().error.is_some()));
    assert_eq!(
        rig.engine.state().error.as_deref(),
        Some("deterministic source failure")
    );
}

/// Renders `frames` frames in callbacks of `chunk` frames and returns the
/// left channel.
fn render_left(output: &CaptureHandle, frames: usize, chunk: usize, channels: usize) -> Vec<f32> {
    let mut left = Vec::with_capacity(frames);
    while left.len() < frames {
        let n = chunk.min(frames - left.len());
        left.extend(
            output
                .render(n)
                .chunks_exact(channels)
                .map(|frame| frame[0]),
        );
    }
    left
}

#[test]
fn gapless_transition_is_sample_exact_across_callback_splits() {
    let rig = rig(1000, 2, 1000);
    rig.engine.set_current(
        T1,
        Box::new(FakeStream::counter(0.0, 37, format(1000, 2)).with_max_read(5)),
        0,
    );
    rig.engine
        .set_next(
            T2,
            Box::new(FakeStream::counter(1000.0, 50, format(1000, 2))),
        )
        .unwrap();
    assert!(rig.engine.wait_until_buffered(WAIT));
    assert!(rig.engine.wait_until_next_buffered(WAIT));
    rig.engine.play().unwrap();

    let left = render_left(&rig.output, 100, 7, 2);

    let expected: Vec<f32> = (0..37)
        .map(|i| i as f32)
        .chain((0..50).map(|i| 1000.0 + i as f32))
        .chain(std::iter::repeat_n(0.0, 13))
        .collect();
    assert_eq!(left, expected);
    assert!(
        rig.events
            .wait_for(|e| *e == EngineEvent::Ended { track: T2 })
    );
}

#[test]
fn gapless_transition_stays_exact_when_both_tracks_are_resampled() {
    // 44.1 kHz into a 48 kHz output, then 22.05 kHz: one second each.
    let rig = rig(48_000, 2, 2000);
    rig.engine.set_current(
        T1,
        Box::new(FakeStream::constant(0.25, 44_100, format(44_100, 2))),
        0,
    );
    rig.engine
        .set_next(
            T2,
            Box::new(FakeStream::constant(0.75, 22_050, format(22_050, 1))),
        )
        .unwrap();
    assert!(rig.engine.wait_until_buffered(WAIT));
    assert!(rig.engine.wait_until_next_buffered(WAIT));
    rig.engine.play().unwrap();

    let mut left = Vec::new();
    while left.len() < 100_000 {
        // Keep the decoder ahead, as a real device's pace would.
        assert!(until(|| {
            let state = rig.engine.state();
            !state.buffering || state.ended
        }));
        left.extend(
            rig.output
                .render(1000)
                .as_chunks::<2>()
                .0
                .iter()
                .map(|frame| frame[0]),
        );
    }

    assert!(left[1_000..47_000].iter().all(|s| (s - 0.25).abs() < 1e-3));
    assert!(left[49_000..95_000].iter().all(|s| (s - 0.75).abs() < 1e-3));
    // The join is at frame 48 000: the last frame of the first Track and
    // the first of the second, each nearer its own level.
    assert!((left[47_999] - 0.25).abs() < (left[47_999] - 0.75).abs());
    assert!((left[48_000] - 0.75).abs() < (left[48_000] - 0.25).abs());
    // And the second Track is exactly 48 000 frames long.
    // Its last frame rings (band-limited step) but is not silence.
    assert!(left[95_999].abs() > 0.1);
    assert!(left[96_000..].iter().all(|s| *s == 0.0));
}

#[test]
fn seeks_while_paused_with_a_full_ring_keep_the_ring() {
    // Spike B lost its ring on the third seek while paused, because the
    // paused callback never drained the handoff queue.
    let rig = rig(1000, 2, 200);
    rig.engine.set_current(
        T1,
        Box::new(FakeStream::counter(0.0, 60_000, format(1000, 2))),
        0,
    );
    assert!(rig.engine.wait_until_buffered(WAIT));
    assert!(until(
        || rig.engine.state().position_ms == 0 && !rig.engine.state().buffering
    ));
    for target in [5_000, 10_000, 15_000, 20_000, 25_000, 30_000] {
        rig.engine.seek(target).unwrap();
        assert_eq!(rig.engine.state().position_ms, target);
    }
    assert!(rig.engine.wait_until_buffered(WAIT));
    rig.engine.play().unwrap();

    let left = render_left(&rig.output, 150, 64, 2);
    let expected: Vec<f32> = (30_000..30_150).map(|i| i as f32).collect();
    assert_eq!(left, expected);
    assert_eq!(rig.engine.state().position_ms, 30_150);
}

#[test]
fn seek_during_playback_resumes_at_the_exact_frame() {
    let rig = rig(1000, 2, 500);
    rig.engine.set_current(
        T1,
        Box::new(FakeStream::counter(0.0, 10_000, format(1000, 2))),
        0,
    );
    assert!(rig.engine.wait_until_buffered(WAIT));
    rig.engine.play().unwrap();
    rig.output.render(100);

    rig.engine.seek(1234).unwrap();
    assert!(rig.engine.wait_until_buffered(WAIT));
    let left = render_left(&rig.output, 10, 10, 2);
    assert_eq!(left, (1234..1244).map(|i| i as f32).collect::<Vec<_>>());
}

#[test]
fn starting_at_a_position_reports_it_and_plays_from_it() {
    let rig = rig(1000, 2, 500);
    rig.engine.set_current(
        T1,
        Box::new(FakeStream::counter(0.0, 10_000, format(1000, 2))),
        4500,
    );
    assert_eq!(rig.engine.state().position_ms, 4500);
    assert!(rig.engine.wait_until_buffered(WAIT));
    rig.engine.play().unwrap();
    assert_eq!(render_left(&rig.output, 3, 3, 2), [4500.0, 4501.0, 4502.0]);
}

#[test]
fn last_track_ending_publishes_ended_and_stops_output() {
    let rig = rig(100, 2, 1000);
    rig.engine.set_current(
        T1,
        Box::new(FakeStream::constant(0.5, 10, format(100, 2))),
        0,
    );
    assert!(rig.engine.wait_until_buffered(WAIT));
    rig.engine.play().unwrap();

    let output = rig.output.render(20);
    assert!(output[..20].iter().all(|s| *s == 0.5));
    assert!(output[20..].iter().all(|s| *s == 0.0));

    assert!(
        rig.events
            .wait_for(|e| *e == EngineEvent::Ended { track: T1 })
    );
    assert!(until(|| !rig.output.is_running()));
    let state = rig.engine.state();
    assert!(state.ended);
    assert!(!state.playing);
    assert!(!state.buffering);
    assert_eq!(state.position_ms, 100);
    // Play on an ended Track does not restart output for nothing.
    rig.engine.play().unwrap();
    assert!(!rig.output.is_running());
    // A seek leaves the terminal state.
    rig.engine.seek(0).unwrap();
    assert!(!rig.engine.state().ended);
}

#[test]
fn a_failed_stream_plays_what_it_decoded_then_stops_without_moving_on() {
    let rig = rig(100, 2, 1000);
    rig.engine.set_current(
        T1,
        Box::new(FakeStream::constant(0.5, 100, format(100, 2)).failing_at(30, "connection reset")),
        0,
    );
    rig.engine
        .set_next(T2, Box::new(FakeStream::constant(0.9, 100, format(100, 2))))
        .unwrap();
    assert!(
        rig.events
            .wait_for(|e| matches!(e, EngineEvent::Failed { track: T1, .. }))
    );
    rig.engine.play().unwrap();

    let left = render_left(&rig.output, 40, 40, 2);
    assert!(left[..30].iter().all(|s| *s == 0.5));
    assert!(left[30..].iter().all(|s| *s == 0.0));
    let state = rig.engine.state();
    assert_eq!(state.current, Some(T1));
    assert_eq!(state.error.as_deref(), Some("connection reset"));
    assert!(!state.ended);
    // Output stops once the audio before the failure has played.
    assert!(until(
        || !rig.engine.state().playing && !rig.output.is_running()
    ));
}

#[test]
fn a_failed_next_track_lets_the_current_one_end() {
    let rig = rig(100, 2, 1000);
    rig.engine.set_current(
        T1,
        Box::new(FakeStream::constant(0.5, 10, format(100, 2))),
        0,
    );
    rig.engine
        .set_next(
            T2,
            Box::new(FakeStream::constant(0.9, 10, format(100, 2)).failing_at(0, "gone")),
        )
        .unwrap();
    assert!(
        rig.events
            .wait_for(|e| matches!(e, EngineEvent::Failed { track: T2, .. }))
    );
    assert!(rig.engine.wait_until_buffered(WAIT));
    rig.engine.play().unwrap();

    rig.output.render(20);
    assert!(
        rig.events
            .wait_for(|e| *e == EngineEvent::Ended { track: T1 })
    );
    assert!(
        !rig.events
            .all()
            .iter()
            .any(|e| matches!(e, EngineEvent::Transitioned { .. }))
    );
}

#[test]
fn a_format_change_mid_stream_keeps_the_length_exact() {
    // Implicit SBR: the first 2 400 frames at 24 kHz, the rest at 48 kHz.
    let rig = rig(48_000, 2, 2000);
    rig.engine.set_current(
        T1,
        Box::new(
            FakeStream::constant(0.5, 7_200, format(24_000, 2))
                .changing_format_at(2_400, format(48_000, 2)),
        ),
        0,
    );
    assert!(rig.engine.wait_until_buffered(WAIT));
    rig.engine.play().unwrap();

    let left = render_left(&rig.output, 12_000, 12_000, 2);
    // 2 400 frames at 24 kHz are 4 800 at 48 kHz, then 4 800 more.
    assert!(left[9_599] != 0.0);
    assert!(left[9_600..].iter().all(|s| *s == 0.0));
    assert!(left[5_000..9_500].iter().all(|s| *s == 0.5));
}

#[test]
fn a_volume_change_during_playback_ramps() {
    let rig = rig(1000, 1, 1000);
    rig.engine.set_current(
        T1,
        Box::new(FakeStream::constant(1.0, 1000, format(1000, 1))),
        0,
    );
    assert!(rig.engine.wait_until_buffered(WAIT));
    rig.engine.play().unwrap();
    assert!(rig.output.render(10).iter().all(|s| *s == 1.0));

    rig.engine.set_volume(0.0).unwrap();
    // Callbacks shorter than the 10 ms ramp still finish it in 10 ms.
    let ramp: Vec<f32> = (0..10).flat_map(|_| rig.output.render(2)).collect();
    assert!(ramp.windows(2).all(|w| w[1] <= w[0]));
    assert!(ramp[0] < 1.0 && ramp[0] > 0.8);
    assert!(ramp[9] < 1e-6);
    assert!(ramp[10..].iter().all(|s| *s == 0.0));
    assert!(rig.engine.set_volume(1.5).is_err());
}

#[test]
fn pause_stops_output_and_play_resumes_where_it_was() {
    let rig = rig(1000, 2, 500);
    rig.engine.set_current(
        T1,
        Box::new(FakeStream::counter(0.0, 5000, format(1000, 2))),
        0,
    );
    assert!(rig.engine.wait_until_buffered(WAIT));
    rig.engine.play().unwrap();
    rig.output.render(25);
    rig.engine.pause();
    assert!(!rig.output.is_running());
    assert_eq!(rig.engine.state().position_ms, 25);
    rig.engine.play().unwrap();
    assert_eq!(render_left(&rig.output, 2, 2, 2), [25.0, 26.0]);
}

#[test]
fn clearing_the_current_track_stops_and_empties_the_engine() {
    let rig = rig(100, 2, 1000);
    rig.engine.set_current(
        T1,
        Box::new(FakeStream::constant(0.5, 100, format(100, 2))),
        0,
    );
    rig.engine
        .set_next(T2, Box::new(FakeStream::constant(0.5, 100, format(100, 2))))
        .unwrap();
    rig.engine.play().unwrap();
    rig.engine.clear_current();
    let state = rig.engine.state();
    assert_eq!(
        (state.current, state.next, state.playing),
        (None, None, false)
    );
    assert!(!rig.output.is_running());
    assert!(rig.engine.play().is_err());
    assert!(rig.engine.seek(0).is_err());
}

#[test]
fn replacing_the_next_track_drops_its_decoded_audio() {
    let rig = rig(100, 2, 1000);
    rig.engine.set_current(
        T1,
        Box::new(FakeStream::constant(0.1, 10, format(100, 2))),
        0,
    );
    rig.engine
        .set_next(T2, Box::new(FakeStream::constant(0.2, 50, format(100, 2))))
        .unwrap();
    assert!(rig.engine.wait_until_next_buffered(WAIT));
    let t3 = TrackToken(3);
    rig.engine
        .set_next(t3, Box::new(FakeStream::constant(0.3, 50, format(100, 2))))
        .unwrap();
    assert!(rig.engine.wait_until_buffered(WAIT));
    assert!(rig.engine.wait_until_next_buffered(WAIT));
    rig.engine.play().unwrap();

    let left = render_left(&rig.output, 20, 20, 2);
    assert!(left[..10].iter().all(|s| *s == 0.1));
    assert!(left[10..].iter().all(|s| *s == 0.3));
    assert!(
        rig.events
            .wait_for(|e| *e == EngineEvent::Transitioned { from: T1, to: t3 })
    );
}

#[test]
fn a_next_track_set_after_the_transition_follows_gaplessly() {
    let rig = rig(100, 2, 1000);
    let t3 = TrackToken(3);
    rig.engine.set_current(
        T1,
        Box::new(FakeStream::constant(0.1, 10, format(100, 2))),
        0,
    );
    rig.engine
        .set_next(T2, Box::new(FakeStream::constant(0.2, 10, format(100, 2))))
        .unwrap();
    assert!(rig.engine.wait_until_next_buffered(WAIT));
    rig.engine.play().unwrap();
    rig.output.render(15);
    assert!(
        rig.events
            .wait_for(|e| *e == EngineEvent::Transitioned { from: T1, to: T2 })
    );

    rig.engine
        .set_next(t3, Box::new(FakeStream::constant(0.3, 10, format(100, 2))))
        .unwrap();
    assert!(rig.engine.wait_until_next_buffered(WAIT));
    let left = render_left(&rig.output, 10, 10, 2);
    assert!(left[..5].iter().all(|s| *s == 0.2));
    assert!(left[5..].iter().all(|s| *s == 0.3));
}

#[test]
fn an_output_failure_pauses_and_is_reported() {
    let rig = rig(100, 2, 1000);
    rig.engine.set_current(
        T1,
        Box::new(FakeStream::constant(0.5, 100, format(100, 2))),
        0,
    );
    rig.engine.play().unwrap();
    rig.output
        .fail(OutputError::Backend("device unplugged".into()));
    assert!(
        rig.events
            .wait_for(|e| matches!(e, EngineEvent::OutputFailed { .. }))
    );
    assert!(until(|| !rig.engine.state().playing));
    assert!(!rig.output.is_running());
    assert_eq!(rig.output.opened(), 1);
}

#[test]
fn a_failed_resampled_stream_still_plays_what_it_decoded() {
    // 500 frames are less than one resampler chunk: all of them sit in the
    // resampler when the stream fails, and must still play.
    let rig = rig(48_000, 2, 1000);
    rig.engine.set_current(
        T1,
        Box::new(FakeStream::constant(0.5, 44_100, format(44_100, 2)).failing_at(500, "reset")),
        0,
    );
    assert!(
        rig.events
            .wait_for(|e| matches!(e, EngineEvent::Failed { track: T1, .. }))
    );
    // The failure is reported after its audio is in the ring.

    rig.engine.play().unwrap();

    let left = render_left(&rig.output, 1000, 1000, 2);
    let played = left.iter().rposition(|s| *s != 0.0).map_or(0, |i| i + 1);
    // round(500 * 48 000 / 44 100)
    assert_eq!(played, 544);
}

#[test]
fn promoting_right_after_a_transition_does_not_go_back() {
    let rig = rig(100, 2, 1000);
    rig.engine.set_current(
        T1,
        Box::new(FakeStream::constant(0.1, 10, format(100, 2))),
        0,
    );
    rig.engine
        .set_next(T2, Box::new(FakeStream::constant(0.2, 50, format(100, 2))))
        .unwrap();
    assert!(rig.engine.wait_until_next_buffered(WAIT));
    rig.engine.play().unwrap();
    rig.output.render(15);

    // The finished Track may still sit in the other slot: it is not next.
    assert!(!rig.engine.promote_next());
    let state = rig.engine.state();
    assert_eq!((state.current, state.next), (Some(T2), None));
    let left = render_left(&rig.output, 5, 5, 2);
    assert!(left.iter().all(|s| *s == 0.2));
}
