//! The Local Track index.
//!
//! Files found under a Folder Grant are fingerprinted by the core and
//! grouped into Local Tracks by content. Every file stays as a hidden
//! playback candidate; the preferred one is FLAC, then MP3, then any other
//! format, and within a rank the higher bitrate (`CONTEXT.md`, Local Track).
//! Each file keeps the metadata read from it, and a Local Track shows that
//! of its preferred available file, or of its preferred file when none is
//! available, so the choice follows every reindex and rescan.
//!
//! A rescan marks every file unavailable ([`LocalIndex::begin_scan`]), then
//! confirms each file still found with the same size and modification time
//! ([`LocalIndex::confirm_file`]); a changed file is indexed again with
//! [`LocalIndex::upsert`]. A Local Track is available while one of its files
//! is.

use ristle_domain::{Availability, ConnectionId, Track};

use crate::sql::{flag, int, read_flag, to_u64, uint};
use crate::{Error, Tx};

const TRACKS: &str = "local_tracks";
const FILES: &str = "local_files";

/// The reason a Local Track without a reachable file reads as unavailable.
pub const UNAVAILABLE_REASON: &str = "Local file is unavailable";

/// The content identity of a Local Track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalContent {
    /// Ristle's id for the content, the Local Track's id.
    pub content_id: String,
    /// The acoustic fingerprint.
    pub fingerprint: String,
    /// A hash of the fingerprint, for finding candidates to compare.
    pub fingerprint_hash: u32,
    /// The decoded duration, when known.
    pub duration_ms: Option<u64>,
}

/// One file of a Local Track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalFile {
    /// Where it is, as the Local Files Plugin names it.
    pub location: String,
    /// The Local Track it belongs to.
    pub content_id: String,
    /// Lowercase container or codec name, such as `flac` or `mp3`.
    pub format: String,
    /// Its bitrate.
    pub bitrate_kbps: u32,
    /// Its size, to recognise it unchanged on a rescan.
    pub size_bytes: u64,
    /// Its modification time, to recognise it unchanged on a rescan.
    pub modified_ms: u64,
    /// Whether the last scan found it.
    pub available: bool,
}

/// The `local_tracks` and `local_files` tables.
pub struct LocalIndex<'a> {
    tx: &'a Tx<'a>,
}

/// SQL ordering files by preference.
const PREFERENCE: &str =
    "CASE format WHEN 'flac' THEN 0 WHEN 'mp3' THEN 1 ELSE 2 END, bitrate_kbps DESC, location";

impl<'a> LocalIndex<'a> {
    pub(crate) fn new(tx: &'a Tx<'a>) -> Self {
        Self { tx }
    }

    /// Indexes one file of a Local Track with the metadata read from it,
    /// stamped with the clock.
    pub fn upsert(
        &self,
        connection: &ConnectionId,
        content: &LocalContent,
        track: &Track,
        file: &LocalFile,
    ) -> Result<(), Error> {
        let conn = self.tx.write_conn()?;
        if content.content_id.is_empty() || content.fingerprint.is_empty() {
            return Err(Error::invalid(
                "Local Track",
                "the content id or fingerprint is empty",
            ));
        }
        if file.content_id != content.content_id || file.location.is_empty() {
            return Err(Error::invalid(
                "local file",
                "it names another Local Track or no location",
            ));
        }
        if track.reference.id() != content.content_id {
            return Err(Error::invalid(
                "Local Track",
                "the Track id is not the content id",
            ));
        }
        let json = serde_json::to_string(track)
            .map_err(|error| Error::invalid("Track", error.to_string()))?;
        conn.prepare_cached(
            "INSERT INTO local_tracks (connection_id, content_id, fingerprint, fingerprint_hash,
                 duration_ms, indexed_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (connection_id, content_id) DO UPDATE SET
                 fingerprint = excluded.fingerprint, fingerprint_hash = excluded.fingerprint_hash,
                 duration_ms = excluded.duration_ms, indexed_ms = excluded.indexed_ms",
        )?
        .execute(rusqlite::params![
            connection.as_str(),
            content.content_id,
            content.fingerprint,
            i64::from(content.fingerprint_hash),
            content
                .duration_ms
                .map(|d| int(d, "duration"))
                .transpose()?,
            int(self.tx.now_ms(), "index time")?,
        ])?;
        conn.prepare_cached(
            "INSERT INTO local_files (connection_id, location, content_id, format, bitrate_kbps,
                 size_bytes, modified_ms, available, track)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT (connection_id, location) DO UPDATE SET content_id = excluded.content_id,
                 format = excluded.format, bitrate_kbps = excluded.bitrate_kbps,
                 size_bytes = excluded.size_bytes, modified_ms = excluded.modified_ms,
                 available = excluded.available, track = excluded.track",
        )?
        .execute(rusqlite::params![
            connection.as_str(),
            file.location,
            file.content_id,
            file.format,
            i64::from(file.bitrate_kbps),
            int(file.size_bytes, "file size")?,
            int(file.modified_ms, "modification time")?,
            flag(file.available),
            json,
        ])?;
        // A file that moved to other content may leave a Local Track
        // without files; it no longer exists.
        conn.prepare_cached(
            "DELETE FROM local_tracks WHERE connection_id = ?1 AND NOT EXISTS (
                 SELECT 1 FROM local_files f
                 WHERE f.connection_id = local_tracks.connection_id
                   AND f.content_id = local_tracks.content_id)",
        )?
        .execute([connection.as_str()])?;
        Ok(())
    }

    /// The file to play for a Local Track: the preferred available one.
    pub fn best_file(
        &self,
        connection: &ConnectionId,
        content_id: &str,
    ) -> Result<Option<LocalFile>, Error> {
        Ok(self
            .files(connection, content_id)?
            .into_iter()
            .find(|file| file.available))
    }

    /// Every file of a Local Track, preferred first, available or not. The
    /// fallback order when the best file fails.
    pub fn files(
        &self,
        connection: &ConnectionId,
        content_id: &str,
    ) -> Result<Vec<LocalFile>, Error> {
        let mut select = self.tx.conn().prepare_cached(&format!(
            "SELECT location, content_id, format, bitrate_kbps, size_bytes, modified_ms, available
             FROM local_files WHERE connection_id = ?1 AND content_id = ?2 ORDER BY {PREFERENCE}"
        ))?;
        let mut rows = select.query([connection.as_str(), content_id])?;
        let mut files = Vec::new();
        while let Some(row) = rows.next()? {
            files.push(read_file(row)?);
        }
        Ok(files)
    }

    /// The file stored at a location.
    pub fn file(
        &self,
        connection: &ConnectionId,
        location: &str,
    ) -> Result<Option<LocalFile>, Error> {
        let mut select = self.tx.conn().prepare_cached(
            "SELECT location, content_id, format, bitrate_kbps, size_bytes, modified_ms, available
             FROM local_files WHERE connection_id = ?1 AND location = ?2",
        )?;
        let mut rows = select.query([connection.as_str(), location])?;
        rows.next()?.map(read_file).transpose()
    }

    /// Every Local Track of a connection, by content id, available while one
    /// of its files is.
    pub fn tracks(&self, connection: &ConnectionId) -> Result<Vec<Track>, Error> {
        let mut select = self.tx.conn().prepare_cached(&format!(
            "SELECT (SELECT track FROM local_files f
                     WHERE f.connection_id = t.connection_id AND f.content_id = t.content_id
                     ORDER BY available DESC, {PREFERENCE} LIMIT 1),
                    EXISTS (SELECT 1 FROM local_files f WHERE f.connection_id = t.connection_id
                            AND f.content_id = t.content_id AND f.available = 1)
             FROM local_tracks t WHERE t.connection_id = ?1 ORDER BY t.content_id"
        ))?;
        let mut rows = select.query([connection.as_str()])?;
        let mut tracks = Vec::new();
        while let Some(row) = rows.next()? {
            let json: String = row.get(0)?;
            let mut track: Track = serde_json::from_str(&json).map_err(|error| {
                Error::corrupt(TRACKS, format!("Track does not decode: {error}"))
            })?;
            track.availability = if read_flag(row.get(1)?, TRACKS, "available")? {
                Availability::Available
            } else {
                Availability::Unavailable {
                    reason: Some(UNAVAILABLE_REASON.to_owned()),
                }
            };
            tracks.push(track);
        }
        Ok(tracks)
    }

    /// Every Local Track's content identity, by content id.
    pub fn contents(&self, connection: &ConnectionId) -> Result<Vec<LocalContent>, Error> {
        let mut select = self.tx.conn().prepare_cached(
            "SELECT content_id, fingerprint, fingerprint_hash, duration_ms
             FROM local_tracks WHERE connection_id = ?1 ORDER BY content_id",
        )?;
        let mut rows = select.query([connection.as_str()])?;
        let mut contents = Vec::new();
        while let Some(row) = rows.next()? {
            contents.push(LocalContent {
                content_id: row.get(0)?,
                fingerprint: row.get(1)?,
                fingerprint_hash: u32::try_from(row.get::<_, i64>(2)?)
                    .map_err(|_| Error::corrupt(TRACKS, "fingerprint_hash is out of range"))?,
                duration_ms: row
                    .get::<_, Option<i64>>(3)?
                    .map(|d| uint(d, TRACKS, "duration_ms"))
                    .transpose()?,
            });
        }
        Ok(contents)
    }

    /// Starts a rescan: every file of the connection is unavailable until
    /// [`LocalIndex::confirm_file`] or [`LocalIndex::upsert`] finds it again.
    pub fn begin_scan(&self, connection: &ConnectionId) -> Result<u64, Error> {
        Ok(to_u64(
            self.tx
                .write_conn()?
                .prepare_cached("UPDATE local_files SET available = 0 WHERE connection_id = ?1")?
                .execute([connection.as_str()])?,
        ))
    }

    /// Marks a file available again if it is unchanged: same size and
    /// modification time. `false` means it must be indexed again.
    pub fn confirm_file(
        &self,
        connection: &ConnectionId,
        location: &str,
        size_bytes: u64,
        modified_ms: u64,
    ) -> Result<bool, Error> {
        Ok(self
            .tx
            .write_conn()?
            .prepare_cached(
                "UPDATE local_files SET available = 1
                 WHERE connection_id = ?1 AND location = ?2 AND size_bytes = ?3 AND modified_ms = ?4",
            )?
            .execute(rusqlite::params![
                connection.as_str(),
                location,
                int(size_bytes, "file size")?,
                int(modified_ms, "modification time")?,
            ])?
            > 0)
    }
}

fn read_file(row: &rusqlite::Row<'_>) -> Result<LocalFile, Error> {
    Ok(LocalFile {
        location: row.get(0)?,
        content_id: row.get(1)?,
        format: row.get(2)?,
        bitrate_kbps: u32::try_from(row.get::<_, i64>(3)?)
            .map_err(|_| Error::corrupt(FILES, "bitrate_kbps is out of range"))?,
        size_bytes: uint(row.get(4)?, FILES, "size_bytes")?,
        modified_ms: uint(row.get(5)?, FILES, "modified_ms")?,
        available: read_flag(row.get(6)?, FILES, "available")?,
    })
}
