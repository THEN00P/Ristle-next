//! Keeping an offset-addressed sequence true to a collection that changes
//! under it (`docs/plugin-api/walkthrough.md`, "Spotify Liked Songs:
//! positional handles", steps 3 and 4).
//!
//! The rule: a change never reshuffles. What the user can see (the
//! materialized lookahead, and everything resolved) keeps its place, and
//! changes are applied to what has not been decided yet.
//!
//! The limit, which is Spotify's: an unlike and a like made elsewhere
//! between two reads keep the total equal and shift positions, which is only
//! detected on pages read before. An unresolved slot may then land on a
//! different Track than it would have, which the user never saw, and the
//! duplicate rule below keeps it from repeating one.

use std::collections::HashMap;

use std::collections::HashSet;

use ristle_domain::{Addressing, CollectionDescriptor, Count, TrackRef};

use super::{Sequence, SequenceSource, Slot, SlotId};
use crate::random::Random;

/// A change to a sequence's collection made in Ristle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CollectionChange {
    /// `track` was added to source `source`, which puts it at position `at`
    /// (Spotify adds a like at 0).
    Added {
        /// Index into the descriptor's sources.
        source: usize,
        /// Where the Provider puts it.
        at: u64,
        /// The Track.
        track: TrackRef,
    },
    /// `track` was removed from source `source`.
    Removed {
        /// Index into the descriptor's sources.
        source: usize,
        /// The Track.
        track: TrackRef,
        /// Its position, when known. Otherwise the position of a slot that
        /// holds it is used.
        at: Option<u64>,
    },
}

/// What [`Sequence::apply_local_change`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeOutcome {
    /// The slot created for an added Track.
    pub inserted: Option<SlotId>,
    /// Slots taken out of what was still to come. Any of them that were
    /// materialized must leave the Canonical Queue too.
    pub removed: Vec<SlotId>,
}

/// One page of a source as it was just read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedPage {
    /// Index into the descriptor's sources.
    pub source: usize,
    /// The offset of the page's first entry.
    pub offset: u64,
    /// The Tracks at `offset`, `offset + 1`, and so on.
    pub tracks: Vec<TrackRef>,
    /// The collection's revision as the read reported it.
    pub revision: Option<String>,
    /// The collection's total as the read reported it.
    pub total: Option<Count>,
}

/// What [`Sequence::rebase`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RebaseOutcome {
    /// How far positions around the page moved, worked out from the Tracks
    /// already known on it. Zero when none were known.
    pub shift: i64,
    /// Unresolved slots that now hold a Track.
    pub resolved: Vec<SlotId>,
    /// Unresolved slots that turned out to repeat a Track another slot
    /// already holds (one played in this pass, say), taken out of the order.
    pub dropped: Vec<SlotId>,
    /// Slots created for Tracks new to the collection, such as likes made
    /// elsewhere.
    pub inserted: Vec<SlotId>,
}

impl Sequence {
    /// Applies a like or unlike made in Ristle without voiding the order.
    ///
    /// An added Track moves every position at or after it down by one and
    /// joins what is still to come: at a random point when shuffled, else at
    /// its natural place, never inside the materialized lookahead. A removed
    /// Track leaves what is still to come, and the positions behind it move
    /// up by one. Nothing resolved moves in the order, and nothing
    /// reshuffles.
    pub fn apply_local_change(&mut self, change: CollectionChange) -> ChangeOutcome {
        let mut outcome = ChangeOutcome::default();
        let mut shifted = false;
        match change {
            CollectionChange::Added { source, at, track } => {
                if source >= self.state.descriptor.sources.len() {
                    return outcome;
                }
                self.shift_offsets(source, at, 1);
                shifted = true;
                outcome.inserted = self.insert_slot(source, at, Some(track));
            }
            CollectionChange::Removed { source, track, at } => {
                let known = at.or_else(|| {
                    self.state
                        .slots
                        .iter()
                        .find(|slot| slot.source == source && slot.track.as_ref() == Some(&track))
                        .map(|slot| slot.offset)
                });
                outcome.removed = self.remove_upcoming(|slot| {
                    slot.source == source && slot.track.as_ref() == Some(&track)
                });
                // No slot knew the Track: the one still standing for its
                // position is the one that goes.
                if outcome.removed.is_empty()
                    && let Some(at) = at
                {
                    outcome.removed = self.remove_upcoming(|slot| {
                        slot.source == source && slot.track.is_none() && slot.offset == at
                    });
                }
                if let Some(position) = known {
                    self.shift_offsets(source, position.saturating_add(1), -1);
                    shifted = true;
                }
            }
        }
        // Moved offsets make plans made before stale too.
        if outcome.inserted.is_some() || !outcome.removed.is_empty() || shifted {
            self.state.revision = self.state.revision.wrapping_add(1);
        }
        outcome
    }

    /// Re-bases the sequence on a page just read, after the collection may
    /// have changed elsewhere.
    ///
    /// The Tracks already known on the page tell how far positions moved
    /// (the most common move among them). Other slots from there on are
    /// moved by that much, and those that land on the page take its Tracks.
    /// A Track that turns up at a slot while another slot already holds it,
    /// more often than the page shows it, is a repeat caused by the drift
    /// and that slot is dropped; this is how a Track played in this pass is
    /// not played again. Page entries no slot covers that hold a Track no
    /// slot holds are new to the collection and join what is still to come
    /// as in [`Sequence::apply_local_change`]. Unresolved slots that end up
    /// on a resolved slot's position, or past an exact total, no longer
    /// stand for anything and are dropped too.
    pub fn rebase(&mut self, page: ObservedPage) -> RebaseOutcome {
        let mut outcome = RebaseOutcome::default();
        let source = page.source;
        // A page whose positions do not fit is malformed: ignore it whole.
        if u64::try_from(page.tracks.len())
            .ok()
            .and_then(|len| page.offset.checked_add(len))
            .is_none()
        {
            return outcome;
        }
        let Some(SequenceSource::Collection(descriptor)) =
            self.state.descriptor.sources.get_mut(source)
        else {
            return outcome;
        };
        let positional = descriptor.addressing == Addressing::Offset;
        let CollectionDescriptor {
            revision, total, ..
        } = descriptor;
        if page.revision.is_some() {
            revision.clone_from(&page.revision);
        }
        if page.total.is_some() {
            *total = page.total;
        }

        let position = |index: usize| page.offset + index as u64;
        let mut on_page: HashMap<&TrackRef, Vec<u64>> = HashMap::new();
        for (index, track) in page.tracks.iter().enumerate() {
            on_page.entry(track).or_default().push(position(index));
        }

        // 1. How far did the known Tracks move?
        let mut moves: Vec<(usize, u64, i64)> = Vec::new();
        for (index, slot) in self.state.slots.iter().enumerate() {
            let Some(track) = slot.track.as_ref().filter(|_| slot.source == source) else {
                continue;
            };
            if let Some(seen) = on_page.get(track)
                && let Some(&now) = seen.iter().min_by_key(|now| now.abs_diff(slot.offset))
            {
                moves.push((index, slot.offset, signed(now) - signed(slot.offset)));
            }
        }
        let shift = most_common(moves.iter().map(|&(_, _, delta)| delta));
        let from = moves
            .iter()
            .filter(|&&(_, _, delta)| delta == shift)
            .map(|&(_, old, _)| old)
            .min();
        // Every other slot from there on moved by the same amount, resolved
        // or not; the known ones are where the page saw them.
        let matched: HashSet<usize> = moves.iter().map(|&(index, _, _)| index).collect();
        if let Some(from) = from
            && shift != 0
        {
            for (index, slot) in self.state.slots.iter_mut().enumerate() {
                if slot.source == source && slot.offset >= from && !matched.contains(&index) {
                    slot.offset = apply(slot.offset, shift);
                }
            }
        }
        for &(index, old, delta) in &moves {
            self.state.slots[index].offset = apply(old, delta);
        }
        outcome.shift = shift;

        // 2. Resolve what landed on the page; drop repeats; add what is new.
        let current = self.current();
        for (index, track) in page.tracks.iter().enumerate() {
            let at = position(index);
            let here = self
                .state
                .slots
                .iter()
                .filter(|slot| slot.source == source && slot.offset == at)
                .map(|slot| (slot.id, slot.track.is_some()))
                .collect::<Vec<_>>();
            let held = self
                .state
                .slots
                .iter()
                .filter(|slot| slot.source == source && slot.track.as_ref() == Some(track))
                .count();
            let shown = on_page.get(track).map_or(0, Vec::len);
            let repeat = held >= shown;
            if here.iter().any(|&(_, resolved)| resolved) {
                // A resolved slot keeps its Track, which the user may have
                // seen; unresolved ones moved here are surplus.
                for &(id, resolved) in &here {
                    if !resolved && Some(id) != current {
                        self.drop_slot(id);
                        outcome.dropped.push(id);
                    }
                }
                // A different Track now at this position, which no slot
                // holds, is new to the collection (an unlike and a like made
                // elsewhere): it joins what is still to come.
                if !repeat {
                    outcome
                        .inserted
                        .extend(self.insert_slot(source, at, Some(track.clone())));
                }
                continue;
            }
            match here.first() {
                Some(&(id, _)) if repeat && Some(id) != current => {
                    self.drop_slot(id);
                    outcome.dropped.push(id);
                }
                Some(&(id, _)) => {
                    if let Some(slot) = self.state.slots.iter_mut().find(|slot| slot.id == id) {
                        slot.track = Some(track.clone());
                    }
                    outcome.resolved.push(id);
                }
                None if !repeat => {
                    outcome
                        .inserted
                        .extend(self.insert_slot(source, at, Some(track.clone())));
                }
                None => {}
            }
        }

        // A collection that grew needs a slot for every position: the
        // positions no slot covers get unresolved slots, read later like any
        // other. The repeat rule settles any Track they turn out to repeat.
        if positional && let Some(Count::Exact(total)) = page.total {
            let covered: HashSet<u64> = self
                .state
                .slots
                .iter()
                .filter(|slot| slot.source == source)
                .map(|slot| slot.offset)
                .collect();
            let held = u64::try_from(covered.len()).unwrap_or(u64::MAX);
            let missing = total.saturating_sub(held);
            let free: Vec<u64> = (0..total)
                .filter(|offset| !covered.contains(offset))
                .take(usize::try_from(missing).unwrap_or(usize::MAX))
                .collect();
            for offset in free {
                outcome
                    .inserted
                    .extend(self.insert_slot(source, offset, None));
            }
        }

        // Positions past an exact end no longer exist.
        if let Some(Count::Exact(total)) = page.total {
            let beyond: Vec<SlotId> = self
                .state
                .slots
                .iter()
                .filter(|slot| {
                    slot.source == source
                        && slot.track.is_none()
                        && slot.offset >= total
                        && Some(slot.id) != current
                })
                .map(|slot| slot.id)
                .collect();
            for id in beyond {
                self.drop_slot(id);
                outcome.dropped.push(id);
            }
        }

        if shift != 0
            || !outcome.resolved.is_empty()
            || !outcome.dropped.is_empty()
            || !outcome.inserted.is_empty()
        {
            self.state.revision = self.state.revision.wrapping_add(1);
        }
        outcome
    }

    /// Moves the offsets of `source`'s slots at or after `from` by `by`.
    fn shift_offsets(&mut self, source: usize, from: u64, by: i64) {
        for slot in &mut self.state.slots {
            if slot.source == source && slot.offset >= from {
                slot.offset = apply(slot.offset, by);
            }
        }
    }

    /// Creates a slot for an entry new to `source` at `offset`, resolved when
    /// its Track is known, and puts it among what is still to come, after
    /// the materialized lookahead. `None`, changing nothing, once slot ids are exhausted.
    fn insert_slot(
        &mut self,
        source: usize,
        offset: u64,
        track: Option<TrackRef>,
    ) -> Option<SlotId> {
        let id = self.state.next_slot;
        self.state.next_slot = SlotId(id.0.checked_add(1)?);

        // Natural order: before the first slot of the source that sits
        // after it, else after the source's last slot.
        let slots = &self.state.slots;
        let natural = slots
            .iter()
            .position(|slot| slot.source == source && slot.offset > offset)
            .or_else(|| {
                slots
                    .iter()
                    .rposition(|slot| slot.source == source)
                    .map(|last| last + 1)
            })
            .unwrap_or(slots.len());
        self.state.slots.insert(
            natural,
            Slot {
                id,
                source,
                offset,
                track,
            },
        );

        let earliest = self.state.materialized_end.max(self.upcoming_start());
        let len = self.state.order.len();
        let at = match self.state.shuffle {
            Some(seed) => {
                let mut random = Random::new(seed ^ id.0.rotate_left(32) ^ self.state.revision);
                earliest + random.below((len - earliest) as u64 + 1) as usize
            }
            None => {
                let rank: HashMap<SlotId, usize> = self
                    .state
                    .slots
                    .iter()
                    .enumerate()
                    .map(|(index, slot)| (slot.id, index))
                    .collect();
                let after = self
                    .current()
                    .and_then(|current| rank.get(&current))
                    .map_or(0, |&rank| rank + 1);
                let key = |slot: &SlotId| {
                    let rank = rank.get(slot).copied().unwrap_or(usize::MAX);
                    (rank < after, rank)
                };
                let new = key(&id);
                self.state.order[earliest..]
                    .iter()
                    .position(|slot| key(slot) > new)
                    .map_or(len, |offset| earliest + offset)
            }
        };
        self.state.order.insert(at, id);
        Some(id)
    }

    /// Takes the slots still to come that match `doomed` out of the order
    /// and the slots.
    fn remove_upcoming(&mut self, doomed: impl Fn(&Slot) -> bool) -> Vec<SlotId> {
        let start = self.upcoming_start();
        let doomed: Vec<SlotId> = self.state.order[start..]
            .iter()
            .copied()
            .filter(|id| self.slot(*id).is_some_and(&doomed))
            .collect();
        for &id in &doomed {
            self.drop_slot(id);
        }
        doomed
    }

    /// Removes one slot from the order and the slots, keeping the cursor
    /// and the materialized end on the same slots.
    fn drop_slot(&mut self, id: SlotId) {
        if let Some(index) = self.state.order.iter().position(|&slot| slot == id) {
            self.state.order.remove(index);
            if index < self.state.materialized_end {
                self.state.materialized_end -= 1;
            }
            if let Some(cursor) = self.state.cursor
                && index < cursor
            {
                self.state.cursor = Some(cursor - 1);
            }
        }
        self.state.slots.retain(|slot| slot.id != id);
    }
}

fn signed(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn apply(offset: u64, by: i64) -> u64 {
    offset.saturating_add_signed(by)
}

/// The most frequent value, preferring the smaller move on a tie; zero for
/// none.
fn most_common(values: impl Iterator<Item = i64>) -> i64 {
    let mut counts: HashMap<i64, usize> = HashMap::new();
    for value in values {
        *counts.entry(value).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by(|(a, count_a), (b, count_b)| {
            count_a
                .cmp(count_b)
                .then_with(|| b.unsigned_abs().cmp(&a.unsigned_abs()))
                .then_with(|| b.cmp(a))
        })
        .map_or(0, |(value, _)| value)
}
