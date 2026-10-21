//! What a decoder is asked to decode, and the HE-AAC routing rules of ADR 0004.

use symphonia::core::audio::Channels;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::well_known as ids;
use symphonia::core::codecs::audio::{AudioCodecId, AudioCodecParameters};

/// An audio codec, as far as decoder selection cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Codec {
    /// MPEG-4 AAC in any profile (LC, HE-AAC v1 and v2).
    Aac,
    /// MPEG-1/2 Layer III.
    Mp3,
    /// MPEG-1/2 Layer II.
    Mp2,
    /// MPEG-1/2 Layer I.
    Mp1,
    /// Vorbis.
    Vorbis,
    /// Opus.
    Opus,
    /// FLAC.
    Flac,
    /// Apple Lossless.
    Alac,
    /// Uncompressed PCM in any sample format.
    Pcm,
    /// ADPCM variants.
    Adpcm,
    /// Anything else a container can declare. No provider shipped with the
    /// host claims it.
    Other,
}

impl Codec {
    fn from_symphonia(id: AudioCodecId) -> Self {
        match id {
            ids::CODEC_ID_AAC => Self::Aac,
            ids::CODEC_ID_MP3 => Self::Mp3,
            ids::CODEC_ID_MP2 => Self::Mp2,
            ids::CODEC_ID_MP1 => Self::Mp1,
            ids::CODEC_ID_VORBIS => Self::Vorbis,
            ids::CODEC_ID_OPUS => Self::Opus,
            ids::CODEC_ID_FLAC => Self::Flac,
            ids::CODEC_ID_ALAC => Self::Alac,
            id if is_pcm(id) => Self::Pcm,
            id if is_adpcm(id) => Self::Adpcm,
            _ => Self::Other,
        }
    }

    /// The Symphonia id for codecs whose parameters a host demuxer can
    /// describe without Symphonia's help. PCM and ADPCM need more than a
    /// codec id, so they only come from Symphonia's own demuxers.
    fn symphonia_id(self) -> Option<AudioCodecId> {
        Some(match self {
            Self::Aac => ids::CODEC_ID_AAC,
            Self::Mp3 => ids::CODEC_ID_MP3,
            Self::Mp2 => ids::CODEC_ID_MP2,
            Self::Mp1 => ids::CODEC_ID_MP1,
            Self::Vorbis => ids::CODEC_ID_VORBIS,
            Self::Opus => ids::CODEC_ID_OPUS,
            Self::Flac => ids::CODEC_ID_FLAC,
            Self::Alac => ids::CODEC_ID_ALAC,
            Self::Pcm | Self::Adpcm | Self::Other => return None,
        })
    }
}

fn is_pcm(id: AudioCodecId) -> bool {
    (ids::CODEC_ID_PCM_S32LE..=ids::CODEC_ID_PCM_MULAW).contains(&id)
}

fn is_adpcm(id: AudioCodecId) -> bool {
    (ids::CODEC_ID_ADPCM_G722..=ids::CODEC_ID_ADPCM_IMA_QT).contains(&id)
}

/// How a stream announces HE-AAC (SBR, optionally with Parametric Stereo).
///
/// Symphonia 0.6 has no SBR or PS. Given explicit HE-AAC it refuses; given
/// implicit HE-AAC it silently decodes the AAC core at half the rate, and in
/// mono for HE-AAC v2. ADR 0004 therefore routes every stream that is not
/// [`HeAac::No`] to a platform AAC decoder when one is registered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeAac {
    /// Plain AAC-LC, or not AAC at all.
    No,
    /// The AudioSpecificConfig names object type 5 (SBR) or 29 (PS).
    Explicit,
    /// The configuration says AAC-LC, but a hint from outside the bitstream
    /// (the HLS `CODECS` attribute or a Plugin's MIME type) says HE-AAC.
    Hinted,
    /// The configuration says AAC-LC at a core rate of 24 kHz or less. That is
    /// almost always implicit SBR (ADTS, and MP4s remuxed from it). A full AAC
    /// decoder handles real low-rate AAC-LC too, so the false positive is
    /// harmless.
    LikelyImplicit,
}

/// A stream's codec as the demuxer found it, plus what is known about it from
/// outside the bitstream. Decoder providers choose from this
/// ([`DecoderProvider::supports`](crate::DecoderProvider::supports)).
#[derive(Clone, Debug)]
pub struct CodecDescription {
    /// The codec.
    pub codec: Codec,
    /// The sample rate the container declares. For implicit HE-AAC this is
    /// the core rate or the output rate, depending on the muxer, so decoders
    /// report their actual output separately.
    pub sample_rate: Option<u32>,
    /// The channel count the container declares.
    pub channels: Option<u16>,
    /// Codec configuration: the AudioSpecificConfig for AAC, the OpusHead for
    /// Opus, STREAMINFO for FLAC, the Vorbis headers, the ALAC magic cookie.
    pub extra_data: Option<Box<[u8]>>,
    /// An RFC 6381 codecs string from outside the bitstream, such as
    /// `mp4a.40.5` from an HLS `CODECS` attribute or a Plugin's MIME type.
    pub profile_hint: Option<String>,
    /// The exact parameters Symphonia's demuxer produced, when it produced
    /// them. Kept because a Symphonia decoder needs more than the fields
    /// above for some codecs (PCM sample formats, ADPCM block sizes).
    native: Option<AudioCodecParameters>,
}

impl CodecDescription {
    /// A description with only the codec known.
    pub fn new(codec: Codec) -> Self {
        Self {
            codec,
            sample_rate: None,
            channels: None,
            extra_data: None,
            profile_hint: None,
            native: None,
        }
    }

    pub(crate) fn from_symphonia(params: &CodecParameters) -> Option<Self> {
        let CodecParameters::Audio(params) = params else {
            return None;
        };
        Some(Self {
            codec: Codec::from_symphonia(params.codec),
            sample_rate: params.sample_rate,
            channels: params
                .channels
                .as_ref()
                .and_then(|c| u16::try_from(c.count()).ok()),
            extra_data: params.extra_data.clone(),
            profile_hint: None,
            native: Some(params.clone()),
        })
    }

    /// Symphonia parameters for this description. The demuxer's own
    /// parameters win; fields set on the description since then (for example
    /// configuration a host demuxer found) are applied over them.
    pub(crate) fn to_symphonia(&self) -> Option<AudioCodecParameters> {
        let mut params = match &self.native {
            Some(native) => native.clone(),
            None => {
                let mut params = AudioCodecParameters::new();
                params.codec = self.codec.symphonia_id()?;
                params
            }
        };
        if let Some(rate) = self.sample_rate {
            params.sample_rate = Some(rate);
        }
        if let Some(channels) = self.channels
            && params.channels.as_ref().map(Channels::count) != Some(usize::from(channels))
        {
            params.channels = Some(channels_for(channels));
        }
        params.extra_data = self.extra_data.clone();
        Some(params)
    }

    /// The AAC audio object type from the AudioSpecificConfig (2 for LC, 5 for
    /// SBR, 29 for PS), if this is AAC with a configuration.
    pub fn aac_object_type(&self) -> Option<u8> {
        if self.codec != Codec::Aac {
            return None;
        }
        let asc = self.extra_data.as_deref()?;
        let aot = asc.first()? >> 3;
        if aot == 31 {
            // Escape: 6 more bits follow the 5.
            Some(32 + (((asc[0] & 0x07) << 3) | (asc.get(1)? >> 5)))
        } else {
            Some(aot)
        }
    }

    /// The AAC core sample rate from the AudioSpecificConfig. An MP4 sample
    /// entry may say 44.1 kHz while the configuration says 22.05 kHz.
    pub fn aac_core_rate(&self) -> Option<u32> {
        const RATES: [u32; 13] = [
            96_000, 88_200, 64_000, 48_000, 44_100, 32_000, 24_000, 22_050, 16_000, 12_000, 11_025,
            8_000, 7_350,
        ];
        self.aac_object_type()?;
        let asc = self.extra_data.as_deref()?;
        if asc.first()? >> 3 == 31 {
            // Escaped object types shift the frequency index; none of them is
            // a profile the host routes on, so the core rate is not needed.
            return None;
        }
        let index = usize::from(((asc[0] & 0x07) << 1) | (asc.get(1)? >> 7));
        if index == 0x0f {
            // An explicit 24-bit rate follows.
            let b = |i: usize| asc.get(i).copied().map(u32::from);
            return Some(((b(1)? & 0x7f) << 17) | (b(2)? << 9) | (b(3)? << 1) | (b(4)? >> 7));
        }
        RATES.get(index).copied()
    }

    /// How this stream announces HE-AAC, if at all (ADR 0004).
    pub fn he_aac(&self) -> HeAac {
        if self.codec != Codec::Aac {
            return HeAac::No;
        }
        if matches!(self.aac_object_type(), Some(5 | 29)) {
            return HeAac::Explicit;
        }
        if self.profile_hint.as_deref().is_some_and(hints_he_aac) {
            return HeAac::Hinted;
        }
        let lc_or_unknown = self.aac_object_type().is_none_or(|aot| aot == 2);
        let core_rate = self.aac_core_rate().or(self.sample_rate);
        if lc_or_unknown && core_rate.is_some_and(|rate| rate <= 24_000) {
            return HeAac::LikelyImplicit;
        }
        HeAac::No
    }
}

/// Whether an RFC 6381 codecs string (possibly a comma-separated list, as in
/// an HLS `CODECS` attribute) names HE-AAC v1 (`mp4a.40.5`) or v2
/// (`mp4a.40.29`).
fn hints_he_aac(hint: &str) -> bool {
    hint.split(',').map(str::trim).any(|codec| {
        let codec = codec.to_ascii_lowercase();
        codec == "mp4a.40.5" || codec == "mp4a.40.29"
    })
}

fn channels_for(count: u16) -> Channels {
    use symphonia::core::audio::layouts;
    match count {
        1 => layouts::CHANNEL_LAYOUT_MONO,
        2 => layouts::CHANNEL_LAYOUT_STEREO,
        n => Channels::Discrete(n),
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
    fn reads_object_type_and_core_rate_from_the_audio_specific_config() {
        // AAC-LC, 44.1 kHz, stereo.
        let lc = aac(&[0x12, 0x10]);
        assert_eq!(lc.aac_object_type(), Some(2));
        assert_eq!(lc.aac_core_rate(), Some(44_100));
        // SBR (object type 5), core 22.05 kHz.
        let sbr = aac(&[0x2b, 0x92, 0x08, 0x00]);
        assert_eq!(sbr.aac_object_type(), Some(5));
        assert_eq!(sbr.aac_core_rate(), Some(22_050));
        // Escaped: 32 + 1 = 33 (MPEG Layer 2).
        assert_eq!(aac(&[0xf8, 0x20]).aac_object_type(), Some(33));
        // Explicit 24-bit rate: index 15, then 12345 Hz.
        let explicit = aac(&[0x17, 0x80, 0x18, 0x1c, 0x80]);
        assert_eq!(explicit.aac_core_rate(), Some(12_345));
    }

    #[test]
    fn explicit_he_aac_is_recognized_from_the_configuration() {
        assert_eq!(aac(&[0x2b, 0x92, 0x08, 0x00]).he_aac(), HeAac::Explicit);
        // Object type 29 (Parametric Stereo).
        assert_eq!(aac(&[0xeb, 0x8a, 0x08, 0x00]).he_aac(), HeAac::Explicit);
    }

    #[test]
    fn a_codecs_hint_marks_an_lc_configuration_as_he_aac() {
        let mut hinted = aac(&[0x12, 0x10]);
        hinted.profile_hint = Some("avc1.4d401f, mp4a.40.5".into());
        assert_eq!(hinted.he_aac(), HeAac::Hinted);
        hinted.profile_hint = Some("MP4A.40.29".into());
        assert_eq!(hinted.he_aac(), HeAac::Hinted);
        hinted.profile_hint = Some("mp4a.40.2".into());
        assert_eq!(hinted.he_aac(), HeAac::No);
    }

    #[test]
    fn a_low_core_rate_is_treated_as_implicit_sbr() {
        // AAC-LC at 22.05 kHz in the configuration, whatever the container says.
        let mut implicit = aac(&[0x13, 0x90]);
        implicit.sample_rate = Some(44_100);
        assert_eq!(implicit.he_aac(), HeAac::LikelyImplicit);
        // ADTS-style: no configuration, only the declared rate.
        let mut bare = CodecDescription::new(Codec::Aac);
        bare.sample_rate = Some(24_000);
        assert_eq!(bare.he_aac(), HeAac::LikelyImplicit);
        bare.sample_rate = Some(32_000);
        assert_eq!(bare.he_aac(), HeAac::No);
    }

    #[test]
    fn non_aac_codecs_are_never_he_aac() {
        let mut opus = CodecDescription::new(Codec::Opus);
        opus.sample_rate = Some(16_000);
        opus.profile_hint = Some("mp4a.40.5".into());
        assert_eq!(opus.he_aac(), HeAac::No);
        assert_eq!(opus.aac_object_type(), None);
    }
}
