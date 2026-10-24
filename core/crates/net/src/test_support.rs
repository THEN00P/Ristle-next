//! A local HTTP/1.1 server and an in-memory cache store, for tests of this
//! crate and of every crate that fetches through it. No test touches the
//! internet (`docs/plan/phase-2.md`, section 1).
//!
//! The server is written against the socket rather than a server library so
//! a test controls every byte: oversized heads, chunked bodies that stall,
//! bodies that break off.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use bytes::Bytes;
use ristle_domain::{ConnectionId, PluginId};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{AbortHandle, JoinHandle};
use url::Url;

use crate::cache::{CacheKey, HttpCacheStore, StoreError, StoredResponse};
use crate::headers::Header;
use crate::reach::{GrantedOrigin, Reach};

/// A request the server received.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedRequest {
    /// The method.
    pub method: String,
    /// The path and query.
    pub path: String,
    /// The headers, names lowercase.
    pub headers: Vec<Header>,
    /// The body.
    pub body: Bytes,
}

impl RecordedRequest {
    /// The first value of a header.
    pub fn header(&self, name: &str) -> Option<&str> {
        crate::headers::get(&self.headers, name)
    }
}

/// What the server answers.
#[derive(Clone, Debug)]
pub struct TestResponse {
    /// The status.
    pub status: u16,
    /// The headers. `Content-Length` or `Transfer-Encoding` is added.
    pub headers: Vec<Header>,
    /// The body, sent with `Content-Length`, unless `chunks` is set.
    pub body: Bytes,
    /// Sent chunked instead, with `chunk_delay` before each chunk.
    pub chunks: Option<Vec<Bytes>>,
    /// The wait before each chunk.
    pub chunk_delay: Duration,
    /// The wait before the response head.
    pub delay: Duration,
    /// Declare `body.len() + 1` as `Content-Length`, send the body and
    /// close the connection.
    pub truncate: bool,
}

impl TestResponse {
    /// A response with a status and nothing else.
    pub fn status(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Bytes::new(),
            chunks: None,
            chunk_delay: Duration::ZERO,
            delay: Duration::ZERO,
            truncate: false,
        }
    }

    /// A 200 with a body.
    pub fn ok(body: impl Into<Bytes>) -> Self {
        Self {
            body: body.into(),
            ..Self::status(200)
        }
    }

    /// Adds a header.
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }
}

type Handler = Arc<dyn Fn(&RecordedRequest) -> TestResponse + Send + Sync>;

/// A server on 127.0.0.1 with a random port.
#[derive(Debug)]
pub struct TestServer {
    address: SocketAddr,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    task: JoinHandle<()>,
    connections: Arc<Mutex<Vec<AbortHandle>>>,
}

impl TestServer {
    /// Starts a server answering every request with `handler`.
    pub async fn start(
        handler: impl Fn(&RecordedRequest) -> TestResponse + Send + Sync + 'static,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let handler: Handler = Arc::new(handler);
        let recorded = requests.clone();
        let connections: Arc<Mutex<Vec<AbortHandle>>> = Arc::default();
        let open = connections.clone();
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let handler = handler.clone();
                let recorded = recorded.clone();
                let connection = tokio::spawn(async move {
                    let _ = serve(stream, handler, recorded).await;
                });
                open.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(connection.abort_handle());
            }
        });
        Ok(Self {
            address,
            requests,
            task,
            connections,
        })
    }

    /// `http://127.0.0.1:<port>`.
    pub fn origin(&self) -> String {
        format!("http://{}", self.address)
    }

    /// A URL on this server.
    ///
    /// # Panics
    ///
    /// When `path_and_query` does not make a URL; it is a test's literal.
    #[allow(clippy::panic)]
    pub fn url(&self, path_and_query: &str) -> Url {
        match Url::parse(&format!("{}{path_and_query}", self.origin())) {
            Ok(url) => url,
            Err(error) => panic!("bad test URL {path_and_query:?}: {error}"),
        }
    }

    /// The server's origin as a Server Grant the user confirmed for this
    /// device, which is how a test reaches a loopback server.
    pub fn granted(&self) -> GrantedOrigin {
        GrantedOrigin {
            scheme: crate::Scheme::Http,
            host: self.address.ip().to_string(),
            port: self.address.port(),
            public: false,
            confirmed: vec![IpAddr::V4(Ipv4Addr::LOCALHOST)],
        }
    }

    /// A reach covering this server only.
    pub fn reach(&self) -> Arc<Reach> {
        Arc::new(Reach::none().with_granted(vec![self.granted()]))
    }

    /// Every request received so far.
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Drop for TestServer {
    /// Stops accepting and closes every open connection, so the server is
    /// unreachable afterwards, kept-alive connections included.
    fn drop(&mut self) {
        self.task.abort();
        for connection in self
            .connections
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
        {
            connection.abort();
        }
    }
}

async fn serve(
    mut stream: TcpStream,
    handler: Handler,
    recorded: Arc<Mutex<Vec<RecordedRequest>>>,
) -> std::io::Result<()> {
    let mut buffer = Vec::new();
    loop {
        let Some(request) = read_request(&mut stream, &mut buffer).await? else {
            return Ok(());
        };
        recorded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(request.clone());
        let response = handler(&request);
        tokio::time::sleep(response.delay).await;
        let mut head = format!("HTTP/1.1 {} X\r\n", response.status);
        for (name, value) in &response.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        if response.chunks.is_some() {
            head.push_str("transfer-encoding: chunked\r\n\r\n");
        } else {
            let length = response.body.len() + usize::from(response.truncate);
            head.push_str(&format!("content-length: {length}\r\n\r\n"));
        }
        stream.write_all(head.as_bytes()).await?;
        if request.method == "HEAD" {
            // A HEAD response ends with its head.
        } else if let Some(chunks) = &response.chunks {
            for chunk in chunks {
                tokio::time::sleep(response.chunk_delay).await;
                stream
                    .write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
                    .await?;
                stream.write_all(chunk).await?;
                stream.write_all(b"\r\n").await?;
                stream.flush().await?;
            }
            stream.write_all(b"0\r\n\r\n").await?;
        } else {
            stream.write_all(&response.body).await?;
        }
        stream.flush().await?;
        if response.truncate {
            return Ok(());
        }
    }
}

async fn read_request(
    stream: &mut TcpStream,
    buffer: &mut Vec<u8>,
) -> std::io::Result<Option<RecordedRequest>> {
    let invalid =
        |what: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, what.to_owned());
    loop {
        let mut slots = [httparse::EMPTY_HEADER; 128];
        let mut parsed = httparse::Request::new(&mut slots);
        if let httparse::Status::Complete(head_length) =
            parsed.parse(buffer).map_err(|_| invalid("bad request"))?
        {
            let method = parsed.method.unwrap_or_default().to_owned();
            let path = parsed.path.unwrap_or_default().to_owned();
            let headers: Vec<Header> = parsed
                .headers
                .iter()
                .map(|header| {
                    (
                        header.name.to_ascii_lowercase(),
                        String::from_utf8_lossy(header.value).into_owned(),
                    )
                })
                .collect();
            let length = crate::headers::get(&headers, "content-length")
                .and_then(|length| length.parse::<usize>().ok())
                .unwrap_or(0);
            while buffer.len() < head_length + length {
                if !read_more(stream, buffer).await? {
                    return Err(invalid("body broke off"));
                }
            }
            let body = Bytes::copy_from_slice(&buffer[head_length..head_length + length]);
            buffer.drain(..head_length + length);
            return Ok(Some(RecordedRequest {
                method,
                path,
                headers,
                body,
            }));
        }
        if !read_more(stream, buffer).await? {
            return Ok(None);
        }
    }
}

async fn read_more(stream: &mut TcpStream, buffer: &mut Vec<u8>) -> std::io::Result<bool> {
    let mut chunk = [0u8; 8192];
    let read = stream.read(&mut chunk).await?;
    buffer.extend_from_slice(&chunk[..read]);
    Ok(read > 0)
}

/// An [`HttpCacheStore`] in memory.
#[derive(Debug, Default)]
pub struct MemoryCacheStore {
    entries: Mutex<HashMap<CacheKey, (StoredResponse, u64)>>,
    uses: AtomicU64,
}

impl MemoryCacheStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// How many responses it holds.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Whether it holds nothing.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The stored response, without counting a use.
    pub fn peek(&self, key: &CacheKey) -> Option<StoredResponse> {
        self.lock().get(key).map(|(stored, _)| stored.clone())
    }

    /// Every key.
    pub fn keys(&self) -> Vec<CacheKey> {
        self.lock().keys().cloned().collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<CacheKey, (StoredResponse, u64)>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn tick(&self) -> u64 {
        self.uses.fetch_add(1, Ordering::SeqCst)
    }

    fn evict_while(
        entries: &mut HashMap<CacheKey, (StoredResponse, u64)>,
        mut matches: impl FnMut(&CacheKey) -> bool,
        max_bytes: u64,
    ) -> u64 {
        let mut evicted = 0;
        loop {
            let total: u64 = entries
                .iter()
                .filter(|(key, _)| matches(key))
                .map(|(_, (stored, _))| stored.size())
                .sum();
            if total <= max_bytes {
                return evicted;
            }
            let oldest = entries
                .iter()
                .filter(|(key, _)| matches(key))
                .min_by_key(|(_, (_, used))| *used)
                .map(|(key, _)| key.clone());
            match oldest {
                Some(key) => {
                    entries.remove(&key);
                    evicted += 1;
                }
                None => return evicted,
            }
        }
    }
}

impl HttpCacheStore for MemoryCacheStore {
    fn get(&self, key: &CacheKey) -> Result<Option<StoredResponse>, StoreError> {
        let tick = self.tick();
        Ok(self.lock().get_mut(key).map(|(stored, used)| {
            *used = tick;
            stored.clone()
        }))
    }

    fn put(&self, key: &CacheKey, response: &StoredResponse) -> Result<(), StoreError> {
        let tick = self.tick();
        self.lock().insert(key.clone(), (response.clone(), tick));
        Ok(())
    }

    fn remove(&self, key: &CacheKey) -> Result<(), StoreError> {
        self.lock().remove(key);
        Ok(())
    }

    fn evict_older_than(&self, last_fetched_before_ms: u64) -> Result<u64, StoreError> {
        let mut entries = self.lock();
        let before = entries.len();
        entries.retain(|_, (stored, _)| stored.fetched_at_ms >= last_fetched_before_ms);
        Ok((before - entries.len()) as u64)
    }

    fn remove_connection(
        &self,
        plugin: &PluginId,
        connection: &ConnectionId,
    ) -> Result<u64, StoreError> {
        let mut entries = self.lock();
        let before = entries.len();
        entries
            .retain(|key, _| !(&key.scope.plugin == plugin && &key.scope.connection == connection));
        Ok((before - entries.len()) as u64)
    }

    fn remove_plugin(&self, plugin: &PluginId) -> Result<u64, StoreError> {
        let mut entries = self.lock();
        let before = entries.len();
        entries.retain(|key, _| &key.scope.plugin != plugin);
        Ok((before - entries.len()) as u64)
    }

    fn trim(
        &self,
        plugin: &PluginId,
        plugin_max_bytes: u64,
        total_max_bytes: u64,
    ) -> Result<u64, StoreError> {
        let mut entries = self.lock();
        let mut evicted = Self::evict_while(
            &mut entries,
            |key| &key.scope.plugin == plugin,
            plugin_max_bytes,
        );
        evicted += Self::evict_while(&mut entries, |_| true, total_max_bytes);
        Ok(evicted)
    }
}
