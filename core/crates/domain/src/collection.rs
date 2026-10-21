//! How Provider collections are named and read.

use serde::{Deserialize, Serialize};

use crate::ids::ConnectionId;

/// The collection a sequence or page is read from: one Provider Connection's
/// list, such as `playlist:42` or Liked Songs.
///
/// This is the routing id used to read the list. The entity the user opened,
/// kept for display and navigation, is a separate value (`phase-2.md`,
/// legacy trap 5).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CollectionRef {
    /// The Provider Connection that reads it.
    pub connection: ConnectionId,
    /// The Plugin's id for the list, stable for the same list. Opaque.
    pub id: String,
}

/// How the entries of a collection may be addressed
/// (`docs/plugin-api/walkthrough.md`, "Spotify Liked Songs").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Addressing {
    /// Pages follow each other through a `next` token. Entries carry stable
    /// Track ids.
    Cursor,
    /// Any offset can be read directly. A position names a slot, not an
    /// item, and moves when the collection changes, so a position is never
    /// played before it has been read in the current revision.
    Offset,
}

/// A count a Provider reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Count {
    /// The exact number.
    Exact(u64),
    /// The Provider's estimate.
    Estimate(u64),
}

impl Count {
    /// The number, exact or not.
    pub fn value(self) -> u64 {
        match self {
            Self::Exact(n) | Self::Estimate(n) => n,
        }
    }
}

/// What the host knows about a collection before walking it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CollectionDescriptor {
    /// Which collection.
    pub reference: CollectionRef,
    /// How its entries are addressed.
    pub addressing: Addressing,
    /// How many entries it has, when the Provider says.
    pub total: Option<Count>,
    /// Changes whenever the content or order changes (an etag, Spotify's
    /// `snapshot_id`). `None` when the Provider offers nothing reliable.
    pub revision: Option<String>,
    /// Never ends, like a generated radio: played in order, never shuffled
    /// as a whole.
    pub endless: bool,
}
