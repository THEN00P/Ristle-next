//! Demux with Symphonia, decode through the registry, collect interleaved f32.

use crate::registry::{DecoderRegistry, StreamInfo, Support};
use anyhow::{anyhow, Context, Result};
use std::io::Cursor;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;

pub struct Decoded {
    pub samples: Vec<f32>,
    pub rate: u32,
    pub channels: usize,
    pub decoder: &'static str,
    pub support: Support,
    pub format: &'static str,
    /// Frames trimmed by the host from an MP4 edit list (start, end).
    pub host_trim: Option<(u64, u64)>,
    /// Encoder delay and padding the demuxer found (LAME tag for MP3).
    pub track_delay: Option<u32>,
    pub track_padding: Option<u32>,
}

impl Decoded {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1)
    }
    pub fn seconds(&self) -> f64 {
        self.frames() as f64 / self.rate as f64
    }
}

pub fn open(source: Box<dyn MediaSource>, ext: Option<&str>) -> Result<Box<dyn FormatReader>> {
    let mss = MediaSourceStream::new(source, Default::default());
    let mut hint = Hint::new();
    if let Some(e) = ext {
        hint.with_extension(e);
    }
    Ok(symphonia::default::get_probe().probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())?)
}

pub fn audio_stream(format: &dyn FormatReader, profile_hint: Option<String>) -> Result<(u32, StreamInfo)> {
    let track = format.default_track(TrackType::Audio).ok_or_else(|| anyhow!("no audio track"))?;
    let Some(CodecParameters::Audio(params)) = &track.codec_params else {
        return Err(anyhow!("track has no audio codec parameters"));
    };
    Ok((track.id, StreamInfo { params: params.clone(), profile_hint }))
}

/// Decodes a whole file held in memory. `gapless_mp4` applies the MP4 edit
/// list in the host, because Symphonia's MP4 reader parses `elst` but never
/// turns it into packet trims.
pub fn decode_bytes(reg: &DecoderRegistry, bytes: &[u8], ext: &str, gapless_mp4: bool) -> Result<Decoded> {
    let mut format = open(Box::new(Cursor::new(bytes.to_vec())), Some(ext))?;
    let (track_id, stream) = audio_stream(format.as_ref(), None)?;
    let track = format.tracks().iter().find(|t| t.id == track_id).unwrap();
    let (track_delay, track_padding) = (track.delay, track.padding);
    let (mut dec, name, support) = reg.create(&stream).context("creating decoder")?;
    let mut samples = Vec::with_capacity(1 << 20);
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => break,
            Err(e) => return Err(e.into()),
        };
        if packet.track_id != track_id {
            continue;
        }
        match dec.decode(&packet, &mut samples) {
            Ok(()) => {}
            Err(e) if e.downcast_ref::<SymError>().is_some_and(|e| matches!(e, SymError::DecodeError(_))) => {
                eprintln!("decode error, packet skipped: {e}");
            }
            Err(e) => return Err(e),
        }
    }
    let (rate, channels) = dec.output();
    let mut host_trim = None;
    if gapless_mp4 && matches!(ext, "m4a" | "mp4") {
        if let Some(edit) = crate::mp4edit::read_edit(bytes) {
            let frames = samples.len() / channels;
            let start = edit.media_time.min(frames as u64);
            let keep = edit.duration_frames.min(frames as u64 - start);
            samples.drain(..start as usize * channels);
            samples.truncate(keep as usize * channels);
            host_trim = Some((start, frames as u64 - start - keep));
        }
    }
    Ok(Decoded {
        samples,
        rate,
        channels,
        decoder: name,
        support,
        format: format.format_info().short_name,
        host_trim,
        track_delay,
        track_padding,
    })
}
