//! The Playback Session over real storage: queue mutations, their
//! persistence, cold-start restore, command receipts and what observers
//! see. Ported from Ristle legacy's `application_core_test.cpp` and
//! `app_core_test.cpp` (names from `docs/plan/phase-2.md`, section 8).

// clippy.toml exempts `#[test]` functions only; the helpers are test code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ristle_app::session::PREVIOUS_RESTARTS_AFTER_MS;
use ristle_app::{
    Core, CoreConfig, CoreSnapshot, Error, LibraryChange, Observer, Platform, PlatformError,
    PlaybackNotice, PlaybackState, Receipt, RepeatMode, Session,
};
use ristle_domain::{Availability, ProviderId, Track, TrackRef};
use ristle_queue::{Advance, EntryId, Origin, QueueState};
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

    /// Opens a core, as a cold start does.
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

fn local(id: &str) -> Track {
    track("local", id)
}

fn current_id(queue: &QueueState) -> Option<String> {
    let entry = match queue.current.as_ref()? {
        ristle_queue::Current::AtCursor => &queue.collection[queue.cursor?],
        ristle_queue::Current::Detached(entry) => entry,
    };
    Some(entry.track.reference.id().to_owned())
}

async fn current(session: &Session) -> String {
    current_id(&session.queue().await).unwrap()
}

/// The Track ids of the request lane.
fn requests(queue: &QueueState) -> Vec<String> {
    queue
        .requests
        .iter()
        .map(|entry| entry.track.reference.id().to_owned())
        .collect()
}

#[tokio::test]
async fn queues_request_before_playlist_continuation() {
    let database = Database::new();
    let core = database.open();
    let session = core.session();
    session
        .start_tracks(
            vec![track("spotify", "first"), track("spotify", "second")],
            0,
        )
        .await
        .unwrap();

    session
        .add_to_queue(track("soundcloud", "request"), None)
        .await
        .unwrap();
    assert_eq!(session.queue().await.requests.len(), 1);

    session.advance(Advance::TrackEnded).await.unwrap();
    assert_eq!(current(session).await, "request");
    session.advance(Advance::TrackEnded).await.unwrap();
    assert_eq!(current(session).await, "second");
}

#[tokio::test]
async fn previews_queue_moves_without_mutating_session() {
    let database = Database::new();
    let core = database.open();
    let session = core.session();
    session
        .start_tracks(vec![local("first"), local("second"), local("third")], 1)
        .await
        .unwrap();
    let before = session.queue().await;

    let next = session.peek_next(Advance::Skip).await.unwrap();
    let previous = session.peek_previous().await.unwrap();

    assert_eq!(next.track.reference.id(), "third");
    assert_eq!(previous.track.reference.id(), "first");
    assert_eq!(session.queue().await, before);
}

#[tokio::test]
async fn persists_direct_queue_selection() {
    let database = Database::new();
    {
        let core = database.open();
        let session = core.session();
        session
            .start_tracks(
                vec![
                    local("first"),
                    local("second"),
                    local("third"),
                    local("fourth"),
                ],
                0,
            )
            .await
            .unwrap();
        let fourth = session.queue().await.collection[3].id;
        session.seek(30_000).await.unwrap();

        assert!(session.jump_to(fourth).await.unwrap());
        assert_eq!(current(session).await, "fourth");
        assert_eq!(session.transport().await.1, 0);
        assert_eq!(session.queue().await.cursor, Some(3));
    }

    let reopened = database.open();
    let restored = reopened.session().queue().await;
    assert_eq!(current_id(&restored).as_deref(), Some("fourth"));
    assert_eq!(restored.cursor, Some(3));
}

#[tokio::test]
async fn persists_request_history_after_direct_selection() {
    let database = Database::new();
    {
        let core = database.open();
        let session = core.session();
        session
            .start_tracks(vec![local("current")], 0)
            .await
            .unwrap();
        session
            .add_to_queue(track("spotify", "request-one"), None)
            .await
            .unwrap();
        let target = session
            .add_to_queue(track("spotify", "request-two"), None)
            .await
            .unwrap();

        assert!(session.jump_to(target).await.unwrap());
        let queue = session.queue().await;
        assert_eq!(queue.cursor, Some(2));
        assert_eq!(queue.collection[1].track.reference.id(), "request-one");
        assert_eq!(queue.collection[1].origin, Origin::Request);
    }

    let reopened = database.open();
    let session = reopened.session();
    assert!(session.go_back().await.unwrap());
    let queue = session.queue().await;
    assert_eq!(current_id(&queue).as_deref(), Some("request-one"));
    assert_eq!(
        queue.collection[queue.cursor.unwrap()].origin,
        Origin::Request
    );
}

#[tokio::test]
async fn go_back_returns_to_previous_track_and_resets_playhead() {
    let database = Database::new();
    let core = database.open();
    let session = core.session();
    session
        .start_tracks(vec![local("first"), local("second"), local("third")], 0)
        .await
        .unwrap();
    session.advance(Advance::TrackEnded).await.unwrap();
    session
        .set_transport(Intent::Playing, 45_000)
        .await
        .unwrap();
    assert_eq!(current(session).await, "second");
    assert_eq!(session.transport().await.1, 45_000);

    assert!(session.go_back().await.unwrap());
    assert_eq!(current(session).await, "first");
    assert_eq!(session.transport().await.1, 0);
}

#[tokio::test]
async fn rejects_disabled_direct_queue_request() {
    let database = Database::new();
    let core = database.open();
    let mut disabled = track("spotify", "disabled");
    disabled.availability = Availability::Unavailable { reason: None };

    let result = core.session().add_to_queue(disabled, None).await;

    assert!(matches!(result, Err(Error::InvalidArgument { .. })));
    assert_eq!(core.session().queue().await, QueueState::default());
    drop(core);
    assert_eq!(
        database.open().session().queue().await,
        QueueState::default()
    );
}

#[tokio::test]
async fn persists_transport_and_repeat_state() {
    let database = Database::new();
    {
        let core = database.open();
        let session = core.session();
        session.start_tracks(vec![local("track")], 0).await.unwrap();
        session.set_transport(Intent::Paused, 45_000).await.unwrap();
        assert_eq!(session.transport().await, (Intent::Paused, 45_000));
        session.set_repeat(RepeatMode::Track).await.unwrap();
        assert_eq!(
            session.queue().await.repeat,
            ristle_queue::RepeatMode::Track
        );
        session.set_shuffle(true).await.unwrap();
        assert!(core.snapshot().shuffle);
    }

    let restored = database.open();
    assert_eq!(
        restored.session().transport().await,
        (Intent::Paused, 45_000)
    );
    let snapshot = restored.snapshot();
    assert_eq!(snapshot.repeat, RepeatMode::Track);
    assert!(snapshot.shuffle);
    assert_eq!(snapshot.transport.position_ms, 45_000);
}

/// CONTEXT.md, Playback Session: a cold start restores paused, never
/// playing.
#[tokio::test]
async fn a_cold_start_restores_a_playing_session_paused() {
    let database = Database::new();
    {
        let core = database.open();
        let session = core.session();
        session.start_tracks(vec![local("track")], 0).await.unwrap();
        session
            .set_transport(Intent::Playing, 12_000)
            .await
            .unwrap();
        assert_eq!(core.snapshot().transport.state, PlaybackState::Playing);
    }

    let restored = database.open();
    assert_eq!(
        restored.session().transport().await,
        (Intent::Paused, 12_000)
    );
    assert_eq!(restored.snapshot().transport.state, PlaybackState::Paused);
    assert_eq!(
        restored
            .snapshot()
            .queue
            .current
            .map(|entry| entry.track.item.id),
        Some("track".to_owned())
    );
}

#[tokio::test]
async fn skip_leaves_repeated_track_but_track_end_replays_it() {
    let database = Database::new();
    let core = database.open();
    let session = core.session();
    session
        .start_tracks(vec![local("first"), local("second")], 0)
        .await
        .unwrap();
    session.set_repeat(RepeatMode::Track).await.unwrap();

    session.advance(Advance::TrackEnded).await.unwrap();
    assert_eq!(current(session).await, "first");
    assert_eq!(
        session
            .peek_next(Advance::Skip)
            .await
            .unwrap()
            .track
            .reference
            .id(),
        "second"
    );

    session.advance(Advance::Skip).await.unwrap();
    assert_eq!(current(session).await, "second");
    assert_eq!(
        session.queue().await.repeat,
        ristle_queue::RepeatMode::Track
    );
}

#[tokio::test]
async fn persists_queue_editing_and_undo() {
    let database = Database::new();
    {
        let core = database.open();
        let session = core.session();
        session
            .start_tracks(vec![local("current"), local("later")], 0)
            .await
            .unwrap();
        session
            .add_to_queue(track("spotify", "fifo"), None)
            .await
            .unwrap();
        let priority = session
            .play_next(track("soundcloud", "priority"), None)
            .await
            .unwrap();
        assert_eq!(session.queue().await.requests[0].id, priority);

        assert!(session.remove(priority).await.unwrap());
        assert_eq!(session.queue().await.requests.len(), 1);
        assert!(session.undo_removal().await.unwrap());
        assert_eq!(session.queue().await.requests[0].id, priority);
        assert!(session.move_request(priority, 1).await.unwrap());
        assert_eq!(requests(&session.queue().await)[1], "priority");
        session.clear_upcoming().await.unwrap();
        let cleared = session.queue().await;
        assert!(cleared.requests.is_empty());
        assert_eq!(cleared.collection.len(), 1);
    }

    let reloaded = database.open();
    assert_eq!(reloaded.session().queue().await.collection.len(), 1);
}

#[tokio::test]
async fn playback_command_receipt_survives_restart_and_prevents_repeated_mutation() {
    let database = Database::new();
    let receipt = |scope: &str, key: &str, outcome: &str| Receipt {
        scope: scope.to_owned(),
        key: key.to_owned(),
        outcome: outcome.to_owned(),
    };
    {
        let core = database.open();
        let session = core.session();
        session
            .start_tracks(vec![track("spotify", "one"), track("spotify", "two")], 0)
            .await
            .unwrap();
        let applied = session
            .apply_once(receipt("room", "1", "applied"), |edit| {
                edit.start_tracks(vec![track("spotify", "replacement")], 0)?;
                edit.put_setting("frozen-routing", "replacement-route");
                Ok(())
            })
            .await
            .unwrap();
        assert_eq!(applied, Some(()));
    }

    let reopened = database.open();
    let session = reopened.session();
    let mut called = false;
    let repeated = session
        .apply_once(receipt("room", "1", "applied"), |_| {
            called = true;
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(repeated, None);
    assert!(!called);
    assert_eq!(
        session.receipt("room", "1").await.unwrap().as_deref(),
        Some("applied")
    );
    assert_eq!(current(session).await, "replacement");
    assert_eq!(
        session.setting("frozen-routing").await.unwrap().as_deref(),
        Some("replacement-route")
    );
    assert_eq!(
        session
            .apply_once(receipt("other", "1", "rejected: unavailable"), |_| Ok(()))
            .await
            .unwrap(),
        Some(())
    );
    session.clear_receipts("room").await.unwrap();
    assert_eq!(session.receipt("room", "1").await.unwrap(), None);
    assert_eq!(
        session.receipt("other", "1").await.unwrap().as_deref(),
        Some("rejected: unavailable")
    );
}

#[tokio::test]
async fn playback_mutation_failure_restores_memory_database_routing_and_receipt() {
    let database = Database::new();
    let receipt = || Receipt {
        scope: "room".to_owned(),
        key: "failed".to_owned(),
        outcome: "applied".to_owned(),
    };
    let core = database.open();
    let session = core.session();
    session
        .start_tracks(vec![track("spotify", "original")], 0)
        .await
        .unwrap();
    let initial = session.queue().await;
    session
        .put_setting("frozen-routing", "original-route")
        .await
        .unwrap();

    let failed = session
        .apply_once(receipt(), |edit| -> Result<(), Error> {
            edit.start_tracks(vec![track("spotify", "replacement")], 0)?;
            edit.put_setting("frozen-routing", "replacement-route");
            Err(Error::Internal {
                reason: "failure after nested writes".to_owned(),
            })
        })
        .await;

    assert!(failed.is_err());
    assert_eq!(session.queue().await, initial);
    assert_eq!(
        session.setting("frozen-routing").await.unwrap().as_deref(),
        Some("original-route")
    );
    assert_eq!(session.receipt("room", "failed").await.unwrap(), None);
    {
        let reopened = database.open();
        let session = reopened.session();
        assert_eq!(current(session).await, "original");
        assert_eq!(
            session.setting("frozen-routing").await.unwrap().as_deref(),
            Some("original-route")
        );
        assert_eq!(session.receipt("room", "failed").await.unwrap(), None);
    }
    assert_eq!(
        session.apply_once(receipt(), |_| Ok(())).await.unwrap(),
        Some(())
    );
}

/// Starts `current` with requests `request-one` and `request-two`, then
/// selects `request-two` directly.
async fn select_second_request(session: &Session, continuation: bool) -> EntryId {
    let mut collection = vec![local("current")];
    if continuation {
        collection.push(local("continuation"));
    }
    session.start_tracks(collection, 0).await.unwrap();
    session
        .add_to_queue(local("request-one"), None)
        .await
        .unwrap();
    let target = session
        .add_to_queue(local("request-two"), None)
        .await
        .unwrap();
    assert!(session.jump_to(target).await.unwrap());
    target
}

#[tokio::test]
async fn direct_queue_selection_preserves_skipped_requests_for_previous() {
    let database = Database::new();
    let core = database.open();
    let session = core.session();
    select_second_request(session, false).await;
    let queue = session.queue().await;
    assert_eq!(current_id(&queue).as_deref(), Some("request-two"));
    assert_eq!(
        queue.collection[queue.cursor.unwrap()].origin,
        Origin::Request
    );

    session.seek(10_000).await.unwrap();
    session.previous().await.unwrap();
    // Past the threshold, Previous restarts the Track.
    const { assert!(10_000 >= PREVIOUS_RESTARTS_AFTER_MS) };
    assert_eq!(current(session).await, "request-two");
    assert_eq!(session.transport().await.1, 0);
    session.previous().await.unwrap();
    assert_eq!(current(session).await, "request-one");
}

#[tokio::test]
async fn consecutive_direct_queue_selections_do_not_duplicate_requests() {
    let database = Database::new();
    let core = database.open();
    let session = core.session();
    session
        .start_tracks(vec![local("current"), local("continuation")], 0)
        .await
        .unwrap();
    let continuation = session.queue().await.collection[1].id;
    let request_one = session
        .add_to_queue(local("request-one"), None)
        .await
        .unwrap();
    session
        .add_to_queue(local("request-two"), None)
        .await
        .unwrap();

    assert!(session.jump_to(request_one).await.unwrap());
    assert!(session.jump_to(continuation).await.unwrap());
    assert_eq!(current(session).await, "continuation");

    let restored = ristle_queue::CanonicalQueue::restore(session.queue().await).unwrap();
    assert_eq!(restored.history().len(), 3);
}

#[tokio::test]
async fn direct_queue_selection_history_restores_after_going_back() {
    let database = Database::new();
    {
        let core = database.open();
        let session = core.session();
        select_second_request(session, false).await;
        session.seek(10_000).await.unwrap();
        session.previous().await.unwrap();
        session.previous().await.unwrap();
        assert_eq!(current(session).await, "request-one");
    }

    let restored = database.open();
    let queue = restored.session().queue().await;
    assert_eq!(current_id(&queue).as_deref(), Some("request-one"));
    assert_eq!(
        queue.collection[queue.cursor.unwrap() + 1]
            .track
            .reference
            .id(),
        "request-two"
    );
}

#[tokio::test]
async fn undo_restores_preserved_request_after_direct_selection() {
    let database = Database::new();
    let core = database.open();
    let session = core.session();
    let request_two = select_second_request(session, true).await;

    assert!(session.go_back().await.unwrap());
    assert!(session.remove(request_two).await.unwrap());
    assert!(session.undo_removal().await.unwrap());

    let canonical = ristle_queue::CanonicalQueue::restore(session.queue().await).unwrap();
    assert_eq!(canonical.upcoming()[0].track.reference.id(), "request-two");
}

/// Records every snapshot an observer receives.
struct Recorder(Mutex<Sender<CoreSnapshot>>);

impl Observer for Recorder {
    fn on_snapshot(&self, snapshot: CoreSnapshot) {
        let _ = self.0.lock().unwrap().send(snapshot);
    }
    fn on_notice(&self, _notice: PlaybackNotice) {}
    fn on_library_changed(&self, _change: LibraryChange) {}
}

fn record(core: &Core) -> (ristle_app::Subscription, Receiver<CoreSnapshot>) {
    let (sender, snapshots) = channel();
    let subscription = core.subscribe(Arc::new(Recorder(Mutex::new(sender))));
    // The current snapshot comes first.
    snapshots.recv_timeout(WAIT).unwrap();
    (subscription, snapshots)
}

/// The Track ids a snapshot lists from the current entry on.
fn listed(snapshot: &CoreSnapshot) -> Vec<String> {
    let queue = &snapshot.queue;
    queue
        .current
        .iter()
        .chain(&queue.requests)
        .chain(&queue.upcoming)
        .map(|entry| entry.track.item.id.clone())
        .collect()
}

#[tokio::test]
async fn every_queue_mutation_notifies_observers() {
    let database = Database::new();
    let core = database.open();
    let session = core.session();
    session
        .start_tracks(vec![local("current"), local("collection-next")], 0)
        .await
        .unwrap();
    let (_subscription, snapshots) = record(&core);
    let latest = || snapshots.recv_timeout(WAIT).unwrap();

    let request = session.play_next(local("requested"), None).await.unwrap();
    assert_eq!(
        listed(&latest()),
        ["current", "requested", "collection-next"]
    );
    session.move_request(request, 1).await.unwrap();
    latest();
    session.remove(request).await.unwrap();
    latest();
    session.undo_removal().await.unwrap();
    latest();
    session.clear_upcoming().await.unwrap();
    latest();
    session.add_to_queue(local("tail"), None).await.unwrap();
    assert_eq!(listed(&latest()), ["current", "tail"]);
}

#[tokio::test]
async fn published_snapshots_have_monotonic_revisions() {
    let database = Database::new();
    let core = database.open();
    let session = core.session();
    let (_subscription, snapshots) = record(&core);

    session
        .start_tracks(vec![local("current")], 0)
        .await
        .unwrap();
    let started = snapshots.recv_timeout(WAIT).unwrap().revision;
    session.play_next(local("next"), None).await.unwrap();
    let queued = snapshots.recv_timeout(WAIT).unwrap().revision;
    session.set_shuffle(true).await.unwrap();
    let shuffled = snapshots.recv_timeout(WAIT).unwrap();

    assert!(queued > started);
    assert!(shuffled.revision > queued);
    assert!(shuffled.shuffle);
}

/// Synchronous facade commands run in the order they were sent, on the
/// core runtime, and their effects reach observers.
#[tokio::test]
async fn synchronous_commands_apply_in_order() {
    let database = Database::new();
    let core = database.open();
    let session = core.session();
    session
        .start_tracks(vec![local("first"), local("second"), local("third")], 0)
        .await
        .unwrap();
    let (_subscription, snapshots) = record(&core);

    core.next().unwrap();
    core.next().unwrap();
    core.previous().unwrap();
    core.set_repeat(RepeatMode::Queue).unwrap();

    let mut last = snapshots.recv_timeout(WAIT).unwrap();
    while last.repeat != RepeatMode::Queue {
        last = snapshots.recv_timeout(WAIT).unwrap();
    }
    assert_eq!(listed(&last)[0], "second");
}
