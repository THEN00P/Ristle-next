//! A trimmed copy of Ristle legacy's SoundCloud Plugin (`v2.rs`): search and
//! stream resolution against the anonymous `api-v2.soundcloud.com`, behind a
//! throwaway WIT. The logic is generic over [`Http`] so the same code runs as
//! the WASM component (HTTP is a host import) and natively as the baseline.

use std::cell::RefCell;

use serde::Deserialize;

const API_ROOT: &str = "https://api-v2.soundcloud.com";
const WEB_ROOT: &str = "https://soundcloud.com/";
const BUNDLE_PREFIX: &str = "https://a-v2.sndcdn.com/assets/";
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36";

pub struct Request {
    pub url: String,
    pub headers: Vec<(String, String)>,
}

pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

pub trait Http {
    fn get_batch(&self, requests: Vec<Request>) -> Vec<Result<Response, String>>;

    fn get(&self, request: Request) -> Result<Response, String> {
        self.get_batch(vec![request]).pop().expect("one response")
    }
}

pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub duration_ms: Option<u64>,
    pub artwork: Option<String>,
    pub permalink: Option<String>,
    pub playable: bool,
    pub unavailable_reason: Option<String>,
}

pub struct Artist {
    pub id: String,
    pub name: String,
    pub artwork: Option<String>,
}

pub struct Playlist {
    pub id: String,
    pub title: String,
    pub owner: Option<String>,
    pub track_count: Option<u64>,
    pub artwork: Option<String>,
}

pub struct SearchResults {
    pub tracks: Vec<Track>,
    pub artists: Vec<Artist>,
    pub playlists: Vec<Playlist>,
    pub next_page: Option<String>,
}

pub enum StreamProtocol {
    Hls,
    Progressive,
}

pub struct StreamSource {
    pub url: String,
    pub protocol: StreamProtocol,
    pub mime_type: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Collection<T> {
    Paged { collection: Vec<T>, next_href: Option<String> },
    List(Vec<T>),
}

impl<T> Collection<T> {
    fn into_parts(self) -> (Vec<T>, Option<String>) {
        match self {
            Self::Paged { collection, next_href } => (collection, next_href),
            Self::List(items) => (items, None),
        }
    }
}

#[derive(Deserialize)]
struct V2Track {
    urn: String,
    title: String,
    artwork_url: Option<String>,
    duration: Option<u64>,
    permalink_url: Option<String>,
    streamable: Option<bool>,
    policy: Option<String>,
    user: V2User,
    media: Option<Media>,
    track_authorization: Option<String>,
}

#[derive(Deserialize)]
struct V2User {
    urn: String,
    username: String,
    avatar_url: Option<String>,
}

#[derive(Deserialize)]
struct V2Playlist {
    urn: Option<String>,
    id: u64,
    title: String,
    artwork_url: Option<String>,
    track_count: Option<u64>,
    user: Option<V2User>,
}

#[derive(Deserialize)]
struct Media {
    transcodings: Vec<Transcoding>,
}

#[derive(Deserialize)]
struct Transcoding {
    url: String,
    #[serde(default)]
    preset: String,
    #[serde(default)]
    quality: String,
    #[serde(default)]
    format: Format,
}

#[derive(Default, Deserialize)]
struct Format {
    #[serde(default)]
    protocol: String,
    #[serde(default)]
    mime_type: String,
}

#[derive(Deserialize)]
struct StreamUrl {
    url: String,
}

/// Plugin state. Production would keep the client id in host storage; the
/// spike keeps it for the instance's lifetime.
#[derive(Default)]
pub struct Plugin {
    client_id: RefCell<Option<String>>,
}

impl Plugin {
    pub fn search(&self, http: &impl Http, query: &str, limit: u32) -> Result<SearchResults, String> {
        self.with_client_id(http, |client_id| search(http, client_id, query, limit))
    }

    pub fn resolve(&self, http: &impl Http, track_id: &str) -> Result<StreamSource, String> {
        self.with_client_id(http, |client_id| resolve(http, client_id, track_id))
    }

    /// Runs `call` with the anonymous web client id, rediscovering it once if
    /// SoundCloud rejects it (the web app rotates it).
    fn with_client_id<T>(
        &self,
        http: &impl Http,
        call: impl Fn(&str) -> Result<T, ApiError>,
    ) -> Result<T, String> {
        for attempt in 0..2 {
            let cached = self.client_id.borrow().clone();
            let client_id = match cached {
                Some(id) => id,
                None => {
                    let id = discover_client_id(http)?;
                    *self.client_id.borrow_mut() = Some(id.clone());
                    id
                }
            };
            match call(&client_id) {
                Err(ApiError::Unauthorized(_)) if attempt == 0 => {
                    *self.client_id.borrow_mut() = None;
                }
                result => return result.map_err(ApiError::into_message),
            }
        }
        unreachable!()
    }
}

enum ApiError {
    Unauthorized(String),
    Other(String),
}

impl ApiError {
    fn into_message(self) -> String {
        match self {
            Self::Unauthorized(message) | Self::Other(message) => message,
        }
    }
}

impl From<String> for ApiError {
    fn from(message: String) -> Self {
        Self::Other(message)
    }
}

/// The anonymous soundcloud.com web player embeds its client id in one of its
/// script bundles. Ristle legacy captured the same id from a signed-in web
/// view; this needs no sign-in.
pub fn discover_client_id(http: &impl Http) -> Result<String, String> {
    let page = http.get(browser_request(WEB_ROOT))?;
    let html = String::from_utf8_lossy(&page.body);
    let bundles: Vec<&str> = html
        .match_indices(BUNDLE_PREFIX)
        .filter_map(|(start, _)| {
            let rest = &html[start..];
            let end = rest.find('"')?;
            rest[..end].ends_with(".js").then(|| &rest[..end])
        })
        .collect();
    // The id lives in the last, largest bundle, so search from the end.
    for bundle in bundles.iter().rev() {
        let script = http.get(browser_request(bundle))?;
        if let Some(id) = find_client_id(&script.body) {
            return Ok(id);
        }
    }
    Err("SoundCloud client id not found in the web player bundles".to_string())
}

fn find_client_id(script: &[u8]) -> Option<String> {
    const NEEDLE: &[u8] = b"client_id:\"";
    let start = script.windows(NEEDLE.len()).position(|w| w == NEEDLE)? + NEEDLE.len();
    let id: Vec<u8> = script[start..].iter().take_while(|b| b.is_ascii_alphanumeric()).copied().collect();
    (id.len() >= 16).then(|| String::from_utf8(id).expect("ascii"))
}

fn browser_request(url: &str) -> Request {
    Request { url: url.to_string(), headers: vec![("User-Agent".to_string(), USER_AGENT.to_string())] }
}

fn api_request(path_and_query: &str, client_id: &str) -> Request {
    let separator = if path_and_query.contains('?') { '&' } else { '?' };
    let url = if path_and_query.starts_with("https://") {
        format!("{path_and_query}{separator}client_id={client_id}")
    } else {
        format!("{API_ROOT}{path_and_query}{separator}client_id={client_id}")
    };
    Request { url, headers: vec![("Accept".to_string(), "application/json; charset=utf-8".to_string())] }
}

fn decode<T: for<'de> Deserialize<'de>>(response: Result<Response, String>, what: &str) -> Result<T, ApiError> {
    let response = response.map_err(|e| ApiError::Other(format!("SoundCloud {what} request failed: {e}")))?;
    if response.status == 401 || response.status == 403 {
        return Err(ApiError::Unauthorized(format!("SoundCloud rejected the client id: HTTP {}", response.status)));
    }
    if !(200..300).contains(&response.status) {
        let body = String::from_utf8_lossy(&response.body);
        let detail: String = body.trim().chars().take(200).collect();
        return Err(ApiError::Other(format!("SoundCloud {what} failed: HTTP {}: {detail}", response.status)));
    }
    serde_json::from_slice(&response.body).map_err(|e| ApiError::Other(format!("SoundCloud {what} response was invalid: {e}")))
}

fn allocate_search_limit(limit: u32) -> [u32; 3] {
    let limit = limit.clamp(1, 200);
    let each = limit / 3;
    let remainder = limit % 3;
    [each + u32::from(remainder > 0), each + u32::from(remainder > 1), each]
}

fn search(http: &impl Http, client_id: &str, query: &str, limit: u32) -> Result<SearchResults, ApiError> {
    let [track_limit, user_limit, playlist_limit] = allocate_search_limit(limit);
    let q = encode_query(query);
    let requests = [("tracks", track_limit), ("users", user_limit), ("playlists", playlist_limit)]
        .into_iter()
        .map(|(path, limit)| api_request(&format!("/search/{path}?q={q}&limit={}&linked_partitioning=1", limit.max(1)), client_id))
        .collect();
    let mut responses = http.get_batch(requests).into_iter();
    let tracks: Collection<V2Track> = decode(responses.next().expect("tracks"), "track search")?;
    let users: Collection<V2User> = decode(responses.next().expect("users"), "user search")?;
    let playlists: Collection<V2Playlist> = decode(responses.next().expect("playlists"), "playlist search")?;
    let (tracks, next_page) = tracks.into_parts();
    Ok(SearchResults {
        tracks: tracks.into_iter().take(track_limit as usize).map(track_value).collect(),
        artists: users.into_parts().0.into_iter().take(user_limit as usize).map(artist_value).collect(),
        playlists: playlists.into_parts().0.into_iter().take(playlist_limit as usize).map(playlist_value).collect(),
        next_page,
    })
}

fn resolve(http: &impl Http, client_id: &str, track_id: &str) -> Result<StreamSource, ApiError> {
    let id = track_id.rsplit(':').next().unwrap_or(track_id);
    let track: V2Track = decode(http.get(api_request(&format!("/tracks/{id}"), client_id)), "track")?;
    if let Some(reason) = unplayable_reason(&track) {
        return Err(ApiError::Other(reason.to_string()));
    }
    let authorization = track
        .track_authorization
        .as_deref()
        .ok_or_else(|| ApiError::Other("SoundCloud Track is missing its stream authorization".to_string()))?;
    let transcodings = track.media.as_ref().map(|m| m.transcodings.as_slice()).unwrap_or_default();
    let mut last_error = None;
    // Progressive MP3 was retired at the end of 2025 (404), so only AAC-HLS.
    for transcoding in modern_stream_transcodings(transcodings) {
        let url = format!("{}?track_authorization={authorization}", transcoding.url);
        match decode::<StreamUrl>(http.get(api_request(&url, client_id)), "stream") {
            Ok(resolved) => {
                return Ok(StreamSource {
                    url: resolved.url,
                    protocol: StreamProtocol::Hls,
                    mime_type: transcoding.format.mime_type.clone(),
                })
            }
            Err(e @ ApiError::Unauthorized(_)) => return Err(e),
            Err(e) => last_error = Some(e),
        }
    }
    Err(last_error.unwrap_or_else(|| ApiError::Other("SoundCloud Track has no supported stream".to_string())))
}

fn track_value(track: V2Track) -> Track {
    let unavailable_reason = unplayable_reason(&track).map(str::to_string);
    Track {
        id: track.urn,
        title: track.title,
        artist: track.user.username,
        duration_ms: track.duration,
        artwork: artwork(track.artwork_url.or(track.user.avatar_url)),
        permalink: track.permalink_url,
        playable: unavailable_reason.is_none(),
        unavailable_reason,
    }
}

fn artist_value(user: V2User) -> Artist {
    Artist { id: user.urn, name: user.username, artwork: artwork(user.avatar_url) }
}

fn playlist_value(playlist: V2Playlist) -> Playlist {
    Playlist {
        id: playlist.urn.unwrap_or_else(|| format!("soundcloud:playlists:{}", playlist.id)),
        title: playlist.title,
        owner: playlist.user.map(|u| u.username),
        track_count: playlist.track_count,
        artwork: artwork(playlist.artwork_url),
    }
}

/// SoundCloud artwork URLs end in `-large`; ask for the 500 px variant.
fn artwork(url: Option<String>) -> Option<String> {
    let url = url?;
    Some(match url.rfind("-large.") {
        Some(index) => format!("{}-t500x500{}", &url[..index], &url[index + "-large".len()..]),
        None => url,
    })
}

fn modern_stream_transcodings(transcodings: &[Transcoding]) -> Vec<&Transcoding> {
    let mut ordered: Vec<_> = transcodings
        .iter()
        .filter(|t| t.format.protocol == "hls" && transcoding_rank(t).is_some())
        .collect();
    ordered.sort_by_key(|t| transcoding_rank(t));
    ordered
}

fn transcoding_rank(transcoding: &Transcoding) -> Option<(u8, u8, std::cmp::Reverse<(u32, u32)>)> {
    let protocol = transcoding.format.protocol.as_str();
    let mime = transcoding.format.mime_type.to_ascii_lowercase();
    let preset = transcoding.preset.to_ascii_lowercase();
    let quality = match transcoding.quality.to_ascii_lowercase().as_str() {
        "hq" => 0,
        "sq" => 1,
        _ => 2,
    };
    let mut parts = preset.split('_').skip(1).filter_map(|p| p.parse::<u32>().ok());
    let preset_rank = std::cmp::Reverse((parts.next().unwrap_or_default(), parts.next().unwrap_or_default()));
    if protocol == "hls" && (mime.contains("aac") || mime.contains("mp4") || preset.starts_with("aac_")) {
        Some((0, quality, preset_rank))
    } else if protocol == "progressive"
        && ["mpeg", "mp3", "aac", "mp4", "ogg", "opus"].iter().any(|m| mime.contains(m))
    {
        Some((1, quality, preset_rank))
    } else {
        None
    }
}

fn unplayable_reason(track: &V2Track) -> Option<&'static str> {
    if !(track.streamable.unwrap_or(true) && !matches!(track.policy.as_deref(), Some("BLOCK") | Some("SNIP"))) {
        return Some("SoundCloud does not allow full off-platform playback");
    }
    let transcodings = track.media.as_ref().map(|m| m.transcodings.as_slice()).unwrap_or_default();
    if !modern_stream_transcodings(transcodings).is_empty() {
        return None;
    }
    if transcodings.iter().any(|t| t.format.protocol.contains("encrypted")) {
        Some("SoundCloud only streams this Track with DRM protection, which Ristle cannot play")
    } else {
        Some("SoundCloud does not offer a stream Ristle can play for this Track")
    }
}

fn encode_query(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(target_arch = "wasm32")]
mod component {
    wit_bindgen::generate!({ path: "../wit", world: "soundcloud" });

    use exports::ristle::spike_sc::provider as wit;
    use ristle::spike_sc::http as host;

    struct HostHttp;

    impl super::Http for HostHttp {
        fn get_batch(&self, requests: Vec<super::Request>) -> Vec<Result<super::Response, String>> {
            let requests = requests.into_iter().map(|r| host::Request { url: r.url, headers: r.headers }).collect::<Vec<_>>();
            host::get_batch(&requests)
                .into_iter()
                .map(|r| r.map(|r| super::Response { status: r.status, body: r.body }))
                .collect()
        }
    }

    thread_local! {
        static PLUGIN: super::Plugin = super::Plugin::default();
    }

    struct Component;

    impl wit::Guest for Component {
        fn search(query: String, limit: u32) -> Result<wit::SearchResults, String> {
            let r = PLUGIN.with(|p| p.search(&HostHttp, &query, limit))?;
            Ok(wit::SearchResults {
                tracks: r
                    .tracks
                    .into_iter()
                    .map(|t| wit::Track {
                        id: t.id,
                        title: t.title,
                        artist: t.artist,
                        duration_ms: t.duration_ms,
                        artwork: t.artwork,
                        permalink: t.permalink,
                        playable: t.playable,
                        unavailable_reason: t.unavailable_reason,
                    })
                    .collect(),
                artists: r.artists.into_iter().map(|a| wit::Artist { id: a.id, name: a.name, artwork: a.artwork }).collect(),
                playlists: r
                    .playlists
                    .into_iter()
                    .map(|p| wit::Playlist { id: p.id, title: p.title, owner: p.owner, track_count: p.track_count, artwork: p.artwork })
                    .collect(),
                next_page: r.next_page,
            })
        }

        fn resolve(track_id: String) -> Result<wit::StreamSource, String> {
            let s = PLUGIN.with(|p| p.resolve(&HostHttp, &track_id))?;
            Ok(wit::StreamSource {
                url: s.url,
                protocol: match s.protocol {
                    super::StreamProtocol::Hls => wit::StreamProtocol::Hls,
                    super::StreamProtocol::Progressive => wit::StreamProtocol::Progressive,
                },
                mime_type: s.mime_type,
            })
        }
    }

    export!(Component);
}
