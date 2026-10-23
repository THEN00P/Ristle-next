//! The background half of an HLS stream: fetches playlists, keys, init
//! sections and segments, decrypts them, and hands plaintext to the decode
//! thread through a bounded channel. The channel bounds the read-ahead; a
//! seek aborts the task and starts a new one.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use bytes::Bytes;
use tokio::sync::mpsc;
use url::Url;

use super::crypto::CbcStream;
use super::fmp4::{Sample, TrackInfo, parse_moof};
use super::playlist::{self, Key, Map, MediaPlaylist, Playlist, Segment, sequence_iv};
use crate::Error;
use crate::demux::mp4::{be32, be64};
use crate::fetch::{ByteRange, Fetch, FetchBody, FetchError, FetchRequest, read_all, redact_url};

/// Plaintext chunks in flight. With 16-64 KB chunks this keeps a few
/// seconds of high-bitrate audio ahead without holding whole files.
pub(crate) const CHANNEL_CAPACITY: usize = 64;

const PLAYLIST_LIMIT: usize = 16 << 20;
const INIT_LIMIT: usize = 16 << 20;
/// Bytes read from a segment's start to find its first `moof`.
const MOOF_PROBE: u64 = 4096;
/// Attempts per request when the network fails before any bytes arrive.
const ATTEMPTS: u32 = 3;

/// What the loader sends to the demuxer, in order.
#[derive(Debug)]
pub(crate) enum Message {
    /// A new init section applies from the next segment on.
    Init(Arc<Vec<u8>>),
    /// A segment starts.
    Segment {
        /// On the stream's timeline: the playlist position for VOD, the time
        /// since playback started for live.
        start_us: u64,
        discontinuity: bool,
        /// Set when the segment's bytes start at a sample inside its first
        /// `mdat` (a byte-range seek).
        resume: Option<Resume>,
    },
    /// Plaintext of the current segment.
    Data(Bytes),
    SegmentEnd,
    /// The playlist ended and every segment was sent.
    End,
    Failed(Error),
}

/// A byte-range seek's starting point inside an fMP4 segment.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Resume {
    /// Samples from the one the bytes start at, with their decode times.
    pub(crate) samples: Vec<(Sample, u64)>,
    pub(crate) mdat_end: Option<u64>,
    /// Decode time of the segment's first sample: the segment's start.
    pub(crate) origin: u64,
}

/// Decrypted init sections by location.
type InitCache = HashMap<(Url, Option<ByteRange>), Arc<Vec<u8>>>;

/// Fetching state shared by every loader of one stream: the Stream Source's
/// headers and origin, and the key and init caches, which live as long as
/// the stream, not one segment.
pub(crate) struct Shared {
    fetch: Arc<dyn Fetch>,
    origin: url::Origin,
    headers: Vec<(String, String)>,
    keys: Mutex<HashMap<Url, [u8; 16]>>,
    inits: Mutex<InitCache>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    pub(crate) fn new(fetch: Arc<dyn Fetch>, url: &Url, headers: Vec<(String, String)>) -> Self {
        Self {
            fetch,
            origin: url.origin(),
            headers,
            keys: Mutex::new(HashMap::new()),
            inits: Mutex::new(HashMap::new()),
        }
    }

    /// The Stream Source's headers go only to its own origin (WIT
    /// `stream-source`, `hls.headers`).
    fn request(&self, url: &Url, range: Option<ByteRange>) -> FetchRequest {
        let headers = if url.origin() == self.origin {
            self.headers.clone()
        } else {
            Vec::new()
        };
        FetchRequest {
            url: redact_url(url.as_str()),
            headers,
            range,
        }
    }

    pub(crate) async fn get(
        &self,
        url: &Url,
        range: Option<ByteRange>,
    ) -> Result<Box<dyn FetchBody>, Error> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.fetch.get(self.request(url, range)).await {
                Ok(body) => return Ok(body),
                Err(FetchError::Network(detail)) if attempt < ATTEMPTS => {
                    tracing::debug!(url = %redact_url(url.as_str()), attempt, %detail, "retrying");
                    tokio::time::sleep(Duration::from_millis(250 * u64::from(attempt))).await;
                }
                Err(error) => {
                    return Err(Error::Fetch {
                        url: redact_url(url.as_str()),
                        error,
                    });
                }
            }
        }
    }

    async fn bytes(
        &self,
        url: &Url,
        range: Option<ByteRange>,
        limit: usize,
    ) -> Result<Vec<u8>, Error> {
        let body = self.get(url, range).await?;
        read_all(body, limit).await.map_err(|error| Error::Fetch {
            url: redact_url(url.as_str()),
            error,
        })
    }

    pub(crate) async fn playlist(&self, url: &Url) -> Result<Playlist, Error> {
        let bytes = self.bytes(url, None, PLAYLIST_LIMIT).await?;
        let text =
            String::from_utf8(bytes).map_err(|_| Error::malformed("HLS playlist", "not UTF-8"))?;
        playlist::parse(&text, url)
    }

    async fn key(&self, key: &Key) -> Result<[u8; 16], Error> {
        if let Some(cached) = lock(&self.keys).get(&key.url) {
            return Ok(*cached);
        }
        let bytes = self.bytes(&key.url, None, 64).await?;
        let value: [u8; 16] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| Error::malformed("AES-128 key", format!("{} bytes", bytes.len())))?;
        lock(&self.keys).insert(key.url.clone(), value);
        Ok(value)
    }

    fn cipher(&self, key: [u8; 16], iv: [u8; 16]) -> CbcStream {
        CbcStream::new(&key, &iv)
    }

    /// The init section, decrypted when a key applies to it (ADR 0004: an
    /// `EXT-X-KEY` before the map encrypts the init section too).
    pub(crate) async fn init(&self, map: &Map) -> Result<Arc<Vec<u8>>, Error> {
        let cache_key = (map.url.clone(), map.range);
        if let Some(cached) = lock(&self.inits).get(&cache_key) {
            return Ok(cached.clone());
        }
        let bytes = self.bytes(&map.url, map.range, INIT_LIMIT).await?;
        let plain = match &map.key {
            Some(key) => {
                let iv = key.iv.unwrap_or_else(|| {
                    // RFC 8216bis requires an IV for an encrypted init
                    // section; fall back to the sequence-number rule.
                    tracing::warn!(url = %redact_url(map.url.as_str()), "encrypted init section without an IV");
                    sequence_iv(map.sequence)
                });
                let mut cipher = self.cipher(self.key(key).await?, iv);
                let mut plain = Vec::with_capacity(bytes.len());
                cipher.push(&bytes, &mut plain);
                cipher.finish(&mut plain)?;
                plain
            }
            None => bytes,
        };
        let init = Arc::new(plain);
        lock(&self.inits).insert(cache_key, init.clone());
        Ok(init)
    }
}

/// Where a loader starts.
pub(crate) enum Plan {
    /// A playlist with an end, from `start`. With `seek`, the first segment
    /// is entered by a byte-range seek if it is fMP4.
    Vod {
        playlist: Arc<MediaPlaylist>,
        start: usize,
        seek: Option<RangedSeek>,
    },
    /// A live playlist, from near its live edge.
    Live { playlist: MediaPlaylist },
}

/// A byte-range seek inside the first segment.
#[derive(Clone, Debug)]
pub(crate) struct RangedSeek {
    /// Microseconds into the segment.
    pub(crate) offset_us: u64,
    pub(crate) track: TrackInfo,
}

pub(crate) async fn run(shared: Arc<Shared>, plan: Plan, tx: mpsc::Sender<Message>) {
    let result = match plan {
        Plan::Vod {
            playlist,
            start,
            seek,
        } => vod(&shared, &playlist, start, seek, &tx).await,
        Plan::Live { playlist } => live(&shared, playlist, &tx).await,
    };
    match result {
        Ok(()) => {
            let _ = tx.send(Message::End).await;
        }
        // The receiver is gone: the stream was dropped or seeked.
        Err(Stop::Closed) => {}
        Err(Stop::Failed(error)) => {
            let _ = tx.send(Message::Failed(error)).await;
        }
    }
}

enum Stop {
    Closed,
    Failed(Error),
}

impl From<Error> for Stop {
    fn from(error: Error) -> Self {
        Self::Failed(error)
    }
}

async fn send(tx: &mpsc::Sender<Message>, message: Message) -> Result<(), Stop> {
    tx.send(message).await.map_err(|_| Stop::Closed)
}

async fn vod(
    shared: &Shared,
    playlist: &MediaPlaylist,
    start: usize,
    seek: Option<RangedSeek>,
    tx: &mpsc::Sender<Message>,
) -> Result<(), Stop> {
    let mut map: Option<Arc<Map>> = None;
    for (index, segment) in playlist.segments.iter().enumerate().skip(start) {
        send_init_if_changed(shared, segment, &mut map, tx).await?;
        let ranged = match &seek {
            Some(seek) if index == start && segment.map.is_some() => {
                match ranged_start(shared, segment, seek).await {
                    Ok(found) => found,
                    Err(error) => {
                        tracing::warn!(%error, "byte-range seek failed; reading the whole segment");
                        None
                    }
                }
            }
            _ => None,
        };
        stream_segment(
            shared,
            segment,
            segment.start_us,
            segment.discontinuity,
            ranged,
            tx,
        )
        .await?;
    }
    Ok(())
}

/// Where live playback starts: no closer than three segments to the live
/// edge (RFC 8216, 6.3.3).
pub(crate) fn live_start(playlist: &MediaPlaylist) -> usize {
    playlist.segments.len().saturating_sub(3)
}

async fn live(
    shared: &Shared,
    mut playlist: MediaPlaylist,
    tx: &mpsc::Sender<Message>,
) -> Result<(), Stop> {
    let start = live_start(&playlist);
    let mut next_sequence = playlist.segments.get(start).map_or(0, |s| s.sequence);
    let mut timeline_us = 0u64;
    let mut map: Option<Arc<Map>> = None;
    let mut first = true;
    loop {
        let mut played = false;
        let mut jumped = playlist
            .segments
            .first()
            .is_some_and(|s| s.sequence > next_sequence);
        if jumped && !first {
            tracing::warn!(
                next_sequence,
                "fell behind the live playlist; skipping ahead"
            );
        }
        let from = next_sequence;
        for segment in playlist.segments.iter().filter(|s| s.sequence >= from) {
            send_init_if_changed(shared, segment, &mut map, tx).await?;
            let discontinuity = segment.discontinuity || (jumped && !first);
            stream_segment(shared, segment, timeline_us, discontinuity, None, tx).await?;
            timeline_us += segment.duration_us;
            next_sequence = segment.sequence + 1;
            played = true;
            jumped = false;
            first = false;
        }
        if playlist.ended {
            return Ok(());
        }
        // RFC 8216, 6.3.4: reload after the target duration, or half of it
        // when the playlist did not change.
        let target = playlist.target_duration_us.max(1_000_000);
        let wait = if played { target } else { target / 2 };
        tokio::select! {
            () = tokio::time::sleep(Duration::from_micros(wait)) => {}
            () = tx.closed() => return Err(Stop::Closed),
        }
        playlist = match shared.playlist(&playlist.url).await? {
            Playlist::Media(media) => media,
            Playlist::Master(_) => {
                return Err(
                    Error::malformed("HLS playlist", "a live playlist became a master").into(),
                );
            }
        };
    }
}

async fn send_init_if_changed(
    shared: &Shared,
    segment: &Segment,
    current: &mut Option<Arc<Map>>,
    tx: &mpsc::Sender<Message>,
) -> Result<(), Stop> {
    let Some(map) = &segment.map else {
        return Ok(());
    };
    if current.as_deref() == Some(map.as_ref()) {
        return Ok(());
    }
    let init = shared.init(map).await?;
    send(tx, Message::Init(init)).await?;
    *current = Some(map.clone());
    Ok(())
}

/// Where a ranged read of a segment starts.
struct Ranged {
    resume: Resume,
    /// Offset of the first sample within the segment.
    offset: u64,
}

/// Streams one segment's plaintext, from the start or from a sample.
async fn stream_segment(
    shared: &Shared,
    segment: &Segment,
    start_us: u64,
    discontinuity: bool,
    ranged: Option<Ranged>,
    tx: &mpsc::Sender<Message>,
) -> Result<(), Stop> {
    let key = match &segment.key {
        Some(key) => Some(shared.key(key).await?),
        None => None,
    };
    let iv = segment
        .key
        .as_ref()
        .and_then(|k| k.iv)
        .unwrap_or_else(|| sequence_iv(segment.sequence));
    let (resume, offset) = match ranged {
        Some(Ranged { resume, offset }) => (Some(resume), offset),
        None => (None, 0),
    };
    send(
        tx,
        Message::Segment {
            start_us,
            discontinuity,
            resume,
        },
    )
    .await?;

    // With encryption, CBC needs the ciphertext block before the offset as
    // the IV, so the range starts one block early and the plaintext before
    // the offset is dropped.
    let (fetch_from, mut cipher, mut skip) = match key {
        Some(key) => {
            let aligned = offset / 16 * 16;
            if aligned == 0 {
                (0, Some((key, Some(iv))), offset as usize)
            } else {
                (aligned - 16, Some((key, None)), (offset - aligned) as usize)
            }
        }
        None => (0, None, 0),
    };
    let from = fetch_from.max(if cipher.is_none() { offset } else { 0 });
    let range = sub_range(segment.range, from);
    let mut body = shared.get(&segment.url, range).await?;
    let mut stream: Option<CbcStream> = None;
    let mut iv_block: Vec<u8> = Vec::new();
    let mut plain = Vec::new();
    loop {
        let chunk = body.chunk().await.map_err(|error| Error::Fetch {
            url: redact_url(segment.url.as_str()),
            error,
        })?;
        let Some(chunk) = chunk else { break };
        let mut data: &[u8] = &chunk;
        plain.clear();
        match &mut cipher {
            None => plain.extend_from_slice(data),
            Some((key, iv)) => {
                if stream.is_none() {
                    let iv = match iv {
                        Some(iv) => *iv,
                        None => {
                            // The first 16 bytes are the previous block.
                            let need = 16 - iv_block.len();
                            let take = need.min(data.len());
                            iv_block.extend_from_slice(&data[..take]);
                            data = &data[take..];
                            if iv_block.len() < 16 {
                                continue;
                            }
                            let mut iv = [0u8; 16];
                            iv.copy_from_slice(&iv_block);
                            iv
                        }
                    };
                    stream = Some(CbcStream::new(key, &iv));
                }
                if let Some(stream) = &mut stream {
                    stream.push(data, &mut plain);
                }
            }
        }
        forward(&plain, &mut skip, tx).await?;
    }
    if let Some(stream) = stream {
        plain.clear();
        stream.finish(&mut plain)?;
        forward(&plain, &mut skip, tx).await?;
    } else if cipher.is_some() {
        return Err(Error::malformed("AES-128 segment", "empty").into());
    }
    send(tx, Message::SegmentEnd).await
}

async fn forward(plain: &[u8], skip: &mut usize, tx: &mpsc::Sender<Message>) -> Result<(), Stop> {
    let dropped = (*skip).min(plain.len());
    *skip -= dropped;
    if plain.len() > dropped {
        send(tx, Message::Data(Bytes::copy_from_slice(&plain[dropped..]))).await?;
    }
    Ok(())
}

/// The part of a segment's resource from `offset` (within the segment) on.
fn sub_range(range: Option<ByteRange>, offset: u64) -> Option<ByteRange> {
    match range {
        Some(range) => Some(ByteRange {
            start: range.start + offset,
            length: range.length.map(|length| length.saturating_sub(offset)),
        }),
        None if offset > 0 => Some(ByteRange {
            start: offset,
            length: None,
        }),
        None => None,
    }
}

/// Reads a segment's first `moof` and finds the sample at the seek offset
/// (ADR 0004 amendment 5: two round trips and a few KB regardless of
/// segment length). `None` when the segment cannot be entered that way,
/// such as when its first fragment ends before the target.
async fn ranged_start(
    shared: &Shared,
    segment: &Segment,
    seek: &RangedSeek,
) -> Result<Option<Ranged>, Error> {
    let mut probe = MOOF_PROBE;
    let head = loop {
        let length = segment
            .range
            .and_then(|r| r.length)
            .map_or(probe, |l| l.min(probe));
        let range = Some(match segment.range {
            Some(r) => ByteRange {
                start: r.start,
                length: Some(length),
            },
            None => ByteRange {
                start: 0,
                length: Some(length),
            },
        });
        let mut head = shared
            .bytes(&segment.url, range, (length + 32) as usize)
            .await?;
        if let Some(key) = &segment.key {
            let iv = key.iv.unwrap_or_else(|| sequence_iv(segment.sequence));
            let mut plain = Vec::new();
            let mut cipher = CbcStream::new(&shared.key(key).await?, &iv);
            // Whole blocks only; the probe ends inside the segment.
            head.truncate(head.len() / 16 * 16);
            cipher.push(&head, &mut plain);
            head = plain;
        }
        match find_moof(&head)? {
            Found::Moof { start, size, mdat } => break (head, start, size, mdat),
            Found::Need(bytes) if bytes > probe && bytes <= 1 << 20 => probe = bytes,
            Found::Need(_) | Found::Missing => return Ok(None),
        }
    };
    let (head, moof_start, moof_size, mdat) = head;
    let header = if be32(&head, moof_start) == Some(1) {
        16
    } else {
        8
    };
    let body = &head[moof_start + header..moof_start + moof_size];
    let fragment = parse_moof(body, moof_start as u64, &seek.track)?;
    let origin = fragment.decode_time.unwrap_or(0);
    let target = origin + seek.offset_us * u64::from(seek.track.timescale) / 1_000_000;
    let mut time = origin;
    let mut timed = Vec::with_capacity(fragment.samples.len());
    for sample in &fragment.samples {
        timed.push((*sample, time));
        time += u64::from(sample.duration);
    }
    if time <= target || timed.is_empty() {
        // The target lies in a later fragment of this segment.
        return Ok(None);
    }
    let index = timed.iter().rposition(|(_, t)| *t <= target).unwrap_or(0);
    let mdat_end = mdat.and_then(|(start, size)| size.map(|size| start + size));
    if let Some(end) = mdat_end
        && timed[index..]
            .iter()
            .any(|(s, _)| s.offset + u64::from(s.size) > end)
    {
        return Ok(None);
    }
    let offset = timed[index].0.offset;
    Ok(Some(Ranged {
        resume: Resume {
            samples: timed.split_off(index),
            mdat_end,
            origin,
        },
        offset,
    }))
}

enum Found {
    /// The moof at `start` of `size` bytes, and the mdat right after it
    /// (start, size if known) when its header is in the head.
    Moof {
        start: usize,
        size: usize,
        mdat: Option<(u64, Option<u64>)>,
    },
    /// The head must be this long to hold the moof and the mdat header.
    Need(u64),
    Missing,
}

fn find_moof(head: &[u8]) -> Result<Found, Error> {
    let mut at = 0usize;
    while at + 8 <= head.len() {
        let size32 = be32(head, at).unwrap_or(0);
        let kind = &head[at + 4..at + 8];
        let size = match size32 {
            0 => return Ok(Found::Missing),
            1 => match be64(head, at + 8) {
                Some(size) => size,
                None => return Ok(Found::Need(at as u64 + 16)),
            },
            n => u64::from(n),
        };
        if kind == b"moof" {
            let end = at as u64 + size;
            // The mdat header, and a block an encrypted probe holds back.
            if (head.len() as u64) < end + 16 {
                return Ok(Found::Need(end + 32));
            }
            let end = end as usize;
            let mdat = (&head[end + 4..end + 8] == b"mdat").then(|| {
                let size = match be32(head, end).unwrap_or(0) {
                    0 => None,
                    1 => be64(head, end + 8),
                    n => Some(u64::from(n)),
                };
                (end as u64, size)
            });
            return Ok(Found::Moof {
                start: at,
                size: size as usize,
                mdat,
            });
        }
        if kind == b"mdat" {
            return Ok(Found::Missing);
        }
        at = at
            .checked_add(usize::try_from(size).unwrap_or(usize::MAX))
            .ok_or_else(|| Error::malformed("fMP4 segment", "box size overflows"))?;
    }
    Ok(Found::Need(at as u64 + 16))
}
