//! [`MediaStream`]: decoded, trimmed PCM that the audio engine pulls.
//!
//! This is the one place a stream's gapless information is applied (ADR
//! 0004). Positions are kept on the track's timeline in output frames:
//!
//! - The **origin** is where presentation time zero sits on that timeline:
//!   the MP4 edit list's media time, or else the end of the first packet's
//!   start trim (LAME and Vorbis delay), plus the Opus pre-skip.
//! - Each packet's **trims** are applied to that packet's decoded output.
//! - The edit list's **length** ends the presentation; frames after it are
//!   encoder padding.
//! - A **seek** decodes from a little before the target and drops frames
//!   until the target, so `seek(t)` followed by `read` starts exactly at `t`
//!   of presentation time.

use crate::decode::{Decoder, DecoderRegistry, Packet};
use crate::demux::{Demuxed, Demuxer, Seeked, TimeBase, Window};
use crate::{Codec, Error};

/// The format of decoded PCM: interleaved f32 frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StreamFormat {
    /// Frames per second.
    pub sample_rate: u32,
    /// Samples per frame.
    pub channels: u16,
}

/// The result of [`MediaStream::read`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Read {
    /// `n` frames (never 0) were written to the start of the buffer, in the
    /// format [`MediaStream::format`] reported before the call.
    Frames(usize),
    /// No frames are available yet, and the stream has not ended. Try again
    /// shortly.
    Pending,
    /// The stream has ended and its decoder has been flushed. Returned again
    /// on every read until the next [`MediaStream::seek`].
    End,
}

/// Decoded PCM pulled by the audio engine's decode thread (section 3 of the
/// phase 2 plan).
pub trait MediaStream: Send {
    /// The format of the frames the next [`MediaStream::read`] returns. It may
    /// change once, early in a stream (implicit SBR), and never within one
    /// read; re-read it after every read.
    fn format(&self) -> StreamFormat;

    /// Fills `out` (whose length is a multiple of the channel count) with as
    /// many frames as are ready.
    fn read(&mut self, out: &mut [f32]) -> Result<Read, Error>;

    /// Moves to `position_ms` of presentation time (after the encoder delay).
    /// The next read starts exactly there. A position past the end ends the
    /// stream.
    fn seek(&mut self, position_ms: u64) -> Result<(), Error>;
}

/// Milliseconds as frames at `rate`, saturating: a position from outside
/// (an FFI command) may be anything.
fn ms_to_frames(ms: u64, rate: u32) -> i64 {
    i64::try_from(u128::from(ms) * u128::from(rate) / 1000).unwrap_or(i64::MAX)
}

/// How much audio before a seek target to decode and drop, so the decoder's
/// state (MDCT overlap, the MP3 bit reservoir, Opus' adaptive state) has
/// settled by the target.
fn preroll_ms(codec: Codec) -> u64 {
    match codec {
        Codec::Aac => 50,
        Codec::Mp3 | Codec::Mp2 | Codec::Mp1 => 100,
        Codec::Vorbis => 100,
        // RFC 7845, section 4.6: at least 80 ms.
        Codec::Opus => 80,
        Codec::Flac | Codec::Alac | Codec::Pcm | Codec::Adpcm | Codec::Other => 0,
    }
}

/// A [`MediaStream`] over any [`Demuxer`].
pub(crate) struct DecodedStream<D> {
    demuxer: D,
    decoder: Box<dyn Decoder>,
    decoder_name: &'static str,
    time_base: TimeBase,
    codec: Codec,
    /// The format the container declares, until the decoder reports one.
    declared: StreamFormat,
    /// The decoder's output format as of the last decoded frames.
    output: Option<StreamFormat>,
    /// Opus pre-skip in 48 kHz frames, applied at the origin.
    pre_skip: u64,
    /// The container's presentation window.
    window: Option<Window>,
    /// Presentation zero in the track's time base. Set when the first packet
    /// is seen, unless the container's window sets it.
    origin: Option<i64>,
    /// The first packet's pts: seeks never aim before it.
    first_pts: Option<i64>,
    /// Presentation time to start at (0 when opened, the target after a
    /// seek), in milliseconds.
    target_ms: u64,
    /// Timeline position of the next decoded frame, in output frames. `None`
    /// until a packet anchors it after opening, a seek or a corrupt packet.
    position: Option<i64>,
    /// Anchor for `position`: the pts of the first packet after it was reset.
    anchor: Option<i64>,
    /// Decoded, trimmed frames waiting to be read, in `pending_format`.
    pending: Vec<f32>,
    pending_read: usize,
    pending_format: Option<StreamFormat>,
    /// Frames decoded after a format change, held until `pending` drains.
    held: Option<(StreamFormat, Vec<f32>)>,
    /// Scratch for one packet's decoded output.
    chunk: Vec<f32>,
    ended: bool,
    /// The next decoded chunk is not audio: a Vorbis decoder's first packet
    /// after a reset only primes the overlap (Vorbis I, section 1.3.2).
    /// Ogg signals it as a trim, Matroska does not.
    discard_next_chunk: bool,
}

impl<D: Demuxer> DecodedStream<D> {
    /// Opens a decoder for the demuxer's track and decodes until the first
    /// frames, so [`MediaStream::format`] reports the real output format.
    pub(crate) fn open(demuxer: D, registry: &DecoderRegistry) -> Result<Self, Error> {
        let declared_codec = demuxer.codec().clone();
        // Decoders never trim (ADR 0004), so they get no pre-skip either.
        let pre_skip = match declared_codec.codec {
            Codec::Opus => declared_codec
                .extra_data
                .as_deref()
                .and_then(crate::decode::opus_pre_skip)
                .unwrap_or(0),
            _ => 0,
        };
        let opened = registry.open(&declared_codec)?;
        tracing::debug!(provider = opened.provider, support = ?opened.support, "decoder opened");
        let declared = StreamFormat {
            sample_rate: declared_codec.sample_rate.unwrap_or(0),
            channels: declared_codec.channels.unwrap_or(0),
        };
        let mut stream = Self {
            time_base: demuxer.time_base(),
            window: demuxer.window(),
            demuxer,
            decoder: opened.decoder,
            decoder_name: opened.provider,
            codec: declared_codec.codec,
            declared,
            output: None,
            pre_skip: u64::from(pre_skip),
            origin: None,
            first_pts: None,
            target_ms: 0,
            position: None,
            anchor: None,
            pending: Vec::new(),
            pending_read: 0,
            pending_format: None,
            held: None,
            chunk: Vec::new(),
            ended: false,
            discard_next_chunk: false,
        };
        stream.discard_next_chunk = stream.codec == Codec::Vorbis;
        stream.origin = stream.window.map(|w| w.start);
        stream.prime()?;
        Ok(stream)
    }

    pub(crate) fn decoder_name(&self) -> &'static str {
        self.decoder_name
    }

    /// Decodes until frames are ready (or the stream pends or ends).
    fn prime(&mut self) -> Result<(), Error> {
        while self.pending_frames() == 0 && self.held.is_none() && !self.ended {
            if !self.step()? {
                break;
            }
        }
        Ok(())
    }

    fn pending_frames(&self) -> usize {
        let channels = self
            .pending_format
            .map_or(1, |f| usize::from(f.channels).max(1));
        (self.pending.len() - self.pending_read) / channels
    }

    /// The rate positions are counted in: the decoder's output, or the
    /// container's declaration before the first frames.
    fn rate(&self) -> u32 {
        self.output
            .map_or(self.declared.sample_rate, |f| f.sample_rate)
    }

    /// Decodes one packet. Returns false when the demuxer is waiting for
    /// bytes.
    fn step(&mut self) -> Result<bool, Error> {
        let packet = match self.demuxer.next_packet()? {
            Demuxed::Packet(packet) => packet,
            Demuxed::End => {
                self.finish()?;
                return Ok(true);
            }
        };
        if self.origin.is_none() {
            // No edit list: presentation zero is where the first packet's
            // start trim ends (LAME and Vorbis report the delay as a negative
            // pts with a matching trim). The Opus pre-skip is added below, in
            // frames, because it is not in the time base.
            let trim = i64::try_from(packet.trim_start).unwrap_or(i64::MAX);
            self.origin = Some(packet.pts.saturating_add(trim));
        }
        if self.first_pts.is_none() {
            self.first_pts = Some(packet.pts);
        }
        if self.anchor.is_none() {
            self.anchor = Some(packet.pts);
        }
        self.chunk.clear();
        let decoded = self.decoder.decode(
            &Packet {
                data: &packet.data,
                timestamp: packet.pts,
                duration: packet.dur,
            },
            &mut self.chunk,
        );
        match decoded {
            Ok(()) => {}
            Err(Error::CorruptPacket { decoder, detail }) => {
                tracing::warn!(decoder, %detail, pts = packet.pts, "dropped a corrupt packet");
                // Count from the next packet's own timestamp.
                self.position = None;
                self.anchor = None;
                self.discard_next_chunk = self.codec == Codec::Vorbis;
                return Ok(true);
            }
            Err(error) => return Err(error),
        }
        let trims = (packet.trim_start, packet.trim_end);
        self.accept_chunk(trims);
        Ok(true)
    }

    /// End of the demuxer's packets: flush the decoder.
    fn finish(&mut self) -> Result<(), Error> {
        self.chunk.clear();
        self.decoder.finish(&mut self.chunk)?;
        self.accept_chunk((0, 0));
        self.ended = true;
        Ok(())
    }

    /// Trims the decoded chunk to the presentation and queues what is left.
    fn accept_chunk(&mut self, (trim_start, trim_end): (u64, u64)) {
        let Some(format) = self.decoder.format() else {
            // Nothing decoded yet (MediaCodec still filling its pipeline).
            return;
        };
        let channels = usize::from(format.channels).max(1);
        let frames = self.chunk.len() / channels;
        if frames == 0 {
            return;
        }
        self.observe_format(format);
        let rate = format.sample_rate;
        let discard = std::mem::take(&mut self.discard_next_chunk);
        let tb = self.time_base;
        let position = match self.position {
            Some(position) => position,
            None => tb.to_frames(self.anchor.unwrap_or(0), rate),
        };
        self.position = Some(position.saturating_add(frames as i64));
        if discard {
            return;
        }

        // Window on the timeline, in output frames.
        let origin = tb.to_frames(self.origin.unwrap_or(0), rate).saturating_add(
            i64::try_from(self.pre_skip * u64::from(rate) / 48_000).unwrap_or(i64::MAX),
        );
        let start = origin.saturating_add(ms_to_frames(self.target_ms, rate));
        let end = self.window.and_then(|w| w.length).map(|length| {
            origin.saturating_add(tb.to_frames(i64::try_from(length).unwrap_or(i64::MAX), rate))
        });

        let mut keep_from = tb
            .to_frames(i64::try_from(trim_start).unwrap_or(0), rate)
            .max(0);
        let mut keep_to = frames as i64 - tb.to_frames(i64::try_from(trim_end).unwrap_or(0), rate);
        keep_from = keep_from.max(start.saturating_sub(position));
        if let Some(end) = end {
            keep_to = keep_to.min(end.saturating_sub(position));
        }
        let keep_from = keep_from.clamp(0, frames as i64) as usize;
        let keep_to = keep_to.clamp(0, frames as i64) as usize;
        if keep_from >= keep_to {
            return;
        }
        let kept = &self.chunk[keep_from * channels..keep_to * channels];
        match self.pending_format {
            Some(current) if current != format && self.pending_frames() > 0 => {
                // Frames already queued keep their format; these wait.
                match &mut self.held {
                    Some((held_format, held)) if *held_format == format => {
                        held.extend_from_slice(kept)
                    }
                    _ => self.held = Some((format, kept.to_vec())),
                }
            }
            _ => {
                if self.pending_read == self.pending.len() {
                    self.pending.clear();
                    self.pending_read = 0;
                }
                self.pending_format = Some(format);
                self.pending.extend_from_slice(kept);
            }
        }
    }

    /// Rescales the position when the decoder's output rate changes
    /// (implicit SBR doubles it).
    fn observe_format(&mut self, format: StreamFormat) {
        let previous = self.output.replace(format);
        if let (Some(previous), Some(position)) = (previous, self.position)
            && previous.sample_rate != format.sample_rate
            && previous.sample_rate > 0
        {
            let scaled = i128::from(position) * i128::from(format.sample_rate)
                / i128::from(previous.sample_rate);
            self.position = Some(i64::try_from(scaled).unwrap_or(i64::MAX));
            tracing::info!(
                from = previous.sample_rate,
                to = format.sample_rate,
                decoder = self.decoder_name,
                "decoder output rate changed"
            );
        }
    }
}

impl<D: Demuxer> MediaStream for DecodedStream<D> {
    fn format(&self) -> StreamFormat {
        if self.pending_frames() > 0
            && let Some(format) = self.pending_format
        {
            return format;
        }
        if let Some((format, _)) = &self.held {
            return *format;
        }
        self.output.unwrap_or(self.declared)
    }

    fn read(&mut self, out: &mut [f32]) -> Result<Read, Error> {
        // Frames are returned only in the format `format()` reported.
        let reported = self.format();
        loop {
            let available = self.pending_frames();
            if available > 0 {
                let format = self.pending_format.unwrap_or(self.declared);
                if format != reported {
                    // The decoder was starved and its first new frames came
                    // in a new format during this call. Report it first.
                    return Ok(Read::Pending);
                }
                let channels = usize::from(format.channels).max(1);
                let frames = available.min(out.len() / channels);
                if frames == 0 {
                    return Err(Error::OutputTooSmall {
                        channels: format.channels,
                    });
                }
                let samples = frames * channels;
                out[..samples]
                    .copy_from_slice(&self.pending[self.pending_read..self.pending_read + samples]);
                self.pending_read += samples;
                // Decode ahead, so `format()` describes the next frames.
                self.prime()?;
                return Ok(Read::Frames(frames));
            }
            if let Some((format, frames)) = self.held.take() {
                self.pending = frames;
                self.pending_read = 0;
                self.pending_format = Some(format);
                continue;
            }
            if self.ended {
                return Ok(Read::End);
            }
            if !self.step()? {
                return Ok(Read::Pending);
            }
        }
    }

    fn seek(&mut self, position_ms: u64) -> Result<(), Error> {
        let rate = self.rate();
        if rate == 0 {
            return Err(Error::NotSeekable);
        }
        // The origin is known once the first packet was read, which opening
        // the stream did.
        let origin = self.origin.unwrap_or(0);
        let tb = self.time_base;
        let frames = |ms: u64| ms_to_frames(ms, rate);
        let pre_skip = i64::try_from(self.pre_skip * u64::from(rate) / 48_000).unwrap_or(0);
        // The pre-roll is taken on the track's timeline, so a target near the
        // start still decodes the priming packets before it.
        let target_frames = pre_skip
            .saturating_add(frames(position_ms))
            .saturating_sub(frames(preroll_ms(self.codec)));
        let target = origin
            .saturating_add(tb.ts_at_or_before(target_frames, rate))
            .max(self.first_pts.unwrap_or(i64::MIN));
        let seeked = self.demuxer.seek(target)?;
        self.decoder.reset();
        self.pending.clear();
        self.pending_read = 0;
        self.pending_format = None;
        self.held = None;
        self.position = None;
        self.anchor = None;
        self.discard_next_chunk = self.codec == Codec::Vorbis;
        self.target_ms = position_ms;
        self.ended = matches!(seeked, Seeked::PastEnd);
        self.prime()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::decode::{DecoderProvider, Support};
    use crate::demux::DemuxedPacket;
    use crate::{CodecDescription, StreamFormat};

    /// Packets with whatever timestamps a malformed container claims.
    struct Packets {
        pts: Vec<i64>,
        codec: CodecDescription,
    }

    impl Demuxer for Packets {
        fn codec(&self) -> &CodecDescription {
            &self.codec
        }
        fn time_base(&self) -> TimeBase {
            TimeBase {
                numer: 1,
                denom: 48_000,
            }
        }
        fn window(&self) -> Option<Window> {
            None
        }
        fn next_packet(&mut self) -> Result<Demuxed, Error> {
            if self.pts.is_empty() {
                return Ok(Demuxed::End);
            }
            let pts = self.pts.remove(0);
            Ok(Demuxed::Packet(DemuxedPacket {
                data: Box::new([0]),
                pts,
                dur: 10,
                trim_start: 0,
                trim_end: 0,
            }))
        }
        fn seek(&mut self, _: i64) -> Result<Seeked, Error> {
            Ok(Seeked::At)
        }
    }

    /// Ten frames of silence per packet.
    struct Silence;

    impl Decoder for Silence {
        fn decode(&mut self, _: &Packet<'_>, out: &mut Vec<f32>) -> Result<(), Error> {
            out.extend_from_slice(&[0.0; 10]);
            Ok(())
        }
        fn format(&self) -> Option<StreamFormat> {
            Some(StreamFormat {
                sample_rate: 48_000,
                channels: 1,
            })
        }
        fn reset(&mut self) {}
    }

    impl DecoderProvider for Silence {
        fn name(&self) -> &'static str {
            "silence"
        }
        fn supports(&self, _: &CodecDescription) -> Support {
            Support::Full
        }
        fn open(&self, _: &CodecDescription) -> Result<Box<dyn Decoder>, Error> {
            Ok(Box::new(Silence))
        }
    }

    #[test]
    fn timestamps_at_the_edge_of_the_timeline_do_not_overflow() {
        let mut registry = DecoderRegistry::empty();
        registry.register(Arc::new(Silence));
        let mut codec = CodecDescription::new(Codec::Pcm);
        codec.sample_rate = Some(48_000);
        codec.channels = Some(1);
        let demuxer = Packets {
            pts: vec![i64::MAX - 5, i64::MAX, i64::MIN],
            codec,
        };
        let mut stream = DecodedStream::open(demuxer, &registry).unwrap();
        let mut out = [0f32; 64];
        while stream.read(&mut out).unwrap() != Read::End {}
        stream.seek(u64::MAX).unwrap();
    }
}
