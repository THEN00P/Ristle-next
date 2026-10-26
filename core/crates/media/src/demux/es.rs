//! Framing of self-delimiting audio elementary streams: ADTS (AAC) and MPEG
//! audio (MP1/MP2/MP3). HLS carries them inside MPEG-TS and as packed audio,
//! and both arrive in arbitrary chunks while a segment downloads, so the
//! framer is fed bytes and hands out whole frames as packets.
//!
//! Timestamps come from outside the stream (PES headers, the packed-audio
//! ID3 timestamp) as 90 kHz marks at byte offsets. A mark applies to the
//! first frame that starts at or after it; frames without one continue from
//! the previous frame.

use std::collections::VecDeque;

use super::DemuxedPacket;
use crate::Error;
use crate::codec::{Codec, CodecDescription};

/// Which elementary stream a framer reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EsKind {
    Adts,
    MpegAudio,
}

/// The 33-bit MPEG clock wraps every 26.5 hours.
const PTS_WRAP: i64 = 1 << 33;

/// A frame header's facts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Header {
    /// Bytes before the payload (the ADTS header; MPEG audio keeps its
    /// header in the packet).
    skip: usize,
    /// Bytes of the whole frame, header included.
    len: usize,
    /// Frames of audio the frame decodes to.
    samples: u32,
    rate: u32,
    channels: u16,
    codec: Codec,
    /// ADTS: the AudioSpecificConfig the header implies.
    asc: Option<[u8; 2]>,
    /// A frame the host does not decode; it is skipped whole.
    unsupported: bool,
}

const ADTS_RATES: [u32; 13] = [
    96_000, 88_200, 64_000, 48_000, 44_100, 32_000, 24_000, 22_050, 16_000, 12_000, 11_025, 8_000,
    7_350,
];

fn adts_header(b: &[u8]) -> Option<Header> {
    if b.len() < 7 || b[0] != 0xff || b[1] & 0xf6 != 0xf0 {
        return None;
    }
    let protection_absent = b[1] & 0x01 != 0;
    let object_type = (b[2] >> 6) + 1;
    let rate_index = (b[2] >> 2) & 0x0f;
    let channel_config = ((b[2] & 0x01) << 2) | (b[3] >> 6);
    let len = (usize::from(b[3] & 0x03) << 11) | (usize::from(b[4]) << 3) | usize::from(b[5] >> 5);
    let blocks = u32::from(b[6] & 0x03) + 1;
    let skip = if protection_absent { 7 } else { 9 };
    let rate = *ADTS_RATES.get(usize::from(rate_index))?;
    if len <= skip || channel_config == 0 {
        // Channel configuration 0 needs a program config element in the
        // payload, which no HLS encoder writes.
        return None;
    }
    let asc = [
        (object_type << 3) | (rate_index >> 1),
        ((rate_index & 1) << 7) | (channel_config << 3),
    ];
    Some(Header {
        skip,
        len,
        samples: 1024 * blocks,
        rate,
        channels: if channel_config == 7 {
            8
        } else {
            u16::from(channel_config)
        },
        codec: Codec::Aac,
        asc: Some(asc),
        // Several raw data blocks per frame (rare) need splitting, and with
        // CRC their header carries block positions: not supported.
        unsupported: blocks > 1,
    })
}

fn mpeg_audio_header(b: &[u8]) -> Option<Header> {
    if b.len() < 4 || b[0] != 0xff || b[1] & 0xe0 != 0xe0 {
        return None;
    }
    let version = (b[1] >> 3) & 0x03; // 0: 2.5, 2: 2, 3: 1
    let layer = (b[1] >> 1) & 0x03; // 1: III, 2: II, 3: I
    let bitrate_index = usize::from(b[2] >> 4);
    let rate_index = usize::from((b[2] >> 2) & 0x03);
    let padding = usize::from((b[2] >> 1) & 0x01);
    if version == 1 || layer == 0 || bitrate_index == 0 || bitrate_index == 15 || rate_index == 3 {
        // Reserved values, or free format (no frame length in the header).
        return None;
    }
    let mpeg1 = version == 3;
    const KBPS_V1: [[u32; 15]; 3] = [
        [
            0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448,
        ],
        [
            0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
        ],
        [
            0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
        ],
    ];
    const KBPS_V2: [[u32; 15]; 2] = [
        [
            0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256,
        ],
        [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160],
    ];
    let layer_number = 4 - layer; // 1, 2 or 3
    let kbps = if mpeg1 {
        KBPS_V1[usize::from(layer_number - 1)][bitrate_index]
    } else {
        KBPS_V2[usize::from(layer_number != 1)][bitrate_index]
    };
    let rate = [44_100, 48_000, 32_000][rate_index]
        >> match version {
            3 => 0,
            2 => 1,
            _ => 2,
        };
    let bitrate = kbps * 1000;
    let (samples, len) = match layer_number {
        1 => (384, (12 * bitrate / rate) as usize * 4 + padding * 4),
        2 => (1152, (144 * bitrate / rate) as usize + padding),
        _ if mpeg1 => (1152, (144 * bitrate / rate) as usize + padding),
        _ => (576, (72 * bitrate / rate) as usize + padding),
    };
    let codec = match layer_number {
        1 => Codec::Mp1,
        2 => Codec::Mp2,
        _ => Codec::Mp3,
    };
    let channels = if b[3] >> 6 == 3 { 1 } else { 2 };
    Some(Header {
        skip: 0,
        len,
        samples,
        rate,
        channels,
        codec,
        asc: None,
        unsupported: false,
    })
}

/// Turns elementary-stream bytes into packets.
pub(crate) struct Framer {
    kind: EsKind,
    /// Bytes not yet framed; `buffer[0]` is at stream offset `consumed`.
    buffer: Vec<u8>,
    consumed: u64,
    /// 90 kHz timestamps at stream offsets.
    marks: VecDeque<(u64, i64)>,
    /// The last mark used, for unwrapping.
    last_mark: Option<i64>,
    /// Time of the next frame, in frames at the stream's rate.
    next_pts: Option<i64>,
    codec: Option<CodecDescription>,
    /// Whether the next header is at a known boundary. After a resync the
    /// header is only trusted once the following one is seen too.
    in_sync: bool,
    skipped: u64,
}

impl Framer {
    pub(crate) fn new(kind: EsKind) -> Self {
        Self {
            kind,
            buffer: Vec::new(),
            consumed: 0,
            marks: VecDeque::new(),
            last_mark: None,
            next_pts: None,
            codec: None,
            in_sync: false,
            skipped: 0,
        }
    }

    pub(crate) fn kind(&self) -> EsKind {
        self.kind
    }

    /// The stream's codec, known once the first frame was framed.
    pub(crate) fn codec(&self) -> Option<&CodecDescription> {
        self.codec.as_ref()
    }

    /// Stream offset of the next byte `push` appends.
    fn end_offset(&self) -> u64 {
        self.consumed + self.buffer.len() as u64
    }

    /// Appends bytes. `pts` (90 kHz) applies to the first frame starting at
    /// or after the first of these bytes.
    pub(crate) fn push(&mut self, data: &[u8], pts: Option<i64>) {
        if let Some(pts) = pts {
            self.marks.push_back((self.end_offset(), pts));
        }
        self.buffer.extend_from_slice(data);
    }

    /// Drops bytes held for an incomplete frame, after a gap in the input
    /// (lost TS packets, a seek, a discontinuity). Timing restarts from the
    /// next mark.
    pub(crate) fn discontinuity(&mut self) {
        self.consumed += self.buffer.len() as u64;
        self.buffer.clear();
        self.marks.clear();
        self.next_pts = None;
        self.in_sync = false;
    }

    fn header(&self, at: usize) -> Option<Header> {
        let bytes = self.buffer.get(at..)?;
        match self.kind {
            EsKind::Adts => adts_header(bytes),
            EsKind::MpegAudio => mpeg_audio_header(bytes),
        }
    }

    /// The next whole frame. With `end` set, no more bytes are coming, so a
    /// frame at the very end is accepted without seeing the header after it.
    pub(crate) fn next_packet(&mut self, end: bool) -> Result<Option<DemuxedPacket>, Error> {
        // A loop, not recursion: a stream may hold any number of frames to
        // skip in a row.
        let header = loop {
            let Some(header) = self.next_frame(end)? else {
                return Ok(None);
            };
            if !header.unsupported {
                break header;
            }
            tracing::warn!("skipping an ADTS frame with several raw data blocks");
            // It still takes its place on the timeline and its mark.
            let pts = self.frame_pts(header.rate);
            self.next_pts = Some(pts + i64::from(header.samples));
            self.drop_front(header.len);
        };

        self.check_codec(&header)?;
        let pts = self.frame_pts(header.rate);
        self.next_pts = Some(pts + i64::from(header.samples));
        let data = self.buffer[header.skip..header.len]
            .to_vec()
            .into_boxed_slice();
        self.drop_front(header.len);
        Ok(Some(DemuxedPacket {
            data,
            pts,
            dur: u64::from(header.samples),
            trim_start: 0,
            trim_end: 0,
        }))
    }

    /// Finds the next whole frame and moves it to the front of the buffer.
    fn next_frame(&mut self, end: bool) -> Result<Option<Header>, Error> {
        let mut at = 0;
        let header = loop {
            if at + 9 > self.buffer.len() && !(end && at < self.buffer.len()) {
                self.drop_front(at);
                return Ok(None);
            }
            let Some(header) = self.header(at) else {
                at += 1;
                self.in_sync = false;
                continue;
            };
            if at + header.len > self.buffer.len() {
                if end {
                    // A truncated last frame: nothing more will complete it.
                    self.skipped += (self.buffer.len() - at) as u64;
                    self.drop_front(self.buffer.len());
                    return Ok(None);
                }
                self.drop_front(at);
                return Ok(None);
            }
            if !self.in_sync {
                // A false sync word inside a frame is likely; trust it only if
                // another header follows right after this frame.
                let following = at + header.len;
                match self.header(following) {
                    Some(next) if same_stream(&header, &next) => {}
                    None if end && following >= self.buffer.len() => {}
                    None if following + 9 > self.buffer.len() && !end => {
                        self.drop_front(at);
                        return Ok(None);
                    }
                    _ => {
                        at += 1;
                        continue;
                    }
                }
            }
            break header;
        };
        if at > 0 {
            self.skipped += at as u64;
            tracing::debug!(bytes = at, "skipped bytes to find an audio frame");
        }
        self.drop_front(at);
        self.in_sync = true;
        Ok(Some(header))
    }

    /// Bytes skipped while looking for frames.
    #[cfg(test)]
    pub(crate) fn skipped(&self) -> u64 {
        self.skipped
    }

    fn drop_front(&mut self, n: usize) {
        self.buffer.drain(..n);
        self.consumed += n as u64;
    }

    fn check_codec(&mut self, header: &Header) -> Result<(), Error> {
        match &self.codec {
            None => {
                let mut codec = CodecDescription::new(header.codec);
                codec.sample_rate = Some(header.rate);
                codec.channels = Some(header.channels);
                codec.extra_data = header.asc.map(|asc| asc.to_vec().into_boxed_slice());
                self.codec = Some(codec);
                Ok(())
            }
            Some(codec)
                if codec.codec != header.codec
                    || codec.sample_rate != Some(header.rate)
                    || codec.channels != Some(header.channels)
                    || codec.extra_data.as_deref() != header.asc.as_ref().map(|a| &a[..]) =>
            {
                // The decoder was opened for the first configuration and is
                // not reopened, also not across an HLS discontinuity: a
                // change of rate, layout or AAC profile ends the stream.
                Err(Error::UnsupportedCodec(format!(
                    "a change from {:?} {:?} Hz to {:?} {} Hz mid-stream",
                    codec.codec, codec.sample_rate, header.codec, header.rate
                )))
            }
            Some(_) => Ok(()),
        }
    }

    /// The frame's time in frames at `rate`: from the latest mark at or
    /// before its first byte, else continuing from the previous frame.
    fn frame_pts(&mut self, rate: u32) -> i64 {
        let mut mark = None;
        while let Some(&(offset, pts)) = self.marks.front() {
            if offset > self.consumed {
                break;
            }
            mark = Some(pts);
            self.marks.pop_front();
        }
        match (mark, self.next_pts) {
            (Some(pts), _) => {
                let pts = self.unwrap(pts);
                // To the nearest frame: a 90 kHz clock does not divide
                // audio frame boundaries exactly.
                (pts * i64::from(rate) + 45_000).div_euclid(90_000)
            }
            (None, Some(next)) => next,
            (None, None) => 0,
        }
    }

    fn unwrap(&mut self, pts: i64) -> i64 {
        let unwrapped = match self.last_mark {
            Some(last) => {
                let mut candidate = pts + (last.div_euclid(PTS_WRAP)) * PTS_WRAP;
                if candidate < last - PTS_WRAP / 2 {
                    candidate += PTS_WRAP;
                } else if candidate > last + PTS_WRAP / 2 {
                    candidate -= PTS_WRAP;
                }
                candidate
            }
            None => pts,
        };
        self.last_mark = Some(unwrapped);
        unwrapped
    }
}

fn same_stream(a: &Header, b: &Header) -> bool {
    a.codec == b.codec && a.rate == b.rate && a.channels == b.channels
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// An ADTS frame of `payload` bytes: AAC-LC, 44.1 kHz, stereo.
    pub(crate) fn adts_frame(payload: &[u8]) -> Vec<u8> {
        let len = payload.len() + 7;
        let mut frame = vec![
            0xff,
            0xf1,
            (1 << 6) | (4 << 2),
            (2 << 6) | ((len >> 11) & 0x03) as u8,
            ((len >> 3) & 0xff) as u8,
            (((len & 0x07) << 5) as u8) | 0x1f,
            0xfc,
        ];
        frame.extend_from_slice(payload);
        frame
    }

    fn drain(framer: &mut Framer, end: bool) -> Vec<DemuxedPacket> {
        std::iter::from_fn(|| framer.next_packet(end).unwrap()).collect()
    }

    #[test]
    fn adts_frames_split_across_pushes_come_out_whole() {
        let stream: Vec<u8> = (0..4u8).flat_map(|i| adts_frame(&[i; 50])).collect();
        let mut framer = Framer::new(EsKind::Adts);
        let mut packets = Vec::new();
        for chunk in stream.chunks(13) {
            framer.push(chunk, None);
            packets.extend(drain(&mut framer, false));
        }
        packets.extend(drain(&mut framer, true));
        assert_eq!(packets.len(), 4);
        for (i, packet) in packets.iter().enumerate() {
            assert_eq!(&*packet.data, &[i as u8; 50]);
            assert_eq!(packet.pts, i as i64 * 1024);
            assert_eq!(packet.dur, 1024);
        }
        let codec = framer.codec().unwrap();
        assert_eq!(codec.sample_rate, Some(44_100));
        assert_eq!(codec.channels, Some(2));
        assert_eq!(codec.aac_object_type(), Some(2));
        assert_eq!(codec.aac_core_rate(), Some(44_100));
    }

    #[test]
    fn garbage_before_and_false_syncs_inside_are_skipped() {
        // The payload contains a fake sync word with a plausible header.
        let mut payload = vec![0u8; 60];
        payload[10..17].copy_from_slice(&adts_frame(&[0; 3])[..7]);
        // Start mid-frame, before the fake header.
        let mut stream = payload[5..].to_vec();
        for _ in 0..3 {
            stream.extend(adts_frame(&payload));
        }
        let mut framer = Framer::new(EsKind::Adts);
        framer.push(&stream, None);
        let packets = drain(&mut framer, true);
        assert_eq!(packets.len(), 3);
        assert!(packets.iter().all(|p| p.data.len() == 60));
        assert_eq!(framer.skipped(), 55);
    }

    #[test]
    fn marks_time_the_first_frame_at_or_after_them() {
        let frame = adts_frame(&[7; 30]);
        let mut framer = Framer::new(EsKind::Adts);
        // 90 kHz: 2 s.
        framer.push(&frame, Some(180_000));
        framer.push(&frame[..10], None);
        // Marks the middle of the second frame, so it applies to the third.
        framer.push(&frame[10..], Some(999_999_999));
        framer.push(&frame, None);
        let packets = drain(&mut framer, true);
        let pts: Vec<i64> = packets.iter().map(|p| p.pts).collect();
        let from_mark = (999_999_999 * 44_100 + 45_000) / 90_000;
        assert_eq!(pts, vec![88_200, 88_200 + 1024, from_mark]);
    }

    #[test]
    fn timestamps_unwrap_across_the_33_bit_boundary() {
        let frame = adts_frame(&[1; 20]);
        let mut framer = Framer::new(EsKind::Adts);
        framer.push(&frame, Some(PTS_WRAP - 1920));
        framer.push(&frame, Some(0));
        let packets = drain(&mut framer, true);
        assert_eq!(
            packets[1].pts - packets[0].pts,
            (PTS_WRAP * 44_100 / 90_000) - ((PTS_WRAP - 1920) * 44_100 / 90_000)
        );
    }

    #[test]
    fn skipped_frames_keep_their_place_on_the_timeline() {
        let mut multi = adts_frame(&[9; 30]);
        multi[6] |= 0x01; // two raw data blocks: 2048 frames
        let mut framer = Framer::new(EsKind::Adts);
        framer.push(&adts_frame(&[1; 30]), Some(0));
        framer.push(&multi, Some(90_000));
        framer.push(&adts_frame(&[2; 30]), None);
        let packets = drain(&mut framer, true);
        assert_eq!(packets[1].pts, 44_100 + 2048);
        // A run of skipped frames leaves no marks behind.
        let mut framer = Framer::new(EsKind::Adts);
        for i in 0..100 {
            framer.push(&multi, Some(i * 4000));
        }
        assert!(drain(&mut framer, true).is_empty());
        assert!(framer.marks.is_empty());
    }

    #[test]
    fn frames_with_several_raw_data_blocks_are_skipped() {
        let mut multi = adts_frame(&[9; 30]);
        multi[6] |= 0x01; // two raw data blocks
        let mut framer = Framer::new(EsKind::Adts);
        framer.push(&adts_frame(&[1; 30]), None);
        framer.push(&multi, None);
        framer.push(&adts_frame(&[2; 30]), None);
        let packets = drain(&mut framer, true);
        let firsts: Vec<u8> = packets.iter().map(|p| p.data[0]).collect();
        assert_eq!(firsts, vec![1, 2]);
    }

    #[test]
    fn long_runs_of_skipped_frames_do_not_recurse() {
        let mut multi = adts_frame(&[]);
        multi[6] |= 0x01;
        let mut framer = Framer::new(EsKind::Adts);
        for _ in 0..200_000 {
            framer.push(&multi, None);
        }
        framer.push(&adts_frame(&[4; 10]), None);
        let packets = drain(&mut framer, true);
        assert_eq!(packets.len(), 1);
    }

    #[test]
    fn a_profile_change_after_a_discontinuity_is_refused() {
        let mut main = adts_frame(&[0; 20]);
        main[2] &= 0x3f; // object type 1 (AAC Main), same rate and layout
        let mut framer = Framer::new(EsKind::Adts);
        framer.push(&adts_frame(&[0; 20]), None);
        assert!(framer.next_packet(true).unwrap().is_some());
        framer.discontinuity();
        framer.push(&main, None);
        framer.push(&main, None);
        assert!(matches!(
            framer.next_packet(true),
            Err(Error::UnsupportedCodec(_))
        ));
    }

    #[test]
    fn a_rate_change_without_a_discontinuity_is_an_error() {
        let mut other = adts_frame(&[0; 20]);
        other[2] = (1 << 6) | (3 << 2); // 48 kHz
        let mut framer = Framer::new(EsKind::Adts);
        framer.push(&adts_frame(&[0; 20]), None);
        framer.push(&adts_frame(&[0; 20]), None);
        framer.push(&other, None);
        assert!(framer.next_packet(true).unwrap().is_some());
        assert!(framer.next_packet(true).unwrap().is_some());
        assert!(framer.next_packet(true).is_err());
    }

    #[test]
    fn mpeg_audio_headers_give_frame_lengths_and_durations() {
        // MPEG-1 Layer III, 128 kbps, 44.1 kHz, no padding, stereo: 417 bytes.
        let h = mpeg_audio_header(&[0xff, 0xfb, 0x90, 0x00]).unwrap();
        assert_eq!(
            (h.len, h.samples, h.rate, h.channels, h.codec),
            (417, 1152, 44_100, 2, Codec::Mp3)
        );
        // MPEG-2 Layer III, 64 kbps, 22.05 kHz, padded, mono.
        let h = mpeg_audio_header(&[0xff, 0xf3, 0x82, 0xc0]).unwrap();
        assert_eq!(
            (h.len, h.samples, h.rate, h.channels),
            (209, 576, 22_050, 1)
        );
        // MPEG-1 Layer II, 192 kbps, 48 kHz.
        let h = mpeg_audio_header(&[0xff, 0xfd, 0xa4, 0x00]).unwrap();
        assert_eq!((h.len, h.samples, h.codec), (576, 1152, Codec::Mp2));
        // Free format and reserved values are not frames.
        assert_eq!(mpeg_audio_header(&[0xff, 0xfb, 0x00, 0x00]), None);
        assert_eq!(mpeg_audio_header(&[0xff, 0xfb, 0x9c, 0x00]), None);
    }

    #[test]
    fn a_discontinuity_drops_the_partial_frame_and_its_timing() {
        let frame = adts_frame(&[3; 40]);
        let mut framer = Framer::new(EsKind::Adts);
        framer.push(&frame, Some(90_000));
        framer.push(&frame[..20], None);
        assert_eq!(drain(&mut framer, false).len(), 1);
        framer.discontinuity();
        framer.push(&frame, Some(0));
        framer.push(&frame, None);
        let packets = drain(&mut framer, true);
        assert_eq!(
            packets.iter().map(|p| p.pts).collect::<Vec<_>>(),
            vec![0, 1024]
        );
    }
}
