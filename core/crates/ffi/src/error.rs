//! Errors in both directions across the boundary.

use crate::types::ProviderErrorKind;

/// Every way a core call fails. Kotlin sees `CoreException`, Swift
/// `CoreError`.
#[derive(Debug, Clone, PartialEq, thiserror::Error, uniffi::Error)]
pub enum CoreError {
    /// The method exists but its milestone has not landed. `feature` is the
    /// method's name.
    #[error("{feature} is not implemented yet")]
    NotReady {
        /// The method.
        feature: String,
    },
    /// The call was cancelled through its `CancelToken`.
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
    /// A `PlatformHost` call failed.
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

impl From<ristle_app::Error> for CoreError {
    fn from(error: ristle_app::Error) -> Self {
        use ristle_app::Error as E;
        match error {
            E::NotReady { feature } => Self::NotReady { feature },
            E::Cancelled => Self::Cancelled,
            E::InvalidArgument { reason } => Self::InvalidArgument { reason },
            E::Provider { kind, reason } => Self::Provider { kind, reason },
            E::Storage { reason } => Self::Storage { reason },
            E::Platform { reason } => Self::Platform { reason },
            E::Internal { reason } => Self::Internal { reason },
        }
    }
}

/// How a `PlatformHost` call fails. Kotlin throws `PlatformException`, Swift
/// `PlatformError`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
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

/// A platform callback threw something other than `PlatformError`.
impl From<uniffi::UnexpectedUniFFICallbackError> for PlatformError {
    fn from(error: uniffi::UnexpectedUniFFICallbackError) -> Self {
        Self::Failed {
            reason: format!("unexpected error from the platform: {}", error.reason),
        }
    }
}

impl From<PlatformError> for ristle_app::PlatformError {
    fn from(error: PlatformError) -> Self {
        match error {
            PlatformError::Unavailable { reason } => Self::Unavailable { reason },
            PlatformError::Failed { reason } => Self::Failed { reason },
        }
    }
}
