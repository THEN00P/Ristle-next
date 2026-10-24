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
    fn connection_kv(&self, connection: &ConnectionId) -> Box<dyn KvScope>;
    /// The next `http.response.sequence` of a connection. It grows with
    /// every call and never repeats or goes back, across restarts too.
    fn next_response_sequence(&self, connection: &ConnectionId) -> Result<u64, StoreError>;
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
