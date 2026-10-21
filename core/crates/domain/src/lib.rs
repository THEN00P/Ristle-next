//! Domain types shared by every core crate.
//!
//! This crate holds the vocabulary of `CONTEXT.md` as plain data: Track
//! identity ([`TrackRef`]: Provider, Provider-local id and, for Server Grants
//! only, the Provider Connection), [`Track`] with its playback capabilities
//! and availability, [`MediaDate`] with its original precision, artwork,
//! Ristle Playlists, collection descriptors (offset or cursor addressing,
//! revision, total) and the injected [`Clock`].
//!
//! It has no I/O, no async and no dependency on another core crate. Anything
//! that needs a database, the network or a runtime belongs elsewhere.
//!
//! Every type is `serde`-serializable. Types with invariants ([`TrackRef`],
//! [`MediaDate`]) check them on deserialization too, so a corrupt row or
//! message cannot produce a value the constructors would refuse.

mod clock;
mod collection;
mod ids;
mod media_date;
mod playlist;
mod track;

#[cfg(any(test, feature = "test-support"))]
pub use clock::ManualClock;
pub use clock::{Clock, SystemClock};
pub use collection::{Addressing, CollectionDescriptor, CollectionRef, Count};
pub use ids::{ConnectionId, PluginId, ProviderId};
pub use media_date::{DatePrecision, InvalidMediaDate, MediaDate};
pub use playlist::{PlaylistId, RistlePlaylist};
pub use track::{
    Artwork, AttributeValue, Availability, InvalidTrackRef, MetadataAttribute, Named,
    PlaybackCapabilities, Track, TrackRef,
};

/// Every error this crate returns.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// A Track identity is missing a part.
    #[error(transparent)]
    InvalidTrackRef(#[from] InvalidTrackRef),
    /// A Media Date names a month or day that does not exist.
    #[error(transparent)]
    InvalidMediaDate(#[from] InvalidMediaDate),
}
