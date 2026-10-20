//! State shared between the control side, the decode thread and the render
//! callback.
//!
//! The engine has two slots. Each owns a ring and a [`Record`] saying which
//! Track the ring holds and where its audio starts and ends. The slot the
//! render callback reads is `current`; the other one holds the prepared next
//! Track. A Gapless Transition is the callback switching slots at the exact
//! frame where the current record ends.
//!
//! Only the control side and the decode thread publish records, always while
//! holding the feed lock, so a record cell has one writer at a time. The
//! render callback reads records through a sequence lock that never blocks
//! it: a torn read is retried a few times and otherwise the previous view is
//! kept for one more iteration.

use std::hint;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering, fence};
use std::thread::Thread;

use crate::TrackToken;
use crate::output::OutputInfo;
use crate::ring::Ring;

/// What a slot holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct Record {
    /// Unique per publication that changes the slot's audio (a new Track, a
    /// seek, a clear). Republishing to set `end` or `failed` keeps it.
    pub(crate) seq: u64,
    /// The Track in the slot, or `None` when the slot is empty.
    pub(crate) token: Option<TrackToken>,
    /// Ring position where this record's audio starts.
    pub(crate) start: u64,
    /// Track position, in output frames, of the audio at `start`.
    pub(crate) start_frame: u64,
    /// Ring position where this record's audio ends, once the stream ended.
    pub(crate) end: Option<u64>,
    /// The stream failed. Its audio up to `end` still plays, and playback
    /// stops there instead of moving to the next slot.
    pub(crate) failed: bool,
}

const NONE: u64 = u64::MAX;
const FLAG_OCCUPIED: u64 = 1;
const FLAG_FAILED: u64 = 2;

/// A sequence lock over a [`Record`], built from atomics only.
pub(crate) struct RecordCell {
    version: AtomicU64,
    seq: AtomicU64,
    flags: AtomicU64,
    token: AtomicU64,
    start: AtomicU64,
    start_frame: AtomicU64,
    end: AtomicU64,
}

impl Default for RecordCell {
    fn default() -> Self {
        Self {
            version: AtomicU64::new(0),
            seq: AtomicU64::new(0),
            flags: AtomicU64::new(0),
            token: AtomicU64::new(0),
            start: AtomicU64::new(0),
            start_frame: AtomicU64::new(0),
            end: AtomicU64::new(NONE),
        }
    }
}

impl RecordCell {
    /// Publishes `record`. The caller holds the feed lock (one writer).
    pub(crate) fn publish(&self, record: Record) {
        let version = self.version.load(Ordering::Relaxed);
        self.version
            .store(version.wrapping_add(1), Ordering::Relaxed);
        fence(Ordering::Release);
        let mut flags = 0;
        if record.token.is_some() {
            flags |= FLAG_OCCUPIED;
        }
        if record.failed {
            flags |= FLAG_FAILED;
        }
        self.seq.store(record.seq, Ordering::Relaxed);
        self.flags.store(flags, Ordering::Relaxed);
        self.token
            .store(record.token.map_or(0, |t| t.0), Ordering::Relaxed);
        self.start.store(record.start, Ordering::Relaxed);
        self.start_frame
            .store(record.start_frame, Ordering::Relaxed);
        self.end
            .store(record.end.unwrap_or(NONE), Ordering::Relaxed);
        self.version
            .store(version.wrapping_add(2), Ordering::Release);
    }

    /// Reads the record, or `None` when a publication is in progress.
    /// Real-time safe.
    pub(crate) fn try_load(&self) -> Option<Record> {
        for _ in 0..64 {
            let before = self.version.load(Ordering::Acquire);
            if before % 2 == 1 {
                hint::spin_loop();
                continue;
            }
            let seq = self.seq.load(Ordering::Relaxed);
            let flags = self.flags.load(Ordering::Relaxed);
            let token = self.token.load(Ordering::Relaxed);
            let start = self.start.load(Ordering::Relaxed);
            let start_frame = self.start_frame.load(Ordering::Relaxed);
            let end = self.end.load(Ordering::Relaxed);
            fence(Ordering::Acquire);
            if self.version.load(Ordering::Relaxed) != before {
                hint::spin_loop();
                continue;
            }
            return Some(Record {
                seq,
                token: (flags & FLAG_OCCUPIED != 0).then_some(TrackToken(token)),
                start,
                start_frame,
                end: (end != NONE).then_some(end),
                failed: flags & FLAG_FAILED != 0,
            });
        }
        None
    }
}

pub(crate) struct Slot {
    pub(crate) ring: Ring,
    pub(crate) record: RecordCell,
}

pub(crate) struct Shared {
    pub(crate) info: OutputInfo,
    pub(crate) slots: [Slot; 2],
    /// The slot the render callback reads. Changed by compare-exchange only:
    /// by the callback at a Gapless Transition, by the control side when
    /// promoting the prepared next Track.
    pub(crate) current: AtomicUsize,
    pub(crate) playing: AtomicBool,
    /// Target gain, as `f32` bits.
    pub(crate) volume: AtomicU32,
    /// `seq` of the record whose Track ended (playback reached its end with
    /// nothing to follow), 0 when none.
    pub(crate) ended: AtomicU64,
    /// Bumped on every discontinuity, so a reader can tell a position from
    /// before a seek or Track change from one after it.
    pub(crate) sequence: AtomicU64,
    wake_requested: AtomicBool,
    pub(crate) decoder: OnceLock<Thread>,
}

impl Shared {
    pub(crate) fn new(info: OutputInfo, ring_frames: usize) -> Self {
        let channels = usize::from(info.channels);
        let slot = || Slot {
            ring: Ring::new(ring_frames, channels),
            record: RecordCell::default(),
        };
        Self {
            info,
            slots: [slot(), slot()],
            current: AtomicUsize::new(0),
            playing: AtomicBool::new(false),
            volume: AtomicU32::new(1.0_f32.to_bits()),
            ended: AtomicU64::new(0),
            sequence: AtomicU64::new(0),
            wake_requested: AtomicBool::new(false),
            decoder: OnceLock::new(),
        }
    }

    /// Wakes the decode thread. Real-time safe: an atomic swap and, at most
    /// once per decode pass, an unpark.
    pub(crate) fn wake_decoder(&self) {
        if !self.wake_requested.swap(true, Ordering::AcqRel)
            && let Some(thread) = self.decoder.get()
        {
            thread.unpark();
        }
    }

    /// Called by the decode thread before it looks for work.
    pub(crate) fn clear_wake(&self) {
        self.wake_requested.store(false, Ordering::Release);
    }

    pub(crate) fn bump_sequence(&self) -> u64 {
        self.sequence.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub(crate) fn frames_to_ms(&self, frames: u64) -> u64 {
        frames.saturating_mul(1000) / u64::from(self.info.sample_rate)
    }

    pub(crate) fn ms_to_frames(&self, ms: u64) -> u64 {
        ms.saturating_mul(u64::from(self.info.sample_rate)) / 1000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_round_trips_through_the_cell() {
        let cell = RecordCell::default();
        assert_eq!(cell.try_load(), Some(Record::default()));
        let record = Record {
            seq: 7,
            token: Some(TrackToken(0)),
            start: 10,
            start_frame: 2000,
            end: Some(99),
            failed: true,
        };
        cell.publish(record);
        assert_eq!(cell.try_load(), Some(record));
    }
}
