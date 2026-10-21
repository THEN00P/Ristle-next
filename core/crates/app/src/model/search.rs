//! Unified Search.

use super::items::ItemView;

/// Which catalogs a search covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SearchScope {
    /// Everywhere Search: the full catalogs of every enabled Provider
    /// Connection.
    Everywhere,
    /// My Library Search: the user's own music and collections.
    MyLibrary,
}

/// The kind of results in a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SearchKind {
    /// Tracks.
    Tracks,
    /// Albums.
    Albums,
    /// Artists.
    Artists,
    /// Users.
    Users,
    /// Playlists.
    Playlists,
}

/// The results of one kind, with Providers interleaved by relevance band.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchGroup {
    /// The kind.
    pub kind: SearchKind,
    /// Best match first.
    pub items: Vec<ItemView>,
}

/// One stable snapshot of a Unified Search: taken once every Provider
/// answered or five seconds passed. Late answers never reorder it.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchSnapshot {
    /// Identifies the snapshot, for `PlaySource::SearchResults`.
    pub id: u64,
    /// The query as searched.
    pub query: String,
    /// One group per kind that has results, in display order.
    pub groups: Vec<SearchGroup>,
    /// Provider Connections whose results are missing: failed, or too late.
    pub missing: Vec<String>,
}
