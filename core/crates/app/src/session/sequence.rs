//! The active sequence: the collection the queue's continuation comes from,
//! walked without materializing it whole.
//!
//! The Canonical Queue holds a window of it: up to [`HISTORY`] played
//! entries, the current one and [`LOOKAHEAD`] upcoming ones. After every
//! mutation the window is topped up from the sequence and its history
//! trimmed, so the queue stays bounded however large the collection is
//! (legacy `ApplicationCoreRefillsLargeSequenceWithoutGrowingCanonicalQueue`).
//!
//! This module plays Ristle Playlists, whose Tracks are all known locally.
//! Provider collections, which are read and hydrated through Plugins, use
//! the same sequence and window.

use std::collections::HashMap;
use std::sync::Arc;

use ristle_domain::{RistlePlaylist, Track, TrackRef};
use ristle_queue::sequence::{
    Opened, SequenceDescriptor, SequenceId, SequenceSource, SequenceState, Slot, SlotId,
};
use ristle_queue::{
    CanonicalQueue, CollectionEntry, CollectionPosition, EntryId, Origin, Sequence, StartAt,
};

use crate::error::Error;
use crate::model::items::ItemRef;
use crate::model::session::QueueSource;

/// Played collection entries the queue keeps.
pub(crate) const HISTORY: usize = 50;
/// Upcoming collection entries the queue keeps materialized.
pub(crate) const LOOKAHEAD: usize = 50;

/// The id of a Ristle Playlist's sequence.
pub(crate) fn playlist_sequence_id(playlist: &str) -> SequenceId {
    SequenceId(format!("ristle-playlist:{playlist}"))
}

/// The sequence the queue's continuation comes from, with the Tracks its
/// slots stand for.
#[derive(Debug, Clone)]
pub(crate) struct Active {
    pub(crate) sequence: Sequence,
    /// The Ristle Playlist it plays. A slot's offset indexes its Tracks.
    pub(crate) playlist: Arc<RistlePlaylist>,
}

fn invalid(reason: impl Into<String>) -> Error {
    Error::InvalidArgument {
        reason: reason.into(),
    }
}

fn descriptor(playlist: &RistlePlaylist) -> SequenceDescriptor {
    SequenceDescriptor {
        id: playlist_sequence_id(playlist.id.as_str()),
        title: playlist.title.clone(),
        opened: Opened::RistlePlaylist(playlist.id.clone()),
        sources: vec![SequenceSource::RistlePlaylist(playlist.id.clone())],
    }
}

impl Active {
    /// Restores a saved sequence over the playlist it plays.
    pub(crate) fn restore(
        state: SequenceState,
        playlist: Arc<RistlePlaylist>,
    ) -> Result<Self, Error> {
        let sequence = Sequence::restore(state).map_err(|corrupt| Error::Internal {
            reason: format!("the saved sequence is corrupt: {corrupt}"),
        })?;
        Ok(Self { sequence, playlist })
    }

    /// Starts `playlist` in `queue`: at the occurrence `start` (an index into
    /// its Tracks) or at its beginning, shuffled with `seed` if given. The
    /// first occurrence that can play at or after the start plays, even past
    /// a whole window of Disabled Tracks (legacy
    /// `ApplicationCoreStartsPastAnUnavailableInitialPlaylistWindow`). When
    /// nothing can play, `queue` is left as it was.
    pub(crate) fn start(
        playlist: Arc<RistlePlaylist>,
        start: Option<usize>,
        seed: Option<u64>,
        queue: &mut CanonicalQueue,
    ) -> Result<Self, Error> {
        let slots: Vec<Slot> = playlist
            .tracks
            .iter()
            .enumerate()
            .map(|(index, track)| Slot {
                id: SlotId(index as u64),
                source: 0,
                offset: index as u64,
                track: Some(track.reference.clone()),
            })
            .collect();
        if slots.is_empty() {
            return Err(invalid("the playlist is empty"));
        }
        let at = match start {
            None => StartAt::Beginning,
            Some(index) if index < slots.len() => StartAt::Slot(SlotId(index as u64)),
            Some(index) => return Err(invalid(format!("the playlist has no Track {index}"))),
        };
        let sequence = Sequence::start(descriptor(&playlist), slots, at, seed)
            .map_err(|error| invalid(error.to_string()))?;
        let mut active = Self { sequence, playlist };

        // From the start on, until LOOKAHEAD entries after the first one
        // that can play, however many Disabled Tracks come first.
        let order = active.sequence.state().order.clone();
        let begin = active.sequence.state().cursor.unwrap_or(0);
        let mut entries = Vec::new();
        let mut playable = 0;
        let mut end = begin;
        for &slot in &order[begin..] {
            end += 1;
            if let Some(entry) = active.entry(slot) {
                playable += usize::from(entry.track.is_queueable());
                entries.push(entry);
            }
            if playable > LOOKAHEAD {
                break;
            }
        }
        if !queue.start_collection(entries, 0).started {
            return Err(invalid("nothing in the playlist can play"));
        }
        let current = queue
            .current()
            .and_then(|entry| entry.slot)
            .ok_or_else(|| Error::Internal {
                reason: "a started playlist has no current slot".to_owned(),
            })?;
        active.sequence.advance_to(current);
        active.sequence.set_materialized_end(end);
        active.sync(queue);
        Ok(active)
    }

    /// The queue entry a slot materializes into, if the playlist still has
    /// the Track the slot names.
    fn entry(&self, slot: SlotId) -> Option<CollectionEntry> {
        let slot = self.sequence.slot(slot)?;
        let track: &Track = self
            .playlist
            .tracks
            .get(usize::try_from(slot.offset).ok()?)?;
        (slot.track.as_ref() == Some(&track.reference)).then(|| CollectionEntry {
            track: track.clone(),
            position: CollectionPosition(slot.offset),
            slot: Some(slot.id),
        })
    }

    fn order_index(&self) -> HashMap<SlotId, usize> {
        self.sequence
            .state()
            .order
            .iter()
            .enumerate()
            .map(|(index, &slot)| (slot, index))
            .collect()
    }

    /// The queue's materialized lookahead: upcoming collection entries whose
    /// slots come after the current one. Entries of a next pass, which the
    /// queue lists while it wraps, are behind it and do not count.
    fn lookahead(&self, queue: &CanonicalQueue) -> Vec<EntryId> {
        let index = self.order_index();
        let cursor = self.sequence.state().cursor;
        queue
            .upcoming()
            .into_iter()
            .filter(|entry| matches!(entry.origin, Origin::Collection { .. }))
            .filter(|entry| {
                entry
                    .slot
                    .and_then(|slot| index.get(&slot))
                    .is_some_and(|&at| cursor.is_none_or(|cursor| at > cursor))
            })
            .map(|entry| entry.id)
            .collect()
    }

    /// Follows the queue: moves the sequence to the slot that plays, tops
    /// the lookahead up to [`LOOKAHEAD`] entries and trims the history to
    /// [`HISTORY`].
    pub(crate) fn sync(&mut self, queue: &mut CanonicalQueue) {
        if let Some(slot) = queue.current().and_then(|entry| entry.slot) {
            self.sequence.seek_to(slot);
        }
        loop {
            let ahead = self.lookahead(queue).len();
            if ahead >= LOOKAHEAD {
                break;
            }
            let begin = self.sequence.state().materialized_end;
            let batch = self.sequence.refill_batch(LOOKAHEAD - ahead);
            if batch.is_empty() {
                break;
            }
            let entries = batch.iter().filter_map(|&slot| self.entry(slot)).collect();
            queue.append_collection(entries);
            self.sequence.set_materialized_end(begin + batch.len());
        }
        queue.trim_history(HISTORY);
    }

    /// Takes the materialized lookahead out of the queue; the next sync
    /// materializes it again from the sequence.
    fn drop_lookahead(&self, queue: &mut CanonicalQueue) {
        for entry in self.lookahead(queue) {
            queue.remove(entry);
        }
    }

    /// Shuffles what is still to come with `seed`, or restores the natural
    /// order with `None`. The current entry and what played keep their
    /// places.
    pub(crate) fn set_shuffle(&mut self, seed: Option<u64>, queue: &mut CanonicalQueue) {
        self.drop_lookahead(queue);
        self.sequence.set_shuffle(seed);
        self.sync(queue);
    }

    /// Whether the continuation is shuffled.
    pub(crate) fn shuffled(&self) -> bool {
        self.sequence.state().shuffle.is_some()
    }

    /// Follows an edit of the playlist.
    ///
    /// Playback goes on from the current occurrence, traced through the
    /// edit (see [`align`]), so a duplicate elsewhere is never mistaken for
    /// it (legacy trap 6). What follows is the edited playlist after it or, shuffled,
    /// every occurrence this pass has not played, shuffled with the same
    /// seed. When the current Track is gone, the whole edited playlist
    /// follows. Returns `None` when the edit leaves nothing to play from,
    /// so the caller detaches the queue from the playlist.
    pub(crate) fn reconcile(
        mut self,
        edited: Arc<RistlePlaylist>,
        queue: &mut CanonicalQueue,
    ) -> Result<Option<Self>, Error> {
        if edited.tracks.is_empty() {
            return Ok(None);
        }
        let old = self.sequence.state().clone();
        let anchor = self
            .sequence
            .current()
            .and_then(|id| self.sequence.slot(id).cloned());

        // New slots, numbered after every old one so no queue entry's slot
        // is reused; the anchor keeps its id.
        let anchor_at = anchor.as_ref().and_then(|anchor| {
            let track = anchor.track.as_ref()?;
            let before: Vec<&TrackRef> =
                self.playlist.tracks.iter().map(|t| &t.reference).collect();
            let after: Vec<&TrackRef> = edited.tracks.iter().map(|t| &t.reference).collect();
            let at = usize::try_from(anchor.offset).ok()?;
            align(&before, &after, at)
                .filter(|&index| after.get(index) == Some(&track))
                .or_else(|| {
                    // Not traceable through the edit: the nearest
                    // occurrence of the Track.
                    after
                        .iter()
                        .enumerate()
                        .filter(|(_, candidate)| **candidate == track)
                        .min_by_key(|(index, _)| (*index as u64).abs_diff(anchor.offset))
                        .map(|(index, _)| index)
                })
        });
        let mut next = old.next_slot.0;
        let slots: Vec<Slot> = edited
            .tracks
            .iter()
            .enumerate()
            .map(|(index, track)| {
                let id = match (&anchor, anchor_at) {
                    (Some(anchor), Some(at)) if at == index => anchor.id,
                    _ => {
                        next += 1;
                        SlotId(next - 1)
                    }
                };
                Slot {
                    id,
                    source: 0,
                    offset: index as u64,
                    track: Some(track.reference.clone()),
                }
            })
            .collect();
        let anchor_id = anchor_at.map(|at| slots[at].id);

        // What played in this pass, by Track and count, before the anchor.
        let mut played: HashMap<&TrackRef, usize> = HashMap::new();
        let played_until = old.cursor.unwrap_or(0);
        let old_slots: HashMap<SlotId, &Slot> = old.slots.iter().map(|s| (s.id, s)).collect();
        if old.cursor.is_some() {
            for id in &old.order[..played_until] {
                if let Some(track) = old_slots.get(id).and_then(|slot| slot.track.as_ref()) {
                    *played.entry(track).or_default() += 1;
                }
            }
        }

        let (order, cursor) = match old.shuffle {
            None => {
                let order: Vec<SlotId> = slots.iter().map(|slot| slot.id).collect();
                let cursor = anchor_at;
                (order, cursor)
            }
            Some(seed) => {
                let mut history = Vec::new();
                let mut rest = Vec::new();
                for slot in &slots {
                    if Some(slot.id) == anchor_id {
                        continue;
                    }
                    let seen = slot.track.as_ref().and_then(|track| played.get_mut(track));
                    match seen {
                        Some(count) if *count > 0 => {
                            *count -= 1;
                            history.push(slot.id);
                        }
                        _ => rest.push(slot.clone()),
                    }
                }
                let shuffled: Vec<SlotId> = if rest.is_empty() {
                    Vec::new()
                } else {
                    Sequence::start(descriptor(&edited), rest, StartAt::Beginning, Some(seed))
                        .map_err(|error| invalid(error.to_string()))?
                        .state()
                        .order
                        .clone()
                };
                let mut order = history;
                let cursor = match anchor_id {
                    Some(anchor) => {
                        order.push(anchor);
                        Some(order.len() - 1)
                    }
                    None => order.len().checked_sub(1),
                };
                order.extend(shuffled);
                (order, cursor)
            }
        };
        // Above every slot id, old ones included, so none is ever reused.
        let next_slot = slots
            .iter()
            .map(|slot| slot.id.0 + 1)
            .max()
            .unwrap_or(0)
            .max(next);
        let state = SequenceState {
            descriptor: descriptor(&edited),
            slots,
            order,
            cursor,
            materialized_end: cursor.map_or(0, |cursor| cursor + 1),
            shuffle: old.shuffle,
            revision: old.revision.wrapping_add(1),
            next_slot: SlotId(next_slot),
        };

        self.drop_lookahead(queue);
        self.sequence = Sequence::restore(state).map_err(|corrupt| Error::Internal {
            reason: format!("a reconciled sequence is corrupt: {corrupt}"),
        })?;
        self.playlist = edited;
        self.sync(queue);
        Ok(Some(self))
    }

    /// The sequence's id.
    pub(crate) fn id(&self) -> &SequenceId {
        &self.sequence.state().descriptor.id
    }

    /// The playlist's id.
    pub(crate) fn playlist_id(&self) -> &str {
        self.playlist.id.as_str()
    }

    /// Where the continuation comes from, for display.
    pub(crate) fn source(&self) -> QueueSource {
        QueueSource {
            title: self.sequence.state().descriptor.title.clone(),
            opened: Some(ItemRef::RistlePlaylist {
                id: self.playlist.id.as_str().to_owned(),
            }),
        }
    }

    /// Occurrences still to come after the current one.
    pub(crate) fn remaining(&self) -> u64 {
        let state = self.sequence.state();
        let played = state.cursor.map_or(0, |cursor| cursor + 1);
        state.order.len().saturating_sub(played) as u64
    }
}

/// Largest changed stretch, in compared pairs, that [`align`] aligns
/// exactly. Edits are usually small; a larger one falls back to the
/// nearest occurrence.
const ALIGN_LIMIT: usize = 4_000_000;

/// Where the entry at `at` in `before` is in `after`, if it is still there:
/// unchanged leading and trailing stretches map directly, and the changed
/// stretch between them through a longest common subsequence. `None` when
/// the entry was removed or the changed stretch is too large to align.
pub(crate) fn align<T: PartialEq>(before: &[T], after: &[T], at: usize) -> Option<usize> {
    let prefix = before.iter().zip(after).take_while(|(a, b)| a == b).count();
    if at < prefix {
        return Some(at);
    }
    let suffix = before[prefix..]
        .iter()
        .rev()
        .zip(after[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    if at >= before.len() - suffix {
        return Some(at + after.len() - before.len());
    }
    let old = &before[prefix..before.len() - suffix];
    let new = &after[prefix..after.len() - suffix];
    if old.len().saturating_mul(new.len()) > ALIGN_LIMIT {
        return None;
    }
    // lengths[i][j]: the longest common subsequence of old[i..] and new[j..].
    let width = new.len() + 1;
    let mut lengths = vec![0u32; (old.len() + 1) * width];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            lengths[i * width + j] = if old[i] == new[j] {
                lengths[(i + 1) * width + j + 1] + 1
            } else {
                lengths[(i + 1) * width + j].max(lengths[i * width + j + 1])
            };
        }
    }
    let target = at - prefix;
    let (mut i, mut j) = (0, 0);
    while i < old.len() && j < new.len() {
        if old[i] == new[j] && lengths[i * width + j] == lengths[(i + 1) * width + j + 1] + 1 {
            if i == target {
                return Some(prefix + j);
            }
            i += 1;
            j += 1;
        } else if lengths[(i + 1) * width + j] >= lengths[i * width + j + 1] {
            if i == target {
                return None;
            }
            i += 1;
        } else {
            j += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::align;

    #[test]
    fn aligns_through_insertions_and_removals() {
        let before = ["a", "b", "a", "c"];
        // An insertion before the anchor.
        assert_eq!(align(&before, &["new", "a", "b", "a", "c"], 2), Some(3));
        // The first duplicate removed.
        assert_eq!(align(&before, &["b", "a", "c"], 2), Some(1));
        // Another duplicate inserted before.
        assert_eq!(align(&before, &["a", "a", "b", "a", "c"], 2), Some(3));
        // The anchor itself removed.
        assert_eq!(align(&before, &["a", "b", "c"], 2), None);
        // Unchanged.
        assert_eq!(align(&before, &before, 3), Some(3));
        // A change in the middle, anchor after it.
        assert_eq!(
            align(&["x", "y", "z", "w"], &["x", "q", "r", "z", "w"], 2),
            Some(3)
        );
    }
}
