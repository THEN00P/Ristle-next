//! The Canonical Queue.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use ristle_domain::TrackRef;

use crate::entry::{CollectionPosition, EntryId, Origin, QueueEntry, RepeatMode};
use crate::error::CorruptState;

mod ops;
#[cfg(test)]
mod tests;

pub use ops::{AddOutcome, CollectionEntry, Lane, Rejection, Removed, StartOutcome};

/// Which entry is playing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Current {
    /// The collection entry at the cursor.
    AtCursor,
    /// A Queued Request taken from the request lane. It is in neither the
    /// collection nor the lane while it plays; the cursor stays where the
    /// collection will resume.
    Detached(Box<QueueEntry>),
}

/// Everything the Canonical Queue holds, in a form that persists and
/// restores exactly.
///
/// Built by [`CanonicalQueue::snapshot`] and checked by
/// [`CanonicalQueue::restore`], which rejects corrupt state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueueState {
    /// The collection: history, the entry at the cursor, and what follows.
    /// Queued Requests that a direct selection passed are moved here, with
    /// [`Origin::Request`], so that Previous reaches them. Previous can then
    /// leave one at or after the cursor, where it plays again on Next as the
    /// history it is.
    pub collection: Vec<QueueEntry>,
    /// Index into `collection` of the entry that is playing or last played.
    /// `None` before the collection has started.
    pub cursor: Option<usize>,
    /// The request lane, in play order: Play Next entries first, then Add to
    /// queue entries in FIFO order.
    pub requests: Vec<QueueEntry>,
    /// How many entries at the front of `requests` came from Play Next. A new
    /// Play Next goes after them and before every other request.
    pub play_next_count: usize,
    /// The entry that is playing, if any.
    pub current: Option<Current>,
    /// What repeats.
    pub repeat: RepeatMode,
    /// The seed that shuffled the upcoming collection, while it is shuffled.
    /// The original order is the order of entry ids, which grow as entries
    /// are created.
    pub shuffle: Option<u64>,
    /// The id the next new entry gets. Greater than every id in the state.
    pub next_id: EntryId,
    /// Collection entries of this pass that `trim_history` dropped, so that
    /// reconciling a shuffled pass still counts them as played. Cleared when
    /// a new pass starts.
    #[serde(default)]
    pub trimmed: Vec<Trimmed>,
}

/// A collection entry `trim_history` dropped from this pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trimmed {
    /// Its Track.
    pub track: TrackRef,
    /// Its position in the collection.
    pub position: CollectionPosition,
}

/// The state of an empty queue: nothing started, repeat off, ids from 1.
impl Default for QueueState {
    fn default() -> Self {
        Self {
            collection: Vec::new(),
            cursor: None,
            requests: Vec::new(),
            play_next_count: 0,
            current: None,
            repeat: RepeatMode::Off,
            shuffle: None,
            next_id: EntryId(1),
            trimmed: Vec::new(),
        }
    }
}

impl QueueState {
    /// Checks the invariants [`CanonicalQueue`] relies on.
    pub fn check(&self) -> Result<(), CorruptState> {
        if let Some(cursor) = self.cursor
            && cursor >= self.collection.len()
        {
            return Err(CorruptState::CursorOutOfRange {
                cursor,
                len: self.collection.len(),
            });
        }
        // A next id of u64::MAX could only be handed out by overflowing.
        if self.next_id.0 == u64::MAX {
            return Err(CorruptState::EntryIdsExhausted);
        }
        if self.play_next_count > self.requests.len() {
            return Err(CorruptState::PlayNextBeyondRequests {
                play_next_count: self.play_next_count,
                requests: self.requests.len(),
            });
        }

        let mut ids = HashSet::new();
        let mut check_id = |entry: &QueueEntry| {
            if !ids.insert(entry.id) {
                return Err(CorruptState::DuplicateEntry(entry.id));
            }
            if entry.id >= self.next_id {
                return Err(CorruptState::EntryNotBeforeNextId {
                    entry: entry.id,
                    next_id: self.next_id,
                });
            }
            Ok(())
        };
        for entry in &self.collection {
            check_id(entry)?;
        }
        for entry in &self.requests {
            check_id(entry)?;
            if entry.origin != Origin::Request {
                return Err(CorruptState::MisplacedEntry(entry.id));
            }
        }
        match &self.current {
            None => {}
            Some(Current::AtCursor) => {
                if self.cursor.is_none() {
                    return Err(CorruptState::CurrentWithoutCursor);
                }
            }
            Some(Current::Detached(entry)) => {
                check_id(entry)?;
                if entry.origin != Origin::Request {
                    return Err(CorruptState::MisplacedEntry(entry.id));
                }
            }
        }
        Ok(())
    }
}

/// The authoritative ordered sequence of Tracks Ristle intends to play
/// across all Providers.
#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalQueue {
    state: QueueState,
}

impl Default for CanonicalQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl CanonicalQueue {
    /// An empty queue with repeat off.
    pub fn new() -> Self {
        Self {
            state: QueueState::default(),
        }
    }

    /// The queue's state, for persisting. [`CanonicalQueue::restore`] turns it
    /// back into an equal queue.
    pub fn snapshot(&self) -> QueueState {
        self.state.clone()
    }

    /// A queue from persisted state, or why the state is corrupt.
    ///
    /// Availability is not checked: a Track that became unavailable after it
    /// was queued is pruned by the session, not treated as corruption.
    pub fn restore(state: QueueState) -> Result<Self, CorruptState> {
        state.check()?;
        Ok(Self { state })
    }

    /// The entry that is playing, if any.
    pub fn current(&self) -> Option<&QueueEntry> {
        match self.state.current.as_ref()? {
            Current::AtCursor => self.state.collection.get(self.state.cursor?),
            Current::Detached(entry) => Some(entry),
        }
    }

    /// What repeats.
    pub fn repeat(&self) -> RepeatMode {
        self.state.repeat
    }
}

#[cfg(test)]
mod state_tests {
    use ristle_domain::{Track, TrackRef};

    use super::*;
    use crate::entry::{CollectionPosition, Submitter};

    fn track(provider: &str, id: &str) -> Track {
        Track::new(TrackRef::catalog(provider.into(), id.into()).unwrap(), id)
    }

    fn entry(id: u64, track: Track, origin: Origin) -> QueueEntry {
        QueueEntry {
            id: EntryId(id),
            track,
            origin,
            slot: None,
            submitter: None,
        }
    }

    fn collection_entry(id: u64, position: u64, name: &str) -> QueueEntry {
        entry(
            id,
            track("spotify", name),
            Origin::Collection {
                position: CollectionPosition(position),
            },
        )
    }

    fn request(id: u64, name: &str) -> QueueEntry {
        entry(id, track("soundcloud", name), Origin::Request)
    }

    /// A state that passes every check: a request passed by a direct
    /// selection sits in the collection, one request plays detached, and the
    /// lane holds one Play Next and one Add to queue entry.
    fn valid_state() -> QueueState {
        QueueState {
            collection: vec![
                collection_entry(1, 0, "first"),
                request(5, "passed"),
                collection_entry(2, 2, "second"),
                collection_entry(3, 3, "third"),
            ],
            cursor: Some(2),
            requests: vec![
                request(7, "next"),
                QueueEntry {
                    submitter: Some(Submitter {
                        id: "member-1".into(),
                        display_name: "Ada".into(),
                    }),
                    ..request(8, "later")
                },
            ],
            play_next_count: 1,
            current: Some(Current::Detached(Box::new(request(6, "playing")))),
            repeat: RepeatMode::Queue,
            shuffle: Some(42),
            next_id: EntryId(9),
            trimmed: vec![Trimmed {
                track: track("spotify", "trimmed").reference,
                position: CollectionPosition(0),
            }],
        }
    }

    #[test]
    fn queue_restore_rejects_duplicate_entry_ids() {
        let state = QueueState {
            collection: vec![
                collection_entry(1, 0, "first"),
                collection_entry(1, 1, "second"),
            ],
            cursor: Some(0),
            current: Some(Current::AtCursor),
            next_id: EntryId(2),
            ..QueueState::default()
        };

        assert_eq!(
            CanonicalQueue::restore(state),
            Err(CorruptState::DuplicateEntry(EntryId(1)))
        );
    }

    #[test]
    fn restore_of_a_snapshot_is_identity() {
        let queue = CanonicalQueue::restore(valid_state()).unwrap();
        assert_eq!(queue.snapshot(), valid_state());
        assert_eq!(CanonicalQueue::restore(queue.snapshot()).unwrap(), queue);
        assert_eq!(queue.current().unwrap().id, EntryId(6));

        let json = serde_json::to_string(&valid_state()).unwrap();
        assert_eq!(
            serde_json::from_str::<QueueState>(&json).unwrap(),
            valid_state()
        );
    }

    #[test]
    fn current_at_cursor_reads_the_collection() {
        let state = QueueState {
            current: Some(Current::AtCursor),
            ..valid_state()
        };
        let queue = CanonicalQueue::restore(state).unwrap();
        assert_eq!(queue.current().unwrap().id, EntryId(2));
        assert_eq!(CanonicalQueue::new().current(), None);
    }

    // Previous after a direct selection parks the cursor on a passed request,
    // or leaves passed requests after the cursor. Both are valid history
    // (legacy `QueueStateRoundTripsWithPreservedRequestAfterCursor`).
    #[test]
    fn restore_accepts_passed_requests_at_and_after_the_cursor() {
        let at_cursor = QueueState {
            cursor: Some(1),
            current: Some(Current::AtCursor),
            ..valid_state()
        };
        let queue = CanonicalQueue::restore(at_cursor).unwrap();
        assert_eq!(queue.current().unwrap().origin, Origin::Request);

        let after_cursor = QueueState {
            cursor: Some(0),
            current: Some(Current::AtCursor),
            ..valid_state()
        };
        assert!(CanonicalQueue::restore(after_cursor).is_ok());
    }

    #[test]
    fn restore_rejects_each_corruption() {
        let corrupt = |change: fn(&mut QueueState)| {
            let mut state = valid_state();
            change(&mut state);
            CanonicalQueue::restore(state).unwrap_err()
        };

        assert_eq!(
            corrupt(|s| s.cursor = Some(4)),
            CorruptState::CursorOutOfRange { cursor: 4, len: 4 }
        );
        assert_eq!(
            corrupt(|s| s.play_next_count = 3),
            CorruptState::PlayNextBeyondRequests {
                play_next_count: 3,
                requests: 2
            }
        );
        assert_eq!(
            corrupt(|s| s.requests[1].id = EntryId(3)),
            CorruptState::DuplicateEntry(EntryId(3))
        );
        assert_eq!(
            corrupt(|s| s.current = Some(Current::Detached(Box::new(request(1, "copy"))))),
            CorruptState::DuplicateEntry(EntryId(1))
        );
        assert_eq!(
            corrupt(|s| s.next_id = EntryId(8)),
            CorruptState::EntryNotBeforeNextId {
                entry: EntryId(8),
                next_id: EntryId(8)
            }
        );
        assert_eq!(
            corrupt(|s| s.requests[0] = collection_entry(7, 9, "lane")),
            CorruptState::MisplacedEntry(EntryId(7))
        );
        assert_eq!(
            corrupt(|s| s.current = Some(Current::Detached(Box::new(collection_entry(6, 9, "x"))))),
            CorruptState::MisplacedEntry(EntryId(6))
        );
        assert_eq!(
            corrupt(|s| s.next_id = EntryId(u64::MAX)),
            CorruptState::EntryIdsExhausted
        );
        assert_eq!(
            corrupt(|s| {
                s.cursor = None;
                s.current = Some(Current::AtCursor);
            }),
            CorruptState::CurrentWithoutCursor
        );
    }
}
