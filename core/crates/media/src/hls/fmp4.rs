//! Fragmented MP4 segments: `moof` sample tables and the samples they point
//! into, cut from `mdat` while the segment downloads.
//!
//! Host requirements from the spike B review (ADR 0004): only the selected
//! audio track's `traf` is read (muxed variants put several in one `moof`),
//! and a `trun` without `data_offset` continues where the previous run ended
//! (ISO/IEC 14496-12, 8.8.8), which matters because Apple's packager writes
//! several runs per fragment.

use std::collections::VecDeque;

use crate::Error;
use crate::demux::DemuxedPacket;
use crate::demux::mp4::{be32, be64, boxes, child, find_trak, timescale};

/// What the init segment says about the selected track.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TrackInfo {
    pub(crate) track_id: u32,
    pub(crate) timescale: u32,
    /// `trex` defaults, used when a fragment does not override them.
    pub(crate) default_duration: u32,
    pub(crate) default_size: u32,
}

impl TrackInfo {
    pub(crate) fn from_moov(moov: &[u8], track_id: u32) -> Result<Self, Error> {
        let trak = find_trak(moov, track_id)?
            .ok_or_else(|| malformed(format!("no trak for track {track_id}")))?;
        let timescale = child(trak, b"mdia")?
            .map(|mdia| child(mdia, b"mdhd"))
            .transpose()?
            .flatten()
            .and_then(timescale)
            .ok_or_else(|| malformed("no media timescale"))?;
        let mut info = Self {
            track_id,
            timescale,
            default_duration: 0,
            default_size: 0,
        };
        if let Some(mvex) = child(moov, b"mvex")? {
            for item in boxes(mvex) {
                let (kind, trex) = item?;
                // version/flags, track_ID, description index, duration, size.
                if &kind == b"trex" && be32(trex, 4) == Some(track_id) {
                    info.default_duration = be32(trex, 12).unwrap_or(0);
                    info.default_size = be32(trex, 16).unwrap_or(0);
                }
            }
        }
        Ok(info)
    }
}

fn malformed(detail: impl Into<String>) -> Error {
    Error::malformed("fMP4 fragment", detail)
}

/// One sample of the selected track.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Sample {
    /// Byte offset within the segment.
    pub(crate) offset: u64,
    pub(crate) size: u32,
    pub(crate) duration: u32,
}

/// A `moof`'s samples for the selected track.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Fragment {
    /// `tfdt`: decode time of the first sample, in the track's timescale.
    pub(crate) decode_time: Option<u64>,
    pub(crate) samples: Vec<Sample>,
}

/// Parses a `moof` body that starts at segment offset `moof_offset` (where
/// its header starts).
pub(crate) fn parse_moof(
    moof: &[u8],
    moof_offset: u64,
    track: &TrackInfo,
) -> Result<Fragment, Error> {
    let mut fragment = Fragment::default();
    // Without base-data-offset or default-base-is-moof, a traf's data starts
    // where the previous traf's data ended (8.8.7.1); the first one at the
    // moof.
    let mut previous_traf_end = moof_offset;
    for item in boxes(moof) {
        let (kind, traf) = item?;
        if &kind != b"traf" {
            continue;
        }
        let tfhd = child(traf, b"tfhd")?.ok_or_else(|| malformed("traf without tfhd"))?;
        let flags = be32(tfhd, 0).ok_or_else(|| malformed("short tfhd"))? & 0x00ff_ffff;
        let track_id = be32(tfhd, 4).ok_or_else(|| malformed("short tfhd"))?;
        let mut at = 8;
        let mut field = |len: usize| -> Result<usize, Error> {
            let start = at;
            at += len;
            if tfhd.len() < at {
                return Err(malformed("short tfhd"));
            }
            Ok(start)
        };
        let base_data_offset = if flags & 0x01 != 0 {
            be64(tfhd, field(8)?)
        } else {
            None
        };
        if flags & 0x02 != 0 {
            field(4)?;
        }
        let default_duration = if flags & 0x08 != 0 {
            be32(tfhd, field(4)?)
        } else {
            None
        };
        let default_size = if flags & 0x10 != 0 {
            be32(tfhd, field(4)?)
        } else {
            None
        };
        let base = match base_data_offset {
            Some(offset) => offset,
            None if flags & 0x02_0000 != 0 => moof_offset,
            None => previous_traf_end,
        };
        let selected = track_id == track.track_id;
        let default_duration = default_duration.unwrap_or(track.default_duration);
        let default_size = default_size.unwrap_or(track.default_size);

        if selected && let Some(tfdt) = child(traf, b"tfdt")? {
            fragment.decode_time = match tfdt.first() {
                Some(1) => be64(tfdt, 4),
                _ => be32(tfdt, 4).map(u64::from),
            };
        }
        let mut run_end = base;
        for item in boxes(traf) {
            let (kind, trun) = item?;
            if &kind != b"trun" {
                continue;
            }
            let bad = || malformed("short trun");
            let flags = be32(trun, 0).ok_or_else(bad)? & 0x00ff_ffff;
            let count = be32(trun, 4).ok_or_else(bad)? as usize;
            let mut at = 8;
            let mut offset = run_end;
            if flags & 0x01 != 0 {
                let data_offset = be32(trun, at).ok_or_else(bad)? as i32;
                offset = base
                    .checked_add_signed(i64::from(data_offset))
                    .ok_or_else(|| malformed("data offset before the segment"))?;
                at += 4;
            }
            if flags & 0x04 != 0 {
                at += 4;
            }
            let per_sample = [0x100, 0x200, 0x400, 0x800]
                .iter()
                .filter(|&&f| flags & f != 0)
                .count()
                * 4;
            if trun.len() < at + count * per_sample {
                return Err(bad());
            }
            for _ in 0..count {
                let mut read = |present: bool, default: u32| -> u32 {
                    if present {
                        let value = be32(trun, at).unwrap_or(0);
                        at += 4;
                        value
                    } else {
                        default
                    }
                };
                let duration = read(flags & 0x100 != 0, default_duration);
                let size = read(flags & 0x200 != 0, default_size);
                read(flags & 0x400 != 0, 0);
                read(flags & 0x800 != 0, 0);
                if selected {
                    fragment.samples.push(Sample {
                        offset,
                        size,
                        duration,
                    });
                }
                offset += u64::from(size);
            }
            run_end = offset;
        }
        previous_traf_end = run_end;
    }
    Ok(fragment)
}

/// A box's kind, header length and total size (`None`: to the end).
type BoxHeader = ([u8; 4], u64, Option<u64>);

/// Where a segment parser is in the box structure.
enum State {
    /// Expecting a box header at `next_box`.
    Boxes,
    /// Collecting a `moof` that starts at `start` and is `size` long.
    Moof { start: u64, size: u64 },
    /// Inside an `mdat` that ends at `end` (`None`: the end of the segment).
    Mdat { end: Option<u64> },
    /// Skipping a box that ends at `end`.
    Skip { end: Option<u64> },
}

/// Cuts the selected track's samples out of one segment, fed in chunks.
pub(crate) struct SegmentParser {
    track: TrackInfo,
    /// Unconsumed bytes; `buffer[0]` is at segment offset `buffer_start`.
    buffer: Vec<u8>,
    buffer_start: u64,
    state: State,
    /// Samples announced by the last `moof`, with their decode times.
    samples: VecDeque<(Sample, u64)>,
    /// Decode time of the next sample when a `moof` has no `tfdt`.
    next_time: u64,
    ready: VecDeque<DemuxedPacket>,
}

impl SegmentParser {
    pub(crate) fn new(track: TrackInfo) -> Self {
        Self {
            track,
            buffer: Vec::new(),
            buffer_start: 0,
            state: State::Boxes,
            samples: VecDeque::new(),
            next_time: 0,
            ready: VecDeque::new(),
        }
    }

    /// A parser that starts inside an `mdat` at a sample boundary, for a
    /// byte-range seek: the bytes fed start at `samples[0].offset`.
    pub(crate) fn resume(
        track: TrackInfo,
        samples: Vec<(Sample, u64)>,
        mdat_end: Option<u64>,
    ) -> Self {
        let buffer_start = samples.first().map_or(0, |(s, _)| s.offset);
        let next_time = samples.last().map_or(0, |(s, t)| t + u64::from(s.duration));
        Self {
            track,
            buffer: Vec::new(),
            buffer_start,
            state: State::Mdat { end: mdat_end },
            samples: samples.into(),
            next_time,
            ready: VecDeque::new(),
        }
    }

    /// Feeds segment bytes; whole samples become packets whose `pts` is the
    /// decode time in the track's timescale.
    pub(crate) fn push(&mut self, data: &[u8]) -> Result<(), Error> {
        self.buffer.extend_from_slice(data);
        self.advance(false)
    }

    /// The end of the segment.
    pub(crate) fn finish(&mut self) -> Result<(), Error> {
        self.advance(true)?;
        if !self.samples.is_empty() {
            tracing::warn!(
                missing = self.samples.len(),
                "segment ended before its samples"
            );
            self.samples.clear();
        }
        Ok(())
    }

    pub(crate) fn next_packet(&mut self) -> Option<DemuxedPacket> {
        self.ready.pop_front()
    }

    fn buffer_end(&self) -> u64 {
        self.buffer_start + self.buffer.len() as u64
    }

    fn consume_to(&mut self, offset: u64) {
        let n = offset
            .saturating_sub(self.buffer_start)
            .min(self.buffer.len() as u64) as usize;
        self.buffer.drain(..n);
        self.buffer_start += n as u64;
    }

    fn advance(&mut self, at_end: bool) -> Result<(), Error> {
        loop {
            match self.state {
                State::Boxes => {
                    let Some((kind, header, size)) = self.box_header()? else {
                        if at_end && !self.buffer.is_empty() {
                            return Err(malformed("a truncated box at the end of the segment"));
                        }
                        return Ok(());
                    };
                    let start = self.buffer_start;
                    let end = size.map(|size| start + size);
                    match &kind {
                        b"moof" => {
                            let size = size.ok_or_else(|| malformed("a moof without a size"))?;
                            self.state = State::Moof { start, size };
                        }
                        b"mdat" => {
                            self.consume_to(start + header);
                            self.state = State::Mdat { end };
                        }
                        _ => self.state = State::Skip { end },
                    }
                }
                State::Moof { start, size } => {
                    if self.buffer_end() < start + size {
                        if at_end {
                            return Err(malformed("a truncated moof"));
                        }
                        return Ok(());
                    }
                    let header = if be32(&self.buffer, 0) == Some(1) {
                        16
                    } else {
                        8
                    };
                    let body = &self.buffer[header..size as usize];
                    let fragment = parse_moof(body, start, &self.track)?;
                    let mut time = fragment.decode_time.unwrap_or(self.next_time);
                    for sample in fragment.samples {
                        self.samples.push_back((sample, time));
                        time += u64::from(sample.duration);
                    }
                    self.next_time = time;
                    self.consume_to(start + size);
                    self.state = State::Boxes;
                }
                State::Mdat { end } => {
                    self.cut_samples(end);
                    let finished = match end {
                        Some(end) => self.buffer_end() >= end,
                        None => at_end,
                    };
                    if !finished {
                        return Ok(());
                    }
                    if let Some(end) = end {
                        self.consume_to(end);
                    } else {
                        self.consume_to(self.buffer_end());
                    }
                    self.state = State::Boxes;
                }
                State::Skip { end } => match end {
                    Some(end) if self.buffer_end() >= end => {
                        self.consume_to(end);
                        self.state = State::Boxes;
                    }
                    _ => {
                        // Keep nothing of a box being skipped.
                        self.consume_to(self.buffer_end());
                        return Ok(());
                    }
                },
            }
        }
    }

    /// Box kind, header length and total size (`None`: to the end).
    fn box_header(&self) -> Result<Option<BoxHeader>, Error> {
        if self.buffer.len() < 8 {
            return Ok(None);
        }
        let size32 = be32(&self.buffer, 0).unwrap_or(0);
        let kind = [
            self.buffer[4],
            self.buffer[5],
            self.buffer[6],
            self.buffer[7],
        ];
        let (header, size) = match size32 {
            0 => (8, None),
            1 => match be64(&self.buffer, 8) {
                Some(size) => (16, Some(size)),
                None => return Ok(None),
            },
            n => (8, Some(u64::from(n))),
        };
        if size.is_some_and(|size| size < header) {
            return Err(malformed(format!("a {size:?}-byte box")));
        }
        Ok(Some((kind, header, size)))
    }

    /// Emits every announced sample that is buffered, and drops the bytes
    /// before the next one (other tracks' samples, gaps between runs).
    fn cut_samples(&mut self, mdat_end: Option<u64>) {
        while let Some(&(sample, time)) = self.samples.front() {
            let sample_end = sample.offset + u64::from(sample.size);
            if sample.offset < self.buffer_start || mdat_end.is_some_and(|end| sample_end > end) {
                tracing::warn!(
                    offset = sample.offset,
                    "a sample outside its mdat; dropping it"
                );
                self.samples.pop_front();
                continue;
            }
            if self.buffer_end() < sample_end {
                // Drop what lies before the sample; it is not ours.
                self.consume_to(sample.offset);
                return;
            }
            let from = (sample.offset - self.buffer_start) as usize;
            let data = self.buffer[from..from + sample.size as usize]
                .to_vec()
                .into_boxed_slice();
            self.ready.push_back(DemuxedPacket {
                data,
                pts: i64::try_from(time).unwrap_or(i64::MAX),
                dur: u64::from(sample.duration),
                trim_start: 0,
                trim_end: 0,
            });
            self.samples.pop_front();
            self.consume_to(sample_end);
        }
        // No sample pending: nothing in the buffer is needed.
        if let Some(end) = mdat_end {
            self.consume_to(end.min(self.buffer_end()));
        } else {
            self.consume_to(self.buffer_end());
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::demux::mp4::tests::mp4_box;

    pub(crate) fn full(flags: u32, rest: &[u8]) -> Vec<u8> {
        let mut out = flags.to_be_bytes().to_vec();
        out.extend_from_slice(rest);
        out
    }

    /// A trun with explicit durations and sizes, and optionally a data offset.
    pub(crate) fn trun(data_offset: Option<i32>, samples: &[(u32, u32)]) -> Vec<u8> {
        let flags = 0x300 | if data_offset.is_some() { 0x01 } else { 0 };
        let mut rest = (samples.len() as u32).to_be_bytes().to_vec();
        if let Some(offset) = data_offset {
            rest.extend_from_slice(&offset.to_be_bytes());
        }
        for (duration, size) in samples {
            rest.extend_from_slice(&duration.to_be_bytes());
            rest.extend_from_slice(&size.to_be_bytes());
        }
        mp4_box(b"trun", &full(flags, &rest))
    }

    pub(crate) fn traf(track_id: u32, tfdt: u64, truns: &[Vec<u8>]) -> Vec<u8> {
        // default-base-is-moof.
        let mut body = mp4_box(b"tfhd", &full(0x02_0000, &track_id.to_be_bytes()));
        body.extend(mp4_box(
            b"tfdt",
            &[&[1u8, 0, 0, 0][..], &tfdt.to_be_bytes()].concat(),
        ));
        for trun in truns {
            body.extend_from_slice(trun);
        }
        mp4_box(b"traf", &body)
    }

    const AUDIO: TrackInfo = TrackInfo {
        track_id: 2,
        timescale: 44_100,
        default_duration: 1024,
        default_size: 0,
    };

    /// A segment: styp, then a moof with a video traf (track 1) and an
    /// audio traf (track 2) with two runs (the second without a data offset)
    /// over an mdat holding video, then the audio samples.
    pub(crate) fn muxed_segment(audio: &[&[u8]]) -> Vec<u8> {
        let video = vec![0xee; 300];
        let video_run = trun(Some(0), &[(3000, 300)]);
        let first: Vec<(u32, u32)> = audio[..2].iter().map(|s| (1024, s.len() as u32)).collect();
        let second: Vec<(u32, u32)> = audio[2..].iter().map(|s| (1024, s.len() as u32)).collect();
        let build = |video_offset: i32, audio_offset: i32| {
            let mut moof = traf(1, 0, &[trun(Some(video_offset), &[(3000, 300)])]);
            moof.extend(traf(
                2,
                88_200,
                &[trun(Some(audio_offset), &first), trun(None, &second)],
            ));
            mp4_box(b"moof", &moof)
        };
        let _ = video_run;
        let styp = mp4_box(b"styp", b"msdh\0\0\0\0msdhmsix");
        // Offsets are relative to the moof start; compute with a first pass.
        let moof_len = build(0, 0).len() as i32;
        let data = moof_len + 8;
        let moof = build(data, data + 300);
        let mut mdat_body = video;
        for sample in audio {
            mdat_body.extend_from_slice(sample);
        }
        [styp, moof, mp4_box(b"mdat", &mdat_body)].concat()
    }

    fn samples() -> Vec<Vec<u8>> {
        (0..5u8).map(|i| vec![i; 40 + usize::from(i)]).collect()
    }

    #[test]
    fn only_the_selected_tracks_samples_come_out_across_runs() {
        let audio = samples();
        let refs: Vec<&[u8]> = audio.iter().map(Vec::as_slice).collect();
        let segment = muxed_segment(&refs);
        for chunk in [1, 7, 64, segment.len()] {
            let mut parser = SegmentParser::new(AUDIO);
            let mut packets = Vec::new();
            for piece in segment.chunks(chunk) {
                parser.push(piece).unwrap();
                packets.extend(std::iter::from_fn(|| parser.next_packet()));
            }
            parser.finish().unwrap();
            packets.extend(std::iter::from_fn(|| parser.next_packet()));
            let data: Vec<Vec<u8>> = packets.iter().map(|p| p.data.to_vec()).collect();
            assert_eq!(data, audio, "chunk {chunk}");
            let pts: Vec<i64> = packets.iter().map(|p| p.pts).collect();
            assert_eq!(pts, vec![88_200, 89_224, 90_248, 91_272, 92_296]);
        }
    }

    #[test]
    fn a_run_without_data_offset_continues_the_previous_run() {
        let audio = samples();
        let refs: Vec<&[u8]> = audio.iter().map(Vec::as_slice).collect();
        let segment = muxed_segment(&refs);
        let moof_start = segment.windows(4).position(|w| w == b"moof").unwrap() - 4;
        let size = be32(&segment, moof_start).unwrap() as usize;
        let fragment = parse_moof(
            &segment[moof_start + 8..moof_start + size],
            moof_start as u64,
            &AUDIO,
        )
        .unwrap();
        let offsets: Vec<u64> = fragment.samples.iter().map(|s| s.offset).collect();
        let mdat_body = (moof_start + size + 8) as u64;
        let mut expected = Vec::new();
        let mut at = mdat_body + 300;
        for sample in &audio {
            expected.push(at);
            at += sample.len() as u64;
        }
        assert_eq!(offsets, expected);
        assert_eq!(fragment.decode_time, Some(88_200));
    }

    #[test]
    fn resuming_inside_an_mdat_cuts_from_the_given_sample() {
        let audio = samples();
        let refs: Vec<&[u8]> = audio.iter().map(Vec::as_slice).collect();
        let segment = muxed_segment(&refs);
        let moof_start = segment.windows(4).position(|w| w == b"moof").unwrap() - 4;
        let size = be32(&segment, moof_start).unwrap() as usize;
        let fragment = parse_moof(
            &segment[moof_start + 8..moof_start + size],
            moof_start as u64,
            &AUDIO,
        )
        .unwrap();
        let from = 3;
        let mut time = 88_200;
        let mut timed = Vec::new();
        for sample in &fragment.samples {
            timed.push((*sample, time));
            time += u64::from(sample.duration);
        }
        let mut parser =
            SegmentParser::resume(AUDIO, timed[from..].to_vec(), Some(segment.len() as u64));
        parser
            .push(&segment[fragment.samples[from].offset as usize..])
            .unwrap();
        parser.finish().unwrap();
        let packets: Vec<_> = std::iter::from_fn(|| parser.next_packet()).collect();
        assert_eq!(packets.len(), 2);
        assert_eq!(&*packets[0].data, audio[3].as_slice());
        assert_eq!(packets[0].pts, 88_200 + 3 * 1024);
    }

    #[test]
    fn trex_defaults_apply_when_the_fragment_has_none() {
        let mut trak = mp4_box(
            b"tkhd",
            &[&[0u8; 12][..], &7u32.to_be_bytes(), &[0u8; 68]].concat(),
        );
        let mut mdhd = vec![0u8; 12];
        mdhd.extend_from_slice(&48_000u32.to_be_bytes());
        mdhd.extend_from_slice(&[0u8; 8]);
        trak.extend(mp4_box(b"mdia", &mp4_box(b"mdhd", &mdhd)));
        let trex = full(
            0,
            &[
                &7u32.to_be_bytes()[..],
                &1u32.to_be_bytes(),
                &960u32.to_be_bytes(),
                &120u32.to_be_bytes(),
                &0u32.to_be_bytes(),
            ]
            .concat(),
        );
        let moov = [
            mp4_box(b"trak", &trak),
            mp4_box(b"mvex", &mp4_box(b"trex", &trex)),
        ]
        .concat();
        let info = TrackInfo::from_moov(&moov, 7).unwrap();
        assert_eq!(
            info,
            TrackInfo {
                track_id: 7,
                timescale: 48_000,
                default_duration: 960,
                default_size: 120
            }
        );
    }

    #[test]
    fn a_truncated_trun_is_malformed() {
        let mut bad = traf(2, 0, &[trun(Some(0), &[(1024, 10), (1024, 10)])]);
        let len = bad.len();
        bad.truncate(len - 4);
        // Fix up the box sizes so only the trun is short.
        let traf_len = bad.len() as u32;
        bad[..4].copy_from_slice(&traf_len.to_be_bytes());
        let trun_at = bad.windows(4).position(|w| w == b"trun").unwrap() - 4;
        let trun_len = (bad.len() - trun_at) as u32;
        bad[trun_at..trun_at + 4].copy_from_slice(&trun_len.to_be_bytes());
        assert!(parse_moof(&bad, 0, &AUDIO).is_err());
    }
}
