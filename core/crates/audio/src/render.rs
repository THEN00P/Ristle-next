//! The render callback: drains the current slot's ring into the output
//! buffer, switches slots at the exact end frame (Gapless Transition),
//! detects the end of playback and applies the one core-owned gain.
//!
//! Runs on the output's real-time thread: no locks, no allocation, no
//! logging. It reports to the decode thread through a preallocated queue and
//! an unpark.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::output::Render;
use crate::shared::{Record, Shared};

/// What the render callback tells the decode thread.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Note {
    /// The callback moved from `from` (in slot `from_slot`) to `to`.
    Transitioned {
        from_slot: usize,
        from: Record,
        to: Record,
    },
    /// Playback reached the end of `record` and nothing followed.
    Ended { record: Record },
    /// Playback reached the point where `record`'s stream failed.
    Stopped { record: Record },
}

/// Capacity of the note queue. A note needs a Track change, and every Track
/// change needs the control side, so the queue never holds more than a few.
pub(crate) const NOTE_CAPACITY: usize = 64;

/// Length of a gain change, in milliseconds.
const GAIN_RAMP_MS: u64 = 10;

/// The `Render` handed to the output. The output may call it before the
/// engine knows the output format, so the engine installs the real state
/// through a one-element queue once `open` has returned.
pub(crate) struct Renderer {
    install: rtrb::Consumer<RenderState>,
    state: Option<RenderState>,
}

impl Renderer {
    pub(crate) fn new(install: rtrb::Consumer<RenderState>) -> Self {
        Self {
            install,
            state: None,
        }
    }
}

impl Render for Renderer {
    fn render(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        if self.state.is_none() {
            self.state = self.install.pop().ok();
        }
        if let Some(state) = self.state.as_mut() {
            state.render(out);
        }
    }
}

pub(crate) struct RenderState {
    shared: Arc<Shared>,
    notes: rtrb::Producer<Note>,
    /// Last consistent view of each slot's record.
    views: [Record; 2],
    /// `seq` of the record each slot held when the callback last left it, so
    /// a slot that already played is never entered again.
    played: [u64; 2],
    /// `seq` of the record that ended, so the end is reported once.
    ended: u64,
    gain: f32,
    /// The gain being ramped to, and the per-frame step that gets there in
    /// `ramp_frames`, fixed when the ramp starts.
    ramp_target: f32,
    ramp_step: f32,
    /// Whether the previous callback produced audio. A gain change ramps only
    /// then; otherwise it applies at once.
    audible: bool,
    ramp_frames: u64,
}

impl RenderState {
    pub(crate) fn new(shared: Arc<Shared>, notes: rtrb::Producer<Note>) -> Self {
        let ramp_frames = (u64::from(shared.info.sample_rate) * GAIN_RAMP_MS / 1000).max(1);
        Self {
            shared,
            notes,
            views: [Record::default(); 2],
            played: [0; 2],
            ended: 0,
            gain: 1.0,
            ramp_target: 1.0,
            ramp_step: 0.0,
            audible: false,
            ramp_frames,
        }
    }

    fn view(&mut self, slot: usize) -> Record {
        if let Some(record) = self.shared.slots[slot].record.try_load() {
            self.views[slot] = record;
        }
        self.views[slot]
    }

    fn render(&mut self, out: &mut [f32]) {
        let shared = Arc::clone(&self.shared);
        if !shared.playing.load(Ordering::Acquire) {
            self.audible = false;
            return;
        }
        let channels = usize::from(shared.info.channels);
        let frames = out.len() / channels;
        let mut done = 0;
        // Each pass either reads audio, or stops, or switches slots. Two
        // switches in one buffer need two empty Tracks in a row; the bound
        // only keeps a misbehaving control side from spinning the callback.
        let mut switches = 0;
        while done < frames && switches < 4 {
            let current = shared.current.load(Ordering::Acquire);
            let record = self.view(current);
            if record.token.is_none() || self.ended == record.seq {
                break;
            }
            let ring = &shared.slots[current].ring;
            // A restart publishes its record before it discards the old
            // audio. Seeing the record, drop that audio here rather than
            // play it under the new position.
            if ring.read_position() < record.start {
                ring.discard_to(record.start);
            }
            let limit = record
                .end
                .map_or(u64::MAX, |end| end.saturating_sub(ring.read_position()));
            let read = ring.read_into(&mut out[done * channels..frames * channels], limit);
            if read > 0 {
                done += read;
                if ring.readable() < ring.capacity() / 2 {
                    shared.wake_decoder();
                }
                continue;
            }
            let at_end = record.end.is_some_and(|end| ring.read_position() >= end);
            if !at_end {
                // Starved: the decoder is behind or waiting on the network.
                shared.wake_decoder();
                break;
            }
            if record.failed {
                // The decode thread already reported the failure; it stops
                // output now that the audio before it has played.
                self.ended = record.seq;
                let _ = self.notes.push(Note::Stopped { record });
                shared.wake_decoder();
                break;
            }
            switches += 1;
            let other = 1 - current;
            let next = self.view(other);
            if next.token.is_some() && !next.failed && next.seq != self.played[other] {
                if shared
                    .current
                    .compare_exchange(current, other, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    self.played[current] = record.seq;
                    shared.bump_sequence();
                    let _ = self.notes.push(Note::Transitioned {
                        from_slot: current,
                        from: record,
                        to: next,
                    });
                    shared.wake_decoder();
                }
                continue;
            }
            self.ended = record.seq;
            shared.ended.store(record.seq, Ordering::Release);
            shared.bump_sequence();
            let _ = self.notes.push(Note::Ended { record });
            shared.wake_decoder();
            break;
        }
        self.apply_gain(&mut out[..done * channels], channels);
        self.audible = done > 0;
    }

    fn apply_gain(&mut self, out: &mut [f32], channels: usize) {
        let target = f32::from_bits(self.shared.volume.load(Ordering::Acquire));
        if !self.audible {
            self.gain = target;
        }
        if self.gain == target {
            self.ramp_target = target;
            if target != 1.0 {
                out.iter_mut().for_each(|sample| *sample *= target);
            }
            return;
        }
        if self.ramp_target != target {
            self.ramp_target = target;
            self.ramp_step = (target - self.gain) / self.ramp_frames as f32;
        }
        for frame in out.chunks_exact_mut(channels) {
            if (target - self.gain).abs() <= self.ramp_step.abs() {
                self.gain = target;
            } else {
                self.gain += self.ramp_step;
            }
            frame.iter_mut().for_each(|sample| *sample *= self.gain);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TrackToken;
    use crate::output::OutputInfo;

    #[test]
    fn a_published_restart_never_plays_the_audio_it_replaces() {
        let shared = Arc::new(Shared::new(
            OutputInfo {
                sample_rate: 100,
                channels: 1,
            },
            16,
        ));
        let (notes, _notes_out) = rtrb::RingBuffer::new(NOTE_CAPACITY);
        let mut state = RenderState::new(Arc::clone(&shared), notes);
        let slot = &shared.slots[0];
        let record = |seq, start, start_frame| Record {
            seq,
            token: Some(TrackToken(1)),
            start,
            start_frame,
            end: None,
            failed: false,
        };
        slot.record.publish(record(1, 0, 0));
        slot.ring.write(&[1.0; 6]);
        shared.playing.store(true, Ordering::Release);
        let mut out = [0.0; 2];
        state.render(&mut out);
        assert_eq!(out, [1.0, 1.0]);

        // A seek published its record; the control side has not discarded
        // the old audio yet when the callback runs.
        slot.record.publish(record(2, 6, 500));
        slot.ring.write(&[2.0; 3]);
        let mut out = [0.0; 4];
        state.render(&mut out);
        assert_eq!(out, [2.0, 2.0, 2.0, 0.0]);
    }
}
