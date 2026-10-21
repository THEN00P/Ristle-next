//! The `Decoder` contract and the decoder registry (ADR 0004 and its
//! amendment).
//!
//! Demuxers hand a decoder packets. The decoder returns interleaved f32
//! **aligned to the input timeline**: across a stream, output frame `n` is
//! input frame `n`, counted from the first frame of the first packet after
//! opening or [`Decoder::reset`]. Decoder latency (FDK's output trails by
//! 1685 frames) is therefore the provider's problem, and every frame a
//! provider holds back comes out of [`Decoder::finish`].
//!
//! Decoders never trim. Encoder delay, padding, edit lists and Opus pre-skip
//! are applied by the stream, in one place
//! ([`crate::stream`]), so a stream sounds the same whichever provider
//! decodes it.

mod symphonia;

use std::sync::Arc;

use crate::codec::CodecDescription;
use crate::{Error, StreamFormat};

pub(crate) use self::symphonia::opus_pre_skip;
pub use self::symphonia::{LibopusProvider, SymphoniaProvider};

/// One demuxed packet.
#[derive(Clone, Copy, Debug)]
pub struct Packet<'a> {
    /// The encoded bytes.
    pub data: &'a [u8],
    /// When the packet's first decoded frame plays, in the track's time base.
    /// Negative for encoder delay. Decoders that need a timestamp (MediaCodec
    /// wants one per input buffer) take it from here; nothing else depends on
    /// it.
    pub timestamp: i64,
    /// How long the packet's decoded output is, in the track's time base.
    pub duration: u64,
}

/// How well a provider decodes a stream. Ordered: the registry picks the
/// highest, and a later registration wins a tie.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Support {
    /// The provider cannot decode the stream.
    No,
    /// The provider decodes the stream with a known defect, such as the AAC
    /// core of an HE-AAC stream (half the bandwidth, mono for v2). Used only
    /// when nothing better is registered.
    Degraded,
    /// The provider decodes the stream completely.
    Full,
}

/// A decoder for one stream.
///
/// The contract (ADR 0004): output is interleaved f32 aligned to the input
/// timeline (see the module documentation), packets are decoded in order,
/// and nothing is trimmed.
pub trait Decoder: Send {
    /// Decodes one packet and appends whatever frames are ready to `out`, in
    /// the format [`Decoder::format`] reports afterwards.
    ///
    /// [`Error::CorruptPacket`] means this packet was dropped and decoding can
    /// continue with the next one. Any other error ends the stream.
    fn decode(&mut self, packet: &Packet<'_>, out: &mut Vec<f32>) -> Result<(), Error>;

    /// The format of the frames the last call appended, or `None` before the
    /// first frames. It may change once, early in a stream: implicit SBR
    /// doubles the rate when the decoder first sees the extension, and
    /// MediaCodec reports that through a format change.
    fn format(&self) -> Option<StreamFormat>;

    /// Drops all state, for a seek. The next packet starts a new timeline.
    fn reset(&mut self);

    /// End of stream: appends every frame the decoder still holds. MediaCodec
    /// keeps several packets in flight, and dropping them loses the end of
    /// the track and breaks gapless playback.
    fn finish(&mut self, out: &mut Vec<f32>) -> Result<(), Error> {
        let _ = out;
        Ok(())
    }
}

/// Something that can make decoders: Symphonia, libopus, or a platform
/// decoder (MediaCodec, AudioToolbox, Media Foundation) registered by the
/// platform glue.
///
/// A platform AAC provider should claim exactly the streams Symphonia cannot
/// decode: [`Support::Full`] when [`CodecDescription::he_aac`] is not
/// [`HeAac::No`](crate::HeAac::No), and [`Support::No`] for plain AAC-LC,
/// which Symphonia decodes at a twentieth of MediaCodec's cost.
pub trait DecoderProvider: Send + Sync {
    /// A short name for logs and diagnostics, such as `"symphonia"`.
    fn name(&self) -> &'static str;

    /// How well this provider decodes the stream.
    fn supports(&self, codec: &CodecDescription) -> Support;

    /// Opens a decoder. Called only after [`DecoderProvider::supports`]
    /// returned more than [`Support::No`].
    fn open(&self, codec: &CodecDescription) -> Result<Box<dyn Decoder>, Error>;
}

/// The decoder a registry opened, and who provided it.
pub struct OpenedDecoder {
    /// The decoder.
    pub decoder: Box<dyn Decoder>,
    /// The provider's name.
    pub provider: &'static str,
    /// How well the provider said it supports the stream.
    pub support: Support,
}

impl std::fmt::Debug for OpenedDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenedDecoder")
            .field("provider", &self.provider)
            .field("support", &self.support)
            .finish_non_exhaustive()
    }
}

/// The providers a host decodes with, in registration order.
#[derive(Clone, Default)]
pub struct DecoderRegistry {
    providers: Vec<Arc<dyn DecoderProvider>>,
}

impl std::fmt::Debug for DecoderRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.providers.iter().map(|p| p.name()))
            .finish()
    }
}

impl DecoderRegistry {
    /// A registry without providers.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Symphonia and libopus, the providers on every platform. The platform
    /// glue registers its AAC provider after these.
    pub fn with_defaults() -> Self {
        let mut registry = Self::empty();
        registry.register(Arc::new(SymphoniaProvider::new()));
        registry.register(Arc::new(LibopusProvider::new()));
        registry
    }

    /// Adds a provider. It wins ties against everything registered before it.
    pub fn register(&mut self, provider: Arc<dyn DecoderProvider>) {
        self.providers.push(provider);
    }

    /// Providers that support the stream, best first.
    fn candidates(&self, codec: &CodecDescription) -> Vec<(&dyn DecoderProvider, Support)> {
        let mut candidates: Vec<_> = self
            .providers
            .iter()
            .enumerate()
            .map(|(index, provider)| (index, provider.as_ref(), provider.supports(codec)))
            .filter(|(_, _, support)| *support > Support::No)
            .collect();
        // Best support first; among equals, the latest registration first.
        candidates.sort_by(|a, b| b.2.cmp(&a.2).then(b.0.cmp(&a.0)));
        candidates.into_iter().map(|(_, p, s)| (p, s)).collect()
    }

    /// The provider that would decode the stream, and how well.
    pub fn select(&self, codec: &CodecDescription) -> Option<(&'static str, Support)> {
        self.candidates(codec)
            .first()
            .map(|(provider, support)| (provider.name(), *support))
    }

    /// Opens a decoder from the best provider. If that provider fails to
    /// open, the next best is tried, so a broken platform decoder falls back
    /// to Symphonia's degraded decode instead of failing the Track.
    pub fn open(&self, codec: &CodecDescription) -> Result<OpenedDecoder, Error> {
        let mut last_error = None;
        for (provider, support) in self.candidates(codec) {
            match provider.open(codec) {
                Ok(decoder) => {
                    if support == Support::Degraded {
                        tracing::warn!(
                            provider = provider.name(),
                            codec = ?codec.codec,
                            "decoding with known defects: no full decoder is registered"
                        );
                    }
                    return Ok(OpenedDecoder {
                        decoder,
                        provider: provider.name(),
                        support,
                    });
                }
                Err(error) => {
                    tracing::warn!(provider = provider.name(), %error, "decoder failed to open");
                    last_error = Some(error);
                }
            }
        }
        Err(last_error.unwrap_or_else(|| Error::UnsupportedCodec(format!("{:?}", codec.codec))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::Codec;

    struct Fake {
        name: &'static str,
        support: Support,
        opens: bool,
    }

    struct Silent;

    impl Decoder for Silent {
        fn decode(&mut self, _: &Packet<'_>, _: &mut Vec<f32>) -> Result<(), Error> {
            Ok(())
        }
        fn format(&self) -> Option<StreamFormat> {
            None
        }
        fn reset(&mut self) {}
    }

    impl DecoderProvider for Fake {
        fn name(&self) -> &'static str {
            self.name
        }
        fn supports(&self, _: &CodecDescription) -> Support {
            self.support
        }
        fn open(&self, _: &CodecDescription) -> Result<Box<dyn Decoder>, Error> {
            if self.opens {
                Ok(Box::new(Silent))
            } else {
                Err(Error::Decoder {
                    decoder: self.name,
                    detail: "broken".into(),
                })
            }
        }
    }

    fn registry(fakes: Vec<Fake>) -> DecoderRegistry {
        let mut registry = DecoderRegistry::empty();
        for fake in fakes {
            registry.register(Arc::new(fake));
        }
        registry
    }

    fn fake(name: &'static str, support: Support, opens: bool) -> Fake {
        Fake {
            name,
            support,
            opens,
        }
    }

    #[test]
    fn the_best_support_wins_and_later_registrations_win_ties() {
        let aac = CodecDescription::new(Codec::Aac);
        let registry = registry(vec![
            fake("a", Support::Full, true),
            fake("b", Support::Degraded, true),
            fake("c", Support::Full, true),
            fake("d", Support::No, true),
        ]);
        assert_eq!(registry.select(&aac), Some(("c", Support::Full)));
    }

    #[test]
    fn a_provider_that_fails_to_open_falls_back_to_the_next_best() {
        let aac = CodecDescription::new(Codec::Aac);
        let registry = registry(vec![
            fake("symphonia", Support::Degraded, true),
            fake("platform", Support::Full, false),
        ]);
        let opened = registry.open(&aac).unwrap();
        assert_eq!(
            (opened.provider, opened.support),
            ("symphonia", Support::Degraded)
        );
    }

    #[test]
    fn no_supporting_provider_is_an_unsupported_codec() {
        let registry = registry(vec![fake("a", Support::No, true)]);
        let error = registry
            .open(&CodecDescription::new(Codec::Other))
            .unwrap_err();
        assert!(matches!(error, Error::UnsupportedCodec(_)), "{error}");
    }
}
