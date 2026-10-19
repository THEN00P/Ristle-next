//! Ristle Playlists.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::track::Track;

/// A Ristle Playlist's id, assigned by Ristle.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlaylistId(String);

impl PlaylistId {
    /// Wraps an id.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The id as it was given.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PlaylistId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A playlist owned by Ristle. It may mix Tracks from several Providers,
/// keeps duplicates, and keeps Disabled Tracks with their cached metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RistlePlaylist {
    /// Its id.
    pub id: PlaylistId,
    /// Its title.
    pub title: String,
    /// Its Tracks in order, duplicates included.
    pub tracks: Vec<Track>,
    /// When it was created, in milliseconds since the Unix epoch.
    pub created_ms: u64,
    /// When its title or Tracks last changed, in milliseconds since the Unix
    /// epoch.
    pub updated_ms: u64,
}
