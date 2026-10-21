//! Canonical Queue entries and the values that describe them.

use std::fmt;

use ristle_domain::Track;
use serde::{Deserialize, Serialize};

use crate::sequence::SlotId;

/// One entry of the Canonical Queue. Unique within a queue and never reused,
/// so two occurrences of the same Track stay distinct entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntryId(pub u64);

impl fmt::Display for EntryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Where an entry sits in the collection it came from, counting every entry
/// of that collection, including Disabled Tracks that were skipped.
///
/// Persisted with the entry, so a continuation after an edit is found by
/// position rather than by counting earlier duplicates of the same Track
/// (`phase-2.md`, legacy trap 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CollectionPosition(pub u64);

/// How an entry entered the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Origin {
    /// Part of the collection being played.
    Collection {
        /// Its position in that collection.
        position: CollectionPosition,
    },
    /// A Queued Request: added with Add to queue or Play Next.
    Request,
}

/// Who put an entry into the queue.
///
/// The queue never interprets `id`; the caller owns it (in a Ristle Party,
/// the member id). `display_name` is stored with the entry on purpose: the
/// queue outlives the Party, and the roster that could resolve the id may be
/// gone when the entry is read back.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Submitter {
    /// The caller's opaque id for the submitter.
    pub id: String,
    /// The name to show.
    pub display_name: String,
}

/// One entry of the Canonical Queue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueueEntry {
    /// Its id.
    pub id: EntryId,
    /// The Track it plays, with the metadata known when it was queued.
    pub track: Track,
    /// How it entered the queue.
    pub origin: Origin,
    /// The sequence slot it materializes, when the collection is walked as a
    /// sequence.
    pub slot: Option<SlotId>,
    /// Who queued it. `None` when the local listener did, which is the
    /// common case.
    pub submitter: Option<Submitter>,
}

/// What the queue repeats.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RepeatMode {
    /// Nothing: playback stops after the last entry.
    #[default]
    Off,
    /// The current entry, until the user skips.
    Track,
    /// The collection, which starts over after its last entry.
    Queue,
}

/// Why the queue moves to the next entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Advance {
    /// The current Track played to its end. With [`RepeatMode::Track`] it
    /// plays again.
    TrackEnded,
    /// The user asked for the next Track. This leaves a repeated Track.
    Skip,
}
