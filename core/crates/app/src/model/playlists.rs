//! Ristle Playlists and Playlist Add.

use super::items::ItemId;

/// A Ristle Playlist in a list of playlists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistSummary {
    /// The playlist's id.
    pub id: String,
    /// The title.
    pub title: String,
    /// The number of Tracks.
    pub track_count: u64,
    /// When it last changed, in milliseconds since the Unix epoch.
    pub updated_at_ms: u64,
}

/// Where a Playlist Add goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaylistDestination {
    /// An existing Ristle Playlist.
    Ristle {
        /// The playlist's id.
        id: String,
    },
    /// A new Ristle Playlist with this title.
    NewRistle {
        /// The title.
        title: String,
    },
    /// An editable Provider Playlist. Only Tracks of its Provider are added.
    Provider {
        /// The playlist.
        item: ItemId,
    },
}

/// What to do with Tracks the destination already has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Duplicates {
    /// Add nothing yet if there are duplicates, and report them.
    Ask,
    /// Add every Track.
    AddAll,
    /// Add only Tracks the destination does not have.
    SkipExisting,
}

/// The result of a Playlist Add.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaylistAddOutcome {
    /// Tracks were added.
    Added {
        /// The destination (its id, when it was created by this add).
        playlist: String,
        /// How many Tracks were added.
        added: u64,
        /// How many were left out: duplicates skipped, Disabled Tracks, or
        /// Tracks of another Provider for a Provider Playlist.
        skipped: u64,
    },
    /// Only with [`Duplicates::Ask`]: nothing was added, because some
    /// Tracks are already there. Ask the user and add again.
    DuplicatesFound {
        /// Tracks that are already in the destination.
        duplicates: u64,
        /// Tracks the add would contain in all.
        total: u64,
    },
}
