use anyhow::{bail, Result};
use audiolab::{cputime::thread_cpu, decode, registry};
use std::path::Path;
use std::time::Duration;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("bench") => bench(&args[1..]),
        Some("gapless") => gapless(&args[1..]),
        Some("hls") => hls(&args[1..]),
        #[cfg(feature = "fdk")]
        Some("fdkenc") => fdkenc(&args[1..]),
        _ => bail!("usage: audiolab bench [--fdk-all] [--min-cpu-ms N] FILE..."),
    }
}

fn ext_of(path: &str) -> String {
    Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

/// Decode CPU per stream: decode each file from memory repeatedly on this
/// thread until at least `min_cpu` of thread CPU time has accumulated, then
/// report CPU time per second of audio. Demux + decode + f32 interleave are
/// included; file I/O is not (the file is read once up front).
fn bench(args: &[String]) -> Result<()> {
    let mut min_cpu = Duration::from_millis(3000);
    let mut fdk_all = false;
    let mut resample = false;
    let mut files = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--fdk-all" => fdk_all = true,
            "--resample" => resample = true,
            "--min-cpu-ms" => min_cpu = Duration::from_millis(it.next().unwrap().parse()?),
            f => files.push(f.to_string()),
        }
    }
    #[allow(unused_mut)]
    let mut reg = registry::default_registry();
    #[cfg(feature = "fdk")]
    if fdk_all {
        reg.register(Box::new(registry::AdapterProvider::fdk_aac_all()));
    }
    let _ = fdk_all;
    for f in &files {
        let bytes = std::fs::read(f)?;
        let ext = ext_of(f);
        let first = match decode::decode_bytes(&reg, &bytes, &ext, true) {
            Ok(d) => d,
            Err(e) => {
                println!("{{\"file\":\"{f}\",\"error\":\"{}\"}}", format!("{e:#}").replace('"', "'"));
                continue;
            }
        };
        let audio_s = first.seconds();
        let mut reps = 0u32;
        let t0 = thread_cpu();
        while reps < 3 || thread_cpu() - t0 < min_cpu {
            let d = decode::decode_bytes(&reg, &bytes, &ext, true)?;
            std::hint::black_box(&d.samples);
            reps += 1;
        }
        let cpu = (thread_cpu() - t0).as_secs_f64() / reps as f64;
        let mut line = format!(
            "{{\"file\":\"{}\",\"format\":\"{}\",\"decoder\":\"{}\",\"support\":\"{:?}\",\"rate\":{},\"channels\":{},\"audio_s\":{:.3},\"reps\":{},\"cpu_ms_per_audio_s\":{:.3},\"pct_one_core\":{:.3}",
            Path::new(f).file_name().unwrap().to_string_lossy(), first.format, first.decoder, first.support,
            first.rate, first.channels, audio_s, reps, cpu * 1000.0 / audio_s, cpu / audio_s * 100.0
        );
        if resample && first.rate != 48_000 {
            let pct = resample_cost(&first, 48_000, min_cpu)?;
            line += &format!(",\"resample_to_48k_pct_one_core\":{pct:.3}");
        }
        println!("{line}}}");
    }
    Ok(())
}

/// Cost of converting decoded audio to 48 kHz with rubato's FFT resampler,
/// the likely production choice (the legacy linear resampler aliases).
fn resample_cost(d: &decode::Decoded, to: u32, min_cpu: Duration) -> Result<f64> {
    use rubato::{FftFixedIn, Resampler};
    let ch = d.channels;
    let chunk = 1024;
    let planar: Vec<Vec<f32>> = (0..ch).map(|c| d.samples.iter().skip(c).step_by(ch).copied().collect()).collect();
    let frames = planar[0].len();
    let mut reps = 0u32;
    let t0 = thread_cpu();
    while reps < 3 || thread_cpu() - t0 < min_cpu {
        let mut r = FftFixedIn::<f32>::new(d.rate as usize, to as usize, chunk, 2, ch)?;
        let mut out = r.output_buffer_allocate(true);
        let mut pos = 0;
        while pos + chunk <= frames {
            let input: Vec<&[f32]> = planar.iter().map(|p| &p[pos..pos + chunk]).collect();
            r.process_into_buffer(&input, &mut out, None)?;
            std::hint::black_box(&out);
            pos += chunk;
        }
        reps += 1;
    }
    let cpu = (thread_cpu() - t0).as_secs_f64() / reps as f64;
    Ok(cpu / d.seconds() * 100.0)
}

/// Encodes raw s16le stereo 44.1/48 kHz PCM to ADTS HE-AAC with FDK, for test
/// media only (ffmpeg's native AAC encoder has no SBR).
/// `audiolab fdkenc IN.s16 OUT.aac RATE he|hev2|lc VBR(1-5)`
#[cfg(feature = "fdk")]
fn fdkenc(args: &[String]) -> Result<()> {
    use fdk_aac::enc::*;
    let pcm = std::fs::read(&args[0])?;
    let rate: u32 = args[2].parse()?;
    let aot = match args[3].as_str() {
        "he" => AudioObjectType::Mpeg4HeAac,
        "hev2" => AudioObjectType::Mpeg4HeAacV2,
        _ => AudioObjectType::Mpeg4LowComplexity,
    };
    let bit_rate = match args[4].as_str() {
        "1" => BitRate::VbrVeryLow,
        "2" => BitRate::VbrLow,
        "3" => BitRate::VbrMedium,
        "4" => BitRate::VbrHigh,
        _ => BitRate::VbrVeryHigh,
    };
    let enc = Encoder::new(EncoderParams { bit_rate, sample_rate: rate, transport: Transport::Adts, channels: ChannelMode::Stereo, audio_object_type: aot })
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let samples: Vec<i16> = pcm.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
    let mut out = Vec::new();
    let mut buf = vec![0u8; 8192];
    let mut pos = 0;
    while pos < samples.len() {
        let info = enc.encode(&samples[pos..], &mut buf).map_err(|e| anyhow::anyhow!("{e:?}"))?;
        if info.input_consumed == 0 && info.output_size == 0 { break; }
        pos += info.input_consumed;
        out.extend_from_slice(&buf[..info.output_size]);
    }
    // Flush: feed silence for the encoder delay.
    let zeros = vec![0i16; 4096];
    for _ in 0..8 {
        let info = enc.encode(&zeros, &mut buf).map_err(|e| anyhow::anyhow!("{e:?}"))?;
        out.extend_from_slice(&buf[..info.output_size]);
    }
    std::fs::write(&args[1], out)?;
    Ok(())
}

/// `audiolab gapless DIR`: t0..t2.{ext} against source.wav, u0..u2.opus
/// against source48.wav. MP4 is run with and without the host edit-list trim.
fn gapless(args: &[String]) -> Result<()> {
    use audiolab::gapless::{load, run};
    let dir = &args[0];
    #[allow(unused_mut)]
    let mut reg = registry::default_registry();
    #[cfg(feature = "fdk")]
    if args.get(1).is_some_and(|a| a == "--fdk-all") {
        reg.register(Box::new(registry::AdapterProvider::fdk_aac_all()));
    }
    let cases: &[(&str, &str, &str, bool)] = &[
        ("mp3", "t", "source.wav", false),
        ("m4a", "t", "source.wav", false),
        ("m4a", "t", "source.wav", true),
        ("aac", "t", "source.wav", false),
        ("ogg", "t", "source.wav", false),
        ("opus", "u", "source48.wav", false),
        ("flac", "t", "source.wav", false),
    ];
    for (ext, prefix, src, host_trim) in cases {
        let source = load(&reg, &format!("{dir}/{src}"), false)?;
        let mut tracks = Vec::new();
        for i in 0..3 {
            let wav = load(&reg, &format!("{dir}/{prefix}{i}.wav"), false)?;
            let f = format!("{prefix}{i}.{ext}");
            let mut d = load(&reg, &format!("{dir}/{f}"), *host_trim)?;
            // Positive control: GAPLESS_CONTROL_INSERT=N prepends N zero frames to track 1.
            if i == 1 {
                if let Some(n) = std::env::var("GAPLESS_CONTROL_INSERT").ok().and_then(|v| v.parse::<usize>().ok()) {
                    let mut s = vec![0.0; n * d.channels];
                    s.extend_from_slice(&d.samples);
                    d.samples = s;
                }
            }
            tracks.push((f, d, wav.frames()));
        }
        let r = run(&reg, &source, &tracks)?;
        let label = if *host_trim { format!("{ext}+host-elst") } else { ext.to_string() };
        println!("== {label}: total delta {} frames, SNR {:.1} dB, boundary windows {:.1?} dB, window median {:.1} / p1 {:.1} / min {:.1} dB",
            r.total_delta_frames, r.snr_db, r.boundary_snrs, r.median_window_snr_db, r.p1_window_snr_db, r.min_window_snr_db);
        for t in &r.tracks {
            println!("   {} [{}] source {} decoded {} (delta {:+}) start lag {:+}  {}",
                t.file, t.decoder, t.source_frames, t.decoded_frames, t.decoded_frames as i64 - t.source_frames as i64, t.lag, t.trims);
        }
    }
    Ok(())
}

/// `audiolab hls [--rtt-ms N] [--kbps N] [--seeks N] (DIR | URL)...`
/// DIR is a fixture variant served locally (simulated network); URL is a
/// public playlist. Reports start latency and seek latency (seek call → first
/// decoded frame at the target, playlist already loaded).
fn hls(args: &[String]) -> Result<()> {
    use audiolab::hls;
    let mut rtt = 0u64;
    let mut kbps = 0u64;
    let mut seeks = 20usize;
    let mut streaming = false;
    let mut targets = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--rtt-ms" => rtt = it.next().unwrap().parse()?,
            "--kbps" => kbps = it.next().unwrap().parse()?,
            "--seeks" => seeks = it.next().unwrap().parse()?,
            "--streaming" => streaming = true,
            t => targets.push(t.to_string()),
        }
    }
    let reg = registry::default_registry();
    let mut source: Option<Vec<f32>> = None;
    for t in &targets {
        if !t.starts_with("http") && source.is_none() {
            let src = Path::new(t).parent().unwrap().join("source.wav");
            if let Ok(d) = audiolab::gapless::load(&reg, src.to_str().unwrap(), false) {
                source = Some(d.samples.chunks_exact(d.channels).map(|f| f.iter().sum::<f32>() / d.channels as f32).collect());
            }
        }
        let url = if t.starts_with("http") {
            t.clone()
        } else {
            let p = Path::new(t);
            let base = audiolab::server::spawn(p.parent().unwrap().to_path_buf(), Duration::from_millis(rtt), kbps);
            format!("{base}/{}/index.m3u8", p.file_name().unwrap().to_string_lossy())
        };
        let mut f = hls::Fetcher::new();
        f.streaming = streaming;
        let t0 = std::time::Instant::now();
        let pl = match hls::load(&f, &url) {
            Ok(p) => p,
            Err(e) => { println!("{t}: playlist error: {e:#}"); continue; }
        };
        let playlist_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let init = pl.init.as_ref().map(|u| f.bytes(u)).transpose()?;
        let dur = pl.duration_s();
        let start = match hls::open_at(&reg, &f, &pl, init.clone(), 0.0) {
            Ok((s, _)) => s,
            Err(e) => { println!("{t}: open error: {e:#}"); continue; }
        };
        let mut lat = Vec::new();
        let mut fetch = Vec::new();
        let mut lags = Vec::new();
        // Deterministic spread of targets over the track, avoiding segment starts.
        for i in 0..seeks {
            let target = (dur - 8.0) * ((i as f64 * 0.618_034) % 1.0) + 1.234;
            let (s, _) = hls::open_at(&reg, &f, &pl, init.clone(), target)?;
            lat.push(s.latency.as_secs_f64() * 1000.0);
            fetch.push(s.fetch.as_secs_f64() * 1000.0);
            // True position error: align the post-seek audio with the source.
            if let (Some(src), false) = (&source, t.starts_with("http")) {
                let at = (s.first_frame_s * s.rate as f64).round() as i64;
                lags.push(align(&s.audio, src, at, 2048));
            }
        }
        lat.sort_by(|a, b| a.partial_cmp(b).unwrap());
        fetch.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p = |v: &Vec<f64>, q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
        println!(
            "{{\"stream\":\"{t}\",\"container\":\"{:?}\",\"codecs\":{:?},\"decoder\":\"{}\",\"segments\":{},\"seg_s\":{:.2},\"rtt_ms\":{rtt},\"kbps\":{kbps},\"streaming\":{streaming},\"playlist_ms\":{playlist_ms:.0},\"start_ms\":{:.0},\"seek_ms_p50\":{:.0},\"seek_ms_p95\":{:.0},\"seek_ms_max\":{:.0},\"fetch_ms_p50\":{:.0},\"seek_lag_frames\":{:?}}}",
            start.container, pl.codecs.clone().unwrap_or_default(), start.decoder, pl.segments.len(), pl.segments[0].dur_s,
            start.latency.as_secs_f64() * 1000.0, p(&lat, 0.5), p(&lat, 0.95), lat[lat.len() - 1], p(&fetch, 0.5), { lags.sort(); lags.dedup(); lags }
        );
    }
    Ok(())
}

/// Lag (frames) at which `audio` best matches `src` around frame `at`.
fn align(audio: &[f32], src: &[f32], at: i64, max: i64) -> i64 {
    let n = audio.len().min(4096);
    let mut best = (f64::MIN, 0);
    for lag in -max..=max {
        let s0 = at + lag;
        if s0 < 0 || (s0 as usize + n) > src.len() { continue; }
        let (mut dot, mut e) = (0f64, 0f64);
        for i in 0..n {
            let b = src[s0 as usize + i] as f64;
            dot += audio[i] as f64 * b;
            e += b * b;
        }
        let score = dot / e.sqrt().max(1e-9);
        if score > best.0 { best = (score, lag); }
    }
    best.1
}
