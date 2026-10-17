//! HLS audio client: playlist parsing, segment fetching, AES-128 decryption,
//! packed-audio ID3 stripping and TS demuxing, exposed as one byte stream to
//! Symphonia. Ported from Ristle legacy's `provider-media` and extended with
//! EXT-X-MEDIA audio renditions, packed ADTS and MPEG-TS.

use crate::registry::{DecoderRegistry, StreamInfo};
use crate::ts::TsDemuxer;
use anyhow::{anyhow, bail, Context, Result};
use std::collections::HashMap;
use std::io::{self, Read};
use std::time::{Duration, Instant};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatReader;
use symphonia::core::io::ReadOnlySource;
use url::Url;

#[derive(Clone, Debug)]
pub struct Key {
    pub url: String,
    pub iv: Option<[u8; 16]>,
}

#[derive(Clone, Debug)]
pub struct Segment {
    pub url: String,
    pub start_s: f64,
    pub dur_s: f64,
    pub key: Option<Key>,
    pub seq: u64,
}

#[derive(Clone, Debug)]
pub struct Playlist {
    pub url: String,
    pub init: Option<String>,
    pub segments: Vec<Segment>,
    /// CODECS of the chosen variant, e.g. "mp4a.40.2" — the only place an
    /// HE-AAC stream announces itself before decoding.
    pub codecs: Option<String>,
}

impl Playlist {
    pub fn duration_s(&self) -> f64 {
        self.segments.last().map_or(0.0, |s| s.start_s + s.dur_s)
    }
    pub fn index_at(&self, t: f64) -> usize {
        self.segments.iter().rposition(|s| s.start_s <= t).unwrap_or(0)
    }
}

#[derive(Clone)]
pub struct Fetcher {
    agent: ureq::Agent,
    pub requests: std::sync::Arc<std::sync::atomic::AtomicU64>,
    pub fetch_ns: std::sync::Arc<std::sync::atomic::AtomicU64>,
    /// Keys live as long as the playlist, not one segment reader.
    keys: std::sync::Arc<std::sync::Mutex<HashMap<String, [u8; 16]>>>,
    /// Decode segments while they download instead of buffering each whole.
    pub streaming: bool,
}

impl Fetcher {
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .into();
        Self { agent, requests: Default::default(), fetch_ns: Default::default(), keys: Default::default(), streaming: false }
    }
    fn get(&self, url: &str) -> Result<ureq::http::Response<ureq::Body>> {
        let (base, range) = match url.split_once("#bytes=") {
            Some((b, r)) => {
                let (o, l) = r.split_once('-').ok_or_else(|| anyhow!("bad range"))?;
                let (o, l): (u64, u64) = (o.parse()?, l.parse()?);
                (b, Some(format!("bytes={o}-{}", o + l - 1)))
            }
            None => (url, None),
        };
        let mut req = self.agent.get(base);
        if let Some(r) = range {
            req = req.header("Range", r);
        }
        Ok(req.call().with_context(|| format!("GET {url}"))?)
    }

    pub fn bytes(&self, url: &str) -> Result<Vec<u8>> {
        self.requests.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let t0 = Instant::now();
        let mut r = self.get(url)?;
        let body = r.body_mut().with_config().limit(64 << 20).read_to_vec()?;
        self.fetch_ns.fetch_add(t0.elapsed().as_nanos() as u64, std::sync::atomic::Ordering::Relaxed);
        Ok(body)
    }
    pub fn stream(&self, url: &str) -> Result<Box<dyn Read + Send + Sync>> {
        self.requests.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let t0 = Instant::now();
        let r = self.get(url)?;
        self.fetch_ns.fetch_add(t0.elapsed().as_nanos() as u64, std::sync::atomic::Ordering::Relaxed);
        Ok(Box::new(TimedReader { inner: Box::new(r.into_body().into_reader()), fetch_ns: self.fetch_ns.clone() }))
    }
    pub fn text(&self, url: &str) -> Result<String> {
        Ok(String::from_utf8(self.bytes(url)?)?)
    }
}

/// Counts time spent inside the body's reads (waiting for the network) as
/// fetch time, so streaming runs report their network share too.
struct TimedReader {
    inner: Box<dyn Read + Send + Sync>,
    fetch_ns: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl Read for TimedReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let t0 = Instant::now();
        let n = self.inner.read(buf)?;
        self.fetch_ns.fetch_add(t0.elapsed().as_nanos() as u64, std::sync::atomic::Ordering::Relaxed);
        Ok(n)
    }
}

fn attr(attrs: &str, name: &str) -> Option<String> {
    let mut rest = attrs;
    while let Some(i) = rest.find('=') {
        let key = rest[..i].trim_matches(|c: char| c == ',' || c.is_whitespace());
        let after = &rest[i + 1..];
        let (value, remaining) = if let Some(q) = after.strip_prefix('"') {
            let end = q.find('"')?;
            (&q[..end], &q[end + 1..])
        } else {
            let end = after.find(',').unwrap_or(after.len());
            (&after[..end], &after[end..])
        };
        if key.eq_ignore_ascii_case(name) {
            return Some(value.to_string());
        }
        rest = remaining;
    }
    None
}

fn join(base: &str, r: &str) -> Result<String> {
    Ok(Url::parse(base)?.join(r)?.to_string())
}

fn parse_iv(v: &str) -> Result<[u8; 16]> {
    let h = v.trim_start_matches("0x").trim_start_matches("0X");
    if h.len() != 32 {
        bail!("IV must be 16 bytes");
    }
    let mut iv = [0u8; 16];
    for (i, b) in iv.iter_mut().enumerate() {
        *b = u8::from_str_radix(&h[i * 2..i * 2 + 2], 16)?;
    }
    Ok(iv)
}

/// Loads a playlist. For a master playlist, prefers an audio-only rendition
/// (EXT-X-MEDIA TYPE=AUDIO, or a variant whose CODECS has no video), else the
/// lowest-bandwidth variant.
pub fn load(f: &Fetcher, url: &str) -> Result<Playlist> {
    let text = f.text(url)?;
    if !text.trim_start().starts_with("#EXTM3U") {
        bail!("not an M3U8 playlist");
    }
    if text.contains("#EXT-X-STREAM-INF") {
        let mut audio_groups: HashMap<String, (String, Option<String>)> = HashMap::new();
        let mut variants: Vec<(u64, String, Option<String>, Option<String>)> = Vec::new();
        let mut pending: Option<String> = None;
        for line in text.lines().map(str::trim) {
            if let Some(a) = line.strip_prefix("#EXT-X-MEDIA:") {
                if attr(a, "TYPE").as_deref() == Some("AUDIO") {
                    if let (Some(g), Some(u)) = (attr(a, "GROUP-ID"), attr(a, "URI")) {
                        audio_groups.entry(g).or_insert((join(&text_base(url), &u)?, None));
                    }
                }
            } else if let Some(a) = line.strip_prefix("#EXT-X-STREAM-INF:") {
                pending = Some(a.to_string());
            } else if let (Some(a), false) = (&pending, line.is_empty() || line.starts_with('#')) {
                let bw = attr(a, "BANDWIDTH").and_then(|b| b.parse().ok()).unwrap_or(0);
                variants.push((bw, join(url, line)?, attr(a, "CODECS"), attr(a, "AUDIO")));
                pending = None;
            }
        }
        // 1) An audio rendition group referenced by a variant.
        for (_, _, codecs, group) in &variants {
            if let Some((u, _)) = group.as_ref().and_then(|g| audio_groups.get(g)) {
                let audio_codec = codecs.as_deref().and_then(|c| c.split(',').find(|c| c.trim().starts_with("mp4a")).map(|c| c.trim().to_string()));
                let mut p = load(f, u)?;
                p.codecs = audio_codec;
                return Ok(p);
            }
        }
        // 2) An audio-only variant, else the lowest bandwidth one.
        let audio_only = variants.iter().filter(|v| v.2.as_deref().is_some_and(|c| !c.contains("avc") && !c.contains("hvc") && !c.contains("hev")));
        let pick = audio_only.min_by_key(|v| v.0).or_else(|| variants.iter().min_by_key(|v| v.0)).ok_or_else(|| anyhow!("no variants"))?;
        let mut p = load(f, &pick.1)?;
        p.codecs = pick.2.clone();
        return Ok(p);
    }
    parse_media(&text, url)
}

fn text_base(url: &str) -> String {
    url.to_string()
}

pub fn parse_media(text: &str, url: &str) -> Result<Playlist> {
    let mut segments = Vec::new();
    let mut init = None;
    let mut key: Option<Key> = None;
    let mut dur = 0.0;
    let mut t = 0.0;
    let mut seq = 0u64;
    let mut range: Option<(u64, Option<u64>)> = None;
    let mut last_end: HashMap<String, u64> = HashMap::new();
    let parse_range = |v: &str| -> Result<(u64, Option<u64>)> {
        let v = v.trim().trim_matches('"');
        Ok(match v.split_once('@') {
            Some((n, o)) => (n.parse()?, Some(o.parse()?)),
            None => (v.parse()?, None),
        })
    };
    for line in text.lines().map(str::trim) {
        if let Some(r) = line.strip_prefix("#EXTINF:") {
            dur = r.split(',').next().unwrap_or("0").trim().parse().unwrap_or(0.0);
        } else if let Some(r) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            seq = r.trim().parse().unwrap_or(0);
        } else if let Some(r) = line.strip_prefix("#EXT-X-MAP:") {
            init = attr(r, "URI").map(|u| join(url, &u)).transpose()?;
            if let (Some(i), Some(br)) = (&init, attr(r, "BYTERANGE")) {
                let (n, o) = parse_range(&br)?;
                init = Some(format!("{i}#bytes={}-{n}", o.unwrap_or(0)));
            }
        } else if let Some(r) = line.strip_prefix("#EXT-X-KEY:") {
            let method = attr(r, "METHOD").unwrap_or_default();
            key = match method.as_str() {
                "NONE" => None,
                "AES-128" if attr(r, "KEYFORMAT").is_none_or(|k| k == "identity") => Some(Key {
                    url: join(url, &attr(r, "URI").ok_or_else(|| anyhow!("key without URI"))?)?,
                    iv: attr(r, "IV").map(|v| parse_iv(&v)).transpose()?,
                }),
                m => bail!("unsupported HLS encryption {m} (SAMPLE-AES or DRM)"),
            };
        } else if let Some(r) = line.strip_prefix("#EXT-X-BYTERANGE:") {
            range = Some(parse_range(r)?);
        } else if !line.is_empty() && !line.starts_with('#') {
            let mut seg_url = join(url, line)?;
            if let Some((n, o)) = range.take() {
                let off = o.unwrap_or_else(|| last_end.get(&seg_url).copied().unwrap_or(0));
                last_end.insert(seg_url.clone(), off + n);
                seg_url = format!("{seg_url}#bytes={off}-{n}");
            }
            segments.push(Segment { url: seg_url, start_s: t, dur_s: dur, key: key.clone(), seq });
            t += dur;
            seq += 1;
        }
    }
    if segments.is_empty() {
        bail!("playlist has no segments");
    }
    Ok(Playlist { url: url.to_string(), init, segments, codecs: None })
}

fn decrypt(data: Vec<u8>, key: &[u8; 16], iv: &[u8; 16]) -> io::Result<Vec<u8>> {
    use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
    let mut d = data;
    let n = cbc::Decryptor::<aes::Aes128>::new(key.into(), iv.into())
        .decrypt_padded_mut::<Pkcs7>(&mut d)
        .map_err(|_| io::Error::other("AES-128 decryption failed"))?
        .len();
    d.truncate(n);
    Ok(d)
}

/// Strips a leading ID3v2 tag (packed audio carries one per segment).
fn strip_id3(d: &mut Vec<u8>) {
    while d.len() >= 10 && &d[..3] == b"ID3" {
        let size = (d[6] as usize) << 21 | (d[7] as usize) << 14 | (d[8] as usize) << 7 | d[9] as usize;
        let footer = if d[5] & 0x10 != 0 { 10 } else { 0 };
        d.drain(..(10 + size + footer).min(d.len()));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    Fmp4,
    PackedAudio,
    Ts,
}

pub fn sniff(first: &[u8]) -> Container {
    if first.first() == Some(&0x47) && first.get(188) == Some(&0x47) {
        Container::Ts
    } else if first.get(4..8).is_some_and(|b| b == b"ftyp" || b == b"styp" || b == b"moof" || b == b"moov") {
        Container::Fmp4
    } else {
        Container::PackedAudio
    }
}

/// Splices segments from `start` onwards into one byte stream.
pub struct SegmentReader {
    f: Fetcher,
    pl: Playlist,
    next: usize,
    sent_init: bool,
    cur: Vec<u8>,
    off: usize,
    ts: Option<TsDemuxer>,
    live: Option<LiveSegment>,
    pub container: Option<Container>,
    pub fetch_time: Duration,
    pub first_ts_pts: Option<u64>,
}

impl SegmentReader {
    pub fn new(f: Fetcher, pl: Playlist, start: usize, init_cache: Option<Vec<u8>>) -> Self {
        let mut r = Self {
            f,
            pl,
            next: start,
            sent_init: false,
            cur: Vec::new(),
            off: 0,
            ts: None,
            live: None,
            container: None,
            fetch_time: Duration::ZERO,
            first_ts_pts: None,
        };
        if let Some(init) = init_cache {
            r.cur = init;
            r.sent_init = true;
        }
        r
    }

    fn key(&mut self, url: &str) -> io::Result<[u8; 16]> {
        if let Some(k) = self.f.keys.lock().unwrap().get(url) {
            return Ok(*k);
        }
        let b = self.f.bytes(url).map_err(io::Error::other)?;
        let k: [u8; 16] = b.as_slice().try_into().map_err(|_| io::Error::other("key is not 16 bytes"))?;
        self.f.keys.lock().unwrap().insert(url.to_string(), k);
        Ok(k)
    }

    fn advance(&mut self) -> io::Result<bool> {
        let t0 = Instant::now();
        if !self.sent_init {
            self.sent_init = true;
            if let Some(u) = self.pl.init.clone() {
                self.cur = self.f.bytes(&u).map_err(io::Error::other)?;
                self.off = 0;
                self.fetch_time += t0.elapsed();
                return Ok(true);
            }
        }
        if self.live.is_some() {
            return self.pump();
        }
        let Some(seg) = self.pl.segments.get(self.next).cloned() else { return Ok(false) };
        self.next += 1;
        if self.f.streaming {
            let body = self.f.stream(&seg.url).map_err(io::Error::other)?;
            let dec = match &seg.key {
                Some(k) => {
                    use aes::cipher::KeyIvInit;
                    let key = self.key(&k.url)?;
                    let iv = k.iv.unwrap_or_else(|| (seg.seq as u128).to_be_bytes());
                    Some(cbc::Decryptor::<aes::Aes128>::new(&key.into(), &iv.into()))
                }
                None => None,
            };
            self.live = Some(LiveSegment { body, dec, held: Vec::new(), first: true, head: Vec::new() });
            return self.pump();
        }
        let mut body = self.f.bytes(&seg.url).map_err(io::Error::other)?;
        if let Some(k) = &seg.key {
            let key = self.key(&k.url)?;
            let iv = k.iv.unwrap_or_else(|| (seg.seq as u128).to_be_bytes());
            body = decrypt(body, &key, &iv)?;
        }
        self.fetch_time += t0.elapsed();
        let c = *self.container.get_or_insert_with(|| sniff(&body));
        match c {
            Container::Ts => {
                let d = self.ts.get_or_insert_with(TsDemuxer::new);
                let mut es = Vec::with_capacity(body.len());
                d.push(&body, &mut es)?;
                d.flush(&mut es);
                if self.first_ts_pts.is_none() {
                    self.first_ts_pts = d.first_pts;
                }
                body = es;
            }
            Container::PackedAudio => strip_id3(&mut body),
            Container::Fmp4 => {}
        }
        self.cur = body;
        self.off = 0;
        Ok(true)
    }
}

/// A segment being decoded while it downloads.
struct LiveSegment {
    body: Box<dyn Read + Send + Sync>,
    dec: Option<cbc::Decryptor<aes::Aes128>>,
    /// Undecrypted tail (CBC works on 16-byte blocks; the last block holds padding).
    held: Vec<u8>,
    first: bool,
    /// Plaintext gathered until container sniffing and ID3 stripping can happen.
    head: Vec<u8>,
}

impl SegmentReader {
    /// Reads the next chunk of the live segment into `cur`. Returns false only
    /// at the end of the last segment.
    fn pump(&mut self) -> io::Result<bool> {
        use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut};
        loop {
            let live = self.live.as_mut().unwrap();
            let mut chunk = vec![0u8; 16 * 1024];
            let n = live.body.read(&mut chunk)?;
            chunk.truncate(n);
            let eof = n == 0;
            let mut plain = if let Some(dec) = live.dec.as_mut() {
                live.held.extend_from_slice(&chunk);
                if eof {
                    let mut last = std::mem::take(&mut live.held);
                    let len = dec
                        .clone()
                        .decrypt_padded_mut::<Pkcs7>(&mut last)
                        .map_err(|_| io::Error::other("AES-128 decryption failed"))?
                        .len();
                    last.truncate(len);
                    last
                } else {
                    // Keep at least one whole block back: the last one carries the padding.
                    let whole = (live.held.len().saturating_sub(1) / 16) * 16;
                    let mut ready: Vec<u8> = live.held.drain(..whole).collect();
                    for b in ready.chunks_exact_mut(16) {
                        dec.decrypt_block_mut(b.into());
                    }
                    ready
                }
            } else {
                chunk
            };
            if live.first {
                live.head.append(&mut plain);
                if live.head.len() < 400 && !eof {
                    continue;
                }
                live.first = false;
                plain = std::mem::take(&mut live.head);
                let c = *self.container.get_or_insert_with(|| sniff(&plain));
                if c == Container::PackedAudio {
                    strip_id3(&mut plain);
                }
            }
            let out = match self.container {
                Some(Container::Ts) => {
                    let d = self.ts.get_or_insert_with(TsDemuxer::new);
                    let mut es = Vec::new();
                    d.push(&plain, &mut es)?;
                    if eof {
                        d.flush(&mut es);
                    }
                    es
                }
                _ => plain,
            };
            if eof {
                self.live = None;
            }
            if !out.is_empty() {
                self.cur = out;
                self.off = 0;
                return Ok(true);
            }
            if eof {
                return self.advance();
            }
        }
    }
}

impl Read for SegmentReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.off >= self.cur.len() {
            if !self.advance()? {
                return Ok(0);
            }
        }
        let n = buf.len().min(self.cur.len() - self.off);
        buf[..n].copy_from_slice(&self.cur[self.off..self.off + n]);
        self.off += n;
        Ok(n)
    }
}

pub struct SeekResult {
    /// Seek call to the first decoded frame at the target, wall clock.
    pub latency: Duration,
    /// Network share of it (playlist already loaded; segment + key + init).
    pub fetch: Duration,
    pub container: Container,
    pub decoder: &'static str,
    pub requests: u64,
    pub discarded_frames: u64,
    pub first_frame_s: f64,
    /// Mono audio from the target onwards, for checking against the source.
    pub audio: Vec<f32>,
    pub rate: u32,
}

/// Opens the stream at `t` seconds: picks the segment, fetches it, probes,
/// decodes and discards up to the exact target. Returns when the first frame
/// at the target is decoded. Opening at 0 is "start playback".
pub fn open_at(reg: &DecoderRegistry, f: &Fetcher, pl: &Playlist, init_cache: Option<Vec<u8>>, t: f64) -> Result<(SeekResult, Box<dyn FormatReader>)> {
    let started = Instant::now();
    let req0 = f.requests.load(std::sync::atomic::Ordering::Relaxed);
    let ns0 = f.fetch_ns.load(std::sync::atomic::Ordering::Relaxed);
    let idx = pl.index_at(t);
    let seg_start = pl.segments[idx].start_s;
    let mut reader = SegmentReader::new(f.clone(), pl.clone(), idx, init_cache);
    // Prime the first segment so the container is known for the probe hint.
    let mut peek = [0u8; 1];
    let n = reader.read(&mut peek)?;
    reader.off -= n;
    if reader.pl.init.is_some() && reader.container.is_none() {
        reader.container = Some(Container::Fmp4);
    }
    let container = reader.container.unwrap_or(Container::PackedAudio);
    let ext = match (container, reader.ts.as_ref().and_then(|d| d.kind)) {
        (Container::Fmp4, _) => "mp4",
        (_, Some(crate::ts::EsKind::MpegAudio)) => "mp3",
        _ => "aac",
    };
    let mut format = crate::decode::open(Box::new(ReadOnlySource::new(reader)), Some(ext))?;
    let (track_id, mut stream) = crate::decode::audio_stream(format.as_ref(), pl.codecs.clone())?;
    stream.profile_hint = pl.codecs.clone();
    let (mut dec, name, _) = reg.create(&StreamInfo { ..stream })?;
    let mut pcm = Vec::new();
    let mut discarded = 0u64;
    let first_frame_s;
    // Symphonia numbers packets from 0 for every container here (it ignores
    // fMP4 tfdt, and ADTS/TS carry no timestamps it reads), so the position is
    // the playlist's segment start plus decoded frames.
    let mut after: Vec<f32> = Vec::new();
    let mut rate_ch = (0u32, 0usize);
    loop {
        let p = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => bail!("stream ended before the seek target"),
            Err(SymError::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => bail!("eof before target"),
            Err(e) => return Err(e.into()),
        };
        if p.track_id != track_id {
            continue;
        }
        pcm.clear();
        dec.decode(&p, &mut pcm)?;
        let (rate, ch) = dec.output();
        rate_ch = (rate, ch);
        let frames = pcm.len() / ch.max(1);
        let pkt_t = seg_start + discarded as f64 / rate as f64;
        if pkt_t + frames as f64 / rate as f64 > t {
            let skip = ((t - pkt_t).max(0.0) * rate as f64) as u64;
            discarded += skip;
            first_frame_s = pkt_t + skip as f64 / rate as f64;
            after.extend(pcm[skip as usize * ch..].chunks_exact(ch).map(|f| f.iter().sum::<f32>() / ch as f32));
            break;
        }
        discarded += frames as u64;
    }
    let latency = started.elapsed();
    // Not part of the latency: collect a little more audio for the accuracy check.
    while after.len() < 8192 {
        let Ok(Some(p)) = format.next_packet() else { break };
        if p.track_id != track_id { continue; }
        pcm.clear();
        dec.decode(&p, &mut pcm)?;
        let ch = rate_ch.1;
        after.extend(pcm.chunks_exact(ch).map(|f| f.iter().sum::<f32>() / ch as f32));
    }
    Ok((
        SeekResult {
            latency,
            fetch: Duration::from_nanos(f.fetch_ns.load(std::sync::atomic::Ordering::Relaxed) - ns0),
            container,
            decoder: name,
            requests: f.requests.load(std::sync::atomic::Ordering::Relaxed) - req0,
            discarded_frames: discarded,
            first_frame_s,
            audio: after,
            rate: rate_ch.0,
        },
        format,
    ))
}
