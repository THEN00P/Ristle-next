//! The HTTP the media pipeline needs, as a port.
//!
//! `ristle-net` owns the one HTTP client (connection pool, request policy,
//! per-host rate limits, the manifest's `media-hosts` check), so `ristle-media`
//! asks for bytes through [`Fetch`] and `ristle-app` implements it over that
//! client. Tests use an in-memory implementation.

use async_trait::async_trait;
use bytes::Bytes;

/// Bytes `start..start + length` of a resource, or from `start` to its end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ByteRange {
    /// The first byte.
    pub start: u64,
    /// How many bytes, or `None` for the rest of the resource.
    pub length: Option<u64>,
}

impl ByteRange {
    /// The HTTP `Range` header value.
    pub fn header_value(&self) -> String {
        match self.length {
            Some(length) => {
                format!(
                    "bytes={}-{}",
                    self.start,
                    self.start + length.saturating_sub(1)
                )
            }
            None => format!("bytes={}-", self.start),
        }
    }
}

/// One GET.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchRequest {
    /// An absolute URL.
    pub url: String,
    /// Headers to send with it. The caller has already applied the Stream
    /// Source's rule that its headers only go to its own origin.
    pub headers: Vec<(String, String)>,
    /// A byte range, sent as `Range`. A server answering 200 with the whole
    /// resource instead is a [`FetchError`].
    pub range: Option<ByteRange>,
}

/// Why a fetch failed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FetchError {
    /// The server answered with an error status.
    #[error("HTTP {status}")]
    Status {
        /// The status code.
        status: u16,
    },
    /// The request was not made: the URL is not one the Stream Source may
    /// reach (ADR 0004 and the WIT `stream-source` rules).
    #[error("request not allowed: {0}")]
    NotAllowed(String),
    /// The connection failed or the body broke off. Worth retrying.
    #[error("network: {0}")]
    Network(String),
}

/// A response body, read chunk by chunk as it arrives.
#[async_trait]
pub trait FetchBody: Send {
    /// The next chunk, or `None` at the end of the body.
    async fn chunk(&mut self) -> Result<Option<Bytes>, FetchError>;

    /// The whole resource's length, when the response says (`Content-Range`
    /// for a range, `Content-Length` otherwise).
    fn total_length(&self) -> Option<u64> {
        None
    }
}

/// Makes GET requests.
#[async_trait]
pub trait Fetch: Send + Sync {
    /// Starts a request and returns once the response headers arrived with a
    /// success status (206 for a range).
    async fn get(&self, request: FetchRequest) -> Result<Box<dyn FetchBody>, FetchError>;
}

/// A URL for errors and logs: without its query, fragment and userinfo,
/// which carry signatures and tokens in signed CDN URLs.
pub(crate) fn redact_url(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(mut parsed) => {
            let had_secret = parsed.query().is_some()
                || parsed.fragment().is_some()
                || !parsed.username().is_empty()
                || parsed.password().is_some();
            parsed.set_query(None);
            parsed.set_fragment(None);
            let _ = parsed.set_username("");
            let _ = parsed.set_password(None);
            if had_secret {
                format!("{parsed}?<redacted>")
            } else {
                parsed.to_string()
            }
        }
        Err(_) => "<unparsable URL>".to_owned(),
    }
}

/// Reads a whole body.
pub(crate) async fn read_all(
    mut body: Box<dyn FetchBody>,
    limit: usize,
) -> Result<Vec<u8>, FetchError> {
    let mut out = Vec::new();
    while let Some(chunk) = body.chunk().await? {
        if out.len() + chunk.len() > limit {
            return Err(FetchError::Network(format!(
                "body larger than {limit} bytes"
            )));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redaction_drops_query_fragment_and_userinfo() {
        assert_eq!(
            redact_url("https://TOKEN@cdn.example/a.ogg?sig=S#f"),
            "https://cdn.example/a.ogg?<redacted>"
        );
        assert_eq!(
            redact_url("https://u:p@cdn.example/a"),
            "https://cdn.example/a?<redacted>"
        );
        assert_eq!(redact_url("https://cdn.example/a"), "https://cdn.example/a");
    }

    #[test]
    fn ranges_render_as_inclusive_http_ranges() {
        assert_eq!(
            ByteRange {
                start: 100,
                length: Some(50)
            }
            .header_value(),
            "bytes=100-149"
        );
        assert_eq!(
            ByteRange {
                start: 7,
                length: None
            }
            .header_value(),
            "bytes=7-"
        );
    }
}
