//! HTTP cache semantics (`plugin-api/wit/host.wit`, `http`).
//!
//! - Only GET and HEAD are cached, and never a range request.
//! - [`CachePolicy::Http`] follows `Cache-Control` as a private cache does:
//!   `max-age`, then `Expires`; no heuristic freshness. `no-store` keeps the
//!   response out, `no-cache` stores it for revalidation only, and `Vary`
//!   selects by the request's headers (`Vary: *` is never stored).
//! - [`CachePolicy::Override`] treats a 2xx response as fresh for the given
//!   time whatever its headers say, and may replace the URL as the key.
//! - Stale responses are served only when the Provider is unreachable or
//!   rate-limiting (stale-if-error: the response's `stale-if-error`
//!   directive, or the override's `stale-seconds`). Nothing is ever
//!   revalidated in the background.
//! - Nothing is kept or served [`MAX_AGE_MS`] after it was last fetched from
//!   the Provider, and longer freshness is capped there.
//! - Keys always carry the Plugin, the Provider Connection and its
//!   generation.
//! - All entries of one Plugin take at most [`PLUGIN_BUDGET_BYTES`], and of
//!   every Plugin [`TOTAL_BUDGET_BYTES`], least recently used evicted
//!   first.

use std::time::Duration;

use ristle_domain::{ConnectionId, PluginId};
use serde::{Deserialize, Serialize};

use crate::headers::{self, Header};

/// No cached response is served, even stale, or kept, once it was last
/// fetched from the Provider this long ago: 30 days.
pub const MAX_AGE_MS: u64 = 30 * 24 * 60 * 60 * 1000;

/// What all cached responses of one Plugin may take: 64 MiB.
pub const PLUGIN_BUDGET_BYTES: u64 = 64 * 1024 * 1024;

/// What all cached responses together may take: 512 MiB.
pub const TOTAL_BUDGET_BYTES: u64 = 512 * 1024 * 1024;

/// Whose cache a request reads and writes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CacheScope {
    /// The Plugin.
    pub plugin: PluginId,
    /// The Provider Connection, the anonymous one included.
    pub connection: ConnectionId,
    /// The connection's generation. It changes with every accepted
    /// authorization, so nothing cached for one account answers the next.
    pub generation: u64,
}

/// A Plugin's caching instruction for one request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheOverride {
    /// Fresh for this long, whatever the headers say.
    pub ttl: Duration,
    /// Then served for this long when the Provider is unreachable or
    /// rate-limiting.
    pub stale: Duration,
    /// Replaces the URL as the key.
    pub key: Option<String>,
}

/// How a request uses the cache.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum CachePolicy {
    /// Neither read nor write.
    #[default]
    Bypass,
    /// Follow `Cache-Control`.
    Http(CacheScope),
    /// The Plugin's override.
    Override(CacheScope, CacheOverride),
}

impl CachePolicy {
    pub(crate) fn scope(&self) -> Option<&CacheScope> {
        match self {
            CachePolicy::Bypass => None,
            CachePolicy::Http(scope) | CachePolicy::Override(scope, _) => Some(scope),
        }
    }
}

/// The methods the cache keeps responses for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CacheMethod {
    /// GET.
    Get,
    /// HEAD.
    Head,
}

/// Identifies one cached response.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CacheKey {
    /// Whose it is.
    pub scope: CacheScope,
    /// The request method.
    pub method: CacheMethod,
    /// The request URL without its fragment, or the Plugin's override key.
    pub key: String,
}

/// A cached response with what the cache needs to decide whether to use it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredResponse {
    /// The status.
    pub status: u16,
    /// Every header, merged with those of later revalidations.
    pub headers: Vec<Header>,
    /// The body.
    pub body: Vec<u8>,
    /// The URL the response came from, after redirects.
    pub url: String,
    /// When any of it was last fetched from the Provider (a full response
    /// or a revalidation), in Unix milliseconds.
    pub fetched_at_ms: u64,
    /// Fresh until then.
    pub fresh_until_ms: u64,
    /// Served stale when the Provider fails until then.
    pub stale_until_ms: u64,
    /// The request header values that selected it, for `Vary`: lowercase
    /// names and the joined values, `None` when the request lacked it.
    pub vary: Vec<(String, Option<String>)>,
}

impl StoredResponse {
    /// The bytes it occupies, as counted against the cache budgets.
    pub fn size(&self) -> u64 {
        let headers: usize = self
            .headers
            .iter()
            .map(|(name, value)| name.len() + value.len())
            .sum();
        let vary: usize = self
            .vary
            .iter()
            .map(|(name, value)| name.len() + value.as_ref().map_or(0, String::len))
            .sum();
        (self.body.len() + headers + vary + self.url.len()) as u64
    }

    pub(crate) fn is_fresh(&self, now_ms: u64) -> bool {
        now_ms < self.fresh_until_ms && self.within_max_age(now_ms)
    }

    pub(crate) fn usable_on_error(&self, now_ms: u64) -> bool {
        now_ms < self.stale_until_ms.max(self.fresh_until_ms) && self.within_max_age(now_ms)
    }

    pub(crate) fn within_max_age(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.fetched_at_ms) < MAX_AGE_MS
    }

    /// Whether a request with these headers may use it.
    pub(crate) fn matches_vary(&self, request: &[Header]) -> bool {
        self.vary
            .iter()
            .all(|(name, value)| headers::joined(request, name).as_deref() == value.as_deref())
    }

    /// The headers of a conditional request that revalidates it.
    pub(crate) fn validators(&self) -> Vec<Header> {
        let mut out = Vec::new();
        if let Some(etag) = headers::get(&self.headers, "etag") {
            out.push(("if-none-match".to_owned(), etag.to_owned()));
        }
        if let Some(modified) = headers::get(&self.headers, "last-modified") {
            out.push(("if-modified-since".to_owned(), modified.to_owned()));
        }
        out
    }
}

/// A cache store failed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("cache store: {0}")]
pub struct StoreError(pub String);

/// Where cached responses live. `ristle-app` implements it over
/// `ristle-storage`'s `cache.sqlite`; `test_support` (feature `test-support`) has an
/// in-memory one.
///
/// The client calls it from a blocking thread, never from the runtime.
pub trait HttpCacheStore: Send + Sync {
    /// The stored response. Counts as a use for least-recently-used
    /// eviction.
    fn get(&self, key: &CacheKey) -> Result<Option<StoredResponse>, StoreError>;
    /// Stores or replaces the response. Counts as a use.
    fn put(&self, key: &CacheKey, response: &StoredResponse) -> Result<(), StoreError>;
    /// Removes it, if present.
    fn remove(&self, key: &CacheKey) -> Result<(), StoreError>;
    /// Removes every response last fetched before the time. Returns how many.
    fn evict_older_than(&self, last_fetched_before_ms: u64) -> Result<u64, StoreError>;
    /// Removes every response of one Provider Connection, in every
    /// generation. Returns how many.
    fn remove_connection(
        &self,
        plugin: &PluginId,
        connection: &ConnectionId,
    ) -> Result<u64, StoreError>;
    /// Removes every response of a Plugin. Returns how many.
    fn remove_plugin(&self, plugin: &PluginId) -> Result<u64, StoreError>;
    /// Evicts least recently used responses until the Plugin's take at
    /// most `plugin_max_bytes` and all together at most `total_max_bytes`,
    /// sizes counted by [`StoredResponse::size`]. Returns how many it
    /// evicted.
    fn trim(
        &self,
        plugin: &PluginId,
        plugin_max_bytes: u64,
        total_max_bytes: u64,
    ) -> Result<u64, StoreError>;
}

/// Statuses a response may be stored with under [`CachePolicy::Http`]
/// (RFC 9110, "heuristically cacheable", without 206).
const CACHEABLE: [u16; 10] = [200, 203, 204, 300, 301, 308, 404, 405, 410, 414];

/// When a fresh response may be used and how long after it may be served
/// on error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Freshness {
    pub(crate) fresh_until_ms: u64,
    pub(crate) stale_until_ms: u64,
}

impl Freshness {
    fn capped(fetched_at_ms: u64, fresh_until_ms: u64, stale_until_ms: u64) -> Self {
        let limit = fetched_at_ms.saturating_add(MAX_AGE_MS);
        let fresh_until_ms = fresh_until_ms.min(limit);
        Self {
            fresh_until_ms,
            stale_until_ms: stale_until_ms.max(fresh_until_ms).min(limit),
        }
    }
}

/// Parsed `Cache-Control` directives. Names are lowercase.
struct Directives(Vec<(String, Option<String>)>);

impl Directives {
    fn of(headers: &[Header]) -> Self {
        let Some(value) = headers::joined(headers, "cache-control") else {
            return Self(Vec::new());
        };
        Self(
            value
                .split(',')
                .filter_map(|part| {
                    let part = part.trim();
                    if part.is_empty() {
                        return None;
                    }
                    let (name, argument) = match part.split_once('=') {
                        Some((name, argument)) => (
                            name.trim(),
                            Some(argument.trim().trim_matches('"').to_owned()),
                        ),
                        None => (part, None),
                    };
                    Some((name.to_ascii_lowercase(), argument))
                })
                .collect(),
        )
    }

    fn has(&self, name: &str) -> bool {
        self.0.iter().any(|(candidate, _)| candidate == name)
    }

    fn seconds(&self, name: &str) -> Option<u64> {
        self.0
            .iter()
            .find(|(candidate, _)| candidate == name)
            .and_then(|(_, argument)| argument.as_deref())
            .and_then(|argument| argument.parse::<u64>().ok())
    }
}

fn date_ms(value: &str) -> Option<u64> {
    let time = httpdate::parse_http_date(value.trim()).ok()?;
    let since = time.duration_since(std::time::UNIX_EPOCH).ok()?;
    u64::try_from(since.as_millis()).ok()
}

/// Whether the request itself asks not to be answered from the cache.
pub(crate) fn request_wants_revalidation(request: &[Header]) -> bool {
    let directives = Directives::of(request);
    directives.has("no-cache")
        || directives.seconds("max-age") == Some(0)
        || headers::get(request, "pragma").is_some_and(|pragma| {
            pragma
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case("no-cache"))
        })
}

/// Whether the request forbids storing.
pub(crate) fn request_forbids_store(request: &[Header]) -> bool {
    Directives::of(request).has("no-store")
}

/// Freshness of a response under [`CachePolicy::Http`], or `None` when it
/// must not be stored.
pub(crate) fn http_freshness(
    status: u16,
    response: &[Header],
    fetched_at_ms: u64,
) -> Option<Freshness> {
    if !CACHEABLE.contains(&status) {
        return None;
    }
    let directives = Directives::of(response);
    if directives.has("no-store") {
        return None;
    }
    if headers::joined(response, "vary")
        .is_some_and(|vary| vary.split(',').any(|v| v.trim() == "*"))
    {
        return None;
    }
    let date = headers::get(response, "date").and_then(date_ms);
    let lifetime_ms = if directives.has("no-cache") {
        0
    } else if let Some(max_age) = directives.seconds("max-age") {
        max_age.saturating_mul(1000)
    } else if let Some(expires) = headers::get(response, "expires") {
        // An unparseable `Expires` (often "0") means already expired.
        match date_ms(expires) {
            Some(expires) => expires.saturating_sub(date.unwrap_or(fetched_at_ms)),
            None => 0,
        }
    } else {
        0
    };
    let age_header_ms = headers::get(response, "age")
        .and_then(|age| age.trim().parse::<u64>().ok())
        .map_or(0, |age| age.saturating_mul(1000));
    let apparent_age_ms = date.map_or(0, |date| fetched_at_ms.saturating_sub(date));
    let age_ms = age_header_ms.max(apparent_age_ms);
    let fresh_until_ms = fetched_at_ms.saturating_add(lifetime_ms.saturating_sub(age_ms));
    let may_serve_stale = !directives.has("must-revalidate")
        && !directives.has("proxy-revalidate")
        && !directives.has("no-cache");
    let stale_ms = if may_serve_stale {
        directives
            .seconds("stale-if-error")
            .map_or(0, |seconds| seconds.saturating_mul(1000))
    } else {
        0
    };
    let has_validator = headers::has(response, "etag") || headers::has(response, "last-modified");
    let freshness = Freshness::capped(
        fetched_at_ms,
        fresh_until_ms,
        fresh_until_ms.saturating_add(stale_ms),
    );
    let useful = freshness.stale_until_ms > fetched_at_ms || has_validator;
    useful.then_some(freshness)
}

/// Freshness of a response under a Plugin override, or `None` when it must
/// not be stored.
pub(crate) fn override_freshness(
    status: u16,
    rule: &CacheOverride,
    fetched_at_ms: u64,
) -> Option<Freshness> {
    if !(200..300).contains(&status) || status == 206 {
        return None;
    }
    let ttl_ms = u64::try_from(rule.ttl.as_millis()).unwrap_or(u64::MAX);
    let stale_ms = u64::try_from(rule.stale.as_millis()).unwrap_or(u64::MAX);
    let fresh_until_ms = fetched_at_ms.saturating_add(ttl_ms);
    Some(Freshness::capped(
        fetched_at_ms,
        fresh_until_ms,
        fresh_until_ms.saturating_add(stale_ms),
    ))
}

/// The request header values a response's `Vary` names.
pub(crate) fn vary_of(response: &[Header], request: &[Header]) -> Vec<(String, Option<String>)> {
    let Some(vary) = headers::joined(response, "vary") else {
        return Vec::new();
    };
    let mut names: Vec<String> = vary
        .split(',')
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect();
    names.sort();
    names.dedup();
    names
        .into_iter()
        .map(|name| {
            let value = headers::joined(request, &name);
            (name, value)
        })
        .collect()
}

/// The stored headers updated with those of a 304 (RFC 9111, section 3.2):
/// every header the 304 carries replaces the stored ones of that name,
/// except the framing ones, which describe the stored body.
pub(crate) fn merge_headers(stored: &[Header], fresh: &[Header]) -> Vec<Header> {
    const KEEP: [&str; 4] = [
        "content-length",
        "content-encoding",
        "transfer-encoding",
        "content-range",
    ];
    let replaced = |name: &str| {
        !KEEP.iter().any(|keep| name.eq_ignore_ascii_case(keep)) && headers::has(fresh, name)
    };
    let mut out: Vec<Header> = stored
        .iter()
        .filter(|(name, _)| !replaced(name))
        .cloned()
        .collect();
    out.extend(
        fresh
            .iter()
            .filter(|(name, _)| !KEEP.iter().any(|keep| name.eq_ignore_ascii_case(keep)))
            .cloned(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000_000;

    fn h(pairs: &[(&str, &str)]) -> Vec<Header> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    fn http_date(ms: u64) -> String {
        httpdate::fmt_http_date(std::time::UNIX_EPOCH + Duration::from_millis(ms))
    }

    #[test]
    fn max_age_sets_freshness_minus_age() {
        let freshness = http_freshness(
            200,
            &h(&[("Cache-Control", "max-age=60"), ("Age", "10")]),
            NOW,
        )
        .unwrap();
        assert_eq!(freshness.fresh_until_ms, NOW + 50_000);
        assert_eq!(freshness.stale_until_ms, NOW + 50_000);
    }

    #[test]
    fn expires_is_relative_to_date() {
        let response = h(&[
            ("Date", &http_date(NOW - 5_000)),
            ("Expires", &http_date(NOW + 55_000)),
        ]);
        let freshness = http_freshness(200, &response, NOW).unwrap();
        // 60 s lifetime, 5 s apparent age.
        assert_eq!(freshness.fresh_until_ms, NOW + 55_000);
        assert!(http_freshness(200, &h(&[("Expires", "0")]), NOW).is_none());
    }

    #[test]
    fn no_store_vary_star_and_odd_statuses_are_not_stored() {
        assert!(
            http_freshness(200, &h(&[("Cache-Control", "no-store, max-age=60")]), NOW).is_none()
        );
        assert!(
            http_freshness(
                200,
                &h(&[("Cache-Control", "max-age=60"), ("Vary", "*")]),
                NOW
            )
            .is_none()
        );
        for status in [201, 206, 302, 307, 400, 401, 429, 500, 503] {
            assert!(
                http_freshness(status, &h(&[("Cache-Control", "max-age=60")]), NOW).is_none(),
                "{status}"
            );
        }
        assert!(http_freshness(404, &h(&[("Cache-Control", "max-age=60")]), NOW).is_some());
    }

    #[test]
    fn without_freshness_only_validators_make_it_worth_storing() {
        assert!(http_freshness(200, &[], NOW).is_none());
        let with_etag = http_freshness(200, &h(&[("ETag", "\"a\"")]), NOW).unwrap();
        assert_eq!(with_etag.fresh_until_ms, NOW);
        let no_cache = http_freshness(
            200,
            &h(&[
                ("Cache-Control", "no-cache, max-age=600"),
                ("ETag", "\"a\""),
            ]),
            NOW,
        )
        .unwrap();
        assert_eq!(no_cache.fresh_until_ms, NOW);
    }

    #[test]
    fn stale_if_error_extends_only_without_must_revalidate() {
        let freshness = http_freshness(
            200,
            &h(&[("Cache-Control", "max-age=60, stale-if-error=600")]),
            NOW,
        )
        .unwrap();
        assert_eq!(freshness.stale_until_ms, NOW + 660_000);
        let strict = http_freshness(
            200,
            &h(&[(
                "Cache-Control",
                "max-age=60, stale-if-error=600, must-revalidate",
            )]),
            NOW,
        )
        .unwrap();
        assert_eq!(strict.stale_until_ms, NOW + 60_000);
    }

    #[test]
    fn freshness_is_capped_at_thirty_days() {
        let freshness = http_freshness(
            200,
            &h(&[("Cache-Control", "max-age=31536000, stale-if-error=31536000")]),
            NOW,
        )
        .unwrap();
        assert_eq!(freshness.fresh_until_ms, NOW + MAX_AGE_MS);
        assert_eq!(freshness.stale_until_ms, NOW + MAX_AGE_MS);
        let rule = CacheOverride {
            ttl: Duration::from_secs(u64::from(u32::MAX)),
            stale: Duration::from_secs(u64::from(u32::MAX)),
            key: None,
        };
        let overridden = override_freshness(200, &rule, NOW).unwrap();
        assert_eq!(overridden.fresh_until_ms, NOW + MAX_AGE_MS);
        assert_eq!(overridden.stale_until_ms, NOW + MAX_AGE_MS);
    }

    #[test]
    fn overrides_store_success_only() {
        let rule = CacheOverride {
            ttl: Duration::from_secs(60),
            stale: Duration::from_secs(600),
            key: None,
        };
        let freshness = override_freshness(200, &rule, NOW).unwrap();
        assert_eq!(freshness.fresh_until_ms, NOW + 60_000);
        assert_eq!(freshness.stale_until_ms, NOW + 660_000);
        for status in [206, 304, 404, 500] {
            assert!(override_freshness(status, &rule, NOW).is_none(), "{status}");
        }
    }

    #[test]
    fn stored_responses_expire_after_thirty_days_even_stale() {
        let stored = StoredResponse {
            status: 200,
            headers: Vec::new(),
            body: Vec::new(),
            url: String::new(),
            fetched_at_ms: NOW,
            fresh_until_ms: u64::MAX,
            stale_until_ms: u64::MAX,
            vary: Vec::new(),
        };
        assert!(stored.is_fresh(NOW + MAX_AGE_MS - 1));
        assert!(!stored.is_fresh(NOW + MAX_AGE_MS));
        assert!(!stored.usable_on_error(NOW + MAX_AGE_MS));
    }

    #[test]
    fn vary_selects_by_request_headers() {
        let vary = vary_of(
            &h(&[("Vary", "Accept-Language, accept")]),
            &h(&[("Accept-Language", "de")]),
        );
        assert_eq!(
            vary,
            vec![
                ("accept".to_owned(), None),
                ("accept-language".to_owned(), Some("de".to_owned())),
            ]
        );
        let stored = StoredResponse {
            status: 200,
            headers: Vec::new(),
            body: Vec::new(),
            url: String::new(),
            fetched_at_ms: NOW,
            fresh_until_ms: NOW,
            stale_until_ms: NOW,
            vary,
        };
        assert!(stored.matches_vary(&h(&[("accept-language", "de")])));
        assert!(!stored.matches_vary(&h(&[("accept-language", "fr")])));
        assert!(!stored.matches_vary(&h(&[("accept-language", "de"), ("accept", "x")])));
    }

    #[test]
    fn a_304_replaces_headers_but_not_framing() {
        let merged = merge_headers(
            &h(&[
                ("Content-Length", "5"),
                ("ETag", "\"a\""),
                ("Set-Cookie", "a=1"),
                ("Set-Cookie", "b=2"),
                ("X-Kept", "1"),
            ]),
            &h(&[
                ("etag", "\"b\""),
                ("Set-Cookie", "c=3"),
                ("Content-Length", "0"),
            ]),
        );
        assert_eq!(
            merged,
            h(&[
                ("Content-Length", "5"),
                ("X-Kept", "1"),
                ("etag", "\"b\""),
                ("Set-Cookie", "c=3"),
            ])
        );
    }

    #[test]
    fn requests_can_ask_for_revalidation_or_no_store() {
        assert!(request_wants_revalidation(&h(&[(
            "Cache-Control",
            "no-cache"
        )])));
        assert!(request_wants_revalidation(&h(&[(
            "Cache-Control",
            "max-age=0"
        )])));
        assert!(request_wants_revalidation(&h(&[("Pragma", "no-cache")])));
        assert!(!request_wants_revalidation(&h(&[(
            "Cache-Control",
            "max-age=10"
        )])));
        assert!(request_forbids_store(&h(&[("Cache-Control", "no-store")])));
    }
}
