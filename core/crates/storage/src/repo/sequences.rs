//! Playable sequences.
//!
//! A sequence is stored in four tables: `sequences` (the descriptor),
//! `sequence_routes` (its sources, once per sequence), `sequence_slots` (one
//! row per occurrence, with its natural and play index) and
//! `sequence_progress` (cursor, materialized end, seed, revision). A save
//! rewrites only rows whose values changed, so advancing writes one
//! progress row and a shuffle rewrites play indices but never routes
//! (`phase-2.md`, legacy trap 9).

use ristle_domain::{
    Addressing, CollectionDescriptor, CollectionRef, ConnectionId, Count, PlaylistId,
};
use ristle_queue::SequenceState;
use ristle_queue::sequence::{SequenceDescriptor, SequenceId, SequenceSource, Slot, SlotId};
use rusqlite::types::Value;

use crate::sql::{
    Changes, RowSet, flag, index, int, optional, read_flag, read_track_ref, seed_from_sql,
    seed_to_sql, to_u64, track_ref_values, uindex, uint,
};
use crate::{Error, Tx};

const SEQUENCES: &str = "sequences";
const ROUTES: &str = "sequence_routes";
const SLOTS: &str = "sequence_slots";
const PROGRESS: &str = "sequence_progress";

const ROUTE_ROWS: RowSet = RowSet {
    table: ROUTES,
    scope: Some("sequence_id"),
    key: "source_index",
    columns: &[
        "kind",
        "connection_id",
        "collection_id",
        "addressing",
        "total",
        "total_exact",
        "revision",
        "endless",
        "playlist_id",
    ],
};

const SLOT_ROWS: RowSet = RowSet {
    table: SLOTS,
    scope: Some("sequence_id"),
    key: "slot_id",
    columns: &[
        "natural_index",
        "play_index",
        "source_index",
        "source_offset",
        "provider_id",
        "track_id",
        "connection_id",
    ],
};

/// The sequence tables.
pub struct Sequences<'a> {
    tx: &'a Tx<'a>,
}

impl<'a> Sequences<'a> {
    pub(crate) fn new(tx: &'a Tx<'a>) -> Self {
        Self { tx }
    }

    /// Saves a sequence under its id, replacing what was saved under that id
    /// and writing only the rows that differ. Refuses state that
    /// [`SequenceState::check`] rejects.
    pub fn save(&self, state: &SequenceState) -> Result<Changes, Error> {
        let conn = self.tx.write_conn()?;
        state
            .check()
            .map_err(|corrupt| Error::invalid("sequence state", corrupt.to_string()))?;
        let descriptor = &state.descriptor;
        let id = descriptor.id.0.as_str();
        let opened = serde_json::to_string(&descriptor.opened)
            .map_err(|error| Error::invalid("sequence", error.to_string()))?;
        let next_slot = int(state.next_slot.0, "next slot id")?;

        let mut changes = Changes::default();
        let stored = optional(
            conn,
            "SELECT title, opened, next_slot FROM sequences WHERE sequence_id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )?;
        match stored {
            None => {
                conn.prepare_cached(
                    "INSERT INTO sequences (sequence_id, title, opened, next_slot)
                     VALUES (?1, ?2, ?3, ?4)",
                )?
                .execute(rusqlite::params![
                    id,
                    descriptor.title,
                    opened,
                    next_slot
                ])?;
                changes.inserted += 1;
            }
            Some(row) if row == (descriptor.title.clone(), opened.clone(), next_slot) => {}
            Some(_) => {
                changes.updated += to_u64(
                    conn.prepare_cached(
                        "UPDATE sequences SET title = ?2, opened = ?3, next_slot = ?4
                         WHERE sequence_id = ?1",
                    )?
                    .execute(rusqlite::params![
                        id,
                        descriptor.title,
                        opened,
                        next_slot
                    ])?,
                );
            }
        }

        let routes = descriptor
            .sources
            .iter()
            .enumerate()
            .map(|(i, source)| Ok((index(i, "source index")?, route_values(source)?)))
            .collect::<Result<Vec<_>, Error>>()?;
        changes += ROUTE_ROWS.sync(conn, Some(id), routes)?;

        let mut play_index = vec![0usize; state.slots.len()];
        let natural: std::collections::HashMap<SlotId, usize> = state
            .slots
            .iter()
            .enumerate()
            .map(|(i, slot)| (slot.id, i))
            .collect();
        for (position, slot) in state.order.iter().enumerate() {
            // `check` made the order a permutation of the slots.
            if let Some(&i) = natural.get(slot) {
                play_index[i] = position;
            }
        }
        let slots = state
            .slots
            .iter()
            .enumerate()
            .map(|(i, slot)| {
                let mut values = vec![
                    Value::Integer(index(i, "natural index")?),
                    Value::Integer(index(play_index[i], "play index")?),
                    Value::Integer(index(slot.source, "source index")?),
                    Value::Integer(int(slot.offset, "slot offset")?),
                ];
                values.extend(track_ref_values(slot.track.as_ref()));
                Ok((int(slot.id.0, "slot id")?, values))
            })
            .collect::<Result<Vec<_>, Error>>()?;
        changes += SLOT_ROWS.sync(conn, Some(id), slots)?;

        let progress = (
            state
                .cursor
                .map(|c| index(c, "sequence cursor"))
                .transpose()?,
            index(state.materialized_end, "materialized end")?,
            state.shuffle.map(seed_to_sql),
            int(state.revision, "sequence revision")?,
        );
        let stored = optional(
            conn,
            "SELECT cursor, materialized_end, shuffle_seed, revision
             FROM sequence_progress WHERE sequence_id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        if stored != Some(progress) {
            let written = conn
                .prepare_cached(
                    "INSERT INTO sequence_progress
                         (sequence_id, cursor, materialized_end, shuffle_seed, revision)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT (sequence_id) DO UPDATE SET cursor = excluded.cursor,
                         materialized_end = excluded.materialized_end,
                         shuffle_seed = excluded.shuffle_seed, revision = excluded.revision",
                )?
                .execute(rusqlite::params![
                    id, progress.0, progress.1, progress.2, progress.3
                ])?;
            if stored.is_some() {
                changes.updated += to_u64(written);
            } else {
                changes.inserted += to_u64(written);
            }
        }
        Ok(changes)
    }

    /// The sequence saved under `id`.
    pub fn load(&self, id: &SequenceId) -> Result<Option<SequenceState>, Error> {
        let conn = self.tx.conn();
        let key = id.0.as_str();
        let Some((title, opened, next_slot)) = optional(
            conn,
            "SELECT title, opened, next_slot FROM sequences WHERE sequence_id = ?1",
            [key],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )?
        else {
            return Ok(None);
        };
        let opened = serde_json::from_str(&opened).map_err(|error| {
            Error::corrupt(SEQUENCES, format!("opened does not decode: {error}"))
        })?;

        let mut sources = Vec::new();
        {
            let mut select = conn.prepare_cached(
                "SELECT source_index, kind, connection_id, collection_id, addressing, total,
                        total_exact, revision, endless, playlist_id
                 FROM sequence_routes WHERE sequence_id = ?1 ORDER BY source_index",
            )?;
            let mut rows = select.query([key])?;
            while let Some(row) = rows.next()? {
                if uindex(row.get(0)?, ROUTES, "source_index")? != sources.len() {
                    return Err(Error::corrupt(ROUTES, "source indices are not contiguous"));
                }
                sources.push(read_route(row)?);
            }
        }

        let mut slots = Vec::new();
        let mut order = Vec::new();
        {
            let mut select = conn.prepare_cached(
                "SELECT slot_id, natural_index, play_index, source_index, source_offset,
                        provider_id, track_id, connection_id
                 FROM sequence_slots WHERE sequence_id = ?1 ORDER BY natural_index",
            )?;
            let mut rows = select.query([key])?;
            while let Some(row) = rows.next()? {
                let id = SlotId(uint(row.get(0)?, SLOTS, "slot_id")?);
                if uindex(row.get(1)?, SLOTS, "natural_index")? != slots.len() {
                    return Err(Error::corrupt(SLOTS, "natural indices are not contiguous"));
                }
                order.push((uindex(row.get(2)?, SLOTS, "play_index")?, id));
                slots.push(Slot {
                    id,
                    source: uindex(row.get(3)?, SLOTS, "source_index")?,
                    offset: uint(row.get(4)?, SLOTS, "source_offset")?,
                    track: read_track_ref(SLOTS, row.get(5)?, row.get(6)?, row.get(7)?)?,
                });
            }
        }
        order.sort_unstable();
        if order.iter().enumerate().any(|(i, (play, _))| *play != i) {
            return Err(Error::corrupt(SLOTS, "play indices are not a permutation"));
        }

        let (cursor, materialized_end, shuffle, revision) = optional(
            conn,
            "SELECT cursor, materialized_end, shuffle_seed, revision
             FROM sequence_progress WHERE sequence_id = ?1",
            [key],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )?
        .ok_or_else(|| Error::corrupt(PROGRESS, format!("sequence {key} has no progress")))?;

        let state = SequenceState {
            descriptor: SequenceDescriptor {
                id: id.clone(),
                title,
                opened,
                sources,
            },
            slots,
            order: order.into_iter().map(|(_, id)| id).collect(),
            cursor: cursor.map(|c| uindex(c, PROGRESS, "cursor")).transpose()?,
            materialized_end: uindex(materialized_end, PROGRESS, "materialized_end")?,
            shuffle: shuffle.map(seed_from_sql),
            revision: uint(revision, PROGRESS, "revision")?,
            next_slot: SlotId(uint(next_slot, SEQUENCES, "next_slot")?),
        };
        state
            .check()
            .map_err(|corrupt| Error::corrupt(SEQUENCES, corrupt.to_string()))?;
        Ok(Some(state))
    }

    /// Deletes a sequence with its routes, slots and progress. Fails at
    /// commit while the Playback Session still names it.
    pub fn delete(&self, id: &SequenceId) -> Result<bool, Error> {
        let conn = self.tx.write_conn()?;
        let deleted = conn
            .prepare_cached("DELETE FROM sequences WHERE sequence_id = ?1")?
            .execute([id.0.as_str()])?;
        Ok(deleted > 0)
    }

    /// The ids of every saved sequence, sorted.
    pub fn ids(&self) -> Result<Vec<SequenceId>, Error> {
        let mut select = self
            .tx
            .conn()
            .prepare_cached("SELECT sequence_id FROM sequences ORDER BY sequence_id")?;
        let ids = select
            .query_map([], |row| row.get::<_, String>(0).map(SequenceId))?
            .collect::<Result<_, _>>()?;
        Ok(ids)
    }
}

fn route_values(source: &SequenceSource) -> Result<Vec<Value>, Error> {
    let text = |s: &str| Value::Text(s.to_owned());
    Ok(match source {
        SequenceSource::Collection(c) => {
            let (total, exact) = match c.total {
                None => (Value::Null, Value::Null),
                Some(Count::Exact(n)) => (
                    Value::Integer(int(n, "collection total")?),
                    Value::Integer(1),
                ),
                Some(Count::Estimate(n)) => (
                    Value::Integer(int(n, "collection total")?),
                    Value::Integer(0),
                ),
            };
            vec![
                Value::Integer(0),
                text(c.reference.connection.as_str()),
                text(&c.reference.id),
                Value::Integer(match c.addressing {
                    Addressing::Cursor => 0,
                    Addressing::Offset => 1,
                }),
                total,
                exact,
                c.revision.as_deref().map_or(Value::Null, text),
                Value::Integer(flag(c.endless)),
                Value::Null,
            ]
        }
        SequenceSource::RistlePlaylist(playlist) => {
            let mut values = vec![Value::Integer(1)];
            values.extend(std::iter::repeat_n(Value::Null, 7));
            values.push(text(playlist.as_str()));
            values
        }
        SequenceSource::Fixed => {
            let mut values = vec![Value::Integer(2)];
            values.extend(std::iter::repeat_n(Value::Null, 8));
            values
        }
    })
}

fn read_route(row: &rusqlite::Row<'_>) -> Result<SequenceSource, Error> {
    Ok(match row.get::<_, i64>(1)? {
        0 => {
            let (Some(connection), Some(id), Some(addressing), Some(endless)) = (
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<i64>>(8)?,
            ) else {
                return Err(Error::corrupt(ROUTES, "a collection route is incomplete"));
            };
            let total = match (row.get::<_, Option<i64>>(5)?, row.get::<_, Option<i64>>(6)?) {
                (None, None) => None,
                (Some(n), Some(exact)) => {
                    let n = uint(n, ROUTES, "total")?;
                    Some(if read_flag(exact, ROUTES, "total_exact")? {
                        Count::Exact(n)
                    } else {
                        Count::Estimate(n)
                    })
                }
                _ => return Err(Error::corrupt(ROUTES, "total and total_exact disagree")),
            };
            SequenceSource::Collection(CollectionDescriptor {
                reference: CollectionRef {
                    connection: ConnectionId::new(connection),
                    id,
                },
                addressing: match addressing {
                    0 => Addressing::Cursor,
                    1 => Addressing::Offset,
                    other => {
                        return Err(Error::corrupt(
                            ROUTES,
                            format!("unknown addressing {other}"),
                        ));
                    }
                },
                total,
                revision: row.get(7)?,
                endless: read_flag(endless, ROUTES, "endless")?,
            })
        }
        1 => {
            let playlist: Option<String> = row.get(9)?;
            SequenceSource::RistlePlaylist(PlaylistId::new(
                playlist
                    .ok_or_else(|| Error::corrupt(ROUTES, "a playlist route has no playlist"))?,
            ))
        }
        2 => SequenceSource::Fixed,
        other => {
            return Err(Error::corrupt(
                ROUTES,
                format!("unknown route kind {other}"),
            ));
        }
    })
}
