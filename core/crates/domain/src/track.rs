//! Tracks and their identity.

use serde::{Deserialize, Serialize};

use crate::ids::{ConnectionId, ProviderId};
use crate::media_date::MediaDate;

/// Why a [`TrackRef`] could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidTrackRef {
    /// The Provider id is empty.
    #[error("a Track identity needs a Provider id")]
    EmptyProvider,
    /// The Provider's Track id is empty.
    #[error("a Track identity needs the Provider's Track id")]
    EmptyId,
    /// A server-scoped Track names an empty Provider Connection.
    #[error("a server-scoped Track identity needs a Provider Connection id")]
    EmptyConnection,
}

/// Track identity: which exact Provider item a Track is.
///
/// `connection` is set only for Providers whose ids are server-scoped (Server
/// Grants), because a Jellyfin id means nothing on another server. Catalog
/// Tracks compare without it: the same Spotify Track listed through two
/// accounts is one Track. Another copy of the same song from a different
/// Provider is a different Track (`CONTEXT.md`, Track).
///
/// Equality and hashing are identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "TrackRefRepr", into = "TrackRefRepr")]
pub struct TrackRef {
    provider: ProviderId,
    id: String,
    connection: Option<ConnectionId>,
}

impl TrackRef {
    /// A Track from a Provider's catalog, the same for every Provider
    /// Connection.
    pub fn catalog(provider: ProviderId, id: String) -> Result<Self, InvalidTrackRef> {
        Self::checked(provider, id, None)
    }

    /// A Track whose id is only meaningful on the server one Provider
    /// Connection reaches (a Server Grant).
    pub fn server_scoped(
        provider: ProviderId,
        id: String,
        connection: ConnectionId,
    ) -> Result<Self, InvalidTrackRef> {
        Self::checked(provider, id, Some(connection))
    }

    fn checked(
        provider: ProviderId,
        id: String,
        connection: Option<ConnectionId>,
    ) -> Result<Self, InvalidTrackRef> {
        if provider.is_empty() {
            return Err(InvalidTrackRef::EmptyProvider);
        }
        if id.is_empty() {
            return Err(InvalidTrackRef::EmptyId);
        }
        if connection.as_ref().is_some_and(ConnectionId::is_empty) {
            return Err(InvalidTrackRef::EmptyConnection);
        }
        Ok(Self {
            provider,
            id,
            connection,
        })
    }

    /// The Provider the Track belongs to.
    pub fn provider(&self) -> &ProviderId {
        &self.provider
    }

    /// The Provider's own id for the Track. Ristle never parses it.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The Provider Connection a server-scoped id belongs to; `None` for
    /// catalog Tracks.
    pub fn connection(&self) -> Option<&ConnectionId> {
        self.connection.as_ref()
    }

    /// One string that is equal for two references exactly when the
    /// references are equal, for use as a single storage or map key. Each
    /// part is length-prefixed, so no id can be crafted to collide with
    /// another.
    pub fn identity_key(&self) -> String {
        let mut key = String::new();
        for part in [Some(self.provider.as_str()), Some(self.id.as_str())]
            .into_iter()
            .chain([self.connection.as_ref().map(ConnectionId::as_str)])
            .flatten()
        {
            key.push_str(&part.len().to_string());
            key.push(':');
            key.push_str(part);
        }
        key
    }
}

/// The serialized form of a [`TrackRef`], checked on the way in.
#[derive(Serialize, Deserialize)]
struct TrackRefRepr {
    provider: ProviderId,
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    connection: Option<ConnectionId>,
}

impl TryFrom<TrackRefRepr> for TrackRef {
    type Error = InvalidTrackRef;

    fn try_from(repr: TrackRefRepr) -> Result<Self, Self::Error> {
        Self::checked(repr.provider, repr.id, repr.connection)
    }
}

impl From<TrackRef> for TrackRefRepr {
    fn from(reference: TrackRef) -> Self {
        Self {
            provider: reference.provider,
            id: reference.id,
            connection: reference.connection,
        }
    }
}

/// An artist, user or album by name, with the Provider's id when it has one.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Named {
    /// The Provider's id, when the Provider gives one.
    pub id: Option<String>,
    /// The name as the Provider shows it.
    pub name: String,
}

/// One rendition of an image. `width` and `height` are hints for choosing a
/// rendition and are never trusted.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Artwork {
    /// Where the image is fetched from.
    pub url: String,
    /// Width in pixels, as the Provider claims it.
    pub width: Option<u32>,
    /// Height in pixels, as the Provider claims it.
    pub height: Option<u32>,
}

/// What the player may do with a Track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlaybackCapabilities {
    /// Seeking is possible. On-demand Tracks are seekable by default.
    pub seekable: bool,
    /// A Live Track: its end is unknown or advancing, so end-of-Track
    /// preparation cannot be relied on.
    pub live: bool,
}

impl Default for PlaybackCapabilities {
    fn default() -> Self {
        Self {
            seekable: true,
            live: false,
        }
    }
}

/// Whether a Track can play.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Availability {
    /// The Provider says it can play.
    Available,
    /// A Disabled Track: known to be unavailable, or its Provider Plugin is
    /// disabled or absent. It keeps its cached metadata but cannot enter the
    /// Canonical Queue.
    Unavailable {
        /// Why, as the Provider or the host explains it, when known.
        reason: Option<String>,
    },
    /// Not known yet. Such a Track may be queued; playback finds out.
    #[default]
    Unknown,
}

/// A Metadata Attribute: an optional Provider-supplied fact that Ristle may
/// show but never requires. Well-known keys are `genre`, `bpm`,
/// `musical-key`, `mood`, `label` and `copyright`; any other key is prefixed
/// with the Provider id, such as `soundcloud:plays`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetadataAttribute {
    /// What the value means.
    pub key: String,
    /// The value.
    pub value: AttributeValue,
}

/// The value of a [`MetadataAttribute`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AttributeValue {
    /// Free text.
    Text(String),
    /// A number, such as a tempo.
    Number(f64),
    /// A yes-or-no fact.
    YesNo(bool),
}

/// The exact Provider item a user selected, with what Ristle knows about it.
///
/// Optional facts are `None` when unknown, never zero.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    /// Which item this is.
    pub reference: TrackRef,
    /// The title as the Provider shows it.
    pub title: String,
    /// Artists in the Provider's order.
    pub artists: Vec<Named>,
    /// The album, when the Track is on one.
    pub album: Option<Named>,
    /// Length in milliseconds, when known. Live Tracks usually have none.
    pub duration_ms: Option<u64>,
    /// Renditions of the Track's artwork.
    pub artwork: Vec<Artwork>,
    /// Release date with its original precision.
    pub released: Option<MediaDate>,
    /// Upload date with its original precision.
    pub uploaded: Option<MediaDate>,
    /// The Provider marks it as explicit.
    pub explicit: bool,
    /// What the player may do with it.
    pub capabilities: PlaybackCapabilities,
    /// Whether it can play.
    pub availability: Availability,
    /// The Track's page on the Provider, for "Open on <Provider>" and
    /// sharing.
    pub url: Option<String>,
    /// Authoritative recording id, used to group equivalent search results.
    /// Never used to substitute one Track for another.
    pub isrc: Option<String>,
    /// Optional Provider facts.
    pub attributes: Vec<MetadataAttribute>,
}

impl Track {
    /// A Track known only by identity and title: no artists, artwork or
    /// dates, seekable, availability unknown. Fill in the rest with struct
    /// update syntax.
    pub fn new(reference: TrackRef, title: impl Into<String>) -> Self {
        Self {
            reference,
            title: title.into(),
            artists: Vec::new(),
            album: None,
            duration_ms: None,
            artwork: Vec::new(),
            released: None,
            uploaded: None,
            explicit: false,
            capabilities: PlaybackCapabilities::default(),
            availability: Availability::Unknown,
            url: None,
            isrc: None,
            attributes: Vec::new(),
        }
    }

    /// Whether the Track may enter the Canonical Queue. A Disabled Track may
    /// not; a Track whose availability is unknown may.
    pub fn is_queueable(&self) -> bool {
        !matches!(self.availability, Availability::Unavailable { .. })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn catalog(provider: &str, id: &str) -> TrackRef {
        TrackRef::catalog(provider.into(), id.into()).unwrap()
    }

    // Legacy carried the listing connection on every reference and compared
    // without it. Here a catalog reference has no connection to begin with,
    // so the same Track listed through two accounts builds one identity.
    #[test]
    fn track_identity_ignores_provider_connection() {
        let through_account_a = catalog("spotify", "track-1");
        let through_account_b = catalog("spotify", "track-1");

        assert_eq!(through_account_a, through_account_b);
        assert_eq!(
            through_account_a.identity_key(),
            through_account_b.identity_key()
        );
        assert_eq!(through_account_a.connection(), None);
        let set: HashSet<_> = [through_account_a, through_account_b].into();
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn track_identity_keeps_providers_separate() {
        let spotify = catalog("spotify", "track-1");
        let soundcloud = catalog("soundcloud", "track-1");

        assert_ne!(spotify, soundcloud);
        assert_ne!(spotify.identity_key(), soundcloud.identity_key());
    }

    #[test]
    fn invalid_track_identity_is_rejected() {
        assert_eq!(
            TrackRef::catalog("".into(), "track-1".into()),
            Err(InvalidTrackRef::EmptyProvider)
        );
        assert_eq!(
            TrackRef::catalog("spotify".into(), String::new()),
            Err(InvalidTrackRef::EmptyId)
        );
        assert_eq!(
            TrackRef::server_scoped("jellyfin".into(), "item-1".into(), "".into()),
            Err(InvalidTrackRef::EmptyConnection)
        );
    }

    #[test]
    fn server_scoped_identity_keeps_connections_separate() {
        let home =
            TrackRef::server_scoped("jellyfin".into(), "item-1".into(), "home".into()).unwrap();
        let friend =
            TrackRef::server_scoped("jellyfin".into(), "item-1".into(), "friend".into()).unwrap();
        let unscoped = catalog("jellyfin", "item-1");

        assert_ne!(home, friend);
        assert_ne!(home, unscoped);
        assert_ne!(home.identity_key(), friend.identity_key());
        assert_ne!(home.identity_key(), unscoped.identity_key());
    }

    #[test]
    fn identity_key_cannot_be_forged_by_shifting_parts() {
        // Without length prefixes both would read "ab" + "c".
        assert_ne!(
            catalog("ab", "c").identity_key(),
            catalog("a", "bc").identity_key()
        );
        // A catalog id that spells out a connection is not that connection.
        let scoped = TrackRef::server_scoped("p".into(), "i".into(), "c".into()).unwrap();
        let forged = catalog("p", "i1:c");
        assert_ne!(scoped.identity_key(), forged.identity_key());
    }

    #[test]
    fn track_ref_round_trips_and_rejects_invalid_input() {
        let scoped =
            TrackRef::server_scoped("jellyfin".into(), "item-1".into(), "home".into()).unwrap();
        let json = serde_json::to_string(&scoped).unwrap();
        assert_eq!(serde_json::from_str::<TrackRef>(&json).unwrap(), scoped);

        let json = serde_json::to_string(&catalog("spotify", "t")).unwrap();
        assert_eq!(json, r#"{"provider":"spotify","id":"t"}"#);

        assert!(serde_json::from_str::<TrackRef>(r#"{"provider":"","id":"t"}"#).is_err());
        assert!(
            serde_json::from_str::<TrackRef>(r#"{"provider":"p","id":"t","connection":""}"#)
                .is_err()
        );
    }

    #[test]
    fn only_unavailable_tracks_are_not_queueable() {
        let mut track = Track::new(catalog("spotify", "t"), "Title");
        assert!(track.is_queueable());
        track.availability = Availability::Available;
        assert!(track.is_queueable());
        track.availability = Availability::Unavailable { reason: None };
        assert!(!track.is_queueable());
    }

    #[test]
    fn track_round_trips_through_serde() {
        let track = Track {
            artists: vec![Named {
                id: Some("a1".into()),
                name: "Artist".into(),
            }],
            album: Some(Named {
                id: None,
                name: "Album".into(),
            }),
            duration_ms: Some(215_000),
            artwork: vec![Artwork {
                url: "https://i.example/a.jpg".into(),
                width: Some(640),
                height: None,
            }],
            released: Some(MediaDate::new(2019, Some(2), None).unwrap()),
            explicit: true,
            capabilities: PlaybackCapabilities {
                seekable: false,
                live: true,
            },
            availability: Availability::Unavailable {
                reason: Some("Region".into()),
            },
            isrc: Some("USRC17607839".into()),
            attributes: vec![
                MetadataAttribute {
                    key: "bpm".into(),
                    value: AttributeValue::Number(128.5),
                },
                MetadataAttribute {
                    key: "soundcloud:downloadable".into(),
                    value: AttributeValue::YesNo(true),
                },
            ],
            ..Track::new(catalog("soundcloud", "42"), "Title")
        };
        let json = serde_json::to_string(&track).unwrap();
        assert_eq!(serde_json::from_str::<Track>(&json).unwrap(), track);
    }
}
