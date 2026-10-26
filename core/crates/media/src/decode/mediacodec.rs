//! Android MediaCodec as a [`DecoderProvider`]: the platform decoder for
//! HE-AAC (ADR 0004 amendment 1). Symphonia has no SBR or Parametric Stereo;
//! every Android device ships Codec2's FDK-based AAC decoder
//! (`c2.android.aac.decoder`), which runs in the media codec service
//! process, so its CPU is not on our threads.
//!
//! Contract details (spike B, measured on a Mi 9T):
//! - Codec2 compensates its own decoder delay, so output lines up with the
//!   input timeline frame for frame, as the [`Decoder`] contract requires.
//! - It holds several packets in flight; [`Decoder::finish`] queues end of
//!   stream and drains them, or the end of every Track is lost.
//! - Implicit SBR doubles the rate after the first frames, reported through
//!   an output format change, which [`Decoder::format`] follows.

use std::time::{Duration, Instant};

use ndk::media::media_codec::{
    DequeuedInputBufferResult, DequeuedOutputBufferInfoResult, MediaCodec, MediaCodecDirection,
};
use ndk::media::media_format::MediaFormat;

use super::{Decoder, DecoderProvider, Packet, Support};
use crate::codec::{Codec, CodecDescription, HeAac};
use crate::{Error, StreamFormat};

const MIME_AAC: &str = "audio/mp4a-latm";
const BUFFER_FLAG_END_OF_STREAM: u32 = 4;
const ENCODING_PCM_16BIT: i32 = 2;
const ENCODING_PCM_FLOAT: i32 = 4;
/// How long `finish` waits for the codec to give back its last frames.
const FINISH_TIMEOUT: Duration = Duration::from_secs(2);

/// MediaCodec for AAC.
#[derive(Debug, Clone, Copy)]
pub struct MediaCodecProvider {
    all_aac: bool,
}

impl MediaCodecProvider {
    /// Claims HE-AAC only: plain AAC-LC costs about twenty times
    /// Symphonia's CPU through MediaCodec (spike B).
    pub fn new() -> Self {
        Self { all_aac: false }
    }

    /// Claims every AAC stream. For tests that check the provider against
    /// Symphonia on AAC-LC fixtures, since no HE-AAC encoder is GPL-usable.
    pub fn for_all_aac() -> Self {
        Self { all_aac: true }
    }
}

impl Default for MediaCodecProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl DecoderProvider for MediaCodecProvider {
    fn name(&self) -> &'static str {
        "mediacodec"
    }

    fn supports(&self, codec: &CodecDescription) -> Support {
        if codec.codec != Codec::Aac {
            return Support::No;
        }
        if self.all_aac || codec.he_aac() != HeAac::No {
            Support::Full
        } else {
            Support::No
        }
    }

    fn open(&self, codec: &CodecDescription) -> Result<Box<dyn Decoder>, Error> {
        Ok(Box::new(MediaCodecDecoder::open(codec)?))
    }
}

/// One AMediaCodec, used from one thread at a time.
struct MediaCodecDecoder {
    codec: MediaCodec,
    format: Option<StreamFormat>,
    encoding: i32,
    /// The codec returned end of stream; it needs a flush before new input.
    drained: bool,
}

// SAFETY: AMediaCodec has no thread affinity; its calls may come from any
// thread as long as they do not overlap, which `&mut self` on every
// `Decoder` method guarantees. The decoder moves to the decode thread once
// and is used only there.
#[allow(unsafe_code)]
unsafe impl Send for MediaCodecDecoder {}

/// The codec's output format and PCM encoding.
fn output_format(codec: &MediaCodec) -> (Option<StreamFormat>, i32) {
    let format = codec.output_format();
    let rate = format
        .i32("sample-rate")
        .and_then(|r| u32::try_from(r).ok());
    let channels = format
        .i32("channel-count")
        .and_then(|c| u16::try_from(c).ok());
    let stream = match (rate, channels) {
        (Some(sample_rate), Some(channels)) => Some(StreamFormat {
            sample_rate,
            channels,
        }),
        _ => None,
    };
    (
        stream,
        format.i32("pcm-encoding").unwrap_or(ENCODING_PCM_16BIT),
    )
}

fn failed(what: &str, error: impl std::fmt::Debug) -> Error {
    Error::Decoder {
        decoder: "mediacodec",
        detail: format!("{what}: {error:?}"),
    }
}

/// A 2-byte AAC-LC AudioSpecificConfig for streams without one (ADTS in TS,
/// packed audio always have one from their framer; this covers the rest).
fn lc_config(rate: u32, channels: u16) -> Result<[u8; 2], Error> {
    const RATES: [u32; 13] = [
        96_000, 88_200, 64_000, 48_000, 44_100, 32_000, 24_000, 22_050, 16_000, 12_000, 11_025,
        8_000, 7_350,
    ];
    let index = RATES
        .iter()
        .position(|&r| r == rate)
        .ok_or_else(|| Error::UnsupportedCodec(format!("AAC at {rate} Hz")))? as u8;
    let channels = u8::try_from(channels).unwrap_or(0) & 0x0f;
    Ok([
        (2 << 3) | (index >> 1),
        ((index & 1) << 7) | (channels << 3),
    ])
}

impl MediaCodecDecoder {
    fn open(description: &CodecDescription) -> Result<Self, Error> {
        let channels = description.channels.unwrap_or(2);
        // The configuration's own rate: the core rate for implicit SBR;
        // MediaCodec reports the real output rate through a format change.
        let rate = description
            .aac_core_rate()
            .or(description.sample_rate)
            .ok_or_else(|| Error::UnsupportedCodec("AAC without a sample rate".into()))?;
        let config = match description.extra_data.as_deref() {
            Some(asc) => asc.to_vec(),
            None => lc_config(rate, channels)?.to_vec(),
        };
        let codec = MediaCodec::from_decoder_type(MIME_AAC).ok_or_else(|| {
            Error::UnsupportedCodec(format!("no {MIME_AAC} decoder on this device"))
        })?;
        let mut format = MediaFormat::new();
        format.set_str("mime", MIME_AAC);
        format.set_i32("sample-rate", i32::try_from(rate).unwrap_or(44_100));
        format.set_i32("channel-count", i32::from(channels));
        format.set_buffer("csd-0", &config);
        format.set_i32("is-adts", 0);
        format.set_i32("pcm-encoding", ENCODING_PCM_FLOAT);
        codec
            .configure(&format, None, MediaCodecDirection::Decoder)
            .map_err(|e| failed("configure", e))?;
        codec.start().map_err(|e| failed("start", e))?;
        if let Ok(name) = codec.name() {
            tracing::debug!(%name, "MediaCodec decoder started");
        }
        Ok(Self {
            codec,
            format: None,
            encoding: ENCODING_PCM_16BIT,
            drained: false,
        })
    }

    /// Appends ready output. Returns true once end of stream came out.
    fn drain(&mut self, out: &mut Vec<f32>, timeout: Duration) -> Result<bool, Error> {
        loop {
            match self
                .codec
                .dequeue_output_buffer(timeout)
                .map_err(|e| failed("dequeue output", e))?
            {
                DequeuedOutputBufferInfoResult::TryAgainLater => return Ok(false),
                DequeuedOutputBufferInfoResult::OutputFormatChanged => {
                    (self.format, self.encoding) = output_format(&self.codec);
                }
                DequeuedOutputBufferInfoResult::OutputBuffersChanged => {}
                DequeuedOutputBufferInfoResult::Buffer(buffer) => {
                    if self.format.is_none() {
                        (self.format, self.encoding) = output_format(&self.codec);
                    }
                    let info = *buffer.info();
                    let start = usize::try_from(info.offset()).unwrap_or(0);
                    let end = start + usize::try_from(info.size()).unwrap_or(0);
                    let data = buffer.buffer().get(start..end).unwrap_or_default();
                    if self.encoding == ENCODING_PCM_FLOAT {
                        out.extend(
                            data.as_chunks::<4>()
                                .0
                                .iter()
                                .map(|b| f32::from_le_bytes(*b)),
                        );
                    } else {
                        out.extend(
                            data.as_chunks::<2>()
                                .0
                                .iter()
                                .map(|b| f32::from(i16::from_le_bytes(*b)) / 32_768.0),
                        );
                    }
                    let end_of_stream = info.flags() & BUFFER_FLAG_END_OF_STREAM != 0;
                    self.codec
                        .release_output_buffer(buffer, false)
                        .map_err(|e| failed("release", e))?;
                    if end_of_stream {
                        return Ok(true);
                    }
                }
            }
        }
    }

    fn queue(
        &mut self,
        data: &[u8],
        pts_us: u64,
        flags: u32,
        out: &mut Vec<f32>,
    ) -> Result<(), Error> {
        loop {
            match self
                .codec
                .dequeue_input_buffer(Duration::from_millis(5))
                .map_err(|e| failed("dequeue input", e))?
            {
                DequeuedInputBufferResult::Buffer(mut buffer) => {
                    let target = buffer.buffer_mut();
                    if target.len() < data.len() {
                        return Err(failed(
                            "queue",
                            format!("{} bytes in a {}-byte buffer", data.len(), target.len()),
                        ));
                    }
                    for (slot, byte) in target.iter_mut().zip(data) {
                        slot.write(*byte);
                    }
                    return self
                        .codec
                        .queue_input_buffer(buffer, 0, data.len(), pts_us, flags)
                        .map_err(|e| failed("queue", e));
                }
                // No free input buffer: make room by taking output.
                DequeuedInputBufferResult::TryAgainLater => {
                    self.drain(out, Duration::from_millis(5))?;
                }
            }
        }
    }
}

impl Decoder for MediaCodecDecoder {
    fn decode(&mut self, packet: &Packet<'_>, out: &mut Vec<f32>) -> Result<(), Error> {
        if self.drained {
            self.reset();
        }
        // MediaCodec wants microseconds; the codec does not depend on them.
        let rate = u64::from(self.format.map_or(44_100, |f| f.sample_rate).max(1));
        let pts_us = u64::try_from(packet.timestamp.max(0))
            .unwrap_or(0)
            .saturating_mul(1_000_000)
            / rate;
        self.queue(packet.data, pts_us, 0, out)?;
        self.drain(out, Duration::ZERO)?;
        Ok(())
    }

    fn format(&self) -> Option<StreamFormat> {
        self.format
    }

    fn reset(&mut self) {
        if let Err(error) = self.codec.flush() {
            tracing::warn!(?error, "flushing MediaCodec failed");
        }
        self.drained = false;
    }

    fn finish(&mut self, out: &mut Vec<f32>) -> Result<(), Error> {
        if self.drained {
            return Ok(());
        }
        self.queue(&[], 0, BUFFER_FLAG_END_OF_STREAM, out)?;
        let deadline = Instant::now() + FINISH_TIMEOUT;
        while !self.drain(out, Duration::from_millis(20))? {
            if Instant::now() > deadline {
                return Err(failed("finish", "no end of stream from the codec"));
            }
        }
        self.drained = true;
        Ok(())
    }
}

impl Drop for MediaCodecDecoder {
    fn drop(&mut self) {
        let _ = self.codec.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_an_lc_configuration_for_streams_without_one() {
        assert_eq!(lc_config(44_100, 2).unwrap(), [0x12, 0x10]);
        assert_eq!(lc_config(22_050, 1).unwrap(), [0x13, 0x88]);
        assert!(lc_config(44_000, 2).is_err());
    }

    #[test]
    fn claims_he_aac_and_leaves_plain_aac_to_symphonia() {
        let provider = MediaCodecProvider::new();
        let mut lc = CodecDescription::new(Codec::Aac);
        lc.extra_data = Some([0x12, 0x10].into());
        assert_eq!(provider.supports(&lc), Support::No);
        let mut implicit = CodecDescription::new(Codec::Aac);
        implicit.extra_data = Some([0x13, 0x90].into());
        assert_eq!(provider.supports(&implicit), Support::Full);
        assert_eq!(
            MediaCodecProvider::for_all_aac().supports(&lc),
            Support::Full
        );
        assert_eq!(
            provider.supports(&CodecDescription::new(Codec::Opus)),
            Support::No
        );
    }
}
