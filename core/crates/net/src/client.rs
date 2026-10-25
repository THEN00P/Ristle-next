//! The client: request policy, redirects, rate limits and the cache.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use http::Uri;
use http::header::{HeaderName, HeaderValue};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::Client as HyperClient;
use hyper_util::rt::{TokioExecutor, TokioTimer};
use ristle_domain::{Clock, ConnectionId, PluginId};
use url::Url;

use crate::Error;
use crate::cache::{
    self, CacheKey, CacheMethod, CachePolicy, HttpCacheStore, StoreError, StoredResponse,
};
use crate::connect::{GuardedConnector, Refused};
use crate::headers::{self, Header, MAX_RESPONSE_HEADER_BYTES, MAX_RESPONSE_HEADERS};
use crate::rate_limit::{self, DEFAULT_HOLD_MS, RateLimits};
use crate::reach::{self, Granted, Reach};

/// The longest a request may take, and the default: 30 s.
pub const MAX_TIMEOUT: Duration = Duration::from_secs(30);

/// The largest body a buffered request reads by default: 16 MiB.
pub const MAX_BODY_BYTES: u64 = 16 * 1024 * 1024;

/// Most redirects followed for one request.
pub const MAX_REDIRECTS: usize = 10;

/// How long a streamed body may go without a chunk.
pub const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// How many hyper clients for Server Grant origins are kept. Each has its
/// own connection pool.
const GRANTED_CLIENTS: usize = 32;

/// How often the cache's 30-day purge runs at most, driven by writes.
const PURGE_INTERVAL_MS: u64 = 60 * 60 * 1000;

/// A request method.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Method {
    /// GET.
    Get,
    /// HEAD.
    Head,
    /// POST.
    Post,
    /// PUT.
    Put,
    /// PATCH.
    Patch,
    /// DELETE.
    Delete,
}

impl Method {
    fn as_http(self) -> http::Method {
        match self {
            Method::Get => http::Method::GET,
            Method::Head => http::Method::HEAD,
            Method::Post => http::Method::POST,
            Method::Put => http::Method::PUT,
            Method::Patch => http::Method::PATCH,
            Method::Delete => http::Method::DELETE,
        }
    }

    fn cache_method(self) -> Option<CacheMethod> {
        match self {
            Method::Get => Some(CacheMethod::Get),
            Method::Head => Some(CacheMethod::Head),
            _ => None,
        }
    }
}

/// Headers added to every hop whose URL they are meant for, such as a
/// Credential Record's `attach` rules. They are checked like any other
/// header list.
pub trait Attach: Send + Sync {
    /// The headers for a request to `url`.
    fn headers(&self, url: &Url) -> Vec<Header>;
}

/// One request.
#[derive(Clone)]
pub struct Request {
    /// The method.
    pub method: Method,
    /// The URL. Its fragment is never sent.
    pub url: Url,
    /// The caller's headers. Sent to the request's origin, and kept on
    /// redirects within it; dropped at the first redirect to another
    /// origin.
    pub headers: Vec<Header>,
    /// The body.
    pub body: Bytes,
    /// Where the request and its redirects may go.
    pub reach: Arc<Reach>,
    /// Headers added per hop, for the hop's URL.
    pub attach: Option<Arc<dyn Attach>>,
    /// How the cache is used.
    pub cache: CachePolicy,
    /// Revalidate a cached response the request would otherwise reuse: the
    /// user asked to refresh.
    pub revalidate: bool,
    /// For the whole exchange; at most [`MAX_TIMEOUT`] for buffered
    /// requests. For a stream, until the response headers arrived.
    pub timeout: Duration,
    /// The largest body accepted.
    pub max_body: u64,
}

impl Request {
    /// A request with no headers, no body, no cache and the default bounds.
    pub fn new(method: Method, url: Url, reach: Arc<Reach>) -> Self {
        Self {
            method,
            url,
            headers: Vec::new(),
            body: Bytes::new(),
            reach,
            attach: None,
            cache: CachePolicy::Bypass,
            revalidate: false,
            timeout: MAX_TIMEOUT,
            max_body: MAX_BODY_BYTES,
        }
    }
}

impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the path, query or headers: they may carry tokens.
        f.debug_struct("Request")
            .field("method", &self.method)
            .field("origin", &reach::origin_text(&self.url))
            .field("cache", &self.cache)
            .finish_non_exhaustive()
    }
}

/// A buffered response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    /// The status.
    pub status: u16,
    /// Every header, merged with the stored ones for a revalidated response.
    pub headers: Vec<Header>,
    /// The body.
    pub body: Bytes,
    /// The URL it came from, after redirects.
    pub url: Url,
    /// Any part of it came from the cache.
    pub cached: bool,
    /// Only the headers that arrived from the Provider for this request.
    pub received: Vec<Header>,
    /// A response from the Provider arrived for this request (a full one or
    /// a 304). The Plugin host numbers these.
    pub reached_provider: bool,
    /// When it was last fetched from the Provider, in Unix milliseconds. A
    /// result built from several responses is as old as its oldest.
    pub fetched_at_ms: u64,
}

/// A response whose body is read as it arrives. Never cached.
pub struct StreamResponse {
    /// The status.
    pub status: u16,
    /// The headers.
    pub headers: Vec<Header>,
    /// The URL it came from, after redirects.
    pub url: Url,
    body: Incoming,
    remaining: u64,
}

impl std::fmt::Debug for StreamResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamResponse")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

impl StreamResponse {
    /// The next chunk, or `None` at the end. Fails when no chunk arrives
    /// within [`STREAM_IDLE_TIMEOUT`] or the body passes the request's
    /// `max_body`.
    pub async fn chunk(&mut self) -> Result<Option<Bytes>, Error> {
        loop {
            let frame = tokio::time::timeout(STREAM_IDLE_TIMEOUT, self.body.frame())
                .await
                .map_err(|_| Error::Timeout)?;
            let Some(frame) = frame else {
                return Ok(None);
            };
            let frame = frame.map_err(|error| Error::Network(chain(&error)))?;
            if let Ok(data) = frame.into_data() {
                if data.is_empty() {
                    continue;
                }
                let length = data.len() as u64;
                if length > self.remaining {
                    return Err(Error::TooLarge("body exceeds its bound".to_owned()));
                }
                self.remaining -= length;
                return Ok(Some(data));
            }
        }
    }

    /// The whole resource's length: from `Content-Range` for a partial
    /// response, from `Content-Length` otherwise.
    pub fn total_length(&self) -> Option<u64> {
        if self.status == 206 {
            let range = headers::get(&self.headers, "content-range")?;
            let total = range.rsplit_once('/')?.1.trim();
            return total.parse().ok();
        }
        headers::get(&self.headers, "content-length")?
            .trim()
            .parse()
            .ok()
    }
}

/// Settings of a [`Client`].
#[derive(Clone, Debug, Default)]
pub struct ClientConfig {
    /// Certificate authorities trusted besides the built-in Mozilla roots,
    /// DER-encoded.
    pub extra_roots: Vec<Vec<u8>>,
}

type Hyper = HyperClient<HttpsConnector<GuardedConnector>, Full<Bytes>>;

/// The core's one HTTP client. Cloning shares the connection pools, the
/// rate limits and the cache.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").finish_non_exhaustive()
    }
}

struct Inner {
    tls: rustls::ClientConfig,
    public: Hyper,
    granted: Mutex<Vec<(Granted, Hyper)>>,
    limits: RateLimits,
    cache: Option<Arc<dyn HttpCacheStore>>,
    clock: Arc<dyn Clock>,
    last_purge_ms: AtomicU64,
}

/// A response whose body is still to be read.
struct Exchange {
    status: u16,
    headers: Vec<Header>,
    url: Url,
    body: Incoming,
    /// The headers the final hop was sent, the caller's (where kept) and
    /// the attached ones, which a `Vary` selector is built from.
    sent: Vec<Header>,
}

/// A cached response's validators and the URL that supplied them: they are
/// sent to that resource only.
struct Validators {
    url: Url,
    headers: Vec<Header>,
}

impl Client {
    /// A client. `cache` is where cached responses live; without one,
    /// every request bypasses the cache.
    pub fn new(
        config: ClientConfig,
        clock: Arc<dyn Clock>,
        cache: Option<Arc<dyn HttpCacheStore>>,
    ) -> Result<Self, Error> {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        for der in config.extra_roots {
            roots
                .add(rustls::pki_types::CertificateDer::from(der))
                .map_err(|error| Error::NotAllowed(format!("extra root: {error}")))?;
        }
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|error| Error::Network(format!("TLS setup: {error}")))?
        .with_root_certificates(roots)
        .with_no_client_auth();
        let public = hyper_client(&tls, Arc::from(Vec::new()));
        Ok(Self {
            inner: Arc::new(Inner {
                tls,
                public,
                granted: Mutex::new(Vec::new()),
                limits: RateLimits::new(clock.clone()),
                cache,
                clock,
                last_purge_ms: AtomicU64::new(0),
            }),
        })
    }

    /// Holds requests to `host` for `wait`, as if it had answered 429 with
    /// that `Retry-After` (`http.rate-limit`).
    pub fn report_rate_limit(&self, host: &str, wait: Duration) {
        self.inner
            .limits
            .hold(host, u64::try_from(wait.as_millis()).unwrap_or(u64::MAX));
    }

    /// How long requests to `host` are still held back.
    pub fn rate_limit_remaining(&self, host: &str) -> Option<Duration> {
        self.inner.limits.remaining(host).map(Duration::from_millis)
    }

    /// Sends a request and reads the whole response, using the cache as
    /// its policy says.
    pub async fn send(&self, request: Request) -> Result<Response, Error> {
        let caller_headers = headers::checked(&request.headers)?;
        request.reach.check(&request.url)?;
        let timeout = request.timeout.min(MAX_TIMEOUT);
        let key = self.cache_key(&request);
        let stored = match &key {
            Some(key) => self.cache_get(key, &request).await,
            None => None,
        };
        let now = self.inner.clock.now_ms();
        if let Some(stored) = &stored
            && stored.is_fresh(now)
            && !request.revalidate
            && !cache::request_wants_revalidation(&request.headers)
        {
            return served(stored, &request);
        }
        let conditional = match &stored {
            Some(stored)
                if !headers::has(&request.headers, "if-none-match")
                    && !headers::has(&request.headers, "if-modified-since") =>
            {
                Url::parse(&stored.url)
                    .ok()
                    .map(|url| Validators {
                        url,
                        headers: stored.validators(),
                    })
                    .filter(|validators| !validators.headers.is_empty())
            }
            _ => None,
        };
        let max_body = request.max_body;
        let result = tokio::time::timeout(timeout, async {
            let exchange = self
                .open(&request, &caller_headers, conditional.as_ref())
                .await?;
            let body = read_body(
                exchange.body,
                &exchange.headers,
                bodiless(request.method, exchange.status),
                max_body,
            )
            .await?;
            Ok::<_, Error>((
                exchange.status,
                exchange.headers,
                exchange.url,
                body,
                exchange.sent,
            ))
        })
        .await
        .unwrap_or(Err(Error::Timeout));
        let now = self.inner.clock.now_ms();
        let (status, response_headers, url, body, sent) = match result {
            Ok(parts) => parts,
            Err(error) => {
                if provider_failed(&error)
                    && let Some(stored) = &stored
                    && stored.usable_on_error(now)
                {
                    return served(stored, &request);
                }
                return Err(error);
            }
        };

        // Only the resource that supplied the validators can confirm them.
        if status == 304
            && conditional
                .as_ref()
                .is_some_and(|validators| validators.url == url)
            && let (Some(stored), Some(key)) = (&stored, &key)
        {
            return self
                .revalidated(key, stored, &request, response_headers, &sent, now)
                .await;
        }
        if matches!(status, 500 | 502 | 503 | 504)
            && let Some(stored) = &stored
            && stored.usable_on_error(now)
        {
            return served(stored, &request);
        }
        if let Some(key) = &key {
            self.store_response(
                key,
                &request,
                status,
                &response_headers,
                &body,
                &url,
                &sent,
                now,
            )
            .await;
        }
        Ok(Response {
            status,
            received: response_headers.clone(),
            headers: response_headers,
            body,
            url,
            cached: false,
            reached_provider: true,
            fetched_at_ms: now,
        })
    }

    /// Sends a request and returns once the response headers arrived, for
    /// bodies read as they arrive (progressive and HLS media). Never uses
    /// the cache.
    pub async fn stream(&self, request: Request) -> Result<StreamResponse, Error> {
        let caller_headers = headers::checked(&request.headers)?;
        request.reach.check(&request.url)?;
        let exchange =
            tokio::time::timeout(request.timeout, self.open(&request, &caller_headers, None))
                .await
                .unwrap_or(Err(Error::Timeout))?;
        if !bodiless(request.method, exchange.status)
            && let Some(length) = headers::get(&exchange.headers, "content-length")
                .and_then(|length| length.trim().parse::<u64>().ok())
            && length > request.max_body
        {
            return Err(Error::TooLarge(format!(
                "body of {length} bytes exceeds {}",
                request.max_body
            )));
        }
        Ok(StreamResponse {
            status: exchange.status,
            headers: exchange.headers,
            url: exchange.url,
            body: exchange.body,
            remaining: request.max_body,
        })
    }

    /// Drops every cached response of a Provider Connection, in every
    /// generation: its generation changed or ended.
    pub async fn remove_cached_connection(
        &self,
        plugin: &PluginId,
        connection: &ConnectionId,
    ) -> Result<u64, StoreError> {
        let (plugin, connection) = (plugin.clone(), connection.clone());
        self.with_store(move |store| store.remove_connection(&plugin, &connection))
            .await
            .unwrap_or(Ok(0))
    }

    /// Drops every cached response of a Plugin: a version was installed, or
    /// the Plugin was removed.
    pub async fn remove_cached_plugin(&self, plugin: &PluginId) -> Result<u64, StoreError> {
        let plugin = plugin.clone();
        self.with_store(move |store| store.remove_plugin(&plugin))
            .await
            .unwrap_or(Ok(0))
    }

    /// Deletes every cached response last fetched more than 30 days ago. It
    /// also runs by itself, at most hourly, as responses are stored.
    pub async fn purge_expired(&self) -> Result<u64, StoreError> {
        let now = self.inner.clock.now_ms();
        self.inner.last_purge_ms.store(now, Ordering::Relaxed);
        let before = expiry_cutoff(now);
        self.with_store(move |store| store.evict_older_than(before))
            .await
            .unwrap_or(Ok(0))
    }

    /// Follows redirects and returns the final response with its body
    /// unread. Rate limits are checked for every hop and recorded for the
    /// host that answered.
    async fn open(
        &self,
        request: &Request,
        caller_headers: &[(HeaderName, HeaderValue)],
        validators: Option<&Validators>,
    ) -> Result<Exchange, Error> {
        let conditional = match validators {
            Some(validators) => headers::checked(&validators.headers)?,
            None => Vec::new(),
        };
        let mut url = without_fragment(&request.url);
        let mut method = request.method.as_http();
        let mut body = request.body.clone();
        let mut keep_caller_headers = true;
        let mut keep_content_headers = true;
        let client = self.client_for(request.reach.granted());
        for hop in 0..=MAX_REDIRECTS {
            request.reach.check(&url)?;
            let host = url.host_str().unwrap_or_default().to_owned();
            if let Some(remaining) = self.inner.limits.remaining(&host) {
                return Err(Error::RateLimited {
                    retry_after_ms: Some(remaining),
                });
            }
            let attached_text = match &request.attach {
                Some(attach) => attach.headers(&url),
                None => Vec::new(),
            };
            let attached = headers::checked(&attached_text)?;
            let mut sent: Vec<Header> = if keep_caller_headers {
                request
                    .headers
                    .iter()
                    .filter(|(name, _)| {
                        keep_content_headers
                            || !HeaderName::from_bytes(name.as_bytes())
                                .is_ok_and(|name| is_content_header(&name))
                    })
                    .cloned()
                    .collect()
            } else {
                Vec::new()
            };
            sent.extend(attached_text);
            let uri: Uri = url
                .as_str()
                .parse()
                .map_err(|_| Error::NotAllowed("URL is not a valid URI".to_owned()))?;
            let mut builder = http::Request::builder().method(method.clone()).uri(uri);
            if let Some(map) = builder.headers_mut() {
                if keep_caller_headers {
                    for (name, value) in caller_headers {
                        if !keep_content_headers && is_content_header(name) {
                            continue;
                        }
                        map.append(name.clone(), value.clone());
                    }
                }
                if validators.is_some_and(|validators| validators.url == url) {
                    for (name, value) in &conditional {
                        map.append(name.clone(), value.clone());
                    }
                }
                for (name, value) in attached {
                    map.append(name, value);
                }
            }
            let outgoing = builder
                .body(Full::new(body.clone()))
                .map_err(|error| Error::NotAllowed(format!("request: {error}")))?;
            let unsafe_hop = !matches!(method, http::Method::GET | http::Method::HEAD);
            let response = client.request(outgoing).await.map_err(map_hyper_error)?;
            let status = response.status().as_u16();
            // A successful mutation is known from its status: the cache
            // forgets this URL at once, whatever later hops or the body do.
            if unsafe_hop && (200..400).contains(&status) {
                self.invalidate(request, vec![url.clone()]).await;
            }
            let response_headers = headers::from_response(response.headers())?;

            let now = self.inner.clock.now_ms();
            if let Some(wait) = rate_limit::limit_of(status, &response_headers, now) {
                self.inner
                    .limits
                    .hold(&host, wait.unwrap_or(DEFAULT_HOLD_MS));
                return Err(Error::RateLimited {
                    retry_after_ms: wait,
                });
            }

            let location = matches!(status, 301 | 302 | 303 | 307 | 308)
                .then(|| headers::get(&response_headers, "location"))
                .flatten()
                .and_then(|location| url.join(location).ok())
                .map(|next| without_fragment(&next));
            let Some(next) = location else {
                return Ok(Exchange {
                    status,
                    headers: response_headers,
                    url,
                    body: response.into_body(),
                    sent,
                });
            };
            let cross_origin = !reach::same_origin(&url, &next);
            // As browsers do: 303 becomes GET (HEAD stays HEAD), and so does
            // a POST answered with 301 or 302. The body and the headers
            // describing it go. Any other redirect replays the request as
            // it was.
            let to_get = (status == 303 && method != http::Method::HEAD)
                || (matches!(status, 301 | 302) && method == http::Method::POST);
            let replays = !to_get
                && (!body.is_empty() || !matches!(method, http::Method::GET | http::Method::HEAD));
            if hop == MAX_REDIRECTS {
                return Err(Error::Network(format!(
                    "more than {MAX_REDIRECTS} redirects"
                )));
            }
            // Not followed: the caller gets the redirect and decides. A
            // body, or a method that changes state, is never replayed to
            // another origin.
            if request.reach.check(&next).is_err() || (cross_origin && replays) {
                return Ok(Exchange {
                    status,
                    headers: response_headers,
                    url,
                    body: response.into_body(),
                    sent,
                });
            }
            if to_get {
                method = http::Method::GET;
                body = Bytes::new();
                keep_content_headers = false;
            }
            if cross_origin {
                keep_caller_headers = false;
            }
            url = next;
        }
        Err(Error::Network(format!(
            "more than {MAX_REDIRECTS} redirects"
        )))
    }

    fn client_for(&self, granted: &[crate::GrantedOrigin]) -> Hyper {
        if granted.is_empty() {
            return self.inner.public.clone();
        }
        let mut clients = self
            .inner
            .granted
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some((_, client)) = clients.iter().find(|(key, _)| key.as_ref() == granted) {
            return client.clone();
        }
        let key: Granted = Arc::from(granted.to_vec());
        let client = hyper_client(&self.inner.tls, key.clone());
        if clients.len() >= GRANTED_CLIENTS {
            clients.remove(0);
        }
        clients.push((key, client.clone()));
        client
    }

    fn cache_key(&self, request: &Request) -> Option<CacheKey> {
        self.inner.cache.as_ref()?;
        let scope = request.cache.scope()?.clone();
        let method = request.method.cache_method()?;
        if headers::has(&request.headers, "range") {
            return None;
        }
        let key = match &request.cache {
            CachePolicy::Override(_, rule) => rule.key.clone(),
            _ => None,
        }
        .unwrap_or_else(|| without_fragment(&request.url).to_string());
        Some(CacheKey { scope, method, key })
    }

    async fn cache_get(&self, key: &CacheKey, request: &Request) -> Option<StoredResponse> {
        let lookup = key.clone();
        let stored = match self.with_store(move |store| store.get(&lookup)).await? {
            Ok(stored) => stored?,
            Err(error) => {
                tracing::warn!(%error, "HTTP cache read failed; treating it as a miss");
                return None;
            }
        };
        let now = self.inner.clock.now_ms();
        if !stored.within_max_age(now) {
            let expired = key.clone();
            self.with_store(move |store| store.remove(&expired)).await;
            return None;
        }
        // Selected by what this request would send to the URL the response
        // came from. Override entries are stored without `Vary`, so they
        // always match.
        let url = Url::parse(&stored.url).unwrap_or_else(|_| request.url.clone());
        stored
            .matches_vary(&selecting_headers(request, &url))
            .then_some(stored)
    }

    #[allow(clippy::too_many_arguments)]
    async fn store_response(
        &self,
        key: &CacheKey,
        request: &Request,
        status: u16,
        response_headers: &[Header],
        body: &Bytes,
        url: &Url,
        sent: &[Header],
        now: u64,
    ) {
        if cache::request_forbids_store(&request.headers) {
            return;
        }
        // Under HTTP rules a redirect's own cacheability decides whether
        // the URL may be answered from the cache, and the client keeps only
        // the final response: so a response reached through redirects is
        // not stored for the URL that redirected. An override is the
        // Plugin's own choice.
        if matches!(request.cache, CachePolicy::Http(_)) && *url != without_fragment(&request.url) {
            let stale = key.clone();
            self.with_store(move |store| store.remove(&stale)).await;
            return;
        }
        let (freshness, vary) = match &request.cache {
            CachePolicy::Bypass => return,
            CachePolicy::Http(_) => (
                cache::http_freshness(status, response_headers, now),
                cache::vary_of(response_headers, sent),
            ),
            CachePolicy::Override(_, rule) => {
                (cache::override_freshness(status, rule, now), Vec::new())
            }
        };
        let Some(freshness) = freshness else {
            // A response that may not be stored replaces nothing stale.
            let stale = key.clone();
            self.with_store(move |store| store.remove(&stale)).await;
            return;
        };
        let stored = StoredResponse {
            status,
            headers: response_headers.to_vec(),
            body: body.to_vec(),
            url: url.to_string(),
            fetched_at_ms: now,
            fresh_until_ms: freshness.fresh_until_ms,
            stale_until_ms: freshness.stale_until_ms,
            vary,
        };
        self.put(key, stored).await;
    }

    async fn revalidated(
        &self,
        key: &CacheKey,
        stored: &StoredResponse,
        request: &Request,
        received: Vec<Header>,
        sent: &[Header],
        now: u64,
    ) -> Result<Response, Error> {
        let merged = cache::merge_headers(&stored.headers, &received);
        // Each part fits the bounds; together they must too.
        let bytes: usize = merged
            .iter()
            .map(|(name, value)| name.len() + value.len())
            .sum();
        if merged.len() > MAX_RESPONSE_HEADERS || bytes > MAX_RESPONSE_HEADER_BYTES {
            return Err(Error::TooLarge(format!(
                "revalidated headers exceed {MAX_RESPONSE_HEADERS} headers or {MAX_RESPONSE_HEADER_BYTES} bytes"
            )));
        }
        let (freshness, vary) = match &request.cache {
            CachePolicy::Override(_, rule) => (
                cache::override_freshness(stored.status, rule, now),
                Vec::new(),
            ),
            _ => (
                cache::http_freshness(stored.status, &merged, now),
                // The 304 may have changed `Vary`; select by the new one.
                cache::vary_of(&merged, sent),
            ),
        };
        let updated = StoredResponse {
            headers: merged,
            vary,
            fetched_at_ms: now,
            fresh_until_ms: freshness.map_or(now, |f| f.fresh_until_ms),
            stale_until_ms: freshness.map_or(now, |f| f.stale_until_ms),
            ..stored.clone()
        };
        if freshness.is_some() {
            self.put(key, updated.clone()).await;
        } else {
            let gone = key.clone();
            self.with_store(move |store| store.remove(&gone)).await;
        }
        check_body(updated.body.len(), request.max_body)?;
        Ok(Response {
            status: updated.status,
            headers: updated.headers,
            body: Bytes::from(updated.body),
            url: Url::parse(&updated.url).unwrap_or_else(|_| request.url.clone()),
            cached: true,
            received,
            reached_provider: true,
            fetched_at_ms: now,
        })
    }

    /// A successful unsafe hop invalidates what the cache holds for its URL
    /// (RFC 9111, section 4.4); redirect targets each in turn.
    async fn invalidate(&self, request: &Request, targets: Vec<Url>) {
        let Some(scope) = request.cache.scope().cloned() else {
            return;
        };
        if !matches!(request.cache, CachePolicy::Http(_)) {
            return;
        }
        let urls: Vec<String> = targets.iter().map(ToString::to_string).collect();
        self.with_store(move |store| {
            for url in &urls {
                for method in [CacheMethod::Get, CacheMethod::Head] {
                    store.remove(&CacheKey {
                        scope: scope.clone(),
                        method,
                        key: url.clone(),
                    })?;
                }
            }
            Ok::<_, StoreError>(())
        })
        .await;
    }

    async fn put(&self, key: &CacheKey, stored: StoredResponse) {
        let key = key.clone();
        let plugin = key.scope.plugin.clone();
        let now = self.inner.clock.now_ms();
        let last = self.inner.last_purge_ms.load(Ordering::Relaxed);
        let purge = now.saturating_sub(last) >= PURGE_INTERVAL_MS;
        if purge {
            self.inner.last_purge_ms.store(now, Ordering::Relaxed);
        }
        let result = self
            .with_store(move |store| {
                store.put(&key, &stored)?;
                store.trim(
                    &plugin,
                    cache::PLUGIN_BUDGET_BYTES,
                    cache::TOTAL_BUDGET_BYTES,
                )?;
                if purge {
                    store.evict_older_than(expiry_cutoff(now))?;
                }
                Ok::<_, StoreError>(())
            })
            .await;
        if let Some(Err(error)) = result {
            tracing::warn!(%error, "HTTP cache write failed");
        }
    }

    /// Runs `f` on a blocking thread with the store, if there is one.
    async fn with_store<T: Send + 'static>(
        &self,
        f: impl FnOnce(&dyn HttpCacheStore) -> T + Send + 'static,
    ) -> Option<T> {
        let store = self.inner.cache.clone()?;
        match tokio::task::spawn_blocking(move || f(store.as_ref())).await {
            Ok(value) => Some(value),
            Err(error) => {
                tracing::warn!(%error, "HTTP cache task failed");
                None
            }
        }
    }
}

fn hyper_client(tls: &rustls::ClientConfig, granted: Granted) -> Hyper {
    let connector = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(tls.clone())
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .wrap_connector(GuardedConnector::new(granted));
    HyperClient::builder(TokioExecutor::new())
        .pool_timer(TokioTimer::new())
        .timer(TokioTimer::new())
        .pool_idle_timeout(Duration::from_secs(90))
        .http1_max_headers(MAX_RESPONSE_HEADERS)
        // The head (status line and headers) is parsed within this buffer,
        // so an oversized head fails before it is kept; the exact header
        // bound is checked once parsed.
        .http1_max_buf_size(MAX_RESPONSE_HEADER_BYTES + 8 * 1024)
        .http2_max_header_list_size(
            u32::try_from(MAX_RESPONSE_HEADER_BYTES + 32 * MAX_RESPONSE_HEADERS)
                .unwrap_or(u32::MAX),
        )
        .build(connector)
}

/// The headers a request would send to `url`, the caller's and the
/// attached ones, which a cached response's `Vary` selector is compared
/// with. An attached cookie that changed selects another representation.
fn selecting_headers(request: &Request, url: &Url) -> Vec<Header> {
    let mut out = request.headers.clone();
    if let Some(attach) = &request.attach {
        out.extend(attach.headers(url));
    }
    out
}

/// Responses fetched before this are [`cache::MAX_AGE_MS`] old or older,
/// so never served again.
fn expiry_cutoff(now_ms: u64) -> u64 {
    now_ms.saturating_sub(cache::MAX_AGE_MS).saturating_add(1)
}

/// A cached response as the caller sees it, within the request's body
/// bound like a fetched one.
fn served(stored: &StoredResponse, request: &Request) -> Result<Response, Error> {
    check_body(stored.body.len(), request.max_body)?;
    Ok(from_stored(stored, &request.url))
}

fn check_body(length: usize, max: u64) -> Result<(), Error> {
    if length as u64 > max {
        return Err(Error::TooLarge(format!("body exceeds {max} bytes")));
    }
    Ok(())
}

/// Responses that carry no body whatever their `Content-Length` says.
fn bodiless(method: Method, status: u16) -> bool {
    method == Method::Head || status == 204 || status == 304 || (100..200).contains(&status)
}

/// A cached response as the caller sees it.
fn from_stored(stored: &StoredResponse, request_url: &Url) -> Response {
    Response {
        status: stored.status,
        headers: stored.headers.clone(),
        body: Bytes::from(stored.body.clone()),
        url: Url::parse(&stored.url).unwrap_or_else(|_| request_url.clone()),
        cached: true,
        received: Vec::new(),
        reached_provider: false,
        fetched_at_ms: stored.fetched_at_ms,
    }
}

/// Failures for which a stale response may stand in.
fn provider_failed(error: &Error) -> bool {
    matches!(
        error,
        Error::RateLimited { .. } | Error::Timeout | Error::Network(_)
    )
}

fn is_content_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "content-type" | "content-encoding" | "content-language" | "content-location"
    )
}

fn without_fragment(url: &Url) -> Url {
    let mut url = url.clone();
    url.set_fragment(None);
    url
}

async fn read_body(
    mut body: Incoming,
    response_headers: &[Header],
    bodiless: bool,
    max: u64,
) -> Result<Bytes, Error> {
    if !bodiless
        && let Some(length) = headers::get(response_headers, "content-length")
            .and_then(|length| length.trim().parse::<u64>().ok())
        && length > max
    {
        return Err(Error::TooLarge(format!(
            "body of {length} bytes exceeds {max}"
        )));
    }
    let mut out = BytesMut::new();
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|error| Error::Network(chain(&error)))?;
        if let Ok(data) = frame.into_data() {
            if (out.len() + data.len()) as u64 > max {
                return Err(Error::TooLarge(format!("body exceeds {max} bytes")));
            }
            out.extend_from_slice(&data);
        }
    }
    Ok(out.freeze())
}

fn map_hyper_error(error: hyper_util::client::legacy::Error) -> Error {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&error);
    while let Some(current) = source {
        if let Some(refused) = current.downcast_ref::<Refused>() {
            return Error::AddressRefused {
                origin: refused.origin.clone(),
                granted: refused.granted,
                addresses: refused.addresses.clone(),
            };
        }
        if let Some(hyper_error) = current.downcast_ref::<hyper::Error>()
            && hyper_error.is_parse()
            // `is_parse_too_large` needs hyper's server feature; this is
            // the message of the same error kind.
            && hyper_error.to_string().contains("too large")
        {
            return Error::TooLarge(format!(
                "response headers exceed {MAX_RESPONSE_HEADERS} headers or {MAX_RESPONSE_HEADER_BYTES} bytes"
            ));
        }
        source = current.source();
    }
    Error::Network(chain(&error))
}

/// An error and its sources, as one line.
fn chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(current) = source {
        text.push_str(": ");
        text.push_str(&current.to_string());
        source = current.source();
    }
    text
}
