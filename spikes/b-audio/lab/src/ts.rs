//! Minimal MPEG-TS demuxer for HLS audio: PAT → PMT → the first audio PES
//! stream, emitted as its elementary stream (ADTS AAC or MPEG audio frames are
//! self-delimiting, so concatenated PES payloads are a valid ADTS/MP3 stream
//! for Symphonia). Video and ID3 timed-metadata streams are skipped.

use std::io;

const PACKET: usize = 188;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EsKind {
    Adts,
    MpegAudio,
}

#[derive(Default)]
pub struct TsDemuxer {
    pmt_pid: Option<u16>,
    audio_pid: Option<u16>,
    pub kind: Option<EsKind>,
    /// PTS (90 kHz) of the first audio PES seen.
    pub first_pts: Option<u64>,
    pes_header_pending: bool,
    pes_buf: Vec<u8>,
    carry: Vec<u8>,
}

impl TsDemuxer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds TS bytes (any alignment), appends elementary-stream bytes to `out`.
    pub fn push(&mut self, data: &[u8], out: &mut Vec<u8>) -> io::Result<()> {
        let mut buf = std::mem::take(&mut self.carry);
        buf.extend_from_slice(data);
        let mut pos = 0;
        while pos + PACKET <= buf.len() {
            if buf[pos] != 0x47 {
                // Resync on the next sync byte.
                pos += 1;
                continue;
            }
            self.packet(&buf[pos..pos + PACKET], out)?;
            pos += PACKET;
        }
        self.carry = buf[pos..].to_vec();
        Ok(())
    }

    /// Flushes a PES still being assembled (end of segment or stream).
    pub fn flush(&mut self, out: &mut Vec<u8>) {
        self.finish_pes(out);
    }

    fn packet(&mut self, p: &[u8], out: &mut Vec<u8>) -> io::Result<()> {
        let pusi = p[1] & 0x40 != 0;
        let pid = u16::from(p[1] & 0x1f) << 8 | u16::from(p[2]);
        let afc = (p[3] >> 4) & 3;
        let mut off = 4;
        if afc & 2 != 0 {
            off += 1 + p[4] as usize;
        }
        if afc & 1 == 0 || off >= PACKET {
            return Ok(());
        }
        let payload = &p[off..];
        if pid == 0 {
            self.pat(payload, pusi);
        } else if Some(pid) == self.pmt_pid {
            self.pmt(payload, pusi);
        } else if Some(pid) == self.audio_pid {
            if pusi {
                self.finish_pes(out);
                self.pes_header_pending = true;
            }
            self.pes_buf.extend_from_slice(payload);
            if self.pes_header_pending && self.pes_buf.len() >= 9 {
                let b = &self.pes_buf;
                if b[0..3] != [0, 0, 1] {
                    return Err(io::Error::other("ts: bad PES start code"));
                }
                let header_len = 9 + b[8] as usize;
                if b.len() >= header_len {
                    if b[7] & 0x80 != 0 && self.first_pts.is_none() && header_len >= 14 {
                        let pts = (u64::from(b[9] >> 1 & 7) << 30)
                            | (u64::from(b[10]) << 22)
                            | (u64::from(b[11] >> 1) << 15)
                            | (u64::from(b[12]) << 7)
                            | u64::from(b[13] >> 1);
                        self.first_pts = Some(pts);
                    }
                    self.pes_buf.drain(..header_len);
                    self.pes_header_pending = false;
                }
            }
            // Emit eagerly once the header is gone; PES length is often 0 for audio.
            if !self.pes_header_pending {
                out.append(&mut self.pes_buf);
            }
        }
        Ok(())
    }

    fn finish_pes(&mut self, out: &mut Vec<u8>) {
        if !self.pes_header_pending {
            out.append(&mut self.pes_buf);
        }
        self.pes_buf.clear();
        self.pes_header_pending = false;
    }

    fn section(payload: &[u8], pusi: bool) -> Option<&[u8]> {
        if !pusi {
            return None; // Sections spanning packets do not occur for small PAT/PMTs.
        }
        let pointer = *payload.first()? as usize;
        let s = payload.get(1 + pointer..)?;
        let len = (usize::from(s.get(1)? & 0x0f) << 8) | usize::from(*s.get(2)?);
        s.get(..3 + len)
    }

    fn pat(&mut self, payload: &[u8], pusi: bool) {
        let Some(s) = Self::section(payload, pusi) else { return };
        // Program loop between the 8-byte header and the 4-byte CRC.
        for e in s[8..s.len() - 4].chunks_exact(4) {
            let program = u16::from(e[0]) << 8 | u16::from(e[1]);
            if program != 0 {
                self.pmt_pid = Some(u16::from(e[2] & 0x1f) << 8 | u16::from(e[3]));
                return;
            }
        }
    }

    fn pmt(&mut self, payload: &[u8], pusi: bool) {
        let Some(s) = Self::section(payload, pusi) else { return };
        let info_len = (usize::from(s[10] & 0x0f) << 8) | usize::from(s[11]);
        let mut i = 12 + info_len;
        while i + 5 <= s.len() - 4 {
            let stream_type = s[i];
            let pid = u16::from(s[i + 1] & 0x1f) << 8 | u16::from(s[i + 2]);
            let es_len = (usize::from(s[i + 3] & 0x0f) << 8) | usize::from(s[i + 4]);
            let kind = match stream_type {
                0x0f => Some(EsKind::Adts),            // ISO 13818-7 ADTS AAC
                0x03 | 0x04 => Some(EsKind::MpegAudio), // MPEG-1/2 audio
                _ => None,                              // video, ID3 (0x15), SAMPLE-AES types
            };
            if let (Some(k), None) = (kind, self.audio_pid) {
                self.audio_pid = Some(pid);
                self.kind = Some(k);
            }
            i += 5 + es_len;
        }
    }
}
