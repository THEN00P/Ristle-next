//! What the Plugin host needs from storage, as ports `ristle-app`
//! implements over `ristle-storage` (`docs/plan/phase-2.md`, section 3).
//! Plugin state lives here only, never only in instance memory (ADR 0003).

use ristle_domain::{ConnectionId, PluginId};

/// A store failed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("plugin store: {0}")]
pub struct StoreError(pub String);

/// Whether a value was stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stored {
    /// It was.
    Yes,
    /// Storing it would take the Plugin over its quota; nothing changed.
    OverQuota,
}

/// One key-value namespace: a Plugin's `storage`, or one Provider
/// Connection's `connection` data.
///
/// The quota is the Plugin's `storage-mib`, and it counts the Plugin's
/// `storage` and the data of all its connections together, each entry as
/// the bytes of its key and value. `set` checks it and writes in one step,
/// so concurrent calls cannot overshoot it.
pub trait KvScope: Send + Sync {
    /// The value, unless absent or expired at `now_ms`.
    fn get(&self, key: &str, now_ms: u64) -> Result<Option<Vec<u8>>, StoreError>;
    /// Stores or replaces a value, expiring at `expires_at_ms` if given,
    /// unless that takes the Plugin over `quota_bytes`. Expired entries do
    /// not count.
    fn set(
        &self,
        key: &str,
        value: &[u8],
        expires_at_ms: Option<u64>,
        quota_bytes: u64,
        now_ms: u64,
    ) -> Result<Stored, StoreError>;
    /// Removes the value, if present.
    fn delete(&self, key: &str) -> Result<(), StoreError>;
    /// Removes every value: a connection's generation changed or ended.
    fn clear(&self) -> Result<(), StoreError>;
}

/// The Plugin host's persistence.
pub trait PluginStore: Send + Sync {
    /// The Plugin's `storage` namespace.
    fn plugin_kv(&self, plugin: &PluginId) -> Box<dyn KvScope>;
    /// One Provider Connection's data, counted against its Plugin's quota.
    /// Keyed by Plugin and connection: connection ids are unique within a
    /// Plugin only, and every Plugin's anonymous connection has the empty
    /// id.
    fn connection_kv(&self, plugin: &PluginId, connection: &ConnectionId) -> Box<dyn KvScope>;
    /// The next `http.response.sequence` of a connection of a Plugin. It
    /// grows with every call and never repeats or goes back, across
    /// restarts too.
    fn next_response_sequence(
        &self,
        plugin: &PluginId,
        connection: &ConnectionId,
    ) -> Result<u64, StoreError>;
}

/// The platform credential store failed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("secret vault: {0}")]
pub struct VaultError(pub String);

/// Secrets in the platform's credential store (Android Keystore, the iOS
/// Keychain, the desktop's secret service). Implemented by the facade's
/// `PlatformHost`. Credential Records live here, never in the database.
pub trait SecretVault: Send + Sync {
    /// The secret stored under `key`.
    fn read(&self, key: &str) -> Result<Option<Vec<u8>>, VaultError>;
    /// Stores or replaces the secret under `key`.
    fn write(&self, key: &str, value: &[u8]) -> Result<(), VaultError>;
    /// Removes the secret under `key`, if any.
    fn delete(&self, key: &str) -> Result<(), VaultError>;
}

/// One entry of a granted folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    /// Its name within its folder.
    pub name: String,
    /// It is a folder.
    pub folder: bool,
    /// Its size in bytes; 0 for folders.
    pub size: u64,
    /// When it was last modified, in Unix milliseconds.
    pub modified_ms: Option<u64>,
}

/// Why folder access failed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FolderError {
    /// No such file or folder, or it lies outside the granted folder (a
    /// link leading out of it included).
    #[error("not found")]
    NotFound,
    /// The system no longer lets Ristle read the granted folder; the user
    /// must choose it again.
    #[error("access to the folder was lost")]
    AccessLost,
    /// Reading failed.
    #[error("reading failed: {0}")]
    Io(String),
}

/// Reading the folders users granted (Folder Grants), implemented per
/// platform over its own access: an Android Storage Access Framework tree,
/// an iOS security-scoped bookmark, a desktop path. The host has already
/// checked that `folder` is granted to the connection and that `path` is a
/// relative path without `.` or `..` components; the implementation must
/// never leave the folder, links included.
pub trait FolderAccess: Send + Sync {
    /// Up to `limit` entries of the folder at `path` (`""` is the granted
    /// folder itself), sorted by name, after the name `after`.
    fn list(
        &self,
        folder: &str,
        path: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<FileEntry>, FolderError>;
    /// Up to `length` bytes of the file at `path` from `offset`; fewer at
    /// its end.
    fn read(
        &self,
        folder: &str,
        path: &str,
        offset: u64,
        length: u32,
    ) -> Result<Vec<u8>, FolderError>;
}
