//! Progressive Stream Sources: a whole resource over HTTP with Range seeks,
//! or a Plugin Byte Source, optionally decrypted by the host (ADR 0004
//! variants 1, 3 and 4).

use std::sync::Arc;

use tokio::runtime::Handle;
use tokio::task::JoinHandle;

use crate::codec::CodecDescription;
use crate::demux::{
    Demuxed, Demuxer, MediaHint, Seeked, SymphoniaDemuxer, TimeBase, TsDemuxer, TsFileDemuxer,
    Window,
};
use crate::source::decrypt::Decryption;
use crate::source::{
    ByteSourceFeed, ByteSourceHandle, ByteSourceReader, Feed, HttpFeed, Remote, RemoteSource,
    WATERMARK,
};
use crate::stream::DecodedStream;
use crate::{DecoderRegistry, Error, Fetch, MediaStream, Read, StreamFormat};

/// Where a progressive resource's bytes come from.
#[derive(Clone)]
pub enum Location {
    /// HTTP GET with Range requests (WIT `progressive`). The headers go only
    /// to the URL's origin; following redirects within it is the HTTP
    /// client's job.
    Http {
        /// The resource.
        url: String,
        /// Sent with every request.
        headers: Vec<(String, String)>,
    },
    /// A Plugin Byte Source (WIT `plugin-bytes`).
    ByteSource {
        /// Reads through the Plugin host.
        reader: Arc<dyn ByteSourceReader>,
        /// The Plugin's handle.
        handle: ByteSourceHandle,
        /// The resource's length, if the Plugin knows it.
        length: Option<u64>,
    },
}

impl std::fmt::Debug for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Header values may be credentials.
            Self::Http { url, headers } => f
                .debug_struct("Http")
                .field("url", &crate::fetch::redact_url(url))
                .field(
                    "headers",
                    &headers.iter().map(|(name, _)| name).collect::<Vec<_>>(),
                )
                .finish(),
            // The handle's Debug shows only its length.
            Self::ByteSource { handle, length, .. } => f
                .debug_struct("ByteSource")
                .field("handle", handle)
                .field("length", length)
                .finish_non_exhaustive(),
        }
    }
}

/// A progressive Stream Source.
#[derive(Clone, Debug)]
pub struct ProgressiveSource {
    /// Where the bytes come from.
    pub location: Location,
    /// The Plugin's MIME hint.
    pub hint: MediaHint,
    /// Host-side decryption, if the resource is encrypted.
    pub decrypt: Option<Decryption>,
}

/// A [`MediaStream`] over a progressive resource.
pub struct ProgressiveStream {
    inner: DecodedStream<ProgressiveDemuxer>,
    _fetching: FetchGuard,
}

/// Stops the fetch task when dropped: when the stream goes away, and when
/// `open` fails or its future is dropped before the stream exists.
struct FetchGuard {
    remote: Arc<Remote>,
    task: JoinHandle<()>,
}

impl Drop for FetchGuard {
    fn drop(&mut self) {
        self.remote.close();
        self.task.abort();
    }
}

impl std::fmt::Debug for ProgressiveStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProgressiveStream")
            .field("decoder", &self.inner.decoder_name())
            .finish_non_exhaustive()
    }
}

impl ProgressiveStream {
    /// Starts fetching, probes the container and decodes the first frames.
    /// Fetching continues in the background on `runtime`.
    pub async fn open(
        source: ProgressiveSource,
        fetch: Arc<dyn Fetch>,
        runtime: Handle,
        registry: &DecoderRegistry,
    ) -> Result<Self, Error> {
        if let Some(decryption) = &source.decrypt {
            decryption.validate()?;
        }
        let feed: Arc<dyn Feed> = match source.location {
            Location::Http { url, headers } => Arc::new(HttpFeed {
                fetch,
                url,
                headers,
            }),
            Location::ByteSource {
                reader,
                handle,
                length,
            } => Arc::new(ByteSourceFeed {
                reader,
                handle,
                length,
            }),
        };
        let remote = Remote::new();
        let skip = source.decrypt.as_ref().map_or(0, |d| u64::from(d.skip));
        let task = runtime.spawn(remote.clone().run(feed, source.decrypt.clone()));
        let guard = FetchGuard {
            remote: remote.clone(),
            task,
        };
        let registry = registry.clone();
        let hint = source.hint;
        let probing = remote.clone();
        // Probing reads wherever the container says (an MP4 moov at the
        // end), so it runs where blocking is allowed.
        let opened = runtime
            .spawn_blocking(
                move || -> Result<DecodedStream<ProgressiveDemuxer>, Error> {
                    let mut reader = RemoteSource::new(probing.clone(), skip);
                    let mut head = [0u8; 189];
                    let head_len = read_up_to(&mut reader, &mut head)?;
                    std::io::Seek::seek(&mut reader, std::io::SeekFrom::Start(0))?;
                    let inner: Box<dyn Demuxer> = if TsDemuxer::sniff(&head[..head_len]) {
                        Box::new(TsFileDemuxer::open(reader)?)
                    } else {
                        Box::new(SymphoniaDemuxer::open(reader, &hint)?)
                    };
                    DecodedStream::open(
                        ProgressiveDemuxer {
                            inner,
                            remote: probing,
                        },
                        &registry,
                    )
                },
            )
            .await;
        let inner = match opened {
            Ok(Ok(inner)) => inner,
            // Dropping the guard stops the fetch.
            Ok(Err(error)) => return Err(remote.take_error().unwrap_or(error)),
            Err(join) => return Err(Error::malformed("progressive stream", join.to_string())),
        };
        Ok(Self {
            inner,
            _fetching: guard,
        })
    }

    /// The name of the decoder provider in use.
    pub fn decoder(&self) -> &'static str {
        self.inner.decoder_name()
    }
}

fn read_up_to<R: std::io::Read>(reader: &mut R, buf: &mut [u8]) -> Result<usize, Error> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(filled)
}

impl MediaStream for ProgressiveStream {
    fn format(&self) -> StreamFormat {
        self.inner.format()
    }

    fn read(&mut self, out: &mut [f32]) -> Result<Read, Error> {
        self.inner.read(out)
    }

    fn seek(&mut self, position_ms: u64) -> Result<(), Error> {
        self.inner.seek(position_ms)
    }
}

/// Demuxes only once enough bytes are buffered, so the decode thread does
/// not block on the network.
pub(crate) struct ProgressiveDemuxer {
    inner: Box<dyn Demuxer>,
    remote: Arc<Remote>,
}

impl Demuxer for ProgressiveDemuxer {
    fn codec(&self) -> &CodecDescription {
        self.inner.codec()
    }

    fn time_base(&self) -> TimeBase {
        self.inner.time_base()
    }

    fn window(&self) -> Option<Window> {
        self.inner.window()
    }

    fn next_packet(&mut self) -> Result<Demuxed, Error> {
        if !self.remote.ready(WATERMARK) {
            return Ok(Demuxed::Pending);
        }
        // The container only saw an I/O error; report the fetch's own.
        self.inner
            .next_packet()
            .map_err(|error| self.remote.take_error().unwrap_or(error))
    }

    fn seek(&mut self, ts: i64) -> Result<Seeked, Error> {
        self.inner
            .seek(ts)
            .map_err(|error| self.remote.take_error().unwrap_or(error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_hides_url_tokens_and_header_values() {
        let location = Location::Http {
            url: "https://user:pw@cdn.example/audio.ogg?sig=SECRET&exp=1".into(),
            headers: vec![("Authorization".into(), "Bearer TOKEN".into())],
        };
        let shown = format!("{location:?}");
        assert!(shown.contains("cdn.example/audio.ogg"), "{shown}");
        for secret in ["SECRET", "TOKEN", "pw@"] {
            assert!(!shown.contains(secret), "{shown}");
        }
    }
}
