//! Ristle legacy's `storage_test.cpp` and the two storage cases of
//! `application_core_test.cpp`, ported to the Rust API. Each test keeps the
//! legacy assertions; where legacy set up state through the C++ queue or
//! sequence types, the state is built directly from `ristle-queue`'s state
//! types.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Dirs, detailed, disabled, reference, stored_sequence};
use ristle_domain::{Availability, PlaylistId, PluginId, RistlePlaylist, Track};
use ristle_queue::sequence::{SequenceId, SlotId};
use ristle_queue::{
    CollectionPosition, Current, EntryId, Origin, QueueEntry, QueueState, RepeatMode, Submitter,
};
use ristle_storage::repo::{
    CachedPage, Intent, LocalContent, LocalFile, PageKey, PlaybackSession, PluginRecord, Receipts,
    RecentlyPlayed,
};
use ristle_storage::{Error, OpenStatus, SCHEMA_VERSION, Storage};

fn entry(id: u64, track: Track, origin: Origin, submitter: Option<Submitter>) -> QueueEntry {
    QueueEntry {
        id: EntryId(id),
        track,
        origin,
        slot: None,
        submitter,
    }
}

fn at(position: u64) -> Origin {
    Origin::Collection {
        position: CollectionPosition(position),
    }
}

fn session(queue: QueueState) -> PlaybackSession {
    PlaybackSession {
        queue,
        sequence: None,
        position_ms: 0,
        intent: Intent::Paused,
    }
}

fn plugin(id: &str) -> PluginRecord {
    PluginRecord {
        id: PluginId::new(id),
        version: "1.0.0".into(),
        package_hash: "sha256:00".into(),
        publisher_key: "ed25519:00".into(),
        enabled: true,
        installed_ms: 1,
        updated_ms: 1,
    }
}

#[test]
fn storage_creates_current_schema() {
    let dirs = Dirs::new();
    let storage = dirs.open();

    assert_eq!(storage.status(), &OpenStatus::Ready);
    assert_eq!(storage.schema_version(), SCHEMA_VERSION);
    assert_eq!(dirs.user_version(), SCHEMA_VERSION);
}

#[test]
fn playback_session_restores_paused_with_exact_queue_state() {
    let dirs = Dirs::new();
    // Legacy started a two-Track collection, queued a request, advanced
    // into it and set repeat Track: the request plays detached.
    let queue = QueueState {
        collection: vec![
            entry(1, detailed("spotify", "playlist-track"), at(0), None),
            entry(2, detailed("soundcloud", "next-track"), at(1), None),
        ],
        cursor: Some(0),
        requests: vec![],
        play_next_count: 0,
        current: Some(Current::Detached(Box::new(entry(
            3,
            detailed("soundcloud", "request-track"),
            Origin::Request,
            None,
        )))),
        repeat: RepeatMode::Track,
        shuffle: Some(0xdead_beef_dead_beef),
        next_id: EntryId(4),
    };
    let saved = PlaybackSession {
        queue: queue.clone(),
        sequence: None,
        position_ms: 73_125,
        intent: Intent::Playing,
    };
    {
        let storage = dirs.open();
        let saved = saved.clone();
        storage
            .write_blocking(move |tx| tx.session().save(&saved))
            .unwrap();
    }

    let reopened = dirs.open();
    let restored = reopened
        .read_blocking(|tx| tx.session().load_for_cold_start())
        .unwrap()
        .unwrap();

    assert_eq!(restored.session.intent, Intent::Paused);
    assert!(restored.was_playing);
    assert_eq!(restored.session.position_ms, 73_125);
    assert_eq!(restored.session.queue.shuffle, Some(0xdead_beef_dead_beef));
    assert_eq!(restored.session.queue.repeat, RepeatMode::Track);
    let Some(Current::Detached(current)) = &restored.session.queue.current else {
        panic!("the request no longer plays detached");
    };
    assert_eq!(current.track.reference.provider().as_str(), "soundcloud");
    assert_eq!(current.track.reference.id(), "request-track");
    assert_eq!(current.track.artists.len(), 1);
    assert_eq!(current.track.artwork[0].width, Some(640));
    assert_eq!(current.track.attributes.len(), 3);
    // Exact: everything else round-trips too.
    assert_eq!(restored.session.queue, queue);
}

#[test]
fn queue_submitter_survives_the_party_that_named_it() {
    let dirs = Dirs::new();
    let queue = QueueState {
        collection: vec![entry(1, detailed("spotify", "host-track"), at(0), None)],
        cursor: Some(0),
        requests: vec![
            entry(
                2,
                detailed("soundcloud", "guest-request"),
                Origin::Request,
                Some(Submitter {
                    id: "7".into(),
                    display_name: "Ada".into(),
                }),
            ),
            // What the local listener queues carries no submitter at all.
            entry(3, detailed("spotify", "own-request"), Origin::Request, None),
        ],
        current: Some(Current::AtCursor),
        next_id: EntryId(4),
        ..QueueState::default()
    };
    {
        let storage = dirs.open();
        let saved = session(queue);
        storage
            .write_blocking(move |tx| tx.session().save(&saved))
            .unwrap();
    }

    let restored = dirs
        .open()
        .read_blocking(|tx| tx.session().load_for_cold_start())
        .unwrap()
        .unwrap();
    let requests = &restored.session.queue.requests;
    assert_eq!(requests.len(), 2);
    // The name is stored beside the id precisely so it still reads back once
    // the Party that could resolve that id is gone.
    let guest = requests[0].submitter.as_ref().unwrap();
    assert_eq!(guest.id, "7");
    assert_eq!(guest.display_name, "Ada");
    assert!(requests[1].submitter.is_none());
}

#[test]
fn ristle_playlist_preserves_mixed_and_unavailable_tracks() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    let playlist = RistlePlaylist {
        id: PlaylistId::new("mixed"),
        title: "Mixed Playlist".into(),
        tracks: vec![
            detailed("spotify", "first"),
            disabled("soundcloud", "disabled"),
            detailed("local", "third"),
        ],
        created_ms: 10,
        updated_ms: 20,
    };
    storage
        .write_blocking(move |tx| tx.playlists().save(&playlist))
        .unwrap();

    let restored = storage
        .read_blocking(|tx| tx.playlists().load(&PlaylistId::new("mixed")))
        .unwrap()
        .unwrap();
    assert_eq!(restored.tracks.len(), 3);
    assert_eq!(
        restored.tracks[1].availability,
        Availability::Unavailable {
            reason: Some("Not licensed in this market".into())
        }
    );
    assert_eq!(restored.tracks[2].reference.provider().as_str(), "local");

    let summaries = storage.read_blocking(|tx| tx.playlists().list()).unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].track_count, 3);
}

#[test]
fn recently_played_uses_duration_threshold_and_keeps_latest_hundred() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    let record = |track: Track, listened: u64, at: u64| {
        dirs.clock.set(at);
        storage
            .write_blocking(move |tx| tx.recently_played().record(&track, listened))
            .unwrap()
    };

    let short = Track {
        duration_ms: Some(20_000),
        ..detailed("local", "short")
    };
    assert!(!record(short.clone(), 9_999, 1));
    assert!(record(short, 10_000, 2));

    let mut live = detailed("youtube", "live");
    live.capabilities.live = true;
    live.duration_ms = None;
    assert!(!record(live.clone(), 29_999, 3));
    assert!(record(live, 30_000, 4));

    for index in 0..105 {
        assert!(record(
            detailed("spotify", &format!("track-{index}")),
            30_000,
            100 + index
        ));
    }

    let entries = storage
        .read_blocking(|tx| tx.recently_played().list())
        .unwrap();
    assert_eq!(entries.len() as u64, RecentlyPlayed::LIMIT);
    assert_eq!(entries[0].track.reference.id(), "track-104");
    assert_eq!(entries.last().unwrap().track.reference.id(), "track-5");
}

fn page_key(provider: &str, connection: &str, namespace: &str, key: &str) -> PageKey {
    PageKey {
        provider: provider.into(),
        connection: connection.into(),
        generation: 0,
        namespace: namespace.into(),
        key: key.into(),
    }
}

fn page(revision: Option<&str>, payload: &str, fetched_ms: u64) -> CachedPage {
    CachedPage {
        revision: revision.map(Into::into),
        payload: payload.as_bytes().to_vec(),
        fetched_ms,
    }
}

#[test]
fn clear_cache_preserves_user_owned_playlists() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    storage
        .write_blocking(|tx| {
            tx.playlists().save(&RistlePlaylist {
                id: PlaylistId::new("preserved"),
                title: "Preserved".into(),
                tracks: vec![detailed("spotify", "track")],
                created_ms: 0,
                updated_ms: 0,
            })?;
            tx.page_cache().put(
                &page_key("spotify", "primary-account", "playlist", "remote-playlist"),
                &page(Some("snapshot-1"), "cached payload", 100),
            )
        })
        .unwrap();

    storage.write_blocking(|tx| tx.clear_caches()).unwrap();

    let (playlist, cached) = storage
        .read_blocking(|tx| {
            Ok((
                tx.playlists().load(&PlaylistId::new("preserved"))?,
                tx.page_cache().get(&page_key(
                    "spotify",
                    "primary-account",
                    "playlist",
                    "remote-playlist",
                ))?,
            ))
        })
        .unwrap();
    assert!(playlist.is_some());
    assert!(cached.is_none());
}

#[test]
fn clear_provider_cache_only_removes_the_requested_namespace() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    storage
        .write_blocking(|tx| {
            let cache = tx.page_cache();
            cache.put(
                &page_key("soundcloud", "default", "provider-library", "tracks"),
                &page(None, "library", 100),
            )?;
            cache.put(
                &page_key("soundcloud", "default", "provider-playlist", "playlist"),
                &page(None, "playlist", 100),
            )?;
            cache.put(
                &page_key("spotify", "default", "provider-library", "tracks"),
                &page(None, "spotify-library", 100),
            )?;
            cache.clear_namespace(&"soundcloud".into(), &"default".into(), "provider-library")
        })
        .unwrap();

    let present = |key: PageKey| {
        storage
            .read_blocking(move |tx| tx.page_cache().get(&key))
            .unwrap()
            .is_some()
    };
    assert!(!present(page_key(
        "soundcloud",
        "default",
        "provider-library",
        "tracks"
    )));
    assert!(present(page_key(
        "soundcloud",
        "default",
        "provider-playlist",
        "playlist"
    )));
    assert!(present(page_key(
        "spotify",
        "default",
        "provider-library",
        "tracks"
    )));
}

#[test]
fn settings_persist_and_upsert_by_key() {
    let dirs = Dirs::new();
    {
        let storage = dirs.open();
        assert_eq!(
            storage
                .read_blocking(|tx| tx.settings().get("audio.volume"))
                .unwrap(),
            None
        );
        storage
            .write_blocking(|tx| {
                tx.settings().put("audio.volume", "0.42")?;
                tx.settings().put("audio.volume", "0.75")
            })
            .unwrap();
    }

    let reopened = dirs.open();
    assert_eq!(
        reopened
            .read_blocking(|tx| tx.settings().get("audio.volume"))
            .unwrap(),
        Some("0.75".into())
    );
}

fn local_connection(storage: &Storage) {
    storage
        .write_blocking(|tx| {
            tx.plugins().put(&plugin("local-files"))?;
            tx.connections()
                .create(&"local-library".into(), &PluginId::new("local-files"))?;
            Ok(())
        })
        .unwrap();
}

fn local_file(
    location: &str,
    format: &str,
    bitrate_kbps: u32,
    size_bytes: u64,
    modified_ms: u64,
) -> LocalFile {
    LocalFile {
        location: location.into(),
        content_id: "fingerprint-content".into(),
        format: format.into(),
        bitrate_kbps,
        size_bytes,
        modified_ms,
        available: true,
    }
}

#[test]
fn local_track_candidates_prefer_flac_then_bitrate_and_fall_back() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    local_connection(&storage);
    let content = LocalContent {
        content_id: "fingerprint-content".into(),
        fingerprint: "encoded-fingerprint".into(),
        fingerprint_hash: 1234,
        duration_ms: Some(240_000),
    };
    let track = detailed("local", "fingerprint-content");
    let mp3 = local_file("track.mp3", "mp3", 320, 100, 10);
    let flac = local_file("track.flac", "flac", 700, 200, 10);
    {
        let (content, track, mp3, flac) =
            (content.clone(), track.clone(), mp3.clone(), flac.clone());
        storage
            .write_blocking(move |tx| {
                let index = tx.local_index();
                index.upsert(&"local-library".into(), &content, &track, &mp3)?;
                index.upsert(&"local-library".into(), &content, &track, &flac)
            })
            .unwrap();
    }

    let best = || {
        storage
            .read_blocking(|tx| {
                tx.local_index()
                    .best_file(&"local-library".into(), "fingerprint-content")
            })
            .unwrap()
            .map(|file| file.location)
    };
    let tracks = || {
        storage
            .read_blocking(|tx| tx.local_index().tracks(&"local-library".into()))
            .unwrap()
    };
    assert_eq!(best(), Some(flac.location.clone()));
    assert_eq!(
        storage
            .read_blocking(|tx| tx.local_index().contents(&"local-library".into()))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(tracks().len(), 1);

    let (location, size, modified) = (mp3.location.clone(), mp3.size_bytes, mp3.modified_ms);
    let confirmed = storage
        .write_blocking(move |tx| {
            let index = tx.local_index();
            index.begin_scan(&"local-library".into())?;
            index.confirm_file(&"local-library".into(), &location, size, modified)
        })
        .unwrap();
    assert!(confirmed);
    assert_eq!(best(), Some(mp3.location.clone()));
    assert_eq!(tracks()[0].availability, Availability::Available);

    storage
        .write_blocking(|tx| tx.local_index().begin_scan(&"local-library".into()))
        .unwrap();
    assert_eq!(best(), None);
    assert!(matches!(
        tracks()[0].availability,
        Availability::Unavailable { .. }
    ));
}

#[test]
fn local_track_candidate_only_revalidates_when_file_state_matches() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    local_connection(&storage);
    let content = LocalContent {
        content_id: "fingerprint-content".into(),
        fingerprint: "fingerprint".into(),
        fingerprint_hash: 12,
        duration_ms: Some(30_000),
    };
    let file = local_file("candidate.flac", "flac", 1000, 500, 100);
    storage
        .write_blocking(move |tx| {
            let index = tx.local_index();
            index.upsert(
                &"local-library".into(),
                &content,
                &detailed("local", "fingerprint-content"),
                &file,
            )?;
            index.begin_scan(&"local-library".into())?;
            Ok(())
        })
        .unwrap();

    let confirm = |size: u64, modified: u64| {
        storage
            .write_blocking(move |tx| {
                tx.local_index().confirm_file(
                    &"local-library".into(),
                    "candidate.flac",
                    size,
                    modified,
                )
            })
            .unwrap()
    };
    assert!(!confirm(501, 100));
    assert!(confirm(500, 100));
}

#[test]
fn failed_migration_preserves_prior_database_and_enters_recovery() {
    let dirs = Dirs::new();
    // A view where schema v1 creates a table makes migration 1 fail.
    dirs.raw("CREATE VIEW settings AS SELECT 'preserved' AS marker");

    {
        let storage = dirs.open();
        let OpenStatus::Recovery { reason } = storage.status() else {
            panic!("the failed migration was not reported");
        };
        assert!(!reason.is_empty());
        assert!(matches!(
            storage.read_blocking(|tx| tx.settings().get("x")),
            Err(Error::Recovery { .. })
        ));
    }

    assert_eq!(dirs.user_version(), 0);
    let conn = rusqlite::Connection::open(dirs.paths.database()).unwrap();
    let marker: String = conn
        .query_row("SELECT marker FROM settings", [], |r| r.get(0))
        .unwrap();
    assert_eq!(marker, "preserved");
    let tables: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(tables, 0, "the failed migration left tables behind");
}

#[test]
fn newer_database_schema_enters_recovery_without_modification() {
    let dirs = Dirs::new();
    dirs.raw("PRAGMA user_version = 999");
    let before = std::fs::read(dirs.paths.database()).unwrap();

    let storage = dirs.open();

    assert!(matches!(storage.status(), OpenStatus::Recovery { .. }));
    assert_eq!(storage.schema_version(), 999);
    drop(storage);
    assert_eq!(dirs.user_version(), 999);
    assert_eq!(std::fs::read(dirs.paths.database()).unwrap(), before);
    assert!(
        !ristle_storage_wal(&dirs).exists(),
        "the newer database was switched to WAL"
    );
}

fn ristle_storage_wal(dirs: &Dirs) -> std::path::PathBuf {
    let mut wal = dirs.paths.database().into_os_string();
    wal.push("-wal");
    wal.into()
}

#[test]
fn playable_sequence_manifest_and_exact_order_round_trip() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    // Shuffled with seed 42, as legacy's `StartShuffled(manifest, 42)`.
    let mut sequence = stored_sequence("playlist:large");
    sequence.order = vec![SlotId(2), SlotId(0), SlotId(1)];
    sequence.shuffle = Some(42);
    let saved = sequence.clone();
    storage
        .write_blocking(move |tx| tx.sequences().save(&saved))
        .unwrap();

    let restored = storage
        .read_blocking(|tx| tx.sequences().load(&SequenceId("playlist:large".into())))
        .unwrap()
        .unwrap();
    assert_eq!(restored.slots.len(), 3);
    assert_eq!(restored.slots[0].track, restored.slots[1].track);
    assert_eq!(restored.slots[2].track, None);
    assert_eq!(restored.slots[2].offset, 2);
    assert_eq!(restored.order, sequence.order);
    assert_eq!(restored.shuffle, Some(42));
    assert_eq!(restored, sequence);
}

#[test]
fn sequence_progress_does_not_rewrite_unchanged_occurrence_order() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    let mut sequence = stored_sequence("playlist:large");
    let saved = sequence.clone();
    storage
        .write_blocking(move |tx| tx.sequences().save(&saved))
        .unwrap();
    dirs.raw(
        "CREATE TRIGGER forbid_slot_delete BEFORE DELETE ON sequence_slots
             BEGIN SELECT RAISE(ABORT, 'unchanged order deleted'); END;
         CREATE TRIGGER forbid_slot_insert BEFORE INSERT ON sequence_slots
             BEGIN SELECT RAISE(ABORT, 'unchanged order inserted'); END;
         CREATE TRIGGER forbid_slot_update BEFORE UPDATE ON sequence_slots
             BEGIN SELECT RAISE(ABORT, 'unchanged order updated'); END;",
    );

    // Advance and materialize the rest.
    sequence.cursor = Some(1);
    sequence.materialized_end = 3;
    let saved = sequence.clone();
    let changes = storage
        .write_blocking(move |tx| tx.sequences().save(&saved))
        .unwrap();
    assert_eq!(changes.updated, 1, "only the progress row is written");
    assert_eq!(changes.inserted + changes.deleted, 0);

    let restored = dirs
        .open()
        .read_blocking(|tx| tx.sequences().load(&SequenceId("playlist:large".into())))
        .unwrap()
        .unwrap();
    assert_eq!(restored.cursor, Some(1));
    assert_eq!(restored.materialized_end, 3);
    assert_eq!(restored.order, sequence.order);
}

#[test]
fn deleting_playable_sequence_manifest_cascades_its_order() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    let id = SequenceId("playlist:large".into());
    storage
        .write_blocking(|tx| tx.sequences().save(&stored_sequence("playlist:large")))
        .unwrap();

    let deleted = {
        let id = id.clone();
        storage
            .write_blocking(move |tx| tx.sequences().delete(&id))
            .unwrap()
    };
    assert!(deleted);
    assert_eq!(
        storage
            .read_blocking(move |tx| tx.sequences().load(&id))
            .unwrap(),
        None
    );
    let conn = rusqlite::Connection::open(dirs.paths.database()).unwrap();
    for table in ["sequence_routes", "sequence_slots", "sequence_progress"] {
        let rows: i64 = conn
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0, "{table} kept rows of a deleted sequence");
    }
}

#[test]
fn frozen_sequence_routes_and_incremental_progress_survive_restart_atomically() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    let mut sequence = stored_sequence("playlist:large");
    let saved = sequence.clone();
    storage
        .write_blocking(move |tx| tx.sequences().save(&saved))
        .unwrap();
    dirs.raw(
        "CREATE TRIGGER freeze_routes BEFORE DELETE ON sequence_routes
             BEGIN SELECT RAISE(ABORT, 'route rewritten'); END;
         CREATE TRIGGER freeze_route_updates BEFORE UPDATE ON sequence_routes
             BEGIN SELECT RAISE(ABORT, 'route rewritten'); END;
         CREATE TRIGGER freeze_slots BEFORE DELETE ON sequence_slots
             BEGIN SELECT RAISE(ABORT, 'unchanged slot removed'); END;",
    );

    // Progress moves on, and the positional slot is read.
    sequence.materialized_end = 2;
    sequence.slots[2].track = Some(reference("spotify", "third"));
    let saved = sequence.clone();
    storage
        .write_blocking(move |tx| tx.sequences().save(&saved))
        .unwrap();

    let id = SequenceId("playlist:large".into());
    let reopened = dirs.open();
    let restored = {
        let id = id.clone();
        reopened
            .read_blocking(move |tx| tx.sequences().load(&id))
            .unwrap()
            .unwrap()
    };
    assert_eq!(restored.descriptor.sources.len(), 1);
    assert_eq!(restored.slots.len(), 3);
    assert_eq!(restored.slots[0].track, restored.slots[1].track);
    assert_eq!(restored.slots[2].track, Some(reference("spotify", "third")));
    assert_eq!(restored.materialized_end, 2);

    // A failure after the progress write rolls back everything the closure
    // wrote.
    let mut updated = sequence.clone();
    updated.cursor = Some(1);
    let result = reopened.write_blocking(move |tx| {
        tx.sequences().save(&updated)?;
        tx.settings().put("transaction-marker", "must roll back")?;
        Err::<(), _>(Error::Aborted {
            reason: "injected failure after progress write".into(),
        })
    });
    assert!(matches!(result, Err(Error::Aborted { .. })));
    let (restored, marker) = reopened
        .read_blocking(move |tx| {
            Ok((
                tx.sequences().load(&id)?,
                tx.settings().get("transaction-marker")?,
            ))
        })
        .unwrap();
    assert_eq!(restored.unwrap().cursor, Some(0));
    assert_eq!(marker, None);
}

/// Legacy's `SavePlaybackState(session, order, manifest)`: the session and
/// its sequence in one transaction, dropping the sequence it replaces.
fn save_playback_state(
    storage: &Storage,
    session: &PlaybackSession,
    sequence: Option<ristle_queue::SequenceState>,
) -> Result<(), Error> {
    let session = session.clone();
    storage.write_blocking(move |tx| {
        let previous = tx
            .session()
            .load_for_cold_start()?
            .and_then(|restored| restored.session.sequence);
        if let Some(sequence) = &sequence {
            tx.sequences().save(sequence)?;
        }
        tx.session().save(&session)?;
        if let Some(previous) = previous
            && Some(&previous) != session.sequence.as_ref()
        {
            tx.sequences().delete(&previous)?;
        }
        Ok(())
    })
}

fn sequence_session(sequence: Option<&str>, position_ms: u64) -> PlaybackSession {
    PlaybackSession {
        sequence: sequence.map(|id| SequenceId(id.into())),
        position_ms,
        ..session(QueueState::default())
    }
}

#[test]
fn playback_state_failure_rolls_back_replacement_manifest_and_exact_order() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    let original = stored_sequence("manifest-1");
    save_playback_state(
        &storage,
        &sequence_session(Some("manifest-1"), 1),
        Some(original.clone()),
    )
    .unwrap();
    dirs.raw(
        "CREATE TRIGGER fail_playback BEFORE UPDATE ON playback_session
             BEGIN SELECT RAISE(ABORT, 'injected write failure'); END;",
    );

    let mut replacement = stored_sequence("replacement");
    replacement.cursor = Some(1);
    replacement.materialized_end = 2;
    let replacing = sequence_session(Some("replacement"), 2);
    assert!(save_playback_state(&storage, &replacing, Some(replacement.clone())).is_err());

    {
        let reopened = dirs.open();
        let (session, kept, replaced) = reopened
            .read_blocking(|tx| {
                Ok((
                    tx.session().load_for_cold_start()?.unwrap().session,
                    tx.sequences().load(&SequenceId("manifest-1".into()))?,
                    tx.sequences().load(&SequenceId("replacement".into()))?,
                ))
            })
            .unwrap();
        assert_eq!(session.position_ms, 1);
        assert_eq!(session.sequence, Some(SequenceId("manifest-1".into())));
        assert_eq!(kept.unwrap().cursor, original.cursor);
        assert_eq!(replaced, None);
    }

    dirs.raw("DROP TRIGGER fail_playback");
    save_playback_state(&storage, &replacing, Some(replacement.clone())).unwrap();
    let (kept, replaced) = storage
        .read_blocking(|tx| {
            Ok((
                tx.sequences().load(&SequenceId("manifest-1".into()))?,
                tx.sequences().load(&SequenceId("replacement".into()))?,
            ))
        })
        .unwrap();
    assert_eq!(kept, None);
    assert_eq!(replaced.unwrap().cursor, replacement.cursor);

    save_playback_state(&storage, &sequence_session(None, 3), None).unwrap();
    assert_eq!(
        storage
            .read_blocking(|tx| tx.sequences().load(&SequenceId("replacement".into())))
            .unwrap(),
        None
    );
}

#[test]
fn playback_state_failure_does_not_advance_durable_sequence_order() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    let original = stored_sequence("manifest-1");
    let mut advanced = original.clone();
    advanced.cursor = Some(1);
    advanced.materialized_end = 2;
    let session = sequence_session(Some("manifest-1"), 0);
    save_playback_state(&storage, &session, Some(original.clone())).unwrap();
    dirs.raw(
        "CREATE TRIGGER fail_playback BEFORE UPDATE ON playback_session
             BEGIN SELECT RAISE(ABORT, 'injected write failure'); END;",
    );

    assert!(save_playback_state(&storage, &session, Some(advanced)).is_err());
    let stored = storage
        .read_blocking(|tx| tx.sequences().load(&SequenceId("manifest-1".into())))
        .unwrap()
        .unwrap();
    assert_eq!(stored.cursor, original.cursor);
}

#[test]
fn playback_receipt_limit_rejects_new_commands_without_evicting_dedupe_history() {
    let dirs = Dirs::new();
    let storage = dirs.open();
    dirs.raw(&format!(
        "WITH RECURSIVE numbers(n) AS (VALUES(1) UNION ALL SELECT n + 1 FROM numbers WHERE n < {})
         INSERT INTO command_receipts (scope, key, outcome, created_ms)
         SELECT 'party-1', n, 'applied', 0 FROM numbers;",
        Receipts::LIMIT
    ));

    let result = storage.write_blocking(|tx| tx.receipts().put("party-1", "new", "applied"));
    assert!(matches!(result, Err(Error::ReceiptLimit { .. })));
    let get = |scope: &'static str, key: &'static str| {
        storage
            .read_blocking(move |tx| tx.receipts().get(scope, key))
            .unwrap()
    };
    assert_eq!(get("party-1", "1"), Some("applied".into()));
    assert_eq!(get("party-1", "10000"), Some("applied".into()));
    assert_eq!(get("party-1", "new"), None);

    storage
        .write_blocking(|tx| {
            tx.receipts().clear_scope("party-1")?;
            tx.receipts().put("party-2", "1", "rejected")
        })
        .unwrap();
    assert_eq!(get("party-2", "1"), Some("rejected".into()));
}

#[test]
fn provider_page_cache_entries_survive_reopen() {
    let dirs = Dirs::new();
    {
        let storage = dirs.open();
        storage
            .write_blocking(|tx| {
                tx.page_cache().put(
                    &page_key("spotify", "default", "provider-playlist", "playlist:abc"),
                    &page(Some("snapshot-1"), r#"{"tracks":["one"]}"#, 1234),
                )
            })
            .unwrap();
    }

    let cached = dirs
        .open()
        .read_blocking(|tx| {
            tx.page_cache().get(&page_key(
                "spotify",
                "default",
                "provider-playlist",
                "playlist:abc",
            ))
        })
        .unwrap()
        .unwrap();
    assert_eq!(cached.revision.as_deref(), Some("snapshot-1"));
    assert_eq!(cached.payload, br#"{"tracks":["one"]}"#);
    assert_eq!(cached.fetched_ms, 1234);
}

#[test]
fn plugin_permission_grants_survive_reopen() {
    let dirs = Dirs::new();
    {
        let storage = dirs.open();
        storage
            .write_blocking(|tx| {
                tx.plugins().put(&plugin("ristle.spotify"))?;
                tx.grants().set_permissions(
                    &PluginId::new("ristle.spotify"),
                    &["credentials:spotify".into(), "network:spotify".into()],
                )
            })
            .unwrap();
    }

    let grants = dirs
        .open()
        .read_blocking(|tx| tx.grants().all_permissions())
        .unwrap();
    assert_eq!(grants.len(), 1);
    assert_eq!(grants[0].0.as_str(), "ristle.spotify");
    assert_eq!(grants[0].1.len(), 2);
    assert_eq!(grants[0].1[0], "credentials:spotify");
    assert_eq!(grants[0].1[1], "network:spotify");
}
