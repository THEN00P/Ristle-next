//! Streams over a complete, randomly accessible container: local files (a
//! Folder Grant's `file` location) and anything else that implements
//! `Read + Seek`.

use std::fs::File;
use std::io::{Read as IoRead, Seek, SeekFrom};
use std::path::Path;

use symphonia::core::io::MediaSource;

use crate::demux::{MediaHint, SymphoniaDemuxer};
use crate::stream::DecodedStream;
use crate::{DecoderRegistry, Error, MediaStream, Read, StreamFormat};

/// A [`MediaStream`] decoding a container with random access.
pub struct ContainerStream {
    inner: DecodedStream<SymphoniaDemuxer>,
}

impl std::fmt::Debug for ContainerStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContainerStream")
            .field("decoder", &self.decoder())
            .field("format", &self.format())
            .finish_non_exhaustive()
    }
}

impl ContainerStream {
    /// Opens a local file. Without an extension in `hint`, the path's own is
    /// used as a probing hint; the bytes decide either way.
    pub fn open_file(
        path: &Path,
        hint: &MediaHint,
        registry: &DecoderRegistry,
    ) -> Result<Self, Error> {
        let mut hint = hint.clone();
        if hint.extension.is_none() {
            hint.extension = path
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase);
        }
        let file = File::open(path)?;
        Self::open_reader(file, &hint, registry)
    }

    /// Opens any seekable byte source, such as an in-memory buffer.
    pub fn open_reader<R>(
        reader: R,
        hint: &MediaHint,
        registry: &DecoderRegistry,
    ) -> Result<Self, Error>
    where
        R: IoRead + Seek + Send + Sync + 'static,
    {
        let demuxer = SymphoniaDemuxer::open(Source::new(reader)?, hint)?;
        Ok(Self {
            inner: DecodedStream::open(demuxer, registry)?,
        })
    }

    /// The name of the decoder provider in use, such as `"symphonia"`.
    pub fn decoder(&self) -> &'static str {
        self.inner.decoder_name()
    }
}

impl MediaStream for ContainerStream {
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

/// Adapts `Read + Seek` to Symphonia's source trait.
struct Source<R> {
    inner: R,
    len: u64,
}

impl<R: IoRead + Seek> Source<R> {
    fn new(mut inner: R) -> Result<Self, Error> {
        let len = inner.seek(SeekFrom::End(0))?;
        inner.seek(SeekFrom::Start(0))?;
        Ok(Self { inner, len })
    }
}

impl<R: IoRead> IoRead for Source<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl<R: Seek> Seek for Source<R> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

impl<R: IoRead + Seek + Send + Sync> MediaSource for Source<R> {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.len)
    }
}
