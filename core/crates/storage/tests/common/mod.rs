//! Helpers shared by the storage integration tests.

#![allow(dead_code)]

use std::path::Path;
use std::sync::Arc;

use ristle_domain::{
    Addressing, Artwork, AttributeValue, Availability, CollectionDescriptor, CollectionRef, Count,
    ManualClock, MediaDate, MetadataAttribute, Named, PlaybackCapabilities, Track, TrackRef,
};
use ristle_queue::SequenceState;
use ristle_queue::sequence::{
    Opened, SequenceDescriptor, SequenceId, SequenceSource, Slot, SlotId, Target,
};
use ristle_storage::{Storage, StoragePaths};
use tempfile::TempDir;

/// A data and a cache directory that disappear with the test.
pub(crate) struct Dirs {
    pub(crate) root: TempDir,
    pub(crate) paths: StoragePaths,
    pub(crate) clock: Arc<ManualClock>,
}

impl Dirs {
    pub(crate) fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let paths = StoragePaths::new(root.path().join("data"), root.path().join("cache"));
        Self {
            root,
            paths,
            clock: Arc::new(ManualClock::new(1_000)),
        }
    }

    pub(crate) fn open(&self) -> Storage {
        Storage::open(&self.paths, self.clock.clone()).unwrap()
    }

    /// Runs SQL on `ristle.sqlite` through a connection of its own, as
    /// another process would.
    pub(crate) fn raw(&self, sql: &str) {
        raw(&self.paths.database(), sql);
    }

    pub(crate) fn user_version(&self) -> u32 {
        let conn = rusqlite::Connection::open(self.paths.database()).unwrap();
        conn.pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap()
    }
}

pub(crate) fn raw(path: &Path, sql: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(sql).unwrap();
}

pub(crate) fn reference(provider: &str, id: &str) -> TrackRef {
    TrackRef::catalog(provider.into(), id.into()).unwrap()
}

/// A Track with every field set, as legacy's `DetailedTrack`.
pub(crate) fn detailed(provider: &str, id: &str) -> Track {
    Track {
        artists: vec![Named {
            id: Some("artist-1".into()),
            name: "Stored Artist".into(),
        }],
        album: Some(Named {
            id: Some("album-1".into()),
            name: "Stored Album".into(),
        }),
        duration_ms: Some(240_000),
        artwork: vec![Artwork {
            url: "https://example.test/artwork.jpg".into(),
            width: Some(640),
            height: Some(640),
        }],
        released: Some(MediaDate::new(2024, Some(5), None).unwrap()),
        uploaded: Some(MediaDate::new(2024, Some(5), Some(12)).unwrap()),
        explicit: true,
        capabilities: PlaybackCapabilities {
            seekable: true,
            live: false,
        },
        availability: Availability::Available,
        url: Some(format!("https://example.test/{provider}/{id}")),
        isrc: Some("USRC17607839".into()),
        attributes: vec![
            MetadataAttribute {
                key: "spotify:danceability".into(),
                value: AttributeValue::Number(0.82),
            },
            MetadataAttribute {
                key: "mood".into(),
                value: AttributeValue::Text("focused".into()),
            },
            MetadataAttribute {
                key: "ristle.test:featured".into(),
                value: AttributeValue::YesNo(true),
            },
        ],
        ..Track::new(reference(provider, id), "Stored Track")
    }
}

pub(crate) fn disabled(provider: &str, id: &str) -> Track {
    Track {
        availability: Availability::Unavailable {
            reason: Some("Not licensed in this market".into()),
        },
        ..detailed(provider, id)
    }
}

/// Legacy's `StoredSequenceManifest`: a revisioned, offset-addressed
/// playlist of three occurrences, the first two the same Track and the
/// third not read yet. Natural order, before the first slot.
pub(crate) fn stored_sequence(id: &str) -> SequenceState {
    let source = SequenceSource::Collection(CollectionDescriptor {
        reference: CollectionRef {
            connection: "account-1".into(),
            id: "playlist:playlist-1".into(),
        },
        addressing: Addressing::Offset,
        total: Some(Count::Exact(3)),
        revision: Some("snapshot-1".into()),
        endless: false,
    });
    let slot = |n: u64, track: Option<TrackRef>| Slot {
        id: SlotId(n),
        source: 0,
        offset: n,
        track,
    };
    SequenceState {
        descriptor: SequenceDescriptor {
            id: SequenceId(id.into()),
            title: "Large playlist".into(),
            opened: Opened::Provider {
                connection: "account-1".into(),
                target: Target::Playlist("playlist-1".into()),
            },
            sources: vec![source],
        },
        slots: vec![
            slot(0, Some(reference("spotify", "same"))),
            slot(1, Some(reference("spotify", "same"))),
            slot(2, None),
        ],
        order: vec![SlotId(0), SlotId(1), SlotId(2)],
        cursor: Some(0),
        materialized_end: 1,
        shuffle: None,
        revision: 0,
        next_slot: SlotId(3),
    }
}
