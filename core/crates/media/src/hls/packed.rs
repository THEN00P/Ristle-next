//! HLS packed audio (RFC 8216, 3.4): ADTS or MPEG audio segments, each
//! starting with an ID3 tag whose `PRIV` frame
//! `com.apple.streaming.transportStreamTimestamp` holds the 33-bit MPEG
//! timestamp of the first frame.

use crate::demux::es::EsKind;

const TIMESTAMP_OWNER: &[u8] = b"com.apple.streaming.transportStreamTimestamp\0";

/// What the start of a packed-audio segment turned out to be.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Head {
    /// More bytes are needed to tell.
    NeedMore,
    /// ID3 tags of `len` bytes, then audio; `timestamp` from the tag (90 kHz).
    Tagged { len: usize, timestamp: Option<i64> },
    /// No tag: audio starts at once.
    Untagged,
}

/// Reads the ID3 tags at the start of `data`. Several tags may follow each
/// other.
pub(crate) fn read_head(data: &[u8]) -> Head {
    let mut at = 0;
    let mut timestamp = None;
    loop {
        let rest = &data[at..];
        if rest.len() < 3 {
            return if at == 0 && !b"ID3".starts_with(rest) {
                Head::Untagged
            } else if at > 0 && !b"ID3".starts_with(rest) {
                Head::Tagged { len: at, timestamp }
            } else {
                Head::NeedMore
            };
        }
        if &rest[..3] != b"ID3" {
            return if at == 0 {
                Head::Untagged
            } else {
                Head::Tagged { len: at, timestamp }
            };
        }
        if rest.len() < 10 {
            return Head::NeedMore;
        }
        let size = syncsafe(&rest[6..10]);
        let footer = if rest[5] & 0x10 != 0 { 10 } else { 0 };
        let total = 10 + size + footer;
        if rest.len() < total {
            return Head::NeedMore;
        }
        if timestamp.is_none() {
            timestamp = find_timestamp(&rest[10..10 + size], rest[3]);
        }
        at += total;
    }
}

fn syncsafe(b: &[u8]) -> usize {
    b.iter()
        .fold(0, |acc, &byte| (acc << 7) | usize::from(byte & 0x7f))
}

/// The transport stream timestamp in an ID3v2.3/2.4 tag body.
fn find_timestamp(body: &[u8], version: u8) -> Option<i64> {
    let mut at = 0;
    while at + 10 <= body.len() {
        let id = &body[at..at + 4];
        if id == [0, 0, 0, 0] {
            return None; // padding
        }
        let size_bytes = &body[at + 4..at + 8];
        let size = if version >= 4 {
            syncsafe(size_bytes)
        } else {
            size_bytes
                .iter()
                .fold(0, |acc, &b| (acc << 8) | usize::from(b))
        };
        let frame = body.get(at + 10..at + 10 + size)?;
        if id == b"PRIV" && frame.starts_with(TIMESTAMP_OWNER) {
            let stamp = frame.get(TIMESTAMP_OWNER.len()..TIMESTAMP_OWNER.len() + 8)?;
            let value = stamp.iter().fold(0i64, |acc, &b| (acc << 8) | i64::from(b));
            return Some(value & ((1 << 33) - 1));
        }
        at += 10 + size;
    }
    None
}

/// Which elementary stream starts `data`, if it is one.
pub(crate) fn sniff_audio(data: &[u8]) -> Option<EsKind> {
    let (&first, rest) = data.split_first()?;
    let &second = rest.first()?;
    if first != 0xff {
        return None;
    }
    if second & 0xf6 == 0xf0 {
        Some(EsKind::Adts)
    } else if second & 0xe0 == 0xe0 && (second >> 1) & 0x03 != 0 && (second >> 3) & 0x03 != 1 {
        Some(EsKind::MpegAudio)
    } else {
        None
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// An ID3v2.4 tag with the transport stream timestamp, as Apple writes it.
    pub(crate) fn timestamp_tag(pts: i64) -> Vec<u8> {
        let mut frame = TIMESTAMP_OWNER.to_vec();
        frame.extend_from_slice(&pts.to_be_bytes());
        let mut body = b"PRIV".to_vec();
        body.extend_from_slice(&(frame.len() as u32).to_be_bytes()); // < 128: syncsafe too
        body.extend_from_slice(&[0, 0]);
        body.extend_from_slice(&frame);
        let mut tag = b"ID3\x04\x00\x00".to_vec();
        let size = body.len();
        tag.extend_from_slice(&[0, 0, (size >> 7) as u8 & 0x7f, size as u8 & 0x7f]);
        tag.extend_from_slice(&body);
        tag
    }

    #[test]
    fn reads_the_transport_stream_timestamp_and_the_tag_length() {
        let tag = timestamp_tag(900_000);
        let mut segment = tag.clone();
        segment.extend_from_slice(&[0xff, 0xf1, 0x50, 0x80]);
        assert_eq!(
            read_head(&segment),
            Head::Tagged {
                len: tag.len(),
                timestamp: Some(900_000)
            }
        );
        assert_eq!(read_head(&segment[..tag.len() - 1]), Head::NeedMore);
        assert_eq!(read_head(&segment[..2]), Head::NeedMore);
        assert_eq!(read_head(&[0xff, 0xf1, 0x50]), Head::Untagged);
        assert_eq!(sniff_audio(&segment[tag.len()..]), Some(EsKind::Adts));
        assert_eq!(sniff_audio(&[0xff, 0xfb, 0x90]), Some(EsKind::MpegAudio));
        assert_eq!(sniff_audio(&[0x47, 0x40]), None);
    }
}
