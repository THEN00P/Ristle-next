//! The Canonical Queue's operations.

use ristle_domain::{Track, TrackRef};

use super::{CanonicalQueue, Current};
use crate::entry::{
    Advance, CollectionPosition, EntryId, Origin, QueueEntry, RepeatMode, Submitter,
};
use crate::random::Random;
use crate::sequence::SlotId;

/// One entry of a collection being started, appended or reconciled.
#[derive(Debug, Clone, PartialEq)]
pub struct CollectionEntry {
    /// The Track.
    pub track: Track,
    /// Where it sits in its collection, counting Disabled Tracks too.
    pub position: CollectionPosition,
    /// The sequence slot it materializes, when the collection is walked as a
    /// sequence.
    pub slot: Option<SlotId>,
}

impl CollectionEntry {
    /// An entry at `position`, outside any sequence.
    pub fn new(track: Track, position: u64) -> Self {
        Self {
            track,
            position: CollectionPosition(position),
            slot: None,
        }
    }

    /// Entries for `tracks` in order, numbered from `first`.
    pub fn numbered(tracks: impl IntoIterator<Item = Track>, first: u64) -> Vec<Self> {
        (first..)
            .zip(tracks)
            .map(|(position, track)| Self::new(track, position))
            .collect()
    }
}

/// What [`CanonicalQueue::start_collection`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartOutcome {
    /// Entries that can play.
    pub accepted: usize,
    /// Disabled Tracks, left out silently.
    pub skipped: usize,
    /// Whether the collection started. When it did not, because no entry at
    /// or after the start can play, the queue is unchanged.
    pub started: bool,
}

/// What an operation that adds several entries did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AddOutcome {
    /// Entries added.
    pub added: usize,
    /// Entries left out: Disabled Tracks, and entries the collection already
    /// holds.
    pub skipped: usize,
}

/// Why a Track was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Rejection {
    /// A Disabled Track cannot enter the Canonical Queue.
    #[error("the Track is unavailable")]
    Unavailable,
}

/// Which part of the queue a removed entry came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    /// The request lane.
    Requests,
    /// The upcoming part of the collection.
    Collection,
}

/// An entry taken out by [`CanonicalQueue::remove`], kept so that
/// [`CanonicalQueue::undo_remove`] can put it back where it was.
#[derive(Debug, Clone, PartialEq)]
pub struct Removed {
    entry: QueueEntry,
    lane: Lane,
    index: usize,
    play_next: bool,
}

impl Removed {
    /// The entry that was removed.
    pub fn entry(&self) -> &QueueEntry {
        &self.entry
    }

    /// Where it was.
    pub fn lane(&self) -> Lane {
        self.lane
    }
}

impl CanonicalQueue {
    fn create(
        &mut self,
        track: Track,
        origin: Origin,
        slot: Option<SlotId>,
        submitter: Option<Submitter>,
    ) -> QueueEntry {
        let id = self.state.next_id;
        self.state.next_id = EntryId(id.0 + 1);
        QueueEntry {
            id,
            track,
            origin,
            slot,
            submitter,
        }
    }

    fn create_from(&mut self, entry: CollectionEntry) -> QueueEntry {
        self.create(
            entry.track,
            Origin::Collection {
                position: entry.position,
            },
            entry.slot,
            None,
        )
    }

    /// Index of the first collection entry after the cursor.
    fn upcoming_start(&self) -> usize {
        self.state
            .cursor
            .map_or(0, |cursor| (cursor + 1).min(self.state.collection.len()))
    }

    fn contains(&self, id: EntryId) -> bool {
        self.current().is_some_and(|entry| entry.id == id)
            || self.state.collection.iter().any(|entry| entry.id == id)
            || self.state.requests.iter().any(|entry| entry.id == id)
    }

    /// Replaces the collection and plays the first entry that can play at or
    /// after index `start` of `entries`. Disabled Tracks are left out
    /// silently. History, the request lane and the shuffle are cleared; the
    /// repeat mode stays.
    ///
    /// When nothing at or after `start` can play, the queue is left as it
    /// was, so that a collection that cannot start never stops the one that
    /// is playing.
    pub fn start_collection(
        &mut self,
        entries: Vec<CollectionEntry>,
        start: usize,
    ) -> StartOutcome {
        let skipped = entries
            .iter()
            .filter(|entry| !entry.track.is_queueable())
            .count();
        let accepted = entries.len() - skipped;
        let first = entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.track.is_queueable())
            .position(|(index, _)| index >= start);
        let Some(cursor) = first else {
            return StartOutcome {
                accepted,
                skipped,
                started: false,
            };
        };

        self.state.collection.clear();
        self.state.requests.clear();
        self.state.play_next_count = 0;
        self.state.shuffle = None;
        for entry in entries
            .into_iter()
            .filter(|entry| entry.track.is_queueable())
        {
            let entry = self.create_from(entry);
            self.state.collection.push(entry);
        }
        self.state.cursor = Some(cursor);
        self.state.current = Some(Current::AtCursor);
        StartOutcome {
            accepted,
            skipped,
            started: true,
        }
    }

    /// Adds a Queued Request at the end of the request lane.
    pub fn add_to_queue(
        &mut self,
        track: Track,
        by: Option<Submitter>,
    ) -> Result<EntryId, Rejection> {
        if !track.is_queueable() {
            return Err(Rejection::Unavailable);
        }
        let entry = self.create(track, Origin::Request, None, by);
        let id = entry.id;
        self.state.requests.push(entry);
        Ok(id)
    }

    /// Adds Queued Requests in order, leaving out Disabled Tracks.
    pub fn add_all_to_queue(&mut self, tracks: Vec<Track>, by: Option<Submitter>) -> AddOutcome {
        let mut outcome = AddOutcome::default();
        for track in tracks {
            match self.add_to_queue(track, by.clone()) {
                Ok(_) => outcome.added += 1,
                Err(Rejection::Unavailable) => outcome.skipped += 1,
            }
        }
        outcome
    }

    /// Adds a Queued Request after the earlier Play Next requests and before
    /// every Add to queue request.
    pub fn play_next(&mut self, track: Track, by: Option<Submitter>) -> Result<EntryId, Rejection> {
        if !track.is_queueable() {
            return Err(Rejection::Unavailable);
        }
        let entry = self.create(track, Origin::Request, None, by);
        let id = entry.id;
        self.state
            .requests
            .insert(self.state.play_next_count, entry);
        self.state.play_next_count += 1;
        Ok(id)
    }

    /// Appends the next page of the collection.
    ///
    /// An entry the collection already holds is skipped: with a slot, the
    /// same slot; without one, the same Track identity, because a Provider's
    /// pages can overlap when the list changes between reads. Queued Requests
    /// kept in the collection as history never count, so a Track's real
    /// occurrence on a later page is not lost because it was also requested.
    pub fn append_collection(&mut self, entries: Vec<CollectionEntry>) -> AddOutcome {
        let mut outcome = AddOutcome::default();
        for entry in entries {
            let held = self.state.collection.iter().any(|held| {
                matches!(held.origin, Origin::Collection { .. })
                    && match entry.slot {
                        Some(slot) => held.slot == Some(slot),
                        None => held.track.reference == entry.track.reference,
                    }
            });
            if !entry.track.is_queueable() || held {
                outcome.skipped += 1;
                continue;
            }
            let entry = self.create_from(entry);
            self.state.collection.push(entry);
            outcome.added += 1;
        }
        outcome
    }

    /// Reconciles the upcoming collection with an edited version of the
    /// whole collection, keeping the current entry, history and Queued
    /// Requests.
    ///
    /// The anchor is the current collection entry or, while a request kept
    /// as history plays, the collection entry that follows it. Playback
    /// resumes after the anchor's occurrence in `entries`: among the
    /// occurrences of its Track, the one whose position is nearest the
    /// anchor's, so a duplicate anywhere else in the collection is never
    /// mistaken for it (legacy trap 6). When the anchor's Track is gone, the
    /// whole edited collection follows, and a removed continuation is
    /// dropped.
    pub fn replace_upcoming_collection(&mut self, entries: Vec<CollectionEntry>) -> AddOutcome {
        let mut outcome = AddOutcome::default();
        let entries: Vec<_> = entries
            .into_iter()
            .filter(|entry| {
                let queueable = entry.track.is_queueable();
                outcome.skipped += usize::from(!queueable);
                queueable
            })
            .collect();

        let start = self.upcoming_start();
        let collection = &self.state.collection;
        let (anchor, continuation) = match self.state.cursor {
            Some(cursor) if matches!(collection[cursor].origin, Origin::Collection { .. }) => {
                (Some(cursor), false)
            }
            Some(_) => (
                collection[start..]
                    .iter()
                    .position(|entry| matches!(entry.origin, Origin::Collection { .. }))
                    .map(|offset| start + offset),
                true,
            ),
            None => (None, false),
        };
        let matched = anchor.and_then(|anchor| {
            let anchor = &collection[anchor];
            let Origin::Collection { position } = anchor.origin else {
                return None;
            };
            entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.track.reference == anchor.track.reference)
                .min_by_key(|(_, entry)| entry.position.0.abs_diff(position.0))
                .map(|(index, entry)| (index, entry.position))
        });

        let mut tail = self.state.collection.split_off(start);
        let kept_continuation = match (anchor, matched) {
            (Some(anchor), Some((_, position))) if continuation => {
                let mut entry = tail.remove(anchor - start);
                entry.origin = Origin::Collection { position };
                Some(entry)
            }
            (Some(anchor), Some((_, position))) => {
                self.state.collection[anchor].origin = Origin::Collection { position };
                None
            }
            _ => None,
        };
        tail.retain(|entry| entry.origin == Origin::Request);
        self.state.collection.append(&mut tail);
        self.state.collection.extend(kept_continuation);

        let resume = matched.map_or(0, |(index, _)| index + 1);
        let first_new = self.state.collection.len();
        for entry in entries.into_iter().skip(resume) {
            let entry = self.create_from(entry);
            self.state.collection.push(entry);
            outcome.added += 1;
        }
        // While shuffled, the new tail is shuffled too, with the same seed.
        // What stayed in place (requests, the continuation) keeps its place.
        if let Some(seed) = self.state.shuffle {
            Random::new(seed).shuffle(&mut self.state.collection[first_new..]);
        }
        outcome
    }

    /// Moves to the next entry: the next Queued Request, else the next
    /// collection entry, wrapping with [`RepeatMode::Queue`]. A Track that
    /// ended plays again with [`RepeatMode::Track`]; a skip leaves it.
    /// Returns whether an entry is now playing.
    pub fn advance(&mut self, how: Advance) -> bool {
        if how == Advance::TrackEnded
            && self.state.repeat == RepeatMode::Track
            && self.state.current.is_some()
        {
            return true;
        }
        self.start_next_request() || self.start_next_collection_entry()
    }

    fn start_next_request(&mut self) -> bool {
        if self.state.requests.is_empty() {
            return false;
        }
        let entry = self.state.requests.remove(0);
        self.state.play_next_count = self.state.play_next_count.saturating_sub(1);
        self.state.current = Some(Current::Detached(Box::new(entry)));
        true
    }

    fn start_next_collection_entry(&mut self) -> bool {
        let len = self.state.collection.len();
        let next = match self.state.cursor {
            _ if len == 0 => None,
            None => Some(0),
            Some(cursor) if cursor + 1 < len => Some(cursor + 1),
            Some(_) if self.state.repeat == RepeatMode::Queue => {
                // Requests kept as history played once; the collection
                // repeats without them.
                self.state
                    .collection
                    .retain(|entry| entry.origin != Origin::Request);
                (!self.state.collection.is_empty()).then_some(0)
            }
            Some(_) => {
                self.state.current = None;
                return false;
            }
        };
        match next {
            Some(cursor) => {
                self.state.cursor = Some(cursor);
                self.state.current = Some(Current::AtCursor);
                true
            }
            None => {
                self.state.cursor = None;
                self.state.current = None;
                false
            }
        }
    }

    /// Jumps straight to an upcoming entry.
    ///
    /// Queued Requests passed on the way, and a request that was playing,
    /// are kept in the collection before the target as history, so Previous
    /// reaches them; requests behind the target keep their place. Returns
    /// whether the target is now playing.
    pub fn advance_to(&mut self, target: EntryId) -> bool {
        if self.current().is_some_and(|entry| entry.id == target) {
            return true;
        }
        let Some(steps) = self
            .upcoming()
            .iter()
            .position(|entry| entry.id == target)
            .map(|index| index + 1)
        else {
            return false;
        };

        let queued = self
            .state
            .requests
            .iter()
            .position(|entry| entry.id == target);
        let consumed = queued.map_or(self.state.requests.len(), |index| index + 1);
        let detached = matches!(self.state.current, Some(Current::Detached(_)));
        if detached || consumed > 0 {
            let mut history = Vec::with_capacity(consumed + 1);
            if let Some(Current::Detached(entry)) = self.state.current.take() {
                history.push(*entry);
            }
            history.extend(self.state.requests.drain(..consumed));
            self.state.play_next_count = self.state.play_next_count.saturating_sub(consumed);
            // A target behind the cursor is reached by wrapping: the passed
            // requests then precede the restarted collection.
            let wraps = queued.is_none()
                && self.state.repeat == RepeatMode::Queue
                && self
                    .state
                    .cursor
                    .is_some_and(|cursor| cursor + 1 >= self.state.collection.len());
            let at = if wraps { 0 } else { self.upcoming_start() };
            self.state.collection.splice(at..at, history);
            let Some(cursor) = self
                .state
                .collection
                .iter()
                .position(|entry| entry.id == target)
            else {
                return false;
            };
            self.state.cursor = Some(cursor);
            self.state.current = Some(Current::AtCursor);
            return true;
        }

        for _ in 0..steps {
            if !self.advance(Advance::Skip) {
                return false;
            }
        }
        self.current().is_some_and(|entry| entry.id == target)
    }

    /// Goes back to the entry that played before the current one. A request
    /// that was playing returns to the front of the request lane, so Next
    /// plays it again. After playback ended, the last entry plays again.
    /// Returns false when there is nothing earlier.
    pub fn go_back(&mut self) -> bool {
        let Some(cursor) = self.state.cursor else {
            return false;
        };
        match self.state.current.take() {
            Some(Current::Detached(entry)) => {
                self.state.requests.insert(0, *entry);
                self.state.play_next_count += 1;
            }
            None => {}
            Some(Current::AtCursor) if cursor > 0 => self.state.cursor = Some(cursor - 1),
            Some(Current::AtCursor) => {
                self.state.current = Some(Current::AtCursor);
                return false;
            }
        }
        self.state.current = Some(Current::AtCursor);
        true
    }

    /// Takes an upcoming entry out of the queue. The current entry and
    /// history cannot be removed.
    pub fn remove(&mut self, entry: EntryId) -> Option<Removed> {
        if let Some(index) = self.state.requests.iter().position(|e| e.id == entry) {
            let play_next = index < self.state.play_next_count;
            if play_next {
                self.state.play_next_count -= 1;
            }
            return Some(Removed {
                entry: self.state.requests.remove(index),
                lane: Lane::Requests,
                index,
                play_next,
            });
        }
        let index = self.state.collection.iter().position(|e| e.id == entry)?;
        if index < self.upcoming_start() {
            return None;
        }
        Some(Removed {
            entry: self.state.collection.remove(index),
            lane: Lane::Collection,
            index,
            play_next: false,
        })
    }

    /// Puts a removed entry back where it was, or as close as the queue
    /// now allows. Returns false when it can no longer go back: its Track
    /// became unavailable, or the queue already holds its id.
    pub fn undo_remove(&mut self, removed: Removed) -> bool {
        let Removed {
            entry,
            lane,
            index,
            play_next,
        } = removed;
        if !entry.track.is_queueable() || self.contains(entry.id) {
            return false;
        }
        let id = entry.id;
        match lane {
            Lane::Requests => {
                if entry.origin != Origin::Request {
                    return false;
                }
                let play_next_count = self.state.play_next_count;
                let at = if play_next {
                    self.state.play_next_count += 1;
                    index.min(play_next_count)
                } else {
                    index.clamp(play_next_count, self.state.requests.len())
                };
                self.state.requests.insert(at, entry);
            }
            Lane::Collection => {
                let at = index
                    .min(self.state.collection.len())
                    .max(self.state.cursor.map_or(0, |cursor| cursor + 1));
                self.state.collection.insert(at, entry);
            }
        }
        self.state.next_id = self.state.next_id.max(EntryId(id.0 + 1));
        true
    }

    /// Moves an upcoming entry to index `to` of the upcoming list.
    ///
    /// The move makes the order explicit: every upcoming entry, the rest of
    /// the collection included, becomes a Queued Request in the order shown,
    /// and the Play Next grouping ends.
    pub fn move_request(&mut self, entry: EntryId, to: usize) -> bool {
        let start = self.upcoming_start();
        let Some(from) = self
            .state
            .requests
            .iter()
            .chain(&self.state.collection[start..])
            .position(|e| e.id == entry)
        else {
            return false;
        };
        let mut lane = Vec::with_capacity(self.state.requests.len() + self.state.collection.len());
        lane.append(&mut self.state.requests);
        lane.extend(self.state.collection.drain(start..));
        let moved = lane.remove(from);
        lane.insert(to.min(lane.len()), moved);
        for entry in &mut lane {
            entry.origin = Origin::Request;
        }
        self.state.requests = lane;
        self.state.play_next_count = 0;
        true
    }

    /// Removes every upcoming entry: the request lane and the rest of the
    /// collection. Returns how many.
    pub fn clear_upcoming(&mut self) -> usize {
        let start = self.upcoming_start();
        let removed = self.state.requests.len() + self.state.collection.len() - start;
        self.state.requests.clear();
        self.state.play_next_count = 0;
        self.state.collection.truncate(start);
        removed
    }

    /// Removes every occurrence of `tracks`, except the entry `keep_current`
    /// when it is the one playing out. A pruned current entry stops being
    /// current; the cursor stays on the last remaining entry before it.
    pub fn prune(&mut self, tracks: &[TrackRef], keep_current: Option<EntryId>) {
        let doomed = |entry: &QueueEntry| {
            Some(entry.id) != keep_current && tracks.contains(&entry.track.reference)
        };

        let state = &mut self.state;
        let mut current_pruned = match &state.current {
            Some(Current::Detached(entry)) => doomed(entry),
            Some(Current::AtCursor) => state
                .cursor
                .is_some_and(|cursor| doomed(&state.collection[cursor])),
            None => false,
        };
        if let Some(cursor) = state.cursor {
            let kept = state.collection[..=cursor]
                .iter()
                .filter(|entry| !doomed(entry))
                .count();
            state.cursor = kept.checked_sub(1);
            current_pruned |=
                state.cursor.is_none() && matches!(state.current, Some(Current::AtCursor));
        }
        state.collection.retain(|entry| !doomed(entry));

        // The Play Next count is a prefix length: shrink it by what left the
        // prefix, so the request after it does not join the Play Next group.
        let left_prefix = state.requests[..state.play_next_count]
            .iter()
            .filter(|entry| doomed(entry))
            .count();
        state.play_next_count -= left_prefix;
        state.requests.retain(|entry| !doomed(entry));

        if current_pruned {
            state.current = None;
        }
    }

    /// Drops collection history beyond the `keep` most recent entries, for
    /// long paged collections. Returns how many were dropped.
    pub fn trim_history(&mut self, keep: usize) -> usize {
        let Some(cursor) = self.state.cursor else {
            return 0;
        };
        let dropped = cursor.saturating_sub(keep);
        self.state.collection.drain(..dropped);
        self.state.cursor = Some(cursor - dropped);
        dropped
    }

    /// Sets what repeats.
    pub fn set_repeat(&mut self, mode: RepeatMode) {
        self.state.repeat = mode;
    }

    /// Shuffles the upcoming collection entries with `seed`. The current
    /// entry, history and Queued Requests (including requests kept in the
    /// collection) keep their places.
    ///
    /// The seed is applied to the original order, not to an earlier shuffle,
    /// so a seed always gives the same order of the same entries.
    pub fn shuffle_upcoming(&mut self, seed: u64) {
        let mut random = Random::new(seed);
        self.reorder_upcoming(|entries| {
            entries.sort_by_key(|entry| entry.id);
            random.shuffle(entries);
        });
        self.state.shuffle = Some(seed);
    }

    /// Puts the upcoming collection entries back in their original order.
    pub fn restore_upcoming_order(&mut self) {
        self.reorder_upcoming(|entries| entries.sort_by_key(|entry| entry.id));
        self.state.shuffle = None;
    }

    /// Reorders the upcoming collection-origin entries among the places they
    /// hold, leaving requests kept in the collection where they are.
    fn reorder_upcoming(&mut self, reorder: impl FnOnce(&mut [QueueEntry])) {
        let start = self.upcoming_start();
        let upcoming = &mut self.state.collection[start..];
        let places: Vec<usize> = upcoming
            .iter()
            .enumerate()
            .filter(|(_, entry)| matches!(entry.origin, Origin::Collection { .. }))
            .map(|(index, _)| index)
            .collect();
        let mut entries: Vec<QueueEntry> = places.iter().map(|&i| upcoming[i].clone()).collect();
        reorder(&mut entries);
        for (place, entry) in places.into_iter().zip(entries) {
            upcoming[place] = entry;
        }
    }

    /// The Queued Requests in play order.
    pub fn requests(&self) -> &[QueueEntry] {
        &self.state.requests
    }

    /// Collection entries before the cursor, oldest first, including
    /// requests kept as history.
    pub fn history(&self) -> &[QueueEntry] {
        let end = self.state.cursor.unwrap_or(0);
        &self.state.collection[..end]
    }

    /// What plays next, in order: the request lane, then the rest of the
    /// collection. At the end of the collection with [`RepeatMode::Queue`],
    /// the collection again, without its requests and without the entry
    /// that is playing (it plays at its own place in the next pass, and an
    /// id appears only once in what is published).
    pub fn upcoming(&self) -> Vec<&QueueEntry> {
        let collection = &self.state.collection;
        let mut upcoming: Vec<&QueueEntry> = self.state.requests.iter().collect();
        match self.state.cursor {
            None => upcoming.extend(collection),
            Some(cursor) if cursor + 1 < collection.len() => {
                upcoming.extend(&collection[cursor + 1..]);
            }
            Some(_) if self.state.repeat == RepeatMode::Queue => {
                let current = self.current().map(|entry| entry.id);
                upcoming.extend(collection.iter().filter(|entry| {
                    matches!(entry.origin, Origin::Collection { .. }) && Some(entry.id) != current
                }));
            }
            Some(_) => {}
        }
        upcoming
    }
}
