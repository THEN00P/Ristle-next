//! Spike B player core: decode thread → SPSC ring → cpal (AAudio on Android).
//!
//! The core owns the queue, position and transport. Platform code (the Media3
//! facade) only mirrors `PlayerState` and forwards commands. Album tracks are
//! decoded back to back into one ring, so transitions are gapless as long as
//! the tracks share a sample rate (the production host resamples to one
//! output rate instead).

use audiolab::{cputime::thread_cpu, decode, mp4edit, registry};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::{Consumer, Producer, RingBuffer};
use std::fs::File;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::*};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::{FormatReader, SeekMode, SeekTo};
use symphonia::core::units::Time;

uniffi::setup_scaffolding!();

#[cfg(target_os = "android")]
mod android;

const RING_SECONDS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Status {
    Buffering,
    Playing,
    Paused,
    Error,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PlayerState {
    pub status: Status,
    pub track_index: u32,
    pub track_count: u32,
    pub title: String,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PlayerStats {
    pub uptime_ms: u64,
    pub frames_played: u64,
    pub callbacks: u64,
    /// Callbacks that found the ring short while playing (our own underrun).
    pub starved_callbacks: u64,
    /// AAudio xruns reported by the stream.
    pub xruns: u64,
    pub max_callback_us: u64,
    pub transitions: u64,
    /// Thread CPU time of the decode thread and of the output callback thread.
    pub decode_cpu_ms: u64,
    pub callback_cpu_ms: u64,
    pub sample_rate: u32,
    pub output_buffer_frames: i64,
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum PlayerError {
    #[error("{msg}")]
    Failed { msg: String },
}

impl From<anyhow::Error> for PlayerError {
    fn from(e: anyhow::Error) -> Self {
        PlayerError::Failed { msg: format!("{e:#}") }
    }
}

#[uniffi::export(with_foreign)]
pub trait StateObserver: Send + Sync {
    fn on_state(&self, state: PlayerState);
}

/// Where the timeline (frames handed to the ring since the last flush) maps
/// into the queue.
#[derive(Clone, Copy)]
struct Boundary {
    generation: u64,
    timeline_start: u64,
    track: usize,
    track_start_frame: u64,
}

struct Shared {
    playing: AtomicBool,
    generation: AtomicU64,
    /// Frames consumed by the callback in the current generation.
    consumed: AtomicU64,
    frames_played: AtomicU64,
    callbacks: AtomicU64,
    starved: AtomicU64,
    xruns: AtomicU64,
    max_cb_ns: AtomicU64,
    decode_cpu_ns: AtomicU64,
    callback_cpu_ns: AtomicU64,
    boundaries: Mutex<Vec<Boundary>>,
    durations: Mutex<Vec<u64>>,
    error: Mutex<Option<String>>,
    buffering: AtomicBool,
    started: Instant,
}

enum Cmd {
    Seek { track: usize, ms: u64 },
    Shutdown,
}

#[derive(uniffi::Object)]
pub struct Player {
    shared: Arc<Shared>,
    titles: Vec<String>,
    rate: u32,
    cmd: Sender<Cmd>,
    observer: Arc<Mutex<Option<Arc<dyn StateObserver>>>>,
    notify: Sender<()>,
    stream: Mutex<Option<cpal::Stream>>,
}

// cpal::Stream is !Send on some hosts; it is only touched under the mutex.
unsafe impl Send for Player {}
unsafe impl Sync for Player {}

#[uniffi::export]
impl Player {
    /// `paths` are local files played in order (looping when `repeat`). All
    /// must share one sample rate in this spike.
    #[uniffi::constructor]
    pub fn new(paths: Vec<String>, titles: Vec<String>, repeat: bool) -> Result<Arc<Self>, PlayerError> {
        #[cfg(target_os = "android")]
        android_logger::init_once(android_logger::Config::default().with_tag("ristle-audio").with_max_level(log::LevelFilter::Info));
        let reg = registry::default_registry();
        let first = open_track(&reg, &paths[0], 0)?;
        let rate = first.rate;
        let channels = 2usize;
        let shared = Arc::new(Shared {
            playing: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            consumed: AtomicU64::new(0),
            frames_played: AtomicU64::new(0),
            callbacks: AtomicU64::new(0),
            starved: AtomicU64::new(0),
            xruns: AtomicU64::new(0),
            max_cb_ns: AtomicU64::new(0),
            decode_cpu_ns: AtomicU64::new(0),
            callback_cpu_ns: AtomicU64::new(0),
            boundaries: Mutex::new(vec![Boundary { generation: 0, timeline_start: 0, track: 0, track_start_frame: 0 }]),
            durations: Mutex::new(vec![0; paths.len()]),
            error: Mutex::new(None),
            buffering: AtomicBool::new(true),
            started: Instant::now(),
        });

        // Ring handoff: a seek builds a fresh ring and passes its consumer to
        // the callback through this one-slot queue, so the RT side never locks.
        let (producer, consumer) = RingBuffer::<f32>::new(rate as usize * channels * RING_SECONDS);
        let (mut handoff_tx, handoff_rx) = RingBuffer::<(u64, Consumer<f32>)>::new(2);
        let stream = build_stream(shared.clone(), consumer, handoff_rx, rate, channels)?;

        let (cmd, cmd_rx) = channel();
        let (notify, notify_rx) = channel();
        let observer: Arc<Mutex<Option<Arc<dyn StateObserver>>>> = Arc::new(Mutex::new(None));
        let player = Arc::new(Player {
            shared: shared.clone(),
            titles,
            rate,
            cmd,
            observer: observer.clone(),
            notify,
            stream: Mutex::new(Some(stream)),
        });
        {
            let shared = shared.clone();
            let paths = paths.clone();
            std::thread::Builder::new()
                .name("ristle-decode".into())
                .spawn(move || {
                    let mut d = DecodeLoop { reg, paths, repeat, shared, producer, track: 0, cur: Some(first), timeline: 0, generation: 0, rate, pending: Vec::new() };
                    d.run(cmd_rx, &mut handoff_tx);
                })
                .map_err(|e| PlayerError::Failed { msg: e.to_string() })?;
        }
        {
            // Observer notifications always come from this core thread, never
            // from the caller's thread (spike D lesson).
            let weak = Arc::downgrade(&player);
            std::thread::Builder::new()
                .name("ristle-state".into())
                .spawn(move || loop {
                    match notify_rx.recv_timeout(Duration::from_millis(500)) {
                        Err(RecvTimeoutError::Disconnected) => return,
                        _ => {}
                    }
                    let Some(p) = weak.upgrade() else { return };
                    let s = p.state();
                    if let Some(o) = observer.lock().unwrap().as_ref() {
                        o.on_state(s);
                    }
                })
                .map_err(|e| PlayerError::Failed { msg: e.to_string() })?;
        }
        Ok(player)
    }

    pub fn set_observer(&self, observer: Arc<dyn StateObserver>) {
        *self.observer.lock().unwrap() = Some(observer);
        let _ = self.notify.send(());
    }

    pub fn play(&self) {
        self.shared.playing.store(true, Release);
        if let Some(s) = self.stream.lock().unwrap().as_ref() {
            let _ = s.play();
        }
        let _ = self.notify.send(());
    }

    /// Pausing stops the AAudio stream, so nothing runs while paused.
    pub fn pause(&self) {
        self.shared.playing.store(false, Release);
        if let Some(s) = self.stream.lock().unwrap().as_ref() {
            let _ = s.pause();
        }
        let _ = self.notify.send(());
    }

    pub fn seek(&self, track: u32, position_ms: u64) {
        let _ = self.cmd.send(Cmd::Seek { track: track as usize, ms: position_ms });
        let _ = self.notify.send(());
    }

    pub fn next(&self) {
        let s = self.state();
        self.seek((s.track_index + 1) % s.track_count, 0);
    }

    pub fn previous(&self) {
        let s = self.state();
        let t = if s.position_ms > 3000 { s.track_index } else { (s.track_index + s.track_count - 1) % s.track_count };
        self.seek(t, 0);
    }

    pub fn state(&self) -> PlayerState {
        let sh = &self.shared;
        let g = sh.generation.load(Acquire);
        let consumed = sh.consumed.load(Acquire);
        let b = sh.boundaries.lock().unwrap();
        let cur = b.iter().rev().find(|b| b.generation == g && b.timeline_start <= consumed).or_else(|| b.iter().rev().find(|b| b.generation == g)).copied().unwrap_or(b[0]);
        drop(b);
        let pos_frames = cur.track_start_frame + consumed.saturating_sub(cur.timeline_start);
        let err = sh.error.lock().unwrap().clone();
        let status = if err.is_some() {
            Status::Error
        } else if !sh.playing.load(Acquire) {
            Status::Paused
        } else if sh.buffering.load(Acquire) {
            Status::Buffering
        } else {
            Status::Playing
        };
        PlayerState {
            status,
            track_index: cur.track as u32,
            track_count: self.titles.len() as u32,
            title: self.titles.get(cur.track).cloned().unwrap_or_default(),
            position_ms: pos_frames * 1000 / self.rate as u64,
            duration_ms: sh.durations.lock().unwrap().get(cur.track).copied().unwrap_or(0),
            error: err,
        }
    }

    pub fn stats(&self) -> PlayerStats {
        let sh = &self.shared;
        let b = sh.boundaries.lock().unwrap();
        PlayerStats {
            uptime_ms: sh.started.elapsed().as_millis() as u64,
            frames_played: sh.frames_played.load(Relaxed),
            callbacks: sh.callbacks.load(Relaxed),
            starved_callbacks: sh.starved.load(Relaxed),
            xruns: sh.xruns.load(Relaxed),
            max_callback_us: sh.max_cb_ns.swap(0, Relaxed) / 1000,
            transitions: TRANSITIONS.load(Relaxed),
            decode_cpu_ms: sh.decode_cpu_ns.load(Relaxed) / 1_000_000,
            callback_cpu_ms: sh.callback_cpu_ns.load(Relaxed) / 1_000_000,
            sample_rate: self.rate,
            output_buffer_frames: b.len() as i64,
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.cmd.send(Cmd::Shutdown);
        self.stream.lock().unwrap().take();
    }
}

static TRANSITIONS: AtomicU64 = AtomicU64::new(0);

fn build_stream(
    shared: Arc<Shared>,
    mut consumer: Consumer<f32>,
    mut handoff: Consumer<(u64, Consumer<f32>)>,
    rate: u32,
    channels: usize,
) -> anyhow::Result<cpal::Stream> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or_else(|| anyhow::anyhow!("no output device"))?;
    let config = cpal::StreamConfig { channels: channels as u16, sample_rate: rate, buffer_size: cpal::BufferSize::Default };
    let sh_err = shared.clone();
    let stream = device.build_output_stream(
        config,
        move |out: &mut [f32], _| {
            let t0 = Instant::now();
            // A seek handed over a fresh ring: switch to it and restart the timeline.
            while let Ok((generation, c)) = handoff.pop() {
                consumer = c;
                shared.consumed.store(0, Release);
                shared.generation.store(generation, Release);
            }
            let playing = shared.playing.load(Acquire);
            let mut n = 0;
            if playing {
                let want = out.len().min(consumer.slots());
                if let Ok(chunk) = consumer.read_chunk(want) {
                    let (a, b) = chunk.as_slices();
                    out[..a.len()].copy_from_slice(a);
                    out[a.len()..a.len() + b.len()].copy_from_slice(b);
                    n = a.len() + b.len();
                    chunk.commit_all();
                }
            }
            out[n..].fill(0.0);
            let frames = (n / channels) as u64;
            shared.consumed.fetch_add(frames, AcqRel);
            shared.frames_played.fetch_add(frames, Relaxed);
            shared.callbacks.fetch_add(1, Relaxed);
            if playing && n < out.len() {
                shared.starved.fetch_add(1, Relaxed);
                shared.buffering.store(true, Release);
            } else if playing {
                shared.buffering.store(false, Release);
            }
            shared.callback_cpu_ns.store(thread_cpu().as_nanos() as u64, Relaxed);
            shared.max_cb_ns.fetch_max(t0.elapsed().as_nanos() as u64, Relaxed);
        },
        move |e| {
            if matches!(e.kind(), cpal::ErrorKind::Xrun) {
                sh_err.xruns.fetch_add(1, Relaxed);
            } else {
                *sh_err.error.lock().unwrap() = Some(format!("output: {e}"));
            }
        },
        None,
    )?;
    stream.pause().ok();
    Ok(stream)
}

struct OpenTrack {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn registry::Decoder>,
    track_id: u32,
    rate: u32,
    /// Host gapless for MP4: frames still to drop at the start, and frames
    /// left before the end padding (None = unlimited).
    skip: u64,
    remaining: Option<u64>,
    duration_ms: u64,
}

fn open_track(reg: &registry::DecoderRegistry, path: &str, start_ms: u64) -> anyhow::Result<OpenTrack> {
    let ext = std::path::Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let edit = if matches!(ext.as_str(), "m4a" | "mp4") { mp4edit::read_edit(&std::fs::read(path)?) } else { None };
    let mut format = decode::open(Box::new(File::open(path)?), Some(&ext))?;
    let (track_id, stream) = decode::audio_stream(format.as_ref(), None)?;
    let rate = stream.params.sample_rate.ok_or_else(|| anyhow::anyhow!("no sample rate"))?;
    let track = format.tracks().iter().find(|t| t.id == track_id).unwrap();
    let total = match (edit, track.num_frames) {
        (Some(e), _) => Some(e.duration_frames),
        (None, Some(n)) => Some(n),
        _ => None,
    };
    let duration_ms = total.map_or(0, |n| n * 1000 / rate as u64);
    let (decoder, _, _) = reg.create(&stream)?;
    let mut skip = edit.map_or(0, |e| e.media_time);
    let mut remaining = edit.map(|e| e.duration_frames);
    if start_ms > 0 {
        let target = start_ms * rate as u64 / 1000;
        let seeked = format.seek(SeekMode::Accurate, SeekTo::Time { time: Time::try_from_secs_f64(start_ms as f64 / 1000.0).unwrap(), track_id: Some(track_id) })?;
        // Decode-and-discard from where the demuxer landed to the exact frame.
        let landed = seeked.actual_ts.get().max(0) as u64;
        skip = target.saturating_sub(landed) + if landed == 0 { skip } else { 0 };
        remaining = remaining.map(|r| r.saturating_sub(target));
    }
    Ok(OpenTrack { format, decoder, track_id, rate, skip, remaining, duration_ms })
}

struct DecodeLoop {
    reg: registry::DecoderRegistry,
    paths: Vec<String>,
    repeat: bool,
    shared: Arc<Shared>,
    producer: Producer<f32>,
    track: usize,
    cur: Option<OpenTrack>,
    /// Frames pushed in the current generation.
    timeline: u64,
    /// Generation of the ring being filled. The callback may still be on the
    /// previous one, so boundaries are stamped with this, not the shared value.
    generation: u64,
    rate: u32,
    pending: Vec<f32>,
}

impl DecodeLoop {
    fn run(&mut self, cmds: Receiver<Cmd>, handoff: &mut Producer<(u64, Consumer<f32>)>) {
        let t_cpu = thread_cpu();
        let mut pcm = Vec::new();
        if let Some(t) = &self.cur {
            self.shared.durations.lock().unwrap()[0] = t.duration_ms;
        }
        loop {
            self.shared.decode_cpu_ns.store((thread_cpu() - t_cpu).as_nanos() as u64, Relaxed);
            match cmds.try_recv() {
                Ok(Cmd::Shutdown) => return,
                Ok(Cmd::Seek { track, ms }) => {
                    if let Err(e) = self.seek(track, ms, handoff) {
                        *self.shared.error.lock().unwrap() = Some(format!("{e:#}"));
                    }
                    continue;
                }
                Err(_) => {}
            }
            // Push what is pending; sleep while the ring is full.
            if !self.pending.is_empty() {
                let n = self.pending.len().min(self.producer.slots());
                if n == 0 {
                    std::thread::sleep(Duration::from_millis(20));
                    continue;
                }
                if let Ok(mut chunk) = self.producer.write_chunk_uninit(n) {
                    let _ = &mut chunk;
                    let written = chunk.fill_from_iter(self.pending.drain(..n));
                    debug_assert_eq!(written, n);
                }
                continue;
            }
            let Some(t) = self.cur.as_mut() else {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            };
            let packet = match t.format.next_packet() {
                Ok(Some(p)) => Some(p),
                Ok(None) => None,
                Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => None,
                Err(e) => {
                    *self.shared.error.lock().unwrap() = Some(format!("demux: {e}"));
                    self.cur = None;
                    continue;
                }
            };
            let Some(packet) = packet else {
                // Flush what the decoder still holds (MediaCodec keeps packets
                // in flight) before the next track starts behind it.
                let mut tail = Vec::new();
                match t.decoder.finish(&mut tail) {
                    Ok(()) => self.timeline += trim_into(t, &tail, &mut self.pending),
                    Err(e) => log_line(&format!("decoder flush failed: {e:#}")),
                }
                self.advance_track();
                continue;
            };
            if packet.track_id != t.track_id {
                continue;
            }
            pcm.clear();
            if let Err(e) = t.decoder.decode(&packet, &mut pcm) {
                log_line(&format!("decode error, packet skipped: {e:#}"));
                continue;
            }
            self.timeline += trim_into(t, &pcm, &mut self.pending);
        }
    }

    fn advance_track(&mut self) {
        let next = self.track + 1;
        if next >= self.paths.len() && !self.repeat {
            self.cur = None;
            return;
        }
        let next = next % self.paths.len();
        match open_track(&self.reg, &self.paths[next], 0) {
            Ok(t) if t.rate == self.rate => {
                self.shared.durations.lock().unwrap()[next] = t.duration_ms;
                let g = self.generation;
                // The next track starts right after the last frame of this one.
                let start = self.timeline;
                self.shared.boundaries.lock().unwrap().push(Boundary { generation: g, timeline_start: start, track: next, track_start_frame: 0 });
                prune(&self.shared, g);
                TRANSITIONS.fetch_add(1, Relaxed);
                self.track = next;
                self.cur = Some(t);
            }
            Ok(t) => {
                *self.shared.error.lock().unwrap() = Some(format!("track {next} is {} Hz, output is {} Hz (spike: no resampler)", t.rate, self.rate));
                self.cur = None;
            }
            Err(e) => {
                *self.shared.error.lock().unwrap() = Some(format!("{e:#}"));
                self.cur = None;
            }
        }
    }

    fn seek(&mut self, track: usize, ms: u64, handoff: &mut Producer<(u64, Consumer<f32>)>) -> anyhow::Result<()> {
        // While paused the callback does not drain the handoff queue. Refuse
        // the seek before touching the active ring rather than lose it.
        if handoff.slots() == 0 {
            anyhow::bail!("seek dropped: two seeks already pending while paused");
        }
        let track = track % self.paths.len();
        let t = open_track(&self.reg, &self.paths[track], ms)?;
        self.shared.durations.lock().unwrap()[track] = t.duration_ms;
        self.generation += 1;
        let g = self.generation;
        let (producer, consumer) = RingBuffer::<f32>::new(self.rate as usize * 2 * RING_SECONDS);
        self.producer = producer;
        self.pending.clear();
        self.timeline = 0;
        self.shared.boundaries.lock().unwrap().push(Boundary { generation: g, timeline_start: 0, track, track_start_frame: ms * self.rate as u64 / 1000 });
        handoff.push((g, consumer)).map_err(|_| anyhow::anyhow!("seek handoff full"))?;
        // While paused the callback is not running, so publish the generation
        // here as well; the callback resets `consumed` when it picks the ring up.
        if !self.shared.playing.load(Acquire) {
            self.shared.generation.store(g, Release);
            self.shared.consumed.store(0, Release);
        }
        prune(&self.shared, g);
        self.track = track;
        self.cur = Some(t);
        Ok(())
    }
}

/// Applies the track's gapless trims (priming skip, valid length) to decoded
/// frames, upmixes mono, appends to `pending`. Returns the frames appended.
fn trim_into(t: &mut OpenTrack, pcm: &[f32], pending: &mut Vec<f32>) -> u64 {
    let ch = t.decoder.output().1.max(1);
    let mut frames = pcm.len() / ch;
    let skip = (t.skip as usize).min(frames);
    t.skip -= skip as u64;
    frames -= skip;
    if let Some(r) = t.remaining.as_mut() {
        frames = frames.min(*r as usize);
        *r -= frames as u64;
    }
    if ch == 2 {
        pending.extend_from_slice(&pcm[skip * 2..(skip + frames) * 2]);
    } else {
        for f in pcm[skip * ch..(skip + frames) * ch].chunks_exact(ch) {
            pending.extend_from_slice(&[f[0], f[0]]);
        }
    }
    frames as u64
}

fn prune(shared: &Shared, generation: u64) {
    let mut b = shared.boundaries.lock().unwrap();
    let consumed = shared.consumed.load(Acquire);
    let current = shared.generation.load(Acquire);
    b.retain(|x| x.generation >= current.min(generation));
    // Keep only the last boundary already passed plus the ones ahead.
    let passed: Vec<usize> = b.iter().enumerate().filter(|(_, x)| x.generation == current && x.timeline_start <= consumed).map(|(i, _)| i).collect();
    if passed.len() > 1 {
        let cut = passed[passed.len() - 2];
        b.drain(..=cut);
    }
}

fn log_line(s: &str) {
    #[cfg(target_os = "android")]
    log::warn!("{s}");
    #[cfg(not(target_os = "android"))]
    eprintln!("{s}");
}
