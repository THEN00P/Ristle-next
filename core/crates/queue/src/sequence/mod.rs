//! Playable sequences: collections the host walks without fetching them
//! whole.
//!
//! A sequence's natural order is a list of [`Slot`]s, one per occurrence in
//! its sources, and its play order is a permutation of their ids. A slot is
//! resolved once the host has read which Track it holds; in an
//! offset-addressed collection such as Spotify Liked Songs it may still be
//! only a position (`docs/plugin-api/walkthrough.md`, "Spotify Liked Songs:
//! positional handles"). An unresolved slot never names a Track.

use std::collections::HashMap;
use std::fmt;

use ristle_domain::{Addressing, CollectionDescriptor, ConnectionId, PlaylistId, TrackRef};
use serde::{Deserialize, Serialize};

use crate::error::CorruptState;

mod drift;
#[cfg(test)]
mod drift_tests;
mod order;
mod shuffle;
#[cfg(test)]
mod tests;

pub use drift::{ChangeOutcome, CollectionChange, ObservedPage, RebaseOutcome};
pub use order::StartAt;

/// One occurrence in a sequence. Unique within the sequence and never
/// reused, so the same Track twice in a playlist is two slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SlotId(pub u64);

impl fmt::Display for SlotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// One occurrence of a sequence.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Slot {
    /// Its id.
    pub id: SlotId,
    /// Index into [`SequenceDescriptor::sources`] of the source that holds it.
    pub source: usize,
    /// Its position in that source when it was last read or adjusted. In an
    /// offset-addressed collection this is the offset the host reads.
    pub offset: u64,
    /// The Track the source held there, once read. Only slots of
    /// offset-addressed collections may be unresolved.
    pub track: Option<TrackRef>,
}

/// Where a sequence's slots come from and how they are read again.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SequenceSource {
    /// A Provider collection, read through its Provider Connection.
    Collection(CollectionDescriptor),
    /// A Ristle Playlist.
    RistlePlaylist(PlaylistId),
    /// A fixed list given at start, such as a search snapshot. Never read
    /// again.
    Fixed,
}

/// A page the user can open on a Provider, as the WIT `target` names it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Target {
    /// An album page.
    Album(String),
    /// An artist page.
    Artist(String),
    /// A user page.
    User(String),
    /// A playlist page.
    Playlist(String),
    /// A Provider View, with the Plugin's opaque parameters.
    View {
        /// The Plugin's view name.
        view: String,
        /// Opaque parameters, passed back unchanged.
        params: String,
    },
}

/// What the user opened to start a sequence, for display and navigation.
/// Never used to read the sequence; that is what the sources are for
/// (`phase-2.md`, legacy trap 5).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Opened {
    /// A Ristle Playlist.
    RistlePlaylist(PlaylistId),
    /// A page of one Provider Connection.
    Provider {
        /// The connection the page belongs to.
        connection: ConnectionId,
        /// The page.
        target: Target,
    },
    /// Liked Tracks, across every Provider Connection that has them.
    LikedTracks,
    /// Search results.
    Search {
        /// The query as the user typed it.
        query: String,
    },
}

/// A sequence's id. A composite's id derives from its members, never from a
/// constant (`phase-2.md`, legacy trap 4).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SequenceId(pub String);

/// What a sequence is and where it is read from.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SequenceDescriptor {
    /// Its id.
    pub id: SequenceId,
    /// The title to show.
    pub title: String,
    /// What the user opened.
    pub opened: Opened,
    /// The sources, each stored once and referenced by index from every
    /// slot. A composite has several.
    pub sources: Vec<SequenceSource>,
}

/// Everything a sequence holds, in a form that persists and restores
/// exactly. Checked by [`Sequence::restore`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SequenceState {
    /// What the sequence is.
    pub descriptor: SequenceDescriptor,
    /// Every occurrence, in natural order.
    pub slots: Vec<Slot>,
    /// The play order: every slot id exactly once.
    pub order: Vec<SlotId>,
    /// Index into `order` of the slot playing or last played. `None` before
    /// the sequence starts, and after a restart for Repeat Queue.
    ///
    /// `order[..=cursor]` is also what played in this pass, occurrence by
    /// occurrence, which a re-base consults so that an occurrence that turns
    /// up again is not played twice. A separate set of Tracks would merge
    /// duplicate occurrences.
    pub cursor: Option<usize>,
    /// Exclusive index into `order` up to which slots have been materialized
    /// into the Canonical Queue. At least the cursor: equal to it while the
    /// current slot itself still has to be materialized (and, in an
    /// offset-addressed source, possibly resolved), as right after a start.
    pub materialized_end: usize,
    /// The seed of the shuffle, while shuffled.
    pub shuffle: Option<u64>,
    /// Increases with every change that invalidates a plan made earlier,
    /// such as a hydration plan made before a shuffle.
    pub revision: u64,
    /// The id the next new slot gets. Greater than every slot id.
    pub next_slot: SlotId,
}

impl SequenceState {
    /// Checks the invariants [`Sequence`] relies on.
    pub fn check(&self) -> Result<(), CorruptState> {
        let sources = &self.descriptor.sources;
        if sources.is_empty() {
            return Err(CorruptState::NoSources);
        }
        let mut slots = HashMap::with_capacity(self.slots.len());
        for slot in &self.slots {
            if slots.insert(slot.id, false).is_some() {
                return Err(CorruptState::DuplicateSlot(slot.id));
            }
            if slot.id >= self.next_slot {
                return Err(CorruptState::SlotNotBeforeNextSlot {
                    slot: slot.id,
                    next_slot: self.next_slot,
                });
            }
            let Some(source) = sources.get(slot.source) else {
                return Err(CorruptState::UnknownSource(slot.id));
            };
            let positional = matches!(
                source,
                SequenceSource::Collection(CollectionDescriptor {
                    addressing: Addressing::Offset,
                    ..
                })
            );
            if slot.track.is_none() && !positional {
                return Err(CorruptState::UnresolvedSlot(slot.id));
            }
        }
        if self.order.len() != self.slots.len() {
            return Err(CorruptState::OrderNotAPermutation);
        }
        for id in &self.order {
            match slots.get_mut(id) {
                Some(seen @ false) => *seen = true,
                _ => return Err(CorruptState::OrderNotAPermutation),
            }
        }
        if let Some(cursor) = self.cursor
            && cursor >= self.order.len()
        {
            return Err(CorruptState::CursorOutOfRange {
                cursor,
                len: self.order.len(),
            });
        }
        let materialized_from = self.cursor.unwrap_or(0);
        if self.materialized_end > self.order.len() || self.materialized_end < materialized_from {
            return Err(CorruptState::MaterializedEndOutOfRange {
                materialized_end: self.materialized_end,
                len: self.order.len(),
            });
        }
        Ok(())
    }
}

/// A collection walked in a seeded, persisted order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sequence {
    state: SequenceState,
}

impl Sequence {
    /// A sequence from persisted state, or why the state is corrupt.
    pub fn restore(state: SequenceState) -> Result<Self, CorruptState> {
        state.check()?;
        Ok(Self { state })
    }

    /// The sequence's state, for persisting. [`Sequence::restore`] turns it
    /// back into an equal sequence.
    pub fn state(&self) -> &SequenceState {
        &self.state
    }
}

#[cfg(test)]
mod state_tests {
    use ristle_domain::{CollectionRef, Count};

    use super::*;

    fn liked(addressing: Addressing) -> SequenceSource {
        SequenceSource::Collection(CollectionDescriptor {
            reference: CollectionRef {
                connection: "spotify-1".into(),
                id: "liked".into(),
            },
            addressing,
            total: Some(Count::Exact(1000)),
            revision: Some("1000".into()),
            endless: false,
        })
    }

    fn track(index: u64) -> TrackRef {
        TrackRef::catalog("spotify".into(), format!("track-{index}")).unwrap()
    }

    /// A sequence over `count` resolved slots in natural order.
    fn manifest(count: u64) -> SequenceState {
        let slots: Vec<_> = (0..count)
            .map(|index| Slot {
                id: SlotId(index),
                source: 0,
                offset: index,
                track: Some(track(index)),
            })
            .collect();
        SequenceState {
            descriptor: SequenceDescriptor {
                id: SequenceId("ristle-playlist:p1".into()),
                title: "Large playlist".into(),
                opened: Opened::RistlePlaylist(PlaylistId::new("p1")),
                sources: vec![SequenceSource::RistlePlaylist(PlaylistId::new("p1"))],
            },
            order: slots.iter().map(|slot| slot.id).collect(),
            slots,
            cursor: Some(0),
            materialized_end: 1,
            shuffle: None,
            revision: 0,
            next_slot: SlotId(count),
        }
    }

    #[test]
    fn sequence_manifest_preserves_duplicate_track_occurrences() {
        let mut state = manifest(2);
        state.slots[1].track = state.slots[0].track.clone();

        let sequence = Sequence::restore(state.clone()).unwrap();
        assert_eq!(sequence.state(), &state);
    }

    #[test]
    fn sequence_manifest_rejects_duplicate_occurrence_ids() {
        let mut state = manifest(2);
        state.slots[1].id = state.slots[0].id;
        state.order = vec![SlotId(0), SlotId(0)];

        assert_eq!(
            Sequence::restore(state),
            Err(CorruptState::DuplicateSlot(SlotId(0)))
        );
    }

    #[test]
    fn sequence_order_rejects_corrupt_persisted_permutation() {
        let mut state = manifest(3);
        state.order = vec![SlotId(0), SlotId(0), SlotId(2)];

        assert_eq!(
            Sequence::restore(state),
            Err(CorruptState::OrderNotAPermutation)
        );
    }

    #[test]
    fn positional_composite_round_trips() {
        let mut state = manifest(4);
        state.descriptor = SequenceDescriptor {
            id: SequenceId("liked:spotify-1+soundcloud-1".into()),
            title: "Liked Songs".into(),
            opened: Opened::LikedTracks,
            sources: vec![liked(Addressing::Offset), liked(Addressing::Cursor)],
        };
        state.slots[1].track = None;
        state.slots[3].source = 1;
        state.order = vec![SlotId(2), SlotId(0), SlotId(3), SlotId(1)];
        state.cursor = Some(1);
        state.materialized_end = 3;
        state.shuffle = Some(42);
        state.revision = 7;

        let sequence = Sequence::restore(state.clone()).unwrap();
        assert_eq!(sequence.state(), &state);
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(serde_json::from_str::<SequenceState>(&json).unwrap(), state);
    }

    #[test]
    fn restore_rejects_each_corruption() {
        let corrupt = |change: fn(&mut SequenceState)| {
            let mut state = manifest(3);
            change(&mut state);
            Sequence::restore(state).unwrap_err()
        };

        assert_eq!(
            corrupt(|s| s.descriptor.sources.clear()),
            CorruptState::NoSources
        );
        assert_eq!(
            corrupt(|s| s.slots[2].source = 1),
            CorruptState::UnknownSource(SlotId(2))
        );
        assert_eq!(
            corrupt(|s| s.slots[1].track = None),
            CorruptState::UnresolvedSlot(SlotId(1))
        );
        assert_eq!(
            corrupt(|s| s.next_slot = SlotId(2)),
            CorruptState::SlotNotBeforeNextSlot {
                slot: SlotId(2),
                next_slot: SlotId(2)
            }
        );
        assert_eq!(
            corrupt(|s| {
                s.order.pop();
            }),
            CorruptState::OrderNotAPermutation
        );
        assert_eq!(
            corrupt(|s| s.order[2] = SlotId(9)),
            CorruptState::OrderNotAPermutation
        );
        assert_eq!(
            corrupt(|s| s.cursor = Some(3)),
            CorruptState::CursorOutOfRange { cursor: 3, len: 3 }
        );
        assert_eq!(
            corrupt(|s| s.materialized_end = 4),
            CorruptState::MaterializedEndOutOfRange {
                materialized_end: 4,
                len: 3
            }
        );
        assert_eq!(
            corrupt(|s| {
                s.cursor = Some(2);
                s.materialized_end = 1;
            }),
            CorruptState::MaterializedEndOutOfRange {
                materialized_end: 1,
                len: 3
            }
        );
    }
}
