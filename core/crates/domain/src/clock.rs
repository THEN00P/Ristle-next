//! Wall-clock time, injected so that tests control it.

use std::time::{SystemTime, UNIX_EPOCH};

/// The source of wall-clock time. No crate reads the system time except
/// [`SystemClock`]; everything else takes a `Clock`.
pub trait Clock: Send + Sync + 'static {
    /// Milliseconds since the Unix epoch, UTC.
    fn now_ms(&self) -> u64;
}

/// The system's wall clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        // A clock set before 1970 reads as the epoch rather than failing.
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
            })
    }
}

#[cfg(any(test, feature = "test-support"))]
pub use manual::ManualClock;

#[cfg(any(test, feature = "test-support"))]
mod manual {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::Clock;

    /// A clock that moves only when told to, for tests.
    #[derive(Debug, Default)]
    pub struct ManualClock(AtomicU64);

    impl ManualClock {
        /// A clock reading `now_ms`.
        pub fn new(now_ms: u64) -> Self {
            Self(AtomicU64::new(now_ms))
        }

        /// Sets the time.
        pub fn set(&self, now_ms: u64) {
            self.0.store(now_ms, Ordering::SeqCst);
        }

        /// Moves the time forward.
        pub fn advance(&self, by_ms: u64) {
            self.0.fetch_add(by_ms, Ordering::SeqCst);
        }
    }

    impl Clock for ManualClock {
        fn now_ms(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_clock_moves_only_when_told() {
        let clock = ManualClock::new(1_000);
        assert_eq!(clock.now_ms(), 1_000);
        clock.advance(500);
        assert_eq!(clock.now_ms(), 1_500);
        clock.set(42);
        assert_eq!(clock.now_ms(), 42);
    }

    #[test]
    fn system_clock_reads_after_2020() {
        assert!(SystemClock.now_ms() > 1_577_836_800_000);
    }
}
