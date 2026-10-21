//! Symphonia's format readers behind [`Demuxer`].

use std::io::{Read, Seek, SeekFrom};

use symphonia::core::errors::{Error as SymphoniaError, SeekErrorKind};
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Timestamp;

use super::mp4;
use super::{Demuxed, DemuxedPacket, Demuxer, MediaHint, Seeked, TimeBase, Window};
use crate::Error;
use crate::codec::{Codec, CodecDescription};

/// A Symphonia format reader positioned on one audio track.
pub(crate) struct SymphoniaDemuxer {
    /// `None` only if reopening failed half-way.
    reader: Option<Box<dyn FormatReader>>,
    probe_hint: Hint,
    /// The reader returned its last packet. Symphonia 0.6.1's MP4 reader
    /// cannot seek after that ("no atom pending read"), so a seek from here
    /// probes the source again first.
    exhausted: bool,
    track_id: u32,
    codec: CodecDescription,
    time_base: TimeBase,
    window: Option<Window>,
}

impl SymphoniaDemuxer {
    /// Probes `source` and selects its audio track: the one the container
    /// marks as default, else the first audio track with a known codec.
    pub(crate) fn open<S>(mut source: S, hint: &MediaHint) -> Result<Self, Error>
    where
        S: MediaSource + Read + Seek + 'static,
    {
        // The MP4 edit list is read by the host (Symphonia ignores it), so the
        // moov is read before Symphonia takes the source.
        let mut head = [0u8; 8];
        let head_len = read_head(&mut source, &mut head)?;
        let moov = if source.is_seekable() && mp4::looks_like_mp4(&head[..head_len]) {
            let moov = mp4::read_moov(&mut source)?;
            source.seek(SeekFrom::Start(0))?;
            moov
        } else {
            None
        };

        let stream = MediaSourceStream::new(Box::new(source), MediaSourceStreamOptions::default());
        let mut probe_hint = Hint::new();
        if let Some(extension) = &hint.extension {
            probe_hint.with_extension(extension);
        }
        if let Some(essence) = hint.essence() {
            probe_hint.mime_type(&essence);
        }
        let reader = probe(&probe_hint, stream).map_err(|error| match error {
            Error::Io(io) => Error::Io(io),
            _ => Error::UnrecognizedContainer {
                hint: hint.mime.clone().or_else(|| hint.extension.clone()),
            },
        })?;

        let track = reader
            .default_track(TrackType::Audio)
            .ok_or(Error::NoAudioTrack)?;
        let mut codec = track
            .codec_params
            .as_ref()
            .and_then(CodecDescription::from_symphonia)
            .ok_or(Error::NoAudioTrack)?;
        if codec.codec == Codec::Other {
            return Err(Error::UnsupportedCodec(format!("{:?}", track.codec_params)));
        }
        codec.profile_hint = hint.codecs();
        let time_base = match track.time_base {
            Some(tb) => TimeBase {
                numer: tb.numer.get(),
                denom: tb.denom.get(),
            },
            None => TimeBase {
                numer: 1,
                denom: codec.sample_rate.ok_or(Error::NoAudioTrack)?,
            },
        };
        let track_id = track.id;
        let window = match &moov {
            Some(moov) => mp4::gapless_for_track(moov, track_id)?.map(|gapless| Window {
                start: i64::try_from(gapless.priming).unwrap_or(i64::MAX),
                length: gapless.length,
            }),
            None => None,
        };
        tracing::debug!(
            format = reader.format_info().short_name,
            track_id,
            codec = ?codec.codec,
            ?window,
            "opened container"
        );
        Ok(Self {
            reader: Some(reader),
            probe_hint,
            exhausted: false,
            track_id,
            codec,
            time_base,
            window,
        })
    }

    /// The selected track's id (the MP4 `tkhd` track_ID).
    pub(crate) fn track_id(&self) -> u32 {
        self.track_id
    }

    fn reader(&mut self) -> Result<&mut Box<dyn FormatReader>, Error> {
        self.reader.as_mut().ok_or_else(lost_reader)
    }

    /// Probes the source again from its start, keeping the selected track.
    fn reopen(&mut self) -> Result<(), Error> {
        let reader = self.reader.take().ok_or_else(lost_reader)?;
        let mut stream = reader.into_inner();
        stream.seek(SeekFrom::Start(0))?;
        let reader = probe(&self.probe_hint, stream)?;
        if !reader
            .tracks()
            .iter()
            .any(|track| track.id == self.track_id)
        {
            return Err(Error::malformed(
                "container",
                "the track changed on reopening",
            ));
        }
        self.reader = Some(reader);
        self.exhausted = false;
        Ok(())
    }
}

fn lost_reader() -> Error {
    Error::malformed("container", "reopening the source failed earlier")
}

fn probe(hint: &Hint, stream: MediaSourceStream<'static>) -> Result<Box<dyn FormatReader>, Error> {
    symphonia::default::get_probe()
        .probe(
            hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|error| match error {
            SymphoniaError::IoError(io) => Error::Io(io),
            other => Error::malformed("container", other.to_string()),
        })
}

fn read_head<S: Read + Seek>(source: &mut S, head: &mut [u8]) -> Result<usize, Error> {
    let mut filled = 0;
    while filled < head.len() {
        match source.read(&mut head[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }
    source.seek(SeekFrom::Start(0))?;
    Ok(filled)
}

impl Demuxer for SymphoniaDemuxer {
    fn codec(&self) -> &CodecDescription {
        &self.codec
    }

    fn time_base(&self) -> TimeBase {
        self.time_base
    }

    fn window(&self) -> Option<Window> {
        self.window
    }

    fn next_packet(&mut self) -> Result<Demuxed, Error> {
        if self.exhausted {
            return Ok(Demuxed::End);
        }
        let track_id = self.track_id;
        loop {
            match self.reader()?.next_packet() {
                Ok(Some(packet)) if packet.track_id == track_id => {
                    return Ok(Demuxed::Packet(DemuxedPacket {
                        pts: packet.pts.get(),
                        dur: packet.dur.get(),
                        trim_start: packet.trim_start.get(),
                        trim_end: packet.trim_end.get(),
                        data: packet.data,
                    }));
                }
                Ok(Some(_)) => {}
                Ok(None) => break,
                // Several readers end a file this way rather than with None.
                Err(SymphoniaError::IoError(error))
                    if error.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                Err(SymphoniaError::IoError(error)) => return Err(error.into()),
                // A chained Ogg stream changes its tracks: a new logical
                // stream is a different recording, so this one ends here.
                Err(SymphoniaError::ResetRequired) => {
                    tracing::info!("the container changed its tracks; ending the stream");
                    break;
                }
                Err(error) => return Err(Error::malformed("container", error.to_string())),
            }
        }
        self.exhausted = true;
        Ok(Demuxed::End)
    }

    fn seek(&mut self, ts: i64) -> Result<Seeked, Error> {
        if self.exhausted {
            self.reopen()?;
        }
        let to = SeekTo::Timestamp {
            ts: Timestamp::new(ts),
            track_id: self.track_id,
        };
        match self.reader()?.seek(SeekMode::Accurate, to) {
            Ok(_) => Ok(Seeked::At),
            Err(SymphoniaError::SeekError(SeekErrorKind::OutOfRange)) => Ok(Seeked::PastEnd),
            Err(SymphoniaError::SeekError(
                SeekErrorKind::Unseekable | SeekErrorKind::ForwardOnly,
            )) => Err(Error::NotSeekable),
            Err(SymphoniaError::IoError(error)) => Err(error.into()),
            Err(error) => Err(Error::malformed("container seek", error.to_string())),
        }
    }
}
