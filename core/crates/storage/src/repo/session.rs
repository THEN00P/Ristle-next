//! The Playback Session: the Canonical Queue, the active sequence and the
//! transport.

use ristle_queue::sequence::SequenceId;
use ristle_queue::sequence::SlotId;
use ristle_queue::{
    CollectionPosition, Current, EntryId, Origin, QueueEntry, QueueState, RepeatMode, Submitter,
    Trimmed,
};
use rusqlite::types::Value;

use crate::sql::{
    Changes, RowSet, TrackColumns, index, int, optional, read_track, read_track_ref, seed_from_sql,
    seed_to_sql, track_ref_values, uindex, uint,
};
use crate::{Error, Tx};

const SESSION: &str = "playback_session";
const ENTRIES: &str = "queue_entries";
const TRIMMED: &str = "queue_trimmed";

/// Whether playback was running when the session was saved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Intent {
    /// Paused, or stopped.
    #[default]
    Paused,
    /// Playing.
    Playing,
}

/// What survives a restart: the Canonical Queue, the active sequence and
/// where playback stood.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackSession {
    /// The Canonical Queue, exactly.
    pub queue: QueueState,
    /// The sequence the collection is materialized from, if any. It must be
    /// saved through [`crate::repo::Sequences`] in the same transaction or
    /// earlier.
    pub sequence: Option<SequenceId>,
    /// Where the current Track stood.
    pub position_ms: u64,
    /// Whether it was playing.
    pub intent: Intent,
}

/// A session as [`Session::load_for_cold_start`] reads it back.
#[derive(Debug, Clone, PartialEq)]
pub struct RestoredSession {
    /// The session. Its intent is always [`Intent::Paused`]: a cold start
    /// never starts playing by itself.
    pub session: PlaybackSession,
    /// What the saved intent was.
    pub was_playing: bool,
    /// When it was saved, by the storage's clock.
    pub saved_ms: u64,
}

/// The `playback_session` and `queue_entries` tables.
pub struct Session<'a> {
    tx: &'a Tx<'a>,
}

const ENTRY_ROWS: RowSet = RowSet {
    table: ENTRIES,
    scope: None,
    key: "entry_id",
    columns: &[
        "lane",
        "position",
        "origin",
        "collection_position",
        "slot_id",
        "submitter_id",
        "submitter_name",
        "provider_id",
        "track_id",
        "connection_id",
        "track",
    ],
};

const TRIMMED_ROWS: RowSet = RowSet {
    table: TRIMMED,
    scope: None,
    key: "trim_index",
    columns: &[
        "collection_position",
        "provider_id",
        "track_id",
        "connection_id",
    ],
};

const LANE_COLLECTION: i64 = 0;
const LANE_REQUESTS: i64 = 1;
const LANE_DETACHED: i64 = 2;

impl<'a> Session<'a> {
    pub(crate) fn new(tx: &'a Tx<'a>) -> Self {
        Self { tx }
    }

    /// Saves the session. Queue entries that did not change are not
    /// rewritten; the session row always is, stamped with the clock.
    ///
    /// Refuses queue state that [`QueueState::check`] rejects, so a corrupt
    /// queue is never persisted. Every row is built and checked before the
    /// first write, so a refused save writes nothing, even when the caller
    /// handles the error and commits the rest of its transaction.
    pub fn save(&self, session: &PlaybackSession) -> Result<Changes, Error> {
        let conn = self.tx.write_conn()?;
        let queue = &session.queue;
        queue
            .check()
            .map_err(|corrupt| Error::invalid("queue state", corrupt.to_string()))?;

        let current = match &queue.current {
            None => 0,
            Some(Current::AtCursor) => 1,
            Some(Current::Detached(_)) => 2,
        };
        let repeat = match queue.repeat {
            RepeatMode::Off => 0,
            RepeatMode::Track => 1,
            RepeatMode::Queue => 2,
        };
        let intent = match session.intent {
            Intent::Paused => 0,
            Intent::Playing => 1,
        };
        let header = rusqlite::params![
            queue.cursor.map(|c| index(c, "queue cursor")).transpose()?,
            index(queue.play_next_count, "Play Next count")?,
            current,
            repeat,
            queue.shuffle.map(seed_to_sql),
            int(queue.next_id.0, "next entry id")?,
            session.sequence.as_ref().map(|id| id.0.as_str()),
            int(session.position_ms, "playback position")?,
            intent,
            int(self.tx.now_ms(), "save time")?,
        ];

        let mut rows = Vec::with_capacity(queue.collection.len() + queue.requests.len() + 1);
        for (lane, entries) in [
            (LANE_COLLECTION, queue.collection.as_slice()),
            (LANE_REQUESTS, queue.requests.as_slice()),
        ] {
            for (position, entry) in entries.iter().enumerate() {
                rows.push(entry_row(entry, lane, position)?);
            }
        }
        if let Some(Current::Detached(entry)) = &queue.current {
            rows.push(entry_row(entry, LANE_DETACHED, 0)?);
        }
        let trimmed = queue
            .trimmed
            .iter()
            .enumerate()
            .map(|(i, trimmed)| trimmed_row(trimmed, i))
            .collect::<Result<Vec<_>, Error>>()?;

        conn.prepare_cached(
            "INSERT INTO playback_session (id, cursor, play_next_count, current, repeat,
                 shuffle_seed, next_entry_id, sequence_id, position_ms, intent, updated_ms)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT (id) DO UPDATE SET cursor = excluded.cursor,
                 play_next_count = excluded.play_next_count, current = excluded.current,
                 repeat = excluded.repeat, shuffle_seed = excluded.shuffle_seed,
                 next_entry_id = excluded.next_entry_id, sequence_id = excluded.sequence_id,
                 position_ms = excluded.position_ms, intent = excluded.intent,
                 updated_ms = excluded.updated_ms",
        )?
        .execute(header)?;
        let mut changes = ENTRY_ROWS.sync(conn, None, rows)?;
        changes += TRIMMED_ROWS.sync(conn, None, trimmed)?;
        Ok(changes)
    }

    /// The saved session, paused, or `None` when nothing was saved.
    pub fn load_for_cold_start(&self) -> Result<Option<RestoredSession>, Error> {
        let conn = self.tx.conn();
        let Some(row) = optional(
            conn,
            "SELECT cursor, play_next_count, current, repeat, shuffle_seed, next_entry_id,
                    sequence_id, position_ms, intent, updated_ms
             FROM playback_session WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                ))
            },
        )?
        else {
            return Ok(None);
        };
        let (
            cursor,
            play_next_count,
            current,
            repeat,
            shuffle,
            next_id,
            sequence,
            position,
            intent,
            saved,
        ) = row;

        let mut collection = Vec::new();
        let mut requests = Vec::new();
        let mut detached = Vec::new();
        {
            let mut select = conn.prepare_cached(
                "SELECT entry_id, lane, position, origin, collection_position, slot_id,
                        submitter_id, submitter_name, provider_id, track_id, connection_id, track
                 FROM queue_entries ORDER BY lane, position",
            )?;
            let mut stored = select.query([])?;
            while let Some(row) = stored.next()? {
                let lane: i64 = row.get(1)?;
                let position = uindex(row.get(2)?, ENTRIES, "position")?;
                let origin = match (row.get::<_, i64>(3)?, row.get::<_, Option<i64>>(4)?) {
                    (0, Some(p)) => Origin::Collection {
                        position: CollectionPosition(uint(p, ENTRIES, "collection_position")?),
                    },
                    (1, None) => Origin::Request,
                    _ => {
                        return Err(Error::corrupt(
                            ENTRIES,
                            "origin and collection position disagree",
                        ));
                    }
                };
                let submitter = match (
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                ) {
                    (Some(id), Some(display_name)) => Some(Submitter { id, display_name }),
                    (None, None) => None,
                    _ => return Err(Error::corrupt(ENTRIES, "a submitter is incomplete")),
                };
                let provider: String = row.get(8)?;
                let track_id: String = row.get(9)?;
                let connection: Option<String> = row.get(10)?;
                let track_json: String = row.get(11)?;
                let entry = QueueEntry {
                    id: EntryId(uint(row.get(0)?, ENTRIES, "entry_id")?),
                    track: read_track(
                        ENTRIES,
                        &track_json,
                        &provider,
                        &track_id,
                        connection.as_deref(),
                    )?,
                    origin,
                    slot: row
                        .get::<_, Option<i64>>(5)?
                        .map(|s| uint(s, ENTRIES, "slot_id").map(SlotId))
                        .transpose()?,
                    submitter,
                };
                let lane = match lane {
                    LANE_COLLECTION => &mut collection,
                    LANE_REQUESTS => &mut requests,
                    LANE_DETACHED => &mut detached,
                    other => return Err(Error::corrupt(ENTRIES, format!("unknown lane {other}"))),
                };
                if position != lane.len() {
                    return Err(Error::corrupt(
                        ENTRIES,
                        "positions in a lane are not contiguous",
                    ));
                }
                lane.push(entry);
            }
        }

        let mut trimmed = Vec::new();
        {
            let mut select = conn.prepare_cached(
                "SELECT trim_index, collection_position, provider_id, track_id, connection_id
                 FROM queue_trimmed ORDER BY trim_index",
            )?;
            let mut stored = select.query([])?;
            while let Some(row) = stored.next()? {
                if uindex(row.get(0)?, TRIMMED, "trim_index")? != trimmed.len() {
                    return Err(Error::corrupt(TRIMMED, "indices are not contiguous"));
                }
                let track = read_track_ref(TRIMMED, row.get(2)?, row.get(3)?, row.get(4)?)?
                    .ok_or_else(|| Error::corrupt(TRIMMED, "a trimmed entry has no Track"))?;
                trimmed.push(Trimmed {
                    track,
                    position: CollectionPosition(uint(
                        row.get(1)?,
                        TRIMMED,
                        "collection_position",
                    )?),
                });
            }
        }

        let current = match (current, detached.pop()) {
            (0, None) => None,
            (1, None) => Some(Current::AtCursor),
            (2, Some(entry)) if detached.is_empty() => Some(Current::Detached(Box::new(entry))),
            _ => {
                return Err(Error::corrupt(
                    SESSION,
                    "the current entry does not match its lane",
                ));
            }
        };
        let queue = QueueState {
            collection,
            cursor: cursor.map(|c| uindex(c, SESSION, "cursor")).transpose()?,
            requests,
            play_next_count: uindex(play_next_count, SESSION, "play_next_count")?,
            current,
            repeat: match repeat {
                0 => RepeatMode::Off,
                1 => RepeatMode::Track,
                2 => RepeatMode::Queue,
                other => {
                    return Err(Error::corrupt(
                        SESSION,
                        format!("unknown repeat mode {other}"),
                    ));
                }
            },
            shuffle: shuffle.map(seed_from_sql),
            next_id: EntryId(uint(next_id, SESSION, "next_entry_id")?),
            trimmed,
        };
        queue
            .check()
            .map_err(|corrupt| Error::corrupt(ENTRIES, corrupt.to_string()))?;
        let was_playing = match intent {
            0 => false,
            1 => true,
            other => return Err(Error::corrupt(SESSION, format!("unknown intent {other}"))),
        };
        Ok(Some(RestoredSession {
            session: PlaybackSession {
                queue,
                sequence: sequence.map(SequenceId),
                position_ms: uint(position, SESSION, "position_ms")?,
                intent: Intent::Paused,
            },
            was_playing,
            saved_ms: uint(saved, SESSION, "updated_ms")?,
        }))
    }

    /// Forgets the session and its queue.
    pub fn clear(&self) -> Result<(), Error> {
        let conn = self.tx.write_conn()?;
        conn.execute("DELETE FROM queue_entries", [])?;
        conn.execute("DELETE FROM queue_trimmed", [])?;
        conn.execute("DELETE FROM playback_session", [])?;
        Ok(())
    }
}

fn trimmed_row(trimmed: &Trimmed, trim_index: usize) -> Result<(i64, Vec<Value>), Error> {
    let mut values = vec![Value::Integer(int(
        trimmed.position.0,
        "collection position",
    )?)];
    values.extend(track_ref_values(Some(&trimmed.track)));
    Ok((index(trim_index, "trimmed index")?, values))
}

fn entry_row(entry: &QueueEntry, lane: i64, position: usize) -> Result<(i64, Vec<Value>), Error> {
    let (origin, collection_position) = match entry.origin {
        Origin::Collection { position } => {
            (0, Value::Integer(int(position.0, "collection position")?))
        }
        Origin::Request => (1, Value::Null),
    };
    let (submitter_id, submitter_name) = match &entry.submitter {
        Some(s) => (
            Value::Text(s.id.clone()),
            Value::Text(s.display_name.clone()),
        ),
        None => (Value::Null, Value::Null),
    };
    let mut values = vec![
        Value::Integer(lane),
        Value::Integer(index(position, "queue position")?),
        Value::Integer(origin),
        collection_position,
        entry
            .slot
            .map(|s| int(s.0, "slot id").map(Value::Integer))
            .transpose()?
            .unwrap_or(Value::Null),
        submitter_id,
        submitter_name,
    ];
    values.extend(TrackColumns::new(&entry.track)?.values());
    Ok((int(entry.id.0, "entry id")?, values))
}
