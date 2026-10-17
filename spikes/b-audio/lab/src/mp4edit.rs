//! Reads AAC gapless info from an MP4: the edit list (`moov/trak/edts/elst`)
//! and, if present, iTunes' `iTunSMPB` tag. Symphonia parses `elst` but does
//! not apply it, so the host has to.

#[derive(Debug, Clone, Copy)]
pub struct Edit {
    /// Priming frames to drop at the start (in the media timescale, which is
    /// the sample rate for audio written by every encoder seen here).
    pub media_time: u64,
    /// Frames to keep after the priming.
    pub duration_frames: u64,
    pub source: &'static str,
}

fn boxes(data: &[u8]) -> impl Iterator<Item = (&[u8; 4], &[u8])> {
    let mut pos = 0usize;
    std::iter::from_fn(move || {
        if pos + 8 > data.len() {
            return None;
        }
        let size32 = u32::from_be_bytes(data[pos..pos + 4].try_into().ok()?) as usize;
        let kind: &[u8; 4] = data[pos + 4..pos + 8].try_into().ok()?;
        let (header, size) = match size32 {
            0 => (8, data.len() - pos),
            1 => (16, u64::from_be_bytes(data.get(pos + 8..pos + 16)?.try_into().ok()?) as usize),
            n => (8, n),
        };
        if size < header || pos + size > data.len() {
            return None;
        }
        let body = &data[pos + header..pos + size];
        pos += size;
        Some((kind, body))
    })
}

fn child<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(data).find(|(k, _)| *k == kind).map(|(_, b)| b)
}

fn be32(b: &[u8], at: usize) -> Option<u64> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?) as u64)
}
fn be64(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// Timescale from `mvhd` or `mdhd` (both full boxes with the same layout up to it).
fn timescale(full_box: &[u8]) -> Option<u64> {
    match full_box.first()? {
        1 => be32(full_box, 20),
        _ => be32(full_box, 12),
    }
}

pub fn read_edit(file: &[u8]) -> Option<Edit> {
    let moov = child(file, b"moov")?;
    if let Some(e) = itunsmpb(moov) {
        return Some(e);
    }
    let movie_ts = timescale(child(moov, b"mvhd")?)?;
    for (kind, trak) in boxes(moov) {
        if kind != b"trak" {
            continue;
        }
        let mdia = child(trak, b"mdia")?;
        let media_ts = timescale(child(mdia, b"mdhd")?)?;
        let Some(elst) = child(trak, b"edts").and_then(|e| child(e, b"elst")) else { continue };
        let version = elst[0];
        let count = be32(elst, 4)?;
        let mut at = 8;
        for _ in 0..count {
            let (dur, media_time) = if version == 1 {
                let r = (be64(elst, at)?, be64(elst, at + 8)? as i64);
                at += 20;
                r
            } else {
                let r = (be32(elst, at)?, be32(elst, at + 4)? as u32 as i32 as i64);
                at += 12;
                r
            };
            if media_time < 0 {
                continue; // empty edit
            }
            return Some(Edit {
                media_time: media_time as u64,
                duration_frames: dur * media_ts / movie_ts,
                source: "elst",
            });
        }
    }
    None
}

/// `iTunSMPB`: " 00000000 PPPPPPPP EEEEEEEE LLLLLLLLLLLLLLLL ..." (priming,
/// end padding, valid length, hex).
fn itunsmpb(moov: &[u8]) -> Option<Edit> {
    let at = moov.windows(8).position(|w| w == b"iTunSMPB")?;
    let data = &moov[at..];
    let start = data.windows(4).position(|w| w == b"data")? + 12;
    let text = std::str::from_utf8(data.get(start..start + 120)?.split(|b| *b == 0).next()?).ok()?;
    let f: Vec<u64> = text.split_whitespace().filter_map(|h| u64::from_str_radix(h, 16).ok()).collect();
    Some(Edit { media_time: *f.get(1)?, duration_frames: *f.get(3)?, source: "iTunSMPB" })
}
