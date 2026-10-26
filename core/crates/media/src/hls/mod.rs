//! HLS (ADR 0004 amendment 5): variant choice, segments decoded while they
//! download, byte-range seeks into fMP4 segments, `EXT-X-BYTERANGE`, and
//! AES-128 including `EXT-X-MAP` init sections. Containers: fMP4 (the
//! selected track's samples only), MPEG-TS, and packed ADTS or MPEG audio.
//!
//! A loader task on the core's runtime fetches and decrypts ahead; the
//! decode thread pulls from it without blocking, so a slow network shows up
//! as [`Read::Pending`](crate::Read::Pending), never as a stalled thread.
//!
//! Positions follow the playlist (`EXTINF` sums): Symphonia ignored fMP4
//! `tfdt` and TS timestamps start anywhere, so each segment's packets are
//! placed at its playlist start. An fMP4 init section's edit list sets the
//! encoder delay; TS and packed audio carry none.

mod crypto;
mod fmp4;
mod loader;
mod packed;
mod playlist;

use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::Arc;

use tokio::runtime::Handle;
use tokio::sync::mpsc::{self, error::TryRecvError};
use tokio::task::JoinHandle;
use url::Url;

use self::fmp4::{SegmentParser, TrackInfo};
use self::loader::{Message, Plan, RangedSeek, Resume, Shared};
use self::playlist::{MediaPlaylist, Playlist};
use crate::codec::CodecDescription;
use crate::demux::es::{EsKind, Framer};
use crate::demux::{
    Demuxed, DemuxedPacket, Demuxer, MediaHint, Seeked, SymphoniaDemuxer, TimeBase, TsDemuxer,
    Window,
};
use crate::stream::DecodedStream;
use crate::{DecoderRegistry, Error, Fetch, MediaStream, Read, StreamFormat};

/// An HLS Stream Source (WIT `hls`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HlsSource {
    /// The master or media playlist.
    pub url: String,
    /// Sent only to the playlist's own origin (WIT `hls.headers`).
    pub headers: Vec<(String, String)>,
    /// The Plugin's MIME hint; its `codecs` parameter routes HE-AAC when no
    /// master playlist names the codec.
    pub hint: MediaHint,
}

/// A [`MediaStream`] over an HLS playlist.
pub struct HlsStream {
    inner: DecodedStream<HlsDemuxer>,
    live: bool,
    duration_ms: Option<u64>,
}

impl std::fmt::Debug for HlsStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HlsStream")
            .field("live", &self.live)
            .field("duration_ms", &self.duration_ms)
            .finish_non_exhaustive()
    }
}

impl HlsStream {
    /// Loads the playlist, chooses a variant, and fetches until the first
    /// audio frame, so the stream's format is known. Fetching continues in
    /// the background on `runtime`.
    pub async fn open(
        source: HlsSource,
        fetch: Arc<dyn Fetch>,
        runtime: Handle,
        registry: &DecoderRegistry,
    ) -> Result<Self, Error> {
        let url = Url::parse(&source.url)
            .map_err(|error| Error::malformed("HLS URL", format!("{}: {error}", source.url)))?;
        let shared = Arc::new(Shared::new(fetch, &url, source.headers.clone()));
        let (media, codecs) = match shared.playlist(&url).await? {
            Playlist::Media(media) => (media, source.hint.codecs()),
            Playlist::Master(master) => {
                let choice = playlist::choose(&master)?;
                tracing::debug!(url = %crate::fetch::redact_url(choice.url.as_str()), codecs = ?choice.codecs, "chose an HLS variant");
                match shared.playlist(&choice.url).await? {
                    Playlist::Media(media) => (media, choice.codecs.or(source.hint.codecs())),
                    Playlist::Master(_) => {
                        return Err(Error::malformed(
                            "HLS playlist",
                            "a variant is a master playlist",
                        ));
                    }
                }
            }
        };
        let live = !media.ended;
        // Initialize from the segment playback starts at: a live window may
        // begin with segments of another container or init section.
        let start = if live { loader::live_start(&media) } else { 0 };
        let first = media
            .segments
            .get(start)
            .ok_or_else(|| Error::malformed("HLS playlist", "no segments"))?;
        let fmp4 = match &first.map {
            Some(map) => Some(Fmp4::from_init(shared.init(map).await?)?),
            None => None,
        };
        let duration_ms = (!live).then(|| media.duration_us() / 1000);
        let playlist = Arc::new(media);
        let plan = if live {
            Plan::Live {
                playlist: (*playlist).clone(),
            }
        } else {
            Plan::Vod {
                playlist: playlist.clone(),
                start: 0,
                seek: None,
            }
        };
        let mut demuxer = HlsDemuxer::new(shared, runtime, playlist, live, fmp4, codecs, plan);
        demuxer.wait_for_first_packet().await?;
        Ok(Self {
            inner: DecodedStream::open(demuxer, registry)?,
            live,
            duration_ms,
        })
    }

    /// Whether the playlist is live (no `EXT-X-ENDLIST`): it cannot seek or
    /// end on its own.
    pub fn is_live(&self) -> bool {
        self.live
    }

    /// The playlist's duration, for a playlist with an end.
    pub fn duration_ms(&self) -> Option<u64> {
        self.duration_ms
    }

    /// The name of the decoder provider in use.
    pub fn decoder(&self) -> &'static str {
        self.inner.decoder_name()
    }
}

impl MediaStream for HlsStream {
    fn format(&self) -> StreamFormat {
        self.inner.format()
    }

    fn read(&mut self, out: &mut [f32]) -> Result<Read, Error> {
        self.inner.read(out)
    }

    fn seek(&mut self, position_ms: u64) -> Result<(), Error> {
        if self.live {
            return Err(Error::NotSeekable);
        }
        self.inner.seek(position_ms)
    }
}

/// An fMP4 rendition: its init section and what it says.
struct Fmp4 {
    init: Arc<Vec<u8>>,
    track: TrackInfo,
    codec: CodecDescription,
    window: Option<Window>,
}

impl Fmp4 {
    fn from_init(init: Arc<Vec<u8>>) -> Result<Self, Error> {
        // Symphonia reads the codec configuration (esds, dOps, dfLa) and
        // picks the audio track; the host reads the rest.
        let symphonia =
            SymphoniaDemuxer::open(Cursor::new(init.to_vec()), &MediaHint::extension("mp4"))?;
        let track_id = symphonia.track_id();
        let moov = crate::demux::mp4::read_moov(&mut Cursor::new(init.as_slice()))?
            .ok_or_else(|| Error::malformed("fMP4 init section", "no moov"))?;
        let track = TrackInfo::from_moov(&moov, track_id)?;
        Ok(Self {
            codec: symphonia.codec().clone(),
            window: symphonia.window(),
            track,
            init,
        })
    }
}

/// The parser for the playlist's container.
enum Parser {
    /// Waiting for the first bytes to tell TS from packed audio.
    Unknown {
        head: Vec<u8>,
    },
    Fmp4(SegmentParser),
    Ts(Box<TsDemuxer>),
    Packed {
        framer: Framer,
        head: Option<Vec<u8>>,
    },
}

/// Where the current segment sits on the stream's timeline.
struct Placement {
    /// Segment start on the stream's timeline.
    start_us: u64,
    /// Container time of the segment's first frame.
    origin: Option<i64>,
}

pub(crate) struct HlsDemuxer {
    shared: Arc<Shared>,
    runtime: Handle,
    playlist: Arc<MediaPlaylist>,
    live: bool,
    fmp4: Option<Fmp4>,
    profile_hint: Option<String>,
    codec: CodecDescription,
    /// Packets are stamped in this time base whatever their container: the
    /// stream's decoder and timeline were set up with it.
    time_base: TimeBase,
    /// TS and packed audio learn the time base from their first frame.
    time_base_known: bool,
    /// A container switch happened: the new container's codec must match
    /// the decoder before its packets are used.
    recheck: bool,
    /// A TS init section (PAT and PMT), pushed into each new TS parser.
    ts_init: Option<Arc<Vec<u8>>>,
    rx: mpsc::Receiver<Message>,
    task: JoinHandle<()>,
    parser: Parser,
    placement: Option<Placement>,
    ready: VecDeque<DemuxedPacket>,
    ended: bool,
    failed: Option<Error>,
}

impl Drop for HlsDemuxer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl HlsDemuxer {
    fn new(
        shared: Arc<Shared>,
        runtime: Handle,
        playlist: Arc<MediaPlaylist>,
        live: bool,
        fmp4: Option<Fmp4>,
        profile_hint: Option<String>,
        plan: Plan,
    ) -> Self {
        let (rx, task) = spawn(&runtime, &shared, plan);
        let time_base_known = fmp4.is_some();
        let (codec, time_base, parser) = match &fmp4 {
            Some(fmp4) => (
                fmp4.codec.clone(),
                TimeBase {
                    numer: 1,
                    denom: fmp4.track.timescale,
                },
                Parser::Fmp4(SegmentParser::new(fmp4.track.clone())),
            ),
            // Known from the first frame.
            None => (
                CodecDescription::new(crate::Codec::Other),
                TimeBase { numer: 1, denom: 1 },
                Parser::Unknown { head: Vec::new() },
            ),
        };
        let mut demuxer = Self {
            shared,
            runtime,
            playlist,
            live,
            fmp4,
            profile_hint,
            codec,
            time_base_known,
            time_base,
            recheck: false,
            ts_init: None,
            rx,
            task,
            parser,
            placement: None,
            ready: VecDeque::new(),
            ended: false,
            failed: None,
        };
        demuxer.codec.profile_hint = demuxer.profile_hint.clone();
        demuxer
    }

    /// Receives until the first packet, which fixes the codec for TS and
    /// packed audio.
    async fn wait_for_first_packet(&mut self) -> Result<(), Error> {
        while self.ready.is_empty() {
            if let Some(error) = self.failed.take() {
                return Err(error);
            }
            if self.ended {
                return Err(Error::NoAudioTrack);
            }
            match self.rx.recv().await {
                Some(message) => self.handle(message)?,
                None => return Err(Error::NoAudioTrack),
            }
        }
        if self.fmp4.is_none() {
            let codec = match &self.parser {
                Parser::Ts(ts) => ts.codec(),
                Parser::Packed { framer, .. } => framer.codec(),
                _ => None,
            };
            let mut codec = codec.cloned().ok_or(Error::NoAudioTrack)?;
            let rate = codec.sample_rate.ok_or(Error::NoAudioTrack)?;
            codec.profile_hint = self.profile_hint.clone();
            self.codec = codec;
            debug_assert_eq!(self.time_base.denom, rate);
        }
        Ok(())
    }

    fn handle(&mut self, message: Message) -> Result<(), Error> {
        match message {
            Message::Init(init) => self.on_init(init)?,
            Message::Segment {
                start_us,
                discontinuity,
                resume,
            } => self.on_segment(start_us, discontinuity, resume),
            Message::Data(bytes) => self.on_data(&bytes)?,
            Message::SegmentEnd => self.on_segment_end()?,
            Message::End => {
                if let Parser::Ts(ts) = &mut self.parser {
                    ts.finish()?;
                }
                self.drain()?;
                self.ended = true;
            }
            Message::Failed(error) => self.failed = Some(error),
        }
        Ok(())
    }

    /// A new init section. An fMP4 one replaces the configuration the fMP4
    /// parser uses (it applies across discontinuities until the next
    /// `EXT-X-MAP`); a TS one (PAT and PMT) is fed to each TS parser first.
    fn on_init(&mut self, init: Arc<Vec<u8>>) -> Result<(), Error> {
        if TsDemuxer::sniff(&init) {
            self.ts_init = Some(init);
            return Ok(());
        }
        if self
            .fmp4
            .as_ref()
            .is_some_and(|current| current.init == init)
        {
            return Ok(());
        }
        let next = Fmp4::from_init(init)?;
        // The decoder was opened from the first configuration and is not
        // reopened: a new codec after a discontinuity (an ad in another
        // encoding) is not followed.
        if self.codec.codec != crate::Codec::Other && !compatible(&self.codec, &next.codec) {
            return Err(Error::UnsupportedCodec(format!(
                "a codec configuration change mid-stream ({:?} to {:?})",
                self.codec.codec, next.codec.codec
            )));
        }
        if matches!(self.parser, Parser::Fmp4(_)) {
            self.parser = Parser::Fmp4(SegmentParser::new(next.track.clone()));
        }
        self.fmp4 = Some(next);
        Ok(())
    }

    fn on_segment(&mut self, start_us: u64, discontinuity: bool, resume: Option<Resume>) {
        let mut origin = None;
        if let Some(resume) = resume {
            // A byte-range seek, which only fMP4 segments get.
            if let Some(fmp4) = &self.fmp4 {
                origin = Some(i64::try_from(resume.origin).unwrap_or(0));
                self.parser = Parser::Fmp4(SegmentParser::resume(
                    fmp4.track.clone(),
                    resume.samples,
                    resume.mdat_end,
                ));
            }
        } else if discontinuity {
            // The container may change at a discontinuity (fMP4, TS and
            // packed audio can follow each other): tell it from the bytes.
            self.resniff();
        } else {
            match &mut self.parser {
                Parser::Fmp4(parser) => {
                    if let Some(fmp4) = &self.fmp4 {
                        *parser = SegmentParser::new(fmp4.track.clone());
                    }
                }
                Parser::Ts(_) | Parser::Unknown { .. } => {}
                // Every packed segment starts with its own ID3 tag.
                Parser::Packed { head, .. } => *head = Some(Vec::new()),
            }
        }
        self.placement = Some(Placement { start_us, origin });
    }

    /// Forgets the container: the next bytes decide it. Packets of the new
    /// container must then match the open decoder.
    fn resniff(&mut self) {
        self.parser = Parser::Unknown { head: Vec::new() };
        self.recheck = self.codec.codec != crate::Codec::Other;
    }

    fn on_data(&mut self, data: &[u8]) -> Result<(), Error> {
        match &mut self.parser {
            Parser::Unknown { head } => {
                head.extend_from_slice(data);
                let head = std::mem::take(head);
                match sniff(&head) {
                    Sniffed::NeedMore => self.parser = Parser::Unknown { head },
                    Sniffed::Fmp4 => {
                        let Some(fmp4) = &self.fmp4 else {
                            return Err(Error::malformed(
                                "HLS playlist",
                                "an fMP4 segment without an EXT-X-MAP",
                            ));
                        };
                        let mut parser = SegmentParser::new(fmp4.track.clone());
                        parser.push(&head)?;
                        self.parser = Parser::Fmp4(parser);
                    }
                    Sniffed::Ts => {
                        let mut ts = TsDemuxer::new();
                        if let Some(init) = &self.ts_init {
                            ts.push(init)?;
                        }
                        ts.push(&head)?;
                        self.parser = Parser::Ts(Box::new(ts));
                    }
                    Sniffed::Packed(kind) => {
                        self.parser = Parser::Packed {
                            framer: Framer::new(kind),
                            head: Some(Vec::new()),
                        };
                        return self.on_data(&head);
                    }
                    Sniffed::Unknown => {
                        return Err(Error::UnrecognizedContainer {
                            hint: Some("HLS segment".into()),
                        });
                    }
                }
            }
            Parser::Fmp4(parser) => parser.push(data)?,
            Parser::Ts(ts) => ts.push(data)?,
            Parser::Packed { framer, head } => match head {
                None => framer.push(data, None),
                Some(buffered) => {
                    buffered.extend_from_slice(data);
                    match packed::read_head(buffered) {
                        packed::Head::NeedMore => {}
                        packed::Head::Untagged => {
                            framer.push(buffered, None);
                            *head = None;
                        }
                        packed::Head::Tagged { len, timestamp } => {
                            framer.push(&buffered[len..], timestamp);
                            *head = None;
                        }
                    }
                }
            },
        }
        self.drain()
    }

    fn on_segment_end(&mut self) -> Result<(), Error> {
        match &mut self.parser {
            Parser::Fmp4(parser) => parser.finish()?,
            // Segments need not end on PES or frame boundaries (RFC 8216,
            // section 3): a partial frame waits for the next segment.
            Parser::Ts(_) => {}
            Parser::Packed { framer, head } => {
                if let Some(buffered) = head.take() {
                    // A segment shorter than its tag would be: all audio.
                    framer.push(&buffered, None);
                }
                self.ready_from_framer_end()?;
            }
            Parser::Unknown { head } if !head.is_empty() => {
                let head = std::mem::take(head);
                self.parser = Parser::Unknown { head: Vec::new() };
                return Err(Error::UnrecognizedContainer {
                    hint: Some(format!("HLS segment of {} bytes", head.len())),
                });
            }
            Parser::Unknown { .. } => {}
        }
        self.drain()
    }

    fn ready_from_framer_end(&mut self) -> Result<(), Error> {
        loop {
            let (packet, codec) = match &mut self.parser {
                Parser::Packed { framer, .. } => {
                    (framer.next_packet(true)?, framer.codec().cloned())
                }
                _ => return Ok(()),
            };
            let Some(packet) = packet else { return Ok(()) };
            self.place(packet, codec.as_ref())?;
        }
    }

    /// Moves parsed packets to `ready`, placed on the stream's timeline.
    fn drain(&mut self) -> Result<(), Error> {
        loop {
            // Ticks per second of the packets' clock: the fMP4 timescale, or
            // the sample rate for framed audio.
            let (packet, codec) = match &mut self.parser {
                Parser::Fmp4(parser) => (parser.next_packet(), None),
                Parser::Ts(ts) => (ts.next_packet()?, ts.codec().cloned()),
                Parser::Packed { framer, .. } => {
                    (framer.next_packet(false)?, framer.codec().cloned())
                }
                Parser::Unknown { .. } => (None, None),
            };
            let Some(packet) = packet else { return Ok(()) };
            self.place(packet, codec.as_ref())?;
        }
    }

    /// Checks a framed container's codec after a switch, then stamps the
    /// packet. `codec` is the framer's, `None` for fMP4.
    fn place(
        &mut self,
        packet: DemuxedPacket,
        codec: Option<&CodecDescription>,
    ) -> Result<(), Error> {
        // Ticks per second of the packet's clock: the fMP4 timescale, or the
        // sample rate for framed audio.
        let clock = match codec {
            Some(codec) => codec.sample_rate,
            None => self.fmp4.as_ref().map(|f| f.track.timescale),
        }
        .unwrap_or(1);
        if !self.time_base_known {
            self.time_base = TimeBase {
                numer: 1,
                denom: clock,
            };
            self.time_base_known = true;
        }
        if self.recheck
            && let Some(codec) = codec
        {
            if !compatible(&self.codec, codec) {
                return Err(Error::UnsupportedCodec(format!(
                    "a codec configuration change mid-stream ({:?} to {:?})",
                    self.codec.codec, codec.codec
                )));
            }
            self.recheck = false;
        }
        stamp(
            &mut self.placement,
            &mut self.ready,
            packet,
            clock,
            self.time_base.denom,
        );
        Ok(())
    }

    fn restart(&mut self, plan: Plan) {
        self.task.abort();
        let (rx, task) = spawn(&self.runtime, &self.shared, plan);
        self.rx = rx;
        self.task = task;
        self.ready.clear();
        self.placement = None;
        self.ended = false;
        self.failed = None;
        // The target segment may be in another container than the current
        // one; a byte-range seek sets fMP4 itself.
        self.resniff();
    }
}

fn spawn(
    runtime: &Handle,
    shared: &Arc<Shared>,
    plan: Plan,
) -> (mpsc::Receiver<Message>, JoinHandle<()>) {
    let (tx, rx) = mpsc::channel(loader::CHANNEL_CAPACITY);
    let task = runtime.spawn(loader::run(shared.clone(), plan, tx));
    (rx, task)
}

/// Places a packet at its segment's start plus its offset within the
/// segment, converted from the container's clock to the stream's time base
/// (`denom` ticks per second).
fn stamp(
    placement: &mut Option<Placement>,
    ready: &mut VecDeque<DemuxedPacket>,
    mut packet: DemuxedPacket,
    clock: u32,
    denom: u32,
) {
    let (clock, denom) = (i128::from(clock.max(1)), i128::from(denom.max(1)));
    let convert = |ticks: i128| (ticks * denom + clock / 2).div_euclid(clock);
    if let Some(placement) = placement {
        let origin = *placement.origin.get_or_insert(packet.pts);
        // To the nearest tick: EXTINF is rounded to microseconds, so 45056
        // frames arrive as 45055.998.
        let start = (i128::from(placement.start_us) * denom + 500_000) / 1_000_000;
        let offset = convert(i128::from(packet.pts) - i128::from(origin));
        packet.pts = i64::try_from(start + offset).unwrap_or(i64::MAX);
    }
    packet.dur = u64::try_from(convert(i128::from(packet.dur))).unwrap_or(u64::MAX);
    ready.push_back(packet);
}

/// Whether a decoder opened for `open` can decode `next`: the same codec,
/// rate and layout, and the same configuration. For AAC that is the
/// AudioSpecificConfig's object type, rate and channel configuration (its
/// first two bytes), so an ADTS-derived configuration matches an fMP4 one
/// that only adds extension signalling.
fn compatible(open: &CodecDescription, next: &CodecDescription) -> bool {
    if open.codec != next.codec
        || open.sample_rate != next.sample_rate
        || open.channels != next.channels
    {
        return false;
    }
    match (
        open.codec,
        open.extra_data.as_deref(),
        next.extra_data.as_deref(),
    ) {
        (crate::Codec::Aac, Some(a), Some(b)) => a.get(..2) == b.get(..2),
        (_, a, b) => a == b,
    }
}

enum Sniffed {
    NeedMore,
    Fmp4,
    Ts,
    Packed(EsKind),
    Unknown,
}

fn sniff(head: &[u8]) -> Sniffed {
    if let Some(kind) = head.get(4..8) {
        if matches!(
            kind,
            b"styp" | b"sidx" | b"moof" | b"prft" | b"emsg" | b"ftyp" | b"free"
        ) {
            return Sniffed::Fmp4;
        }
    } else if head.first() != Some(&0x47)
        && !head.starts_with(b"ID3")
        && head.first() != Some(&0xff)
    {
        return Sniffed::NeedMore;
    }
    if head.first() == Some(&0x47) {
        return if head.len() > 188 {
            if head[188] == 0x47 {
                Sniffed::Ts
            } else {
                Sniffed::Unknown
            }
        } else {
            Sniffed::NeedMore
        };
    }
    let audio = match packed::read_head(head) {
        packed::Head::NeedMore => return Sniffed::NeedMore,
        packed::Head::Untagged => head,
        packed::Head::Tagged { len, .. } => &head[len..],
    };
    if audio.len() < 2 {
        return Sniffed::NeedMore;
    }
    packed::sniff_audio(audio).map_or(Sniffed::Unknown, Sniffed::Packed)
}

impl Demuxer for HlsDemuxer {
    fn codec(&self) -> &CodecDescription {
        &self.codec
    }

    fn time_base(&self) -> TimeBase {
        self.time_base
    }

    fn window(&self) -> Option<Window> {
        self.fmp4.as_ref().and_then(|f| f.window)
    }

    fn next_packet(&mut self) -> Result<Demuxed, Error> {
        loop {
            if let Some(packet) = self.ready.pop_front() {
                return Ok(Demuxed::Packet(packet));
            }
            if let Some(error) = self.failed.take() {
                return Err(error);
            }
            if self.ended {
                return Ok(Demuxed::End);
            }
            match self.rx.try_recv() {
                Ok(message) => self.handle(message)?,
                Err(TryRecvError::Empty) => return Ok(Demuxed::Pending),
                Err(TryRecvError::Disconnected) => {
                    return Err(Error::malformed("HLS", "the loader stopped without ending"));
                }
            }
        }
    }

    fn seek(&mut self, ts: i64) -> Result<Seeked, Error> {
        if self.live {
            return Err(Error::NotSeekable);
        }
        let tb = self.time_base;
        let us = u64::try_from(
            i128::from(ts.max(0)) * i128::from(tb.numer) * 1_000_000 / i128::from(tb.denom),
        )
        .unwrap_or(u64::MAX);
        if us >= self.playlist.duration_us() {
            self.restart(Plan::Vod {
                playlist: self.playlist.clone(),
                start: self.playlist.segments.len(),
                seek: None,
            });
            return Ok(Seeked::PastEnd);
        }
        let (index, _) = self.playlist.segment_index_at(us / 1000);
        let segment_start = self.playlist.segments[index].start_us;
        // The ranged seek reads the target's moof with the current init's
        // track defaults, so it is used only when one init serves them all.
        let one_map = self
            .playlist
            .segments
            .iter()
            .all(|s| s.map == self.playlist.segments[0].map);
        let seek = self
            .fmp4
            .as_ref()
            .filter(|_| one_map)
            .map(|fmp4| RangedSeek {
                offset_us: us - segment_start,
                track: fmp4.track.clone(),
            });
        self.restart(Plan::Vod {
            playlist: self.playlist.clone(),
            start: index,
            seek,
        });
        Ok(Seeked::At)
    }
}
