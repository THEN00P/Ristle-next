//! The Android output, through AAudio (ADR 0004, amendment point 6).
//!
//! The stream asks for PowerSaving performance and the music content type
//! with media usage, which routes it to the deep-buffer (or offload) output:
//! on a Pixel 8, cpal's default landed on the low-latency output at about 95
//! callbacks a second and cost `audioserver` 5.3 % of a core. The rate is
//! left to AAudio on the first open, so the stream runs at the mixer's own
//! rate and the core does the only resampling.
//!
//! AAudio is called through the `ndk` crate, which needs no JVM, so this
//! output needs no `ndk-context` bootstrap. Content type and usage need API
//! level 28, which makes Android 9 the minimum for playback.
//!
//! An AAudio stream is disconnected when its device goes away (headphones
//! unplugged, Bluetooth connected). The output then reopens a stream on the
//! new route at the same rate and format, so the engine's format holds,
//! and keeps it playing or paused as before.

// The data callback receives the output buffer as a raw pointer.
#![allow(unsafe_code)]

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use ndk::audio::{
    AudioCallbackResult, AudioContentType, AudioDirection, AudioError, AudioFormat,
    AudioPerformanceMode, AudioSharingMode, AudioStream, AudioStreamBuilder, AudioUsage,
};

use super::{AudioOutput, OutputError, OutputErrors, OutputInfo, OutputRequest, Render};

/// The default output route, through AAudio.
#[derive(Default)]
pub struct AAudioOutput {
    worker: Option<Worker>,
}

struct Worker {
    commands: mpsc::Sender<Command>,
    thread: JoinHandle<()>,
}

enum Command {
    Pause,
    Resume,
    /// Sent by the error callback when the stream was disconnected.
    Reopen,
    Close,
}

/// The engine's `Render`, shared by successive streams across reopens. Only
/// one stream exists at a time, so the callback's `try_lock` never waits.
type SharedRender = Arc<Mutex<Box<dyn Render>>>;

/// What a stream was opened with, so a reopen gets the same.
#[derive(Clone, Copy)]
struct Settings {
    sample_rate: Option<i32>,
    channels: i32,
}

impl AAudioOutput {
    /// An output on the default route.
    pub fn new() -> Self {
        Self::default()
    }

    fn send(&self, command: Command) {
        if let Some(worker) = &self.worker {
            let _ = worker.commands.send(command);
        }
    }
}

impl AudioOutput for AAudioOutput {
    fn open(
        &mut self,
        request: OutputRequest,
        render: Box<dyn Render>,
    ) -> Result<OutputInfo, OutputError> {
        self.close();
        let (ready, opened) = mpsc::sync_channel(1);
        let (commands, received) = mpsc::channel();
        let reopen = commands.clone();
        let thread = thread::Builder::new()
            .name("ristle-audio-out".into())
            .spawn(move || run(request, render, ready, received, reopen))
            .map_err(|e| OutputError::Backend(format!("output thread: {e}")))?;
        match opened.recv() {
            Ok(Ok(info)) => {
                self.worker = Some(Worker { commands, thread });
                Ok(info)
            }
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => {
                let _ = thread.join();
                Err(OutputError::Backend(
                    "output thread ended while opening".into(),
                ))
            }
        }
    }

    fn pause(&mut self) {
        self.send(Command::Pause);
    }

    fn resume(&mut self) {
        self.send(Command::Resume);
    }

    fn close(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.commands.send(Command::Close);
            if worker.thread.join().is_err() {
                tracing::error!("audio output thread panicked");
            }
        }
    }
}

impl Drop for AAudioOutput {
    fn drop(&mut self) {
        self.close();
    }
}

fn run(
    request: OutputRequest,
    render: Box<dyn Render>,
    ready: mpsc::SyncSender<Result<OutputInfo, OutputError>>,
    commands: mpsc::Receiver<Command>,
    reopen: mpsc::Sender<Command>,
) {
    let render: SharedRender = Arc::new(Mutex::new(render));
    let errors = request.errors.clone();
    let requested = Settings {
        sample_rate: request
            .preferred_rate
            .and_then(|rate| i32::try_from(rate).ok()),
        channels: i32::from(request.channels.max(1)),
    };
    let (mut stream, settings) = match open_stream(requested, &render, &errors, &reopen) {
        Ok((stream, info)) => {
            let settings = Settings {
                sample_rate: i32::try_from(info.sample_rate).ok(),
                channels: i32::from(info.channels),
            };
            let _ = ready.send(Ok(info));
            (Some(stream), settings)
        }
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let mut running = false;
    while let Ok(command) = commands.recv() {
        let result = match command {
            Command::Pause => {
                running = false;
                stream.as_ref().map_or(Ok(()), AudioStream::request_pause)
            }
            Command::Resume => {
                running = true;
                stream.as_ref().map_or(Ok(()), AudioStream::request_start)
            }
            Command::Reopen => {
                // Close the disconnected stream first: its callbacks stop
                // before the new stream's start.
                drop(stream.take());
                match open_stream(settings, &render, &errors, &reopen) {
                    Ok((new, info)) => {
                        tracing::info!(rate = info.sample_rate, "audio output reopened");
                        stream = Some(new);
                        if running {
                            stream.as_ref().map_or(Ok(()), AudioStream::request_start)
                        } else {
                            Ok(())
                        }
                    }
                    Err(error) => {
                        errors.report(error);
                        continue;
                    }
                }
            }
            Command::Close => break,
        };
        if let Err(error) = result {
            errors.report(OutputError::Backend(error.to_string()));
        }
    }
    if let Some(stream) = stream.take() {
        let _ = stream.request_stop();
    }
}

fn open_stream(
    settings: Settings,
    render: &SharedRender,
    errors: &OutputErrors,
    reopen: &mpsc::Sender<Command>,
) -> Result<(AudioStream, OutputInfo), OutputError> {
    let channels = settings.channels;
    let samples_per_frame = usize::try_from(channels).unwrap_or(1).max(1);
    let callback_render = Arc::clone(render);
    let errors = errors.clone();
    let reopen = reopen.clone();
    let mut builder = AudioStreamBuilder::new()
        .map_err(aaudio)?
        .direction(AudioDirection::Output)
        .performance_mode(AudioPerformanceMode::PowerSaving)
        .content_type(AudioContentType::Music)
        .usage(AudioUsage::Media)
        .sharing_mode(AudioSharingMode::Shared)
        .format(AudioFormat::PCM_Float)
        .channel_count(channels)
        .data_callback(Box::new(move |_stream, data, frames| {
            let Ok(frames) = usize::try_from(frames) else {
                return AudioCallbackResult::Continue;
            };
            // SAFETY: AAudio hands the callback a buffer of `frames` frames
            // in the stream's format, which was opened as interleaved f32
            // with `samples_per_frame` channels; it is valid and exclusively
            // ours until the callback returns.
            let out = unsafe {
                std::slice::from_raw_parts_mut(data.cast::<f32>(), frames * samples_per_frame)
            };
            match callback_render.try_lock() {
                Ok(mut render) => render.render(out),
                Err(_) => out.fill(0.0),
            }
            AudioCallbackResult::Continue
        }))
        .error_callback(Box::new(move |_stream, error| {
            // AAudio forbids closing or reopening from this callback; the
            // output thread does it.
            if error == AudioError::Disconnected {
                let _ = reopen.send(Command::Reopen);
            } else {
                errors.report(OutputError::Backend(error.to_string()));
            }
        }));
    if let Some(rate) = settings.sample_rate {
        builder = builder.sample_rate(rate);
    }
    let stream = builder.open_stream().map_err(aaudio)?;
    if stream.format() != AudioFormat::PCM_Float || stream.channel_count() != channels {
        return Err(OutputError::Unsupported(format!(
            "AAudio opened {:?} with {} channels",
            stream.format(),
            stream.channel_count()
        )));
    }
    let info = OutputInfo {
        sample_rate: u32::try_from(stream.sample_rate())
            .map_err(|_| OutputError::Backend("AAudio reported a negative rate".into()))?,
        channels: u16::try_from(channels).unwrap_or(2),
    };
    tracing::info!(
        rate = info.sample_rate,
        burst = stream.frames_per_burst(),
        buffer = stream.buffer_size_in_frames(),
        performance = ?stream.performance_mode(),
        "audio output opened"
    );
    Ok((stream, info))
}

fn aaudio(error: AudioError) -> OutputError {
    match error {
        AudioError::Unavailable | AudioError::NoService => OutputError::NoDevice,
        AudioError::InvalidFormat | AudioError::InvalidRate => {
            OutputError::Unsupported(error.to_string())
        }
        _ => OutputError::Backend(error.to_string()),
    }
}

/// What the stream actually got, for the on-device tests.
#[cfg(feature = "test-support")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opened {
    /// The performance mode AAudio granted.
    pub performance_mode: String,
    /// The content type it kept.
    pub content_type: String,
    /// The usage it kept.
    pub usage: String,
    /// Frames per data callback burst.
    pub frames_per_burst: i32,
}

/// Opens and closes one stream with the output's settings and reports what
/// AAudio granted.
#[cfg(feature = "test-support")]
pub fn probe() -> Result<Opened, OutputError> {
    struct Silence;
    impl Render for Silence {
        fn render(&mut self, out: &mut [f32]) {
            out.fill(0.0);
        }
    }
    let render: SharedRender = Arc::new(Mutex::new(Box::new(Silence)));
    let (reopen, _) = mpsc::channel();
    let settings = Settings {
        sample_rate: None,
        channels: 2,
    };
    let (stream, _) = open_stream(settings, &render, &OutputErrors::new(|_| {}), &reopen)?;
    Ok(Opened {
        performance_mode: format!("{:?}", stream.performance_mode()),
        content_type: format!("{:?}", stream.content_type()),
        usage: format!("{:?}", stream.usage()),
        frames_per_burst: stream.frames_per_burst(),
    })
}
