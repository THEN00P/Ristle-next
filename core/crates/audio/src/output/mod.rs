//! The output port: where rendered PCM goes.
//!
//! An [`AudioOutput`] opens a device stream and calls the engine's
//! [`Render`] from its real-time thread. Backends: the capture output in
//! `test_support` (headless, driven by the test), `cpal` on desktop, and
//! `aaudio` on Android.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(target_os = "android")]
pub mod aaudio;
#[cfg(not(target_os = "android"))]
pub mod cpal;

/// What the engine asks of an output.
#[derive(Clone)]
pub struct OutputRequest {
    /// A sample rate to prefer. `None` takes the device's own rate, which
    /// avoids a resampler in the platform mixer (ADR 0004).
    pub preferred_rate: Option<u32>,
    /// Interleaved channels the engine renders.
    pub channels: u16,
    /// How the output reports failures after `open`, from any thread. The
    /// engine turns them into an `EngineEvent::OutputFailed`.
    pub errors: OutputErrors,
    /// Where the output keeps its current latency up to date.
    pub latency: OutputLatency,
}

impl fmt::Debug for OutputRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OutputRequest")
            .field("preferred_rate", &self.preferred_rate)
            .field("channels", &self.channels)
            .finish_non_exhaustive()
    }
}

/// A sink for output failures that happen after `open`.
#[derive(Clone)]
pub struct OutputErrors(Arc<dyn Fn(OutputError) + Send + Sync>);

impl OutputErrors {
    /// Reports failures to `sink`.
    pub fn new(sink: impl Fn(OutputError) + Send + Sync + 'static) -> Self {
        Self(Arc::new(sink))
    }

    /// Reports one failure. Not real-time safe: call it off the render thread.
    pub fn report(&self, error: OutputError) {
        (self.0)(error);
    }
}

/// How far the listener is behind the render callback: frames rendered but
/// not yet heard (the device's buffers and the path to the speaker). The
/// output updates it as it learns; the engine subtracts it from positions
/// and waits it out before it reports a Track change or stops output.
#[derive(Clone, Debug, Default)]
pub struct OutputLatency(Arc<AtomicU64>);

impl OutputLatency {
    /// A latency of zero, until the output sets one.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the latency. Real-time safe: one atomic store.
    pub fn set_frames(&self, frames: u64) {
        self.0.store(frames, Ordering::Relaxed);
    }

    /// The latest latency.
    pub fn frames(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

/// The stream an output actually opened. The engine renders exactly this.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputInfo {
    /// Frames per second.
    pub sample_rate: u32,
    /// Interleaved channels per frame.
    pub channels: u16,
}

/// Fills output buffers. Called on the output's real-time thread.
pub trait Render: Send {
    /// Fills all of `out` with interleaved frames in the `OutputInfo` format.
    /// Never blocks, locks, allocates or logs.
    fn render(&mut self, out: &mut [f32]);
}

/// An audio output device.
pub trait AudioOutput: Send {
    /// Opens a stream that pulls from `render`. The stream starts paused.
    fn open(
        &mut self,
        request: OutputRequest,
        render: Box<dyn Render>,
    ) -> Result<OutputInfo, OutputError>;
    /// Stops callbacks, keeping the stream open. Pausing is how the engine
    /// saves power: nothing runs while paused.
    fn pause(&mut self);
    /// Starts or restarts callbacks.
    fn resume(&mut self);
    /// Closes the stream and drops the `Render`.
    fn close(&mut self);
}

/// Why an output failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OutputError {
    /// No output device is available.
    #[error("no audio output device")]
    NoDevice,
    /// The device does not support a usable configuration.
    #[error("unsupported output configuration: {0}")]
    Unsupported(String),
    /// The platform's audio API failed.
    #[error("audio output failed: {0}")]
    Backend(String),
}
