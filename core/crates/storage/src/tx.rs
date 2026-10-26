//! The transaction every storage closure receives, and its repositories.

use ristle_domain::Clock;
use rusqlite::Connection;

use crate::Error;
use crate::repo::{
    Connections, Grants, HttpCache, LocalIndex, PageCache, Playlists, PluginKv, Plugins, Receipts,
    RecentlyPlayed, Sequences, Session, Settings,
};

/// One transaction on the storage thread. Repositories hang off it, so
/// everything one closure does commits or rolls back together.
pub struct Tx<'t> {
    conn: &'t Connection,
    clock: &'t dyn Clock,
    writable: bool,
}

impl<'t> Tx<'t> {
    pub(crate) fn new(conn: &'t Connection, clock: &'t dyn Clock, writable: bool) -> Self {
        Self {
            conn,
            clock,
            writable,
        }
    }

    /// The storage's clock, read once per call that stamps a time.
    pub fn now_ms(&self) -> u64 {
        self.clock.now_ms()
    }

    pub(crate) fn conn(&self) -> &Connection {
        self.conn
    }

    /// The connection, for a write. Fails inside [`crate::Storage::read`].
    pub(crate) fn write_conn(&self) -> Result<&Connection, Error> {
        if self.writable {
            Ok(self.conn)
        } else {
            Err(Error::ReadOnly)
        }
    }

    /// The Playback Session and its Canonical Queue.
    pub fn session(&self) -> Session<'_> {
        Session::new(self)
    }

    /// Playable sequences.
    pub fn sequences(&self) -> Sequences<'_> {
        Sequences::new(self)
    }

    /// Ristle Playlists.
    pub fn playlists(&self) -> Playlists<'_> {
        Playlists::new(self)
    }

    /// Recently Played.
    pub fn recently_played(&self) -> RecentlyPlayed<'_> {
        RecentlyPlayed::new(self)
    }

    /// App settings.
    pub fn settings(&self) -> Settings<'_> {
        Settings::new(self)
    }

    /// Installed Plugins.
    pub fn plugins(&self) -> Plugins<'_> {
        Plugins::new(self)
    }

    /// Plugin Permissions, Folder Grants and Server Grants.
    pub fn grants(&self) -> Grants<'_> {
        Grants::new(self)
    }

    /// Provider Connections and their response sequences.
    pub fn connections(&self) -> Connections<'_> {
        Connections::new(self)
    }

    /// The Plugin key-value stores: Plugin-wide `storage` and per-connection
    /// data.
    pub fn plugin_kv(&self) -> PluginKv<'_> {
        PluginKv::new(self)
    }

    /// Command receipts.
    pub fn receipts(&self) -> Receipts<'_> {
        Receipts::new(self)
    }

    /// The Local Track index.
    pub fn local_index(&self) -> LocalIndex<'_> {
        LocalIndex::new(self)
    }

    /// Cached Provider pages (`cache.sqlite`).
    pub fn page_cache(&self) -> PageCache<'_> {
        PageCache::new(self)
    }

    /// Cached HTTP responses (`cache.sqlite`).
    pub fn http_cache(&self) -> HttpCache<'_> {
        HttpCache::new(self)
    }
}
