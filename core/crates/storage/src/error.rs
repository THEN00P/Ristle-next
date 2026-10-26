//! Errors of the storage crate.

/// Every error this crate returns.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The database is in recovery mode (see [`crate::OpenStatus`]), so
    /// nothing is read or written.
    #[error("the database needs recovery: {reason}")]
    Recovery {
        /// Why it was not opened normally.
        reason: String,
    },
    /// SQLite failed.
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    /// The storage threads have stopped, so the call was not run.
    #[error("storage is closed")]
    Closed,
    /// A storage closure panicked. Its transaction was rolled back and the
    /// storage thread kept running.
    #[error("a storage call panicked")]
    Panicked,
    /// A file operation on the database's directories failed.
    #[error("{action}: {source}")]
    Io {
        /// What was being done.
        action: String,
        /// The failure.
        #[source]
        source: std::io::Error,
    },
    /// A stored row does not decode into a valid value.
    #[error("corrupt row in {table}: {detail}")]
    Corrupt {
        /// The table it was read from.
        table: &'static str,
        /// What is wrong with it.
        detail: String,
    },
    /// A value was refused before anything was written.
    #[error("invalid {what}: {detail}")]
    Invalid {
        /// What was refused.
        what: &'static str,
        /// Why.
        detail: String,
    },
    /// A write was attempted inside [`crate::Storage::read`].
    #[error("writes need Storage::write")]
    ReadOnly,
    /// The command receipt table is full. Nothing is evicted, because an
    /// evicted receipt would let its command apply twice.
    #[error("the command receipt limit of {limit} is reached")]
    ReceiptLimit {
        /// The limit.
        limit: u64,
    },
    /// A receipt for this command already exists.
    #[error("a receipt for {scope}/{key} already exists")]
    ReceiptExists {
        /// Its scope.
        scope: String,
        /// Its key.
        key: String,
    },
    /// A Plugin's stored data would exceed its quota.
    #[error("the storage quota of {quota} bytes would be exceeded ({needed} needed)")]
    QuotaExceeded {
        /// The quota.
        quota: u64,
        /// The bytes the Plugin would hold after the write.
        needed: u64,
    },
    /// A write closure gave up, so its transaction was rolled back.
    #[error("write aborted: {reason}")]
    Aborted {
        /// Why.
        reason: String,
    },
}

impl Error {
    pub(crate) fn corrupt(table: &'static str, detail: impl Into<String>) -> Self {
        Self::Corrupt {
            table,
            detail: detail.into(),
        }
    }

    pub(crate) fn invalid(what: &'static str, detail: impl Into<String>) -> Self {
        Self::Invalid {
            what,
            detail: detail.into(),
        }
    }
}
