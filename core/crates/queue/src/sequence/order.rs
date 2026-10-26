//! Starting and walking a sequence.

use std::collections::HashMap;

use ristle_domain::TrackRef;

use super::{Sequence, SequenceDescriptor, SequenceState, Slot, SlotId, shuffle};
use crate::error::InvalidSequence;
use crate::random::Random;

/// Where a sequence starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartAt {
    /// At the first slot of the play order: the first in natural order, or
    /// with shuffle the first of the permutation (Shuffle Play).
    Beginning,
    /// At this slot. Without shuffle the slots before it count as history;
    /// with shuffle every other slot follows it in shuffled order.
    Slot(SlotId),
}

impl Sequence {
    /// Starts a sequence over `slots`, given in natural order.
    pub fn start(
        descriptor: SequenceDescriptor,
        slots: Vec<Slot>,
        at: StartAt,
        shuffle: Option<u64>,
    ) -> Result<Self, InvalidSequence> {
        let last = slots
            .iter()
            .map(|slot| slot.id.0)
            .max()
            .ok_or(InvalidSequence::Empty)?;
        let next_slot = last.checked_add(1).ok_or(InvalidSequence::SlotsExhausted)?;
        let mut order: Vec<SlotId> = slots.iter().map(|slot| slot.id).collect();
        let cursor = match at {
            StartAt::Beginning => 0,
            StartAt::Slot(id) => order
                .iter()
                .position(|&slot| slot == id)
                .ok_or(InvalidSequence::UnknownStart(id))?,
        };
        if shuffle.is_some() {
            // The chosen slot plays first; every other slot follows.
            let first = order.remove(cursor);
            order.insert(0, first);
        }
        let mut sequence = Self::restore(SequenceState {
            descriptor,
            slots,
            order,
            cursor: Some(if shuffle.is_some() { 0 } else { cursor }),
            // Nothing is materialized yet, the current slot included.
            materialized_end: if shuffle.is_some() { 0 } else { cursor },
            shuffle: None,
            revision: 0,
            next_slot: SlotId(next_slot),
        })?;
        if let Some(seed) = shuffle {
            match at {
                StartAt::Beginning => {
                    sequence.state.cursor = None;
                    sequence.reorder_upcoming(Some(seed));
                    sequence.state.cursor = Some(0);
                }
                StartAt::Slot(_) => sequence.reorder_upcoming(Some(seed)),
            }
            sequence.state.shuffle = Some(seed);
            sequence.state.materialized_end = 0;
        }
        Ok(sequence)
    }

    /// Turns shuffle on with `seed`, or off with `None`. The current slot and
    /// the consumed prefix keep their places; only what is still to come is
    /// reordered. On, the seed shuffles the upcoming slots from their natural
    /// order, so a seed always gives the same order. Off, playback continues
    /// in natural order after the current slot, then wraps to the slots
    /// before it, so every slot still plays once in this pass.
    ///
    /// The lookahead must be materialized again: the materialized end falls
    /// back to just past the cursor (or to it, if the current slot was not
    /// materialized yet), and the revision changes, so plans made before are
    /// recognizably stale.
    pub fn set_shuffle(&mut self, seed: Option<u64>) {
        self.reorder_upcoming(seed);
        self.state.shuffle = seed;
        self.state.materialized_end = self.state.materialized_end.min(self.upcoming_start());
        self.state.revision += 1;
    }

    fn upcoming_start(&self) -> usize {
        self.state.cursor.map_or(0, |cursor| cursor + 1)
    }

    fn reorder_upcoming(&mut self, seed: Option<u64>) {
        let start = self.upcoming_start();
        let natural: HashMap<SlotId, usize> = self
            .state
            .slots
            .iter()
            .enumerate()
            .map(|(index, slot)| (slot.id, index))
            .collect();
        let rank = |id: &SlotId| natural.get(id).copied().unwrap_or(usize::MAX);
        let after = self
            .current()
            .map_or(0, |current| rank(&current).saturating_add(1));
        let upcoming = &mut self.state.order[start..];
        // Natural order continuing after the current slot, then wrapping.
        upcoming.sort_by_key(|id| {
            let rank = rank(id);
            (rank < after, rank)
        });
        if let Some(seed) = seed {
            let slots: HashMap<SlotId, &Slot> = self
                .state
                .slots
                .iter()
                .map(|slot| (slot.id, slot))
                .collect();
            shuffle::shuffle(upcoming, &slots, &mut Random::new(seed));
        }
    }

    /// The slot playing or last played.
    pub fn current(&self) -> Option<SlotId> {
        self.state
            .cursor
            .and_then(|cursor| self.state.order.get(cursor).copied())
    }

    /// A slot by id.
    pub fn slot(&self, id: SlotId) -> Option<&Slot> {
        self.state.slots.iter().find(|slot| slot.id == id)
    }

    fn move_cursor(&mut self, cursor: usize) {
        self.state.cursor = Some(cursor);
        // Slots passed over are behind the cursor; the one arrived at is
        // materialized only if it already was.
        self.state.materialized_end = self.state.materialized_end.max(cursor);
    }

    /// Moves to the next slot of the play order. Returns false at the end.
    pub fn advance(&mut self) -> bool {
        let next = self.upcoming_start();
        if next >= self.state.order.len() {
            return false;
        }
        self.move_cursor(next);
        true
    }

    /// Moves forward to `slot`, from the current slot on. Returns true
    /// without moving when `slot` is already current, as
    /// `CanonicalQueue::advance_to` does, and false when it is not ahead.
    pub fn advance_to(&mut self, slot: SlotId) -> bool {
        let from = self.state.cursor.unwrap_or(0);
        match self.state.order[from..].iter().position(|&id| id == slot) {
            Some(offset) => {
                self.move_cursor(from + offset);
                true
            }
            None => false,
        }
    }

    /// Moves to `slot` wherever it is in the play order, behind the cursor
    /// too, as queue navigation can.
    pub fn seek_to(&mut self, slot: SlotId) -> bool {
        match self.state.order.iter().position(|&id| id == slot) {
            Some(index) => {
                self.move_cursor(index);
                true
            }
            None => false,
        }
    }

    /// Moves to the previous slot of the play order.
    pub fn go_back(&mut self) -> bool {
        match self.state.cursor {
            Some(cursor) if cursor > 0 => {
                self.state.cursor = Some(cursor - 1);
                true
            }
            _ => false,
        }
    }

    /// Starts a new pass over the same order, for Repeat Queue. Nothing is
    /// current until the first slot is materialized and advanced to.
    pub fn restart(&mut self) {
        self.state.cursor = None;
        self.state.materialized_end = 0;
        self.state.revision += 1;
    }

    /// The bounded window around the cursor: up to `history` slots before
    /// it, the current slot and up to `lookahead` after it.
    pub fn window(&self, history: usize, lookahead: usize) -> Vec<SlotId> {
        let Some(cursor) = self.state.cursor else {
            return Vec::new();
        };
        let begin = cursor.saturating_sub(history);
        let end = (cursor + lookahead + 1).min(self.state.order.len());
        self.state.order[begin..end].to_vec()
    }

    /// The slots not yet materialized into the Canonical Queue from the
    /// current one up to `lookahead` slots past it (or from the start before
    /// there is a cursor). The current slot is included while it is not
    /// materialized. Unresolved slots among them are read first.
    pub fn hydration_plan(&self, lookahead: usize) -> Vec<SlotId> {
        let end = self
            .state
            .cursor
            .map_or(lookahead, |cursor| cursor + lookahead)
            .saturating_add(1)
            .min(self.state.order.len());
        let begin = self.state.materialized_end.min(end);
        self.state.order[begin..end].to_vec()
    }

    /// Up to `max` slots after the materialized end, whatever the cursor.
    /// Background refill walks on past slots that could not be
    /// materialized, so an unavailable stretch does not stall it.
    pub fn refill_batch(&self, max: usize) -> Vec<SlotId> {
        let begin = self.state.materialized_end;
        let end = begin.saturating_add(max).min(self.state.order.len());
        self.state.order[begin..end].to_vec()
    }

    /// Records that the play order is materialized up to `end` (exclusive).
    /// Returns false, changing nothing, when `end` is before the cursor or
    /// beyond the order.
    pub fn set_materialized_end(&mut self, end: usize) -> bool {
        if end < self.state.cursor.unwrap_or(0) || end > self.state.order.len() {
            return false;
        }
        self.state.materialized_end = end;
        true
    }

    /// Records the Tracks that reads found at unresolved slots. Slots that
    /// are unknown or already resolved are left as they are: a different
    /// Track at a resolved slot is drift, which re-basing handles.
    /// Returns how many slots were resolved.
    pub fn resolve(&mut self, resolved: Vec<(SlotId, TrackRef)>) -> usize {
        let mut tracks: HashMap<SlotId, TrackRef> = resolved.into_iter().collect();
        let mut count = 0;
        for slot in &mut self.state.slots {
            if slot.track.is_none()
                && let Some(track) = tracks.remove(&slot.id)
            {
                slot.track = Some(track);
                count += 1;
            }
        }
        count
    }
}
