//! The manifest (`docs/plugin-api/packaging.md`, "Manifest").
//!
//! Parsed strictly: unknown keys are refused, so a misspelt permission never
//! silently means something else, and a manifest from a newer Plugin API
//! does not install on a host that would ignore part of it.

use std::collections::BTreeSet;

use ristle_domain::{PluginId, ProviderId};
use ristle_net::HostPattern;
use serde::Deserialize;
use url::Url;

use super::PackageError;
use super::key::PublisherKey;

/// `storage-mib` when the manifest says nothing.
pub const DEFAULT_STORAGE_MIB: u32 = 5;
/// The largest `storage-mib` a manifest may ask for.
pub const MAX_STORAGE_MIB: u32 = 64;

const MAX_TEXT: usize = 256;
const MAX_DESCRIPTION: usize = 2048;
const MAX_LIST: usize = 64;

/// A validated manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    /// Reverse DNS, stable forever.
    pub id: PluginId,
    /// Shown as the Plugin's name.
    pub name: String,
    /// Must grow with every update.
    pub version: semver::Version,
    /// Who made it, shown and never trusted.
    pub publisher: String,
    /// The key this package says it is signed with.
    pub publisher_key: PublisherKey,
    /// A short description.
    pub description: Option<String>,
    /// An HTTPS link, opened in a browser.
    pub homepage: Option<Url>,
    /// The Plugin's licence, as text.
    pub license: Option<String>,
    /// The HTTPS update feed.
    pub updates: Option<Url>,
    /// The Provider it serves.
    pub provider: ProviderInfo,
    /// What it may reach.
    pub permissions: Permissions,
    /// Sign-in settings.
    pub auth: Auth,
    /// Batch sizes the host never exceeds.
    pub limits: Limits,
    /// What a Party Host may relay to guests (ADR 0012).
    pub party: Party,
    /// Playback switches (ADR 0012).
    pub playback: Playback,
}

/// `[provider]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderInfo {
    /// The Provider's id; several Plugins may serve one.
    pub id: ProviderId,
    /// The Provider's name.
    pub name: String,
}

/// One `sockets` entry.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SocketTarget {
    /// An exact host name or address.
    pub host: String,
    /// The port.
    pub port: u16,
}

impl std::fmt::Display for SocketTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.host, self.port)
    }
}

/// `[permissions]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Permissions {
    /// HTTPS hosts for `connection.fetch`.
    pub hosts: Vec<HostPattern>,
    /// Hosts stream URLs may use.
    pub media_hosts: Vec<HostPattern>,
    /// Hosts artwork may come from.
    pub image_hosts: Vec<HostPattern>,
    /// Raw TCP destinations.
    pub sockets: Vec<SocketTarget>,
    /// Hosts the sign-in window may load.
    pub web_capture: Vec<HostPattern>,
    /// The subset whose traffic and pages may be watched and scripted.
    pub web_capture_observe: Vec<HostPattern>,
    /// The `storage` quota.
    pub storage_mib: u32,
    /// May ask the user for a server (Server Grants).
    pub servers: bool,
    /// May ask the user for music folders (Folder Grants).
    pub folders: bool,
}

/// One `[auth] redirects` entry.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Redirect {
    /// `loopback:<port>/<path>`: `http://127.0.0.1:<port>/<path>` on desktop.
    Loopback {
        /// The port.
        port: u16,
        /// The path, without its leading slash.
        path: String,
    },
    /// Ristle's custom scheme on mobile.
    Scheme,
    /// Ristle's hosted HTTPS redirect page.
    Remote,
}

/// `[auth]`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Auth {
    /// The OAuth redirects registered with the Provider.
    pub redirects: Vec<Redirect>,
    /// Schemes an `opener: app` URL may use.
    pub app_schemes: Vec<String>,
}

/// `[limits]`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Limits {
    /// Ids per `catalog.tracks` call.
    pub tracks: Option<u32>,
    /// Items per `like-lookup.liked-states` call.
    pub liked_states: Option<u32>,
    /// Tracks per playlist add.
    pub playlist_add: Option<u32>,
    /// Occurrences per playlist removal.
    pub playlist_remove: Option<u32>,
}

/// The four records whose fields `[party] share` names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShareKind {
    /// `track`.
    Track,
    /// `album`.
    Album,
    /// `profile`.
    Profile,
    /// `playlist`.
    Playlist,
}

impl ShareKind {
    /// Every kind.
    pub const ALL: [ShareKind; 4] = [
        ShareKind::Track,
        ShareKind::Album,
        ShareKind::Profile,
        ShareKind::Playlist,
    ];

    /// The WIT field names of the record that may be shared. `track.file`
    /// never is: it is a path on this device.
    pub fn fields(self) -> &'static [&'static str] {
        match self {
            ShareKind::Track => &[
                "id",
                "title",
                "artists",
                "album",
                "duration-ms",
                "artwork",
                "traits",
                "released",
                "uploaded",
                "unavailable",
                "isrc",
                "url",
                "attributes",
            ],
            ShareKind::Album => &[
                "id",
                "title",
                "artists",
                "artwork",
                "released",
                "release-type",
                "track-count",
                "url",
            ],
            ShareKind::Profile => &[
                "id",
                "name",
                "artwork",
                "about",
                "followers",
                "location",
                "url",
            ],
            ShareKind::Playlist => &[
                "id",
                "title",
                "description",
                "owner",
                "artwork",
                "track-count",
                "created",
                "updated",
                "public",
                "editable",
                "folder",
                "url",
            ],
        }
    }
}

/// `[party]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Party {
    /// The field names that may be relayed, or `None` for every field.
    pub share: Option<BTreeSet<String>>,
}

impl Party {
    /// Whether a field of a kind may be relayed.
    pub fn shares(&self, kind: ShareKind, field: &str) -> bool {
        kind.fields().contains(&field)
            && self
                .share
                .as_ref()
                .is_none_or(|share| share.contains(field))
    }
}

/// `[playback]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Playback {
    /// Crossfade or other overlap into or out of this Provider's Tracks.
    pub overlap: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct RawManifest {
    id: String,
    name: String,
    version: String,
    publisher: String,
    publisher_key: String,
    description: Option<String>,
    homepage: Option<String>,
    license: Option<String>,
    updates: Option<String>,
    provider: RawProvider,
    #[serde(default)]
    permissions: RawPermissions,
    #[serde(default)]
    auth: RawAuth,
    #[serde(default)]
    limits: RawLimits,
    #[serde(default)]
    party: RawParty,
    #[serde(default)]
    playback: RawPlayback,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct RawProvider {
    id: String,
    name: String,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct RawPermissions {
    #[serde(default)]
    hosts: Vec<String>,
    #[serde(default)]
    media_hosts: Vec<String>,
    #[serde(default)]
    image_hosts: Vec<String>,
    #[serde(default)]
    sockets: Vec<String>,
    #[serde(default)]
    web_capture: Vec<String>,
    #[serde(default)]
    web_capture_observe: Vec<String>,
    storage_mib: Option<u32>,
    #[serde(default)]
    servers: bool,
    #[serde(default)]
    folders: bool,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct RawAuth {
    #[serde(default)]
    redirects: Vec<String>,
    #[serde(default)]
    app_schemes: Vec<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct RawLimits {
    tracks: Option<u32>,
    liked_states: Option<u32>,
    playlist_add: Option<u32>,
    playlist_remove: Option<u32>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct RawParty {
    share: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct RawPlayback {
    #[serde(default = "yes")]
    overlap: bool,
}

impl Default for RawPlayback {
    fn default() -> Self {
        Self { overlap: true }
    }
}

fn yes() -> bool {
    true
}

/// The two fields read before the signature is checked.
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Identity {
    id: String,
    publisher_key: String,
}

fn bad(message: impl Into<String>) -> PackageError {
    PackageError::Manifest(message.into())
}

fn text(bytes: &[u8]) -> Result<&str, PackageError> {
    std::str::from_utf8(bytes).map_err(|_| bad("not UTF-8"))
}

/// Reads only `id` and `publisher-key` from a manifest that is not trusted
/// yet, to choose the key to verify with (install step 2).
pub(crate) fn identity(bytes: &[u8]) -> Result<(PluginId, PublisherKey), PackageError> {
    let identity: Identity = toml::from_str(text(bytes)?).map_err(|error| bad(error.message()))?;
    Ok((
        plugin_id(&identity.id)?,
        PublisherKey::parse(&identity.publisher_key)?,
    ))
}

/// Parses and validates the whole manifest (install step 4).
pub(crate) fn parse(bytes: &[u8]) -> Result<Manifest, PackageError> {
    let raw: RawManifest = toml::from_str(text(bytes)?).map_err(|error| bad(error.message()))?;
    let version = semver::Version::parse(&raw.version)
        .map_err(|_| bad(format!("version {:?} is not semver", raw.version)))?;
    let permissions = permissions(raw.permissions)?;
    let auth = auth(raw.auth)?;
    let limits = Limits {
        tracks: limit("tracks", raw.limits.tracks)?,
        liked_states: limit("liked-states", raw.limits.liked_states)?,
        playlist_add: limit("playlist-add", raw.limits.playlist_add)?,
        playlist_remove: limit("playlist-remove", raw.limits.playlist_remove)?,
    };
    let share = match raw.party.share {
        None => None,
        Some(fields) => {
            let known: BTreeSet<&str> = ShareKind::ALL
                .iter()
                .flat_map(|kind| kind.fields().iter().copied())
                .collect();
            let mut out = BTreeSet::new();
            for field in fields {
                if !known.contains(field.as_str()) {
                    return Err(bad(format!("party.share names unknown field {field:?}")));
                }
                out.insert(field);
            }
            Some(out)
        }
    };
    Ok(Manifest {
        id: plugin_id(&raw.id)?,
        name: required_text("name", raw.name, MAX_TEXT)?,
        version,
        publisher: required_text("publisher", raw.publisher, MAX_TEXT)?,
        publisher_key: PublisherKey::parse(&raw.publisher_key)?,
        description: optional_text("description", raw.description, MAX_DESCRIPTION)?,
        homepage: raw
            .homepage
            .as_deref()
            .map(|url| https("homepage", url))
            .transpose()?,
        license: optional_text("license", raw.license, MAX_TEXT)?,
        updates: raw
            .updates
            .as_deref()
            .map(|url| https("updates", url))
            .transpose()?,
        provider: ProviderInfo {
            id: provider_id(&raw.provider.id)?,
            name: required_text("provider.name", raw.provider.name, MAX_TEXT)?,
        },
        permissions,
        auth,
        limits,
        party: Party { share },
        playback: Playback {
            overlap: raw.playback.overlap,
        },
    })
}

/// Reverse DNS: at least two dot-separated labels of lowercase letters,
/// digits and hyphens, each starting with a letter.
fn plugin_id(id: &str) -> Result<PluginId, PackageError> {
    let labels: Vec<&str> = id.split('.').collect();
    let valid = id.len() <= 128
        && labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.starts_with(|c: char| c.is_ascii_lowercase())
                && !label.ends_with('-')
                && label
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        });
    if !valid {
        return Err(bad(format!("id {id:?} is not reverse DNS")));
    }
    Ok(PluginId::new(id))
}

/// A short lowercase token: letters, digits, `-` and `_`.
fn provider_id(id: &str) -> Result<ProviderId, PackageError> {
    let valid = !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if !valid {
        return Err(bad(format!("provider.id {id:?} is not a lowercase token")));
    }
    Ok(ProviderId::new(id))
}

fn required_text(field: &str, value: String, max: usize) -> Result<String, PackageError> {
    let value = optional_text(field, Some(value), max)?;
    value.ok_or_else(|| bad(format!("{field} is empty")))
}

fn optional_text(
    field: &str,
    value: Option<String>,
    max: usize,
) -> Result<Option<String>, PackageError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let trimmed = value.trim();
    if trimmed.len() > max {
        return Err(bad(format!("{field} is longer than {max} bytes")));
    }
    if trimmed.chars().any(|c| c.is_control() && c != '\n') {
        return Err(bad(format!("{field} contains control characters")));
    }
    Ok((!trimmed.is_empty()).then(|| trimmed.to_owned()))
}

fn https(field: &str, url: &str) -> Result<Url, PackageError> {
    let parsed = Url::parse(url).map_err(|_| bad(format!("{field} is not a URL")))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(bad(format!("{field} must be an HTTPS URL")));
    }
    Ok(parsed)
}

fn patterns(field: &str, values: Vec<String>) -> Result<Vec<HostPattern>, PackageError> {
    if values.len() > MAX_LIST {
        return Err(bad(format!("{field} has more than {MAX_LIST} entries")));
    }
    let mut out: Vec<HostPattern> = Vec::with_capacity(values.len());
    for value in values {
        let pattern = HostPattern::parse(&value)
            .map_err(|_| bad(format!("{field}: invalid host {value:?}")))?;
        if !out.contains(&pattern) {
            out.push(pattern);
        }
    }
    Ok(out)
}

fn permissions(raw: RawPermissions) -> Result<Permissions, PackageError> {
    let storage_mib = raw.storage_mib.unwrap_or(DEFAULT_STORAGE_MIB);
    if storage_mib > MAX_STORAGE_MIB {
        return Err(bad(format!("storage-mib above {MAX_STORAGE_MIB}")));
    }
    if raw.sockets.len() > MAX_LIST {
        return Err(bad(format!("sockets has more than {MAX_LIST} entries")));
    }
    let mut sockets = Vec::new();
    for socket in raw.sockets {
        let invalid = || bad(format!("sockets: {socket:?} is not host:port"));
        let (host, port) = socket.rsplit_once(':').ok_or_else(invalid)?;
        let port: u16 = port.parse().map_err(|_| invalid())?;
        let pattern = HostPattern::parse(host).map_err(|_| invalid())?;
        if port == 0 || pattern.is_wildcard() {
            return Err(invalid());
        }
        let target = SocketTarget {
            host: pattern.to_string(),
            port,
        };
        if !sockets.contains(&target) {
            sockets.push(target);
        }
    }
    let web_capture = patterns("web-capture", raw.web_capture)?;
    let web_capture_observe = patterns("web-capture-observe", raw.web_capture_observe)?;
    if let Some(outside) = web_capture_observe
        .iter()
        .find(|observed| !web_capture.iter().any(|loaded| loaded.covers(observed)))
    {
        return Err(bad(format!(
            "web-capture-observe host {outside} is not in web-capture"
        )));
    }
    Ok(Permissions {
        hosts: patterns("hosts", raw.hosts)?,
        media_hosts: patterns("media-hosts", raw.media_hosts)?,
        image_hosts: patterns("image-hosts", raw.image_hosts)?,
        sockets,
        web_capture,
        web_capture_observe,
        storage_mib,
        servers: raw.servers,
        folders: raw.folders,
    })
}

fn auth(raw: RawAuth) -> Result<Auth, PackageError> {
    let mut redirects = Vec::new();
    for redirect in raw.redirects {
        let parsed = match redirect.as_str() {
            "scheme" => Redirect::Scheme,
            "remote" => Redirect::Remote,
            other => {
                let invalid = || bad(format!("auth.redirects: {other:?}"));
                let rest = other.strip_prefix("loopback:").ok_or_else(invalid)?;
                let (port, path) = rest.split_once('/').unwrap_or((rest, ""));
                let port: u16 = port.parse().map_err(|_| invalid())?;
                let path_ok = path.len() <= MAX_TEXT
                    && path.chars().all(|c| {
                        c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.' | '~')
                    });
                if port < 1024 || !path_ok {
                    return Err(invalid());
                }
                Redirect::Loopback {
                    port,
                    path: path.to_owned(),
                }
            }
        };
        if !redirects.contains(&parsed) {
            redirects.push(parsed);
        }
    }
    let mut app_schemes = Vec::new();
    for scheme in raw.app_schemes {
        let lower = scheme.to_ascii_lowercase();
        let syntax = lower.starts_with(|c: char| c.is_ascii_lowercase())
            && lower.len() <= 64
            && lower.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '+' | '-' | '.')
            });
        // Schemes the operating system or a browser gives a meaning of
        // their own are never an app's.
        const RESERVED: [&str; 12] = [
            "http",
            "https",
            "file",
            "content",
            "intent",
            "javascript",
            "data",
            "blob",
            "about",
            "ftp",
            "mailto",
            "ristle",
        ];
        if !syntax || RESERVED.contains(&lower.as_str()) {
            return Err(bad(format!("auth.app-schemes: {scheme:?}")));
        }
        if !app_schemes.contains(&lower) {
            app_schemes.push(lower);
        }
    }
    Ok(Auth {
        redirects,
        app_schemes,
    })
}

fn limit(name: &str, value: Option<u32>) -> Result<Option<u32>, PackageError> {
    match value {
        Some(0) => Err(bad(format!("limits.{name} must be at least 1"))),
        Some(value) if value > 10_000 => Err(bad(format!("limits.{name} above 10000"))),
        other => Ok(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "ed25519:d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";

    fn manifest(extra: &str) -> String {
        format!(
            r#"
id = "dev.example.soundcloud"
name = "SoundCloud"
version = "1.4.0"
publisher = "Example Developer"
publisher-key = "{KEY}"
{extra}

[provider]
id = "soundcloud"
name = "SoundCloud"
"#
        )
    }

    fn parsed(text: &str) -> Result<Manifest, PackageError> {
        parse(text.as_bytes())
    }

    #[test]
    fn the_documented_manifest_parses() {
        let text = r#"
id = "dev.example.soundcloud"
name = "SoundCloud"
version = "1.4.0"
publisher = "Example Developer"
publisher-key = "ed25519:d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"
description = "Search, browse and play SoundCloud."
homepage = "https://github.com/example/ristle-soundcloud"
license = "MIT"
updates = "https://example.com/ristle/soundcloud.json"

[provider]
id = "soundcloud"
name = "SoundCloud"

[permissions]
hosts = ["api-v2.soundcloud.com", "api.soundcloud.com", "secure.soundcloud.com"]
media-hosts = ["*.sndcdn.com"]
image-hosts = ["*.sndcdn.com"]
sockets = []
web-capture = ["soundcloud.com", "*.soundcloud.com", "accounts.google.com"]
web-capture-observe = ["soundcloud.com", "*.soundcloud.com"]
storage-mib = 5

[auth]
redirects = ["loopback:8990/login", "scheme", "remote"]

[limits]
tracks = 50
liked-states = 40
playlist-add = 100
playlist-remove = 100

[party]
share = ["title", "artists", "album", "artwork", "duration-ms"]

[playback]
overlap = true
"#;
        let manifest = parsed(text).unwrap();
        assert_eq!(manifest.id.as_str(), "dev.example.soundcloud");
        assert_eq!(manifest.version, semver::Version::new(1, 4, 0));
        assert_eq!(manifest.permissions.hosts.len(), 3);
        assert_eq!(
            manifest.permissions.media_hosts[0].to_string(),
            "*.sndcdn.com"
        );
        assert_eq!(
            manifest.auth.redirects,
            vec![
                Redirect::Loopback {
                    port: 8990,
                    path: "login".into()
                },
                Redirect::Scheme,
                Redirect::Remote
            ]
        );
        assert_eq!(manifest.limits.tracks, Some(50));
        assert!(manifest.party.shares(ShareKind::Track, "title"));
        assert!(!manifest.party.shares(ShareKind::Track, "isrc"));
        assert!(manifest.playback.overlap);
        assert_eq!(identity(text.as_bytes()).unwrap().0, manifest.id);
    }

    #[test]
    fn defaults_apply_when_sections_are_missing() {
        let manifest = parsed(&manifest("")).unwrap();
        assert_eq!(manifest.permissions.storage_mib, DEFAULT_STORAGE_MIB);
        assert!(manifest.permissions.hosts.is_empty());
        assert_eq!(manifest.party.share, None);
        assert!(manifest.party.shares(ShareKind::Playlist, "description"));
        assert!(!manifest.party.shares(ShareKind::Track, "file"));
        assert!(manifest.playback.overlap);
        assert_eq!(manifest.limits, Limits::default());
    }

    #[test]
    fn invalid_manifests_are_refused() {
        let base = manifest("");
        let cases = [
            base.replace("dev.example.soundcloud", "soundcloud"),
            base.replace("dev.example.soundcloud", "Dev.Example.x"),
            base.replace("dev.example.soundcloud", "dev..x"),
            base.replace("1.4.0", "1.4"),
            base.replace("Example Developer", "  "),
            base.replace(KEY, "ed25519:00"),
            base.replace("id = \"soundcloud\"", "id = \"Sound Cloud\""),
            manifest("homepage = \"http://example.com\""),
            manifest("homepage = \"javascript:alert(1)\""),
            manifest("updates = \"http://example.com/feed.json\""),
            manifest("unknown = 1"),
            manifest("[permissions]\nstorage-mib = 65"),
            manifest("[permissions]\nhosts = [\"*.com\"]"),
            manifest("[permissions]\nhosts = [\"https://api.example.com\"]"),
            manifest("[permissions]\nhost = [\"api.example.com\"]"),
            manifest("[permissions]\nsockets = [\"ap.example.com\"]"),
            manifest("[permissions]\nsockets = [\"*.example.com:443\"]"),
            manifest("[permissions]\nsockets = [\"ap.example.com:0\"]"),
            manifest(
                "[permissions]\nweb-capture = [\"example.com\"]\nweb-capture-observe = [\"*.example.com\"]",
            ),
            manifest("[auth]\nredirects = [\"loopback:80/x\"]"),
            manifest("[auth]\nredirects = [\"loopback:8990/a?b\"]"),
            manifest("[auth]\nredirects = [\"intent\"]"),
            manifest("[auth]\napp-schemes = [\"https\"]"),
            manifest("[auth]\napp-schemes = [\"intent\"]"),
            manifest("[auth]\napp-schemes = [\"1abc\"]"),
            manifest("[limits]\ntracks = 0"),
            manifest("[party]\nshare = [\"file\"]"),
            manifest("[party]\nshare = [\"password\"]"),
        ];
        for text in cases {
            assert!(
                matches!(parsed(&text), Err(PackageError::Manifest(_))),
                "accepted:\n{text}"
            );
        }
    }

    #[test]
    fn observed_hosts_may_be_covered_by_wildcards() {
        let manifest = parsed(&manifest(
            "[permissions]\nweb-capture = [\"*.example.com\"]\nweb-capture-observe = [\"login.example.com\", \"*.a.example.com\"]",
        ))
        .unwrap();
        assert_eq!(manifest.permissions.web_capture_observe.len(), 2);
    }

    #[test]
    fn an_empty_share_list_relays_references_only() {
        let manifest = parsed(&manifest("[party]\nshare = []")).unwrap();
        assert!(!manifest.party.shares(ShareKind::Track, "title"));
    }
}
