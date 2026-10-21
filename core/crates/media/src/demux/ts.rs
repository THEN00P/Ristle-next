//! MPEG-TS demuxing for HLS audio (ADR 0004 amendment 4): live radio serves
//! TS, and Symphonia has no TS reader.
//!
//! PAT → PMT → the first audio elementary stream the host decodes (ADTS AAC
//! or MPEG audio). Its PES payloads go to a [`Framer`] with the PES
//! timestamps as marks. Bytes may arrive in any chunking. Video, ID3 timed
//! metadata and other streams are skipped. A lost packet (continuity
//! counter gap) drops the frame it damaged and nothing else.

use std::io::{Read, Seek, SeekFrom};

use super::es::{EsKind, Framer};
use super::{Demuxed, DemuxedPacket, Demuxer, Seeked, TimeBase, Window};
use crate::Error;
use crate::codec::CodecDescription;

const PACKET: usize = 188;
const SYNC: u8 = 0x47;

/// What a PMT stream type means to the host.
enum StreamType {
    Audio(EsKind),
    /// Audio the host has no decoder for (LATM, AC-3, E-AC-3).
    UnsupportedAudio(&'static str),
    /// Audio encrypted with SAMPLE-AES, which needs keys only DRM provides.
    EncryptedAudio,
    Other,
}

fn stream_type(code: u8) -> StreamType {
    match code {
        0x0f => StreamType::Audio(EsKind::Adts),
        0x03 | 0x04 => StreamType::Audio(EsKind::MpegAudio),
        0x11 => StreamType::UnsupportedAudio("AAC in LATM"),
        0x81 => StreamType::UnsupportedAudio("AC-3"),
        0x87 => StreamType::UnsupportedAudio("E-AC-3"),
        0xcf | 0xc1 | 0xc2 => StreamType::EncryptedAudio,
        _ => StreamType::Other,
    }
}

/// A PSI section being collected across packets.
#[derive(Default)]
struct Section {
    bytes: Vec<u8>,
    started: bool,
}

impl Section {
    /// Feeds a packet payload. Returns complete sections.
    fn push(&mut self, payload: &[u8], unit_start: bool) -> Vec<Vec<u8>> {
        let mut complete = Vec::new();
        let mut rest = payload;
        if unit_start {
            let Some((&pointer, after)) = rest.split_first() else {
                return complete;
            };
            let pointer = usize::from(pointer);
            if self.started {
                // The bytes before the pointer finish the previous section.
                self.bytes
                    .extend_from_slice(after.get(..pointer).unwrap_or(after));
                self.take_complete(&mut complete);
            }
            rest = after.get(pointer..).unwrap_or_default();
            self.bytes.clear();
            self.started = true;
        }
        if !self.started {
            return complete;
        }
        self.bytes.extend_from_slice(rest);
        self.take_complete(&mut complete);
        complete
    }

    fn take_complete(&mut self, complete: &mut Vec<Vec<u8>>) {
        loop {
            if self.bytes.first().is_none_or(|&table| table == 0xff) {
                // Stuffing: nothing more in this payload.
                self.bytes.clear();
                return;
            }
            let Some(length) = section_length(&self.bytes) else {
                return;
            };
            if self.bytes.len() < length {
                return;
            }
            complete.push(self.bytes.drain(..length).collect());
        }
    }
}

fn section_length(bytes: &[u8]) -> Option<usize> {
    let high = usize::from(*bytes.get(1)? & 0x0f);
    let low = usize::from(*bytes.get(2)?);
    Some(3 + ((high << 8) | low))
}

/// CRC-32/MPEG-2 over a whole section including its CRC is zero.
fn crc_ok(section: &[u8]) -> bool {
    let mut crc = 0xffff_ffffu32;
    for &byte in section {
        crc ^= u32::from(byte) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04c1_1db7
            } else {
                crc << 1
            };
        }
    }
    crc == 0
}

/// The selected audio stream.
struct Audio {
    pid: u16,
    /// PES header bytes until the header is complete.
    header: Vec<u8>,
    in_payload: bool,
    pts: Option<i64>,
}

/// Turns TS bytes into the selected audio stream's packets.
pub(crate) struct TsDemuxer {
    /// Bytes of an incomplete TS packet.
    carry: Vec<u8>,
    pat: Section,
    pmt_pid: Option<u16>,
    pmt: Section,
    audio: Option<Audio>,
    framer: Option<Framer>,
    /// Whole frames salvaged before a discontinuity reset the framer.
    ready: std::collections::VecDeque<DemuxedPacket>,
    /// Last continuity counter per PID that carries payload.
    continuity: std::collections::HashMap<u16, u8>,
    /// Why the PMT offers nothing playable, if it does not.
    unplayable: Option<Error>,
    /// The last packet was parsed at a known packet boundary.
    in_sync: bool,
    ended: bool,
}

impl TsDemuxer {
    pub(crate) fn new() -> Self {
        Self {
            carry: Vec::new(),
            pat: Section::default(),
            pmt_pid: None,
            pmt: Section::default(),
            audio: None,
            framer: None,
            ready: std::collections::VecDeque::new(),
            continuity: std::collections::HashMap::new(),
            unplayable: None,
            in_sync: false,
            ended: false,
        }
    }

    /// Whether `head` starts like a transport stream.
    pub(crate) fn sniff(head: &[u8]) -> bool {
        head.first() == Some(&SYNC) && head.get(PACKET) == Some(&SYNC)
    }

    /// The audio codec, once the first frame was framed.
    pub(crate) fn codec(&self) -> Option<&CodecDescription> {
        self.framer.as_ref().and_then(Framer::codec)
    }

    /// Feeds TS bytes in any chunking.
    pub(crate) fn push(&mut self, data: &[u8]) -> Result<(), Error> {
        self.carry.extend_from_slice(data);
        self.parse_carry(false)
    }

    /// Parses the whole packets in `carry`. Out of sync, a candidate packet
    /// is trusted only once the next one starts 188 bytes later; at the end
    /// of input (`last`) there is no next one to wait for.
    fn parse_carry(&mut self, last: bool) -> Result<(), Error> {
        let mut at = 0;
        while at + PACKET <= self.carry.len() {
            let next = self.carry.get(at + PACKET).copied();
            if self.carry[at] != SYNC || next.is_some_and(|next| next != SYNC) {
                // Lost sync: find the next byte that starts two packets.
                at += 1;
                self.in_sync = false;
                continue;
            }
            if next.is_none() && !self.in_sync && !last {
                break;
            }
            let packet: [u8; PACKET] = match self.carry[at..at + PACKET].try_into() {
                Ok(packet) => packet,
                Err(_) => break,
            };
            self.packet(&packet)?;
            self.in_sync = true;
            at += PACKET;
        }
        self.carry.drain(..at);
        Ok(())
    }

    /// The end of an HLS segment: segments end on PES boundaries, so the
    /// last frame is complete and need not wait for the next header.
    pub(crate) fn flush(&mut self) -> Result<(), Error> {
        if let Some(framer) = &mut self.framer {
            while let Some(packet) = framer.next_packet(true)? {
                self.ready.push_back(packet);
            }
        }
        Ok(())
    }

    /// No more bytes: the last frame no longer waits for the next header.
    pub(crate) fn finish(&mut self) -> Result<(), Error> {
        self.ended = true;
        self.parse_carry(true)
    }

    /// Restarts after a gap in the input (an HLS discontinuity, a seek):
    /// partial packets, sections and frames are dropped; the PMT stays known.
    pub(crate) fn discontinuity(&mut self) {
        self.carry.clear();
        self.ready.clear();
        self.in_sync = false;
        self.continuity.clear();
        self.ended = false;
        if let Some(audio) = &mut self.audio {
            audio.header.clear();
            audio.in_payload = false;
            audio.pts = None;
        }
        if let Some(framer) = &mut self.framer {
            framer.discontinuity();
        }
    }

    /// The next audio packet, if a whole frame is buffered.
    pub(crate) fn next_packet(&mut self) -> Result<Option<DemuxedPacket>, Error> {
        if let Some(packet) = self.ready.pop_front() {
            return Ok(Some(packet));
        }
        match &mut self.framer {
            Some(framer) => framer.next_packet(self.ended),
            None if self.ended => match self.unplayable.take() {
                Some(error) => Err(error),
                None => Err(Error::NoAudioTrack),
            },
            None => Ok(None),
        }
    }

    fn packet(&mut self, p: &[u8; PACKET]) -> Result<(), Error> {
        if p[1] & 0x80 != 0 {
            // Transport error indicator: the packet is damaged.
            return Ok(());
        }
        let unit_start = p[1] & 0x40 != 0;
        let pid = (u16::from(p[1] & 0x1f) << 8) | u16::from(p[2]);
        let scrambled = p[3] >> 6 != 0;
        let adaptation = p[3] & 0x20 != 0;
        let has_payload = p[3] & 0x10 != 0;
        let counter = p[3] & 0x0f;
        let mut offset = 4;
        let mut discontinuity = false;
        if adaptation {
            let length = usize::from(p[4]);
            discontinuity = length > 0 && p[5] & 0x80 != 0;
            offset += 1 + length;
        }
        if !has_payload || offset >= PACKET || pid == 0x1fff {
            return Ok(());
        }
        let payload = &p[offset..];

        // Continuity: a repeated counter is a duplicate packet; a gap means
        // packets were lost.
        let lost = match self.continuity.insert(pid, counter) {
            Some(last) if !discontinuity && counter == last => return Ok(()),
            Some(last) if !discontinuity => counter != (last + 1) & 0x0f,
            _ => false,
        };

        if pid == 0 {
            for section in self.pat.push(payload, unit_start) {
                self.on_pat(&section);
            }
        } else if Some(pid) == self.pmt_pid {
            for section in self.pmt.push(payload, unit_start) {
                self.on_pmt(&section)?;
            }
        } else if self.audio.as_ref().is_some_and(|a| a.pid == pid) {
            if scrambled {
                return Err(Error::UnsupportedEncryption {
                    scheme: "scrambled MPEG-TS".into(),
                });
            }
            if lost {
                tracing::debug!(pid, "lost TS packets; dropping the damaged frame");
                if let Some(framer) = &mut self.framer {
                    // Frames completed before the gap are intact.
                    while let Some(packet) = framer.next_packet(false)? {
                        self.ready.push_back(packet);
                    }
                    framer.discontinuity();
                }
                if let Some(audio) = &mut self.audio {
                    audio.in_payload = false;
                    audio.header.clear();
                }
            }
            self.on_audio(payload, unit_start)?;
        }
        Ok(())
    }

    fn on_pat(&mut self, section: &[u8]) {
        // current_next_indicator: a table announced for later is not applied.
        if section.first() != Some(&0x00)
            || section.len() < 12
            || section[5] & 0x01 == 0
            || !crc_ok(section)
        {
            return;
        }
        let programs = &section[8..section.len() - 4];
        self.pmt_pid = programs.as_chunks::<4>().0.iter().find_map(|entry| {
            let program = (u16::from(entry[0]) << 8) | u16::from(entry[1]);
            (program != 0).then_some((u16::from(entry[2] & 0x1f) << 8) | u16::from(entry[3]))
        });
    }

    fn on_pmt(&mut self, section: &[u8]) -> Result<(), Error> {
        if section.first() != Some(&0x02)
            || section.len() < 16
            || section[5] & 0x01 == 0
            || !crc_ok(section)
        {
            return Ok(());
        }
        let info = (usize::from(section[10] & 0x0f) << 8) | usize::from(section[11]);
        let end = section.len() - 4;
        let mut at = 12 + info;
        let mut chosen = None;
        let mut unplayable = None;
        while at + 5 <= end {
            let code = section[at];
            let pid = (u16::from(section[at + 1] & 0x1f) << 8) | u16::from(section[at + 2]);
            let es_info = (usize::from(section[at + 3] & 0x0f) << 8) | usize::from(section[at + 4]);
            match stream_type(code) {
                StreamType::Audio(kind) if chosen.is_none() => chosen = Some((pid, kind)),
                StreamType::EncryptedAudio => {
                    unplayable = Some(Error::UnsupportedEncryption {
                        scheme: "SAMPLE-AES".into(),
                    });
                }
                StreamType::UnsupportedAudio(name) if unplayable.is_none() => {
                    unplayable = Some(Error::UnsupportedCodec(name.into()));
                }
                _ => {}
            }
            at += 5 + es_info;
        }
        // The current PMT is authoritative, also over an earlier selection.
        let Some((pid, kind)) = chosen else {
            let had_audio = self.audio.take().is_some();
            if let Some(error) = unplayable {
                return Err(error);
            }
            if had_audio {
                return Err(Error::NoAudioTrack);
            }
            self.unplayable = Some(Error::NoAudioTrack);
            return Ok(());
        };
        if self.audio.as_ref().is_some_and(|audio| audio.pid == pid) {
            return Ok(());
        }
        // First PMT, or a new version that moved the audio.
        match &mut self.framer {
            Some(framer) if framer.kind() == kind => framer.discontinuity(),
            // The decoder was opened for the first codec and is not reopened.
            Some(framer) if framer.codec().is_some() => {
                return Err(Error::UnsupportedCodec(format!(
                    "a change from {:?} to {kind:?} audio mid-stream",
                    framer.kind()
                )));
            }
            _ => self.framer = Some(Framer::new(kind)),
        }
        self.audio = Some(Audio {
            pid,
            header: Vec::new(),
            in_payload: false,
            pts: None,
        });
        Ok(())
    }

    fn on_audio(&mut self, payload: &[u8], unit_start: bool) -> Result<(), Error> {
        let (Some(audio), Some(framer)) = (&mut self.audio, &mut self.framer) else {
            return Ok(());
        };
        let mut payload = payload;
        if unit_start {
            audio.header.clear();
            audio.in_payload = false;
            audio.pts = None;
        } else if !audio.in_payload && audio.header.is_empty() {
            // The middle of a PES whose start was lost.
            return Ok(());
        }
        if !audio.in_payload {
            audio.header.extend_from_slice(payload);
            if audio.header.len() < 9 {
                return Ok(());
            }
            if audio.header[..3] != [0, 0, 1] {
                tracing::debug!("PES without a start code; skipping it");
                audio.header.clear();
                return Ok(());
            }
            let header_len = 9 + usize::from(audio.header[8]);
            if audio.header.len() < header_len {
                return Ok(());
            }
            if audio.header[7] & 0x80 != 0 && header_len >= 14 {
                audio.pts = Some(parse_pts(&audio.header[9..14]));
            }
            let rest = audio.header.split_off(header_len);
            audio.header.clear();
            audio.in_payload = true;
            framer.push(&rest, audio.pts.take());
            payload = &[];
        }
        if !payload.is_empty() {
            framer.push(payload, audio.pts.take());
        }
        Ok(())
    }
}

/// A transport stream read from a seekable byte source, such as a `.ts`
/// file. HLS feeds [`TsDemuxer`] segment by segment instead.
pub(crate) struct TsFileDemuxer<R> {
    reader: R,
    ts: TsDemuxer,
    codec: CodecDescription,
    lookahead: Option<DemuxedPacket>,
    eof: bool,
    chunk: Vec<u8>,
}

impl<R: Read + Seek + Send> TsFileDemuxer<R> {
    /// Reads until the first audio frame, which tells the codec.
    pub(crate) fn open(reader: R) -> Result<Self, Error> {
        let mut demuxer = Self {
            reader,
            ts: TsDemuxer::new(),
            codec: CodecDescription::new(crate::Codec::Other),
            lookahead: None,
            eof: false,
            chunk: vec![0; 64 * 1024],
        };
        let first = demuxer.read_packet()?.ok_or(Error::NoAudioTrack)?;
        demuxer.codec = demuxer.ts.codec().cloned().ok_or(Error::NoAudioTrack)?;
        demuxer.lookahead = Some(first);
        Ok(demuxer)
    }

    fn read_packet(&mut self) -> Result<Option<DemuxedPacket>, Error> {
        loop {
            if let Some(packet) = self.ts.next_packet()? {
                return Ok(Some(packet));
            }
            if self.eof {
                return Ok(None);
            }
            let read = loop {
                match self.reader.read(&mut self.chunk) {
                    Ok(read) => break read,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) => return Err(error.into()),
                }
            };
            if read == 0 {
                self.eof = true;
                self.ts.finish()?;
            } else {
                self.ts.push(&self.chunk[..read])?;
            }
        }
    }
}

impl<R: Read + Seek + Send> Demuxer for TsFileDemuxer<R> {
    fn codec(&self) -> &CodecDescription {
        &self.codec
    }

    fn time_base(&self) -> TimeBase {
        TimeBase {
            numer: 1,
            denom: self.codec.sample_rate.unwrap_or(90_000),
        }
    }

    fn window(&self) -> Option<Window> {
        None
    }

    fn next_packet(&mut self) -> Result<Demuxed, Error> {
        if let Some(packet) = self.lookahead.take() {
            return Ok(Demuxed::Packet(packet));
        }
        Ok(match self.read_packet()? {
            Some(packet) => Demuxed::Packet(packet),
            None => Demuxed::End,
        })
    }

    /// TS has no index: start over, and the stream decodes and drops frames
    /// up to the target. Local `.ts` files are rare; HLS seeks by segment.
    fn seek(&mut self, _ts: i64) -> Result<Seeked, Error> {
        self.reader.seek(SeekFrom::Start(0))?;
        self.ts.discontinuity();
        self.lookahead = None;
        self.eof = false;
        Ok(Seeked::At)
    }
}

fn parse_pts(b: &[u8]) -> i64 {
    (i64::from((b[0] >> 1) & 0x07) << 30)
        | (i64::from(b[1]) << 22)
        | (i64::from(b[2] >> 1) << 15)
        | (i64::from(b[3]) << 7)
        | i64::from(b[4] >> 1)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::demux::es::tests::adts_frame;

    fn crc(section: &[u8]) -> [u8; 4] {
        let mut crc = 0xffff_ffffu32;
        for &byte in section {
            crc ^= u32::from(byte) << 24;
            for _ in 0..8 {
                crc = if crc & 0x8000_0000 != 0 {
                    (crc << 1) ^ 0x04c1_1db7
                } else {
                    crc << 1
                };
            }
        }
        crc.to_be_bytes()
    }

    fn with_crc(mut section: Vec<u8>) -> Vec<u8> {
        let length = section.len() + 4 - 3;
        section[1] = 0xb0 | ((length >> 8) as u8 & 0x0f);
        section[2] = length as u8;
        let crc = crc(&section);
        section.extend_from_slice(&crc);
        section
    }

    pub(crate) fn pat(pmt_pid: u16) -> Vec<u8> {
        with_crc(vec![
            0x00,
            0,
            0,
            0,
            1,
            0xc1,
            0,
            0,
            0,
            1,
            0xe0 | (pmt_pid >> 8) as u8,
            pmt_pid as u8,
        ])
    }

    /// A PMT listing `(stream_type, pid)` pairs.
    pub(crate) fn pmt(streams: &[(u8, u16)]) -> Vec<u8> {
        let mut section = vec![0x02, 0, 0, 0, 1, 0xc1, 0, 0, 0xe1, 0x00, 0xf0, 0x00];
        for &(code, pid) in streams {
            section.extend_from_slice(&[code, 0xe0 | (pid >> 8) as u8, pid as u8, 0xf0, 0x00]);
        }
        with_crc(section)
    }

    /// Splits `payload` into TS packets on `pid`, padding the last with an
    /// adaptation field. PSI payloads get a pointer field.
    pub(crate) fn packets(pid: u16, payload: &[u8], psi: bool, counter: &mut u8) -> Vec<u8> {
        let mut data = Vec::new();
        if psi {
            data.push(0);
        }
        data.extend_from_slice(payload);
        let mut out = Vec::new();
        for (index, chunk) in data.chunks(184).enumerate() {
            let mut packet = vec![
                SYNC,
                if index == 0 { 0x40 } else { 0 } | (pid >> 8) as u8,
                pid as u8,
                0x10 | (*counter & 0x0f),
            ];
            *counter = (*counter + 1) & 0x0f;
            if chunk.len() < 184 {
                packet[3] |= 0x20;
                let stuffing = 184 - chunk.len() - 1;
                packet.push(stuffing as u8);
                if stuffing > 0 {
                    packet.push(0);
                    packet.extend(std::iter::repeat_n(0xff, stuffing - 1));
                }
            }
            packet.extend_from_slice(chunk);
            assert_eq!(packet.len(), PACKET);
            out.extend(packet);
        }
        out
    }

    pub(crate) fn pes(pts: i64, es: &[u8]) -> Vec<u8> {
        let mut pes = vec![0, 0, 1, 0xc0, 0, 0, 0x80, 0x80, 5];
        pes.extend_from_slice(&[
            0x21 | (((pts >> 30) & 0x07) << 1) as u8,
            (pts >> 22) as u8,
            0x01 | (((pts >> 15) & 0x7f) << 1) as u8,
            (pts >> 7) as u8,
            0x01 | ((pts & 0x7f) << 1) as u8,
        ]);
        pes.extend_from_slice(es);
        pes
    }

    fn stream(frames_per_pes: usize, pes_count: usize) -> Vec<u8> {
        let (mut c0, mut c1, mut c2) = (0, 0, 0);
        let mut ts = packets(0, &pat(0x100), true, &mut c0);
        ts.extend(packets(
            0x100,
            &pmt(&[(0x1b, 0x200), (0x15, 0x300), (0x0f, 0x101)]),
            true,
            &mut c1,
        ));
        for i in 0..pes_count {
            let es: Vec<u8> = (0..frames_per_pes)
                .flat_map(|f| adts_frame(&[(i * 10 + f) as u8; 150]))
                .collect();
            let pts = 126_000 + (i * frames_per_pes) as i64 * 1024 * 90_000 / 44_100;
            ts.extend(packets(0x101, &pes(pts, &es), false, &mut c2));
        }
        ts
    }

    fn demux(ts: &[u8], chunk: usize) -> (TsDemuxer, Vec<DemuxedPacket>) {
        let mut demuxer = TsDemuxer::new();
        let mut packets = Vec::new();
        for piece in ts.chunks(chunk) {
            demuxer.push(piece).unwrap();
            while let Some(packet) = demuxer.next_packet().unwrap() {
                packets.push(packet);
            }
        }
        demuxer.finish().unwrap();
        while let Some(packet) = demuxer.next_packet().unwrap() {
            packets.push(packet);
        }
        (demuxer, packets)
    }

    #[test]
    fn audio_frames_come_out_of_pes_spanning_packets_in_any_chunking() {
        let ts = stream(3, 4);
        for chunk in [1, 100, 188, 1000, ts.len()] {
            let (demuxer, packets) = demux(&ts, chunk);
            assert_eq!(packets.len(), 12, "chunk {chunk}");
            for (n, packet) in packets.iter().enumerate() {
                assert_eq!(packet.data[0] as usize, (n / 3) * 10 + n % 3);
                // 1.4 s at 44.1 kHz, then 1024 per frame.
                assert_eq!(packet.pts, 61_740 + n as i64 * 1024, "frame {n}");
            }
            assert_eq!(demuxer.codec().unwrap().sample_rate, Some(44_100));
        }
    }

    #[test]
    fn a_lost_packet_drops_only_the_damaged_frame() {
        let mut ts = stream(3, 4);
        // Remove the second packet of the third PES (its frames 1 and 2 lie
        // in the lost and the following packets).
        let pes_start = ts
            .chunks(PACKET)
            .enumerate()
            .filter(|(_, p)| p[1] & 0x40 != 0 && p[2] == 0x01)
            .nth(2)
            .unwrap()
            .0;
        ts.drain((pes_start + 1) * PACKET..(pes_start + 2) * PACKET);
        let (_, packets) = demux(&ts, 4096);
        let firsts: Vec<u8> = packets.iter().map(|p| p.data[0]).collect();
        assert!(firsts.starts_with(&[0, 1, 2, 10, 11, 12, 20]), "{firsts:?}");
        assert!(firsts.ends_with(&[30, 31, 32]), "{firsts:?}");
        assert!(packets.len() < 12);
    }

    #[test]
    fn garbage_between_packets_is_resynchronized() {
        let ts = stream(2, 3);
        let mut damaged = vec![0x47, 1, 2, 3];
        damaged.extend_from_slice(&ts[..PACKET * 3]);
        damaged.extend_from_slice(&[0x47, 9, 9]);
        damaged.extend_from_slice(&ts[PACKET * 3..]);
        let (_, clean) = demux(&ts, 4096);
        let (_, packets) = demux(&damaged, 4096);
        assert_eq!(packets.len(), clean.len());
    }

    #[test]
    fn sample_aes_and_unsupported_audio_are_named() {
        let (mut c0, mut c1) = (0, 0);
        let mut ts = packets(0, &pat(0x100), true, &mut c0);
        ts.extend(packets(
            0x100,
            &pmt(&[(0x1b, 0x200), (0xcf, 0x101)]),
            true,
            &mut c1,
        ));
        let mut demuxer = TsDemuxer::new();
        assert!(matches!(
            demuxer.push(&ts),
            Err(Error::UnsupportedEncryption { .. })
        ));

        let (mut c0, mut c1) = (0, 0);
        let mut ts = packets(0, &pat(0x100), true, &mut c0);
        ts.extend(packets(0x100, &pmt(&[(0x81, 0x101)]), true, &mut c1));
        let mut demuxer = TsDemuxer::new();
        assert!(matches!(demuxer.push(&ts), Err(Error::UnsupportedCodec(_))));
    }

    #[test]
    fn a_stream_without_audio_ends_with_no_audio_track() {
        let (mut c0, mut c1) = (0, 0);
        let mut ts = packets(0, &pat(0x100), true, &mut c0);
        ts.extend(packets(0x100, &pmt(&[(0x1b, 0x200)]), true, &mut c1));
        let mut demuxer = TsDemuxer::new();
        demuxer.push(&ts).unwrap();
        demuxer.finish().unwrap();
        assert!(matches!(demuxer.next_packet(), Err(Error::NoAudioTrack)));
    }

    #[test]
    fn a_false_sync_at_the_end_of_a_chunk_waits_for_the_next_packet() {
        // Leading garbage with a 0x47, cut so the chunk ends one packet
        // after it: the candidate must not be taken without its successor.
        let ts = stream(2, 3);
        let mut damaged = vec![0x47, 1, 2, 3];
        damaged.extend_from_slice(&ts);
        let (_, clean) = demux(&ts, 4096);
        let mut demuxer = TsDemuxer::new();
        let mut packets = Vec::new();
        for piece in [&damaged[..PACKET], &damaged[PACKET..]] {
            demuxer.push(piece).unwrap();
            while let Some(packet) = demuxer.next_packet().unwrap() {
                packets.push(packet);
            }
        }
        demuxer.finish().unwrap();
        while let Some(packet) = demuxer.next_packet().unwrap() {
            packets.push(packet);
        }
        assert_eq!(packets.len(), clean.len());
    }

    #[test]
    fn tables_announced_for_later_are_not_applied() {
        let (mut c0, mut c1) = (0, 0);
        let mut ts = packets(0, &pat(0x100), true, &mut c0);
        let mut next = pmt(&[(0x0f, 0x101)]);
        next[5] &= !0x01; // current_next_indicator = 0
        let length = next.len();
        let fixed = with_crc(next[..length - 4].to_vec());
        ts.extend(packets(0x100, &fixed, true, &mut c1));
        let mut demuxer = TsDemuxer::new();
        demuxer.push(&ts).unwrap();
        assert!(demuxer.audio.is_none());
    }

    /// A stream, then a new PMT version (written by `pmt_with`).
    fn with_new_pmt(streams: &[(u8, u16)]) -> Vec<u8> {
        let mut ts = stream(2, 2);
        let mut next = pmt(streams);
        next[5] = 0xc3; // version 1, current
        let length = next.len();
        let mut counter = 5;
        ts.extend(packets(
            0x100,
            &with_crc(next[..length - 4].to_vec()),
            true,
            &mut counter,
        ));
        ts
    }

    #[test]
    fn a_pmt_that_changes_the_audio_codec_mid_stream_is_refused() {
        let ts = with_new_pmt(&[(0x03, 0x102)]);
        let mut demuxer = TsDemuxer::new();
        let mut result = Ok(());
        for piece in ts.chunks(188) {
            result = demuxer.push(piece);
            while let Ok(Some(_)) = demuxer.next_packet() {}
            if result.is_err() {
                break;
            }
        }
        assert!(
            matches!(result, Err(Error::UnsupportedCodec(_))),
            "{result:?}"
        );
    }

    #[test]
    fn a_pmt_that_removes_the_audio_ends_the_stream() {
        let ts = with_new_pmt(&[(0x1b, 0x200)]);
        let mut demuxer = TsDemuxer::new();
        let mut result = Ok(());
        for piece in ts.chunks(188) {
            result = demuxer.push(piece);
            while let Ok(Some(_)) = demuxer.next_packet() {}
            if result.is_err() {
                break;
            }
        }
        assert!(matches!(result, Err(Error::NoAudioTrack)), "{result:?}");
    }

    #[test]
    fn sections_with_a_bad_crc_are_ignored() {
        let mut bad = pat(0x100);
        let last = bad.len() - 1;
        bad[last] ^= 1;
        assert!(!crc_ok(&bad));
        assert!(crc_ok(&pat(0x100)));
    }
}
