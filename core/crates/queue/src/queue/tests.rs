//! The Canonical Queue's behavior. The legacy tests of
//! `canonical_queue_test.cpp` keep their names (`docs/plan/phase-2.md`,
//! section 8).

use ristle_domain::{Availability, Track, TrackRef};

use super::*;
use crate::entry::Advance::{Skip, TrackEnded};
use crate::entry::Origin;
use crate::random::Random;

fn track(provider: &str, id: &str) -> Track {
    Track {
        availability: Availability::Available,
        ..Track::new(TrackRef::catalog(provider.into(), id.into()).unwrap(), id)
    }
}

fn unavailable(provider: &str, id: &str) -> Track {
    Track {
        availability: Availability::Unavailable { reason: None },
        ..track(provider, id)
    }
}

/// A Track whose id only means something on one server.
fn scoped(id: &str, connection: &str) -> Track {
    Track::new(
        TrackRef::server_scoped("jellyfin".into(), id.into(), connection.into()).unwrap(),
        id,
    )
}

fn started(tracks: Vec<Track>) -> CanonicalQueue {
    let mut queue = CanonicalQueue::new();
    assert!(
        queue
            .start_collection(CollectionEntry::numbered(tracks, 0), 0)
            .started
    );
    queue
}

fn playing(queue: &CanonicalQueue) -> &str {
    queue.current().unwrap().track.reference.id()
}

fn names<'a>(entries: impl IntoIterator<Item = &'a QueueEntry>) -> Vec<&'a str> {
    entries
        .into_iter()
        .map(|entry| entry.track.reference.id())
        .collect()
}

fn round_trip(queue: &CanonicalQueue) -> CanonicalQueue {
    CanonicalQueue::restore(queue.snapshot()).unwrap()
}

#[test]
fn collection_playback_silently_skips_unavailable_tracks() {
    let mut queue = CanonicalQueue::new();

    let outcome = queue.start_collection(
        CollectionEntry::numbered(
            [
                track("spotify", "first"),
                unavailable("spotify", "disabled"),
                track("soundcloud", "third"),
            ],
            0,
        ),
        0,
    );

    assert_eq!(
        outcome,
        StartOutcome {
            accepted: 2,
            skipped: 1,
            started: true
        }
    );
    assert_eq!(playing(&queue), "first");
    assert!(queue.advance(TrackEnded));
    assert_eq!(playing(&queue), "third");
}

#[test]
fn starting_at_disabled_track_uses_next_available_track() {
    let mut queue = CanonicalQueue::new();

    let outcome = queue.start_collection(
        CollectionEntry::numbered(
            [
                track("spotify", "first"),
                unavailable("spotify", "disabled"),
                track("spotify", "third"),
            ],
            0,
        ),
        1,
    );

    assert!(outcome.started);
    assert_eq!(playing(&queue), "third");
}

#[test]
fn direct_queue_request_rejects_unavailable_track() {
    let mut queue = started(vec![track("spotify", "current")]);

    let result = queue.add_to_queue(unavailable("soundcloud", "disabled"), None);

    assert_eq!(result, Err(Rejection::Unavailable));
    assert!(queue.requests().is_empty());
}

#[test]
fn queued_requests_play_fifo_before_collection_resumes() {
    let mut queue = started(vec![
        track("spotify", "playlist-1"),
        track("spotify", "playlist-2"),
    ]);
    queue
        .add_to_queue(track("soundcloud", "request-1"), None)
        .unwrap();
    queue
        .add_to_queue(track("spotify", "request-2"), None)
        .unwrap();

    assert!(queue.advance(TrackEnded));
    assert_eq!(playing(&queue), "request-1");
    assert_eq!(queue.current().unwrap().origin, Origin::Request);
    assert!(queue.advance(TrackEnded));
    assert_eq!(playing(&queue), "request-2");
    assert!(queue.advance(TrackEnded));
    assert_eq!(playing(&queue), "playlist-2");
    assert!(matches!(
        queue.current().unwrap().origin,
        Origin::Collection { .. }
    ));
}

#[test]
fn repeat_track_also_repeats_queued_request() {
    let mut queue = started(vec![
        track("spotify", "playlist-1"),
        track("spotify", "playlist-2"),
    ]);
    queue
        .add_to_queue(track("soundcloud", "request"), None)
        .unwrap();
    assert!(queue.advance(TrackEnded));
    let entry = queue.current().unwrap().id;
    queue.set_repeat(RepeatMode::Track);

    assert!(queue.advance(TrackEnded));
    assert_eq!(queue.current().unwrap().id, entry);
    assert_eq!(playing(&queue), "request");

    // A skip leaves the repeated Track.
    assert!(queue.advance(Skip));
    assert_eq!(playing(&queue), "playlist-2");
}

#[test]
fn repeat_queue_loops_underlying_collection() {
    let mut queue = started(vec![
        track("spotify", "first"),
        track("soundcloud", "second"),
    ]);
    queue.set_repeat(RepeatMode::Queue);

    assert!(queue.advance(TrackEnded));
    assert_eq!(playing(&queue), "second");
    assert!(queue.advance(TrackEnded));
    assert_eq!(playing(&queue), "first");
}

// A media session queue addresses entries by id, so the wrapped projection at
// the collection tail must not republish the playing entry under the id it
// already holds as current. Traversal is unaffected.
#[test]
fn repeat_queue_wrap_excludes_the_current_entry() {
    let mut queue = started(vec![track("local", "first"), track("local", "second")]);
    queue.set_repeat(RepeatMode::Queue);
    assert!(queue.advance(TrackEnded));
    assert_eq!(playing(&queue), "second");

    let wrapped = queue.upcoming();
    assert_eq!(names(wrapped.iter().copied()), ["first"]);
    let current = queue.current().unwrap().id;
    assert!(wrapped.iter().all(|entry| entry.id != current));

    assert!(queue.advance(TrackEnded));
    assert_eq!(playing(&queue), "first");
    assert!(queue.advance(TrackEnded));
    assert_eq!(playing(&queue), "second");
}

#[test]
fn starting_new_collection_preserves_repeat_mode() {
    let mut queue = CanonicalQueue::new();
    queue.set_repeat(RepeatMode::Queue);

    let outcome =
        queue.start_collection(CollectionEntry::numbered([track("spotify", "first")], 0), 0);

    assert!(outcome.started);
    assert_eq!(queue.repeat(), RepeatMode::Queue);
}

#[test]
fn shuffle_reorders_only_upcoming_collection_tracks() {
    let mut queue = started(
        ["first", "second", "third", "fourth", "fifth"]
            .map(|id| track("local", id))
            .into(),
    );
    queue
        .add_to_queue(track("soundcloud", "request"), None)
        .unwrap();

    queue.shuffle_upcoming(42);

    assert_eq!(playing(&queue), "first");
    assert_eq!(names(queue.upcoming())[0], "request");
    let shuffled = queue.snapshot().collection;
    assert_eq!(names(&shuffled)[0], "first");
    assert_ne!(
        names(&shuffled[1..]),
        ["second", "third", "fourth", "fifth"]
    );
    assert_eq!(queue.snapshot().shuffle, Some(42));
}

#[test]
fn disabling_shuffle_restores_original_upcoming_order() {
    let mut queue = started(
        ["first", "second", "third", "fourth"]
            .map(|id| track("local", id))
            .into(),
    );
    queue.shuffle_upcoming(42);

    queue.restore_upcoming_order();

    assert_eq!(names(queue.upcoming()), ["second", "third", "fourth"]);
    assert_eq!(queue.snapshot().shuffle, None);
}

#[test]
fn shuffle_preserves_forward_request_priority() {
    let mut queue = started(
        ["current", "continuation-one", "continuation-two"]
            .map(|id| track("local", id))
            .into(),
    );
    queue
        .add_to_queue(track("spotify", "request-one"), None)
        .unwrap();
    let request_two = queue
        .add_to_queue(track("spotify", "request-two"), None)
        .unwrap();
    assert!(queue.advance_to(request_two));
    assert!(queue.go_back());
    let before_shuffle = queue.snapshot();

    for seed in 0..32 {
        let mut shuffled = CanonicalQueue::restore(before_shuffle.clone()).unwrap();
        shuffled.shuffle_upcoming(seed);
        assert_eq!(shuffled.upcoming()[0].id, request_two);
    }

    queue.shuffle_upcoming(42);
    queue.restore_upcoming_order();
    let restored = queue.upcoming();
    assert_eq!(restored[0].id, request_two);
    assert_eq!(
        names(restored[1..].iter().copied()),
        ["continuation-one", "continuation-two"]
    );
}

#[test]
fn history_collects_advanced_collection_tracks_in_order() {
    let mut queue = started(
        ["first", "second", "third"]
            .map(|id| track("local", id))
            .into(),
    );
    assert!(queue.history().is_empty());

    assert!(queue.advance(TrackEnded));
    assert!(queue.advance(TrackEnded));

    assert_eq!(playing(&queue), "third");
    assert_eq!(names(queue.history()), ["first", "second"]);
}

#[test]
fn history_excludes_queued_requests_and_resets_on_new_collection() {
    let mut queue = started(vec![track("local", "a1"), track("local", "a2")]);
    queue
        .add_to_queue(track("soundcloud", "request"), None)
        .unwrap();

    // Playing the request does not move the cursor, so it never enters the
    // history.
    assert!(queue.advance(TrackEnded));
    assert_eq!(playing(&queue), "request");
    assert!(queue.history().is_empty());

    assert!(queue.advance(TrackEnded));
    assert_eq!(playing(&queue), "a2");
    assert_eq!(names(queue.history()), ["a1"]);

    // A new collection wipes the history.
    let outcome = queue.start_collection(CollectionEntry::numbered([track("local", "b1")], 0), 0);
    assert!(outcome.started);
    assert!(queue.history().is_empty());
}

#[test]
fn go_back_returns_to_previous_collection_track() {
    let mut queue = started(
        ["first", "second", "third"]
            .map(|id| track("local", id))
            .into(),
    );
    assert!(queue.advance(TrackEnded));
    assert!(queue.advance(TrackEnded));
    assert_eq!(playing(&queue), "third");

    assert!(queue.go_back());
    assert_eq!(playing(&queue), "second");
    assert_eq!(names(queue.upcoming())[0], "third");
    assert_eq!(names(queue.history()), ["first"]);

    assert!(queue.go_back());
    assert_eq!(playing(&queue), "first");
    assert!(queue.history().is_empty());

    // Nothing earlier: going back is a no-op at the head of the collection.
    assert!(!queue.go_back());
    assert_eq!(playing(&queue), "first");
}

#[test]
fn replace_upcoming_collection_preserves_current_and_queued_requests() {
    let mut queue = started(
        ["first", "second", "third"]
            .map(|id| track("spotify", id))
            .into(),
    );
    queue
        .play_next(track("spotify", "requested"), None)
        .unwrap();

    let outcome = queue.replace_upcoming_collection(CollectionEntry::numbered(
        [
            track("spotify", "first"),
            track("spotify", "replacement"),
            unavailable("spotify", "disabled"),
        ],
        0,
    ));

    assert_eq!(
        outcome,
        AddOutcome {
            added: 1,
            skipped: 1
        }
    );
    assert_eq!(playing(&queue), "first");
    assert_eq!(names(queue.upcoming()), ["requested", "replacement"]);
}

#[test]
fn replace_upcoming_collection_preserves_selected_continuation_after_request() {
    let mut queue = started(
        ["first", "selected", "third"]
            .map(|id| track("spotify", id))
            .into(),
    );
    queue
        .add_to_queue(track("soundcloud", "requested"), None)
        .unwrap();
    let selected = queue.upcoming()[1].id;

    assert!(queue.advance_to(selected));
    assert!(queue.go_back());
    assert_eq!(playing(&queue), "requested");

    let outcome = queue.replace_upcoming_collection(CollectionEntry::numbered(
        ["first", "selected", "replacement"].map(|id| track("spotify", id)),
        0,
    ));

    assert_eq!(outcome.added, 1);
    let upcoming = queue.upcoming();
    assert_eq!(upcoming.len(), 2);
    assert_eq!(upcoming[0].id, selected);
    assert_eq!(names(upcoming), ["selected", "replacement"]);
}

// A playlist may hold the same Track twice. Reconciliation must resume after
// the occurrence the continuation actually is, not after an earlier duplicate,
// or it replays Tracks that have already been traversed.
#[test]
fn replace_upcoming_collection_matches_the_duplicated_continuation() {
    let mut queue = started(
        ["anchor", "filler", "anchor", "tail"]
            .map(|id| track("spotify", id))
            .into(),
    );
    assert!(queue.advance(TrackEnded));
    assert_eq!(playing(&queue), "filler");
    queue
        .add_to_queue(track("soundcloud", "requested"), None)
        .unwrap();

    // Select the second "anchor" across the request, then walk back onto the
    // request so that occurrence becomes forward history.
    let selected = queue.upcoming()[1].id;
    assert!(queue.advance_to(selected));
    assert_eq!(playing(&queue), "anchor");
    assert!(queue.go_back());
    assert_eq!(playing(&queue), "requested");

    queue.replace_upcoming_collection(CollectionEntry::numbered(
        ["anchor", "filler", "anchor", "appended"].map(|id| track("spotify", id)),
        0,
    ));

    let upcoming = queue.upcoming();
    assert_eq!(upcoming.len(), 2);
    assert_eq!(upcoming[0].id, selected);
    assert_eq!(names(upcoming), ["anchor", "appended"]);
}

#[test]
fn replace_upcoming_collection_drops_deleted_continuation_after_request() {
    let mut queue = started(
        ["first", "deleted", "third"]
            .map(|id| track("spotify", id))
            .into(),
    );
    queue
        .add_to_queue(track("soundcloud", "requested"), None)
        .unwrap();
    let deleted = queue.upcoming()[1].id;

    assert!(queue.advance_to(deleted));
    assert!(queue.go_back());
    queue.replace_upcoming_collection(CollectionEntry::numbered(
        ["first", "replacement"].map(|id| track("spotify", id)),
        0,
    ));

    assert_eq!(names(queue.upcoming()), ["first", "replacement"]);
}

// Legacy trap 6: legacy found the continuation by counting occurrences within
// trimmed history, so duplicates far apart resumed at the wrong one. The
// persisted position finds the right one however far apart they are.
#[test]
fn replace_upcoming_resumes_after_the_right_duplicate_far_apart() {
    let mut tracks: Vec<Track> = (0..120)
        .map(|i| track("spotify", &format!("t{i}")))
        .collect();
    tracks[10] = track("spotify", "same");
    tracks[70] = track("spotify", "same");
    let mut queue = started(tracks.clone());
    for _ in 0..70 {
        assert!(queue.advance(TrackEnded));
    }
    assert_eq!(playing(&queue), "same");
    // Paged collections keep only recent history.
    queue.trim_history(20);

    // The user deletes an early Track, shifting every later position by one.
    tracks.remove(3);
    queue.replace_upcoming_collection(CollectionEntry::numbered(tracks, 0));

    assert_eq!(playing(&queue), "same");
    assert_eq!(names(queue.upcoming())[0], "t71");
    assert_eq!(queue.upcoming().len(), 49);
    assert!(queue.advance(TrackEnded));
    assert_eq!(playing(&queue), "t71");
}

#[test]
fn advance_to_jumps_directly_to_upcoming_entry() {
    let mut queue = started(
        ["one", "two", "three", "four"]
            .map(|id| track("local", id))
            .into(),
    );
    let fourth = queue.upcoming()[2].id;

    assert!(queue.advance_to(fourth));
    assert_eq!(playing(&queue), "four");
    assert!(queue.upcoming().is_empty());
}

#[test]
fn advance_to_preserves_earlier_queued_requests_as_history() {
    let mut queue = started(
        ["current", "continuation"]
            .map(|id| track("local", id))
            .into(),
    );
    queue
        .add_to_queue(track("spotify", "request-one"), None)
        .unwrap();
    queue
        .add_to_queue(track("spotify", "request-two"), None)
        .unwrap();
    let continuation = queue.upcoming().last().unwrap().id;

    assert!(queue.advance_to(continuation));
    assert_eq!(playing(&queue), "continuation");
    assert!(queue.requests().is_empty());
    assert_eq!(
        names(queue.history()),
        ["current", "request-one", "request-two"]
    );
    assert!(queue.go_back());
    assert_eq!(playing(&queue), "request-two");
    assert!(queue.go_back());
    assert_eq!(playing(&queue), "request-one");
}

#[test]
fn advance_to_does_not_duplicate_preserved_current_request() {
    let mut queue = started(
        ["current", "continuation"]
            .map(|id| track("local", id))
            .into(),
    );
    let request_one = queue
        .add_to_queue(track("spotify", "request-one"), None)
        .unwrap();
    queue
        .add_to_queue(track("spotify", "request-two"), None)
        .unwrap();
    let continuation = queue.upcoming().last().unwrap().id;

    assert!(queue.advance_to(request_one));
    assert!(queue.advance_to(continuation));
    assert_eq!(playing(&queue), "continuation");

    let restored = round_trip(&queue);
    assert_eq!(
        names(restored.history()),
        ["current", "request-one", "request-two"]
    );
}

#[test]
fn advance_to_preserves_request_history_before_wrapped_target() {
    let mut queue = started(
        ["first", "second", "third"]
            .map(|id| track("local", id))
            .into(),
    );
    let second = queue.snapshot().collection[1].id;
    assert!(queue.advance(TrackEnded));
    assert!(queue.advance(TrackEnded));
    queue
        .add_to_queue(track("spotify", "request"), None)
        .unwrap();
    queue.set_repeat(RepeatMode::Queue);

    assert!(queue.advance_to(second));
    assert_eq!(playing(&queue), "second");
    assert_eq!(names(queue.upcoming()), ["third"]);

    assert!(queue.go_back());
    assert_eq!(playing(&queue), "first");
    assert!(queue.go_back());
    assert_eq!(playing(&queue), "request");

    assert!(queue.advance_to(second));
    assert!(queue.advance(TrackEnded));
    assert_eq!(playing(&queue), "third");
    let wrapped = queue.upcoming();
    assert_eq!(wrapped.len(), 2);
    let current = queue.current().unwrap().id;
    assert!(
        wrapped
            .iter()
            .all(|entry| entry.origin != Origin::Request && entry.id != current)
    );
    assert!(queue.advance(TrackEnded));
    assert_eq!(playing(&queue), "first");
    assert_eq!(queue.snapshot().collection.len(), 3);
}

#[test]
fn enabling_repeat_after_selection_does_not_replay_preserved_requests() {
    let mut queue = started(
        ["first", "second", "third"]
            .map(|id| track("local", id))
            .into(),
    );
    queue
        .add_to_queue(track("spotify", "request"), None)
        .unwrap();
    let second = queue.snapshot().collection[1].id;

    assert!(queue.advance_to(second));
    queue.set_repeat(RepeatMode::Queue);
    assert!(queue.advance(TrackEnded));
    assert_eq!(playing(&queue), "third");
    assert!(queue.advance(TrackEnded));
    assert_eq!(playing(&queue), "first");
    assert_eq!(queue.snapshot().collection.len(), 3);
}

#[test]
fn undo_restores_preserved_request_in_collection() {
    let mut queue = started(
        ["current", "continuation"]
            .map(|id| track("local", id))
            .into(),
    );
    queue
        .add_to_queue(track("spotify", "request-one"), None)
        .unwrap();
    let request_two = queue
        .add_to_queue(track("spotify", "request-two"), None)
        .unwrap();

    assert!(queue.advance_to(request_two));
    assert!(queue.go_back());
    let removed = queue.remove(request_two).unwrap();
    assert_eq!(removed.lane(), Lane::Collection);
    assert_eq!(removed.entry().origin, Origin::Request);

    assert!(queue.undo_remove(removed));
    assert_eq!(names(queue.upcoming()), ["request-two", "continuation"]);
}

#[test]
fn collection_queueing_preserves_order_and_skips_disabled_tracks() {
    let mut queue = started(vec![track("spotify", "current")]);

    let outcome = queue.add_all_to_queue(
        vec![
            track("soundcloud", "first"),
            unavailable("spotify", "disabled"),
            track("spotify", "third"),
        ],
        None,
    );

    assert_eq!(
        outcome,
        AddOutcome {
            added: 2,
            skipped: 1
        }
    );
    assert_eq!(names(queue.requests()), ["first", "third"]);
}

#[test]
fn duplicate_tracks_remain_distinct_queue_entries() {
    let mut queue = started(vec![track("spotify", "same")]);
    let first = queue.add_to_queue(track("spotify", "same"), None).unwrap();
    let second = queue.add_to_queue(track("spotify", "same"), None).unwrap();

    assert_eq!(queue.requests().len(), 2);
    assert_ne!(first, second);
}

#[test]
fn queue_state_round_trips_while_playing_queued_request() {
    let mut original = started(vec![
        track("spotify", "playlist-1"),
        track("spotify", "playlist-2"),
    ]);
    original
        .add_to_queue(track("soundcloud", "request-1"), None)
        .unwrap();
    original
        .add_to_queue(track("soundcloud", "request-2"), None)
        .unwrap();
    assert!(original.advance(TrackEnded));
    original.set_repeat(RepeatMode::Track);

    let restored = round_trip(&original);

    assert_eq!(playing(&restored), "request-1");
    assert_eq!(restored.repeat(), RepeatMode::Track);
    assert_eq!(names(restored.requests()), ["request-2"]);
    assert_eq!(restored, original);
}

#[test]
fn queue_state_round_trips_with_preserved_request_after_cursor() {
    let mut original = started(
        ["current", "continuation"]
            .map(|id| track("local", id))
            .into(),
    );
    original
        .add_to_queue(track("spotify", "request-one"), None)
        .unwrap();
    let request_two = original
        .add_to_queue(track("spotify", "request-two"), None)
        .unwrap();

    assert!(original.advance_to(request_two));
    assert!(original.go_back());
    assert_eq!(playing(&original), "request-one");

    let restored = round_trip(&original);
    assert_eq!(playing(&restored), "request-one");
    assert_eq!(names(restored.upcoming())[0], "request-two");
}

#[test]
fn paged_collection_history_can_be_trimmed_without_losing_its_cursor() {
    let mut queue = started(
        ["one", "two", "three", "four"]
            .map(|id| track("soundcloud", id))
            .into(),
    );
    assert!(queue.advance(TrackEnded));
    assert!(queue.advance(TrackEnded));
    assert!(queue.advance(TrackEnded));

    assert_eq!(queue.trim_history(2), 1);
    assert_eq!(names(queue.history()), ["two", "three"]);
    assert_eq!(playing(&queue), "four");

    let outcome =
        queue.append_collection(CollectionEntry::numbered([track("soundcloud", "five")], 4));
    assert_eq!(outcome.added, 1);
    assert!(queue.advance(TrackEnded));
    assert_eq!(playing(&queue), "five");
}

#[test]
fn play_next_precedes_fifo_queue_and_retains_request_order() {
    let mut queue = started(["current", "later"].map(|id| track("local", id)).into());
    queue.add_to_queue(track("spotify", "fifo"), None).unwrap();
    queue
        .play_next(track("soundcloud", "next-one"), None)
        .unwrap();
    queue
        .play_next(track("soundcloud", "next-two"), None)
        .unwrap();

    assert_eq!(names(queue.requests()), ["next-one", "next-two", "fifo"]);
}

// Legacy carried the listing connection on every reference. Here only
// server-scoped Tracks carry one, and those are what the connection
// distinguishes.
#[test]
fn appending_a_provider_page_deduplicates_exact_track_identities() {
    let mut queue = started(vec![scoped("first", "primary")]);

    let outcome = queue.append_collection(CollectionEntry::numbered(
        [
            scoped("first", "primary"),
            scoped("first", "secondary"),
            scoped("second", "primary"),
        ],
        1,
    ));

    assert_eq!(
        outcome,
        AddOutcome {
            added: 2,
            skipped: 1
        }
    );
    assert_eq!(queue.snapshot().collection.len(), 3);
}

// A direct selection keeps a request inside the collection, while playing the
// same request in order leaves it detached. Page deduplication must not
// depend on which happened, so request history never suppresses a Track's
// real occurrence on a later page.
#[test]
fn appending_a_provider_page_ignores_preserved_request_history() {
    let mut queue = started(vec![
        track("soundcloud", "first"),
        track("soundcloud", "second"),
    ]);

    // Queue a Track that also lives on a page not loaded yet, and reach it by
    // direct selection so it is kept in the collection.
    let request = queue
        .add_to_queue(track("soundcloud", "later-page"), None)
        .unwrap();
    assert!(queue.advance_to(request));
    assert_eq!(queue.current().unwrap().origin, Origin::Request);

    let outcome = queue.append_collection(CollectionEntry::numbered(
        [track("soundcloud", "later-page")],
        2,
    ));
    assert_eq!(
        outcome,
        AddOutcome {
            added: 1,
            skipped: 0
        }
    );

    // The page occurrence keeps its own place at the tail of the collection.
    assert!(queue.advance(TrackEnded));
    assert_eq!(playing(&queue), "second");
    assert!(queue.advance(TrackEnded));
    assert_eq!(playing(&queue), "later-page");
    assert!(matches!(
        queue.current().unwrap().origin,
        Origin::Collection { .. }
    ));
}

#[test]
fn queue_removal_can_be_undone_at_its_original_position() {
    let mut queue = started(vec![track("local", "current")]);
    let first = queue.add_to_queue(track("spotify", "first"), None).unwrap();
    queue
        .add_to_queue(track("spotify", "second"), None)
        .unwrap();

    let removed = queue.remove(first).unwrap();
    assert_eq!(queue.requests().len(), 1);
    assert!(queue.undo_remove(removed));
    assert_eq!(queue.requests()[0].id, first);
}

#[test]
fn queued_requests_can_move_and_upcoming_can_clear() {
    let mut queue = started(
        ["current", "collection-next"]
            .map(|id| track("local", id))
            .into(),
    );
    let first = queue.add_to_queue(track("spotify", "first"), None).unwrap();
    queue
        .add_to_queue(track("spotify", "second"), None)
        .unwrap();

    assert!(queue.move_request(first, 1));
    assert_eq!(names(queue.requests())[0], "second");
    let collection_next = queue.upcoming().last().unwrap().id;
    assert!(queue.move_request(collection_next, 0));
    assert_eq!(names(queue.requests())[0], "collection-next");
    assert_eq!(queue.clear_upcoming(), 3);
    assert!(queue.requests().is_empty());
    assert!(queue.upcoming().is_empty());
    assert_eq!(playing(&queue), "current");
}

#[test]
fn pruning_expired_track_occurrences_preserves_only_the_active_occurrence() {
    let expired = track("party-local", "file");
    let mut queue = started(vec![
        expired.clone(),
        track("spotify", "next"),
        expired.clone(),
    ]);
    let current = queue.current().unwrap().id;
    queue.add_to_queue(expired.clone(), None).unwrap();

    queue.prune(std::slice::from_ref(&expired.reference), Some(current));
    assert_eq!(queue.current().unwrap().id, current);
    assert_eq!(queue.upcoming().len(), 1);
    assert!(queue.advance(TrackEnded));

    queue.prune(std::slice::from_ref(&expired.reference), None);
    assert_eq!(playing(&queue), "next");
    assert!(queue.history().is_empty());
    round_trip(&queue);
}

#[test]
fn pruning_a_play_next_entry_keeps_the_following_request_out_of_the_prefix() {
    let mut queue = started(vec![track("spotify", "current")]);
    let expired = track("party-local", "guest-file");
    // An ordinary request, then a Play Next ahead of it: the prefix is the file.
    queue
        .add_to_queue(track("spotify", "request"), None)
        .unwrap();
    queue.play_next(expired.clone(), None).unwrap();
    assert_eq!(names(queue.requests())[0], "guest-file");

    // The guest leaves and its pending file is pruned. The ordinary request
    // must not inherit the vacated Play Next place.
    queue.prune(&[expired.reference], None);
    assert_eq!(queue.requests().len(), 1);
    queue.play_next(track("spotify", "later"), None).unwrap();
    assert_eq!(names(queue.requests()), ["later", "request"]);
}

#[test]
fn pruning_final_current_occurrence_does_not_leave_a_queue_reference() {
    let expired = track("party-local", "last");
    let mut queue = started(vec![expired.clone()]);

    queue.prune(&[expired.reference], None);

    assert_eq!(queue.current(), None);
    assert!(queue.snapshot().collection.is_empty());
    assert!(!queue.advance(TrackEnded));
}

// Behavior beyond the legacy suite.

#[test]
fn a_collection_that_cannot_start_leaves_the_queue_unchanged() {
    let mut queue = started(vec![track("spotify", "playing")]);
    let before = queue.snapshot();

    let outcome = queue.start_collection(
        CollectionEntry::numbered([track("spotify", "early"), unavailable("spotify", "x")], 0),
        1,
    );

    assert!(!outcome.started);
    assert_eq!(queue.snapshot(), before);
}

#[test]
fn going_back_from_a_request_returns_it_to_the_front_of_the_lane() {
    let mut queue = started(["first", "second"].map(|id| track("local", id)).into());
    queue.add_to_queue(track("spotify", "fifo"), None).unwrap();
    queue.play_next(track("spotify", "next"), None).unwrap();
    assert!(queue.advance(TrackEnded));
    assert_eq!(playing(&queue), "next");

    assert!(queue.go_back());
    assert_eq!(playing(&queue), "first");
    assert_eq!(names(queue.upcoming()), ["next", "fifo", "second"]);
    // It still counts as Play Next.
    queue.play_next(track("spotify", "another"), None).unwrap();
    assert_eq!(names(queue.requests()), ["next", "another", "fifo"]);
}

#[test]
fn going_back_after_the_end_replays_the_last_entry() {
    let mut queue = started(["first", "last"].map(|id| track("local", id)).into());
    assert!(queue.advance(TrackEnded));
    assert!(!queue.advance(TrackEnded));
    assert_eq!(queue.current(), None);

    assert!(queue.go_back());
    assert_eq!(playing(&queue), "last");
}

#[test]
fn undo_puts_a_play_next_request_back_in_the_play_next_group() {
    let mut queue = started(vec![track("local", "current")]);
    queue.add_to_queue(track("spotify", "fifo"), None).unwrap();
    let next = queue.play_next(track("spotify", "next"), None).unwrap();

    let removed = queue.remove(next).unwrap();
    assert!(queue.undo_remove(removed));
    queue.play_next(track("spotify", "another"), None).unwrap();

    assert_eq!(names(queue.requests()), ["next", "another", "fifo"]);
}

#[test]
fn a_seed_gives_the_same_order_however_often_shuffle_is_reapplied() {
    let mut queue = started((0..20).map(|i| track("local", &format!("t{i}"))).collect());
    queue.shuffle_upcoming(1);
    let first = queue.snapshot().collection;

    queue.shuffle_upcoming(2);
    queue.shuffle_upcoming(1);

    assert_eq!(queue.snapshot().collection, first);
}

#[test]
fn an_edit_while_shuffled_shuffles_the_new_tail() {
    let tracks: Vec<Track> = (0..20).map(|i| track("local", &format!("t{i}"))).collect();
    let mut queue = started(tracks.clone());
    queue.shuffle_upcoming(42);

    queue.replace_upcoming_collection(CollectionEntry::numbered(tracks, 0));

    assert_eq!(playing(&queue), "t0");
    assert_eq!(queue.snapshot().shuffle, Some(42));
    let upcoming = names(queue.upcoming());
    let natural: Vec<String> = (1..20).map(|i| format!("t{i}")).collect();
    assert_ne!(upcoming, natural);
    let mut sorted: Vec<String> = upcoming.iter().map(|s| s.to_string()).collect();
    sorted.sort();
    let mut expected = natural.clone();
    expected.sort();
    assert_eq!(sorted, expected);

    queue.restore_upcoming_order();
    assert_eq!(names(queue.upcoming()), natural);
}

#[test]
fn history_and_the_current_entry_cannot_be_removed() {
    let mut queue = started(["first", "second"].map(|id| track("local", id)).into());
    let first = queue.current().unwrap().id;
    assert!(queue.advance(TrackEnded));
    let second = queue.current().unwrap().id;

    assert_eq!(queue.remove(first), None);
    assert_eq!(queue.remove(second), None);
}

#[test]
fn appending_a_sequence_page_keeps_intentional_duplicates_and_skips_known_slots() {
    use crate::sequence::SlotId;
    let entry = |id: &str, position: u64, slot: u64| CollectionEntry {
        slot: Some(SlotId(slot)),
        ..CollectionEntry::new(track("spotify", id), position)
    };
    let mut queue = CanonicalQueue::new();
    assert!(
        queue
            .start_collection(vec![entry("same", 0, 0), entry("other", 1, 1)], 0)
            .started
    );

    let outcome = queue.append_collection(vec![entry("other", 1, 1), entry("same", 2, 2)]);

    assert_eq!(
        outcome,
        AddOutcome {
            added: 1,
            skipped: 1
        }
    );
    assert_eq!(names(queue.upcoming()), ["other", "same"]);
}

// Property tests: random operation sequences keep every invariant, and every
// state restores to an equal queue.

fn random_operations(seed: u64, steps: usize) {
    let mut random = Random::new(seed);
    let mut queue = CanonicalQueue::new();
    let pool: Vec<Track> = (0..6)
        .map(|i| {
            if i == 5 {
                unavailable("spotify", "gone")
            } else {
                track("spotify", &format!("t{i}"))
            }
        })
        .collect();
    let pick = |random: &mut Random| pool[random.below(pool.len() as u64) as usize].clone();
    let entry_id = |queue: &CanonicalQueue, random: &mut Random| {
        let state = queue.snapshot();
        let ids: Vec<EntryId> = state
            .collection
            .iter()
            .chain(&state.requests)
            .map(|entry| entry.id)
            .collect();
        if ids.is_empty() {
            EntryId(0)
        } else {
            ids[random.below(ids.len() as u64) as usize]
        }
    };
    let mut removed = Vec::new();

    for step in 0..steps {
        match random.below(17) {
            0 => {
                let tracks: Vec<Track> = (0..random.below(8)).map(|_| pick(&mut random)).collect();
                let start = random.below(4) as usize;
                queue.start_collection(CollectionEntry::numbered(tracks, 0), start);
            }
            1 => {
                let _ = queue.add_to_queue(pick(&mut random), None);
            }
            2 => {
                let _ = queue.play_next(pick(&mut random), None);
            }
            3 => {
                let first = random.below(10);
                let tracks: Vec<Track> = (0..random.below(4)).map(|_| pick(&mut random)).collect();
                queue.append_collection(CollectionEntry::numbered(tracks, first));
            }
            4 => {
                let tracks: Vec<Track> = (0..random.below(8)).map(|_| pick(&mut random)).collect();
                queue.replace_upcoming_collection(CollectionEntry::numbered(tracks, 0));
            }
            5 | 6 => {
                queue.advance(if random.below(2) == 0 {
                    TrackEnded
                } else {
                    Skip
                });
            }
            7 => {
                let target = entry_id(&queue, &mut random);
                queue.advance_to(target);
            }
            8 => {
                queue.go_back();
            }
            9 => {
                let target = entry_id(&queue, &mut random);
                removed.extend(queue.remove(target));
            }
            10 => {
                if let Some(entry) = removed.pop() {
                    queue.undo_remove(entry);
                }
            }
            11 => {
                let target = entry_id(&queue, &mut random);
                queue.move_request(target, random.below(5) as usize);
            }
            12 => {
                if random.below(4) == 0 {
                    queue.clear_upcoming();
                }
            }
            13 => {
                let keep = queue.current().map(|entry| entry.id);
                queue.prune(
                    &[pick(&mut random).reference],
                    keep.filter(|_| random.below(2) == 0),
                );
            }
            14 => {
                queue.trim_history(random.below(3) as usize);
            }
            15 => {
                queue.set_repeat(match random.below(3) {
                    0 => RepeatMode::Off,
                    1 => RepeatMode::Track,
                    _ => RepeatMode::Queue,
                });
            }
            _ => {
                let before: Vec<EntryId> = {
                    let mut ids: Vec<_> = queue.upcoming().iter().map(|e| e.id).collect();
                    ids.sort_unstable();
                    ids
                };
                if random.below(2) == 0 {
                    queue.shuffle_upcoming(random.below(u64::MAX));
                } else {
                    queue.restore_upcoming_order();
                }
                let mut after: Vec<EntryId> = queue.upcoming().iter().map(|e| e.id).collect();
                after.sort_unstable();
                assert_eq!(before, after, "seed {seed} step {step}: not a permutation");
            }
        }

        let state = queue.snapshot();
        state
            .check()
            .unwrap_or_else(|error| panic!("seed {seed} step {step}: {error}"));
        assert_eq!(
            CanonicalQueue::restore(state).unwrap(),
            queue,
            "seed {seed} step {step}"
        );
        assert!(
            state_is_consistent(&queue),
            "seed {seed} step {step}: {:?}",
            queue.snapshot()
        );
    }
}

/// Invariants beyond `QueueState::check`: no Disabled Track entered, and an
/// id appears at most once in what is published as upcoming.
fn state_is_consistent(queue: &CanonicalQueue) -> bool {
    let state = queue.snapshot();
    let queueable = state
        .collection
        .iter()
        .chain(&state.requests)
        .all(|entry| entry.track.is_queueable());
    let mut published: Vec<EntryId> = queue.upcoming().iter().map(|entry| entry.id).collect();
    published.extend(queue.current().map(|entry| entry.id));
    let count = published.len();
    published.sort_unstable();
    published.dedup();
    queueable && published.len() == count
}

#[test]
fn random_operations_keep_every_invariant() {
    for seed in 0..300 {
        random_operations(seed, 200);
    }
}
