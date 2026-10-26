use std::net::IpAddr;

/// Why a request failed.
///
/// The Plugin host maps these onto the WIT `error`: [`Error::NotAllowed`]
/// is `internal` (the Plugin asked for something it may not have),
/// [`Error::RateLimited`] is `rate-limited`, and the rest are `transient`.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Nothing was sent: the URL, scheme, host or a header is outside what
    /// the request may use.
    #[error("request not allowed: {0}")]
    NotAllowed(String),
    /// The name resolved only to addresses the request may not reach. For a
    /// Server Grant origin, `addresses` is what it resolves to now, which the
    /// host shows the user before anything reaches them.
    #[error("{origin} resolved to addresses that may not be reached")]
    AddressRefused {
        /// `scheme://host:port` of the destination.
        origin: String,
        /// Whether the origin belongs to a Server Grant.
        granted: bool,
        /// The addresses found.
        addresses: Vec<IpAddr>,
    },
    /// The Provider asked to slow down (429, or 503 with `Retry-After`, or a
    /// limit a Plugin reported), now or earlier, and the hold on its host is
    /// not over.
    #[error("rate limited")]
    RateLimited {
        /// Milliseconds until requests to the host are sent again, when
        /// known.
        retry_after_ms: Option<u64>,
    },
    /// The request did not finish in time.
    #[error("timed out")]
    Timeout,
    /// The response's headers or body exceed the host's bounds.
    #[error("response too large: {0}")]
    TooLarge(String),
    /// The connection failed or broke off, or the response was malformed.
    #[error("network: {0}")]
    Network(String),
}
