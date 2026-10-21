//! What the core needs from the platform. Platform concerns only
//! (AGENTS.md): the core never calls into UI.

/// A platform call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlatformError {
    /// The store cannot be used now, such as before the first unlock.
    #[error("unavailable: {reason}")]
    Unavailable {
        /// The platform's message.
        reason: String,
    },
    /// The call failed.
    #[error("failed: {reason}")]
    Failed {
        /// The platform's message.
        reason: String,
    },
}

/// Services only the platform has, implemented by each app.
///
/// Called from core threads, never from the UI thread. Implementations may
/// block briefly.
pub trait Platform: Send + Sync + 'static {
    /// Reads a secret from the platform credential store (Android Keystore,
    /// Apple Keychain). `None` when there is none under `key`.
    fn read_secret(&self, key: &str) -> Result<Option<Vec<u8>>, PlatformError>;
    /// Stores a secret, replacing any under `key`.
    fn write_secret(&self, key: &str, value: &[u8]) -> Result<(), PlatformError>;
    /// Deletes a secret. Deleting a missing key succeeds.
    fn delete_secret(&self, key: &str) -> Result<(), PlatformError>;
}
