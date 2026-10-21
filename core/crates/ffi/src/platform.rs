//! The interfaces the platform implements, and their adapters to
//! `ristle-app`'s traits.

use std::sync::Arc;

use crate::error::PlatformError;
use crate::types::{CoreSnapshot, LibraryChange, PlaybackNotice};

/// Receives the core's state, on core threads: never the caller's thread,
/// even for a change a synchronous command caused (ADR 0001). Hop to the UI
/// or media-session thread before touching UI state, and return quickly.
#[uniffi::export(with_foreign)]
pub trait CoreObserver: Send + Sync {
    /// A new snapshot, whole. Its revision is higher than every earlier one.
    fn on_snapshot(&self, snapshot: CoreSnapshot);
    /// A Playback Notice to show above the seek bar.
    fn on_notice(&self, notice: PlaybackNotice);
    /// Something outside the Playback Session changed.
    fn on_library_changed(&self, change: LibraryChange);
}

/// Services only the platform has: platform concerns only (AGENTS.md).
/// Called on core threads.
#[uniffi::export(with_foreign)]
pub trait PlatformHost: Send + Sync {
    /// Reads a secret from the platform credential store. `None` when there
    /// is none under `key`.
    fn read_secret(&self, key: String) -> Result<Option<Vec<u8>>, PlatformError>;
    /// Stores a secret, replacing any under `key`.
    fn write_secret(&self, key: String, value: Vec<u8>) -> Result<(), PlatformError>;
    /// Deletes a secret. Deleting a missing key succeeds.
    fn delete_secret(&self, key: String) -> Result<(), PlatformError>;
}

pub(crate) struct ForeignObserver(pub(crate) Arc<dyn CoreObserver>);

impl ristle_app::Observer for ForeignObserver {
    fn on_snapshot(&self, snapshot: CoreSnapshot) {
        self.0.on_snapshot(snapshot);
    }
    fn on_notice(&self, notice: PlaybackNotice) {
        self.0.on_notice(notice);
    }
    fn on_library_changed(&self, change: LibraryChange) {
        self.0.on_library_changed(change);
    }
}

pub(crate) struct ForeignPlatform(pub(crate) Arc<dyn PlatformHost>);

impl ristle_app::Platform for ForeignPlatform {
    fn read_secret(&self, key: &str) -> Result<Option<Vec<u8>>, ristle_app::PlatformError> {
        Ok(self.0.read_secret(key.to_owned())?)
    }
    fn write_secret(&self, key: &str, value: &[u8]) -> Result<(), ristle_app::PlatformError> {
        Ok(self.0.write_secret(key.to_owned(), value.to_vec())?)
    }
    fn delete_secret(&self, key: &str) -> Result<(), ristle_app::PlatformError> {
        Ok(self.0.delete_secret(key.to_owned())?)
    }
}

#[cfg(test)]
mod tests {
    use ristle_app::Platform;

    use super::*;

    struct Locked;

    impl PlatformHost for Locked {
        fn read_secret(&self, _key: String) -> Result<Option<Vec<u8>>, PlatformError> {
            Err(PlatformError::Unavailable {
                reason: "locked".into(),
            })
        }
        fn write_secret(&self, _key: String, _value: Vec<u8>) -> Result<(), PlatformError> {
            Err(uniffi::UnexpectedUniFFICallbackError::new("IllegalStateException").into())
        }
        fn delete_secret(&self, _key: String) -> Result<(), PlatformError> {
            Ok(())
        }
    }

    #[test]
    fn platform_errors_reach_the_core_typed() {
        let platform = ForeignPlatform(Arc::new(Locked));
        assert_eq!(
            platform.read_secret("k"),
            Err(ristle_app::PlatformError::Unavailable {
                reason: "locked".into()
            })
        );
        assert!(matches!(
            platform.write_secret("k", b"v"),
            Err(ristle_app::PlatformError::Failed { reason }) if reason.contains("IllegalStateException")
        ));
        assert_eq!(platform.delete_secret("k"), Ok(()));
    }
}
