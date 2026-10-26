//! Per-host rate limits.
//!
//! A 429, or a 503 with `Retry-After`, puts a hold on the host that answered:
//! until it is over, every request to that host fails at once with
//! [`Error::RateLimited`](crate::Error::RateLimited), whoever makes it, so
//! stream fetches wait for a limit a Plugin's request hit and the other way
//! round. A Plugin can report a limit the Provider signalled some other way
//! (`http.rate-limit`). Nothing is retried.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use ristle_domain::Clock;

use crate::headers::{self, Header};

/// The hold when a 429 gives no `Retry-After`: one minute.
pub const DEFAULT_HOLD_MS: u64 = 60_000;

/// The longest hold, whatever the Provider asks: one day. A wrong value from
/// a broken server then blocks its host for a day at most.
pub const MAX_HOLD_MS: u64 = 24 * 60 * 60 * 1000;

/// The holds, by lowercase host.
pub(crate) struct RateLimits {
    clock: Arc<dyn Clock>,
    holds: Mutex<HashMap<String, u64>>,
}

impl RateLimits {
    pub(crate) fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            holds: Mutex::new(HashMap::new()),
        }
    }

    /// Milliseconds left on the hold of `host`, if any.
    pub(crate) fn remaining(&self, host: &str) -> Option<u64> {
        let now = self.clock.now_ms();
        let mut holds = self.holds.lock().unwrap_or_else(PoisonError::into_inner);
        let key = host.to_ascii_lowercase();
        match holds.get(&key) {
            Some(&until) if until > now => Some(until - now),
            Some(_) => {
                holds.remove(&key);
                None
            }
            None => None,
        }
    }

    /// Holds `host` for `wait_ms`, capped at [`MAX_HOLD_MS`]. A hold never
    /// gets shorter.
    pub(crate) fn hold(&self, host: &str, wait_ms: u64) {
        let until = self.clock.now_ms().saturating_add(wait_ms.min(MAX_HOLD_MS));
        let mut holds = self.holds.lock().unwrap_or_else(PoisonError::into_inner);
        let entry = holds.entry(host.to_ascii_lowercase()).or_insert(0);
        *entry = (*entry).max(until);
    }
}

/// The wait a response asks for, if it is a rate limit: `Some(None)` for a
/// 429 without a usable `Retry-After`.
pub(crate) fn limit_of(status: u16, headers: &[Header], now_ms: u64) -> Option<Option<u64>> {
    let retry_after = headers::get(headers, "retry-after").and_then(|value| {
        let value = value.trim();
        if let Ok(seconds) = value.parse::<u64>() {
            return Some(seconds.saturating_mul(1000));
        }
        let at = httpdate::parse_http_date(value).ok()?;
        let at_ms =
            u64::try_from(at.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis()).ok()?;
        Some(at_ms.saturating_sub(now_ms))
    });
    match status {
        429 => Some(retry_after),
        503 => retry_after.map(Some),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use ristle_domain::ManualClock;

    use super::*;

    #[test]
    fn holds_end_and_never_shrink() {
        let clock = Arc::new(ManualClock::new(1_000));
        let limits = RateLimits::new(clock.clone());
        assert_eq!(limits.remaining("api.example.com"), None);
        limits.hold("API.example.com", 5_000);
        assert_eq!(limits.remaining("api.example.com"), Some(5_000));
        limits.hold("api.example.com", 1_000);
        assert_eq!(limits.remaining("api.example.com"), Some(5_000));
        clock.advance(4_000);
        assert_eq!(limits.remaining("api.example.com"), Some(1_000));
        clock.advance(1_000);
        assert_eq!(limits.remaining("api.example.com"), None);
        assert_eq!(limits.remaining("other.example.com"), None);
    }

    #[test]
    fn holds_are_capped_at_a_day() {
        let limits = RateLimits::new(Arc::new(ManualClock::new(0)));
        limits.hold("a", u64::MAX);
        assert_eq!(limits.remaining("a"), Some(MAX_HOLD_MS));
    }

    #[test]
    fn retry_after_takes_seconds_or_a_date() {
        let now = 1_800_000_000_000;
        let seconds = vec![("Retry-After".to_owned(), "120".to_owned())];
        assert_eq!(limit_of(429, &seconds, now), Some(Some(120_000)));
        assert_eq!(limit_of(503, &seconds, now), Some(Some(120_000)));
        let date = httpdate::fmt_http_date(
            std::time::UNIX_EPOCH + std::time::Duration::from_millis(now + 30_000),
        );
        let dated = vec![("retry-after".to_owned(), date)];
        assert_eq!(limit_of(429, &dated, now), Some(Some(30_000)));
        assert_eq!(limit_of(429, &[], now), Some(None));
        assert_eq!(limit_of(503, &[], now), None);
        assert_eq!(limit_of(200, &seconds, now), None);
        let garbage = vec![("Retry-After".to_owned(), "soon".to_owned())];
        assert_eq!(limit_of(429, &garbage, now), Some(None));
        assert_eq!(limit_of(503, &garbage, now), None);
    }
}
