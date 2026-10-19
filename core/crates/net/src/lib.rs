//! The core's one HTTP stack.
//!
//! Shared by the Plugin host's `http` import and by `ristle-media`'s
//! progressive and HLS fetching, so both use one connection pool and one view
//! of each host's rate limit:
//!
//! - the client (reqwest on rustls; no OpenSSL),
//! - request policy: the forbidden-header list of `plugin-api/wit/host.wit`,
//!   timeouts, and response header and body caps,
//! - the HTTP cache: `Cache-Control`, Plugin overrides and keys,
//!   stale-if-error only, never background revalidation, and the 30-day
//!   retention cap. The store behind it is a port (`HttpCacheStore`) that
//!   `ristle-app` implements over `ristle-storage`,
//! - per-host rate limits: 429 and `Retry-After`, and limits a Plugin reports
//!   through `http.rate-limit`.
//!
//! Milestone M4 (`docs/plan/phase-2.md`).
