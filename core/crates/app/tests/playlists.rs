//! Ristle Playlists as the queue plays them (a sequence with a bounded
//! window), edits to the one that plays, and Recently Played. Ported from
//! Ristle legacy's `application_core_test.cpp` and `app_core_test.cpp`
//! (names from `docs/plan/phase-2.md`, section 8).

// clippy.toml exempts `#[test]` functions only; the helpers are test code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ristle_app::{
    Core, CoreConfig, CoreSnapshot, ItemRef, LibraryChange, Observer, Platform, PlatformError,
    PlaybackNotice, PlaybackState, Session,
};
use ristle_domain::{Availability, PlaylistId, ProviderId, RistlePlaylist, Track, TrackRef};
use ristle_queue::sequence::SlotId;
use ristle_queue::{Advance, CanonicalQueue, QueueState};
use ristle_storage::repo::Intent;

const WAIT: Duration = Duration::from_secs(5);

struct NoPlatform;

impl Platform for NoPlatform {
    fn read_secret(&self, _key: &str) -> Result<Option<Vec<u8>>, PlatformError> {
        Ok(None)
    }
    fn write_secret(&self, _key: &str, _value: &[u8]) -> Result<(), PlatformError> {
        Ok(())
    }
    fn delete_secret(&self, _key: &str) -> Result<(), PlatformError> {
        Ok(())
    }
}

/// A data directory that outlives the cores opened on it.
struct Database(tempfile::TempDir);

impl Database {
    fn new() -> Self {
        Self(tempfile::tempdir().unwrap())
    }

    fn open(&self) -> Core {
        Core::new(
            CoreConfig {
                data_dir: self.0.path().join("data").to_string_lossy().into_owned(),
                cache_dir: self.0.path().join("cache").to_string_lossy().into_owned(),
            },
            Arc::new(NoPlatform),
        )
        .unwrap()
    }
}

fn track(provider: &str, id: &str) -> Track {
    let mut track = Track::new(
        TrackRef::catalog(ProviderId::new(provider), id.to_owned()).unwrap(),
        "Track",
    );
    track.availability = Availability::Available;
    track.duration_ms = Some(180_000);
    track
}

fn disabled(provider: &str, id: &str) -> Track {
    let mut track = track(provider, id);
    track.availability = Availability::Unavailable { reason: None };
    track
}

fn playlist(id: &str, tracks: Vec<Track>) -> RistlePlaylist {
    RistlePlaylist {
        id: PlaylistId::new(id),
        title: id.to_owned(),
        tracks,
        created_ms: 1,
        updated_ms: 2,
    }
}

/// A playlist of `count` Tracks `track-0`, `track-1`, ...
fn large(id: &str, count: usize) -> RistlePlaylist {
    playlist(
        id,
        (0..count)
            .map(|index| track("provider", &format!("track-{index}")))
            .collect(),
    )
}

fn pid(id: &str) -> PlaylistId {
    PlaylistId::new(id)
}

fn current_entry(queue: &QueueState) -> Option<&ristle_queue::QueueEntry> {
    match queue.current.as_ref()? {
        ristle_queue::Current::AtCursor => queue.collection.get(queue.cursor?),
        ristle_queue::Current::Detached(entry) => Some(entry),
    }
}

async fn current(session: &Session) -> String {
    current_entry(&session.queue().await)
        .unwrap()
        .track
        .reference
        .id()
        .to_owned()
}

/// The Track ids of what plays next, requests first.
async fn upcoming(session: &Session) -> Vec<String> {
    let queue = CanonicalQueue::restore(session.queue().await).unwrap();
    queue
        .upcoming()
        .iter()
        .map(|entry| entry.track.reference.id().to_owned())
        .collect()
}

#[tokio::test]
async fn always_restores_persisted_mixed_playlist_on_cold_start() {
    let database = Database::new();
    {
        let core = database.open();
        let session = core.session();
        session
            .save_playlist(playlist(
                "mixed",
                vec![
                    track("spotify", "first"),
                    disabled("spotify", "disabled"),
                    track("soundcloud", "third"),
                ],
            ))
            .await
            .unwrap();

        session
            .start_playlist(&pid("mixed"), Some(0), false)
            .await
            .unwrap();
        assert_eq!(session.transport().await.0, Intent::Playing);
        assert_eq!(current(session).await, "first");
        assert_eq!(session.queue().await.collection.len(), 2);
    }

    let reopened = database.open();
    assert_eq!(reopened.session().transport().await.0, Intent::Paused);
    assert_eq!(current(reopened.session()).await, "first");
    assert_eq!(reopened.snapshot().transport.state, PlaybackState::Paused);
}

#[tokio::test]
async fn materializes_only_a_window_of_large_ristle_playlist() {
    let database = Database::new();
    let core = database.open();
    let session = core.session();
    session.save_playlist(large("large", 1000)).await.unwrap();

    session
        .start_playlist(&pid("large"), Some(900), false)
        .await
        .unwrap();

    let sequence = session.sequence().await.unwrap();
    assert_eq!(sequence.order.len(), 1000);
    let queue = session.queue().await;
    let current = current_entry(&queue).unwrap();
    assert_eq!(current.track.reference.id(), "track-900");
    assert_eq!(current.slot, Some(SlotId(900)));
    // The current entry and fifty to come.
    assert_eq!(queue.collection.len(), 51);
    let source = core.snapshot().queue.source.unwrap();
    assert_eq!(
        source.opened,
        Some(ItemRef::RistlePlaylist {
            id: "large".to_owned()
        })
    );
}

#[tokio::test]
async fn starts_ristle_playlist_in_a_complete_shuffled_order() {
    let database = Database::new();
    let core = database.open();
    let session = core.session();
    session.save_playlist(large("large", 1000)).await.unwrap();

    session
        .start_playlist(&pid("large"), None, true)
        .await
        .unwrap();

    assert!(core.snapshot().shuffle);
    let queue = session.queue().await;
    assert!(current_entry(&queue).is_some());
    assert_eq!(queue.collection.len(), 51);
    let sequence = session.sequence().await.unwrap();
    assert_eq!(sequence.order.len(), 1000);
    assert!(sequence.shuffle.is_some());
    assert!(sequence.order[..50].iter().any(|slot| slot.0 >= 50));
    assert_eq!(current_entry(&queue).unwrap().slot, Some(sequence.order[0]));
}

#[tokio::test]
async fn persists_exact_large_playlist_shuffle_across_restart() {
    let database = Database::new();
    let shuffled = {
        let core = database.open();
        let session = core.session();
        session.save_playlist(large("large", 1000)).await.unwrap();
        session
            .start_playlist(&pid("large"), Some(0), false)
            .await
            .unwrap();
        session.set_shuffle(true).await.unwrap();
        assert!(core.snapshot().shuffle);
        assert!(session.queue().await.collection.len() <= 51);

        let sequence = session.sequence().await.unwrap();
        assert_eq!(sequence.order.len(), 1000);
        assert_eq!(sequence.cursor, Some(0));
        assert_eq!(sequence.order[0], SlotId(0));
        sequence
    };

    let reopened = database.open();
    assert!(reopened.snapshot().shuffle);
    let restored = reopened.session().sequence().await.unwrap();
    assert_eq!(restored.descriptor.id, shuffled.descriptor.id);
    assert_eq!(restored.order, shuffled.order);
    assert_eq!(restored.cursor, shuffled.cursor);
    assert_eq!(restored.materialized_end, shuffled.materialized_end);
    assert!(reopened.session().queue().await.collection.len() <= 51);
}

#[tokio::test]
async fn refills_large_sequence_without_growing_canonical_queue() {
    let database = Database::new();
    let core = database.open();
    let session = core.session();
    session.save_playlist(large("large", 1000)).await.unwrap();
    session
        .start_playlist(&pid("large"), Some(0), false)
        .await
        .unwrap();
    session.set_shuffle(true).await.unwrap();

    for _ in 0..400 {
        assert!(session.advance(Advance::TrackEnded).await.unwrap());
        // Fifty played entries, the current one and fifty to come.
        assert!(session.queue().await.collection.len() <= 101);
    }

    let sequence = session.sequence().await.unwrap();
    assert_eq!(sequence.cursor, Some(400));
    assert!(sequence.materialized_end <= 451);
}

#[tokio::test]
async fn starts_past_an_unavailable_initial_playlist_window() {
    let database = Database::new();
    let core = database.open();
    let session = core.session();
    // The start and the next fifty are all unavailable.
    let tracks = (0..60)
        .map(|index| {
            let id = format!("track-{index}");
            if index < 55 {
                disabled("provider", &id)
            } else {
                track("provider", &id)
            }
        })
        .collect();
    session
        .save_playlist(playlist("sparse", tracks))
        .await
        .unwrap();

    session
        .start_playlist(&pid("sparse"), Some(0), false)
        .await
        .unwrap();

    assert_eq!(session.transport().await.0, Intent::Playing);
    assert_eq!(current(session).await, "track-55");
}

#[tokio::test]
async fn a_playlist_that_cannot_play_leaves_the_queue_as_it_was() {
    let database = Database::new();
    let core = database.open();
    let session = core.session();
    session.save_playlist(large("good", 3)).await.unwrap();
    session
        .save_playlist(playlist("dead", vec![disabled("provider", "gone")]))
        .await
        .unwrap();
    session
        .start_playlist(&pid("good"), Some(1), false)
        .await
        .unwrap();
    let before = session.queue().await;

    assert!(
        session
            .start_playlist(&pid("dead"), Some(0), false)
            .await
            .is_err()
    );

    assert_eq!(session.queue().await, before);
    assert_eq!(
        core.snapshot().queue.source.unwrap().opened,
        Some(ItemRef::RistlePlaylist {
            id: "good".to_owned()
        })
    );
}

#[tokio::test]
async fn active_ristle_playlist_edits_reconcile_the_persisted_upcoming_queue() {
    let database = Database::new();
    {
        let core = database.open();
        let session = core.session();
        let mut drive = playlist(
            "drive",
            vec![
                track("spotify", "first"),
                track("spotify", "second"),
                track("spotify", "third"),
            ],
        );
        session.save_playlist(drive.clone()).await.unwrap();
        session
            .start_playlist(&pid("drive"), Some(0), false)
            .await
            .unwrap();
        assert!(core.snapshot().queue.source.is_some());
        session
            .play_next(track("soundcloud", "requested"), None)
            .await
            .unwrap();

        drive.updated_ms = 4;
        drive.tracks = vec![track("spotify", "first"), track("spotify", "replacement")];
        session.save_playlist(drive).await.unwrap();

        let queue = session.queue().await;
        assert_eq!(queue.requests.len(), 1);
        assert_eq!(queue.requests[0].track.reference.id(), "requested");
        assert_eq!(upcoming(session).await, ["requested", "replacement"]);
    }

    let reopened = database.open();
    assert_eq!(
        reopened.snapshot().queue.source.unwrap().opened,
        Some(ItemRef::RistlePlaylist {
            id: "drive".to_owned()
        })
    );
    assert_eq!(
        upcoming(reopened.session()).await,
        ["requested", "replacement"]
    );
}

#[tokio::test]
async fn active_playlist_edit_preserves_continuation_after_request_history() {
    let database = Database::new();
    let core = database.open();
    let session = core.session();
    let mut drive = playlist(
        "drive",
        vec![
            track("spotify", "first"),
            track("spotify", "selected"),
            track("spotify", "third"),
        ],
    );
    session.save_playlist(drive.clone()).await.unwrap();
    session
        .start_playlist(&pid("drive"), Some(0), false)
        .await
        .unwrap();
    let selected = session.queue().await.collection[1].id;
    session
        .add_to_queue(track("soundcloud", "requested"), None)
        .await
        .unwrap();

    assert!(session.jump_to(selected).await.unwrap());
    assert!(session.go_back().await.unwrap());
    assert_eq!(current(session).await, "requested");

    drive.updated_ms = 6;
    drive.tracks = vec![
        track("spotify", "first"),
        track("spotify", "selected"),
        track("spotify", "replacement"),
    ];
    session.save_playlist(drive).await.unwrap();

    let queue = CanonicalQueue::restore(session.queue().await).unwrap();
    let upcoming = queue.upcoming();
    assert_eq!(upcoming.len(), 2);
    assert_eq!(upcoming[0].id, selected);
    assert_eq!(upcoming[0].track.reference.id(), "selected");
    assert_eq!(upcoming[1].track.reference.id(), "replacement");
}

/// Legacy trap 6: an edit resumes after the occurrence that plays, not an
/// earlier duplicate of its Track.
#[tokio::test]
async fn an_edit_resumes_after_the_duplicate_that_plays() {
    let database = Database::new();
    let core = database.open();
    let session = core.session();
    let ids = ["a", "b", "a", "c"];
    let mut list = playlist("dupes", ids.iter().map(|id| track("p", id)).collect());
    session.save_playlist(list.clone()).await.unwrap();
    session
        .start_playlist(&pid("dupes"), Some(2), false)
        .await
        .unwrap();
    assert_eq!(upcoming(session).await, ["c"]);

    list.tracks.insert(0, track("p", "new"));
    session.save_playlist(list).await.unwrap();

    assert_eq!(current(session).await, "a");
    assert_eq!(upcoming(session).await, ["c"]);
    let sequence = session.sequence().await.unwrap();
    assert_eq!(sequence.cursor, Some(3));
}

/// Shuffled, an edit reshuffles only what this pass has not played.
#[tokio::test]
async fn a_shuffled_edit_does_not_replay_what_played() {
    let database = Database::new();
    let core = database.open();
    let session = core.session();
    let mut list = large("mix", 20);
    session.save_playlist(list.clone()).await.unwrap();
    session
        .start_playlist(&pid("mix"), None, true)
        .await
        .unwrap();
    let mut played = vec![current(session).await];
    for _ in 0..5 {
        session.advance(Advance::TrackEnded).await.unwrap();
        played.push(current(session).await);
    }

    list.tracks.push(track("provider", "added"));
    session.save_playlist(list).await.unwrap();

    assert_eq!(current(session).await, played[5]);
    let next = upcoming(session).await;
    assert_eq!(next.len(), 15);
    assert!(next.iter().all(|id| !played.contains(id)), "{next:?}");
    assert!(next.contains(&"added".to_owned()));
}

#[tokio::test]
async fn deleting_the_active_ristle_playlist_detaches_its_queue_source() {
    let database = Database::new();
    {
        let core = database.open();
        let session = core.session();
        session
            .save_playlist(playlist("drive", vec![track("spotify", "first")]))
            .await
            .unwrap();
        session
            .start_playlist(&pid("drive"), Some(0), false)
            .await
            .unwrap();

        assert!(session.delete_playlist(&pid("drive")).await.unwrap());
        assert!(core.snapshot().queue.source.is_none());
        assert_eq!(current(session).await, "first");
        assert!(session.sequence().await.is_none());
    }

    let reopened = database.open();
    assert!(reopened.snapshot().queue.source.is_none());
    assert_eq!(current(reopened.session()).await, "first");
    assert!(reopened.session().playlists().await.unwrap().is_empty());
}

/// Records snapshots and library changes.
struct Recorder {
    snapshots: Mutex<Sender<CoreSnapshot>>,
    changes: Mutex<Sender<LibraryChange>>,
}

impl Observer for Recorder {
    fn on_snapshot(&self, snapshot: CoreSnapshot) {
        let _ = self.snapshots.lock().unwrap().send(snapshot);
    }
    fn on_notice(&self, _notice: PlaybackNotice) {}
    fn on_library_changed(&self, change: LibraryChange) {
        let _ = self.changes.lock().unwrap().send(change);
    }
}

fn record(
    core: &Core,
) -> (
    ristle_app::Subscription,
    Receiver<CoreSnapshot>,
    Receiver<LibraryChange>,
) {
    let (snapshot_sender, snapshots) = channel();
    let (change_sender, changes) = channel();
    let subscription = core.subscribe(Arc::new(Recorder {
        snapshots: Mutex::new(snapshot_sender),
        changes: Mutex::new(change_sender),
    }));
    snapshots.recv_timeout(WAIT).unwrap();
    (subscription, snapshots, changes)
}

#[tokio::test]
async fn active_playlist_mutations_notify_observers() {
    let database = Database::new();
    let core = database.open();
    let session = core.session();
    let mut drive = playlist("drive", vec![track("p", "current"), track("p", "next")]);
    session.save_playlist(drive.clone()).await.unwrap();
    session
        .start_playlist(&pid("drive"), Some(0), false)
        .await
        .unwrap();
    let (_subscription, snapshots, changes) = record(&core);

    drive.updated_ms = 3;
    drive.tracks.push(track("p", "tail"));
    session.save_playlist(drive).await.unwrap();
    let after_save = snapshots.recv_timeout(WAIT).unwrap();
    assert_eq!(after_save.queue.upcoming.len(), 2);
    assert_eq!(
        changes.recv_timeout(WAIT).unwrap(),
        LibraryChange::RistlePlaylists
    );

    session
        .add_to_playlist(&pid("drive"), vec![track("p", "last")])
        .await
        .unwrap();
    let after_add = snapshots.recv_timeout(WAIT).unwrap();
    assert_eq!(after_add.queue.upcoming.len(), 3);
    assert!(after_add.revision > after_save.revision);

    assert!(session.delete_playlist(&pid("drive")).await.unwrap());
    let after_delete = snapshots.recv_timeout(WAIT).unwrap();
    assert!(after_delete.queue.source.is_none());
    assert!(after_delete.revision > after_add.revision);
}

/// The facade's playlist commands: create returns at once, the rest queue
/// behind it, and observers hear of each change.
#[tokio::test]
async fn facade_playlist_commands_create_rename_and_delete() {
    let database = Database::new();
    let core = database.open();
    let (_subscription, _snapshots, changes) = record(&core);

    let created = core.create_playlist("Morning".to_owned()).unwrap();
    assert_eq!(created.track_count, 0);
    changes.recv_timeout(WAIT).unwrap();
    assert_eq!(
        core.ristle_playlists().unwrap(),
        std::slice::from_ref(&created)
    );

    core.rename_playlist(created.id.clone(), "Evening".to_owned())
        .unwrap();
    changes.recv_timeout(WAIT).unwrap();
    assert_eq!(core.ristle_playlists().unwrap()[0].title, "Evening");

    core.delete_playlist(created.id).unwrap();
    changes.recv_timeout(WAIT).unwrap();
    assert!(core.ristle_playlists().unwrap().is_empty());
}

/// CONTEXT.md, Recently Played: a Track qualifies after 30 seconds or half
/// its duration, whichever comes first.
#[tokio::test]
async fn recently_played_keeps_tracks_that_played_long_enough() {
    let database = Database::new();
    let core = database.open();
    let session = core.session();
    let (_subscription, _snapshots, changes) = record(&core);
    let mut short = track("p", "short");
    short.duration_ms = Some(40_000);

    assert!(
        !session
            .record_played(track("p", "skipped"), 10_000)
            .await
            .unwrap()
    );
    assert!(session.record_played(short, 20_000).await.unwrap());
    assert_eq!(
        changes.recv_timeout(WAIT).unwrap(),
        LibraryChange::RecentlyPlayed
    );
    assert!(
        session
            .record_played(track("p", "long"), 30_000)
            .await
            .unwrap()
    );

    let played: Vec<String> = core
        .recently_played()
        .unwrap()
        .into_iter()
        .map(|track| track.item.id)
        .collect();
    assert_eq!(played, ["long", "short"]);
    drop(core);
    assert_eq!(
        database
            .open()
            .session()
            .recently_played()
            .await
            .unwrap()
            .len(),
        2
    );
}
