//! Headless output and scripted streams, for this crate's tests and for
//! `ristle-app`'s.

use std::sync::{Arc, Mutex};

use crate::feed::lock;
use crate::output::{
    AudioOutput, OutputError, OutputErrors, OutputInfo, OutputLatency, OutputRequest, Render,
};
use ristle_media::{MediaError, MediaStream, Read, StreamFormat};

/// An output with no device: the test pulls rendered audio through its
/// [`CaptureHandle`], standing in for the device's real-time thread.
pub struct CaptureOutput {
    state: Arc<Mutex<CaptureState>>,
}

/// The test's side of a [`CaptureOutput`].
#[derive(Clone)]
pub struct CaptureHandle {
    state: Arc<Mutex<CaptureState>>,
}

struct CaptureState {
    info: OutputInfo,
    render: Option<Box<dyn Render>>,
    errors: Option<OutputErrors>,
    latency: Option<OutputLatency>,
    running: bool,
    opened: usize,
}

impl CaptureOutput {
    /// An output that opens at this format, whatever the request says.
    pub fn new(sample_rate: u32, channels: u16) -> (Self, CaptureHandle) {
        let state = Arc::new(Mutex::new(CaptureState {
            info: OutputInfo {
                sample_rate,
                channels,
            },
            render: None,
            errors: None,
            latency: None,
            running: false,
            opened: 0,
        }));
        (
            Self {
                state: Arc::clone(&state),
            },
            CaptureHandle { state },
        )
    }
}

impl AudioOutput for CaptureOutput {
    fn open(
        &mut self,
        request: OutputRequest,
        render: Box<dyn Render>,
    ) -> Result<OutputInfo, OutputError> {
        let mut state = lock(&self.state);
        state.render = Some(render);
        state.errors = Some(request.errors);
        state.latency = Some(request.latency);
        state.running = false;
        state.opened += 1;
        Ok(state.info)
    }

    fn pause(&mut self) {
        lock(&self.state).running = false;
    }

    fn resume(&mut self) {
        lock(&self.state).running = true;
    }

    fn close(&mut self) {
        let mut state = lock(&self.state);
        state.running = false;
        state.render = None;
    }
}

impl CaptureHandle {
    /// Runs one device callback of `frames` frames and returns the
    /// interleaved samples. A paused or closed output produces silence
    /// without calling the engine, as a device would.
    pub fn render(&self, frames: usize) -> Vec<f32> {
        let mut state = lock(&self.state);
        let mut out = vec![0.0; frames * usize::from(state.info.channels)];
        if state.running
            && let Some(render) = state.render.as_mut()
        {
            render.render(&mut out);
        }
        out
    }

    /// Whether the engine has the output running.
    pub fn is_running(&self) -> bool {
        lock(&self.state).running
    }

    /// How many times the engine opened the output.
    pub fn opened(&self) -> usize {
        lock(&self.state).opened
    }

    /// Reports an output latency of `frames`, as a device with that much
    /// buffering would.
    pub fn set_latency(&self, frames: u64) {
        if let Some(latency) = &lock(&self.state).latency {
            latency.set_frames(frames);
        }
    }

    /// Reports an output failure, as a device error callback would.
    pub fn fail(&self, error: OutputError) {
        let errors = lock(&self.state).errors.clone();
        if let Some(errors) = errors {
            errors.report(error);
        }
    }
}

/// What a [`FakeStream`]'s frames contain. Every channel of a frame carries
/// the same value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Samples {
    /// The same value throughout.
    Constant(f32),
    /// Frame `n` of the stream holds `offset + n`, so a test can tell
    /// exactly which source frame reached the output.
    Counter {
        /// Value of frame 0.
        offset: f32,
    },
}

/// A scripted `MediaStream`.
#[derive(Debug)]
pub struct FakeStream {
    format: StreamFormat,
    frames: u64,
    position: u64,
    samples: Samples,
    pending_reads: usize,
    fail_at: Option<(u64, String)>,
    max_read: usize,
    format_change: Option<(u64, StreamFormat)>,
}

impl FakeStream {
    /// `frames` frames of `value` in `format`.
    pub fn constant(value: f32, frames: u64, format: StreamFormat) -> Self {
        Self::new(Samples::Constant(value), frames, format)
    }

    /// `frames` frames numbered from `offset` (see [`Samples::Counter`]).
    pub fn counter(offset: f32, frames: u64, format: StreamFormat) -> Self {
        Self::new(Samples::Counter { offset }, frames, format)
    }

    fn new(samples: Samples, frames: u64, format: StreamFormat) -> Self {
        Self {
            format,
            frames,
            position: 0,
            samples,
            pending_reads: 0,
            fail_at: None,
            max_read: usize::MAX,
            format_change: None,
        }
    }

    /// Answers `Pending` to the first `reads` reads, like a stream waiting
    /// on the network.
    pub fn with_pending_reads(mut self, reads: usize) -> Self {
        self.pending_reads = reads;
        self
    }

    /// Fails with `message` once `frame` frames were read.
    pub fn failing_at(mut self, frame: u64, message: &str) -> Self {
        self.fail_at = Some((frame, message.to_owned()));
        self
    }

    /// Returns at most `frames` frames per read.
    pub fn with_max_read(mut self, frames: usize) -> Self {
        self.max_read = frames.max(1);
        self
    }

    /// Switches to `format` after `frame` frames, as implicit SBR does.
    pub fn changing_format_at(mut self, frame: u64, format: StreamFormat) -> Self {
        self.format_change = Some((frame, format));
        self
    }

    fn current_format(&self) -> StreamFormat {
        match self.format_change {
            Some((at, format)) if self.position >= at => format,
            _ => self.format,
        }
    }
}

impl MediaStream for FakeStream {
    fn format(&self) -> StreamFormat {
        self.current_format()
    }

    fn read(&mut self, out: &mut [f32]) -> Result<Read, MediaError> {
        if self.pending_reads > 0 {
            self.pending_reads -= 1;
            return Ok(Read::Pending);
        }
        let mut limit = self.frames;
        if let Some((at, message)) = &self.fail_at {
            if self.position >= *at {
                return Err(failure(message.clone()));
            }
            limit = limit.min(*at);
        }
        if let Some((at, _)) = self.format_change
            && self.position < at
        {
            limit = limit.min(at);
        }
        if self.position >= self.frames {
            return Ok(Read::End);
        }
        let channels = usize::from(self.current_format().channels.max(1));
        let frames = (limit - self.position)
            .min((out.len() / channels) as u64)
            .min(self.max_read as u64) as usize;
        for (index, frame) in out.chunks_exact_mut(channels).take(frames).enumerate() {
            let value = match self.samples {
                Samples::Constant(value) => value,
                Samples::Counter { offset } => offset + (self.position + index as u64) as f32,
            };
            frame.fill(value);
        }
        self.position += frames as u64;
        Ok(Read::Frames(frames))
    }

    fn seek(&mut self, position_ms: u64) -> Result<(), MediaError> {
        let frame = position_ms * u64::from(self.current_format().sample_rate) / 1000;
        if frame > self.frames {
            return Err(failure(format!("{position_ms} ms is past the end")));
        }
        self.position = frame;
        Ok(())
    }
}

/// A stream failure carrying `message`, as a source read error.
fn failure(message: String) -> MediaError {
    MediaError::Io(std::io::Error::other(message))
}
