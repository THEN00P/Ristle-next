//! Android MediaCodec as a `DecoderProvider`: the platform fallback for
//! HE-AAC (ADR 0004). Fed demuxed AAC access units from Symphonia's readers,
//! with the AudioSpecificConfig as `csd-0`. The decode itself runs in the
//! codec service process (`media.swcodec` for Codec2 software codecs), so its
//! CPU is not on our threads; `remote_codec_ticks` reads it from /proc.

use crate::registry::{is_he_aac, Decoder, DecoderProvider, StreamInfo, Support};
use anyhow::{anyhow, bail, Result};
use ndk::media::media_codec::{DequeuedInputBufferResult, DequeuedOutputBufferInfoResult, MediaCodec, MediaCodecDirection};
use ndk::media::media_format::MediaFormat;
use std::time::Duration;
use symphonia::core::codecs::audio::well_known::CODEC_ID_AAC;
use symphonia::core::packet::Packet;

const MIME_AAC: &str = "audio/mp4a-latm";
const BUFFER_FLAG_END_OF_STREAM: u32 = 4;
const ENCODING_PCM_16BIT: i32 = 2;
const ENCODING_PCM_FLOAT: i32 = 4;

pub struct MediaCodecProvider {
    /// Claim every AAC stream, not only HE-AAC (for comparison runs).
    pub all_aac: bool,
}

impl DecoderProvider for MediaCodecProvider {
    fn name(&self) -> &'static str {
        "mediacodec"
    }
    fn support(&self, stream: &StreamInfo) -> Support {
        let aac = stream.params.codec == CODEC_ID_AAC;
        if aac && (self.all_aac || is_he_aac(stream)) {
            Support::Full
        } else {
            Support::No
        }
    }
    fn create(&self, stream: &StreamInfo) -> Result<Box<dyn Decoder>> {
        Ok(Box::new(MediaCodecDecoder::new(stream)?))
    }
}

pub struct MediaCodecDecoder {
    codec: MediaCodec,
    pub codec_name: String,
    rate: u32,
    channels: usize,
    encoding: i32,
}

// AMediaCodec is thread-safe for this use (one owner thread at a time).
unsafe impl Send for MediaCodecDecoder {}

/// Builds a 2-byte AAC-LC AudioSpecificConfig when the demuxer has none (ADTS).
fn asc_for(rate: u32, channels: usize) -> Result<[u8; 2]> {
    const RATES: [u32; 13] = [96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350];
    let idx = RATES.iter().position(|r| *r == rate).ok_or_else(|| anyhow!("no ASC index for {rate} Hz"))? as u8;
    let aot = 2u8;
    Ok([(aot << 3) | (idx >> 1), ((idx & 1) << 7) | ((channels as u8) << 3)])
}

impl MediaCodecDecoder {
    pub fn new(stream: &StreamInfo) -> Result<Self> {
        let p = &stream.params;
        let channels = p.channels.as_ref().map_or(2, |c| c.count());
        let asc: Vec<u8> = match &p.extra_data {
            Some(e) => e.to_vec(),
            None => asc_for(p.sample_rate.ok_or_else(|| anyhow!("no sample rate"))?, channels)?.to_vec(),
        };
        // The ASC's own rate (the core rate for implicit SBR); MediaCodec
        // reports the real output rate through a format change.
        let codec = MediaCodec::from_decoder_type(MIME_AAC).ok_or_else(|| anyhow!("no {MIME_AAC} decoder"))?;
        let mut fmt = MediaFormat::new();
        fmt.set_str("mime", MIME_AAC);
        fmt.set_i32("sample-rate", p.sample_rate.unwrap_or(44100) as i32);
        fmt.set_i32("channel-count", channels as i32);
        fmt.set_buffer("csd-0", &asc);
        fmt.set_i32("is-adts", 0);
        fmt.set_i32("pcm-encoding", ENCODING_PCM_FLOAT);
        codec.configure(&fmt, None, MediaCodecDirection::Decoder).map_err(|e| anyhow!("configure: {e:?}"))?;
        codec.start().map_err(|e| anyhow!("start: {e:?}"))?;
        // AMediaCodec_getName needs API 28; the build targets 26. The codec
        // in use shows in `dumpsys media.player` / logcat instead.
        let codec_name = String::from(MIME_AAC);
        Ok(Self { codec, codec_name, rate: p.sample_rate.unwrap_or(44100), channels, encoding: ENCODING_PCM_16BIT })
    }

    fn read_format(&mut self) {
        let f = self.codec.output_format();
        if let Some(r) = f.i32("sample-rate") {
            self.rate = r as u32;
        }
        if let Some(c) = f.i32("channel-count") {
            self.channels = c as usize;
        }
        self.encoding = f.i32("pcm-encoding").unwrap_or(ENCODING_PCM_16BIT);
    }

    /// Drains available output. Returns true once end of stream came out.
    fn drain(&mut self, out: &mut Vec<f32>, timeout: Duration) -> Result<bool> {
        loop {
            match self.codec.dequeue_output_buffer(timeout).map_err(|e| anyhow!("dequeue output: {e:?}"))? {
                DequeuedOutputBufferInfoResult::TryAgainLater => return Ok(false),
                DequeuedOutputBufferInfoResult::OutputFormatChanged => self.read_format(),
                DequeuedOutputBufferInfoResult::OutputBuffersChanged => {}
                DequeuedOutputBufferInfoResult::Buffer(buf) => {
                    let info = *buf.info();
                    let data = &buf.buffer()[info.offset() as usize..(info.offset() + info.size()) as usize];
                    if self.encoding == ENCODING_PCM_FLOAT {
                        out.extend(data.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])));
                    } else {
                        out.extend(data.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0));
                    }
                    let eos = info.flags() & BUFFER_FLAG_END_OF_STREAM != 0;
                    self.codec.release_output_buffer(buf, false).map_err(|e| anyhow!("release: {e:?}"))?;
                    if eos {
                        return Ok(true);
                    }
                }
            }
        }
    }

    fn queue(&mut self, data: &[u8], pts_us: u64, flags: u32, out: &mut Vec<f32>) -> Result<()> {
        loop {
            match self.codec.dequeue_input_buffer(Duration::from_millis(5)).map_err(|e| anyhow!("dequeue input: {e:?}"))? {
                DequeuedInputBufferResult::Buffer(mut b) => {
                    let dst = b.buffer_mut();
                    if dst.len() < data.len() {
                        bail!("input buffer too small");
                    }
                    for (d, s) in dst.iter_mut().zip(data) {
                        d.write(*s);
                    }
                    self.codec.queue_input_buffer(b, 0, data.len(), pts_us, flags).map_err(|e| anyhow!("queue: {e:?}"))?;
                    return Ok(());
                }
                // No free input: make room by draining output.
                DequeuedInputBufferResult::TryAgainLater => {
                    self.drain(out, Duration::from_millis(5))?;
                }
            }
        }
    }
}

impl Decoder for MediaCodecDecoder {
    fn decode(&mut self, packet: &Packet, out: &mut Vec<f32>) -> Result<()> {
        let pts_us = (packet.pts.get().max(0) as u64) * 1_000_000 / self.rate.max(1) as u64;
        self.queue(&packet.data, pts_us, 0, out)?;
        self.drain(out, Duration::ZERO)?;
        Ok(())
    }
    fn output(&self) -> (u32, usize) {
        (self.rate, self.channels)
    }
    fn reset(&mut self) {
        let _ = self.codec.flush();
    }
    fn finish(&mut self, out: &mut Vec<f32>) -> Result<()> {
        self.queue(&[], 0, BUFFER_FLAG_END_OF_STREAM, out)?;
        while !self.drain(out, Duration::from_millis(20))? {}
        Ok(())
    }
}

impl Drop for MediaCodecDecoder {
    fn drop(&mut self) {
        let _ = self.codec.stop();
    }
}

/// CPU ticks (utime + stime) of the codec service processes, by name.
pub fn remote_codec_ticks() -> Vec<(String, u64)> {
    let mut v = Vec::new();
    let Ok(dir) = std::fs::read_dir("/proc") else { return v };
    for e in dir.flatten() {
        let name = e.file_name();
        let Some(pid) = name.to_str().filter(|n| n.bytes().all(|b| b.is_ascii_digit())) else { continue };
        let Ok(cmd) = std::fs::read(format!("/proc/{pid}/cmdline")) else { continue };
        let cmd = String::from_utf8_lossy(cmd.split(|b| *b == 0).next().unwrap_or(&[])).to_string();
        if !(cmd.contains("media.swcodec") || cmd.contains("media.codec") || cmd.contains("media.c2") || cmd.contains("c2@")) {
            continue;
        }
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else { continue };
        let after = stat.rsplit_once(") ").map_or("", |(_, r)| r);
        let f: Vec<&str> = after.split_whitespace().collect();
        let ticks = f.get(11).and_then(|u| u.parse::<u64>().ok()).unwrap_or(0) + f.get(12).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
        v.push((cmd, ticks));
    }
    v
}
