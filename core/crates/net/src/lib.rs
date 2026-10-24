//! The core's one HTTP stack.
//!
//! Shared by the Plugin host's `http` import and by `ristle-media`'s
//! progressive and HLS fetching, so both use one connection pool and one view
//! of each host's rate limit (`docs/plan/phase-2.md`, M4):
//!
//! - [`Client`]: hyper on rustls (ring, the Mozilla roots; no OpenSSL and no
//!   system proxy). Every request carries a [`Reach`], the destinations the
//!   user consented to; the client checks every URL against it, follows
//!   redirects itself under the WIT rules, and connects only to addresses
//!   the destination may use ([`address`]).
//! - Request policy: the forbidden-header list of `plugin-api/wit/host.wit`
//!   ([`headers`]), a 30 s ceiling, a 16 MiB body bound by default, and at
//!   most 200 response headers in 64 KiB, enforced while parsing.
//! - The HTTP cache ([`cache`]): `Cache-Control`, Plugin overrides and
//!   keys, stale-if-error only, never background revalidation, and the
//!   30-day retention cap. The store behind it is a port
//!   ([`HttpCacheStore`]) that `ristle-app` implements over `ristle-storage`.
//! - Per-host rate limits: 429 and `Retry-After`, and limits a Plugin
//!   reports through `http.rate-limit`.
//!
//! Plugin-specific rules (credential `attach` rules, the response sequence,
//! the per-Plugin concurrency limit, generations) belong to
//! `ristle-plugin-host`, which builds its requests on this crate.

pub mod address;
pub mod cache;
mod client;
mod connect;
mod error;
pub mod headers;
mod rate_limit;
mod reach;
#[cfg(feature = "test-support")]
pub mod test_support;

pub use cache::{
    CacheKey, CacheMethod, CacheOverride, CachePolicy, CacheScope, HttpCacheStore, StoreError,
    StoredResponse,
};
pub use client::{
    Attach, Client, ClientConfig, MAX_BODY_BYTES, MAX_REDIRECTS, MAX_TIMEOUT, Method, Request,
    Response, STREAM_IDLE_TIMEOUT, StreamResponse,
};
pub use error::Error;
pub use headers::Header;
pub use rate_limit::{DEFAULT_HOLD_MS, MAX_HOLD_MS};
pub use reach::{GrantedOrigin, HostPattern, InvalidPattern, Reach, Scheme};
