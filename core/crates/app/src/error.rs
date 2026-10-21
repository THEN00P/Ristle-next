//! The application core's error.

/// What kind of Provider failure it was. It decides playback behavior:
/// `Unplayable`, `Account` and `Unknown` are reported and skipped;
/// `Transient` stops on the Track, paused (legacy trap 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderErrorKind {
    /// This item cannot play or does not exist.
    Unplayable,
    /// The Provider Connection must sign in again, or the account lacks
    /// access.
    Account,
    /// The network or the Provider failed for now; a retry may work.
    Transient,
    /// Anything else.
    Unknown,
}

/// Every way a core call fails.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Error {
    /// The method exists but its milestone has not landed
    /// (`docs/plan/phase-2.md`). `feature` is the method's name.
    #[error("{feature} is not implemented yet")]
    NotReady {
        /// The method.
        feature: String,
    },
    /// The call was cancelled.
    #[error("cancelled")]
    Cancelled,
    /// An argument was invalid, such as an unknown queue entry.
    #[error("invalid argument: {reason}")]
    InvalidArgument {
        /// What was wrong.
        reason: String,
    },
    /// A Provider failed.
    #[error("provider failed ({kind:?}): {reason}")]
    Provider {
        /// What kind of failure.
        kind: ProviderErrorKind,
        /// The Provider's or Plugin's message.
        reason: String,
    },
    /// Storage failed.
    #[error("storage failed: {reason}")]
    Storage {
        /// What failed.
        reason: String,
    },
    /// The platform failed, such as the credential store.
    #[error("platform failed: {reason}")]
    Platform {
        /// What failed.
        reason: String,
    },
    /// A bug in the core.
    #[error("internal error: {reason}")]
    Internal {
        /// What failed.
        reason: String,
    },
}

impl Error {
    pub(crate) fn not_ready(feature: &str) -> Self {
        Self::NotReady {
            feature: feature.to_owned(),
        }
    }
}
