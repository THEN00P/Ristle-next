//! HLS playlists (RFC 8216): master playlists and the variant choice, and
//! media playlists with keys, maps, byte ranges and discontinuities.

use std::sync::Arc;

use url::Url;

use crate::Error;
use crate::fetch::ByteRange;

/// `EXT-X-KEY` with `METHOD=AES-128` and the identity key format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Key {
    pub(crate) url: Url,
    /// The explicit IV; without one, a segment's media sequence number is
    /// its IV.
    pub(crate) iv: Option<[u8; 16]>,
}

/// `EXT-X-MAP`: the fMP4 initialization section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Map {
    pub(crate) url: Url,
    pub(crate) range: Option<ByteRange>,
    /// The key in effect where the map appears: the init section is
    /// encrypted with it (ADR 0004, "AES-128 covers `EXT-X-MAP`").
    pub(crate) key: Option<Key>,
    /// The media sequence number of the first segment the map applies to,
    /// the IV when the key has none.
    pub(crate) sequence: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Segment {
    pub(crate) url: Url,
    pub(crate) range: Option<ByteRange>,
    /// Duration from `EXTINF`, in microseconds.
    pub(crate) duration_us: u64,
    /// Start within the playlist, in microseconds from its first segment.
    pub(crate) start_us: u64,
    pub(crate) sequence: u64,
    pub(crate) key: Option<Key>,
    pub(crate) map: Option<Arc<Map>>,
    /// `EXT-X-DISCONTINUITY` precedes the segment.
    pub(crate) discontinuity: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MediaPlaylist {
    pub(crate) url: Url,
    pub(crate) target_duration_us: u64,
    pub(crate) segments: Vec<Segment>,
    /// `EXT-X-ENDLIST` or `EXT-X-PLAYLIST-TYPE:VOD`: no segments will be
    /// added, so the playlist is seekable and has an end.
    pub(crate) ended: bool,
}

impl MediaPlaylist {
    pub(crate) fn duration_us(&self) -> u64 {
        self.segments
            .last()
            .map_or(0, |s| s.start_us + s.duration_us)
    }

    /// The segment that plays at `position_ms`, and its start in ms. Past
    /// the end, the last segment.
    pub(crate) fn segment_index_at(&self, position_ms: u64) -> (usize, u64) {
        let position_us = position_ms.saturating_mul(1000);
        let index = self
            .segments
            .iter()
            .rposition(|s| s.start_us <= position_us)
            .unwrap_or(0);
        (
            index,
            self.segments.get(index).map_or(0, |s| s.start_us / 1000),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Variant {
    pub(crate) url: Url,
    pub(crate) bandwidth: u64,
    pub(crate) codecs: Option<String>,
    pub(crate) audio_group: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Rendition {
    pub(crate) group: String,
    pub(crate) url: Option<Url>,
    pub(crate) default: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MasterPlaylist {
    pub(crate) variants: Vec<Variant>,
    pub(crate) audio: Vec<Rendition>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Playlist {
    Master(MasterPlaylist),
    Media(MediaPlaylist),
}

/// The media playlist to play from a master playlist, and the audio codec
/// its `CODECS` names (the only place HE-AAC announces itself before
/// decoding).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Choice {
    pub(crate) url: Url,
    pub(crate) codecs: Option<String>,
}

pub(crate) fn parse(text: &str, url: &Url) -> Result<Playlist, Error> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if !text.trim_start().starts_with("#EXTM3U") {
        return Err(malformed("not an M3U8 playlist"));
    }
    if text
        .lines()
        .any(|line| line.trim_start().starts_with("#EXT-X-STREAM-INF"))
    {
        parse_master(text, url).map(Playlist::Master)
    } else {
        parse_media(text, url).map(Playlist::Media)
    }
}

fn malformed(detail: impl Into<String>) -> Error {
    Error::malformed("HLS playlist", detail)
}

fn resolve(base: &Url, reference: &str) -> Result<Url, Error> {
    base.join(reference)
        .map_err(|error| malformed(format!("URI {reference:?}: {error}")))
}

/// An attribute's value from an attribute list, unquoted. Names compare
/// exactly, as RFC 8216 makes them case-sensitive.
pub(crate) fn attribute_value(attributes: &str, name: &str) -> Option<String> {
    let mut rest = attributes;
    loop {
        let rest_trimmed = rest.trim_start_matches([',', ' ', '\t']);
        let (key, after) = rest_trimmed.split_once('=')?;
        let (value, remaining) = match after.strip_prefix('"') {
            Some(quoted) => {
                let end = quoted.find('"')?;
                (&quoted[..end], &quoted[end + 1..])
            }
            None => {
                let end = after.find(',').unwrap_or(after.len());
                (&after[..end], &after[end..])
            }
        };
        if key.trim() == name {
            return Some(value.to_owned());
        }
        rest = remaining;
    }
}

fn parse_iv(value: &str) -> Result<[u8; 16], Error> {
    let hex = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .ok_or_else(|| malformed(format!("IV {value:?} is not hexadecimal")))?;
    if hex.len() != 32 {
        return Err(malformed(format!("IV {value:?} is not 16 bytes")));
    }
    let mut iv = [0u8; 16];
    for (i, byte) in iv.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| malformed(format!("IV {value:?} is not hexadecimal")))?;
    }
    Ok(iv)
}

/// The IV of a segment without an explicit one: its media sequence number,
/// big-endian, in 16 bytes.
pub(crate) fn sequence_iv(sequence: u64) -> [u8; 16] {
    u128::from(sequence).to_be_bytes()
}

/// `n[@o]`, as `EXT-X-BYTERANGE` and the map's `BYTERANGE` write it.
fn parse_range(value: &str) -> Result<(u64, Option<u64>), Error> {
    let bad = || malformed(format!("byte range {value:?}"));
    let value = value.trim().trim_matches('"');
    match value.split_once('@') {
        Some((length, offset)) => Ok((
            length.parse().map_err(|_| bad())?,
            Some(offset.parse().map_err(|_| bad())?),
        )),
        None => Ok((value.parse().map_err(|_| bad())?, None)),
    }
}

/// The key one `EXT-X-KEY` tag describes: `Ok(None)` for `METHOD=NONE`.
fn parse_key(attributes: &str, url: &Url) -> Result<KeyTag, Error> {
    let method = attribute_value(attributes, "METHOD").unwrap_or_else(|| "unknown".into());
    if method == "NONE" {
        return Ok(KeyTag::None);
    }
    let format = attribute_value(attributes, "KEYFORMAT").unwrap_or_else(|| "identity".into());
    if method == "AES-128" && format == "identity" {
        let uri = attribute_value(attributes, "URI")
            .ok_or_else(|| malformed("an AES-128 key without a URI"))?;
        let iv = attribute_value(attributes, "IV")
            .map(|iv| parse_iv(&iv))
            .transpose()?;
        return Ok(KeyTag::Aes128(Key {
            url: resolve(url, &uri)?,
            iv,
        }));
    }
    let system = match format.to_ascii_lowercase().as_str() {
        "urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed" => "Widevine DRM",
        "com.microsoft.playready" => "PlayReady DRM",
        "com.apple.streamingkeydelivery" => "FairPlay DRM",
        _ => "encrypted HLS",
    };
    Ok(KeyTag::Unsupported(format!("{system} ({method})")))
}

enum KeyTag {
    None,
    Aes128(Key),
    Unsupported(String),
}

fn parse_media(text: &str, url: &Url) -> Result<MediaPlaylist, Error> {
    let mut segments = Vec::new();
    let mut target_duration_us = 0;
    let mut ended = false;
    let mut sequence = 0u64;
    let mut duration_us: Option<u64> = None;
    let mut start_us = 0u64;
    let mut key: Option<Key> = None;
    // Consecutive EXT-X-KEY tags offer the same segments in several key
    // systems. The identity one wins; DRM-only fails.
    let mut key_group: Vec<KeyTag> = Vec::new();
    let mut map: Option<Arc<Map>> = None;
    let mut map_pending: Option<(Url, Option<ByteRange>, Option<Key>)> = None;
    let mut range: Option<(u64, Option<u64>)> = None;
    let mut discontinuity = false;
    // Where the previous byte range of each URI ended, for ranges without
    // an offset.
    let mut range_ends: std::collections::HashMap<Url, u64> = std::collections::HashMap::new();

    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if !line.starts_with('#') {
            if !key_group.is_empty() {
                key = resolve_key_group(std::mem::take(&mut key_group))?;
            }
            let segment_url = resolve(url, line)?;
            if let Some((map_url, map_range, map_key)) = map_pending.take() {
                map = Some(Arc::new(Map {
                    url: map_url,
                    range: map_range,
                    key: map_key,
                    sequence,
                }));
            }
            let range = match range.take() {
                Some((length, offset)) => {
                    let start = match offset {
                        Some(offset) => offset,
                        None => *range_ends.get(&segment_url).ok_or_else(|| {
                            malformed("a byte range without an offset after none")
                        })?,
                    };
                    range_ends.insert(segment_url.clone(), start + length);
                    Some(ByteRange {
                        start,
                        length: Some(length),
                    })
                }
                None => None,
            };
            let duration = duration_us
                .take()
                .ok_or_else(|| malformed(format!("segment {line:?} has no EXTINF")))?;
            segments.push(Segment {
                url: segment_url,
                range,
                duration_us: duration,
                start_us,
                sequence,
                key: key.clone(),
                map: map.clone(),
                discontinuity: std::mem::take(&mut discontinuity),
            });
            start_us += duration;
            sequence += 1;
            continue;
        }
        let (tag, value) = line.split_once(':').unwrap_or((line, ""));
        match tag {
            "#EXTINF" => {
                let seconds = value.split(',').next().unwrap_or("").trim();
                let seconds: f64 = seconds
                    .parse()
                    .map_err(|_| malformed(format!("EXTINF {value:?}")))?;
                if !(seconds.is_finite() && seconds >= 0.0) {
                    return Err(malformed(format!("EXTINF {value:?}")));
                }
                duration_us = Some((seconds * 1_000_000.0).round() as u64);
            }
            "#EXT-X-TARGETDURATION" => {
                let seconds: u64 = value
                    .trim()
                    .parse()
                    .map_err(|_| malformed(format!("target duration {value:?}")))?;
                target_duration_us = seconds * 1_000_000;
            }
            "#EXT-X-MEDIA-SEQUENCE" => {
                sequence = value
                    .trim()
                    .parse()
                    .map_err(|_| malformed(format!("media sequence {value:?}")))?;
            }
            "#EXT-X-KEY" => key_group.push(parse_key(value, url)?),
            "#EXT-X-MAP" => {
                let uri = attribute_value(value, "URI")
                    .ok_or_else(|| malformed("EXT-X-MAP without a URI"))?;
                let map_range = attribute_value(value, "BYTERANGE")
                    .map(|r| parse_range(&r))
                    .transpose()?
                    .map(|(length, offset)| ByteRange {
                        start: offset.unwrap_or(0),
                        length: Some(length),
                    });
                // A key tag right before the map applies to it too.
                if !key_group.is_empty() {
                    key = resolve_key_group(std::mem::take(&mut key_group))?;
                }
                map_pending = Some((resolve(url, &uri)?, map_range, key.clone()));
            }
            "#EXT-X-BYTERANGE" => range = Some(parse_range(value)?),
            "#EXT-X-DISCONTINUITY" => discontinuity = true,
            "#EXT-X-ENDLIST" => ended = true,
            "#EXT-X-PLAYLIST-TYPE" if value.trim() == "VOD" => ended = true,
            _ => {}
        }
    }
    if segments.is_empty() && ended {
        return Err(malformed("the playlist has no media segments"));
    }
    Ok(MediaPlaylist {
        url: url.clone(),
        target_duration_us,
        segments,
        ended,
    })
}

fn resolve_key_group(group: Vec<KeyTag>) -> Result<Option<Key>, Error> {
    let mut unsupported = None;
    let mut none = false;
    for tag in group {
        match tag {
            KeyTag::Aes128(key) => return Ok(Some(key)),
            KeyTag::None => none = true,
            KeyTag::Unsupported(scheme) => unsupported = Some(scheme),
        }
    }
    match unsupported {
        Some(scheme) if !none => Err(Error::UnsupportedEncryption { scheme }),
        _ => Ok(None),
    }
}

fn parse_master(text: &str, url: &Url) -> Result<MasterPlaylist, Error> {
    let mut variants = Vec::new();
    let mut audio = Vec::new();
    let mut pending: Option<String> = None;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(attributes) = line.strip_prefix("#EXT-X-MEDIA:") {
            if attribute_value(attributes, "TYPE").as_deref() == Some("AUDIO") {
                let Some(group) = attribute_value(attributes, "GROUP-ID") else {
                    continue;
                };
                audio.push(Rendition {
                    group,
                    url: attribute_value(attributes, "URI")
                        .map(|u| resolve(url, &u))
                        .transpose()?,
                    default: attribute_value(attributes, "DEFAULT").as_deref() == Some("YES"),
                });
            }
        } else if let Some(attributes) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            pending = Some(attributes.to_owned());
        } else if !line.starts_with('#')
            && let Some(attributes) = pending.take()
        {
            variants.push(Variant {
                url: resolve(url, line)?,
                bandwidth: attribute_value(&attributes, "BANDWIDTH")
                    .and_then(|b| b.parse().ok())
                    .unwrap_or(0),
                codecs: attribute_value(&attributes, "CODECS"),
                audio_group: attribute_value(&attributes, "AUDIO"),
            });
        }
    }
    if variants.is_empty() {
        return Err(malformed("a master playlist without variants"));
    }
    Ok(MasterPlaylist { variants, audio })
}

/// Classifies one RFC 6381 codec.
fn is_audio_codec(codec: &str) -> bool {
    let codec = codec.trim().to_ascii_lowercase();
    [
        "mp4a.", "opus", "flac", "alac", "ac-3", "ec-3", "ac-4", "mp3", "vorbis",
    ]
    .iter()
    .any(|prefix| codec.starts_with(prefix))
}

/// Audio codecs no provider decodes.
fn is_undecodable_audio(codec: &str) -> bool {
    let codec = codec.trim().to_ascii_lowercase();
    ["ac-3", "ec-3", "ac-4", "mp4a.a5", "mp4a.a6"]
        .iter()
        .any(|c| codec.starts_with(c))
}

fn audio_codecs(codecs: &str) -> Vec<&str> {
    codecs
        .split(',')
        .map(str::trim)
        .filter(|c| is_audio_codec(c))
        .collect()
}

fn has_video(codecs: &str) -> bool {
    codecs
        .split(',')
        .map(str::trim)
        .any(|c| !c.is_empty() && !is_audio_codec(c))
}

/// Chooses the media playlist to play:
///
/// 1. An audio rendition (`EXT-X-MEDIA TYPE=AUDIO` with a URI) of the first
///    variant that names a group: the `DEFAULT=YES` one, else the first.
/// 2. Else the audio-only variant with the highest bandwidth.
/// 3. Else the variant with the lowest bandwidth, which carries the least
///    video.
///
/// Variants whose only audio codecs no provider decodes (AC-3, E-AC-3) are
/// passed over while others exist.
pub(crate) fn choose(master: &MasterPlaylist) -> Result<Choice, Error> {
    let decodable = |variant: &&Variant| {
        variant.codecs.as_deref().is_none_or(|codecs| {
            let audio = audio_codecs(codecs);
            audio.is_empty() || audio.iter().any(|c| !is_undecodable_audio(c))
        })
    };
    let mut candidates: Vec<&Variant> = master.variants.iter().filter(decodable).collect();
    if candidates.is_empty() {
        let named = master
            .variants
            .iter()
            .filter_map(|v| v.codecs.clone())
            .collect::<Vec<_>>();
        return Err(Error::UnsupportedCodec(named.join("; ")));
    }
    let audio_codec = |variant: &Variant| {
        variant.codecs.as_deref().and_then(|codecs| {
            audio_codecs(codecs)
                .into_iter()
                .find(|c| !is_undecodable_audio(c))
                .map(str::to_owned)
        })
    };

    for variant in &candidates {
        let Some(group) = &variant.audio_group else {
            continue;
        };
        let in_group = master
            .audio
            .iter()
            .filter(|r| &r.group == group && r.url.is_some());
        let rendition = in_group
            .clone()
            .find(|r| r.default)
            .or_else(|| in_group.clone().next());
        if let Some(Rendition { url: Some(url), .. }) = rendition {
            return Ok(Choice {
                url: url.clone(),
                codecs: audio_codec(variant),
            });
        }
    }

    let audio_only = candidates
        .iter()
        .filter(|v| v.codecs.as_deref().is_some_and(|c| !has_video(c)))
        .max_by_key(|v| v.bandwidth);
    if let Some(variant) = audio_only {
        return Ok(Choice {
            url: variant.url.clone(),
            codecs: audio_codec(variant),
        });
    }
    candidates.sort_by_key(|v| v.bandwidth);
    let variant = candidates[0];
    Ok(Choice {
        url: variant.url.clone(),
        codecs: audio_codec(variant),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYLIST_URL: &str = "https://cdn.example.com/audio/media.m3u8";

    fn media(text: &str) -> Result<MediaPlaylist, Error> {
        match parse(text, &Url::parse(PLAYLIST_URL).unwrap())? {
            Playlist::Media(media) => Ok(media),
            Playlist::Master(_) => panic!("expected a media playlist"),
        }
    }

    fn master(text: &str) -> MasterPlaylist {
        match parse(
            text,
            &Url::parse("https://cdn.example.com/master.m3u8").unwrap(),
        )
        .unwrap()
        {
            Playlist::Master(master) => master,
            Playlist::Media(_) => panic!("expected a master playlist"),
        }
    }

    #[test]
    fn parses_media_playlists_with_map_and_endlist() {
        let text = "#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:9.6,\nseg0.m4s\n#EXTINF:10.0,\nseg1.m4s\n#EXT-X-ENDLIST\n";
        let playlist = media(text).unwrap();
        let map = playlist.segments[0].map.as_ref().unwrap();
        assert_eq!(map.url.as_str(), "https://cdn.example.com/audio/init.mp4");
        assert_eq!(playlist.segments.len(), 2);
        assert_eq!(playlist.segments[0].duration_us, 9_600_000);
        assert!(playlist.segments[0].key.is_none());
        assert!(playlist.ended);
        assert_eq!(playlist.segment_index_at(0), (0, 0));
        assert_eq!(playlist.segment_index_at(9700), (1, 9600));
        assert_eq!(playlist.segment_index_at(60_000), (1, 9600));
    }

    #[test]
    fn parses_aes_128_keys_with_sequence_and_explicit_ivs() {
        let text = "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:7\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n#EXTINF:4.0,\nseg7.ts\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\",IV=0x000102030405060708090a0b0c0d0e0f\n#EXTINF:4.0,\nseg8.ts\n#EXT-X-KEY:METHOD=NONE\n#EXTINF:4.0,\nseg9.ts\n#EXT-X-ENDLIST\n";
        let playlist = media(text).unwrap();
        let first = playlist.segments[0].key.as_ref().unwrap();
        assert_eq!(first.url.as_str(), "https://cdn.example.com/audio/key.bin");
        assert_eq!(first.iv, None);
        assert_eq!(playlist.segments[0].sequence, 7);
        assert_eq!(sequence_iv(7)[15], 7);
        let second = playlist.segments[1].key.as_ref().unwrap();
        assert_eq!(
            second.iv,
            Some([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15])
        );
        assert!(playlist.segments[2].key.is_none());
    }

    #[test]
    fn identifies_widevine_hls_before_decoding_ciphertext() {
        let text = "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,KEYFORMAT=\"urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed\",URI=\"data:text/plain;base64,AA==\"\n#EXTINF:4.0,\nseg0.ts\n#EXT-X-ENDLIST\n";
        match media(text) {
            Err(Error::UnsupportedEncryption { scheme }) => {
                assert_eq!(scheme, "Widevine DRM (SAMPLE-AES)");
            }
            other => panic!("DRM playlist should be rejected: {other:?}"),
        }
    }

    #[test]
    fn an_identity_key_next_to_a_drm_key_is_used() {
        let text = "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,KEYFORMAT=\"com.apple.streamingkeydelivery\",URI=\"skd://x\"\n#EXT-X-KEY:METHOD=AES-128,URI=\"k\"\n#EXTINF:4,\ns.ts\n#EXT-X-ENDLIST\n";
        let playlist = media(text).unwrap();
        assert!(playlist.segments[0].key.is_some());
    }

    #[test]
    fn extracts_quoted_and_bare_attributes() {
        let attributes = "BANDWIDTH=128000,CODECS=\"mp4a.40.2\",URI=\"chunk.m3u8\"";
        assert_eq!(
            attribute_value(attributes, "BANDWIDTH").as_deref(),
            Some("128000")
        );
        assert_eq!(
            attribute_value(attributes, "URI").as_deref(),
            Some("chunk.m3u8")
        );
        assert_eq!(
            attribute_value(attributes, "CODECS").as_deref(),
            Some("mp4a.40.2")
        );
        assert_eq!(attribute_value(attributes, "MISSING"), None);
        // A quoted value may hold commas and equals signs.
        let tricky = "CODECS=\"avc1.4d401f,mp4a.40.5\",NAME=\"a=b\",X=1";
        assert_eq!(
            attribute_value(tricky, "CODECS").as_deref(),
            Some("avc1.4d401f,mp4a.40.5")
        );
        assert_eq!(attribute_value(tricky, "X").as_deref(), Some("1"));
    }

    #[test]
    fn byte_ranges_continue_from_the_previous_range_of_the_same_file() {
        let text = "#EXTM3U\n#EXT-X-MAP:URI=\"all.mp4\",BYTERANGE=\"700@0\"\n#EXTINF:4,\n#EXT-X-BYTERANGE:1000@700\nall.mp4\n#EXTINF:4,\n#EXT-X-BYTERANGE:1200\nall.mp4\n#EXT-X-ENDLIST\n";
        let playlist = media(text).unwrap();
        let map = playlist.segments[0].map.as_ref().unwrap();
        assert_eq!(
            map.range,
            Some(ByteRange {
                start: 0,
                length: Some(700)
            })
        );
        assert_eq!(
            playlist.segments[0].range,
            Some(ByteRange {
                start: 700,
                length: Some(1000)
            })
        );
        assert_eq!(
            playlist.segments[1].range,
            Some(ByteRange {
                start: 1700,
                length: Some(1200)
            })
        );
    }

    #[test]
    fn a_key_before_the_map_encrypts_the_init_section() {
        let text = "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:3\n#EXT-X-KEY:METHOD=AES-128,URI=\"k\",IV=0x0000000000000000000000000000000a\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4,\ns3.m4s\n#EXT-X-ENDLIST\n";
        let playlist = media(text).unwrap();
        let map = playlist.segments[0].map.as_ref().unwrap();
        assert_eq!(
            map.key.as_ref().and_then(|k| k.iv).map(|iv| iv[15]),
            Some(10)
        );
        assert_eq!(map.sequence, 3);
        // After the map, it is not.
        let text = "#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXT-X-KEY:METHOD=AES-128,URI=\"k\"\n#EXTINF:4,\ns.m4s\n#EXT-X-ENDLIST\n";
        let playlist = media(text).unwrap();
        assert!(playlist.segments[0].map.as_ref().unwrap().key.is_none());
        assert!(playlist.segments[0].key.is_some());
    }

    #[test]
    fn live_playlists_have_no_end_and_mark_discontinuities() {
        let text = "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXT-X-MEDIA-SEQUENCE:100\n#EXTINF:6,\na.ts\n#EXT-X-DISCONTINUITY\n#EXTINF:6,\nb.ts\n";
        let playlist = media(text).unwrap();
        assert!(!playlist.ended);
        assert_eq!(playlist.target_duration_us, 6_000_000);
        assert_eq!(playlist.segments[1].sequence, 101);
        assert!(playlist.segments[1].discontinuity && !playlist.segments[0].discontinuity);
        assert_eq!(playlist.duration_us(), 12_000_000);
    }

    #[test]
    fn malformed_playlists_are_rejected() {
        assert!(media("not a playlist").is_err());
        assert!(media("#EXTM3U\nseg.ts\n#EXT-X-ENDLIST\n").is_err());
        assert!(media("#EXTM3U\n#EXTINF:abc,\nseg.ts\n").is_err());
        assert!(media("#EXTM3U\n#EXT-X-ENDLIST\n").is_err());
        assert!(media("#EXTM3U\n#EXTINF:4,\n#EXT-X-BYTERANGE:100\nseg.ts\n").is_err());
    }

    #[test]
    fn an_audio_rendition_is_preferred_and_named_by_its_variant_codecs() {
        let master = master(
            "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aac\",NAME=\"en\",URI=\"audio/alt.m3u8\"\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aac\",NAME=\"main\",DEFAULT=YES,URI=\"audio/main.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=900000,CODECS=\"avc1.4d401f,mp4a.40.5\",AUDIO=\"aac\"\nvideo/low.m3u8\n",
        );
        let choice = choose(&master).unwrap();
        assert_eq!(
            choice.url.as_str(),
            "https://cdn.example.com/audio/main.m3u8"
        );
        assert_eq!(choice.codecs.as_deref(), Some("mp4a.40.5"));
    }

    #[test]
    fn the_best_audio_only_variant_wins_over_video() {
        let master = master(
            "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=64000,CODECS=\"mp4a.40.5\"\nlow.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=160000,CODECS=\"mp4a.40.2\"\nhigh.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=900000,CODECS=\"avc1.4d401f,mp4a.40.2\"\nvideo.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=192000,CODECS=\"ec-3\"\natmos.m3u8\n",
        );
        let choice = choose(&master).unwrap();
        assert_eq!(choice.url.as_str(), "https://cdn.example.com/high.m3u8");
        assert_eq!(choice.codecs.as_deref(), Some("mp4a.40.2"));
    }

    #[test]
    fn without_audio_only_variants_the_lightest_video_variant_is_played() {
        let master = master(
            "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=2000000\nhd.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=500000\nsd.m3u8\n",
        );
        assert_eq!(
            choose(&master).unwrap().url.as_str(),
            "https://cdn.example.com/sd.m3u8"
        );
        let only_eac3 =
            master_of("#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1,CODECS=\"ec-3\"\na.m3u8\n");
        assert!(matches!(
            choose(&only_eac3),
            Err(Error::UnsupportedCodec(_))
        ));
    }

    fn master_of(text: &str) -> MasterPlaylist {
        master(text)
    }
}
