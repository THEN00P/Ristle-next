//! Byte-range seek inside an fMP4 HLS segment (SoundCloud's container).
//!
//! Instead of downloading the segment from its start, read the `moof` (first
//! few KB) for the per-sample sizes, then request only the bytes from the
//! target AAC frame onwards and hand those frames straight to the decoder.
//! Two round trips plus a few KB, independent of segment length.

use crate::hls::{Fetcher, Playlist};
use crate::registry::DecoderRegistry;
use anyhow::{anyhow, bail, Result};
use std::io::{Cursor, Read};
use std::time::{Duration, Instant};
use symphonia::core::packet::Packet;
use symphonia::core::units::{Duration as SymDuration, Timestamp};

/// Bytes fetched to find the `moof`. 6 s of AAC is about 260 frames, so the
/// sample table is about 1–2 KB.
const MOOF_PROBE: u64 = 4096;

/// Every sample of the fragment, across all `trun` boxes (Apple's packager
/// writes several per `traf`).
struct Trun {
    /// Offset of each sample relative to the start of the segment.
    offsets: Vec<u64>,
    sizes: Vec<u32>,
    durations: Vec<u32>,
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Finds moof/traf/{tfhd,trun} in the head of a segment.
fn parse_moof(head: &[u8]) -> Result<Trun> {
    let mut pos = 0usize;
    while pos + 8 <= head.len() {
        let size = be32(head, pos).ok_or_else(|| anyhow!("short box"))? as usize;
        let kind = &head[pos + 4..pos + 8];
        if kind == b"moof" {
            let moof_start = pos;
            let moof = head.get(pos + 8..pos + size).ok_or_else(|| anyhow!("moof larger than the probe"))?;
            let mut i = 0;
            let (mut def_size, mut def_dur) = (0u32, 1024u32);
            let mut base_offset: Option<u64> = None;
            let mut all = Trun { offsets: Vec::new(), sizes: Vec::new(), durations: Vec::new() };
            // End of the previous run: a trun without data_offset follows it.
            let mut run_end: Option<u64> = None;
            while i + 8 <= moof.len() {
                let s = be32(moof, i).unwrap() as usize;
                if &moof[i + 4..i + 8] == b"traf" {
                    let traf = &moof[i + 8..i + s];
                    let mut j = 0;
                    while j + 8 <= traf.len() {
                        let bs = be32(traf, j).unwrap() as usize;
                        let k = &traf[j + 4..j + 8];
                        let b = &traf[j + 8..j + bs];
                        if k == b"tfhd" {
                            let flags = be32(b, 0).unwrap() & 0xff_ffff;
                            let mut o = 8; // version/flags + track_ID
                            if flags & 0x1 != 0 {
                                base_offset = Some(u64::from_be_bytes(b[o..o + 8].try_into()?));
                                o += 8;
                            }
                            if flags & 0x2 != 0 { o += 4; }
                            if flags & 0x8 != 0 { def_dur = be32(b, o).unwrap(); o += 4; }
                            if flags & 0x10 != 0 { def_size = be32(b, o).unwrap(); }
                        } else if k == b"trun" {
                            let flags = be32(b, 0).unwrap() & 0xff_ffff;
                            let count = be32(b, 4).unwrap() as usize;
                            let mut o = 8;
                            let mut data_offset: Option<i64> = None;
                            if flags & 0x1 != 0 { data_offset = Some(be32(b, o).unwrap() as i32 as i64); o += 4; }
                            if flags & 0x4 != 0 { o += 4; }
                            // Without an explicit base, offsets count from the moof (default-base-is-moof).
                            let base = base_offset.unwrap_or(moof_start as u64);
                            let mut at = match (data_offset, run_end) {
                                (Some(d), _) => (base as i64 + d) as u64,
                                (None, Some(end)) => end,
                                (None, None) => base,
                            };
                            for _ in 0..count {
                                let d = if flags & 0x100 != 0 { let v = be32(b, o).unwrap(); o += 4; v } else { def_dur };
                                let z = if flags & 0x200 != 0 { let v = be32(b, o).unwrap(); o += 4; v } else { def_size };
                                if flags & 0x400 != 0 { o += 4; }
                                if flags & 0x800 != 0 { o += 4; }
                                all.offsets.push(at);
                                all.sizes.push(z);
                                all.durations.push(d);
                                at += z as u64;
                            }
                            run_end = Some(at);
                        }
                        j += bs.max(8);
                    }
                }
                i += s.max(8);
            }
            if all.sizes.is_empty() {
                bail!("moof without trun");
            }
            return Ok(all);
        }
        pos += size.max(8);
    }
    bail!("no moof in the first {MOOF_PROBE} bytes")
}

/// (url, absolute offset, length) of a segment URL that may carry `#bytes=`.
fn split(url: &str) -> (String, u64, Option<u64>) {
    match url.split_once("#bytes=").and_then(|(b, r)| {
        let (o, l) = r.split_once('-')?;
        Some((b.to_string(), o.parse().ok()?, l.parse().ok()))
    }) {
        Some((b, o, l)) => (b, o, l),
        None => (url.to_string(), 0, None),
    }
}

pub struct RangedSeek {
    pub latency: Duration,
    /// Network time up to the first frame at the target.
    pub fetch: Duration,
    pub requests: u64,
    pub first_frame_s: f64,
    pub audio: Vec<f32>,
    pub rate: u32,
}

/// Seeks to `t` with two range requests. `init` is the cached init segment.
/// Unencrypted fMP4 only (AES-128 would need the preceding cipher block too).
pub fn seek_fmp4(reg: &DecoderRegistry, f: &Fetcher, pl: &Playlist, init: &[u8], t: f64) -> Result<RangedSeek> {
    let started = Instant::now();
    let req0 = f.requests.load(std::sync::atomic::Ordering::Relaxed);
    let ns0 = f.fetch_ns.load(std::sync::atomic::Ordering::Relaxed);
    let mut fetch = Duration::ZERO;
    // Codec parameters from the init segment alone.
    let format = crate::decode::open(Box::new(Cursor::new(init.to_vec())), Some("mp4"))?;
    let (_, stream) = crate::decode::audio_stream(format.as_ref(), pl.codecs.clone())?;
    let rate = stream.params.sample_rate.ok_or_else(|| anyhow!("no rate"))?;
    let (mut dec, _, _) = reg.create(&stream)?;

    let idx = pl.index_at(t);
    let seg = &pl.segments[idx];
    if seg.key.is_some() {
        bail!("encrypted segment: ranged seek not implemented");
    }
    let (base, off, len) = split(&seg.url);
    let probe = MOOF_PROBE.min(len.unwrap_or(u64::MAX));
    let head = f.bytes(&format!("{base}#bytes={off}-{probe}"))?;
    let trun = parse_moof(&head)?;

    // Target frame: decode one frame early so the decoder's overlap state is
    // primed (AAC needs the previous frame for the MDCT overlap).
    let mut acc = 0u64;
    let target = ((t - seg.start_s) * rate as f64) as u64;
    let mut k = 0usize;
    while k + 1 < trun.durations.len() && acc + trun.durations[k] as u64 <= target {
        acc += trun.durations[k] as u64;
        k += 1;
    }
    let prime = k.min(1);
    let k0 = k - prime;
    let acc0 = acc - trun.durations[k0..k].iter().map(|d| *d as u64).sum::<u64>();
    let byte0 = trun.offsets[k0];
    let last = trun.sizes.len() - 1;
    let remaining = trun.offsets[last] + trun.sizes[last] as u64 - byte0;
    let mut body = f.stream(&format!("{base}#bytes={}-{remaining}", off + byte0))?;
    let mut pos = byte0;

    let skip_in_frame = target - acc; // frames to drop inside frame k
    let mut out = Vec::new();
    let mut pcm = Vec::new();
    let mut first_at: Option<f64> = None;
    let mut latency = Duration::ZERO;
    let mut frame_pos = acc0;
    for (n, i) in (k0..trun.sizes.len()).enumerate() {
        // Skip any gap between truns.
        if trun.offsets[i] > pos {
            std::io::copy(&mut (&mut body).take(trun.offsets[i] - pos), &mut std::io::sink())?;
            pos = trun.offsets[i];
        }
        let mut data = vec![0u8; trun.sizes[i] as usize];
        body.read_exact(&mut data)?;
        pos += data.len() as u64;
        let packet = Packet::new(0, Timestamp::new(frame_pos as i64), SymDuration::new(trun.durations[i] as u64), data);
        pcm.clear();
        dec.decode(&packet, &mut pcm)?;
        let ch = dec.output().1.max(1);
        frame_pos += trun.durations[i] as u64;
        if n < prime {
            continue; // priming frame: output discarded
        }
        let start = if first_at.is_none() { (skip_in_frame as usize).min(pcm.len() / ch) } else { 0 };
        if first_at.is_none() {
            if pcm.len() / ch <= start {
                continue;
            }
            latency = started.elapsed();
            fetch = Duration::from_nanos(f.fetch_ns.load(std::sync::atomic::Ordering::Relaxed) - ns0);
            first_at = Some(seg.start_s + (acc + skip_in_frame) as f64 / rate as f64);
        }
        out.extend(pcm[start * ch..].chunks_exact(ch).map(|f| f.iter().sum::<f32>() / ch as f32));
        if out.len() >= 8192 {
            break;
        }
    }
    Ok(RangedSeek {
        latency,
        fetch,
        requests: f.requests.load(std::sync::atomic::Ordering::Relaxed) - req0,
        first_frame_s: first_at.ok_or_else(|| anyhow!("no audio after the seek"))?,
        audio: out,
        rate,
    })
}
