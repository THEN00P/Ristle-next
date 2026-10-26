//! Conversions between Rust values and SQLite columns, and the row diff that
//! keeps saves from rewriting unchanged rows.

use std::collections::HashMap;

use ristle_domain::{ConnectionId, ProviderId, Track, TrackRef};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Row, params_from_iter};

use crate::Error;

/// An unsigned value as an SQLite INTEGER, refused when it does not fit.
pub(crate) fn int(value: u64, what: &'static str) -> Result<i64, Error> {
    i64::try_from(value).map_err(|_| Error::invalid(what, format!("{value} is too large to store")))
}

/// A `usize` as an SQLite INTEGER.
pub(crate) fn index(value: usize, what: &'static str) -> Result<i64, Error> {
    i64::try_from(value).map_err(|_| Error::invalid(what, format!("{value} is too large to store")))
}

/// A stored INTEGER that must not be negative.
pub(crate) fn uint(value: i64, table: &'static str, column: &str) -> Result<u64, Error> {
    u64::try_from(value)
        .map_err(|_| Error::corrupt(table, format!("{column} is negative ({value})")))
}

/// A stored INTEGER that indexes into memory.
pub(crate) fn uindex(value: i64, table: &'static str, column: &str) -> Result<usize, Error> {
    usize::try_from(value)
        .map_err(|_| Error::corrupt(table, format!("{column} is out of range ({value})")))
}

/// A seed, stored bit for bit: seeds use the whole `u64` range.
pub(crate) fn seed_to_sql(seed: u64) -> i64 {
    i64::from_ne_bytes(seed.to_ne_bytes())
}

/// A seed read back from [`seed_to_sql`].
pub(crate) fn seed_from_sql(value: i64) -> u64 {
    u64::from_ne_bytes(value.to_ne_bytes())
}

/// A boolean as an INTEGER 0 or 1.
pub(crate) fn flag(value: bool) -> i64 {
    i64::from(value)
}

/// An INTEGER 0 or 1 read back as a boolean.
pub(crate) fn read_flag(value: i64, table: &'static str, column: &str) -> Result<bool, Error> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        other => Err(Error::corrupt(
            table,
            format!("{column} is {other}, not 0 or 1"),
        )),
    }
}

/// The identity columns and JSON of a Track, for the tables that store
/// whole Tracks.
pub(crate) struct TrackColumns {
    pub(crate) provider: String,
    pub(crate) id: String,
    pub(crate) connection: Option<String>,
    pub(crate) json: String,
}

impl TrackColumns {
    pub(crate) fn new(track: &Track) -> Result<Self, Error> {
        let reference = &track.reference;
        Ok(Self {
            provider: reference.provider().as_str().to_owned(),
            id: reference.id().to_owned(),
            connection: reference.connection().map(|c| c.as_str().to_owned()),
            json: serde_json::to_string(track)
                .map_err(|error| Error::invalid("Track", error.to_string()))?,
        })
    }

    /// The four values in column order: provider, id, connection, JSON.
    pub(crate) fn values(self) -> [Value; 4] {
        [
            Value::Text(self.provider),
            Value::Text(self.id),
            self.connection.map_or(Value::Null, Value::Text),
            Value::Text(self.json),
        ]
    }
}

/// Decodes a stored Track and checks it against its identity columns.
pub(crate) fn read_track(
    table: &'static str,
    json: &str,
    provider: &str,
    id: &str,
    connection: Option<&str>,
) -> Result<Track, Error> {
    let track: Track = serde_json::from_str(json)
        .map_err(|error| Error::corrupt(table, format!("Track does not decode: {error}")))?;
    let reference = &track.reference;
    if reference.provider().as_str() != provider
        || reference.id() != id
        || reference.connection().map(ConnectionId::as_str) != connection
    {
        return Err(Error::corrupt(
            table,
            format!("Track {provider}/{id} is stored as {reference:?}"),
        ));
    }
    Ok(track)
}

/// A Track identity from nullable identity columns: `None` when both are
/// NULL.
pub(crate) fn read_track_ref(
    table: &'static str,
    provider: Option<String>,
    id: Option<String>,
    connection: Option<String>,
) -> Result<Option<TrackRef>, Error> {
    let (provider, id) = match (provider, id) {
        (None, None) if connection.is_none() => return Ok(None),
        (Some(provider), Some(id)) => (provider, id),
        _ => return Err(Error::corrupt(table, "a Track identity is incomplete")),
    };
    let reference = match connection {
        None => TrackRef::catalog(ProviderId::new(provider), id),
        Some(connection) => {
            TrackRef::server_scoped(ProviderId::new(provider), id, ConnectionId::new(connection))
        }
    };
    reference
        .map(Some)
        .map_err(|error| Error::corrupt(table, error.to_string()))
}

/// The identity columns of an optional Track identity.
pub(crate) fn track_ref_values(reference: Option<&TrackRef>) -> [Value; 3] {
    match reference {
        None => [Value::Null, Value::Null, Value::Null],
        Some(reference) => [
            Value::Text(reference.provider().as_str().to_owned()),
            Value::Text(reference.id().to_owned()),
            reference
                .connection()
                .map_or(Value::Null, |c| Value::Text(c.as_str().to_owned())),
        ],
    }
}

/// How many rows a save inserted, updated and deleted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Changes {
    /// Rows added.
    pub inserted: u64,
    /// Rows rewritten because a value changed.
    pub updated: u64,
    /// Rows removed.
    pub deleted: u64,
}

impl Changes {
    /// Whether nothing was written.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

impl std::ops::AddAssign for Changes {
    fn add_assign(&mut self, other: Self) {
        self.inserted += other.inserted;
        self.updated += other.updated;
        self.deleted += other.deleted;
    }
}

/// A set of rows keyed by one INTEGER column, optionally within a scope
/// (all slots of one sequence, all entries of one playlist).
pub(crate) struct RowSet {
    pub(crate) table: &'static str,
    /// The TEXT column that selects the set, if the table holds several.
    pub(crate) scope: Option<&'static str>,
    /// The INTEGER column that identifies a row within the set.
    pub(crate) key: &'static str,
    /// Every other column, in the order rows give their values.
    pub(crate) columns: &'static [&'static str],
}

impl RowSet {
    /// Makes the stored set equal to `rows`, touching only rows that differ:
    /// unchanged rows are neither updated nor deleted and inserted again.
    pub(crate) fn sync(
        &self,
        conn: &Connection,
        scope: Option<&str>,
        rows: Vec<(i64, Vec<Value>)>,
    ) -> Result<Changes, Error> {
        // The scope comes last in every statement's parameters.
        let scope_clause = self
            .scope
            .map(|column| format!(" AND {column} = ?"))
            .unwrap_or_default();
        let scope_values: Vec<Value> = scope
            .map(|s| Value::Text(s.to_owned()))
            .into_iter()
            .collect();

        let mut existing = HashMap::new();
        {
            let sql = format!(
                "SELECT {}, {} FROM {} WHERE 1{scope_clause}",
                self.key,
                self.columns.join(", "),
                self.table
            );
            let mut select = conn.prepare_cached(&sql)?;
            let mut stored = select.query(params_from_iter(&scope_values))?;
            while let Some(row) = stored.next()? {
                let key: i64 = row.get(0)?;
                let values = (1..=self.columns.len())
                    .map(|i| row.get::<_, Value>(i))
                    .collect::<Result<Vec<_>, _>>()?;
                existing.insert(key, values);
            }
        }

        let mut changes = Changes::default();
        let mut inserts = Vec::new();
        let mut updates = Vec::new();
        for (key, values) in rows {
            debug_assert_eq!(values.len(), self.columns.len());
            match existing.remove(&key) {
                Some(stored) if stored == values => {}
                Some(_) => updates.push((key, values)),
                None => inserts.push((key, values)),
            }
        }

        if !existing.is_empty() {
            let sql = format!(
                "DELETE FROM {} WHERE {} = ?{scope_clause}",
                self.table, self.key
            );
            let mut delete = conn.prepare_cached(&sql)?;
            for key in existing.into_keys() {
                let mut values = vec![Value::Integer(key)];
                values.extend(scope_values.iter().cloned());
                changes.deleted += to_u64(delete.execute(params_from_iter(values))?);
            }
        }
        if !updates.is_empty() {
            let assignments: Vec<String> = self
                .columns
                .iter()
                .map(|column| format!("{column} = ?"))
                .collect();
            let sql = format!(
                "UPDATE {} SET {} WHERE {} = ?{scope_clause}",
                self.table,
                assignments.join(", "),
                self.key,
            );
            let mut update = conn.prepare_cached(&sql)?;
            for (key, mut values) in updates {
                values.push(Value::Integer(key));
                values.extend(scope_values.iter().cloned());
                changes.updated += to_u64(update.execute(params_from_iter(values))?);
            }
        }
        if !inserts.is_empty() {
            let columns: Vec<&str> = self
                .scope
                .into_iter()
                .chain([self.key])
                .chain(self.columns.iter().copied())
                .collect();
            let placeholders = vec!["?"; columns.len()].join(", ");
            let sql = format!(
                "INSERT INTO {} ({}) VALUES ({placeholders})",
                self.table,
                columns.join(", ")
            );
            let mut insert = conn.prepare_cached(&sql)?;
            for (key, values) in inserts {
                let mut all = scope_values.clone();
                all.push(Value::Integer(key));
                all.extend(values);
                changes.inserted += to_u64(insert.execute(params_from_iter(all))?);
            }
        }
        Ok(changes)
    }
}

/// A row count from `execute`.
pub(crate) fn to_u64(count: usize) -> u64 {
    u64::try_from(count).unwrap_or(u64::MAX)
}

/// Runs a query expected to return at most one row.
pub(crate) fn optional<T>(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
    map: impl FnOnce(&Row<'_>) -> rusqlite::Result<T>,
) -> Result<Option<T>, Error> {
    Ok(conn
        .prepare_cached(sql)?
        .query_row(params, map)
        .optional()?)
}
