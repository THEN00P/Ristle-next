//! The repositories. Each borrows a [`crate::Tx`], so everything one storage
//! closure does through them commits or rolls back together.

mod cache;
mod local;
mod playlists;
mod plugins;
mod sequences;
mod session;

pub use cache::{CachedPage, CachedResponse, HttpCache, HttpKey, PageCache, PageKey};
pub use local::{LocalContent, LocalFile, LocalIndex, UNAVAILABLE_REASON};
pub use playlists::{PlaylistSummary, Playlists, RecentlyPlayed, RecentlyPlayedEntry, Settings};
pub use plugins::{
    Account, ConfirmedOrigin, ConnectionRecord, Connections, FolderGrant, Grants, KvScope,
    PluginKv, PluginRecord, Plugins, Receipts, ServerGrant,
};
pub use sequences::Sequences;
pub use session::{Intent, PlaybackSession, RestoredSession, Session};
