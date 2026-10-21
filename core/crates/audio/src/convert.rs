//! Stream PCM to output PCM: channel mapping, then resampling to the output
//! rate (ADR 0004: the core resamples, so the platform mixer does not).
//!
//! Each format run of a stream gets its own converter. `finish` flushes the
//! resampler and trims its delay, so a run of `n` input frames always yields
//! `round(n * out_rate / in_rate)` output frames. That keeps Gapless
//! Transitions sample-exact after resampling: the next Track's first frame
//! lands right after this one's last.

use rubato::{FftFixedIn, Resampler};

use crate::stream::StreamFormat;

/// Input frames per resampler chunk.
const CHUNK_FRAMES: usize = 1024;

pub(crate) struct Converter {
    input: StreamFormat,
    out_channels: usize,
    mapped: Vec<f32>,
    resampler: Option<Resample>,
}

impl Converter {
    pub(crate) fn new(
        input: StreamFormat,
        out_rate: u32,
        out_channels: usize,
    ) -> Result<Self, String> {
        if input.sample_rate == 0 || input.channels == 0 {
            return Err(format!(
                "unusable stream format: {} Hz, {} channels",
                input.sample_rate, input.channels
            ));
        }
        let resampler = if input.sample_rate == out_rate {
            None
        } else {
            Some(Resample::new(input.sample_rate, out_rate, out_channels)?)
        };
        Ok(Self {
            input,
            out_channels,
            mapped: Vec::new(),
            resampler,
        })
    }

    pub(crate) fn input(&self) -> StreamFormat {
        self.input
    }

    /// Converts interleaved input frames and appends output samples to `out`.
    pub(crate) fn push(&mut self, input: &[f32], out: &mut Vec<f32>) -> Result<(), String> {
        let in_channels = usize::from(self.input.channels);
        match self.resampler.as_mut() {
            None => {
                map_channels(input, in_channels, self.out_channels, out);
                Ok(())
            }
            Some(resample) => {
                self.mapped.clear();
                map_channels(input, in_channels, self.out_channels, &mut self.mapped);
                resample.push(&self.mapped, out)
            }
        }
    }

    /// Flushes what the resampler still holds. Call once at the end of a
    /// format run.
    pub(crate) fn finish(&mut self, out: &mut Vec<f32>) -> Result<(), String> {
        match self.resampler.as_mut() {
            None => Ok(()),
            Some(resample) => resample.finish(out),
        }
    }
}

/// Maps interleaved frames between channel counts. Mono is copied to every
/// output channel. Otherwise channels are taken in order and missing ones are
/// silent: streams do not report their channel layout, and the first two
/// channels are front left and right in every layout Symphonia and the
/// platform decoders produce.
fn map_channels(input: &[f32], in_channels: usize, out_channels: usize, out: &mut Vec<f32>) {
    if in_channels == out_channels {
        out.extend_from_slice(input);
        return;
    }
    out.reserve(input.len() / in_channels * out_channels);
    for frame in input.chunks_exact(in_channels) {
        for channel in 0..out_channels {
            let sample = if in_channels == 1 {
                frame[0]
            } else {
                frame.get(channel).copied().unwrap_or(0.0)
            };
            out.push(sample);
        }
    }
}

struct Resample {
    inner: FftFixedIn<f32>,
    channels: usize,
    in_rate: u64,
    out_rate: u64,
    pending: Vec<Vec<f32>>,
    planar_out: Vec<Vec<f32>>,
    /// Output frames still to drop for the resampler's delay.
    delay_left: usize,
    frames_in: u64,
    frames_out: u64,
}

impl Resample {
    fn new(in_rate: u32, out_rate: u32, channels: usize) -> Result<Self, String> {
        let inner = FftFixedIn::<f32>::new(
            in_rate as usize,
            out_rate as usize,
            CHUNK_FRAMES,
            2,
            channels,
        )
        .map_err(|e| format!("resampler {in_rate} Hz to {out_rate} Hz: {e}"))?;
        let delay_left = inner.output_delay();
        let planar_out = inner.output_buffer_allocate(true);
        Ok(Self {
            inner,
            channels,
            in_rate: u64::from(in_rate),
            out_rate: u64::from(out_rate),
            pending: vec![Vec::with_capacity(CHUNK_FRAMES * 2); channels],
            planar_out,
            delay_left,
            frames_in: 0,
            frames_out: 0,
        })
    }

    fn push(&mut self, input: &[f32], out: &mut Vec<f32>) -> Result<(), String> {
        for frame in input.chunks_exact(self.channels) {
            for (channel, sample) in frame.iter().enumerate() {
                self.pending[channel].push(*sample);
            }
        }
        self.frames_in += (input.len() / self.channels) as u64;
        loop {
            let needed = self.inner.input_frames_next();
            if self.pending[0].len() < needed {
                return Ok(());
            }
            let (used, produced) = self
                .inner
                .process_into_buffer(&self.pending, &mut self.planar_out, None)
                .map_err(|e| format!("resampling: {e}"))?;
            for channel in &mut self.pending {
                channel.drain(..used);
            }
            self.emit(produced, u64::MAX, out);
        }
    }

    fn finish(&mut self, out: &mut Vec<f32>) -> Result<(), String> {
        let expected = (self.frames_in * self.out_rate + self.in_rate / 2) / self.in_rate;
        let mut rest = Some(std::mem::take(&mut self.pending));
        while self.frames_out < expected {
            let (_, produced) = self
                .inner
                .process_partial_into_buffer(rest.as_deref(), &mut self.planar_out, None)
                .map_err(|e| format!("resampling: {e}"))?;
            rest = None;
            if produced == 0 {
                break;
            }
            self.emit(produced, expected, out);
        }
        self.pending = vec![Vec::new(); self.channels];
        Ok(())
    }

    /// Interleaves `produced` frames into `out`, after the delay and up to
    /// `cap` frames in total.
    fn emit(&mut self, produced: usize, cap: u64, out: &mut Vec<f32>) {
        let skip = self.delay_left.min(produced);
        self.delay_left -= skip;
        let room = cap.saturating_sub(self.frames_out);
        let frames = ((produced - skip) as u64).min(room) as usize;
        out.reserve(frames * self.channels);
        for index in skip..skip + frames {
            for channel in &self.planar_out {
                out.push(channel[index]);
            }
        }
        self.frames_out += frames as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format(sample_rate: u32, channels: u16) -> StreamFormat {
        StreamFormat {
            sample_rate,
            channels,
        }
    }

    #[test]
    fn same_rate_is_passed_through_exactly() {
        let mut converter = Converter::new(format(44_100, 2), 44_100, 2).unwrap();
        let input: Vec<f32> = (0..64).map(|i| i as f32 / 64.0).collect();
        let mut out = Vec::new();
        converter.push(&input, &mut out).unwrap();
        converter.finish(&mut out).unwrap();
        assert_eq!(out, input);
    }

    #[test]
    fn mono_is_copied_to_both_channels_and_surround_keeps_the_front_pair() {
        let mut out = Vec::new();
        map_channels(&[0.1, 0.2], 1, 2, &mut out);
        assert_eq!(out, [0.1, 0.1, 0.2, 0.2]);
        out.clear();
        map_channels(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 6, 2, &mut out);
        assert_eq!(out, [1.0, 2.0]);
    }

    #[test]
    fn resampled_length_is_exact_for_any_split() {
        for (in_rate, out_rate, frames) in [
            (44_100, 48_000, 44_100),
            (48_000, 44_100, 12_345),
            (22_050, 48_000, 999),
            (44_100, 48_000, 7),
        ] {
            let mut converter = Converter::new(format(in_rate, 2), out_rate, 2).unwrap();
            let input = vec![0.25; frames * 2];
            let mut out = Vec::new();
            for chunk in input.chunks(2 * 333) {
                converter.push(chunk, &mut out).unwrap();
            }
            converter.finish(&mut out).unwrap();
            let expected =
                (frames as u64 * u64::from(out_rate) + u64::from(in_rate) / 2) / u64::from(in_rate);
            assert_eq!(
                out.len() as u64,
                expected * 2,
                "{in_rate} -> {out_rate}, {frames} frames"
            );
        }
    }

    #[test]
    fn resampling_keeps_level_and_alignment() {
        // A 1 kHz sine at 44.1 kHz, resampled to 48 kHz, must still be a
        // 1 kHz sine starting at phase zero (the delay is trimmed).
        let in_rate = 44_100.0_f64;
        let out_rate = 48_000.0_f64;
        let input: Vec<f32> = (0..44_100)
            .map(|i| (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / in_rate).sin() as f32 * 0.5)
            .collect();
        let mut converter = Converter::new(format(44_100, 1), 48_000, 1).unwrap();
        let mut out = Vec::new();
        converter.push(&input, &mut out).unwrap();
        converter.finish(&mut out).unwrap();
        assert_eq!(out.len(), 48_000);
        let worst = out[2_000..46_000]
            .iter()
            .enumerate()
            .map(|(i, sample)| {
                let t = (i + 2_000) as f64 / out_rate;
                let expected = (2.0 * std::f64::consts::PI * 1000.0 * t).sin() * 0.5;
                (f64::from(*sample) - expected).abs()
            })
            .fold(0.0, f64::max);
        assert!(worst < 1e-3, "worst deviation {worst}");
    }
}
