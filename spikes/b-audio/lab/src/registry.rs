//! Sketch of the host's `Decoder` registry (ADR 0004).
//!
//! Demuxing stays with Symphonia's format readers (plus the host's own TS
//! demuxer), so a decoder is fed demuxed packets and codec parameters. Several
//! providers can claim a codec. The registry asks each one how well it supports
//! the stream and picks the best, so a platform decoder (MediaCodec,
//! AudioToolbox, Media Foundation) or a C library can take over exactly where
//! Symphonia is missing or weak without the rest of the pipeline knowing.

use anyhow::{anyhow, Result};
use symphonia::core::codecs::audio::well_known::{CODEC_ID_AAC, CODEC_ID_OPUS};
use symphonia::core::codecs::audio::{AudioCodecParameters, AudioDecoder, AudioDecoderOptions};
use symphonia::core::codecs::registry::CodecRegistry;
use symphonia::core::packet::Packet;

/// How well a provider handles a stream. Ordered: higher wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Support {
    No,
    /// Decodes, but with a known defect (for example the AAC core of an HE-AAC
    /// stream without SBR: half the bandwidth). Used only if nothing better exists.
    Degraded,
    Full,
}

/// What the host knows about a stream before the first packet. `profile_hint`
/// comes from outside the bitstream: an HLS `CODECS` attribute such as
/// `mp4a.40.5`, a Plugin's codec hint, or a previous decode that saw SBR.
#[derive(Clone)]
pub struct StreamInfo {
    pub params: AudioCodecParameters,
    pub profile_hint: Option<String>,
}

pub trait Decoder: Send {
    /// Decodes one packet and appends interleaved f32 frames at `output()`.
    /// Gapless trims carried by the packet are applied by the decoder.
    fn decode(&mut self, packet: &Packet, out: &mut Vec<f32>) -> Result<()>;
    /// Output rate and channel count. May change after the first packet
    /// (implicit SBR doubles the rate).
    fn output(&self) -> (u32, usize);
    /// Drops internal state after a seek.
    fn reset(&mut self);
}

pub trait DecoderProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn support(&self, stream: &StreamInfo) -> Support;
    fn create(&self, stream: &StreamInfo) -> Result<Box<dyn Decoder>>;
}

pub struct DecoderRegistry {
    providers: Vec<Box<dyn DecoderProvider>>,
}

impl DecoderRegistry {
    pub fn new() -> Self {
        Self { providers: Vec::new() }
    }

    /// Later registrations win ties, so platform decoders register after Symphonia.
    pub fn register(&mut self, provider: Box<dyn DecoderProvider>) {
        self.providers.push(provider);
    }

    pub fn select(&self, stream: &StreamInfo) -> Option<(&dyn DecoderProvider, Support)> {
        let mut best: Option<(&dyn DecoderProvider, Support)> = None;
        for p in &self.providers {
            let s = p.support(stream);
            if s > Support::No && best.is_none_or(|(_, b)| s >= b) {
                best = Some((p.as_ref(), s));
            }
        }
        best
    }

    pub fn create(&self, stream: &StreamInfo) -> Result<(Box<dyn Decoder>, &'static str, Support)> {
        let (p, s) = self
            .select(stream)
            .ok_or_else(|| anyhow!("no decoder for codec {:?}", stream.params.codec))?;
        Ok((p.create(stream)?, p.name(), s))
    }
}

/// AAC object type from the AudioSpecificConfig (5 = SBR, 29 = PS), if present.
pub fn aac_object_type(params: &AudioCodecParameters) -> Option<u8> {
    let asc = params.extra_data.as_ref()?;
    let aot = asc.first()? >> 3;
    Some(if aot == 31 { 32 + ((asc[0] & 7) << 3 | asc.get(1)? >> 5) } else { aot })
}

fn is_he_aac(stream: &StreamInfo) -> bool {
    if stream.params.codec != CODEC_ID_AAC {
        return false;
    }
    let hinted = stream
        .profile_hint
        .as_deref()
        .is_some_and(|h| h.ends_with("40.5") || h.ends_with("40.29") || h.eq_ignore_ascii_case("he-aac"));
    hinted || matches!(aac_object_type(&stream.params), Some(5 | 29)) || probably_implicit_sbr(stream)
}

/// Implicit SBR (ADTS, and MP4s whose AudioSpecificConfig only says AAC-LC)
/// cannot be seen before decoding, and Symphonia hides it: it skips the SBR
/// fill elements and outputs the core at half rate. A core rate of 24 kHz or
/// less almost always means HE-AAC, and a full decoder handles plain
/// low-rate AAC-LC too, so route those streams away from Symphonia.
pub fn probably_implicit_sbr(stream: &StreamInfo) -> bool {
    stream.params.codec == CODEC_ID_AAC
        && aac_object_type(&stream.params).is_none_or(|aot| aot == 2)
        && asc_sample_rate(&stream.params).or(stream.params.sample_rate).is_some_and(|r| r <= 24_000)
}

/// Core sample rate from the AudioSpecificConfig. An MP4 sample entry may say
/// 44.1 kHz while the ASC says 22.05 kHz (implicit SBR, as ffmpeg remuxes it).
fn asc_sample_rate(params: &AudioCodecParameters) -> Option<u32> {
    const RATES: [u32; 13] = [96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350];
    let asc = params.extra_data.as_ref()?;
    if asc.first()? >> 3 == 31 {
        return None;
    }
    let idx = ((asc[0] & 7) << 1 | asc.get(1)? >> 7) as usize;
    RATES.get(idx).copied()
}

/// Wraps any Symphonia-API decoder (Symphonia's own, or an adapter crate).
struct SymphoniaDecoder {
    inner: Box<dyn AudioDecoder>,
    rate: u32,
    channels: usize,
    scratch: Vec<f32>,
}

impl Decoder for SymphoniaDecoder {
    fn decode(&mut self, packet: &Packet, out: &mut Vec<f32>) -> Result<()> {
        let buf = self.inner.decode(packet)?;
        self.rate = buf.spec().rate();
        self.channels = buf.spec().channels().count();
        self.scratch.clear();
        buf.copy_to_vec_interleaved(&mut self.scratch);
        out.extend_from_slice(&self.scratch);
        Ok(())
    }
    fn output(&self) -> (u32, usize) {
        (self.rate, self.channels)
    }
    fn reset(&mut self) {
        self.inner.reset();
    }
}

fn wrap(inner: Box<dyn AudioDecoder>, params: &AudioCodecParameters) -> Box<dyn Decoder> {
    Box::new(SymphoniaDecoder {
        inner,
        rate: params.sample_rate.unwrap_or(0),
        channels: params.channels.as_ref().map_or(0, |c| c.count()),
        scratch: Vec::new(),
    })
}

/// Symphonia's pure-Rust decoders.
pub struct SymphoniaProvider {
    codecs: &'static CodecRegistry,
}

impl SymphoniaProvider {
    pub fn new() -> Self {
        Self { codecs: symphonia::default::get_codecs() }
    }
}

impl DecoderProvider for SymphoniaProvider {
    fn name(&self) -> &'static str {
        "symphonia"
    }
    fn support(&self, stream: &StreamInfo) -> Support {
        if self.codecs.get_audio_decoder(stream.params.codec).is_none() {
            return Support::No;
        }
        // Explicit HE-AAC is refused by Symphonia ("aac too complex"); implicit
        // SBR decodes the core only. Either way it is not the right decoder.
        if is_he_aac(stream) {
            return if aac_object_type(&stream.params).is_none_or(|aot| aot == 2) { Support::Degraded } else { Support::No };
        }
        Support::Full
    }
    fn create(&self, stream: &StreamInfo) -> Result<Box<dyn Decoder>> {
        let inner = self.codecs.make_audio_decoder(&stream.params, &AudioDecoderOptions::default())?;
        Ok(wrap(inner, &stream.params))
    }
}

/// A C library behind a Symphonia adapter crate (libopus, FDK AAC).
pub struct AdapterProvider {
    name: &'static str,
    codecs: CodecRegistry,
    claims: fn(&StreamInfo) -> bool,
}

impl AdapterProvider {
    #[cfg(feature = "libopus")]
    pub fn libopus() -> Self {
        let mut codecs = CodecRegistry::new();
        codecs.register_audio_decoder::<symphonia_adapter_libopus::OpusDecoder>();
        Self { name: "libopus", codecs, claims: |s| s.params.codec == CODEC_ID_OPUS }
    }

    #[cfg(feature = "fdk")]
    pub fn fdk_aac() -> Self {
        let mut codecs = CodecRegistry::new();
        codecs.register_audio_decoder::<symphonia_adapter_fdk_aac::AacDecoder>();
        Self { name: "fdk-aac", codecs, claims: is_he_aac }
    }

    /// FDK for every AAC stream, to compare it with Symphonia's AAC-LC decoder.
    #[cfg(feature = "fdk")]
    pub fn fdk_aac_all() -> Self {
        let mut codecs = CodecRegistry::new();
        codecs.register_audio_decoder::<symphonia_adapter_fdk_aac::AacDecoder>();
        Self { name: "fdk-aac", codecs, claims: |s| s.params.codec == CODEC_ID_AAC }
    }
}

impl DecoderProvider for AdapterProvider {
    fn name(&self) -> &'static str {
        self.name
    }
    fn support(&self, stream: &StreamInfo) -> Support {
        if (self.claims)(stream) && self.codecs.get_audio_decoder(stream.params.codec).is_some() {
            Support::Full
        } else {
            Support::No
        }
    }
    fn create(&self, stream: &StreamInfo) -> Result<Box<dyn Decoder>> {
        let inner = self.codecs.make_audio_decoder(&stream.params, &AudioDecoderOptions::default())?;
        Ok(wrap(inner, &stream.params))
    }
}

/// The registry as the host would build it, minus platform decoders.
pub fn default_registry() -> DecoderRegistry {
    let mut r = DecoderRegistry::new();
    r.register(Box::new(SymphoniaProvider::new()));
    #[cfg(feature = "libopus")]
    r.register(Box::new(AdapterProvider::libopus()));
    #[cfg(feature = "fdk")]
    r.register(Box::new(AdapterProvider::fdk_aac()));
    r
}
