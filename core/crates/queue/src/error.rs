//! Errors of the queue crate.

use crate::entry::EntryId;
use crate::sequence::SlotId;

/// Why persisted queue or sequence state was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CorruptState {
    /// Two entries share an id.
    #[error("queue entry {0} appears twice")]
    DuplicateEntry(EntryId),
    /// An entry's id is not below the next id to hand out, so a new entry
    /// could repeat it.
    #[error("queue entry {entry} is not below the next id {next_id}")]
    EntryNotBeforeNextId {
        /// The entry.
        entry: EntryId,
        /// The next id.
        next_id: EntryId,
    },
    /// A collection entry is in the request lane, or plays detached.
    #[error("queue entry {0} is a collection entry outside the collection")]
    MisplacedEntry(EntryId),
    /// The current entry is at the cursor, but there is no cursor.
    #[error("the current entry is at the cursor, but there is no cursor")]
    CurrentWithoutCursor,
    /// More Play Next entries than requests.
    #[error("{play_next_count} Play Next entries among {requests} requests")]
    PlayNextBeyondRequests {
        /// The persisted Play Next count.
        play_next_count: usize,
        /// The number of requests.
        requests: usize,
    },
    /// A cursor points past the end.
    #[error("cursor {cursor} is past the end ({len} entries)")]
    CursorOutOfRange {
        /// The cursor.
        cursor: usize,
        /// The length it indexes.
        len: usize,
    },
    /// A sequence has no source to read.
    #[error("the sequence has no source")]
    NoSources,
    /// Two slots share an id.
    #[error("slot {0} appears twice")]
    DuplicateSlot(SlotId),
    /// A slot's id is not below the next slot id.
    #[error("slot {slot} is not below the next slot id {next_slot}")]
    SlotNotBeforeNextSlot {
        /// The slot.
        slot: SlotId,
        /// The next slot id.
        next_slot: SlotId,
    },
    /// A slot names a source the sequence does not have.
    #[error("slot {0} names a source the sequence does not have")]
    UnknownSource(SlotId),
    /// A slot outside an offset-addressed collection has no Track.
    #[error("slot {0} is unresolved but its source is not offset-addressed")]
    UnresolvedSlot(SlotId),
    /// The play order is not a permutation of the slots.
    #[error("the play order is not a permutation of the slots")]
    OrderNotAPermutation,
    /// The materialized end is past the order or not past the cursor.
    #[error("materialized end {materialized_end} is out of range ({len} slots)")]
    MaterializedEndOutOfRange {
        /// The materialized end.
        materialized_end: usize,
        /// The number of slots.
        len: usize,
    },
}

/// Why a sequence could not start.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidSequence {
    /// There is nothing to play.
    #[error("the sequence has no slots")]
    Empty,
    /// The start slot is not in the sequence.
    #[error("the start slot {0} is not in the sequence")]
    UnknownStart(SlotId),
    /// The slots break a sequence invariant.
    #[error(transparent)]
    Invalid(#[from] CorruptState),
}

/// Every error this crate returns.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// Persisted state was rejected.
    #[error(transparent)]
    CorruptState(#[from] CorruptState),
    /// A sequence could not start.
    #[error(transparent)]
    InvalidSequence(#[from] InvalidSequence),
}
