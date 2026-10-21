//! Progressive Stream Sources: HTTP with Range seeks, Plugin Byte Sources,
//! and host-side AES-CTR/CBC decryption. Each must decode exactly what the
//! same file decodes to locally, including after seeks.

// Test helpers outside #[test] functions are not covered by clippy.toml.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use common::fixture;
use common::memory_fetch::MemoryFetch;
use ristle_media::{
    AesMode, ByteSourceHandle, ByteSourceReader, ContainerStream, DecoderRegistry, Decryption,
    Error, Fetch, FetchError, Location, MediaHint, MediaStream, ProgressiveSource,
    ProgressiveStream, Read, SourceError,
};

const URL: &str = "https://media.example/track";

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

    fn open(
        &self,
        location: Location,
        decrypt: Option<Decryption>,
    ) -> Result<ProgressiveStream, Error> {
        let source = ProgressiveSource {
            location,
            hint: MediaHint::default(),
            decrypt,
        };
        let fetch: Arc<dyn Fetch> = self.fetch.clone();
        self.runtime.block_on(ProgressiveStream::open(
            source,
            fetch,
            self.runtime.handle().clone(),
            &DecoderRegistry::with_defaults(),
        ))
    }

    fn http(&self, headers: Vec<(String, String)>) -> Location {
        Location::Http {
            url: URL.to_owned(),
            headers,
        }
    }
}

fn read_all(stream: &mut dyn MediaStream) -> (Vec<f32>, usize) {
    let channels = usize::from(stream.format().channels);
    let mut buffer = vec![0f32; 2048 * channels];
    let mut out = Vec::new();
    let mut pending = 0;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match stream.read(&mut buffer).unwrap() {
            Read::Frames(frames) => out.extend_from_slice(&buffer[..frames * channels]),
            Read::Pending => {
                pending += 1;
                assert!(Instant::now() < deadline, "the stream never continued");
                std::thread::sleep(Duration::from_millis(1));
            }
            Read::End => return (out, pending),
        }
    }
}

fn local(name: &str, seek_ms: Option<u64>) -> Vec<f32> {
    let mut stream = ContainerStream::open_file(
        &fixture(name),
        &MediaHint::default(),
        &DecoderRegistry::with_defaults(),
    )
    .unwrap();
    if let Some(ms) = seek_ms {
        stream.seek(ms).unwrap();
    }
    read_all(&mut stream).0
}

const FILES: [&str; 6] = [
    "album-1.mp3",
    "album-1.m4a",
    "album-1.ogg",
    "album-1.opus",
    "album-1.flac",
    "track-aac.ts",
];

#[test]
fn http_streams_decode_like_the_local_file() {
    for name in FILES {
        // Small chunks: the window fills while the decoder waits.
        let harness = Harness::new(1500);
        harness
            .fetch
            .put(URL, std::fs::read(fixture(name)).unwrap());
        let mut stream = harness.open(harness.http(Vec::new()), None).unwrap();
        let (remote, _) = read_all(&mut stream);
        assert_eq!(remote, local(name, None), "{name}");
    }
}

#[test]
fn http_seeks_land_where_local_seeks_do() {
    for name in FILES {
        let harness = Harness::new(4096);
        harness
            .fetch
            .put(URL, std::fs::read(fixture(name)).unwrap());
        let mut stream = harness.open(harness.http(Vec::new()), None).unwrap();
        stream.seek(700).unwrap();
        let (remote, _) = read_all(&mut stream);
        assert_eq!(remote, local(name, Some(700)), "{name}");
    }
}

#[test]
fn an_mp4_with_its_moov_at_the_end_opens_with_ranges() {
    // ffmpeg writes the moov after the mdat unless asked otherwise.
    let harness = Harness::new(4096);
    let mut file = std::fs::read(fixture("album-1.m4a")).unwrap();
    let moov = file.windows(4).position(|w| w == b"moov").unwrap() - 4;
    let mdat = file.windows(4).position(|w| w == b"mdat").unwrap();
    assert!(moov > mdat);
    // A 16 MB free box before the moov puts it beyond the fetch's 4 MiB
    // read-ahead, so it is reached by a Range request however fast the
    // server is (sample offsets point into the mdat, so they stay valid).
    let mut free = ((16u32 << 20) + 8).to_be_bytes().to_vec();
    free.extend_from_slice(b"free");
    free.resize((16 << 20) + 8, 0);
    file.splice(moov..moov, free);
    let expected = {
        let mut local = ContainerStream::open_reader(
            std::io::Cursor::new(file.clone()),
            &MediaHint::default(),
            &DecoderRegistry::with_defaults(),
        )
        .unwrap();
        read_all(&mut local).0
    };
    harness.fetch.put(URL, file);
    let mut stream = harness
        .open(harness.http(vec![("Cookie".into(), "a=b".into())]), None)
        .unwrap();
    assert_eq!(read_all(&mut stream).0, expected);
    let requests = harness.fetch.requests();
    assert!(
        requests
            .iter()
            .any(|r| r.range.is_some_and(|range| range.start > 0)),
        "{requests:?}"
    );
    assert!(
        requests
            .iter()
            .all(|r| r.headers == vec![("Cookie".to_owned(), "a=b".to_owned())])
    );
}

fn encrypted(name: &str, mode: AesMode, skip: usize) -> (Vec<u8>, Decryption) {
    use cbc::cipher::block_padding::{NoPadding, Pkcs7};
    use cbc::cipher::{BlockEncryptMut, KeyIvInit, StreamCipher};
    let key = vec![0x5a; 16];
    let iv = [0x11; 16];
    let mut plain = vec![0xa5; skip];
    plain.extend(std::fs::read(fixture(name)).unwrap());
    let cipher = match mode {
        AesMode::Ctr => {
            let mut data = plain;
            ctr::Ctr128BE::<aes::Aes128>::new_from_slices(&key, &iv)
                .unwrap()
                .apply_keystream(&mut data);
            data
        }
        AesMode::Cbc => {
            // Without padding the resource is whole blocks; MP3 readers skip
            // the trailing zeros.
            let mut data = plain;
            data.resize(data.len().div_ceil(16) * 16, 0);
            cbc::Encryptor::<aes::Aes128>::new_from_slices(&key, &iv)
                .unwrap()
                .encrypt_padded_vec_mut::<NoPadding>(&data)
        }
        AesMode::CbcPkcs7 => cbc::Encryptor::<aes::Aes128>::new_from_slices(&key, &iv)
            .unwrap()
            .encrypt_padded_vec_mut::<Pkcs7>(&plain),
    };
    (
        cipher,
        Decryption {
            mode,
            key,
            iv,
            skip: skip as u32,
        },
    )
}

#[test]
fn host_side_decryption_covers_ctr_cbc_and_pkcs7_with_seeks() {
    for (name, mode, skip) in [
        // Spotify's files carry a 167-byte header before the Ogg.
        ("album-1.ogg", AesMode::Ctr, 167),
        ("album-1.mp3", AesMode::Cbc, 0),
        ("album-1.flac", AesMode::CbcPkcs7, 5),
    ] {
        let harness = Harness::new(1000);
        let (cipher, decryption) = encrypted(name, mode, skip);
        harness.fetch.put(URL, cipher);
        let mut stream = harness
            .open(harness.http(Vec::new()), Some(decryption))
            .unwrap();
        let (whole, _) = read_all(&mut stream);
        assert_eq!(whole, local(name, None), "{name} {mode:?}");
        stream.seek(450).unwrap();
        let (sought, _) = read_all(&mut stream);
        assert_eq!(
            sought,
            local(name, Some(450)),
            "{name} {mode:?} after a seek"
        );
    }
}

/// A Byte Source over bytes in memory, logging reads.
struct MemoryByteSource {
    data: Vec<u8>,
    reads: Mutex<Vec<(u64, u32)>>,
}

#[async_trait]
impl ByteSourceReader for MemoryByteSource {
    async fn read(
        &self,
        handle: &ByteSourceHandle,
        offset: u64,
        length: u32,
    ) -> Result<Bytes, SourceError> {
        assert_eq!(handle.0, "track:42");
        self.reads.lock().unwrap().push((offset, length));
        let start = (offset as usize).min(self.data.len());
        let end = (start + length as usize).min(self.data.len());
        Ok(Bytes::copy_from_slice(&self.data[start..end]))
    }
}

#[test]
fn byte_sources_are_read_in_64_kib_chunks_and_seek() {
    let harness = Harness::new(4096);
    // Large enough for several chunks.
    let name = "album-1.flac";
    let source = Arc::new(MemoryByteSource {
        data: std::fs::read(fixture(name)).unwrap(),
        reads: Mutex::new(Vec::new()),
    });
    let location = Location::ByteSource {
        reader: source.clone(),
        handle: ByteSourceHandle("track:42".into()),
        length: Some(source.data.len() as u64),
    };
    let mut stream = harness.open(location, None).unwrap();
    let (whole, _) = read_all(&mut stream);
    assert_eq!(whole, local(name, None));
    stream.seek(900).unwrap();
    assert_eq!(read_all(&mut stream).0, local(name, Some(900)));
    assert!(
        source
            .reads
            .lock()
            .unwrap()
            .iter()
            .all(|&(_, length)| length == 64 << 10)
    );
}

#[test]
fn a_failing_byte_source_fails_the_stream() {
    struct Broken;
    #[async_trait]
    impl ByteSourceReader for Broken {
        async fn read(&self, _: &ByteSourceHandle, _: u64, _: u32) -> Result<Bytes, SourceError> {
            Err(SourceError::Failed("expired".into()))
        }
    }
    let harness = Harness::new(4096);
    let location = Location::ByteSource {
        reader: Arc::new(Broken),
        handle: ByteSourceHandle("x".into()),
        length: None,
    };
    match harness.open(location, None) {
        Err(Error::ByteSource(SourceError::Failed(reason))) => assert_eq!(reason, "expired"),
        other => panic!("expected the Byte Source's error: {:?}", other.map(|_| ())),
    }
    let shown = format!(
        "{:?}",
        ByteSourceHandle("https://cdn/x?token=secret".into())
    );
    assert!(!shown.contains("secret"), "{shown}");
}

#[test]
fn a_slow_server_makes_reads_pending_not_blocked() {
    let harness = Harness::new(8192);
    harness
        .fetch
        .put(URL, std::fs::read(fixture("album-1.flac")).unwrap());
    let gate = harness.fetch.gate(URL);
    // Opening needs a few chunks: the header and the first frames.
    gate.add_permits(4);
    let mut stream = harness.open(harness.http(Vec::new()), None).unwrap();
    let mut buffer = vec![0f32; 4096];
    let started = Instant::now();
    let mut saw_pending = false;
    for _ in 0..10_000 {
        match stream.read(&mut buffer).unwrap() {
            Read::Frames(_) => {}
            Read::Pending => {
                saw_pending = true;
                break;
            }
            Read::End => break,
        }
    }
    assert!(
        saw_pending,
        "the stream should starve while the server stalls"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "reads must not block"
    );
    gate.add_permits(1000);
    let (rest, _) = read_all(&mut stream);
    assert!(!rest.is_empty());
}

#[test]
fn a_missing_resource_fails_to_open() {
    let harness = Harness::new(4096);
    match harness.open(harness.http(Vec::new()), None) {
        Err(Error::Fetch {
            url,
            error: FetchError::Status { status: 404 },
        }) => assert_eq!(url, URL),
        other => panic!("expected a typed 404: {:?}", other.map(|_| ())),
    }
}
