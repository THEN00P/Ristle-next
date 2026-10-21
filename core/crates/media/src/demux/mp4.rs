//! MP4 gapless information: the edit list (`elst`) and iTunes' `iTunSMPB`.
//!
//! Symphonia parses `elst` but never applies it, so without this every AAC
//! track starts 1024 frames late and runs into the next one (spike B). The
//! edit list is read from the track that is played: in a muxed file a video
//! track's edit list would trim or truncate the audio (ADR 0004, "host
//! requirements from the spike B review").

use std::io::{Read, Seek, SeekFrom};

use crate::Error;

/// Where a track's presentation starts and how long it is, in the track's
/// media timescale (the time base Symphonia reports for it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Gapless {
    /// Media time of the first presented frame: the encoder delay.
    pub(crate) priming: u64,
    /// How many frames are presented after the priming, if known.
    pub(crate) length: Option<u64>,
}

/// The largest `moov` the host reads. Real audio files have a few hundred
/// KB at most; long files with huge sample tables stay well under this.
const MAX_MOOV: u64 = 64 << 20;

/// Whether `head` starts like an ISO base media file.
pub(crate) fn looks_like_mp4(head: &[u8]) -> bool {
    head.get(4..8).is_some_and(|kind| {
        matches!(
            kind,
            b"ftyp" | b"moov" | b"styp" | b"free" | b"skip" | b"wide" | b"mdat"
        )
    })
}

/// Finds the top-level `moov` box and returns its body. Leaves the reader at
/// an unspecified position.
pub(crate) fn read_moov<R: Read + Seek>(reader: &mut R) -> Result<Option<Vec<u8>>, Error> {
    let mut at = reader.seek(SeekFrom::Start(0))?;
    loop {
        let mut header = [0u8; 8];
        if !read_exact_or_eof(reader, &mut header)? {
            return Ok(None);
        }
        let size32 = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        let kind = [header[4], header[5], header[6], header[7]];
        let (header_len, size) = match size32 {
            0 => {
                // The box runs to the end of the file.
                if &kind != b"moov" {
                    return Ok(None);
                }
                let end = reader.seek(SeekFrom::End(0))?;
                reader.seek(SeekFrom::Start(at + 8))?;
                (8, end - at)
            }
            1 => {
                let mut large = [0u8; 8];
                if !read_exact_or_eof(reader, &mut large)? {
                    return Ok(None);
                }
                (16, u64::from_be_bytes(large))
            }
            n => (8, u64::from(n)),
        };
        if size < header_len {
            return Err(Error::malformed(
                "mp4 box",
                format!("{size}-byte box at {at}"),
            ));
        }
        if &kind == b"moov" {
            let body = size - header_len;
            if body > MAX_MOOV {
                return Err(Error::malformed("mp4 moov", format!("{body} bytes")));
            }
            let mut moov = vec![0u8; usize::try_from(body).unwrap_or(usize::MAX)];
            reader.read_exact(&mut moov)?;
            return Ok(Some(moov));
        }
        at = at
            .checked_add(size)
            .ok_or_else(|| Error::malformed("mp4 box", "size overflows"))?;
        reader.seek(SeekFrom::Start(at))?;
    }
}

fn read_exact_or_eof<R: Read>(reader: &mut R, buf: &mut [u8]) -> Result<bool, Error> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) if filled == 0 => return Ok(false),
            Ok(0) => return Ok(false),
            Ok(n) => filled += n,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(true)
}

/// Iterates the boxes in `data`, yielding `(kind, body)`.
pub(crate) struct Boxes<'a> {
    data: &'a [u8],
    at: usize,
}

pub(crate) fn boxes(data: &[u8]) -> Boxes<'_> {
    Boxes { data, at: 0 }
}

impl<'a> Iterator for Boxes<'a> {
    type Item = Result<([u8; 4], &'a [u8]), Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let rest = self.data.get(self.at..)?;
        if rest.len() < 8 {
            return None;
        }
        let size32 = u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
        let kind = [rest[4], rest[5], rest[6], rest[7]];
        let (header, size) = match size32 {
            0 => (8, rest.len()),
            1 => match rest.get(8..16) {
                Some(large) => {
                    let mut bytes = [0u8; 8];
                    bytes.copy_from_slice(large);
                    (
                        16,
                        usize::try_from(u64::from_be_bytes(bytes)).unwrap_or(usize::MAX),
                    )
                }
                None => return Some(Err(self.fail("truncated 64-bit box size"))),
            },
            n => (8, n),
        };
        if size < header || size > rest.len() {
            return Some(Err(self.fail(&format!(
                "box {} of {size} bytes in {} remaining",
                String::from_utf8_lossy(&kind),
                rest.len()
            ))));
        }
        self.at += size;
        Some(Ok((kind, &rest[header..size])))
    }
}

impl Boxes<'_> {
    fn fail(&mut self, detail: &str) -> Error {
        self.at = self.data.len();
        Error::malformed("mp4 box", detail)
    }
}

/// The body of the first child box of `kind`.
pub(crate) fn child<'a>(data: &'a [u8], kind: &[u8; 4]) -> Result<Option<&'a [u8]>, Error> {
    for item in boxes(data) {
        let (k, body) = item?;
        if &k == kind {
            return Ok(Some(body));
        }
    }
    Ok(None)
}

pub(crate) fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

pub(crate) fn be64(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// `track_ID` from a `tkhd` body.
pub(crate) fn tkhd_track_id(tkhd: &[u8]) -> Option<u32> {
    match tkhd.first()? {
        1 => be32(tkhd, 20),
        _ => be32(tkhd, 12),
    }
}

/// The timescale of an `mvhd` or `mdhd` body (the same layout up to it).
pub(crate) fn timescale(full_box: &[u8]) -> Option<u32> {
    let scale = match full_box.first()? {
        1 => be32(full_box, 20),
        _ => be32(full_box, 12),
    }?;
    (scale != 0).then_some(scale)
}

/// The `trak` whose `tkhd` names `track_id`.
pub(crate) fn find_trak(moov: &[u8], track_id: u32) -> Result<Option<&[u8]>, Error> {
    for item in boxes(moov) {
        let (kind, trak) = item?;
        if &kind != b"trak" {
            continue;
        }
        let Some(tkhd) = child(trak, b"tkhd")? else {
            continue;
        };
        if tkhd_track_id(tkhd) == Some(track_id) {
            return Ok(Some(trak));
        }
    }
    Ok(None)
}

/// Gapless information for the track with `track_id`.
///
/// Precedence: an edit list with a non-zero media time; otherwise
/// `iTunSMPB` (iTunes writes edit lists that start at 0 and puts the real
/// priming there); otherwise whatever the edit list says.
pub(crate) fn gapless_for_track(moov: &[u8], track_id: u32) -> Result<Option<Gapless>, Error> {
    let Some(trak) = find_trak(moov, track_id)? else {
        return Ok(None);
    };
    let edit = edit_list(moov, trak)?;
    if let Some(edit) = edit.filter(|e| e.priming > 0) {
        return Ok(Some(edit));
    }
    if let Some(itunes) = itunsmpb(moov)? {
        return Ok(Some(itunes));
    }
    Ok(edit)
}

/// The track's edit list as a presentation window in media time.
fn edit_list(moov: &[u8], trak: &[u8]) -> Result<Option<Gapless>, Error> {
    let Some(elst) = child(trak, b"edts")?
        .map(|edts| child(edts, b"elst"))
        .transpose()?
        .flatten()
    else {
        return Ok(None);
    };
    let bad = |detail: &str| Error::malformed("mp4 edit list", detail);
    let movie_scale = child(moov, b"mvhd")?
        .and_then(timescale)
        .ok_or_else(|| bad("no movie timescale"))?;
    let media_scale = child(trak, b"mdia")?
        .map(|mdia| child(mdia, b"mdhd"))
        .transpose()?
        .flatten()
        .and_then(timescale)
        .ok_or_else(|| bad("no media timescale"))?;
    let version = *elst.first().ok_or_else(|| bad("empty"))?;
    let count = be32(elst, 4).ok_or_else(|| bad("no entry count"))?;
    let entry_len = if version == 1 { 20 } else { 12 };
    let mut chosen: Option<Gapless> = None;
    let mut extra_edits = 0usize;
    for index in 0..count as usize {
        let at = 8 + index * entry_len;
        let (segment_duration, media_time, rate) = if version == 1 {
            (
                be64(elst, at),
                be64(elst, at + 8).map(|t| t as i64),
                // media_rate: signed 16.16 fixed point.
                be32(elst, at + 16),
            )
        } else {
            (
                be32(elst, at).map(u64::from),
                be32(elst, at + 4).map(|t| i64::from(t as i32)),
                be32(elst, at + 8),
            )
        };
        let (Some(segment_duration), Some(media_time), Some(rate)) =
            (segment_duration, media_time, rate)
        else {
            return Err(bad("truncated entry"));
        };
        if media_time < 0 {
            // An empty edit delays the presentation. Audio players do not
            // insert silence for it, and neither does the host.
            if chosen.is_none() && segment_duration > 0 {
                tracing::debug!(segment_duration, "ignoring an empty edit before the audio");
            }
            continue;
        }
        if chosen.is_some() {
            extra_edits += 1;
            continue;
        }
        if rate != 0x0001_0000 {
            tracing::warn!(
                rate,
                "ignoring an edit list with a media rate other than 1.0"
            );
            return Ok(None);
        }
        // A zero duration means "to the end of the media" (fragmented files).
        let length = (segment_duration > 0).then(|| {
            let scaled =
                u128::from(segment_duration) * u128::from(media_scale) / u128::from(movie_scale);
            u64::try_from(scaled).unwrap_or(u64::MAX)
        });
        chosen = Some(Gapless {
            priming: media_time as u64,
            length,
        });
    }
    if extra_edits > 0 {
        tracing::warn!(
            extra_edits,
            "applying only the first edit of a multi-edit list"
        );
    }
    Ok(chosen)
}

/// iTunes' gapless tag: `moov/udta/meta/ilst/----` named `iTunSMPB`, whose
/// text is " 00000000 PPPPPPPP EEEEEEEE LLLLLLLLLLLLLLLL ..." in hex
/// (priming, end padding, valid length).
fn itunsmpb(moov: &[u8]) -> Result<Option<Gapless>, Error> {
    let Some(udta) = child(moov, b"udta")? else {
        return Ok(None);
    };
    let Some(meta) = child(udta, b"meta")? else {
        return Ok(None);
    };
    // `meta` is a full box: version and flags come before its children.
    let Some(ilst) = meta
        .get(4..)
        .map(|m| child(m, b"ilst"))
        .transpose()?
        .flatten()
    else {
        return Ok(None);
    };
    for item in boxes(ilst) {
        let (kind, freeform) = item?;
        if &kind != b"----" {
            continue;
        }
        let mut name = None;
        let mut data = None;
        for part in boxes(freeform) {
            let (kind, body) = part?;
            match &kind {
                // `name` is a full box; `data` has a type and a locale.
                b"name" => name = body.get(4..),
                b"data" => data = body.get(8..),
                _ => {}
            }
        }
        if name != Some(b"iTunSMPB".as_slice()) {
            continue;
        }
        let Some(text) = data.and_then(|d| std::str::from_utf8(d).ok()) else {
            return Err(Error::malformed("iTunSMPB", "not text"));
        };
        let fields: Vec<u64> = text
            .split_whitespace()
            .map(|field| u64::from_str_radix(field, 16))
            .collect::<Result<_, _>>()
            .map_err(|_| Error::malformed("iTunSMPB", text.to_owned()))?;
        let (Some(&priming), Some(&length)) = (fields.get(1), fields.get(3)) else {
            return Err(Error::malformed("iTunSMPB", text.to_owned()));
        };
        return Ok(Some(Gapless {
            priming,
            length: (length > 0).then_some(length),
        }));
    }
    Ok(None)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Builds a box from its kind and body.
    pub(crate) fn mp4_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out
    }

    fn full(version: u8, rest: &[u8]) -> Vec<u8> {
        let mut out = vec![version, 0, 0, 0];
        out.extend_from_slice(rest);
        out
    }

    fn tkhd(id: u32) -> Vec<u8> {
        let mut rest = vec![0u8; 8];
        rest.extend_from_slice(&id.to_be_bytes());
        rest.extend_from_slice(&[0u8; 68]);
        mp4_box(b"tkhd", &full(0, &rest))
    }

    fn header_with_scale(kind: &[u8; 4], scale: u32) -> Vec<u8> {
        let mut rest = vec![0u8; 8];
        rest.extend_from_slice(&scale.to_be_bytes());
        rest.extend_from_slice(&[0u8; 8]);
        mp4_box(kind, &full(0, &rest))
    }

    fn elst(entries: &[(u32, i32)]) -> Vec<u8> {
        let mut rest = (entries.len() as u32).to_be_bytes().to_vec();
        for (duration, media_time) in entries {
            rest.extend_from_slice(&duration.to_be_bytes());
            rest.extend_from_slice(&media_time.to_be_bytes());
            rest.extend_from_slice(&[0, 1, 0, 0]);
        }
        mp4_box(b"edts", &mp4_box(b"elst", &full(0, &rest)))
    }

    fn trak(id: u32, media_scale: u32, edits: &[(u32, i32)]) -> Vec<u8> {
        let mut body = tkhd(id);
        if !edits.is_empty() {
            body.extend(elst(edits));
        }
        body.extend(mp4_box(b"mdia", &header_with_scale(b"mdhd", media_scale)));
        mp4_box(b"trak", &body)
    }

    fn itunes(text: &str) -> Vec<u8> {
        let mut name = vec![0u8; 4];
        name.extend_from_slice(b"iTunSMPB");
        let mut data = vec![0, 0, 0, 1, 0, 0, 0, 0];
        data.extend_from_slice(text.as_bytes());
        let mut freeform = mp4_box(b"mean", b"\0\0\0\0com.apple.iTunes");
        freeform.extend(mp4_box(b"name", &name));
        freeform.extend(mp4_box(b"data", &data));
        let ilst = mp4_box(b"ilst", &mp4_box(b"----", &freeform));
        mp4_box(b"udta", &mp4_box(b"meta", &full(0, &ilst)))
    }

    fn moov(parts: &[Vec<u8>]) -> Vec<u8> {
        let mut body = header_with_scale(b"mvhd", 1000);
        for part in parts {
            body.extend_from_slice(part);
        }
        body
    }

    #[test]
    fn the_edit_list_comes_from_the_requested_track() {
        // Video first, with its own edit list, as a muxer with B-frames writes.
        let moov = moov(&[
            trak(1, 1000, &[(1200, 100)]),
            trak(2, 44_100, &[(1190, 1024)]),
        ]);
        assert_eq!(
            gapless_for_track(&moov, 2).unwrap(),
            Some(Gapless {
                priming: 1024,
                length: Some(52_479)
            })
        );
        assert_eq!(
            gapless_for_track(&moov, 1).unwrap(),
            Some(Gapless {
                priming: 100,
                length: Some(1200)
            })
        );
        assert_eq!(gapless_for_track(&moov, 3).unwrap(), None);
    }

    #[test]
    fn empty_edits_are_skipped_and_zero_duration_means_to_the_end() {
        let moov = moov(&[trak(1, 48_000, &[(500, -1), (0, 312)])]);
        assert_eq!(
            gapless_for_track(&moov, 1).unwrap(),
            Some(Gapless {
                priming: 312,
                length: None
            })
        );
    }

    #[test]
    fn itunsmpb_wins_over_an_edit_list_that_starts_at_zero() {
        let tag = " 00000000 00000840 000001CA 00000000000B0BB6 00000000 00000000";
        let moov = moov(&[trak(1, 44_100, &[(16_000, 0)]), itunes(tag)]);
        assert_eq!(
            gapless_for_track(&moov, 1).unwrap(),
            Some(Gapless {
                priming: 2112,
                length: Some(0xb0bb6)
            })
        );
        // A real priming in the edit list is preferred.
        let moov = super::tests::moov(&[trak(1, 44_100, &[(16_000, 1024)]), itunes(tag)]);
        assert_eq!(
            gapless_for_track(&moov, 1).unwrap().map(|g| g.priming),
            Some(1024)
        );
    }

    #[test]
    fn an_edit_with_a_fractional_rate_is_not_applied() {
        let mut trak = trak(1, 44_100, &[(1190, 1024)]);
        // The fraction half of media_rate (1.5): the last two bytes of the entry.
        let at = trak.windows(4).position(|w| w == b"elst").unwrap() + 4 + 4 + 4 + 8 + 2;
        trak[at] = 0x80;
        assert_eq!(gapless_for_track(&moov(&[trak]), 1).unwrap(), None);
    }

    #[test]
    fn a_truncated_edit_list_is_malformed() {
        let mut trak_body = tkhd(1);
        // Two entries declared, one present.
        let mut rest = 2u32.to_be_bytes().to_vec();
        rest.extend_from_slice(&[0, 0, 0, 10, 0, 0, 4, 0, 0, 1, 0, 0]);
        trak_body.extend(mp4_box(b"edts", &mp4_box(b"elst", &full(0, &rest))));
        trak_body.extend(mp4_box(b"mdia", &header_with_scale(b"mdhd", 44_100)));
        let moov = moov(&[mp4_box(b"trak", &trak_body)]);
        assert!(matches!(
            gapless_for_track(&moov, 1),
            Err(Error::Malformed { .. })
        ));
    }

    #[test]
    fn finds_the_moov_after_the_media_data() {
        let mut file = mp4_box(b"ftyp", b"M4A \0\0\0\0");
        file.extend(mp4_box(b"mdat", &[0u8; 1000]));
        file.extend(mp4_box(b"moov", b"payload"));
        let moov = read_moov(&mut std::io::Cursor::new(&file)).unwrap();
        assert_eq!(moov.as_deref(), Some(b"payload".as_slice()));
        assert!(looks_like_mp4(&file));
        let none = read_moov(&mut std::io::Cursor::new(mp4_box(b"ftyp", b""))).unwrap();
        assert_eq!(none, None);
    }
}
