//! The engine's control surface.
//!
//! Every method returns at once: none waits on stream I/O or on the render
//! callback. Effects that happen later (a Gapless Transition, the end of
//! playback, a stream failure) arrive as [`EngineEvent`]s on the engine's
//! decode thread, a core thread (ADR 0001).

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::feed::{self, Control, ErrorInbox, Feed, lock};
use crate::output::{AudioOutput, OutputError, OutputErrors, OutputRequest};
use crate::render::{NOTE_CAPACITY, RenderState, Renderer};
use crate::shared::Shared;
use crate::stream::MediaStream;

/// Identifies a Track to the engine. The session passes its queue entry id,
/// and the engine reports it back in state and events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TrackToken(pub u64);

/// How the engine is set up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineConfig {
    /// Decoded audio kept ahead, per slot. The decode thread wakes when half
    /// of it has played, so this also sets how often it runs.
    pub buffer_ms: u32,
    /// Output rate to ask for; `None` uses the device's rate.
    pub preferred_rate: Option<u32>,
    /// Output channels.
    pub channels: u16,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            buffer_ms: 2000,
            preferred_rate: None,
            channels: 2,
        }
    }
}

/// What the engine reports after the fact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineEvent {
    /// Playback moved from one Track to the prepared next one at the exact
    /// frame where the first ended (Gapless Transition).
    Transitioned {
        /// The Track that ended.
        from: TrackToken,
        /// The Track now playing.
        to: TrackToken,
    },
    /// The current Track played to its end with nothing prepared after it.
    /// Output is stopped; this is the terminal state (ADR 0004).
    Ended {
        /// The Track that ended.
        track: TrackToken,
    },
    /// A stream failed, current or prepared. What it decoded before the
    /// failure still plays; playback then stops on that Track.
    Failed {
        /// The Track whose stream failed.
        track: TrackToken,
        /// The stream's error.
        error: String,
    },
    /// The output device failed. Playback is paused.
    OutputFailed {
        /// The output's error.
        error: OutputError,
    },
}

/// Receives [`EngineEvent`]s, on the engine's decode thread. Must return
/// quickly and must not call back into the engine synchronously from a lock
/// the engine's caller holds.
pub trait EngineObserver: Send + Sync {
    /// One event.
    fn on_event(&self, event: EngineEvent);
}

/// A snapshot of the engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineState {
    /// The Track playing or paused, if any.
    pub current: Option<TrackToken>,
    /// The prepared next Track, unless its stream failed.
    pub next: Option<TrackToken>,
    /// Output is running.
    pub playing: bool,
    /// The current Track has no audio ready and has not ended.
    pub buffering: bool,
    /// The current Track played to its end with nothing after it.
    pub ended: bool,
    /// Position in the current Track.
    pub position_ms: u64,
    /// Why the current Track's stream failed, kept for diagnostics until the
    /// Track is replaced or sought.
    pub error: Option<String>,
    /// Grows on every discontinuity (Track change, seek, end). A position
    /// read with an older sequence is stale.
    pub sequence: u64,
}

/// Why a command was refused.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The output could not be opened.
    #[error(transparent)]
    Output(#[from] OutputError),
    /// The command needs a current Track.
    #[error("no current Track")]
    NoCurrent,
    /// Volume outside 0 to 1.
    #[error("volume must be between 0 and 1, got {0}")]
    InvalidVolume(f32),
    /// The output reported a format the engine cannot render.
    #[error("unusable output format: {0} Hz, {1} channels")]
    UnusableFormat(u32, u16),
    /// The decode thread could not start.
    #[error("could not start the audio thread: {0}")]
    Thread(String),
}

/// The playback engine: two slots (current and prepared next), one decode
/// thread, one output.
pub struct Engine {
    control: Arc<Control>,
    thread: Option<JoinHandle<()>>,
}

impl Engine {
    /// Opens `output` and starts the decode thread. Playback starts paused
    /// with no Track.
    pub fn new(
        mut output: Box<dyn AudioOutput>,
        config: EngineConfig,
        observer: Arc<dyn EngineObserver>,
    ) -> Result<Self, Error> {
        let (mut install, installed) = rtrb::RingBuffer::new(1);
        let errors = Arc::new(ErrorInbox::default());
        let request = OutputRequest {
            preferred_rate: config.preferred_rate,
            channels: config.channels,
            errors: {
                let errors = Arc::clone(&errors);
                OutputErrors::new(move |error| errors.report(error))
            },
        };
        let info = output.open(request, Box::new(Renderer::new(installed)))?;
        if info.sample_rate == 0 || info.channels == 0 {
            output.close();
            return Err(Error::UnusableFormat(info.sample_rate, info.channels));
        }
        let ring_frames =
            (u64::from(info.sample_rate) * u64::from(config.buffer_ms.max(1)) / 1000).max(1);
        let shared = Arc::new(Shared::new(info, ring_frames as usize));
        let (notes, notes_out) = rtrb::RingBuffer::new(NOTE_CAPACITY);
        if install
            .push(RenderState::new(Arc::clone(&shared), notes))
            .is_err()
        {
            output.close();
            return Err(Error::Thread("render state was already installed".into()));
        }
        let control = Arc::new(Control {
            shared: Arc::clone(&shared),
            feed: Mutex::new(Feed::new()),
            output: Mutex::new(output),
            output_errors: Arc::clone(&errors),
            observer,
        });
        let thread = {
            let control = Arc::clone(&control);
            thread::Builder::new()
                .name("ristle-audio".into())
                .spawn(move || feed::run(control, notes_out))
        };
        let thread = match thread {
            Ok(thread) => thread,
            Err(error) => {
                lock(&control.output).close();
                return Err(Error::Thread(error.to_string()));
            }
        };
        let _ = shared.decoder.set(thread.thread().clone());
        let _ = errors.decoder.set(thread.thread().clone());
        thread.thread().unpark();
        Ok(Self {
            control,
            thread: Some(thread),
        })
    }

    fn shared(&self) -> &Shared {
        &self.control.shared
    }

    /// Makes `stream` the current Track, starting at `start_ms`, and drops
    /// the prepared next Track. Keeps playing or paused as before.
    pub fn set_current(&self, track: TrackToken, stream: Box<dyn MediaStream>, start_ms: u64) {
        let shared = self.shared();
        let mut feed = lock(&self.control.feed);
        // Empty the other slot first, so the render callback cannot switch
        // into it while the current slot changes; then check that it did not
        // switch before that.
        let current = loop {
            let current = shared.current.load(Ordering::Acquire);
            feed.clear(shared, 1 - current);
            if shared.current.load(Ordering::Acquire) == current {
                break current;
            }
        };
        feed.restart(shared, current, Some(track), Some(stream), start_ms);
        shared.bump_sequence();
    }

    /// Prepares `stream` to follow the current Track gaplessly. Its audio is
    /// decoded ahead while the current Track plays.
    pub fn set_next(&self, track: TrackToken, stream: Box<dyn MediaStream>) -> Result<(), Error> {
        let shared = self.shared();
        let mut feed = lock(&self.control.feed);
        let current = shared.current.load(Ordering::Acquire);
        if feed.slots[current].record.token.is_none() {
            return Err(Error::NoCurrent);
        }
        feed.restart(shared, 1 - current, Some(track), Some(stream), 0);
        Ok(())
    }

    /// Drops the prepared next Track.
    pub fn clear_next(&self) {
        let shared = self.shared();
        let mut feed = lock(&self.control.feed);
        let current = shared.current.load(Ordering::Acquire);
        feed.clear(shared, 1 - current);
    }

    /// Stops and drops both Tracks, for when the Track that should play
    /// could not open: keeping the old one would play the wrong Track.
    pub fn clear_current(&self) {
        let shared = self.shared();
        let mut feed = lock(&self.control.feed);
        shared.playing.store(false, Ordering::Release);
        lock(&self.control.output).pause();
        let current = shared.current.load(Ordering::Acquire);
        feed.clear(shared, 1 - current);
        feed.clear(shared, current);
        shared.bump_sequence();
    }

    /// Makes the prepared next Track current now, using the audio already
    /// decoded for it (Next while a next Track is prepared). Returns false
    /// when there is no next Track, its stream failed, or none of its audio
    /// is decoded yet.
    pub fn promote_next(&self) -> bool {
        let shared = self.shared();
        let mut feed = lock(&self.control.feed);
        let current = shared.current.load(Ordering::Acquire);
        let other = 1 - current;
        let record = feed.slots[other].record;
        let ready = record.token.is_some()
            && !record.failed
            && (record.end.is_some() || shared.slots[other].ring.readable() > 0);
        if !ready {
            return false;
        }
        if shared
            .current
            .compare_exchange(current, other, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            feed.clear(shared, current);
            shared.bump_sequence();
        }
        // Otherwise the render callback switched first, at the end of the
        // current Track: the next Track is current either way.
        true
    }

    /// Starts output. A Track that has ended stays ended until it is
    /// replaced or sought.
    pub fn play(&self) -> Result<(), Error> {
        let shared = self.shared();
        let feed = lock(&self.control.feed);
        let record = feed.slots[shared.current.load(Ordering::Acquire)].record;
        if record.token.is_none() {
            return Err(Error::NoCurrent);
        }
        if shared.ended.load(Ordering::Acquire) == record.seq {
            return Ok(());
        }
        shared.playing.store(true, Ordering::Release);
        lock(&self.control.output).resume();
        Ok(())
    }

    /// Stops output. Nothing runs while paused: the output stops calling
    /// back and the decode thread parks once the rings are full.
    pub fn pause(&self) {
        let _feed = lock(&self.control.feed);
        self.shared().playing.store(false, Ordering::Release);
        lock(&self.control.output).pause();
    }

    /// Moves the current Track to `position_ms`. The new position is the
    /// engine's position at once; the audio follows when the stream has
    /// sought. Returns the new state sequence.
    pub fn seek(&self, position_ms: u64) -> Result<u64, Error> {
        let shared = self.shared();
        let mut feed = lock(&self.control.feed);
        let current = shared.current.load(Ordering::Acquire);
        let Some(track) = feed.slots[current].record.token else {
            return Err(Error::NoCurrent);
        };
        feed.restart(shared, current, Some(track), None, position_ms);
        Ok(shared.bump_sequence())
    }

    /// Sets the one core-owned gain, 0 to 1. Changes during playback ramp
    /// over 10 ms.
    pub fn set_volume(&self, volume: f32) -> Result<(), Error> {
        if !(0.0..=1.0).contains(&volume) {
            return Err(Error::InvalidVolume(volume));
        }
        self.shared()
            .volume
            .store(volume.to_bits(), Ordering::Release);
        Ok(())
    }

    /// The engine's state now.
    pub fn state(&self) -> EngineState {
        let shared = self.shared();
        let feed = lock(&self.control.feed);
        let sequence = shared.sequence.load(Ordering::Acquire);
        let current = shared.current.load(Ordering::Acquire);
        let record = feed.slots[current].record;
        let next = feed.slots[1 - current].record;
        let ring = &shared.slots[current].ring;
        let position = record.start_frame + ring.read_position().saturating_sub(record.start);
        let ended = record.token.is_some() && shared.ended.load(Ordering::Acquire) == record.seq;
        EngineState {
            current: record.token,
            next: next.token.filter(|_| !next.failed),
            playing: shared.playing.load(Ordering::Acquire),
            buffering: record.token.is_some()
                && !ended
                && !record.failed
                && record.end.is_none()
                && ring.readable() == 0,
            ended,
            position_ms: shared.frames_to_ms(position),
            error: record.token.and(feed.slots[current].error.clone()),
            sequence,
        }
    }

    /// The output format the engine renders.
    pub fn output_info(&self) -> crate::output::OutputInfo {
        self.shared().info
    }

    /// Waits until the current Track has audio ready (half a buffer, or all
    /// of it). For tests and tools; the session reacts to state instead.
    #[cfg(feature = "test-support")]
    pub fn wait_until_buffered(&self, timeout: std::time::Duration) -> bool {
        self.wait(timeout, |engine| {
            let feed = lock(&engine.control.feed);
            feed.buffered(
                engine.shared(),
                engine.shared().current.load(Ordering::Acquire),
            )
        })
    }

    /// Waits until the prepared next Track has audio ready. False at once
    /// when there is none or its stream failed.
    #[cfg(feature = "test-support")]
    pub fn wait_until_next_buffered(&self, timeout: std::time::Duration) -> bool {
        self.wait(timeout, |engine| {
            let feed = lock(&engine.control.feed);
            let other = 1 - engine.shared().current.load(Ordering::Acquire);
            feed.buffered(engine.shared(), other)
        })
    }

    #[cfg(feature = "test-support")]
    fn wait(&self, timeout: std::time::Duration, ready: impl Fn(&Self) -> bool) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if ready(self) {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        lock(&self.control.output).close();
        lock(&self.control.feed).shutdown = true;
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            if thread.join().is_err() {
                tracing::error!("audio thread panicked");
            }
        }
    }
}
