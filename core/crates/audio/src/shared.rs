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
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering, fence};
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
/// The `current` word: bit 0 is the slot, bit 1 is `HELD`, and the bits
/// above are the generation (`GENERATION` per step).
const SLOT: u64 = 1;
const HELD: u64 = 2;
const GENERATION: u64 = 4;
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

/// The control side's pin on the current slot; see [`Shared::hold`].
pub(crate) struct Hold<'a> {
    shared: &'a Shared,
    slot: usize,
}

impl Hold<'_> {
    /// The current slot. It cannot change while held, except through
    /// [`Hold::switch`].
    pub(crate) fn current(&self) -> usize {
        self.slot
    }

    pub(crate) fn other(&self) -> usize {
        1 - self.slot
    }

    /// Makes the other slot current (promoting the prepared next Track).
    pub(crate) fn switch(&mut self) {
        self.slot = 1 - self.slot;
        self.shared.current.fetch_xor(SLOT, Ordering::AcqRel);
    }
}

impl Drop for Hold<'_> {
    /// Releases the slot and starts a new generation, so a callback that
    /// read the records before or during the hold cannot switch on them.
    fn drop(&mut self) {
        // HELD is set, so adding HELD clears it and carries one generation.
        self.shared.current.fetch_add(HELD, Ordering::AcqRel);
    }
}

/// The slot in a `current` word.
pub(crate) fn slot_of(word: u64) -> usize {
    usize::from(word & SLOT == 1)
}

/// The generation in a `current` word.
pub(crate) fn generation_of(word: u64) -> u64 {
    word / GENERATION
}

pub(crate) struct Slot {
    pub(crate) ring: Ring,
    pub(crate) record: RecordCell,
}

pub(crate) struct Shared {
    pub(crate) info: OutputInfo,
    pub(crate) slots: [Slot; 2],
    /// Bit 0: the slot the render callback reads. Bit 1 (`HELD`): the
    /// control side has pinned it. Above: a generation, advanced by every
    /// control command (when its hold is released), every Gapless Transition
    /// and every end of playback. The callback switches slots by
    /// compare-exchange from the unheld word it read before its record
    /// views, so it never switches on views a command has since replaced,
    /// and the generation is the state's sequence number.
    current: AtomicU64,
    /// Per slot, the `seq` of the record the callback finished there, so
    /// the control side never treats a finished Track as the next one.
    played: [AtomicU64; 2],
    pub(crate) playing: AtomicBool,
    /// Target gain, as `f32` bits.
    pub(crate) volume: AtomicU32,
    /// `seq` of the record whose Track ended (playback reached its end with
    /// nothing to follow), 0 when none.
    pub(crate) ended: AtomicU64,
    /// `seq` of the failed record whose audio has played out, 0 when none.
    pub(crate) stopped: AtomicU64,
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
            current: AtomicU64::new(0),
            played: [AtomicU64::new(0), AtomicU64::new(0)],
            playing: AtomicBool::new(false),
            volume: AtomicU32::new(1.0_f32.to_bits()),
            ended: AtomicU64::new(0),
            stopped: AtomicU64::new(0),
            wake_requested: AtomicBool::new(false),
            decoder: OnceLock::new(),
        }
    }

    /// The `current` word; see [`slot_of`] and [`generation_of`].
    pub(crate) fn word(&self) -> u64 {
        self.current.load(Ordering::Acquire)
    }

    /// The slot the render callback reads.
    pub(crate) fn current(&self) -> usize {
        slot_of(self.word())
    }

    /// Pins the current slot until the returned guard drops. Call with the
    /// feed lock held, so there is one holder at a time.
    pub(crate) fn hold(&self) -> Hold<'_> {
        let slot = slot_of(self.current.fetch_or(HELD, Ordering::AcqRel));
        Hold { shared: self, slot }
    }

    /// The callback's Gapless Transition: switches to the other slot if the
    /// word is still `seen`, unheld. Fails when a command held or released
    /// the slot since, or the slot was already switched.
    pub(crate) fn try_switch(&self, seen: u64) -> bool {
        seen & HELD == 0
            && self
                .current
                .compare_exchange(
                    seen,
                    (seen ^ SLOT) + GENERATION,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
    }

    /// Starts a new generation without a command (the end of playback).
    pub(crate) fn advance_generation(&self) {
        self.current.fetch_add(GENERATION, Ordering::AcqRel);
    }

    /// Records that the callback finished `seq` in `slot`. The callback
    /// calls it before `try_switch`, so a holder that sees the switch also
    /// sees the finished Track as played.
    pub(crate) fn mark_played(&self, slot: usize, seq: u64) {
        self.played[slot].store(seq, Ordering::Release);
    }

    /// Whether `seq` in `slot` already played to its end.
    pub(crate) fn has_played(&self, slot: usize, seq: u64) -> bool {
        seq != 0 && self.played[slot].load(Ordering::Acquire) == seq
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
    fn a_switch_needs_an_unheld_word_from_before_every_command() {
        let shared = Shared::new(
            OutputInfo {
                sample_rate: 100,
                channels: 1,
            },
            4,
        );
        let seen = shared.word();
        {
            let mut slots = shared.hold();
            assert!(!shared.try_switch(seen));
            assert!(!shared.try_switch(shared.word()));
            slots.switch();
            assert_eq!(shared.current(), 1);
        }
        // The callback's views from before the command are stale.
        assert!(!shared.try_switch(seen));
        let now = shared.word();
        assert_eq!(generation_of(now), generation_of(seen) + 1);
        assert!(shared.try_switch(now));
        assert_eq!(shared.current(), 0);
        assert_eq!(generation_of(shared.word()), generation_of(seen) + 2);
        shared.advance_generation();
        assert_eq!(slot_of(shared.word()), 0);
        assert_eq!(generation_of(shared.word()), generation_of(seen) + 3);
    }

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
