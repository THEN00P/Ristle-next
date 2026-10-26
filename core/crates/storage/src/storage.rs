//! Opening the databases and running work on the storage threads.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ristle_domain::Clock;
use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use tokio::sync::oneshot;

use crate::migrate::{self, MIGRATIONS, Migrated, SCHEMA_VERSION};
use crate::{Error, Tx};

/// Where the two databases live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoragePaths {
    /// For `ristle.sqlite`: state the user cannot recreate. Must survive app
    /// updates and must not be a directory the system clears.
    pub data_dir: PathBuf,
    /// For `cache.sqlite`. The system may clear it while Ristle is closed.
    pub cache_dir: PathBuf,
}

impl StoragePaths {
    /// Both directories.
    pub fn new(data_dir: impl Into<PathBuf>, cache_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            cache_dir: cache_dir.into(),
        }
    }

    /// The path of `ristle.sqlite`.
    pub fn database(&self) -> PathBuf {
        self.data_dir.join("ristle.sqlite")
    }

    /// The path of `cache.sqlite`.
    pub fn cache_database(&self) -> PathBuf {
        self.cache_dir.join("cache.sqlite")
    }
}

/// How [`Storage::open`] found the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenStatus {
    /// Migrated to the current schema and usable.
    Ready,
    /// Not usable: written by a newer Ristle, unreadable, or a migration
    /// failed and was rolled back. The file is as it was before the open,
    /// and every read and write fails with [`Error::Recovery`]. The app
    /// shows `reason` and offers [`Storage::set_aside`].
    Recovery {
        /// What went wrong, for the user and the logs.
        reason: String,
    },
}

type Job = Box<dyn FnOnce(&mut Connection) + Send>;

/// One thread that owns one connection and runs jobs on it in order.
struct Worker {
    jobs: Option<mpsc::Sender<Job>>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    fn spawn(name: &str, mut conn: Connection) -> Result<Self, Error> {
        let (jobs, queue) = mpsc::channel::<Job>();
        let thread = thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                for job in queue {
                    job(&mut conn);
                }
            })
            .map_err(|source| Error::Io {
                action: format!("starting the {name} thread"),
                source,
            })?;
        Ok(Self {
            jobs: Some(jobs),
            thread: Some(thread),
        })
    }

    fn send(&self, job: Job) -> Result<(), Error> {
        self.jobs
            .as_ref()
            .ok_or(Error::Closed)?
            .send(job)
            .map_err(|_| Error::Closed)
    }
}

impl Drop for Worker {
    /// Lets queued jobs finish, then closes the connection.
    fn drop(&mut self) {
        self.jobs = None;
        if let Some(thread) = self.thread.take()
            && thread.thread().id() != thread::current().id()
        {
            let _ = thread.join();
        }
    }
}

struct Workers {
    writer: Worker,
    reader: Worker,
}

/// Ristle's persistent state: `ristle.sqlite` with `cache.sqlite` attached
/// as schema `cache`.
///
/// One thread owns the write connection and runs every [`Storage::write`]
/// in order, each in one immediate transaction. A second thread owns a
/// read-only connection for [`Storage::read`], so reads do not wait behind
/// writes. Both databases use WAL, so a read sees every write that finished
/// before it started.
///
/// Dropping the storage waits for queued work to finish.
pub struct Storage {
    status: OpenStatus,
    schema_version: u32,
    clock: Arc<dyn Clock>,
    workers: Option<Workers>,
}

impl std::fmt::Debug for Storage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Storage")
            .field("status", &self.status)
            .field("schema_version", &self.schema_version)
            .finish_non_exhaustive()
    }
}

impl Storage {
    /// Opens or creates both databases and migrates `ristle.sqlite`.
    ///
    /// A database that cannot be used opens in recovery mode
    /// ([`OpenStatus::Recovery`]) rather than failing, so the app can say
    /// what happened. `Err` means the directories or files could not be
    /// reached at all.
    pub fn open(paths: &StoragePaths, clock: Arc<dyn Clock>) -> Result<Self, Error> {
        for dir in [&paths.data_dir, &paths.cache_dir] {
            std::fs::create_dir_all(dir).map_err(|source| Error::Io {
                action: format!("creating {}", dir.display()),
                source,
            })?;
        }
        let database = paths.database();
        let mut writer = connect(&database, true)?;
        let recovery = |reason: String, version: u32, clock: Arc<dyn Clock>| {
            tracing::error!(%reason, version, "ristle.sqlite opens in recovery mode");
            Self {
                status: OpenStatus::Recovery { reason },
                schema_version: version,
                clock,
                workers: None,
            }
        };
        match migrate::migrate(&mut writer, &database, MIGRATIONS) {
            Migrated::Ready { .. } => {}
            Migrated::Newer { version } => {
                return Ok(recovery(
                    format!(
                        "the database was written by a newer Ristle (schema v{version}); \
                         this version reads up to v{SCHEMA_VERSION}"
                    ),
                    version,
                    clock,
                ));
            }
            Migrated::Failed { version, reason } => return Ok(recovery(reason, version, clock)),
        }

        writer.pragma_update_and_check(None, "journal_mode", "WAL", |_| Ok(()))?;
        writer.pragma_update(None, "synchronous", "NORMAL")?;
        migrate::attach_cache(&writer, &paths.cache_database(), true)?;
        writer.pragma_update(Some("cache"), "synchronous", "NORMAL")?;

        let reader = connect(&database, false)?;
        migrate::attach_cache(&reader, &paths.cache_database(), false)?;
        reader.pragma_update(None, "query_only", true)?;

        let workers = Workers {
            writer: Worker::spawn("ristle-storage-write", writer)?,
            reader: Worker::spawn("ristle-storage-read", reader)?,
        };
        Ok(Self {
            status: OpenStatus::Ready,
            schema_version: SCHEMA_VERSION,
            clock,
            workers: Some(workers),
        })
    }

    /// How the database was found.
    pub fn status(&self) -> &OpenStatus {
        &self.status
    }

    /// The schema version on disk: [`SCHEMA_VERSION`] when ready, otherwise
    /// what the file says.
    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Runs `f` on the storage thread inside one transaction, committed when
    /// `f` returns `Ok` and rolled back otherwise.
    ///
    /// Dropping the returned future does not cancel the write: once sent,
    /// it runs and commits.
    pub async fn write<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Tx<'_>) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, Error> {
        let (reply, result) = oneshot::channel();
        self.send(true, f, move |r| {
            let _ = reply.send(r);
        })?;
        result.await.map_err(|_| Error::Closed)?
    }

    /// Runs `f` on the read thread inside one read transaction, so it sees
    /// one consistent state. Writes fail with [`Error::ReadOnly`].
    pub async fn read<T: Send + 'static>(
        &self,
        f: impl FnOnce(&Tx<'_>) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, Error> {
        let (reply, result) = oneshot::channel();
        self.send(
            false,
            move |tx: &mut Tx<'_>| f(tx),
            move |r| {
                let _ = reply.send(r);
            },
        )?;
        result.await.map_err(|_| Error::Closed)?
    }

    /// [`Storage::write`] for synchronous callers, such as the
    /// implementations of other crates' synchronous storage ports. Blocks
    /// the calling thread; never call it from an async task.
    pub fn write_blocking<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Tx<'_>) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, Error> {
        let (reply, result) = mpsc::sync_channel(1);
        self.send(true, f, move |r| {
            let _ = reply.send(r);
        })?;
        result.recv().map_err(|_| Error::Closed)?
    }

    /// [`Storage::read`] for synchronous callers. Blocks the calling
    /// thread; never call it from an async task.
    pub fn read_blocking<T: Send + 'static>(
        &self,
        f: impl FnOnce(&Tx<'_>) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, Error> {
        let (reply, result) = mpsc::sync_channel(1);
        self.send(
            false,
            move |tx: &mut Tx<'_>| f(tx),
            move |r| {
                let _ = reply.send(r);
            },
        )?;
        result.recv().map_err(|_| Error::Closed)?
    }

    fn send<T: Send + 'static>(
        &self,
        writable: bool,
        f: impl FnOnce(&mut Tx<'_>) -> Result<T, Error> + Send + 'static,
        reply: impl FnOnce(Result<T, Error>) + Send + 'static,
    ) -> Result<(), Error> {
        let workers = match (&self.status, &self.workers) {
            (OpenStatus::Recovery { reason }, _) => {
                return Err(Error::Recovery {
                    reason: reason.clone(),
                });
            }
            (OpenStatus::Ready, None) => return Err(Error::Closed),
            (OpenStatus::Ready, Some(workers)) => workers,
        };
        let clock = Arc::clone(&self.clock);
        let job: Job = Box::new(move |conn| reply(run(conn, &*clock, writable, f)));
        if writable {
            workers.writer.send(job)
        } else {
            workers.reader.send(job)
        }
    }

    /// Moves a database that opened in recovery mode aside, to
    /// `ristle.sqlite.recovery-<time>` next to it, so the next
    /// [`Storage::open`] starts empty. Returns where it went. The database
    /// itself is kept, so it can still be inspected or sent in.
    ///
    /// `cache.sqlite` is deleted first. Its rows are keyed by connection
    /// generations that the new database no longer knows, so a connection id
    /// used again could otherwise read the old account's cached results. A
    /// failure or crash in between leaves the database where it was, still
    /// in recovery.
    pub fn set_aside(self, paths: &StoragePaths) -> Result<PathBuf, Error> {
        if self.status == OpenStatus::Ready {
            return Err(Error::invalid(
                "set aside",
                "only a database in recovery mode is set aside",
            ));
        }
        let cache = paths.cache_database();
        for suffix in ["", "-wal", "-shm", "-journal"] {
            migrate::remove_if_present(&migrate::sibling(&cache, suffix))?;
        }
        let database = paths.database();
        let target = migrate::sibling(&database, &format!(".recovery-{}", self.clock.now_ms()));
        for suffix in ["", "-wal", "-shm"] {
            let from = migrate::sibling(&database, suffix);
            if from.exists() {
                let to = migrate::sibling(&target, suffix);
                std::fs::rename(&from, &to).map_err(|source| Error::Io {
                    action: format!("moving {} to {}", from.display(), to.display()),
                    source,
                })?;
            }
        }
        Ok(target)
    }
}

/// Opens a connection to `ristle.sqlite`. Only the writer may create it.
fn connect(path: &Path, writer: bool) -> Result<Connection, Error> {
    let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    if writer {
        flags |= OpenFlags::SQLITE_OPEN_CREATE;
    }
    let conn = Connection::open_with_flags(path, flags)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.set_prepared_statement_cache_capacity(128);
    Ok(conn)
}

/// Runs one job in its transaction on a storage thread.
fn run<T>(
    conn: &mut Connection,
    clock: &dyn Clock,
    writable: bool,
    f: impl FnOnce(&mut Tx<'_>) -> Result<T, Error>,
) -> Result<T, Error> {
    let behavior = if writable {
        TransactionBehavior::Immediate
    } else {
        TransactionBehavior::Deferred
    };
    let transaction = conn.transaction_with_behavior(behavior)?;
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let mut tx = Tx::new(&transaction, clock, writable);
        f(&mut tx)
    }));
    match outcome {
        Ok(Ok(value)) => {
            if writable {
                transaction.commit()?;
            }
            // A read transaction ends when it is dropped.
            Ok(value)
        }
        // Dropping the transaction rolls it back.
        Ok(Err(error)) => Err(error),
        Err(_) => {
            tracing::error!("a storage closure panicked; its transaction was rolled back");
            Err(Error::Panicked)
        }
    }
}
