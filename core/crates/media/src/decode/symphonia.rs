//! The providers on every platform: Symphonia's decoders, and libopus
//! through its Symphonia adapter.

use symphonia::core::codecs::audio::{AudioCodecParameters, AudioDecoder, AudioDecoderOptions};
use symphonia::core::codecs::registry::{AudioDecoderFactoryFn, CodecRegistry};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::packet::PacketRef;
use symphonia::core::units::{Duration, Timestamp};

use super::{Decoder, DecoderProvider, Packet, Support};
use crate::codec::{Codec, CodecDescription, HeAac};
use crate::{Error, StreamFormat};

/// Symphonia's pure-Rust decoders: MP3, AAC-LC, Vorbis, FLAC, ALAC, PCM and
/// ADPCM.
pub struct SymphoniaProvider {
    codecs: CodecRegistry,
}

impl SymphoniaProvider {
    /// The provider with every Symphonia decoder the crate enables.
    pub fn new() -> Self {
        let mut codecs = CodecRegistry::new();
        symphonia::default::register_enabled_codecs(&mut codecs);
        Self { codecs }
    }
}

impl std::fmt::Debug for SymphoniaProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SymphoniaProvider")
    }
}

impl Default for SymphoniaProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl DecoderProvider for SymphoniaProvider {
    fn name(&self) -> &'static str {
        "symphonia"
    }

    fn supports(&self, codec: &CodecDescription) -> Support {
        let Some(params) = codec.to_symphonia() else {
            return Support::No;
        };
        if self.codecs.get_audio_decoder(params.codec).is_none() {
            return Support::No;
        }
        match codec.he_aac() {
            HeAac::No => Support::Full,
            // Symphonia refuses explicit SBR/PS ("aac too complex").
            HeAac::Explicit => Support::No,
            // It decodes the core of implicit HE-AAC: half the rate, and mono
            // for v2. Playable, but only when nothing better exists.
            HeAac::Hinted | HeAac::LikelyImplicit => Support::Degraded,
        }
    }

    fn open(&self, codec: &CodecDescription) -> Result<Box<dyn Decoder>, Error> {
        open_with(&self.codecs, self.name(), codec)
    }
}

/// libopus (BSD-3-Clause), for Opus on every platform. Symphonia has no Opus
/// decoder.
pub struct LibopusProvider {
    codecs: CodecRegistry,
}

impl LibopusProvider {
    /// The provider.
    pub fn new() -> Self {
        let mut codecs = CodecRegistry::new();
        codecs.register_audio_decoder::<symphonia_adapter_libopus::OpusDecoder>();
        Self { codecs }
    }
}

impl std::fmt::Debug for LibopusProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LibopusProvider")
    }
}

impl Default for LibopusProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl DecoderProvider for LibopusProvider {
    fn name(&self) -> &'static str {
        "libopus"
    }

    fn supports(&self, codec: &CodecDescription) -> Support {
        if codec.codec == Codec::Opus {
            Support::Full
        } else {
            Support::No
        }
    }

    fn open(&self, codec: &CodecDescription) -> Result<Box<dyn Decoder>, Error> {
        // The adapter applies the OpusHead pre-skip itself. The stream applies
        // it instead (decoders never trim), so the adapter must see zero.
        let mut codec = codec.clone();
        if let Some(head) = codec.extra_data.as_deref() {
            codec.extra_data = Some(opus_head_without_pre_skip(head));
        }
        open_with(&self.codecs, self.name(), &codec)
    }
}

/// The OpusHead pre-skip in 48 kHz frames (RFC 7845, section 5.1).
pub(crate) fn opus_pre_skip(head: &[u8]) -> Option<u16> {
    if !head.starts_with(b"OpusHead") {
        return None;
    }
    Some(u16::from_le_bytes([*head.get(10)?, *head.get(11)?]))
}

fn opus_head_without_pre_skip(head: &[u8]) -> Box<[u8]> {
    let mut head = head.to_vec();
    if opus_pre_skip(&head).is_some() {
        head[10] = 0;
        head[11] = 0;
    }
    head.into_boxed_slice()
}

fn open_with(
    codecs: &CodecRegistry,
    name: &'static str,
    codec: &CodecDescription,
) -> Result<Box<dyn Decoder>, Error> {
    let params = codec
        .to_symphonia()
        .ok_or_else(|| Error::UnsupportedCodec(format!("{:?}", codec.codec)))?;
    let mut options = AudioDecoderOptions::default();
    // Symphonia's decoders trim encoder delay and padding when `gapless` is
    // set. The stream does that, so they must not.
    options.gapless = false;
    let factory = codecs
        .get_audio_decoder(params.codec)
        .ok_or_else(|| Error::UnsupportedCodec(format!("{:?}", codec.codec)))?
        .factory;
    let inner = factory(&params, &options).map_err(|error| Error::Decoder {
        decoder: name,
        detail: error.to_string(),
    })?;
    Ok(Box::new(SymphoniaDecoder {
        name,
        inner,
        factory,
        params,
        options,
        format: None,
        scratch: Vec::new(),
    }))
}

/// Any decoder behind Symphonia's `AudioDecoder` trait.
struct SymphoniaDecoder {
    name: &'static str,
    inner: Box<dyn AudioDecoder>,
    /// Rebuilds the decoder on reset: Symphonia's AAC decoder keeps state
    /// across `reset()`, so a seek to 0 would not replay the start exactly.
    factory: AudioDecoderFactoryFn,
    params: AudioCodecParameters,
    options: AudioDecoderOptions,
    format: Option<StreamFormat>,
    scratch: Vec<f32>,
}

impl Decoder for SymphoniaDecoder {
    fn decode(&mut self, packet: &Packet<'_>, out: &mut Vec<f32>) -> Result<(), Error> {
        // Untrimmed: the trims stay with the stream.
        let packet = PacketRef::new(
            0,
            Timestamp::new(packet.timestamp),
            Duration::new(packet.duration),
            packet.data,
        );
        let decoded = match self.inner.decode_ref(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::DecodeError(detail)) => {
                return Err(Error::CorruptPacket {
                    decoder: self.name,
                    detail: detail.into(),
                });
            }
            Err(error) => {
                return Err(Error::Decoder {
                    decoder: self.name,
                    detail: error.to_string(),
                });
            }
        };
        let spec = decoded.spec();
        let channels = u16::try_from(spec.channels().count()).unwrap_or(0);
        if channels == 0 || spec.rate() == 0 {
            return Err(Error::Decoder {
                decoder: self.name,
                detail: format!("decoded {channels} channels at {} Hz", spec.rate()),
            });
        }
        self.format = Some(StreamFormat {
            sample_rate: spec.rate(),
            channels,
        });
        self.scratch.clear();
        decoded.copy_to_vec_interleaved(&mut self.scratch);
        out.extend_from_slice(&self.scratch);
        Ok(())
    }

    fn format(&self) -> Option<StreamFormat> {
        self.format
    }

    fn reset(&mut self) {
        match (self.factory)(&self.params, &self.options) {
            Ok(fresh) => self.inner = fresh,
            Err(error) => {
                tracing::warn!(decoder = self.name, %error, "rebuilding the decoder failed");
                self.inner.reset();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aac(asc: &[u8]) -> CodecDescription {
        let mut codec = CodecDescription::new(Codec::Aac);
        codec.extra_data = Some(asc.into());
        codec
    }

    #[test]
    fn symphonia_refuses_explicit_he_aac_and_degrades_implicit_he_aac() {
        let symphonia = SymphoniaProvider::new();
        assert_eq!(symphonia.supports(&aac(&[0x12, 0x10])), Support::Full);
        assert_eq!(
            symphonia.supports(&aac(&[0x2b, 0x92, 0x08, 0x00])),
            Support::No
        );
        assert_eq!(symphonia.supports(&aac(&[0x13, 0x90])), Support::Degraded);
        assert_eq!(
            symphonia.supports(&CodecDescription::new(Codec::Opus)),
            Support::No
        );
    }

    #[test]
    fn libopus_claims_opus_only() {
        let libopus = LibopusProvider::new();
        assert_eq!(
            libopus.supports(&CodecDescription::new(Codec::Opus)),
            Support::Full
        );
        assert_eq!(
            libopus.supports(&CodecDescription::new(Codec::Vorbis)),
            Support::No
        );
    }

    #[test]
    fn the_libopus_decoder_never_sees_the_pre_skip() {
        let mut head = b"OpusHead".to_vec();
        head.extend_from_slice(&[1, 2, 0x38, 0x01, 0x80, 0xbb, 0, 0, 0, 0, 0]);
        assert_eq!(opus_pre_skip(&head), Some(312));
        let stripped = opus_head_without_pre_skip(&head);
        assert_eq!(opus_pre_skip(&stripped), Some(0));
        assert_eq!(&stripped[..10], &head[..10]);
        assert_eq!(&stripped[12..], &head[12..]);
        assert_eq!(opus_pre_skip(b"OpusTags"), None);
    }
}
