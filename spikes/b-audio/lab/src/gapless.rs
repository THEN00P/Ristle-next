//! Sample-accurate gapless check: decode album tracks back to back and diff
//! the result against the continuous source they were cut from.

use crate::decode::{decode_bytes, Decoded};
use crate::registry::DecoderRegistry;
use anyhow::Result;

pub struct TrackReport {
    pub file: String,
    pub source_frames: usize,
    pub decoded_frames: usize,
    /// Lag (in frames) that best aligns the decoded track with its source
    /// segment. 0 means the start is sample-accurate.
    pub lag: i64,
    pub decoder: &'static str,
    pub trims: String,
}

pub struct Report {
    pub tracks: Vec<TrackReport>,
    pub total_delta_frames: i64,
    /// SNR (dB) of the concatenated decode against the source, whole signal.
    pub snr_db: f64,
    /// Worst SNR in a 10 ms window straddling a track boundary, and the median
    /// over all 10 ms windows. A gap or overlap shows up as a boundary window
    /// far below the median.
    pub worst_boundary_snr_db: f64,
    pub median_window_snr_db: f64,
    /// 1st percentile and minimum over all windows, for scale.
    pub p1_window_snr_db: f64,
    pub min_window_snr_db: f64,
    pub boundary_snrs: Vec<f64>,
}

fn mono(d: &[f32], ch: usize) -> Vec<f32> {
    d.chunks_exact(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect()
}

fn best_lag(decoded: &[f32], source: &[f32], max: i64) -> i64 {
    // Normalised correlation over the first 1 s after skipping 0.1 s.
    let n = 44_100.min(source.len().saturating_sub(10_000));
    let s0 = 4_410usize;
    let mut best = (f64::MIN, 0);
    for lag in -max..=max {
        let mut dot = 0f64;
        let mut e = 0f64;
        for i in s0..s0 + n {
            let j = i as i64 + lag;
            if j < 0 || j as usize >= decoded.len() {
                continue;
            }
            let a = decoded[j as usize] as f64;
            dot += a * source[i] as f64;
            e += a * a;
        }
        let score = dot / e.sqrt().max(1e-9);
        if score > best.0 {
            best = (score, lag);
        }
    }
    best.1
}

fn snr(sig: &[f32], err: &[f32]) -> f64 {
    let ps: f64 = sig.iter().map(|x| (*x as f64).powi(2)).sum();
    let pe: f64 = err.iter().map(|x| (*x as f64).powi(2)).sum();
    10.0 * (ps / pe.max(1e-20)).log10()
}

pub fn run(reg: &DecoderRegistry, source: &Decoded, tracks: &[(String, Decoded, usize)]) -> Result<Report> {
    let ch = source.channels;
    let src = mono(&source.samples, ch);
    let mut out = Vec::new();
    let mut reports = Vec::new();
    let mut start = 0usize;
    let mut boundaries = Vec::new();
    for (file, d, src_frames) in tracks {
        let m = mono(&d.samples, d.channels);
        let seg = &src[start..start + src_frames];
        let lag = best_lag(&m, seg, 3000);
        let trims = format!(
            "demux delay/padding={:?}/{:?} host_trim={:?}",
            d.track_delay, d.track_padding, d.host_trim
        );
        reports.push(TrackReport {
            file: file.clone(),
            source_frames: *src_frames,
            decoded_frames: m.len(),
            lag,
            decoder: d.decoder,
            trims,
        });
        out.extend_from_slice(&m);
        start += src_frames;
        boundaries.push(out.len());
    }
    let _ = reg;
    boundaries.pop();
    let n = out.len().min(src.len());
    let err: Vec<f32> = (0..n).map(|i| out[i] - src[i]).collect();
    let w = (source.rate / 100) as usize;
    let mut win_snr: Vec<f64> = (0..n / w).map(|k| snr(&src[k * w..(k + 1) * w], &err[k * w..(k + 1) * w])).collect();
    let boundary_snrs: Vec<f64> = boundaries
        .iter()
        .map(|b| {
            let a = b.saturating_sub(w / 2);
            let e = (a + w).min(n);
            snr(&src[a..e], &err[a..e])
        })
        .collect();
    let worst_boundary = boundary_snrs.iter().copied().fold(f64::MAX, f64::min);
    win_snr.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Ok(Report {
        tracks: reports,
        total_delta_frames: out.len() as i64 - src.len() as i64,
        snr_db: snr(&src[..n], &err),
        worst_boundary_snr_db: worst_boundary,
        median_window_snr_db: win_snr[win_snr.len() / 2],
        p1_window_snr_db: win_snr[win_snr.len() / 100],
        min_window_snr_db: win_snr[0],
        boundary_snrs,
    })
}

pub fn load(reg: &DecoderRegistry, path: &str, host_mp4_trim: bool) -> Result<Decoded> {
    let ext = std::path::Path::new(path).extension().unwrap().to_str().unwrap().to_ascii_lowercase();
    decode_bytes(reg, &std::fs::read(path)?, &ext, host_mp4_trim)
}
