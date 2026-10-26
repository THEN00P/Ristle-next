//! Ristle Playlists, Recently Played and settings.

use ristle_domain::{PlaylistId, RistlePlaylist, Track};
use rusqlite::types::Value;

use crate::sql::{Changes, RowSet, TrackColumns, index, int, optional, read_track, uint};
use crate::{Error, Tx};

const PLAYLISTS: &str = "ristle_playlists";
const ENTRIES: &str = "ristle_playlist_entries";
const RECENT: &str = "recently_played";

const ENTRY_ROWS: RowSet = RowSet {
    table: ENTRIES,
    scope: Some("playlist_id"),
    key: "position",
    columns: &["provider_id", "track_id", "connection_id", "track"],
};

/// A Ristle Playlist without its Tracks, for lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistSummary {
    /// Its id.
    pub id: PlaylistId,
    /// Its title.
    pub title: String,
    /// How many Tracks it holds, duplicates and Disabled Tracks included.
    pub track_count: u64,
    /// When it was created.
    pub created_ms: u64,
    /// When it last changed.
    pub updated_ms: u64,
}

/// The Ristle Playlist tables.
pub struct Playlists<'a> {
    tx: &'a Tx<'a>,
}

impl<'a> Playlists<'a> {
    pub(crate) fn new(tx: &'a Tx<'a>) -> Self {
        Self { tx }
    }

    /// Every playlist, by title and then id.
    pub fn list(&self) -> Result<Vec<PlaylistSummary>, Error> {
        let mut select = self.tx.conn().prepare_cached(
            "SELECT p.playlist_id, p.title, p.created_ms, p.updated_ms,
                    (SELECT count(*) FROM ristle_playlist_entries e WHERE e.playlist_id = p.playlist_id)
             FROM ristle_playlists p ORDER BY p.title, p.playlist_id",
        )?;
        let mut rows = select.query([])?;
        let mut summaries = Vec::new();
        while let Some(row) = rows.next()? {
            summaries.push(PlaylistSummary {
                id: PlaylistId::new(row.get::<_, String>(0)?),
                title: row.get(1)?,
                created_ms: uint(row.get(2)?, PLAYLISTS, "created_ms")?,
                updated_ms: uint(row.get(3)?, PLAYLISTS, "updated_ms")?,
                track_count: uint(row.get(4)?, PLAYLISTS, "track count")?,
            });
        }
        Ok(summaries)
    }

    /// One playlist with its Tracks.
    pub fn load(&self, id: &PlaylistId) -> Result<Option<RistlePlaylist>, Error> {
        let conn = self.tx.conn();
        let Some((title, created, updated)) = optional(
            conn,
            "SELECT title, created_ms, updated_ms FROM ristle_playlists WHERE playlist_id = ?1",
            [id.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )?
        else {
            return Ok(None);
        };
        let mut select = conn.prepare_cached(
            "SELECT position, provider_id, track_id, connection_id, track
             FROM ristle_playlist_entries WHERE playlist_id = ?1 ORDER BY position",
        )?;
        let mut rows = select.query([id.as_str()])?;
        let mut tracks = Vec::new();
        while let Some(row) = rows.next()? {
            if uint(row.get(0)?, ENTRIES, "position")? != tracks.len() as u64 {
                return Err(Error::corrupt(ENTRIES, "positions are not contiguous"));
            }
            let (provider, track_id, connection, json): (String, String, Option<String>, String) =
                (row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?);
            tracks.push(read_track(
                ENTRIES,
                &json,
                &provider,
                &track_id,
                connection.as_deref(),
            )?);
        }
        Ok(Some(RistlePlaylist {
            id: id.clone(),
            title,
            tracks,
            created_ms: uint(created, PLAYLISTS, "created_ms")?,
            updated_ms: uint(updated, PLAYLISTS, "updated_ms")?,
        }))
    }

    /// Creates or replaces a playlist, rewriting only the positions whose
    /// Track changed.
    pub fn save(&self, playlist: &RistlePlaylist) -> Result<Changes, Error> {
        let conn = self.tx.write_conn()?;
        let id = playlist.id.as_str();
        if id.is_empty() {
            return Err(Error::invalid("Ristle Playlist", "the id is empty"));
        }
        let mut changes = Changes::default();
        let existed = optional(
            conn,
            "SELECT 1 FROM ristle_playlists WHERE playlist_id = ?1",
            [id],
            |_| Ok(()),
        )?
        .is_some();
        let written = conn
            .prepare_cached(
                "INSERT INTO ristle_playlists (playlist_id, title, created_ms, updated_ms)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (playlist_id) DO UPDATE SET title = excluded.title,
                     created_ms = excluded.created_ms, updated_ms = excluded.updated_ms
                 WHERE (title, created_ms, updated_ms)
                       IS NOT (excluded.title, excluded.created_ms, excluded.updated_ms)",
            )?
            .execute(rusqlite::params![
                id,
                playlist.title,
                int(playlist.created_ms, "creation time")?,
                int(playlist.updated_ms, "update time")?,
            ])?;
        if existed {
            changes.updated += crate::sql::to_u64(written);
        } else {
            changes.inserted += crate::sql::to_u64(written);
        }
        let rows = playlist
            .tracks
            .iter()
            .enumerate()
            .map(|(position, track)| {
                Ok((
                    index(position, "playlist position")?,
                    TrackColumns::new(track)?.values().to_vec(),
                ))
            })
            .collect::<Result<Vec<_>, Error>>()?;
        changes += ENTRY_ROWS.sync(conn, Some(id), rows)?;
        Ok(changes)
    }

    /// Deletes a playlist. `false` when there was none.
    pub fn delete(&self, id: &PlaylistId) -> Result<bool, Error> {
        let conn = self.tx.write_conn()?;
        Ok(conn
            .prepare_cached("DELETE FROM ristle_playlists WHERE playlist_id = ?1")?
            .execute([id.as_str()])?
            > 0)
    }
}

/// One Recently Played entry.
#[derive(Debug, Clone, PartialEq)]
pub struct RecentlyPlayedEntry {
    /// The Track, with the metadata it had when it was played.
    pub track: Track,
    /// When it qualified.
    pub played_ms: u64,
}

/// The `recently_played` table.
pub struct RecentlyPlayed<'a> {
    tx: &'a Tx<'a>,
}

impl<'a> RecentlyPlayed<'a> {
    /// How many entries are kept.
    pub const LIMIT: u64 = 100;
    /// How long a Track must play to qualify, unless half its duration is
    /// shorter. Live Tracks and Tracks of unknown length always need this.
    pub const THRESHOLD_MS: u64 = 30_000;

    pub(crate) fn new(tx: &'a Tx<'a>) -> Self {
        Self { tx }
    }

    /// Records a Track that has played for `listened_ms`, stamped with the
    /// clock, if that qualifies it (`CONTEXT.md`, Recently Played). Keeps
    /// the latest [`Self::LIMIT`] entries. Returns whether it was recorded.
    pub fn record(&self, track: &Track, listened_ms: u64) -> Result<bool, Error> {
        let conn = self.tx.write_conn()?;
        let threshold = match track.duration_ms {
            Some(duration) if !track.capabilities.live && duration > 0 => {
                Self::THRESHOLD_MS.min(duration / 2)
            }
            _ => Self::THRESHOLD_MS,
        };
        if listened_ms < threshold {
            return Ok(false);
        }
        let mut values: Vec<Value> = TrackColumns::new(track)?.values().to_vec();
        values.push(Value::Integer(int(self.tx.now_ms(), "play time")?));
        conn.prepare_cached(
            "INSERT INTO recently_played (provider_id, track_id, connection_id, track, played_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?
        .execute(rusqlite::params_from_iter(values))?;
        conn.prepare_cached(
            "DELETE FROM recently_played WHERE entry_id NOT IN (
                 SELECT entry_id FROM recently_played
                 ORDER BY played_ms DESC, entry_id DESC LIMIT ?1)",
        )?
        .execute([int(Self::LIMIT, "limit")?])?;
        Ok(true)
    }

    /// The entries, latest first.
    pub fn list(&self) -> Result<Vec<RecentlyPlayedEntry>, Error> {
        let mut select = self.tx.conn().prepare_cached(
            "SELECT provider_id, track_id, connection_id, track, played_ms
             FROM recently_played ORDER BY played_ms DESC, entry_id DESC",
        )?;
        let mut rows = select.query([])?;
        let mut entries = Vec::new();
        while let Some(row) = rows.next()? {
            let (provider, id, connection, json): (String, String, Option<String>, String) =
                (row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?);
            entries.push(RecentlyPlayedEntry {
                track: read_track(RECENT, &json, &provider, &id, connection.as_deref())?,
                played_ms: uint(row.get(4)?, RECENT, "played_ms")?,
            });
        }
        Ok(entries)
    }

    /// Forgets every entry.
    pub fn clear(&self) -> Result<(), Error> {
        self.tx
            .write_conn()?
            .execute("DELETE FROM recently_played", [])?;
        Ok(())
    }
}

/// The `settings` table: app settings by key.
pub struct Settings<'a> {
    tx: &'a Tx<'a>,
}

impl<'a> Settings<'a> {
    pub(crate) fn new(tx: &'a Tx<'a>) -> Self {
        Self { tx }
    }

    /// The value stored under `key`.
    pub fn get(&self, key: &str) -> Result<Option<String>, Error> {
        optional(
            self.tx.conn(),
            "SELECT value FROM settings WHERE key = ?1",
            [key],
            |row| row.get(0),
        )
    }

    /// Stores `value` under `key`, replacing what was there.
    pub fn put(&self, key: &str, value: &str) -> Result<(), Error> {
        if key.is_empty() {
            return Err(Error::invalid("setting", "the key is empty"));
        }
        self.tx
            .write_conn()?
            .prepare_cached(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            )?
            .execute([key, value])?;
        Ok(())
    }

    /// Removes `key`. `false` when it was not set.
    pub fn remove(&self, key: &str) -> Result<bool, Error> {
        Ok(self
            .tx
            .write_conn()?
            .prepare_cached("DELETE FROM settings WHERE key = ?1")?
            .execute([key])?
            > 0)
    }
}
