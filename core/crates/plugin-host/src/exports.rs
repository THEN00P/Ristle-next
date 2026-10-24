//! Typed wrappers for the exports, returning `ristle-domain` types. WIT
//! types end here, so no other crate sees generated bindings.
//!
//! Every result is checked before anything uses it: each string at most
//! 64 KiB and the whole result at most 4 MiB (`provider.wit`, `catalog`).
//! The check runs on the lifted values, which wasmtime copies out of the
//! instance first; that copy is bounded by the instance's 64 MiB of linear
//! memory.

use ristle_domain::{
    Artwork, AttributeValue, Availability, ConnectionId, MediaDate, MetadataAttribute, Named,
    PlaybackCapabilities, Track, TrackRef,
};

use crate::bindings::ristle::plugin::types as wit;
use crate::runtime::{CallError, CallKind, LoadedPlugin};

/// The longest string a Plugin may return.
pub const MAX_RESULT_STRING: usize = 64 * 1024;
/// The largest result a Plugin may return, counted as its string and byte
/// payloads.
pub const MAX_RESULT_BYTES: usize = 4 * 1024 * 1024;

/// A Plugin's `error`: why a call failed, as the Plugin or the host says.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PluginError {
    /// Credentials are missing, expired or rejected.
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    /// This item cannot be served; retrying will not help.
    #[error("unavailable: {0}")]
    Unavailable(String),
    /// The Provider asked to slow down; milliseconds to wait, when known.
    #[error("rate limited")]
    RateLimited(Option<u32>),
    /// No network, a timeout or a server error; the call may succeed later.
    #[error("transient: {0}")]
    Transient(String),
    /// The target changed under the call.
    #[error("conflict: {0}")]
    Conflict(String),
    /// Not offered for this item or connection.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// Anything else, including a bug in the Plugin, a trap or an exceeded
    /// budget.
    #[error("internal: {0}")]
    Internal(String),
}

impl From<wit::Error> for PluginError {
    fn from(error: wit::Error) -> Self {
        match error {
            wit::Error::Unauthorized(text) => PluginError::Unauthorized(text),
            wit::Error::Unavailable(text) => PluginError::Unavailable(text),
            wit::Error::RateLimited(wait) => PluginError::RateLimited(wait),
            wit::Error::Transient(text) => PluginError::Transient(text),
            wit::Error::Conflict(text) => PluginError::Conflict(text),
            wit::Error::Unsupported(text) => PluginError::Unsupported(text),
            wit::Error::Internal(text) => PluginError::Internal(text),
        }
    }
}

impl From<CallError> for PluginError {
    /// A call that failed in the host is `internal` to its caller: the
    /// Plugin trapped, ran over budget, or could not start.
    fn from(error: CallError) -> Self {
        PluginError::Internal(error.to_string())
    }
}

/// Counts what a result costs to keep, and refuses it past the bounds.
struct Budget {
    bytes: usize,
}

impl Budget {
    fn new() -> Self {
        Self { bytes: 0 }
    }

    fn text(&mut self, text: &str) -> Result<(), PluginError> {
        if text.len() > MAX_RESULT_STRING {
            return Err(PluginError::Internal(
                "the plugin returned a string over 64 KiB".to_owned(),
            ));
        }
        self.bytes(text.len())
    }

    fn bytes(&mut self, length: usize) -> Result<(), PluginError> {
        self.bytes += length;
        if self.bytes > MAX_RESULT_BYTES {
            return Err(PluginError::Internal(
                "the plugin returned more than 4 MiB".to_owned(),
            ));
        }
        Ok(())
    }
}

impl LoadedPlugin {
    /// Full Tracks for ids, in the same order, in batches of at most the
    /// manifest's `limits.tracks`. A Track that no longer exists is
    /// [`PluginError::Unavailable`] in its slot; a failed batch fails the
    /// whole call.
    pub async fn tracks(
        &self,
        connection: &ConnectionId,
        ids: Vec<String>,
    ) -> Result<Vec<Result<Track, PluginError>>, PluginError> {
        let batch = self.manifest().limits.tracks.unwrap_or(1).max(1) as usize;
        let mut out = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(batch) {
            let chunk = chunk.to_vec();
            let requested = chunk.len();
            let results = self
                .call(connection, CallKind::Normal, move |store, guests, conn| {
                    guests.catalog.call_tracks(store, conn, &chunk)
                })
                .await??;
            if results.len() != requested {
                return Err(PluginError::Internal(format!(
                    "asked for {requested} tracks, got {}",
                    results.len()
                )));
            }
            let mut budget = Budget::new();
            for result in results {
                out.push(match result {
                    Ok(track) => Ok(self.track(connection, track, &mut budget)?),
                    Err(error) => Err(PluginError::from(error)),
                });
            }
        }
        Ok(out)
    }

    /// Up to `length` bytes of a Plugin Byte Source from `offset`; fewer at
    /// the end of the stream. Runs under the shorter `byte-source` budget.
    pub async fn read_bytes(
        &self,
        connection: &ConnectionId,
        handle: String,
        offset: u64,
        length: u32,
    ) -> Result<Vec<u8>, PluginError> {
        let bytes = self
            .call(
                connection,
                CallKind::ByteSource,
                move |store, guests, conn| match &guests.byte_source {
                    Some(source) => source
                        .call_read(store, conn, &handle, offset, length)
                        .map(Some),
                    None => Ok(None),
                },
            )
            .await?
            .ok_or(CallError::NotExported("byte-source"))??;
        if bytes.len() > length as usize {
            return Err(PluginError::Internal(format!(
                "asked for {length} bytes, got {}",
                bytes.len()
            )));
        }
        Ok(bytes)
    }

    /// A WIT Track as a domain Track.
    fn track(
        &self,
        connection: &ConnectionId,
        track: wit::Track,
        budget: &mut Budget,
    ) -> Result<Track, PluginError> {
        for text in [&track.id, &track.title] {
            budget.text(text)?;
        }
        let reference = self.reference(connection, track.id)?;
        let mut out = Track::new(reference, track.title);
        out.artists = track
            .artists
            .into_iter()
            .map(|named| self.named(named, budget))
            .collect::<Result<_, _>>()?;
        out.album = track
            .album
            .map(|album| self.named(album, budget))
            .transpose()?;
        out.duration_ms = track.duration_ms.map(u64::from);
        out.artwork = self.artwork(track.artwork, budget)?;
        out.released = track.released.and_then(date);
        out.uploaded = track.uploaded.and_then(date);
        out.explicit = track.traits.contains(wit::TrackTraits::EXPLICIT);
        out.capabilities = PlaybackCapabilities {
            seekable: !track.traits.contains(wit::TrackTraits::UNSEEKABLE),
            live: track.traits.contains(wit::TrackTraits::LIVE),
        };
        out.availability = match track.unavailable {
            Some(reason) => {
                budget.text(&reason)?;
                Availability::Unavailable {
                    reason: (!reason.is_empty()).then_some(reason),
                }
            }
            None => Availability::Available,
        };
        out.url = self.link(track.url, budget)?;
        out.isrc = match track.isrc {
            Some(isrc) => {
                budget.text(&isrc)?;
                Some(isrc)
            }
            None => None,
        };
        out.attributes = track
            .attributes
            .into_iter()
            .map(|attribute| self.attribute(attribute, budget))
            .collect::<Result<_, _>>()?;
        Ok(out)
    }

    /// A Track reference. Items of a Plugin that may hold Server or Folder
    /// Grants are never public: their ids mean something only on the
    /// connection they came from.
    fn reference(&self, connection: &ConnectionId, id: String) -> Result<TrackRef, PluginError> {
        let manifest = self.manifest();
        let provider = manifest.provider.id.clone();
        let scoped = manifest.permissions.servers || manifest.permissions.folders;
        let reference = if scoped {
            TrackRef::server_scoped(provider, id, connection.clone())
        } else {
            TrackRef::catalog(provider, id)
        };
        reference.map_err(|error| PluginError::Internal(format!("invalid track: {error}")))
    }

    fn named(&self, named: wit::Named, budget: &mut Budget) -> Result<Named, PluginError> {
        budget.text(&named.name)?;
        if let Some(id) = &named.id {
            budget.text(id)?;
        }
        Ok(Named {
            id: named.id,
            name: named.name,
        })
    }

    /// Artwork the host may fetch: HTTPS on a host of `image-hosts`. Any
    /// other image is dropped, so it shows as a placeholder.
    fn artwork(
        &self,
        images: Vec<wit::Image>,
        budget: &mut Budget,
    ) -> Result<Vec<Artwork>, PluginError> {
        let hosts = &self.manifest().permissions.image_hosts;
        let mut out = Vec::new();
        for image in images {
            budget.text(&image.url)?;
            let allowed = url::Url::parse(&image.url).is_ok_and(|url| {
                url.scheme() == "https"
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url
                        .host_str()
                        .is_some_and(|host| hosts.iter().any(|pattern| pattern.matches(host)))
            });
            if allowed {
                out.push(Artwork {
                    url: image.url,
                    width: image.width,
                    height: image.height,
                });
            }
        }
        Ok(out)
    }

    /// A link the user may follow: HTTPS only, anything else is dropped.
    fn link(
        &self,
        url: Option<String>,
        budget: &mut Budget,
    ) -> Result<Option<String>, PluginError> {
        let Some(url) = url else {
            return Ok(None);
        };
        budget.text(&url)?;
        let https = url::Url::parse(&url)
            .is_ok_and(|parsed| parsed.scheme() == "https" && parsed.host_str().is_some());
        Ok(https.then_some(url))
    }

    /// A Metadata Attribute; keys outside the well-known ones are prefixed
    /// with the Provider id.
    fn attribute(
        &self,
        attribute: wit::Attribute,
        budget: &mut Budget,
    ) -> Result<MetadataAttribute, PluginError> {
        const WELL_KNOWN: [&str; 6] = ["genre", "bpm", "musical-key", "mood", "label", "copyright"];
        budget.text(&attribute.key)?;
        let provider = self.manifest().provider.id.as_str();
        let prefix = format!("{provider}:");
        let key =
            if WELL_KNOWN.contains(&attribute.key.as_str()) || attribute.key.starts_with(&prefix) {
                attribute.key
            } else {
                format!("{prefix}{}", attribute.key)
            };
        let value = match attribute.value {
            wit::AttributeValue::Text(text) => {
                budget.text(&text)?;
                AttributeValue::Text(text)
            }
            wit::AttributeValue::Number(number) => AttributeValue::Number(number),
            wit::AttributeValue::YesNo(yes) => AttributeValue::YesNo(yes),
        };
        Ok(MetadataAttribute { key, value })
    }
}

/// A Media Date, dropped when invalid rather than invented.
fn date(date: wit::MediaDate) -> Option<MediaDate> {
    MediaDate::new(date.year, date.month, date.day).ok()
}
