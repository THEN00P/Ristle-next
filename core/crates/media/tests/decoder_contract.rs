//! The decoder contract with a stand-in for a platform AAC decoder
//! (MediaCodec): packets in flight that only come out at the end of stream,
//! and an output rate that changes after the first frames (implicit SBR).

// Test helpers outside #[test] functions are not covered by clippy.toml.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::VecDeque;
use std::sync::Arc;

use common::{CUTS_44, fixture, left, read_to_end, reference};
use ristle_media::{
    Codec, CodecDescription, ContainerStream, Decoder, DecoderProvider, DecoderRegistry, Error,
    HeAac, MediaHint, MediaStream, Packet, Read, StreamFormat, Support, SymphoniaProvider,
};

/// Decodes with Symphonia, then behaves like a platform decoder: it holds
/// `latency` packets' output back (released only by `finish`), and from
/// packet `double_from` on doubles the output rate by repeating each frame.
struct PlatformLike {
    inner: Box<dyn Decoder>,
    latency: usize,
    double_from: Option<usize>,
    packets: usize,
    queue: VecDeque<(StreamFormat, Vec<f32>)>,
    format: Option<StreamFormat>,
}

impl PlatformLike {
    fn release(&mut self, out: &mut Vec<f32>, keep: usize) {
        while self.queue.len() > keep {
            let Some((format, frames)) = self.queue.pop_front() else {
                break;
            };
            if self.format.is_some_and(|f| f != format) && !out.is_empty() {
                // One call returns one format: put it back for the next call.
                self.queue.push_front((format, frames));
                break;
            }
            self.format = Some(format);
            out.extend(frames);
        }
    }
}

impl Decoder for PlatformLike {
    fn decode(&mut self, packet: &Packet<'_>, out: &mut Vec<f32>) -> Result<(), Error> {
        let mut decoded = Vec::new();
        self.inner.decode(packet, &mut decoded)?;
        let mut format = self.inner.format().expect("format after decoding");
        if self.double_from.is_some_and(|from| self.packets >= from) {
            format.sample_rate *= 2;
            decoded = decoded
                .as_chunks::<2>()
                .0
                .iter()
                .flat_map(|[l, r]| [*l, *r, *l, *r])
                .collect();
        }
        self.packets += 1;
        self.queue.push_back((format, decoded));
        self.release(out, self.latency);
        Ok(())
    }

    fn format(&self) -> Option<StreamFormat> {
        self.format
    }

    fn reset(&mut self) {
        self.inner.reset();
        self.queue.clear();
        self.packets = 0;
    }

    fn finish(&mut self, out: &mut Vec<f32>) -> Result<(), Error> {
        self.release(out, 0);
        Ok(())
    }
}

struct PlatformAac {
    latency: usize,
    double_from: Option<usize>,
}

impl DecoderProvider for PlatformAac {
    fn name(&self) -> &'static str {
        "platform-aac"
    }

    fn supports(&self, codec: &CodecDescription) -> Support {
        // Claims plain AAC too, for the test; a real one claims HE-AAC only.
        if codec.codec == Codec::Aac {
            Support::Full
        } else {
            Support::No
        }
    }

    fn open(&self, codec: &CodecDescription) -> Result<Box<dyn Decoder>, Error> {
        Ok(Box::new(PlatformLike {
            inner: SymphoniaProvider::new().open(codec)?,
            latency: self.latency,
            double_from: self.double_from,
            packets: 0,
            queue: VecDeque::new(),
            format: None,
        }))
    }
}

fn open_with(provider: PlatformAac) -> ContainerStream {
    let mut registry = DecoderRegistry::with_defaults();
    registry.register(Arc::new(provider));
    let stream =
        ContainerStream::open_file(&fixture("album-1.m4a"), &MediaHint::default(), &registry)
            .expect("open");
    assert_eq!(stream.decoder(), "platform-aac");
    stream
}

const FRAMES: usize = CUTS_44[2] - CUTS_44[1];

#[test]
fn frames_still_in_the_decoder_at_the_end_are_flushed() {
    let mut stream = open_with(PlatformAac {
        latency: 3,
        double_from: None,
    });
    let (_, samples) = read_to_end(&mut stream);
    assert_eq!(
        samples.len() / 2,
        FRAMES,
        "the last packets come out of finish()"
    );
    let source = left(&reference(44_100, CUTS_44[1]..CUTS_44[2]), 2);
    assert_eq!(
        common::best_lag(&left(&samples, 2), &source, 2048, 8192, 3000),
        0
    );
}

#[test]
fn an_output_rate_change_is_reported_between_reads_and_keeps_the_edit_list() {
    // Packet 0 is priming (the edit list drops it), packet 1 comes out at
    // 44.1 kHz, everything after at 88.2 kHz.
    let mut stream = open_with(PlatformAac {
        latency: 1,
        double_from: Some(2),
    });
    let mut buffer = vec![0f32; 4096 * 2];
    let mut by_format: Vec<(StreamFormat, usize)> = Vec::new();
    loop {
        let format = stream.format();
        match stream.read(&mut buffer).unwrap() {
            Read::Frames(frames) => match by_format.last_mut() {
                Some((last, count)) if *last == format => *count += frames,
                _ => by_format.push((format, frames)),
            },
            Read::End => break,
            Read::Pending => unreachable!("file streams never pend"),
        }
    }
    let low = StreamFormat {
        sample_rate: 44_100,
        channels: 2,
    };
    let high = StreamFormat {
        sample_rate: 88_200,
        channels: 2,
    };
    assert_eq!(by_format, vec![(low, 1024), (high, (FRAMES - 1024) * 2)]);
}

#[test]
fn he_aac_routing_prefers_a_platform_decoder_over_degraded_symphonia() {
    let mut implicit = CodecDescription::new(Codec::Aac);
    implicit.extra_data = Some([0x13, 0x90].into()); // AAC-LC at 22.05 kHz
    assert_eq!(implicit.he_aac(), HeAac::LikelyImplicit);
    let mut registry = DecoderRegistry::with_defaults();
    assert_eq!(
        registry.select(&implicit),
        Some(("symphonia", Support::Degraded))
    );

    struct HeOnly;
    impl DecoderProvider for HeOnly {
        fn name(&self) -> &'static str {
            "mediacodec"
        }
        fn supports(&self, codec: &CodecDescription) -> Support {
            if codec.he_aac() == HeAac::No {
                Support::No
            } else {
                Support::Full
            }
        }
        fn open(&self, _: &CodecDescription) -> Result<Box<dyn Decoder>, Error> {
            Err(Error::UnsupportedCodec("not in this test".into()))
        }
    }
    registry.register(Arc::new(HeOnly));
    assert_eq!(
        registry.select(&implicit),
        Some(("mediacodec", Support::Full))
    );
    let mut lc = CodecDescription::new(Codec::Aac);
    lc.extra_data = Some([0x12, 0x10].into());
    assert_eq!(registry.select(&lc), Some(("symphonia", Support::Full)));
}
