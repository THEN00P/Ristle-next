//! Explicit cancellation for long-running calls.
//!
//! Kotlin frees a Rust future when its coroutine is cancelled, but generated
//! Swift does not forward `Task` cancellation (ADR 0001). Every long-running
//! call therefore takes a `CancelToken`; Swift cancels it from
//! `withTaskCancellationHandler`.

use std::sync::Arc;

use tokio::sync::watch;

/// Cancels the calls it was passed to. Once cancelled it stays cancelled;
/// use a new token for the next call.
#[derive(Debug, uniffi::Object)]
pub struct CancelToken {
    cancelled: watch::Sender<bool>,
}

#[uniffi::export]
impl CancelToken {
    /// A token that is not cancelled.
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            cancelled: watch::Sender::new(false),
        })
    }

    /// Cancels every call that holds this token. Such a call ends with
    /// `CoreError::Cancelled` and its work in the core stops. Safe from any
    /// thread, any number of times.
    pub fn cancel(&self) {
        self.cancelled.send_replace(true);
    }

    /// Whether `cancel` was called.
    pub fn is_cancelled(&self) -> bool {
        *self.cancelled.borrow()
    }
}

impl CancelToken {
    /// Completes once the token is cancelled.
    pub(crate) async fn cancelled(&self) {
        let mut cancelled = self.cancelled.subscribe();
        // The sender lives in `self`, so `wait_for` cannot fail while this
        // future is alive.
        let _ = cancelled.wait_for(|cancelled| *cancelled).await;
    }
}
