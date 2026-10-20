//! The desktop output, through cpal (WASAPI, Core Audio, ALSA).
//!
//! `cpal::Stream` is not `Send` on every host, so the stream lives on its
//! own thread for its whole life, and `CpalOutput` drives it over a channel.
//! The stream follows the system's default device where the host supports
//! it; a device that disappears without a replacement is reported through
//! `OutputErrors`.

use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use ::cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ::cpal::{
    ErrorKind, FromSample, SampleFormat, SizedSample, StreamConfig, SupportedStreamConfig,
    SupportedStreamConfigRange,
};

use super::{AudioOutput, OutputError, OutputErrors, OutputInfo, OutputRequest, Render};

/// Samples converted per render call when the device is not `f32`. The
/// buffer is allocated before the stream starts.
const SCRATCH_SAMPLES: usize = 8192;

/// The default output device, through cpal.
#[derive(Default)]
pub struct CpalOutput {
    worker: Option<Worker>,
}

struct Worker {
    commands: mpsc::Sender<Command>,
    thread: JoinHandle<()>,
}

enum Command {
    Pause,
    Resume,
    Close,
}

impl CpalOutput {
    /// An output that opens the system's default device.
    pub fn new() -> Self {
        Self::default()
    }

    fn send(&self, command: Command) {
        if let Some(worker) = &self.worker {
            // A closed channel means the thread already ended; nothing to do.
            let _ = worker.commands.send(command);
        }
    }
}

impl AudioOutput for CpalOutput {
    fn open(
        &mut self,
        request: OutputRequest,
        render: Box<dyn Render>,
    ) -> Result<OutputInfo, OutputError> {
        self.close();
        let (ready, opened) = mpsc::sync_channel(1);
        let (commands, received) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("ristle-audio-out".into())
            .spawn(move || run(request, render, ready, received))
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

impl Drop for CpalOutput {
    fn drop(&mut self) {
        self.close();
    }
}

fn run(
    request: OutputRequest,
    render: Box<dyn Render>,
    ready: mpsc::SyncSender<Result<OutputInfo, OutputError>>,
    commands: mpsc::Receiver<Command>,
) {
    let errors = request.errors.clone();
    let stream = match open_stream(&request, render) {
        Ok((stream, info)) => {
            let _ = ready.send(Ok(info));
            stream
        }
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    while let Ok(command) = commands.recv() {
        let result = match command {
            Command::Pause => match stream.pause() {
                // Some ALSA hardware cannot pause. The stream keeps
                // running and the engine renders silence while paused.
                Err(error) if error.kind() == ErrorKind::UnsupportedOperation => Ok(()),
                result => result,
            },
            Command::Resume => stream.play(),
            Command::Close => break,
        };
        if let Err(error) = result {
            errors.report(OutputError::Backend(error.to_string()));
        }
    }
}

fn open_stream(
    request: &OutputRequest,
    render: Box<dyn Render>,
) -> Result<(::cpal::Stream, OutputInfo), OutputError> {
    let host = ::cpal::default_host();
    let device = host.default_output_device().ok_or(OutputError::NoDevice)?;
    let default = device.default_output_config().map_err(backend)?;
    let supported: Vec<_> = device
        .supported_output_configs()
        .map_err(backend)?
        .collect();
    let chosen = choose_config(&default, &supported, request);
    let config = StreamConfig {
        channels: chosen.channels(),
        sample_rate: chosen.sample_rate(),
        buffer_size: ::cpal::BufferSize::Default,
    };
    let info = OutputInfo {
        sample_rate: config.sample_rate,
        channels: config.channels,
    };
    let errors = request.errors.clone();
    let stream = match chosen.sample_format() {
        SampleFormat::F32 => build_f32(&device, &config, render, errors),
        SampleFormat::F64 => build::<f64>(&device, &config, render, errors),
        SampleFormat::I16 => build::<i16>(&device, &config, render, errors),
        SampleFormat::I32 => build::<i32>(&device, &config, render, errors),
        SampleFormat::U16 => build::<u16>(&device, &config, render, errors),
        SampleFormat::I8 => build::<i8>(&device, &config, render, errors),
        SampleFormat::U8 => build::<u8>(&device, &config, render, errors),
        other => Err(OutputError::Unsupported(format!("sample format {other}"))),
    }?;
    // cpal 0.18 returns built streams stopped, which is how ours start.
    Ok((stream, info))
}

/// Sample formats the output renders, best first.
const FORMATS: [SampleFormat; 7] = [
    SampleFormat::F32,
    SampleFormat::I32,
    SampleFormat::I16,
    SampleFormat::F64,
    SampleFormat::U16,
    SampleFormat::I8,
    SampleFormat::U8,
];

/// Picks the stream configuration. The device's default sample format and
/// rate come first (the mixer's own, so the platform does not resample);
/// the requested channel count and rate where the device supports them in
/// that format. A default format the output cannot render is replaced by
/// the best one the device offers.
fn choose_config(
    default: &SupportedStreamConfig,
    supported: &[SupportedStreamConfigRange],
    request: &OutputRequest,
) -> SupportedStreamConfig {
    let format = if FORMATS.contains(&default.sample_format()) {
        default.sample_format()
    } else {
        FORMATS
            .into_iter()
            .find(|format| {
                supported
                    .iter()
                    .any(|range| range.sample_format() == *format)
            })
            .unwrap_or(default.sample_format())
    };
    let in_format = |channels: u16| -> Vec<SupportedStreamConfigRange> {
        supported
            .iter()
            .filter(|range| range.sample_format() == format && range.channels() == channels)
            .copied()
            .collect()
    };
    let mut ranges = in_format(request.channels);
    if ranges.is_empty() {
        ranges = in_format(default.channels());
    }
    if ranges.is_empty() {
        ranges = supported
            .iter()
            .filter(|range| range.sample_format() == format)
            .copied()
            .collect();
    }
    // The preferred rate if this layout supports it, then the default rate,
    // then the supported rate nearest the default.
    let rates = request
        .preferred_rate
        .into_iter()
        .chain([default.sample_rate()]);
    for rate in rates {
        if let Some(config) = ranges
            .iter()
            .find_map(|range| (*range).try_with_sample_rate(rate))
        {
            return config;
        }
    }
    ranges
        .into_iter()
        .map(|range| {
            let rate = default
                .sample_rate()
                .clamp(range.min_sample_rate(), range.max_sample_rate());
            SupportedStreamConfig::new(
                range.channels(),
                rate,
                *range.buffer_size(),
                range.sample_format(),
            )
        })
        .min_by_key(|config| config.sample_rate().abs_diff(default.sample_rate()))
        .unwrap_or(*default)
}

/// A device that takes `f32` gets the engine's buffer directly.
fn build_f32(
    device: &::cpal::Device,
    config: &StreamConfig,
    mut render: Box<dyn Render>,
    errors: OutputErrors,
) -> Result<::cpal::Stream, OutputError> {
    device
        .build_output_stream(
            *config,
            move |out: &mut [f32], _: &::cpal::OutputCallbackInfo| render.render(out),
            move |error: ::cpal::Error| report(&errors, &error),
            None,
        )
        .map_err(backend)
}

/// Any other sample format renders into preallocated `f32` and converts.
fn build<T>(
    device: &::cpal::Device,
    config: &StreamConfig,
    mut render: Box<dyn Render>,
    errors: OutputErrors,
) -> Result<::cpal::Stream, OutputError>
where
    T: SizedSample + FromSample<f32> + Send + 'static,
{
    let channels = usize::from(config.channels).max(1);
    // Whole frames per pass.
    let mut scratch = vec![0.0_f32; (SCRATCH_SAMPLES / channels).max(1) * channels];
    device
        .build_output_stream(
            *config,
            move |out: &mut [T], _: &::cpal::OutputCallbackInfo| {
                for chunk in out.chunks_mut(scratch.len()) {
                    let scratch = &mut scratch[..chunk.len()];
                    render.render(scratch);
                    for (sample, value) in chunk.iter_mut().zip(scratch.iter()) {
                        *sample = T::from_sample(*value);
                    }
                }
            },
            move |error: ::cpal::Error| report(&errors, &error),
            None,
        )
        .map_err(backend)
}

fn report(errors: &OutputErrors, error: &::cpal::Error) {
    match error.kind() {
        // An xrun is a glitch, not a failure; a reroute to the new default
        // device is handled by the host.
        ErrorKind::Xrun | ErrorKind::DeviceChanged | ErrorKind::RealtimeDenied => {}
        _ => errors.report(OutputError::Backend(error.to_string())),
    }
}

fn backend(error: ::cpal::Error) -> OutputError {
    match error.kind() {
        ErrorKind::DeviceNotAvailable | ErrorKind::HostUnavailable => OutputError::NoDevice,
        ErrorKind::UnsupportedConfig => OutputError::Unsupported(error.to_string()),
        _ => OutputError::Backend(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::cpal::SupportedBufferSize;

    fn range(
        channels: u16,
        min: u32,
        max: u32,
        format: SampleFormat,
    ) -> SupportedStreamConfigRange {
        SupportedStreamConfigRange::new(channels, min, max, SupportedBufferSize::Unknown, format)
    }

    fn request(preferred_rate: Option<u32>) -> OutputRequest {
        OutputRequest {
            preferred_rate,
            channels: 2,
            errors: OutputErrors::new(|_| {}),
        }
    }

    fn default(channels: u16, rate: u32) -> SupportedStreamConfig {
        SupportedStreamConfig::new(
            channels,
            rate,
            SupportedBufferSize::Unknown,
            SampleFormat::F32,
        )
    }

    #[test]
    fn takes_the_device_rate_and_stereo() {
        let supported = [
            range(2, 44_100, 192_000, SampleFormat::F32),
            range(6, 48_000, 48_000, SampleFormat::F32),
        ];
        let chosen = choose_config(&default(6, 48_000), &supported, &request(None));
        assert_eq!((chosen.channels(), chosen.sample_rate()), (2, 48_000));
    }

    #[test]
    fn honours_a_supported_preferred_rate_only() {
        let supported = [range(2, 44_100, 96_000, SampleFormat::F32)];
        let chosen = choose_config(&default(2, 48_000), &supported, &request(Some(44_100)));
        assert_eq!(chosen.sample_rate(), 44_100);
        let chosen = choose_config(&default(2, 48_000), &supported, &request(Some(8_000)));
        assert_eq!(chosen.sample_rate(), 48_000);
    }

    #[test]
    fn checks_the_preferred_rate_against_the_requested_layout() {
        // Stereo only at 44.1 kHz; the 6-channel default runs at 48 kHz.
        let supported = [
            range(6, 48_000, 48_000, SampleFormat::F32),
            range(2, 44_100, 44_100, SampleFormat::F32),
        ];
        let chosen = choose_config(&default(6, 48_000), &supported, &request(Some(44_100)));
        assert_eq!((chosen.channels(), chosen.sample_rate()), (2, 44_100));
        // Without a preference, stereo still wins, at its nearest rate.
        let chosen = choose_config(&default(6, 48_000), &supported, &request(None));
        assert_eq!((chosen.channels(), chosen.sample_rate()), (2, 44_100));
    }

    #[test]
    fn replaces_a_default_format_it_cannot_render() {
        let supported = [
            range(2, 48_000, 48_000, SampleFormat::I24),
            range(2, 48_000, 48_000, SampleFormat::I16),
        ];
        let default =
            SupportedStreamConfig::new(2, 48_000, SupportedBufferSize::Unknown, SampleFormat::I24);
        let chosen = choose_config(&default, &supported, &request(None));
        assert_eq!(
            (chosen.sample_format(), chosen.sample_rate()),
            (SampleFormat::I16, 48_000)
        );
    }

    #[test]
    fn falls_back_to_the_default_layout() {
        let supported = [
            range(8, 48_000, 48_000, SampleFormat::F32),
            range(2, 48_000, 48_000, SampleFormat::I16),
        ];
        let chosen = choose_config(&default(8, 48_000), &supported, &request(None));
        assert_eq!(
            (chosen.channels(), chosen.sample_format()),
            (8, SampleFormat::F32)
        );
    }
}
