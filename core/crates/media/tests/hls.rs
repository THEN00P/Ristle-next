//! HLS end to end against an in-memory server (M6a exit): fMP4, TS and
//! packed audio, muxed `traf` filtering, `EXT-X-BYTERANGE`, AES-128
//! including `EXT-X-MAP`, byte-range seeks, streaming decode, live
//! playlists, and the Stream Source's header rule.
//!
//! `ristle-net` provides the local HTTP test server once workstream C lands;
//! until then [`MemoryFetch`] serves the same bytes through the `Fetch` port.

// Test helpers outside #[test] functions are not covered by clippy.toml.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::memory_fetch::MemoryFetch;
use common::{best_lag, fixture, left, reference, snr};
use ristle_media::{
    ContainerStream, DecoderRegistry, Error, Fetch, FetchError, HlsSource, HlsStream, MediaHint,
    MediaStream, Read,
};

/// Frames in the HLS fixtures' source: the whole 44.1 kHz album signal.
const SOURCE_FRAMES: usize = 158_760;
/// ffmpeg's AAC priming, which its HLS output does not signal.
const PRIMING: usize = 1024;

const BASE: &str = "https://media.example/";

fn hls_file(path: &str) -> Vec<u8> {
    std::fs::read(fixture(&format!("hls/{path}"))).unwrap()
}

/// Serves a fixture rendition under `BASE{prefix}`.
fn serve_rendition(fetch: &MemoryFetch, rendition: &str, prefix: &str) -> Vec<String> {
    let playlist = String::from_utf8(hls_file(&format!("{rendition}/index.m3u8"))).unwrap();
    fetch.put(&format!("{BASE}{prefix}index.m3u8"), playlist.clone());
    let mut names = Vec::new();
    for line in playlist.lines() {
        let name = if let Some(rest) = line.strip_prefix("#EXT-X-MAP:URI=\"") {
            rest.trim_end_matches('"').to_owned()
        } else if !line.starts_with('#') && !line.is_empty() {
            line.to_owned()
        } else {
            continue;
        };
        fetch.put(
            &format!("{BASE}{prefix}{name}"),
            hls_file(&format!("{rendition}/{name}")),
        );
        names.push(name);
    }
    names
}

/// An init section whose audio edit list signals the 1024 priming frames,
/// as packagers other than ffmpeg write it. The audio track is the last.
fn with_priming(mut init: Vec<u8>) -> Vec<u8> {
    let at = init.windows(4).rposition(|w| w == b"elst").unwrap() + 16;
    init[at..at + 4].copy_from_slice(&(PRIMING as u32).to_be_bytes());
    init
}

struct Harness {
    runtime: tokio::runtime::Runtime,
    fetch: Arc<MemoryFetch>,
}

impl Harness {
    fn new(chunk: usize) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        Self {
            runtime,
            fetch: Arc::new(MemoryFetch::new(chunk)),
        }
    }

    fn open_with(&self, url: &str, headers: Vec<(String, String)>) -> Result<HlsStream, Error> {
        let source = HlsSource {
            url: url.to_owned(),
            headers,
            hint: MediaHint::default(),
        };
        let fetch: Arc<dyn Fetch> = self.fetch.clone();
        self.runtime.block_on(HlsStream::open(
            source,
            fetch,
            self.runtime.handle().clone(),
            &DecoderRegistry::with_defaults(),
        ))
    }

    fn open(&self, url: &str) -> HlsStream {
        self.open_with(url, Vec::new())
            .unwrap_or_else(|e| panic!("opening {url}: {e}"))
    }
}

/// Reads to the end, waiting out `Pending`; returns the samples and how
/// many reads were pending.
fn read_all(stream: &mut dyn MediaStream) -> Result<(Vec<f32>, usize), Error> {
    let channels = usize::from(stream.format().channels);
    let mut buffer = vec![0f32; 2048 * channels];
    let mut out = Vec::new();
    let mut pending = 0;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match stream.read(&mut buffer)? {
            Read::Frames(frames) => out.extend_from_slice(&buffer[..frames * channels]),
            Read::Pending => {
                pending += 1;
                assert!(Instant::now() < deadline, "the stream never continued");
                std::thread::sleep(Duration::from_millis(1));
            }
            Read::End => return Ok((out, pending)),
        }
    }
}

/// The decoded signal must start at `source_start` of the reference.
fn assert_matches_source(samples: &[f32], source_start: usize, what: &str) {
    let source = reference(44_100, source_start..SOURCE_FRAMES);
    let lag = best_lag(&left(samples, 2), &left(&source, 2), 4096, 8192, 3000);
    assert_eq!(lag, 0, "{what}: offset from the source");
    let ratio = snr(&source, samples);
    assert!(ratio > 15.0, "{what}: {ratio:.1} dB");
}

#[test]
fn fmp4_plays_with_the_priming_its_edit_list_signals() {
    let harness = Harness::new(4096);
    serve_rendition(&harness.fetch, "fmp4", "a/");
    harness.fetch.put(
        &format!("{BASE}a/init.mp4"),
        with_priming(hls_file("fmp4/init.mp4")),
    );
    let mut stream = harness.open(&format!("{BASE}a/index.m3u8"));
    assert!(!stream.is_live());
    assert_eq!(stream.duration_ms(), Some(3623));
    let (samples, _) = read_all(&mut stream).unwrap();
    // The edit list's priming is gone; ffmpeg's fragmented init gives no
    // length, so the last frame's padding remains (under one frame).
    let frames = samples.len() / 2;
    assert!(
        (SOURCE_FRAMES..SOURCE_FRAMES + 1024).contains(&frames),
        "{frames}"
    );
    assert_matches_source(&samples, 0, "fMP4");
}

#[test]
fn ts_segments_play_and_keep_their_unsignalled_priming() {
    let harness = Harness::new(1000);
    serve_rendition(&harness.fetch, "ts", "t/");
    let mut stream = harness.open(&format!("{BASE}t/index.m3u8"));
    let (samples, _) = read_all(&mut stream).unwrap();
    let source = left(&reference(44_100, 0..SOURCE_FRAMES), 2);
    assert_eq!(
        best_lag(&left(&samples, 2), &source, 4096, 8192, 3000),
        PRIMING as i64
    );
    assert!(samples.len() / 2 >= SOURCE_FRAMES + PRIMING);
}

#[test]
fn a_muxed_rendition_plays_only_its_audio_track() {
    let harness = Harness::new(4096);
    serve_rendition(&harness.fetch, "muxed", "m/");
    harness.fetch.put(
        &format!("{BASE}m/init.mp4"),
        with_priming(hls_file("muxed/init.mp4")),
    );
    let mut stream = harness.open(&format!("{BASE}m/index.m3u8"));
    let (samples, _) = read_all(&mut stream).unwrap();
    assert_matches_source(&samples, 0, "muxed fMP4");
}

/// The fMP4 rendition encrypted with AES-128: a key with an IV before the
/// map (so the init section is encrypted), then a key without one, so the
/// segments use their sequence numbers as IVs.
fn serve_encrypted_fmp4(fetch: &MemoryFetch, prefix: &str) {
    use cbc::cipher::{BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
    let key = *b"0123456789abcdef";
    let init_iv = [0x42u8; 16];
    let encrypt = |data: &[u8], iv: [u8; 16]| {
        cbc::Encryptor::<aes::Aes128>::new(&key.into(), &iv.into())
            .encrypt_padded_vec_mut::<Pkcs7>(data)
    };
    fetch.put(&format!("{BASE}keys/k1"), key.to_vec());
    let plain = String::from_utf8(hls_file("fmp4/index.m3u8")).unwrap();
    let mut playlist = String::new();
    let mut sequence = 0u64;
    for line in plain.lines() {
        if line.starts_with("#EXT-X-MAP") {
            playlist.push_str("#EXT-X-KEY:METHOD=AES-128,URI=\"/keys/k1\",IV=0x42424242424242424242424242424242\n");
            playlist.push_str(line);
            playlist.push_str("\n#EXT-X-KEY:METHOD=AES-128,URI=\"/keys/k1\"\n");
            let init = with_priming(hls_file("fmp4/init.mp4"));
            fetch.put(&format!("{BASE}{prefix}init.mp4"), encrypt(&init, init_iv));
        } else if !line.starts_with('#') && !line.is_empty() {
            let segment = hls_file(&format!("fmp4/{line}"));
            fetch.put(
                &format!("{BASE}{prefix}{line}"),
                encrypt(&segment, u128::from(sequence).to_be_bytes()),
            );
            sequence += 1;
            playlist.push_str(line);
            playlist.push('\n');
        } else {
            playlist.push_str(line);
            playlist.push('\n');
        }
    }
    fetch.put(&format!("{BASE}{prefix}index.m3u8"), playlist);
}

#[test]
fn aes_128_covers_the_init_section_and_every_segment() {
    let harness = Harness::new(777);
    serve_encrypted_fmp4(&harness.fetch, "e/");
    serve_rendition(&harness.fetch, "fmp4", "a/");
    harness.fetch.put(
        &format!("{BASE}a/init.mp4"),
        with_priming(hls_file("fmp4/init.mp4")),
    );
    let (encrypted, _) = read_all(&mut harness.open(&format!("{BASE}e/index.m3u8"))).unwrap();
    let (plain, _) = read_all(&mut harness.open(&format!("{BASE}a/index.m3u8"))).unwrap();
    assert_eq!(encrypted, plain);
    // The key was fetched once for the init and all four segments.
    let key_fetches = harness
        .fetch
        .requests()
        .iter()
        .filter(|r| r.url.ends_with("/keys/k1"))
        .count();
    assert_eq!(key_fetches, 1);
}

#[test]
fn aes_128_ts_segments_decrypt_while_downloading() {
    use cbc::cipher::{BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
    let harness = Harness::new(500);
    let key = [9u8; 16];
    harness.fetch.put(&format!("{BASE}k"), key.to_vec());
    let plain = String::from_utf8(hls_file("ts/index.m3u8")).unwrap();
    let mut playlist = String::new();
    let mut sequence = 0u64;
    for line in plain.lines() {
        if line.starts_with("#EXTINF") && sequence == 0 {
            playlist.push_str("#EXT-X-KEY:METHOD=AES-128,URI=\"k\"\n");
        }
        if !line.starts_with('#') && !line.is_empty() {
            let segment = hls_file(&format!("ts/{line}"));
            let iv = u128::from(sequence).to_be_bytes();
            let encrypted = cbc::Encryptor::<aes::Aes128>::new(&key.into(), &iv.into())
                .encrypt_padded_vec_mut::<Pkcs7>(&segment);
            harness.fetch.put(&format!("{BASE}{line}"), encrypted);
            harness.fetch.put(&format!("{BASE}plain/{line}"), segment);
            sequence += 1;
        }
        playlist.push_str(line);
        playlist.push('\n');
    }
    harness.fetch.put(&format!("{BASE}index.m3u8"), playlist);
    harness.fetch.put(&format!("{BASE}plain/index.m3u8"), plain);
    let (encrypted, _) = read_all(&mut harness.open(&format!("{BASE}index.m3u8"))).unwrap();
    let (clear, _) = read_all(&mut harness.open(&format!("{BASE}plain/index.m3u8"))).unwrap();
    assert_eq!(encrypted, clear);
}

#[test]
fn byte_range_playlists_read_one_file() {
    let harness = Harness::new(4096);
    let mut all = with_priming(hls_file("fmp4/init.mp4"));
    let mut playlist = format!(
        "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-MAP:URI=\"all.mp4\",BYTERANGE=\"{}@0\"\n",
        all.len()
    );
    let plain = String::from_utf8(hls_file("fmp4/index.m3u8")).unwrap();
    let mut first = true;
    for line in plain.lines() {
        if line.starts_with("#EXTINF") {
            playlist.push_str(line);
            playlist.push('\n');
        } else if !line.starts_with('#') && !line.is_empty() {
            let segment = hls_file(&format!("fmp4/{line}"));
            // The first range has an offset; the rest continue from it.
            if first {
                playlist.push_str(&format!(
                    "#EXT-X-BYTERANGE:{}@{}\n",
                    segment.len(),
                    all.len()
                ));
                first = false;
            } else {
                playlist.push_str(&format!("#EXT-X-BYTERANGE:{}\n", segment.len()));
            }
            playlist.push_str("all.mp4\n");
            all.extend(segment);
        }
    }
    playlist.push_str("#EXT-X-ENDLIST\n");
    harness.fetch.put(&format!("{BASE}r/all.mp4"), all);
    harness.fetch.put(&format!("{BASE}r/index.m3u8"), playlist);
    serve_rendition(&harness.fetch, "fmp4", "a/");
    harness.fetch.put(
        &format!("{BASE}a/init.mp4"),
        with_priming(hls_file("fmp4/init.mp4")),
    );
    let (ranged, _) = read_all(&mut harness.open(&format!("{BASE}r/index.m3u8"))).unwrap();
    let (separate, _) = read_all(&mut harness.open(&format!("{BASE}a/index.m3u8"))).unwrap();
    assert_eq!(ranged, separate);
    assert!(
        harness
            .fetch
            .requests()
            .iter()
            .filter(|r| r.url.ends_with("all.mp4"))
            .all(|r| r.range.is_some())
    );
}

/// Packed ADTS segments with Apple's ID3 timestamp tag, cut from `track.aac`.
#[test]
fn packed_audio_plays_like_the_adts_it_was_cut_from() {
    let harness = Harness::new(300);
    let adts = std::fs::read(fixture("track.aac")).unwrap();
    let mut frames = Vec::new();
    let mut at = 0;
    while at + 7 <= adts.len() {
        let len = (usize::from(adts[at + 3] & 3) << 11)
            | (usize::from(adts[at + 4]) << 3)
            | usize::from(adts[at + 5] >> 5);
        frames.push(&adts[at..at + len]);
        at += len;
    }
    let mut playlist = String::from("#EXTM3U\n#EXT-X-TARGETDURATION:1\n");
    for (index, chunk) in frames.chunks(20).enumerate() {
        let pts = 900_000 + (index * 20 * 1024) as i64 * 90_000 / 44_100;
        let mut segment = id3_timestamp(pts);
        for frame in chunk {
            segment.extend_from_slice(frame);
        }
        harness.fetch.put(&format!("{BASE}p/s{index}.aac"), segment);
        playlist.push_str(&format!(
            "#EXTINF:{:.6},\ns{index}.aac\n",
            (chunk.len() * 1024) as f64 / 44_100.0
        ));
    }
    playlist.push_str("#EXT-X-ENDLIST\n");
    harness.fetch.put(&format!("{BASE}p/index.m3u8"), playlist);
    let (packed, _) = read_all(&mut harness.open(&format!("{BASE}p/index.m3u8"))).unwrap();
    let mut file = ContainerStream::open_file(
        &fixture("track.aac"),
        &MediaHint::default(),
        &DecoderRegistry::with_defaults(),
    )
    .unwrap();
    let (whole, _) = read_all(&mut file).unwrap();
    assert_eq!(packed, whole);
}

fn id3_timestamp(pts: i64) -> Vec<u8> {
    let owner = b"com.apple.streaming.transportStreamTimestamp\0";
    let mut frame = owner.to_vec();
    frame.extend_from_slice(&pts.to_be_bytes());
    let mut body = b"PRIV".to_vec();
    body.extend_from_slice(&(frame.len() as u32).to_be_bytes());
    body.extend_from_slice(&[0, 0]);
    body.extend_from_slice(&frame);
    let mut tag = b"ID3\x04\x00\x00\x00\x00".to_vec();
    tag.extend_from_slice(&[(body.len() >> 7) as u8 & 0x7f, body.len() as u8 & 0x7f]);
    tag.extend_from_slice(&body);
    tag
}

#[test]
fn a_seek_into_fmp4_fetches_only_from_the_target_sample() {
    for (prefix, encrypted) in [("a/", false), ("e/", true)] {
        let harness = Harness::new(4096);
        if encrypted {
            serve_encrypted_fmp4(&harness.fetch, prefix);
        } else {
            serve_rendition(&harness.fetch, "fmp4", prefix);
            harness.fetch.put(
                &format!("{BASE}{prefix}init.mp4"),
                with_priming(hls_file("fmp4/init.mp4")),
            );
        }
        // Hold seg0 after its first chunk, so the loader started by opening
        // fetches nothing else and the log after the seek is the seek's own.
        let _gate = harness.fetch.gate(&format!("{BASE}{prefix}seg0.m4s"));
        let mut stream = harness.open(&format!("{BASE}{prefix}index.m3u8"));
        let before = harness.fetch.requests().len();
        stream.seek(2_500).unwrap();
        let (samples, _) = read_all(&mut stream).unwrap();
        let target = 2_500 * 44_100 / 1000;
        assert_matches_source(&samples, target, &format!("seek ({prefix})"));
        let frames = samples.len() / 2;
        assert!(
            (SOURCE_FRAMES - target..SOURCE_FRAMES - target + 1024).contains(&frames),
            "{frames}"
        );
        // The segment holding 2.5 s (seg2) was entered by a range starting
        // inside it, after its moof was read.
        let after: Vec<_> = harness.fetch.requests()[before..].to_vec();
        let seg2: Vec<_> = after
            .iter()
            .filter(|r| r.url.ends_with("seg2.m4s"))
            .collect();
        assert_eq!(seg2.len(), 2, "{prefix}: {seg2:?}");
        assert_eq!(seg2[0].range.map(|r| r.start), Some(0));
        assert!(
            seg2[1]
                .range
                .is_some_and(|r| r.start > 1000 && r.length.is_none()),
            "{seg2:?}"
        );
        assert!(!after.iter().any(|r| r.url.ends_with("seg1.m4s")));
    }
}

#[test]
fn a_seek_into_ts_decodes_from_the_segment_start() {
    let harness = Harness::new(4096);
    serve_rendition(&harness.fetch, "ts", "t/");
    let mut stream = harness.open(&format!("{BASE}t/index.m3u8"));
    stream.seek(1_700).unwrap();
    let (samples, _) = read_all(&mut stream).unwrap();
    // Playlist time includes the unsignalled priming.
    let target = 1_700 * 44_100 / 1000 - PRIMING;
    let source = left(&reference(44_100, target..SOURCE_FRAMES), 2);
    assert_eq!(best_lag(&left(&samples, 2), &source, 2048, 8192, 3000), 0);
    stream.seek(10_000).unwrap();
    assert_eq!(read_all(&mut stream).unwrap().0.len(), 0);
}

#[test]
fn decoding_starts_before_a_segment_has_downloaded() {
    let harness = Harness::new(2000);
    serve_rendition(&harness.fetch, "ts", "t/");
    // Everything after the first 2000 bytes of seg0 waits.
    let gate = harness.fetch.gate(&format!("{BASE}t/seg0.ts"));
    let mut stream = harness.open(&format!("{BASE}t/index.m3u8"));
    let mut buffer = vec![0f32; 4096];
    let mut frames = 0;
    let started = Instant::now();
    loop {
        match stream.read(&mut buffer).unwrap() {
            Read::Frames(n) => frames += n,
            Read::Pending => break,
            Read::End => panic!("ended early"),
        }
        assert!(started.elapsed() < Duration::from_secs(10));
    }
    // Some audio came out of the first 2000 bytes of an 18 KB segment.
    assert!(frames > 0 && frames < 44_100, "{frames}");
    gate.add_permits(1000);
    let (rest, _) = read_all(&mut stream).unwrap();
    assert!(frames + rest.len() / 2 >= SOURCE_FRAMES);
}

#[test]
fn headers_go_only_to_the_playlists_origin() {
    let harness = Harness::new(4096);
    // The playlist names segments on a CDN.
    let plain = String::from_utf8(hls_file("ts/index.m3u8")).unwrap();
    let mut playlist = String::new();
    for line in plain.lines() {
        if !line.starts_with('#') && !line.is_empty() {
            let cdn = format!("https://cdn.example/{line}");
            harness.fetch.put(&cdn, hls_file(&format!("ts/{line}")));
            playlist.push_str(&cdn);
        } else {
            playlist.push_str(line);
        }
        playlist.push('\n');
    }
    harness.fetch.put(&format!("{BASE}index.m3u8"), playlist);
    let headers = vec![("Authorization".to_owned(), "Bearer secret".to_owned())];
    let mut stream = harness
        .open_with(&format!("{BASE}index.m3u8"), headers)
        .unwrap();
    read_all(&mut stream).unwrap();
    for request in harness.fetch.requests() {
        let expected = request.url.starts_with(BASE);
        assert_eq!(!request.headers.is_empty(), expected, "{}", request.url);
    }
}

#[test]
fn a_master_playlist_plays_its_audio_rendition() {
    let harness = Harness::new(4096);
    serve_rendition(&harness.fetch, "fmp4", "audio/");
    serve_rendition(&harness.fetch, "muxed", "video/");
    harness.fetch.put(
        &format!("{BASE}master.m3u8"),
        "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"main\",DEFAULT=YES,URI=\"audio/index.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=300000,CODECS=\"mp4v.20.9,mp4a.40.2\",AUDIO=\"a\"\nvideo/index.m3u8\n",
    );
    let mut stream = harness.open(&format!("{BASE}master.m3u8"));
    read_all(&mut stream).unwrap();
    assert!(
        harness
            .fetch
            .requests()
            .iter()
            .all(|r| !r.url.contains("/video/seg"))
    );
    assert_eq!(stream.decoder(), "symphonia");
}

#[test]
fn a_live_playlist_starts_near_its_edge_and_follows_reloads() {
    let harness = Harness::new(4096);
    serve_rendition(&harness.fetch, "ts", "l/");
    let full = String::from_utf8(hls_file("ts/index.m3u8")).unwrap();
    // Live: no PLAYLIST-TYPE, no ENDLIST; four segments now.
    let live: String = full
        .lines()
        .filter(|l| !l.starts_with("#EXT-X-PLAYLIST-TYPE") && !l.starts_with("#EXT-X-ENDLIST"))
        .map(|l| format!("{l}\n"))
        .collect();
    harness.fetch.put(&format!("{BASE}l/index.m3u8"), live);
    let mut stream = harness.open(&format!("{BASE}l/index.m3u8"));
    assert!(stream.is_live());
    assert!(matches!(stream.seek(0), Err(Error::NotSeekable)));
    // The next reload ends the stream.
    harness.fetch.put(&format!("{BASE}l/index.m3u8"), full);
    let (samples, pending) = read_all(&mut stream).unwrap();
    assert!(pending > 0, "the reload waits for the target duration");
    // Three segments from the edge: seg1..seg3.
    let requested: Vec<String> = harness
        .fetch
        .requests()
        .iter()
        .map(|r| r.url.clone())
        .collect();
    assert!(
        !requested.iter().any(|u| u.ends_with("seg0.ts")),
        "{requested:?}"
    );
    assert!(samples.len() / 2 > 2 * 44_100);
}

#[test]
fn drm_and_missing_segments_fail_with_their_reason() {
    let harness = Harness::new(4096);
    harness.fetch.put(
        &format!("{BASE}drm.m3u8"),
        "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,KEYFORMAT=\"com.apple.streamingkeydelivery\",URI=\"skd://k\"\n#EXTINF:1,\ns.ts\n#EXT-X-ENDLIST\n",
    );
    assert!(matches!(
        harness.open_with(&format!("{BASE}drm.m3u8"), Vec::new()),
        Err(Error::UnsupportedEncryption { .. })
    ));

    serve_rendition(&harness.fetch, "ts", "x/");
    harness
        .fetch
        .files
        .lock()
        .unwrap()
        .remove(&format!("{BASE}x/seg2.ts"));
    let mut stream = harness.open(&format!("{BASE}x/index.m3u8"));
    match read_all(&mut stream) {
        Err(Error::Fetch {
            url,
            error: FetchError::Status { status: 404 },
        }) => {
            assert!(url.ends_with("seg2.ts"));
        }
        other => panic!(
            "expected the missing segment: {:?}",
            other.map(|(s, p)| (s.len(), p))
        ),
    }
}

#[test]
fn live_playback_initializes_from_the_segment_it_starts_at() {
    let harness = Harness::new(4096);
    serve_rendition(&harness.fetch, "ts", "t/");
    serve_rendition(&harness.fetch, "fmp4", "a/");
    harness.fetch.put(
        &format!("{BASE}a/init.mp4"),
        with_priming(hls_file("fmp4/init.mp4")),
    );
    // A TS segment, then a discontinuity into fMP4. Live playback starts
    // three segments from the edge, inside the fMP4 part.
    let playlist = |ended: bool| {
        let mut text = String::from(
            "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:1.0,\nt/seg0.ts\n#EXT-X-DISCONTINUITY\n#EXT-X-MAP:URI=\"a/init.mp4\"\n",
        );
        for segment in ["seg1", "seg2", "seg3"] {
            text.push_str(&format!("#EXTINF:0.998458,\na/{segment}.m4s\n"));
        }
        if ended {
            text.push_str("#EXT-X-ENDLIST\n");
        }
        text
    };
    harness
        .fetch
        .put(&format!("{BASE}live.m3u8"), playlist(false));
    let mut stream = harness.open(&format!("{BASE}live.m3u8"));
    harness
        .fetch
        .put(&format!("{BASE}live.m3u8"), playlist(true));
    let (samples, _) = read_all(&mut stream).unwrap();
    assert!(samples.len() / 2 > 2 * 44_100, "{}", samples.len() / 2);
    assert!(
        !harness
            .fetch
            .requests()
            .iter()
            .any(|r| r.url.ends_with("seg0.ts"))
    );
}

#[test]
fn a_new_init_section_with_another_codec_configuration_is_refused() {
    let harness = Harness::new(4096);
    serve_rendition(&harness.fetch, "fmp4", "a/");
    // The same codec, rate and layout, but AAC Main instead of AAC-LC in
    // the AudioSpecificConfig: the open decoder cannot follow.
    let mut other = hls_file("fmp4/init.mp4");
    let asc = other.windows(2).position(|w| w == [0x12, 0x10]).unwrap();
    other[asc] = 0x0a;
    harness.fetch.put(&format!("{BASE}b/init.mp4"), other);
    harness.fetch.put(
        &format!("{BASE}switch.m3u8"),
        "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-MAP:URI=\"a/init.mp4\"\n#EXTINF:1.021678,\na/seg0.m4s\n#EXT-X-DISCONTINUITY\n#EXT-X-MAP:URI=\"b/init.mp4\"\n#EXTINF:0.998458,\na/seg1.m4s\n#EXT-X-ENDLIST\n",
    );
    let mut stream = harness.open(&format!("{BASE}switch.m3u8"));
    assert!(matches!(
        read_all(&mut stream),
        Err(Error::UnsupportedCodec(_))
    ));
}

#[test]
fn ts_segments_may_split_frames() {
    // RFC 8216 does not require segments to end on PES or frame boundaries:
    // cut one TS file at arbitrary packet boundaries.
    let harness = Harness::new(4096);
    let ts = std::fs::read(fixture("track-aac.ts")).unwrap();
    let packets = ts.len() / 188;
    let cuts = [0, (packets / 3 + 1) * 188, packets * 2 / 3 * 188, ts.len()];
    let mut playlist = String::from("#EXTM3U\n#EXT-X-TARGETDURATION:1\n");
    for (index, range) in cuts.windows(2).enumerate() {
        harness.fetch.put(
            &format!("{BASE}cut/{index}.ts"),
            ts[range[0]..range[1]].to_vec(),
        );
        playlist.push_str(&format!("#EXTINF:0.4,\n{index}.ts\n"));
    }
    playlist.push_str("#EXT-X-ENDLIST\n");
    harness
        .fetch
        .put(&format!("{BASE}cut/index.m3u8"), playlist);
    let (cut, _) = read_all(&mut harness.open(&format!("{BASE}cut/index.m3u8"))).unwrap();
    let mut whole = ContainerStream::open_file(
        &fixture("track-aac.ts"),
        &MediaHint::default(),
        &DecoderRegistry::with_defaults(),
    )
    .unwrap();
    assert_eq!(cut, read_all(&mut whole).unwrap().0);
}

/// `(EXTINF line, URI)` pairs of a fixture rendition's playlist.
fn rendition_segments(rendition: &str) -> Vec<(String, String)> {
    let text = String::from_utf8(hls_file(&format!("{rendition}/index.m3u8"))).unwrap();
    let mut out = Vec::new();
    let mut extinf = None;
    for line in text.lines() {
        if line.starts_with("#EXTINF") {
            extinf = Some(line.to_owned());
        } else if !line.starts_with('#') && !line.is_empty() {
            out.push((extinf.take().unwrap(), line.to_owned()));
        }
    }
    out
}

fn extinf_frames(extinf: &str) -> usize {
    let seconds: f64 = extinf
        .trim_start_matches("#EXTINF:")
        .trim_end_matches(',')
        .parse()
        .unwrap();
    (seconds * 44_100.0).round() as usize
}

/// A playlist that plays `first` (rendition, segment indices), then a
/// discontinuity, then `second`. fMP4 parts carry their `EXT-X-MAP`.
fn mixed_playlist(harness: &Harness, first: (&str, &[usize]), second: (&str, &[usize])) -> usize {
    serve_rendition(&harness.fetch, "ts", "ts/");
    serve_rendition(&harness.fetch, "fmp4", "fmp4/");
    let mut text = String::from("#EXTM3U\n#EXT-X-TARGETDURATION:2\n");
    let mut frames = 0;
    for (part, (rendition, indices)) in [first, second].into_iter().enumerate() {
        if part == 1 {
            text.push_str("#EXT-X-DISCONTINUITY\n");
        }
        if rendition == "fmp4" {
            text.push_str("#EXT-X-MAP:URI=\"fmp4/init.mp4\"\n");
        }
        let segments = rendition_segments(rendition);
        for &index in indices {
            let (extinf, uri) = &segments[index];
            frames += extinf_frames(extinf);
            text.push_str(&format!("{extinf}\n{rendition}/{uri}\n"));
        }
    }
    text.push_str("#EXT-X-ENDLIST\n");
    harness.fetch.put(&format!("{BASE}mixed.m3u8"), text);
    frames
}

#[test]
fn a_format_discontinuity_from_ts_to_fmp4_keeps_playing() {
    let harness = Harness::new(4096);
    let expected = mixed_playlist(&harness, ("ts", &[0, 1]), ("fmp4", &[2, 3]));
    let (samples, _) = read_all(&mut harness.open(&format!("{BASE}mixed.m3u8"))).unwrap();
    let frames = samples.len() / 2;
    // Each container keeps its own priming and padding: within a frame per
    // segment of the playlist's duration.
    assert!(
        frames.abs_diff(expected) <= 4 * 1024,
        "{frames} against {expected}"
    );
    assert!(
        harness
            .fetch
            .requests()
            .iter()
            .any(|r| r.url.ends_with("fmp4/seg3.m4s"))
    );
}

#[test]
fn a_format_discontinuity_from_fmp4_to_ts_keeps_playing() {
    let harness = Harness::new(4096);
    let expected = mixed_playlist(&harness, ("fmp4", &[0, 1]), ("ts", &[2, 3]));
    let (samples, _) = read_all(&mut harness.open(&format!("{BASE}mixed.m3u8"))).unwrap();
    let frames = samples.len() / 2;
    assert!(
        frames.abs_diff(expected) <= 4 * 1024,
        "{frames} against {expected}"
    );
    assert!(
        harness
            .fetch
            .requests()
            .iter()
            .any(|r| r.url.ends_with("ts/seg3.ts"))
    );
}

#[test]
fn a_format_discontinuity_into_another_codec_configuration_is_refused() {
    let harness = Harness::new(4096);
    mixed_playlist(&harness, ("ts", &[0, 1]), ("fmp4", &[2, 3]));
    // AAC Main instead of AAC-LC in the fMP4 part.
    let mut other = hls_file("fmp4/init.mp4");
    let asc = other.windows(2).position(|w| w == [0x12, 0x10]).unwrap();
    other[asc] = 0x0a;
    harness.fetch.put(&format!("{BASE}fmp4/init.mp4"), other);
    let mut stream = harness.open(&format!("{BASE}mixed.m3u8"));
    assert!(matches!(
        read_all(&mut stream),
        Err(Error::UnsupportedCodec(_))
    ));
}

#[test]
fn a_seek_across_a_format_discontinuity_reads_the_other_container() {
    let harness = Harness::new(4096);
    let expected = mixed_playlist(&harness, ("fmp4", &[0, 1]), ("ts", &[2, 3]));
    let mut stream = harness.open(&format!("{BASE}mixed.m3u8"));
    // Into the TS part's second segment (no discontinuity tag of its own),
    // then back into the fMP4 part.
    stream.seek(3_100).unwrap();
    let (tail, _) = read_all(&mut stream).unwrap();
    let after = expected - 3_100 * 44_100 / 1000;
    assert!(
        (tail.len() / 2).abs_diff(after) <= 3 * 1024,
        "{} against {after}",
        tail.len() / 2
    );
    stream.seek(500).unwrap();
    let (again, _) = read_all(&mut stream).unwrap();
    assert!((again.len() / 2).abs_diff(expected - 22_050) <= 4 * 1024);
}
