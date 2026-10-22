//! The offset-collection rules of `docs/plugin-api/walkthrough.md`
//! ("Spotify Liked Songs: positional handles"), one test per step, plus a
//! randomized drift run.

use std::collections::HashSet;

use ristle_domain::{Addressing, CollectionDescriptor, CollectionRef, Count, TrackRef};

use super::*;
use crate::random::Random;

fn like(index: u64) -> TrackRef {
    TrackRef::catalog("spotify".into(), format!("like-{index}")).unwrap()
}

/// Spotify's view of Liked Songs: the newest like first.
struct Liked(Vec<TrackRef>);

impl Liked {
    fn new(count: u64) -> Self {
        Self((0..count).map(like).collect())
    }

    /// Reads `len` entries at `offset`, as `GET /v1/me/tracks` would.
    fn read(&self, offset: u64, len: u64) -> ObservedPage {
        let start = (offset as usize).min(self.0.len());
        let end = ((offset + len) as usize).min(self.0.len());
        ObservedPage {
            source: 0,
            offset,
            tracks: self.0[start..end].to_vec(),
            revision: Some(self.0.len().to_string()),
            total: Some(Count::Exact(self.0.len() as u64)),
        }
    }
}

/// Shuffle Play over `count` Liked Songs known only by position.
fn shuffled_liked(count: u64, seed: u64) -> Sequence {
    let source = SequenceSource::Collection(CollectionDescriptor {
        reference: CollectionRef {
            connection: "spotify-1".into(),
            id: "liked".into(),
        },
        addressing: Addressing::Offset,
        total: Some(Count::Exact(count)),
        revision: Some(count.to_string()),
        endless: false,
    });
    let slots = (0..count)
        .map(|offset| Slot {
            id: SlotId(offset),
            source: 0,
            offset,
            track: None,
        })
        .collect();
    Sequence::start(
        SequenceDescriptor {
            id: SequenceId("liked:spotify-1".into()),
            title: "Liked Songs".into(),
            opened: Opened::LikedTracks,
            sources: vec![source],
        },
        slots,
        StartAt::Beginning,
        Some(seed),
    )
    .unwrap()
}

/// Reads the page around every unresolved slot of the hydration plan, as the
/// session does before it materializes the plan.
fn hydrate(sequence: &mut Sequence, liked: &Liked, lookahead: usize) {
    for _ in 0..100 {
        let unresolved: Vec<u64> = sequence
            .hydration_plan(lookahead)
            .into_iter()
            .filter_map(|id| sequence.slot(id))
            .filter(|slot| slot.track.is_none())
            .map(|slot| slot.offset)
            .collect();
        let Some(&offset) = unresolved.first() else {
            return;
        };
        sequence.rebase(liked.read(offset / 50 * 50, 50));
    }
    panic!("hydration did not settle");
}

fn track_of(sequence: &Sequence, id: SlotId) -> Option<TrackRef> {
    sequence.slot(id).and_then(|slot| slot.track.clone())
}

// Step 1: 5,000 Liked Songs shuffle without a single read; the order is a
// permutation of positions and nothing names a Track yet.
#[test]
fn shuffle_without_fetching_orders_positions() {
    let sequence = shuffled_liked(5000, 42);

    let order: HashSet<SlotId> = sequence.state().order.iter().copied().collect();
    assert_eq!(order.len(), 5000);
    assert!(
        sequence
            .state()
            .slots
            .iter()
            .all(|slot| slot.track.is_none())
    );
    let plan = sequence.hydration_plan(10);
    assert_eq!(plan.len(), 11);
    assert_eq!(plan[0], sequence.current().unwrap());
}

// Step 2: a slot is read before it is shown or played, and keeps the Track it
// was read as, even after the collection moves.
#[test]
fn resolve_before_showing_keeps_the_track_it_read() {
    let mut liked = Liked::new(500);
    let mut sequence = shuffled_liked(500, 7);

    hydrate(&mut sequence, &liked, 10);
    let plan = sequence.hydration_plan(10);
    let seen: Vec<(SlotId, TrackRef)> = plan
        .iter()
        .map(|&id| (id, track_of(&sequence, id).unwrap()))
        .collect();
    for (id, track) in &seen {
        assert_eq!(track, &liked.0[sequence.slot(*id).unwrap().offset as usize]);
    }
    assert!(sequence.set_materialized_end(11));

    // Two likes elsewhere, then every page is read again.
    liked.0.insert(0, like(900));
    liked.0.insert(0, like(901));
    for offset in (0..550).step_by(50) {
        sequence.rebase(liked.read(offset, 50));
    }

    for (id, track) in seen {
        assert_eq!(track_of(&sequence, id), Some(track));
    }
}

// Step 3: a like made in Ristle shifts every position behind it and joins
// what is still to come; an unlike leaves it and shifts positions back.
// Nothing resolved moves and nothing reshuffles.
#[test]
fn local_like_and_unlike_are_applied_not_voided() {
    let mut liked = Liked::new(200);
    let mut sequence = shuffled_liked(200, 3);
    hydrate(&mut sequence, &liked, 10);
    assert!(sequence.set_materialized_end(11));
    let visible = sequence.state().order[..11].to_vec();
    let before = sequence.state().clone();

    let new = like(999);
    liked.0.insert(0, new.clone());
    let outcome = sequence.apply_local_change(CollectionChange::Added {
        source: 0,
        at: 0,
        track: new.clone(),
    });

    let inserted = outcome.inserted.unwrap();
    let order = &sequence.state().order;
    assert_eq!(order[..11], visible[..], "the visible order is untouched");
    assert!(order.iter().position(|&id| id == inserted).unwrap() >= 11);
    let without_new: Vec<SlotId> = order.iter().copied().filter(|&id| id != inserted).collect();
    assert_eq!(without_new, before.order, "nothing reshuffled");
    for slot in &before.slots {
        assert_eq!(sequence.slot(slot.id).unwrap().offset, slot.offset + 1);
    }

    // Unlike a Track still to come whose position is known.
    let upcoming = sequence.state().order[20];
    let gone = sequence.slot(upcoming).unwrap().clone();
    let gone = Slot {
        track: Some(liked.0[gone.offset as usize].clone()),
        ..gone
    };
    sequence.resolve(vec![(gone.id, gone.track.clone().unwrap())]);
    liked.0.remove(gone.offset as usize);
    let outcome = sequence.apply_local_change(CollectionChange::Removed {
        source: 0,
        track: gone.track.clone().unwrap(),
        at: None,
    });
    assert_eq!(outcome.removed, [gone.id]);
    assert!(sequence.slot(gone.id).is_none());

    // The bookkeeping matches the Provider: every page reads without drift.
    hydrate(&mut sequence, &liked, 200);
    for slot in &sequence.state().slots {
        assert_eq!(slot.track.as_ref(), Some(&liked.0[slot.offset as usize]));
    }
}

// Step 4: likes made elsewhere are found on a re-read. The known Tracks on the
// page tell how far positions moved; unresolved positions are moved by that
// much, so they land on the Tracks they stood for.
#[test]
fn likes_elsewhere_are_rebased_from_known_tracks() {
    let mut liked = Liked::new(100);
    let mut sequence = shuffled_liked(100, 11);
    sequence.rebase(liked.read(0, 50));
    let old = liked.0.clone();

    for index in 0..3 {
        liked.0.insert(0, like(500 + index));
    }
    let outcome = sequence.rebase(liked.read(50, 50));

    assert_eq!(outcome.shift, 3);
    assert!(!outcome.resolved.is_empty());
    // Each position read lands on the Track it stood for before the likes.
    for id in &outcome.resolved {
        let slot = sequence.slot(*id).unwrap();
        assert_eq!(slot.track.as_ref(), Some(&liked.0[slot.offset as usize]));
        assert_eq!(slot.track.as_ref(), Some(&old[slot.offset as usize - 3]));
    }
    // Every Track that was liked before still has exactly one slot.
    hydrate(&mut sequence, &liked, 200);
    let tracks: Vec<&TrackRef> = sequence
        .state()
        .slots
        .iter()
        .filter_map(|slot| slot.track.as_ref())
        .collect();
    assert_eq!(tracks.iter().collect::<HashSet<_>>().len(), tracks.len());
    for track in &old {
        assert!(tracks.contains(&track), "{track:?} lost");
    }
}

// Step 4: new likes discovered at the top join what is still to come.
#[test]
fn new_likes_found_at_the_top_join_the_remaining_order() {
    let mut liked = Liked::new(100);
    let mut sequence = shuffled_liked(100, 5);
    sequence.rebase(liked.read(0, 50));
    assert!(sequence.set_materialized_end(1));

    liked.0.insert(0, like(777));
    let outcome = sequence.rebase(liked.read(0, 50));

    assert_eq!(outcome.shift, 1);
    assert_eq!(outcome.inserted.len(), 1);
    let new = outcome.inserted[0];
    assert_eq!(track_of(&sequence, new), Some(like(777)));
    let at = sequence
        .state()
        .order
        .iter()
        .position(|&id| id == new)
        .unwrap();
    assert!(at >= 1, "not before what was already materialized");
}

// Step 4: a Track already played in this pass is not played again when drift
// makes an unresolved position land on it.
#[test]
fn a_track_played_in_this_pass_is_not_played_again_after_drift() {
    let mut liked = Liked::new(100);
    let mut sequence = shuffled_liked(100, 13);
    // Play the first slot of the shuffle.
    hydrate(&mut sequence, &liked, 0);
    let played = sequence.current().unwrap();
    let played_track = track_of(&sequence, played).unwrap();
    let played_at = sequence.slot(played).unwrap().offset;
    assert!(sequence.set_materialized_end(1));

    // Elsewhere, the Tracks around it are unliked, so it moves to positions
    // the host has not read under the new revision.
    let first = played_at.saturating_sub(5);
    for _ in first..played_at {
        liked.0.remove(first as usize);
    }
    hydrate(&mut sequence, &liked, 100);

    let holding: Vec<&Slot> = sequence
        .state()
        .slots
        .iter()
        .filter(|slot| slot.track.as_ref() == Some(&played_track))
        .collect();
    assert_eq!(holding.len(), 1, "{played_track:?} is held twice");
    assert_eq!(holding[0].id, played);
}

// Random drift elsewhere (likes at the top, unlikes anywhere) between reads:
// no Track ever ends up in two slots, the played prefix never changes, and
// hydration always settles.
#[test]
fn random_drift_never_repeats_a_track() {
    for seed in 0..60 {
        let mut random = Random::new(seed);
        let count = 50 + random.below(250);
        let mut liked = Liked::new(count);
        let mut sequence = shuffled_liked(count, seed);
        let mut next_like = 10_000;

        for _ in 0..12 {
            hydrate(&mut sequence, &liked, 10);
            let prefix: Vec<SlotId> = {
                let cursor = sequence.state().cursor.map_or(0, |c| c + 1);
                sequence.state().order[..cursor].to_vec()
            };
            let materialized = sequence
                .state()
                .materialized_end
                .max(sequence.state().cursor.map_or(0, |c| c + 1));
            sequence.set_materialized_end(materialized.min(sequence.state().order.len()));
            for _ in 0..random.below(4) {
                if random.below(2) == 0 {
                    liked.0.insert(0, like(next_like));
                    next_like += 1;
                } else if liked.0.len() > 10 {
                    let at = random.below(liked.0.len() as u64) as usize;
                    liked.0.remove(at);
                }
            }
            for _ in 0..random.below(5) {
                sequence.advance();
            }
            hydrate(&mut sequence, &liked, 10);

            let order = &sequence.state().order;
            assert_eq!(
                order[..prefix.len()],
                prefix[..],
                "seed {seed}: prefix moved"
            );
            let tracks: Vec<&TrackRef> = sequence
                .state()
                .slots
                .iter()
                .filter_map(|slot| slot.track.as_ref())
                .collect();
            let unique: HashSet<_> = tracks.iter().collect();
            assert_eq!(
                unique.len(),
                tracks.len(),
                "seed {seed}: a Track is held twice"
            );
            Sequence::restore(sequence.state().clone())
                .unwrap_or_else(|error| panic!("seed {seed}: {error}"));
        }
    }
}

// An unlike at a known position whose slot is still unresolved removes that
// slot, so no two slots end up standing for one position.
#[test]
fn an_unlike_at_an_unresolved_position_removes_its_slot() {
    let mut liked = Liked::new(100);
    let mut sequence = shuffled_liked(100, 21);
    let upcoming = sequence.state().order[30];
    let at = sequence.slot(upcoming).unwrap().offset;
    let revision = sequence.state().revision;

    let gone = liked.0.remove(at as usize);
    let outcome = sequence.apply_local_change(CollectionChange::Removed {
        source: 0,
        track: gone,
        at: Some(at),
    });

    assert_eq!(outcome.removed, [upcoming]);
    assert_ne!(sequence.state().revision, revision);
    hydrate(&mut sequence, &liked, 200);
    for slot in &sequence.state().slots {
        assert_eq!(slot.track.as_ref(), Some(&liked.0[slot.offset as usize]));
    }
    assert_eq!(sequence.state().slots.len(), 99);
}

// An unlike of a Track already played removes nothing still to come, but
// the positions behind it move, so earlier plans are stale.
#[test]
fn an_unlike_that_only_moves_positions_changes_the_revision() {
    let mut liked = Liked::new(100);
    let mut sequence = shuffled_liked(100, 22);
    hydrate(&mut sequence, &liked, 0);
    let current = sequence.current().unwrap();
    let played = sequence.slot(current).unwrap().clone();
    let revision = sequence.state().revision;

    liked.0.remove(played.offset as usize);
    let outcome = sequence.apply_local_change(CollectionChange::Removed {
        source: 0,
        track: played.track.unwrap(),
        at: None,
    });

    assert!(outcome.removed.is_empty());
    assert_ne!(sequence.state().revision, revision);
}

// Ranges and counters saturate or wrap instead of overflowing (ADR 0001: the
// core never panics across the boundary).
#[test]
fn extreme_lookahead_and_revision_do_not_overflow() {
    let mut sequence = shuffled_liked(10, 1);
    assert!(sequence.advance());
    assert_eq!(sequence.window(usize::MAX, usize::MAX).len(), 10);
    assert_eq!(sequence.hydration_plan(usize::MAX).len(), 9, "from the cursor on");

    let mut state = sequence.state().clone();
    state.revision = u64::MAX;
    let mut sequence = Sequence::restore(state).unwrap();
    sequence.set_shuffle(None);
    sequence.restart();
    assert_eq!(sequence.state().revision, 1);
}
