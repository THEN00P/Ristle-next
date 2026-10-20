//! The decode side: which stream feeds which slot, and the thread that pulls
//! PCM from the streams into the rings.
//!
//! The feed lock guards slot assignment and ring writes. It is never held
//! during stream I/O: the decode thread borrows a slot's source, reads and
//! converts without the lock, and hands the source back. If the control side
//! replaced or sought the slot meanwhile, the data read is dropped.
//!
//! The thread decodes on demand (ADR 0004): it fills both rings and parks.
//! The render callback unparks it when the current ring falls below half, so
//! with the default 2 s ring it wakes about once a second. Only a stream that
//! answers `Pending` (waiting on the network) is retried on a short timer.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use crate::TrackToken;
use crate::convert::Converter;
use crate::engine::{EngineEvent, EngineObserver};
use crate::output::{AudioOutput, OutputError};
use crate::render::Note;
use crate::shared::{Record, Shared};
use crate::stream::{MediaStream, Read};

/// Most frames one read asks a stream for.
const READ_FRAMES: u64 = 4096;
/// A slot is refilled once this many output frames are free, or a quarter of
/// its ring when that is less.
const MIN_WRITE_FRAMES: u64 = 256;
const FIRST_RETRY: Duration = Duration::from_millis(2);
const LONGEST_RETRY: Duration = Duration::from_millis(20);

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Collects output failures from any thread and wakes the decode thread to
/// report them.
#[derive(Default)]
pub(crate) struct ErrorInbox {
    errors: Mutex<Vec<OutputError>>,
    pub(crate) decoder: std::sync::OnceLock<thread::Thread>,
}

impl ErrorInbox {
    pub(crate) fn report(&self, error: OutputError) {
        lock(&self.errors).push(error);
        if let Some(thread) = self.decoder.get() {
            thread.unpark();
        }
    }
}

/// What the control side and the decode thread share.
pub(crate) struct Control {
    pub(crate) shared: Arc<Shared>,
    pub(crate) feed: Mutex<Feed>,
    /// Lock order: `feed`, then `output`.
    pub(crate) output: Mutex<Box<dyn AudioOutput>>,
    pub(crate) output_errors: Arc<ErrorInbox>,
    pub(crate) observer: Arc<dyn EngineObserver>,
}

pub(crate) struct Feed {
    pub(crate) slots: [FeedSlot; 2],
    next_seq: u64,
    pub(crate) shutdown: bool,
}

pub(crate) struct FeedSlot {
    /// Mirror of the published record.
    pub(crate) record: Record,
    source: Option<Source>,
    /// Changes whenever `source` is replaced, so a borrowed source that came
    /// back after its replacement is dropped instead of reinstalled.
    source_id: u64,
    borrowed: bool,
    pending_seek: Option<u64>,
    /// Converted samples that did not fit in the ring yet.
    pending: Vec<f32>,
    stream_ended: bool,
    pub(crate) error: Option<String>,
    retry_at: Option<Instant>,
    backoff: Duration,
}

impl FeedSlot {
    fn new() -> Self {
        Self {
            record: Record::default(),
            source: None,
            source_id: 0,
            borrowed: false,
            pending_seek: None,
            pending: Vec::new(),
            stream_ended: false,
            error: None,
            retry_at: None,
            backoff: FIRST_RETRY,
        }
    }
}

struct Source {
    stream: Box<dyn MediaStream>,
    converter: Option<Converter>,
    input: Vec<f32>,
}

enum Pulled {
    Data(Vec<f32>),
    Pending(Vec<f32>),
    Ended(Vec<f32>),
    Failed(String),
}

impl Source {
    fn new(stream: Box<dyn MediaStream>) -> Self {
        Self {
            stream,
            converter: None,
            input: Vec::new(),
        }
    }

    /// Seeks if asked, then reads up to `frames` input frames and converts
    /// them to the output format.
    fn pull(&mut self, seek: Option<u64>, frames: u64, shared: &Shared) -> Pulled {
        if let Some(position_ms) = seek {
            if let Err(error) = self.stream.seek(position_ms) {
                return Pulled::Failed(format!("seek to {position_ms} ms: {error}"));
            }
            // The resampler's history belongs to the old position.
            self.converter = None;
        }
        let mut data = Vec::new();
        let format = self.stream.format();
        if self.converter.as_ref().is_none_or(|c| c.input() != format) {
            if let Some(mut old) = self.converter.take()
                && let Err(error) = old.finish(&mut data)
            {
                return Pulled::Failed(error);
            }
            match Converter::new(
                format,
                shared.info.sample_rate,
                usize::from(shared.info.channels),
            ) {
                Ok(converter) => self.converter = Some(converter),
                Err(error) => return Pulled::Failed(error),
            }
        }
        let Some(converter) = self.converter.as_mut() else {
            return Pulled::Failed("no converter".into());
        };
        let channels = usize::from(format.channels);
        self.input.resize(frames as usize * channels, 0.0);
        match self.stream.read(&mut self.input) {
            Ok(Read::Frames(n)) => {
                let n = n.min(frames as usize);
                match converter.push(&self.input[..n * channels], &mut data) {
                    Ok(()) => Pulled::Data(data),
                    Err(error) => Pulled::Failed(error),
                }
            }
            Ok(Read::Pending) => Pulled::Pending(data),
            Ok(Read::End) => match converter.finish(&mut data) {
                Ok(()) => Pulled::Ended(data),
                Err(error) => Pulled::Failed(error),
            },
            Err(error) => Pulled::Failed(error.to_string()),
        }
    }
}

impl Feed {
    pub(crate) fn new() -> Self {
        Self {
            slots: [FeedSlot::new(), FeedSlot::new()],
            next_seq: 1,
            shutdown: false,
        }
    }

    fn publish(&mut self, shared: &Shared, slot: usize, record: Record) {
        self.slots[slot].record = record;
        shared.slots[slot].record.publish(record);
    }

    /// Starts new audio in `slot`: drops what the ring holds, and either
    /// installs `stream` (a new Track) or keeps the slot's stream and seeks
    /// it (`stream` is `None`, `seek` is set). Returns the new record.
    pub(crate) fn restart(
        &mut self,
        shared: &Shared,
        slot: usize,
        token: Option<TrackToken>,
        stream: Option<Box<dyn MediaStream>>,
        start_ms: u64,
    ) -> Record {
        let seq = self.next_seq;
        self.next_seq += 1;
        let ring = &shared.slots[slot].ring;
        let start = ring.write_position();
        let record = Record {
            seq,
            token,
            start,
            start_frame: shared.ms_to_frames(start_ms),
            end: None,
            failed: false,
        };
        // Publish before discarding: a callback that sees the discard then
        // also sees the record it belongs to.
        self.publish(shared, slot, record);
        ring.discard_to(start);
        let feed_slot = &mut self.slots[slot];
        let keep_source = token.is_some() && stream.is_none();
        if !keep_source {
            feed_slot.source = stream.map(Source::new);
            feed_slot.source_id += 1;
            feed_slot.borrowed = false;
        }
        feed_slot.pending_seek = (keep_source || start_ms > 0).then_some(start_ms);
        feed_slot.pending.clear();
        feed_slot.stream_ended = false;
        feed_slot.error = None;
        feed_slot.retry_at = None;
        feed_slot.backoff = FIRST_RETRY;
        shared.wake_decoder();
        record
    }

    pub(crate) fn clear(&mut self, shared: &Shared, slot: usize) {
        self.restart(shared, slot, None, None, 0);
    }

    /// Whether `slot` holds audio ready to play: at least half a ring, or the
    /// whole Track.
    #[cfg(feature = "test-support")]
    pub(crate) fn buffered(&self, shared: &Shared, slot: usize) -> bool {
        let record = self.slots[slot].record;
        let ring = &shared.slots[slot].ring;
        record.token.is_some()
            && !record.failed
            && (record.end.is_some() || ring.readable() >= ring.capacity() / 2)
    }

    /// Writes pending samples to the ring, and publishes the end once the
    /// stream ended and everything is in.
    fn flush(&mut self, shared: &Shared, slot: usize) {
        let ring = &shared.slots[slot].ring;
        let channels = usize::from(shared.info.channels);
        let feed_slot = &mut self.slots[slot];
        if !feed_slot.pending.is_empty() {
            let written = ring.write(&feed_slot.pending);
            feed_slot.pending.drain(..written * channels);
        }
        if feed_slot.stream_ended && feed_slot.pending.is_empty() && feed_slot.record.end.is_none()
        {
            let record = Record {
                end: Some(ring.write_position()),
                ..feed_slot.record
            };
            self.publish(shared, slot, record);
        }
    }

    fn fail(&mut self, shared: &Shared, slot: usize, error: String) -> Option<EngineEvent> {
        let feed_slot = &mut self.slots[slot];
        feed_slot.stream_ended = true;
        feed_slot.error = Some(error.clone());
        let record = Record {
            failed: true,
            ..feed_slot.record
        };
        self.publish(shared, slot, record);
        self.flush(shared, slot);
        record
            .token
            .map(|track| EngineEvent::Failed { track, error })
    }

    /// The slot the decode thread should read next, if any, and the earliest
    /// time a waiting slot wants to be retried.
    fn next_work(&self, shared: &Shared, now: Instant) -> (Option<usize>, Option<Instant>) {
        let current = shared.current.load(Ordering::Acquire);
        let mut retry = None;
        for slot in [current, 1 - current] {
            let feed_slot = &self.slots[slot];
            if feed_slot.record.token.is_none()
                || feed_slot.borrowed
                || feed_slot.source.is_none()
                || feed_slot.stream_ended
                || !feed_slot.pending.is_empty()
                || shared.slots[slot].ring.writable() < min_write(&shared.slots[slot].ring)
            {
                continue;
            }
            match feed_slot.retry_at {
                Some(at) if at > now => retry = Some(retry.map_or(at, |r: Instant| r.min(at))),
                _ => return (Some(slot), None),
            }
        }
        (None, retry)
    }
}

fn min_write(ring: &crate::ring::Ring) -> u64 {
    MIN_WRITE_FRAMES.min(ring.capacity() / 4).max(1)
}

/// The decode thread's body.
pub(crate) fn run(control: Arc<Control>, mut notes: rtrb::Consumer<Note>) {
    loop {
        control.shared.clear_wake();
        handle_notes(&control, &mut notes);
        handle_output_errors(&control);
        match fill(&control) {
            Fill::Shutdown => return,
            Fill::Idle => thread::park(),
            Fill::RetryAt(at) => thread::park_timeout(at.saturating_duration_since(Instant::now())),
        }
    }
}

enum Fill {
    Shutdown,
    Idle,
    RetryAt(Instant),
}

fn fill(control: &Control) -> Fill {
    let shared = &*control.shared;
    loop {
        let mut feed = lock(&control.feed);
        if feed.shutdown {
            return Fill::Shutdown;
        }
        for slot in 0..2 {
            feed.flush(shared, slot);
        }
        let (work, retry) = feed.next_work(shared, Instant::now());
        let Some(slot) = work else {
            return retry.map_or(Fill::Idle, Fill::RetryAt);
        };
        let feed_slot = &mut feed.slots[slot];
        let Some(mut source) = feed_slot.source.take() else {
            return Fill::Idle;
        };
        feed_slot.borrowed = true;
        let seq = feed_slot.record.seq;
        let source_id = feed_slot.source_id;
        let seek = feed_slot.pending_seek.take();
        let writable = shared.slots[slot].ring.writable();
        let input_rate = u64::from(source.stream.format().sample_rate.max(1));
        let frames =
            (writable * input_rate / u64::from(shared.info.sample_rate)).clamp(1, READ_FRAMES);
        drop(feed);

        let pulled = source.pull(seek, frames, shared);

        let mut feed = lock(&control.feed);
        let feed_slot = &mut feed.slots[slot];
        if feed_slot.source_id != source_id {
            drop(feed);
            drop(source);
            continue;
        }
        feed_slot.borrowed = false;
        feed_slot.source = Some(source);
        if feed_slot.record.seq != seq {
            // Sought while reading: the data is from the old position.
            continue;
        }
        let event = match pulled {
            Pulled::Data(data) => {
                feed_slot.pending.extend_from_slice(&data);
                feed_slot.retry_at = None;
                feed_slot.backoff = FIRST_RETRY;
                None
            }
            Pulled::Pending(data) => {
                feed_slot.pending.extend_from_slice(&data);
                feed_slot.retry_at = Some(Instant::now() + feed_slot.backoff);
                feed_slot.backoff = (feed_slot.backoff * 2).min(LONGEST_RETRY);
                None
            }
            Pulled::Ended(data) => {
                feed_slot.pending.extend_from_slice(&data);
                feed_slot.stream_ended = true;
                None
            }
            Pulled::Failed(error) => {
                tracing::warn!(slot, %error, "audio stream failed");
                feed.fail(shared, slot, error)
            }
        };
        feed.flush(shared, slot);
        drop(feed);
        if let Some(event) = event {
            control.observer.on_event(event);
        }
    }
}

fn handle_notes(control: &Control, notes: &mut rtrb::Consumer<Note>) {
    let shared = &*control.shared;
    while let Ok(note) = notes.pop() {
        match note {
            Note::Transitioned {
                from_slot,
                from,
                to,
            } => {
                {
                    let mut feed = lock(&control.feed);
                    // Free the finished slot for the next prepared Track,
                    // unless the control side already put one there.
                    if feed.slots[from_slot].record.seq == from.seq {
                        feed.clear(shared, from_slot);
                    }
                }
                if let (Some(from), Some(to)) = (from.token, to.token) {
                    control
                        .observer
                        .on_event(EngineEvent::Transitioned { from, to });
                }
            }
            Note::Stopped { record } => {
                let feed = lock(&control.feed);
                let current = shared.current.load(Ordering::Acquire);
                if feed.slots[current].record.seq == record.seq {
                    shared.playing.store(false, Ordering::Release);
                    lock(&control.output).pause();
                }
            }
            Note::Ended { record } => {
                {
                    let feed = lock(&control.feed);
                    let current = shared.current.load(Ordering::Acquire);
                    // Stop output unless a command already moved on.
                    if feed.slots[current].record.seq == record.seq
                        && shared.ended.load(Ordering::Acquire) == record.seq
                    {
                        shared.playing.store(false, Ordering::Release);
                        lock(&control.output).pause();
                    }
                }
                if let Some(track) = record.token {
                    control.observer.on_event(EngineEvent::Ended { track });
                }
            }
        }
    }
}

fn handle_output_errors(control: &Control) {
    let errors = std::mem::take(&mut *lock(&control.output_errors.errors));
    if errors.is_empty() {
        return;
    }
    {
        let _feed = lock(&control.feed);
        control.shared.playing.store(false, Ordering::Release);
        lock(&control.output).pause();
    }
    for error in errors {
        tracing::warn!(%error, "audio output failed");
        control
            .observer
            .on_event(EngineEvent::OutputFailed { error });
    }
}
