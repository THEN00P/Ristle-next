//! Schema versions and the migration runner.
//!
//! `ristle.sqlite` carries its schema version in `PRAGMA user_version`.
//! Opening it:
//!
//! - **Newer than this build:** recovery mode. Nothing is written, not even
//!   the journal mode, so the newer Ristle that wrote it can still read it.
//! - **Current:** ready.
//! - **Older:** a copy is written next to it (`ristle.sqlite.v<N>.backup`,
//!   replacing an older copy for the same version), then every missing
//!   migration runs in one transaction with foreign keys off, as SQLite's
//!   table-rebuild procedure requires. `PRAGMA foreign_key_check` must come
//!   back empty before the transaction commits. Any failure rolls back, so
//!   the database stays exactly as it was, and the storage opens in recovery
//!   mode. The copy protects against a migration that succeeds but loses
//!   data.
//!
//! Version 0 is an empty database, which migration 1 creates from scratch.
//! Ristle legacy's databases are never opened (`phase-2.md`, open question
//! 3). Adding version N means appending a [`Migration`] to [`MIGRATIONS`]
//! and a test that applies it to the schema-v1 fixture
//! (`tests/fixtures/schema-v1.sql`).
//!
//! `cache.sqlite` is never migrated. Everything in it can be fetched again,
//! so a cache database of another version, or one that does not open, is
//! deleted and created again.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, Transaction, TransactionBehavior};

use crate::Error;

/// The schema version this build writes.
pub const SCHEMA_VERSION: u32 = 1;

/// The version of `cache.sqlite` this build writes.
pub(crate) const CACHE_VERSION: u32 = 1;

pub(crate) const SCHEMA_V1: &str = include_str!("schema/ristle-v1.sql");
const CACHE_V1: &str = include_str!("schema/cache-v1.sql");

/// One step from `version - 1` to `version`.
pub(crate) struct Migration {
    pub(crate) version: u32,
    pub(crate) apply: fn(&Transaction<'_>) -> rusqlite::Result<()>,
}

/// Every migration, in order, starting at 1 without gaps.
pub(crate) const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    apply: |tx| tx.execute_batch(SCHEMA_V1),
}];

/// What [`migrate`] found and did.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Migrated {
    /// The database is at the newest version, from `from`.
    Ready { from: u32 },
    /// Written by a newer build. Untouched.
    Newer { version: u32 },
    /// A migration failed and was rolled back, or the database could not be
    /// read. `version` is what is on disk.
    Failed { version: u32, reason: String },
}

/// Brings the database at `path`, open on `conn`, to the last version in
/// `migrations`.
pub(crate) fn migrate(conn: &mut Connection, path: &Path, migrations: &[Migration]) -> Migrated {
    let version = match user_version(conn, "main") {
        Ok(version) => version,
        Err(error) => {
            return Migrated::Failed {
                version: 0,
                reason: format!("the database cannot be read: {error}"),
            };
        }
    };
    let target = migrations.last().map_or(0, |m| m.version);
    if version > target {
        return Migrated::Newer { version };
    }
    if version == target {
        return Migrated::Ready { from: version };
    }

    if version > 0
        && let Err(error) = back_up(conn, path, version)
    {
        return Migrated::Failed {
            version,
            reason: format!("the backup before migrating failed: {error}"),
        };
    }

    let result = conn
        .pragma_update(None, "foreign_keys", false)
        .map_err(Error::from)
        .and_then(|()| apply(conn, version, target, migrations));
    // Foreign keys are switched back on whatever happened. If even that
    // fails, the connection is not used: the storage opens in recovery.
    let restored = conn.pragma_update(None, "foreign_keys", true);
    match (result, restored) {
        (Ok(()), Ok(())) => {
            tracing::info!(from = version, to = target, "migrated ristle.sqlite");
            Migrated::Ready { from: version }
        }
        (Err(error), _) => Migrated::Failed {
            version,
            reason: format!("migrating from schema v{version} failed: {error}"),
        },
        (Ok(()), Err(error)) => Migrated::Failed {
            version: target,
            reason: format!("foreign keys could not be switched back on: {error}"),
        },
    }
}

fn apply(conn: &mut Connection, from: u32, to: u32, migrations: &[Migration]) -> Result<(), Error> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for migration in migrations.iter().filter(|m| m.version > from) {
        (migration.apply)(&tx)?;
    }
    let violation: Option<String> = {
        let mut check = tx.prepare("PRAGMA foreign_key_check")?;
        let mut rows = check.query([])?;
        match rows.next()? {
            Some(row) => Some(row.get::<_, String>(0)?),
            None => None,
        }
    };
    if let Some(table) = violation {
        return Err(Error::corrupt(
            "foreign keys",
            format!("{table} violates a foreign key after migrating"),
        ));
    }
    tx.pragma_update(None, "user_version", to)?;
    tx.commit()?;
    Ok(())
}

/// Writes a copy of the database before it is migrated.
fn back_up(conn: &Connection, path: &Path, version: u32) -> Result<(), Error> {
    let backup = sibling(path, &format!(".v{version}.backup"));
    remove_if_present(&backup)?;
    let target = backup.to_str().ok_or_else(|| {
        Error::invalid("backup path", format!("{} is not UTF-8", backup.display()))
    })?;
    conn.execute("VACUUM INTO ?1", [target])?;
    Ok(())
}

pub(crate) fn user_version(conn: &Connection, schema: &str) -> rusqlite::Result<u32> {
    conn.pragma_query_value(Some(schema), "user_version", |row| row.get(0))
}

/// Attaches `cache.sqlite` as schema `cache`, creating it, or deleting and
/// recreating it when it is of another version or unreadable. Only the
/// writer creates; the reader attaches what the writer prepared.
pub(crate) fn attach_cache(conn: &Connection, path: &Path, create: bool) -> Result<(), Error> {
    let name = path
        .to_str()
        .ok_or_else(|| Error::invalid("cache path", format!("{} is not UTF-8", path.display())))?;
    if !create {
        conn.execute("ATTACH DATABASE ?1 AS cache", [name])?;
        return Ok(());
    }
    match prepare_cache(conn, name) {
        Ok(()) => Ok(()),
        Err(error) => {
            tracing::warn!(%error, "recreating cache.sqlite");
            // Detaching fails when the attach itself failed; that is fine.
            let _ = conn.execute("DETACH DATABASE cache", []);
            for suffix in ["", "-wal", "-shm", "-journal"] {
                remove_if_present(&sibling(path, suffix))?;
            }
            prepare_cache(conn, name)
        }
    }
}

fn prepare_cache(conn: &Connection, name: &str) -> Result<(), Error> {
    conn.execute("ATTACH DATABASE ?1 AS cache", [name])?;
    let version = user_version(conn, "cache")?;
    if version == CACHE_VERSION {
        return Ok(());
    }
    let tables: i64 =
        conn.query_row("SELECT count(*) FROM cache.sqlite_master", [], |r| r.get(0))?;
    if version != 0 || tables != 0 {
        return Err(Error::corrupt(
            "cache",
            format!("cache.sqlite is version {version}, this build uses {CACHE_VERSION}"),
        ));
    }
    let schema = CACHE_V1
        .replace("CREATE TABLE ", "CREATE TABLE cache.")
        .replace("CREATE INDEX ", "CREATE INDEX cache.");
    conn.execute_batch(&format!(
        "BEGIN IMMEDIATE; {schema} PRAGMA cache.user_version = {CACHE_VERSION}; COMMIT;"
    ))
    .inspect_err(|_| {
        let _ = conn.execute_batch("ROLLBACK");
    })?;
    conn.pragma_update_and_check(Some("cache"), "journal_mode", "WAL", |_| Ok(()))?;
    Ok(())
}

/// `path` with `suffix` appended to its file name.
pub(crate) fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

pub(crate) fn remove_if_present(path: &Path) -> Result<(), Error> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::Io {
            action: format!("removing {}", path.display()),
            source,
        }),
    }
}
