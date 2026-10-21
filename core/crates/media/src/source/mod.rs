//! Remote byte sources for progressive streams: progressive HTTP with Range
//! requests and Plugin Byte Sources (ADR 0004 variants 1, 3 and 4).
//!
//! A fetch task on the core runtime fills a window of the resource ahead of
//! the reader, decrypting as it goes. Symphonia reads the window through a
//! synchronous [`RemoteSource`]. The decode thread asks [`Remote::ready`]
//! before each packet, so it sees `Pending` rather than blocking on the
//! network; only a container's own seeks (an Ogg bisection, an MP4 `moov` at
//! the end) wait for bytes inside a read.

pub(crate) mod decrypt;

use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use symphonia::core::io::MediaSource;
use tokio::sync::Notify;

use self::decrypt::{Decryption, Decryptor};
use crate::Error;
use crate::fetch::{ByteRange, Fetch, FetchBody, FetchRequest};

/// How far ahead of the reader the fetch task fills.
const AHEAD: u64 = 4 << 20;
/// How much behind the reader stays buffered, for a container's re-reads.
const BEHIND: u64 = 256 << 10;
/// A read this far past the window waits for it; further away it restarts
/// the fetch there.
const RESTART_GAP: u64 = 256 << 10;
/// Buffered bytes ahead of the reader before a packet is demuxed: more than
/// any audio packet.
pub(crate) const WATERMARK: u64 = 128 << 10;
/// How long a read inside the container waits before failing.
const READ_TIMEOUT: Duration = Duration::from_secs(20);
/// The size of a Byte Source read (WIT `byte-source.read`: at least 64 KiB).
const BYTE_SOURCE_CHUNK: u32 = 64 << 10;

/// A Plugin's Byte Source handle (WIT `plugin-bytes.handle`). It may carry
/// signed URLs or keys, so `Debug` shows only its length.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ByteSourceHandle(pub String);

impl std::fmt::Debug for ByteSourceHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ByteSourceHandle(<{} bytes>)", self.0.len())
    }
}

/// Why a Byte Source read failed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SourceError {
    /// Worth retrying (the Plugin's own fetch failed).
    #[error("transient: {0}")]
    Transient(String),
    /// The source cannot be read.
    #[error("{0}")]
    Failed(String),
}

/// Reads a Plugin Byte Source: `ristle-app` implements it over the Plugin
/// host's `byte-source.read` export (section 3 of the phase 2 plan).
#[async_trait]
pub trait ByteSourceReader: Send + Sync {
    /// Up to `length` bytes from `offset`. Fewer bytes than asked for means
    /// the end of the stream.
    async fn read(
        &self,
        handle: &ByteSourceHandle,
        offset: u64,
        length: u32,
    ) -> Result<Bytes, SourceError>;
}

/// A resource that can be read from any offset.
#[async_trait]
pub(crate) trait Feed: Send + Sync {
    /// Starts reading at `offset`. Returns the body and, if known, the
    /// resource's total length.
    async fn open(&self, offset: u64) -> Result<(Box<dyn FeedBody>, Option<u64>), Error>;
}

#[async_trait]
pub(crate) trait FeedBody: Send {
    async fn next(&mut self) -> Result<Option<Bytes>, Error>;
}

/// Progressive HTTP: one GET with a Range from the offset to the end.
pub(crate) struct HttpFeed {
    pub(crate) fetch: Arc<dyn Fetch>,
    pub(crate) url: String,
    pub(crate) headers: Vec<(String, String)>,
}

struct HttpBody {
    url: String,
    body: Box<dyn FetchBody>,
}

#[async_trait]
impl FeedBody for HttpBody {
    async fn next(&mut self) -> Result<Option<Bytes>, Error> {
        self.body.chunk().await.map_err(|error| Error::Fetch {
            url: self.url.clone(),
            error,
        })
    }
}

#[async_trait]
impl Feed for HttpFeed {
    async fn open(&self, offset: u64) -> Result<(Box<dyn FeedBody>, Option<u64>), Error> {
        let range = (offset > 0).then_some(ByteRange {
            start: offset,
            length: None,
        });
        let request = FetchRequest {
            url: self.url.clone(),
            headers: self.headers.clone(),
            range,
        };
        let body = self
            .fetch
            .get(request)
            .await
            .map_err(|error| Error::Fetch {
                url: self.url.clone(),
                error,
            })?;
        let total = body.total_length();
        Ok((
            Box::new(HttpBody {
                url: self.url.clone(),
                body,
            }),
            total,
        ))
    }
}

/// A Plugin Byte Source, read in 64 KiB chunks.
pub(crate) struct ByteSourceFeed {
    pub(crate) reader: Arc<dyn ByteSourceReader>,
    pub(crate) handle: ByteSourceHandle,
    pub(crate) length: Option<u64>,
}

struct ByteSourceBody {
    reader: Arc<dyn ByteSourceReader>,
    handle: ByteSourceHandle,
    offset: u64,
    done: bool,
}

#[async_trait]
impl FeedBody for ByteSourceBody {
    async fn next(&mut self) -> Result<Option<Bytes>, Error> {
        if self.done {
            return Ok(None);
        }
        let bytes = self
            .reader
            .read(&self.handle, self.offset, BYTE_SOURCE_CHUNK)
            .await
            .map_err(Error::ByteSource)?;
        if bytes.len() > BYTE_SOURCE_CHUNK as usize {
            return Err(Error::malformed(
                "Byte Source",
                "a read returned more than asked for",
            ));
        }
        // Fewer bytes than asked for: the end.
        self.done = bytes.len() < BYTE_SOURCE_CHUNK as usize;
        self.offset += bytes.len() as u64;
        Ok((!bytes.is_empty()).then_some(bytes))
    }
}

#[async_trait]
impl Feed for ByteSourceFeed {
    async fn open(&self, offset: u64) -> Result<(Box<dyn FeedBody>, Option<u64>), Error> {
        let body = ByteSourceBody {
            reader: self.reader.clone(),
            handle: self.handle.clone(),
            offset,
            done: false,
        };
        Ok((Box::new(body), self.length))
    }
}

/// The window of plaintext the fetch task fills and the reader drains.
struct State {
    /// `data[0]` is at resource offset `start`.
    start: u64,
    data: Vec<u8>,
    /// The resource's (plaintext) length, once known.
    len: Option<u64>,
    /// The fetch reached the end of the resource.
    complete: bool,
    /// Why the fetch failed. Kept until the reader moves elsewhere.
    failed: Option<String>,
    /// The same failure, typed, for the stream to report once.
    error: Option<Error>,
    /// The reader's position (resource offset).
    pos: u64,
    /// The reader wants the fetch to restart here.
    want: Option<u64>,
    /// Bumped on every restart, so a stale fetch stops writing.
    generation: u64,
    closed: bool,
}

impl State {
    fn end(&self) -> u64 {
        self.start + self.data.len() as u64
    }
}

pub(crate) struct Remote {
    state: Mutex<State>,
    /// Wakes a reader waiting for bytes.
    arrived: Condvar,
    /// Wakes the fetch task: a restart request, or room in the window.
    wake: Notify,
    /// Interrupts a fetch waiting on a stalled body: a restart or close.
    restarted: Notify,
}

impl Remote {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                start: 0,
                data: Vec::new(),
                len: None,
                complete: false,
                failed: None,
                error: None,
                pos: 0,
                want: None,
                generation: 0,
                closed: false,
            }),
            arrived: Condvar::new(),
            wake: Notify::new(),
            restarted: Notify::new(),
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Stops the fetch task.
    pub(crate) fn close(&self) {
        self.lock().closed = true;
        self.wake.notify_one();
        self.restarted.notify_one();
        self.arrived.notify_all();
    }

    /// The fetch failure, typed (`Error::Fetch`, `Error::ByteSource`), once.
    /// Symphonia only sees an I/O error; the stream reports this instead.
    pub(crate) fn take_error(&self) -> Option<Error> {
        self.lock().error.take()
    }

    fn request_restart(&self, s: &mut State, at: u64) {
        restart(s, at);
        self.wake.notify_one();
        self.restarted.notify_one();
    }

    /// Whether the next packet can be demuxed without waiting: enough bytes
    /// ahead of the reader, or everything to the end. Starts a fetch at the
    /// reader's position when nothing is coming there.
    pub(crate) fn ready(&self, watermark: u64) -> bool {
        let mut s = self.lock();
        let end = s.end();
        if s.failed.is_some() || s.closed {
            return true;
        }
        if s.pos >= s.start && s.pos <= end {
            if end - s.pos >= watermark || s.complete || s.len.is_some_and(|len| end >= len) {
                return true;
            }
            return false;
        }
        if s.want != Some(s.pos) {
            let pos = s.pos;
            self.request_restart(&mut s, pos);
        }
        false
    }

    /// The fetch loop. Runs until [`Remote::close`].
    pub(crate) async fn run(self: Arc<Self>, feed: Arc<dyn Feed>, decryption: Option<Decryption>) {
        loop {
            let job = {
                let mut s = self.lock();
                if s.closed {
                    return;
                }
                if let Some(want) = s.want.take() {
                    Some((want, s.generation))
                } else if !s.complete && s.failed.is_none() && s.end() < s.pos + AHEAD {
                    Some((s.end(), s.generation))
                } else {
                    None
                }
            };
            let Some((from, generation)) = job else {
                self.wake.notified().await;
                continue;
            };
            if let Err(error) = self
                .fill(&*feed, decryption.as_ref(), from, generation)
                .await
            {
                let mut s = self.lock();
                if s.generation == generation {
                    tracing::warn!(%error, "fetching the stream failed");
                    s.failed = Some(error.to_string());
                    s.error = Some(error);
                }
                drop(s);
                self.arrived.notify_all();
            }
        }
    }

    async fn fill(
        &self,
        feed: &dyn Feed,
        decryption: Option<&Decryption>,
        from: u64,
        generation: u64,
    ) -> Result<(), Error> {
        let (mut decryptor, fetch_from, mut write_at) = match decryption {
            Some(params) => {
                let (decryptor, fetched) = Decryptor::at(params, from)?;
                (Some(decryptor), fetched.from, fetched.output_from)
            }
            None => (None, from, from),
        };
        let (mut body, total) = feed.open(fetch_from).await?;
        {
            let mut s = self.lock();
            if s.generation != generation || s.closed {
                return Ok(());
            }
            if s.data.is_empty() {
                s.start = write_at;
            }
            // With PKCS#7 the plaintext is shorter than the resource; its
            // length is known at the end.
            let padded = decryption.is_some_and(|d| d.mode == decrypt::AesMode::CbcPkcs7);
            if !padded {
                s.len = s.len.or(total);
            }
        }
        let mut plain = Vec::new();
        loop {
            // A restart must not wait for a stalled body: race the read
            // against it. The read future survives spurious wakeups.
            let chunk = {
                let next = body.next();
                tokio::pin!(next);
                loop {
                    tokio::select! {
                        chunk = &mut next => break chunk?,
                        () = self.restarted.notified() => {
                            let s = self.lock();
                            if s.generation != generation || s.closed {
                                return Ok(());
                            }
                        }
                    }
                }
            };
            plain.clear();
            let at_end = chunk.is_none();
            match (&mut decryptor, &chunk) {
                (Some(d), Some(chunk)) => d.push(chunk, &mut plain)?,
                (None, Some(chunk)) => plain.extend_from_slice(chunk),
                (Some(_), None) => {
                    if let Some(d) = decryptor.take() {
                        d.finish(&mut plain)?;
                    }
                }
                (None, None) => {}
            }
            let step = {
                let mut s = self.lock();
                if s.generation != generation || s.closed {
                    Step::Stop
                } else {
                    append(&mut s, write_at, &plain);
                    write_at += plain.len() as u64;
                    if at_end {
                        s.complete = true;
                        if s.len.is_none() || decryption.is_some() {
                            s.len = Some(write_at);
                        }
                        Step::Done
                    } else {
                        // Drop what the reader is well past.
                        if s.pos > s.start + BEHIND && s.pos <= s.end() {
                            let drop_to = s.pos - BEHIND;
                            let n = (drop_to - s.start) as usize;
                            s.data.drain(..n);
                            s.start = drop_to;
                        }
                        Step::More
                    }
                }
            };
            self.arrived.notify_all();
            match step {
                Step::Stop | Step::Done => return Ok(()),
                Step::More => {}
            }
            // Flow control: wait while the window is far enough ahead.
            loop {
                let wait = {
                    let s = self.lock();
                    if s.generation != generation || s.closed {
                        return Ok(());
                    }
                    s.end() >= s.pos + AHEAD && s.pos >= s.start
                };
                if !wait {
                    break;
                }
                self.wake.notified().await;
            }
        }
    }
}

enum Step {
    Stop,
    Done,
    More,
}

/// Appends plaintext that starts at `at`, skipping what the window already
/// holds (a continuation after an error restarts on a block boundary).
fn append(s: &mut State, at: u64, plain: &[u8]) {
    let end = s.end();
    if at > end {
        // A gap: the window restarts here.
        s.data.clear();
        s.start = at;
        s.data.extend_from_slice(plain);
        return;
    }
    let overlap = (end - at).min(plain.len() as u64) as usize;
    s.data.extend_from_slice(&plain[overlap..]);
}

fn restart(s: &mut State, at: u64) {
    s.want = Some(at);
    s.generation += 1;
    s.data.clear();
    s.start = at;
    s.complete = false;
    s.failed = None;
    s.error = None;
}

/// The synchronous side: Symphonia's source.
pub(crate) struct RemoteSource {
    remote: Arc<Remote>,
    /// Plaintext bytes before the container (the decryption `skip`).
    skip: u64,
}

impl RemoteSource {
    pub(crate) fn new(remote: Arc<Remote>, skip: u64) -> Self {
        remote.lock().pos = skip;
        Self { remote, skip }
    }
}

impl Read for RemoteSource {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let remote = &self.remote;
        let mut s = remote.lock();
        loop {
            if s.closed {
                return Err(io::Error::other("the stream was closed"));
            }
            let pos = s.pos;
            let end = s.end();
            if pos >= s.start && pos < end {
                let from = (pos - s.start) as usize;
                let n = buf.len().min(s.data.len() - from);
                buf[..n].copy_from_slice(&s.data[from..from + n]);
                s.pos += n as u64;
                drop(s);
                remote.wake.notify_one();
                return Ok(n);
            }
            if s.len.is_some_and(|len| pos >= len) || (s.complete && pos >= s.start && pos >= end) {
                return Ok(0);
            }
            let inside = pos >= s.start && pos <= end + RESTART_GAP;
            if let Some(failed) = &s.failed
                && inside
            {
                return Err(io::Error::other(failed.clone()));
            }
            if !inside && s.want != Some(pos) {
                remote.request_restart(&mut s, pos);
            }
            let (next, timeout) = remote
                .arrived
                .wait_timeout(s, READ_TIMEOUT)
                .unwrap_or_else(PoisonError::into_inner);
            s = next;
            if timeout.timed_out() && s.pos == pos && !(s.pos >= s.start && s.pos < s.end()) {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "no bytes arrived"));
            }
        }
    }
}

impl Seek for RemoteSource {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let mut s = self.remote.lock();
        let target = match pos {
            SeekFrom::Start(offset) => Some(self.skip + offset),
            SeekFrom::Current(delta) => s.pos.checked_add_signed(delta),
            SeekFrom::End(delta) => s
                .len
                .ok_or_else(|| io::Error::new(io::ErrorKind::Unsupported, "length unknown"))?
                .checked_add_signed(delta),
        };
        let target = target
            .filter(|t| *t >= self.skip)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before the start"))?;
        s.pos = target;
        Ok(target - self.skip)
    }
}

impl MediaSource for RemoteSource {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        self.remote
            .lock()
            .len
            .map(|len| len.saturating_sub(self.skip))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// The first body yields one chunk, then stalls forever; later bodies
    /// serve the resource from their offset.
    struct Stalling {
        data: Bytes,
        opens: AtomicUsize,
    }

    struct Body {
        data: Bytes,
        stall: bool,
        sent: bool,
    }

    #[async_trait]
    impl FeedBody for Body {
        async fn next(&mut self) -> Result<Option<Bytes>, Error> {
            if self.sent {
                if self.stall {
                    std::future::pending::<()>().await;
                }
                return Ok(None);
            }
            self.sent = true;
            Ok(Some(self.data.clone()))
        }
    }

    #[async_trait]
    impl Feed for Stalling {
        async fn open(&self, offset: u64) -> Result<(Box<dyn FeedBody>, Option<u64>), Error> {
            let first = self.opens.fetch_add(1, Ordering::SeqCst) == 0;
            let data = if first {
                self.data.slice(..1000)
            } else {
                self.data.slice(offset as usize..)
            };
            Ok((
                Box::new(Body {
                    data,
                    stall: first,
                    sent: false,
                }),
                Some(self.data.len() as u64),
            ))
        }
    }

    #[test]
    fn a_seek_interrupts_a_stalled_body() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let data: Vec<u8> = (0..(2u32 << 20)).map(|i| (i % 251) as u8).collect();
        let feed = Arc::new(Stalling {
            data: Bytes::from(data.clone()),
            opens: AtomicUsize::new(0),
        });
        let remote = Remote::new();
        let task = runtime.spawn(remote.clone().run(feed, None));
        let mut source = RemoteSource::new(remote.clone(), 0);
        let mut buf = [0u8; 100];
        assert_eq!(source.read(&mut buf).unwrap(), 100);
        // Far outside the window, while the first body is stalled.
        let target = 1_500_000;
        source.seek(SeekFrom::Start(target)).unwrap();
        let started = std::time::Instant::now();
        assert_eq!(source.read(&mut buf).unwrap(), 100);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(&buf[..], &data[target as usize..target as usize + 100]);
        remote.close();
        task.abort();
    }
}
