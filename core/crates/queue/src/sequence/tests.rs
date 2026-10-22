//! Walking and shuffling sequences. The legacy tests of
//! `playable_sequence_test.cpp` keep their names (`docs/plan/phase-2.md`,
//! section 8).

use std::collections::HashSet;

use ristle_domain::{Addressing, CollectionDescriptor, CollectionRef, PlaylistId, TrackRef};

use super::*;
use crate::error::InvalidSequence;
use crate::random::Random;

fn descriptor(sources: Vec<SequenceSource>) -> SequenceDescriptor {
    SequenceDescriptor {
        id: SequenceId("ristle-playlist:p1".into()),
        title: "Large playlist".into(),
        opened: Opened::RistlePlaylist(PlaylistId::new("p1")),
        sources,
    }
}

/// `count` resolved slots of one Ristle Playlist; slot `i` sits at offset `i`.
fn slots(count: u64) -> Vec<Slot> {
    (0..count)
        .map(|index| Slot {
            id: SlotId(index),
            source: 0,
            offset: index,
            track: Some(TrackRef::catalog("spotify".into(), format!("track-{index}")).unwrap()),
        })
        .collect()
}

fn playlist(count: u64, at: StartAt, shuffle: Option<u64>) -> Sequence {
    Sequence::start(
        descriptor(vec![SequenceSource::RistlePlaylist(PlaylistId::new("p1"))]),
        slots(count),
        at,
        shuffle,
    )
    .unwrap()
}

fn order(sequence: &Sequence) -> Vec<u64> {
    sequence.state().order.iter().map(|id| id.0).collect()
}

fn ids(range: impl IntoIterator<Item = u64>) -> Vec<SlotId> {
    range.into_iter().map(SlotId).collect()
}

#[test]
fn shuffle_play_permutes_every_occurrence_exactly_once() {
    let sequence = playlist(1000, StartAt::Beginning, Some(42));
    let order = order(&sequence);

    assert_eq!(order.len(), 1000);
    assert_eq!(order.iter().collect::<HashSet<_>>().len(), 1000);
    assert!(!order.is_sorted());
    assert_eq!(sequence.state().cursor, Some(0));
    assert_eq!(sequence.state().shuffle, Some(42));
}

// One cohort spans SLICES source pages and is used up by rounds of one
// rolling window each, so a cohort covers SLICES * PAGE occurrences and every
// round still draws from all of that cohort's pages.
#[test]
fn shuffle_walks_eight_random_source_slices_per_large_batch() {
    let slices = shuffle::SLICES;
    let page = 50;
    let cohort = slices * page;
    let sequence = playlist(6000, StartAt::Beginning, Some(42));
    let shuffled = order(&sequence);
    let page_of = |position: usize| shuffled[position] / 50;

    let mut first_cohort_pages = HashSet::new();
    for round in 0..slices {
        let round_pages: HashSet<_> = (round * page..(round + 1) * page).map(page_of).collect();
        assert_eq!(round_pages.len(), slices, "round {round}");
        first_cohort_pages.extend(round_pages);
    }
    assert_eq!(first_cohort_pages.len(), slices);

    let second_cohort_pages: HashSet<_> = (cohort..2 * cohort).map(page_of).collect();
    assert_eq!(second_cohort_pages.len(), slices);
    assert!(first_cohort_pages.is_disjoint(&second_cohort_pages));
}

// A round is emitted interleaved, so a rolling window does not read as one
// contiguous run per source page.
#[test]
fn shuffle_interleaves_each_round_across_its_source_pages() {
    let sequence = playlist(6000, StartAt::Beginning, Some(42));
    let shuffled = order(&sequence);

    let page_changes = (1..50)
        .filter(|&position| shuffled[position] / 50 != shuffled[position - 1] / 50)
        .count();
    // Eight contiguous runs would change page only seven times.
    assert!(page_changes > 20, "{page_changes} page changes");
}

#[test]
fn enabling_shuffle_keeps_current_and_consumed_prefix_stable() {
    let mut sequence = playlist(12, StartAt::Slot(SlotId(3)), None);
    let before = order(&sequence);

    sequence.set_shuffle(Some(42));

    assert_eq!(sequence.current(), Some(SlotId(3)));
    let after = order(&sequence);
    assert_eq!(after[..4], before[..4]);
    assert_ne!(after[4..], before[4..]);
}

#[test]
fn disabling_shuffle_restores_natural_unconsumed_order() {
    let mut sequence = playlist(12, StartAt::Slot(SlotId(3)), Some(42));

    sequence.set_shuffle(None);

    assert_eq!(sequence.current(), Some(SlotId(3)));
    // Natural order after the current slot, then the slots before it: every
    // slot still plays once in this pass.
    assert_eq!(order(&sequence), [3, 4, 5, 6, 7, 8, 9, 10, 11, 0, 1, 2]);
    assert_eq!(sequence.state().shuffle, None);
}

#[test]
fn sequence_window_bounds_hydrated_history_and_lookahead() {
    let sequence = playlist(1000, StartAt::Slot(SlotId(500)), None);

    let window = sequence.window(50, 50);

    assert_eq!(window.len(), 101);
    assert_eq!(window.first(), Some(&SlotId(450)));
    assert_eq!(window.last(), Some(&SlotId(550)));
}

#[test]
fn sequence_hydration_plan_contains_only_missing_lookahead() {
    let mut sequence = playlist(1000, StartAt::Slot(SlotId(500)), None);
    assert!(sequence.set_materialized_end(511));

    let missing = sequence.hydration_plan(50);

    assert_eq!(missing.len(), 40);
    assert_eq!(missing.first(), Some(&SlotId(511)));
    assert_eq!(missing.last(), Some(&SlotId(550)));
}

// Behavior beyond the legacy suite.

#[test]
fn starting_at_a_slot_with_shuffle_plays_every_other_slot_after_it() {
    let sequence = playlist(12, StartAt::Slot(SlotId(5)), Some(7));

    let order = order(&sequence);
    assert_eq!(order[0], 5);
    assert_eq!(sequence.current(), Some(SlotId(5)));
    let mut rest = order[1..].to_vec();
    rest.sort_unstable();
    assert_eq!(rest, [0, 1, 2, 3, 4, 6, 7, 8, 9, 10, 11]);
}

#[test]
fn a_seed_gives_the_same_order_however_often_shuffle_is_toggled() {
    let mut sequence = playlist(200, StartAt::Slot(SlotId(10)), None);
    sequence.set_shuffle(Some(1));
    let first = order(&sequence);

    sequence.set_shuffle(Some(2));
    sequence.set_shuffle(None);
    sequence.set_shuffle(Some(1));

    assert_eq!(order(&sequence), first);
}

#[test]
fn changing_shuffle_invalidates_the_materialized_lookahead() {
    let mut sequence = playlist(100, StartAt::Slot(SlotId(10)), None);
    assert!(sequence.set_materialized_end(30));
    let revision = sequence.state().revision;

    sequence.set_shuffle(Some(9));

    assert_eq!(sequence.state().materialized_end, 11);

    // A current slot that was never materialized stays in the plan.
    let mut fresh = playlist(100, StartAt::Slot(SlotId(10)), None);
    fresh.set_shuffle(Some(9));
    assert_eq!(fresh.state().materialized_end, 10);
    assert!(sequence.state().revision > revision);
}

#[test]
fn walking_keeps_the_materialized_end_past_the_cursor() {
    let mut sequence = playlist(10, StartAt::Beginning, None);
    assert!(sequence.advance_to(SlotId(6)));
    assert_eq!(sequence.state().materialized_end, 6);
    assert!(
        sequence.advance_to(SlotId(6)),
        "the current slot is reached"
    );
    assert!(
        !sequence.advance_to(SlotId(2)),
        "advance_to looks ahead only"
    );
    assert!(sequence.seek_to(SlotId(2)));
    assert_eq!(sequence.current(), Some(SlotId(2)));
    assert!(sequence.go_back());
    assert!(sequence.advance());
    assert_eq!(sequence.current(), Some(SlotId(2)));
    assert!(!sequence.set_materialized_end(1), "before the cursor");
    assert!(!sequence.set_materialized_end(11), "beyond the order");
}

#[test]
fn restart_replays_the_same_permutation() {
    let mut sequence = playlist(20, StartAt::Beginning, Some(3));
    let permutation = order(&sequence);
    while sequence.advance() {}
    assert!(!sequence.advance());

    sequence.restart();

    assert_eq!(sequence.current(), None);
    assert_eq!(sequence.hydration_plan(4), ids(permutation[..5].to_vec()));
    assert!(sequence.advance());
    assert_eq!(sequence.current(), Some(SlotId(permutation[0])));
    assert_eq!(order(&sequence), permutation);
}

#[test]
fn refill_walks_on_from_the_materialized_end() {
    let mut sequence = playlist(100, StartAt::Beginning, None);
    assert!(sequence.set_materialized_end(40));

    assert_eq!(sequence.refill_batch(25), ids(40..65));
    assert_eq!(sequence.hydration_plan(10), Vec::<SlotId>::new());
}

#[test]
fn resolve_fills_only_unresolved_slots() {
    let liked = SequenceSource::Collection(CollectionDescriptor {
        reference: CollectionRef {
            connection: "spotify-1".into(),
            id: "liked".into(),
        },
        addressing: Addressing::Offset,
        total: None,
        revision: None,
        endless: false,
    });
    let mut positions = slots(4);
    for slot in &mut positions[1..] {
        slot.track = None;
    }
    let mut sequence =
        Sequence::start(descriptor(vec![liked]), positions, StartAt::Beginning, None).unwrap();
    let track = |id: &str| TrackRef::catalog("spotify".into(), id.into()).unwrap();

    let resolved = sequence.resolve(vec![
        (SlotId(0), track("other")),
        (SlotId(1), track("one")),
        (SlotId(9), track("unknown")),
    ]);

    assert_eq!(resolved, 1);
    assert_eq!(
        sequence.slot(SlotId(0)).unwrap().track,
        Some(track("track-0"))
    );
    assert_eq!(sequence.slot(SlotId(1)).unwrap().track, Some(track("one")));
    assert_eq!(sequence.slot(SlotId(2)).unwrap().track, None);
}

#[test]
fn a_sequence_cannot_start_without_slots_or_at_an_unknown_slot() {
    let source = || vec![SequenceSource::RistlePlaylist(PlaylistId::new("p1"))];
    assert_eq!(
        Sequence::start(descriptor(source()), Vec::new(), StartAt::Beginning, None),
        Err(InvalidSequence::Empty)
    );
    assert_eq!(
        Sequence::start(
            descriptor(source()),
            slots(3),
            StartAt::Slot(SlotId(7)),
            None
        ),
        Err(InvalidSequence::UnknownStart(SlotId(7)))
    );
    let mut duplicated = slots(3);
    duplicated[2].id = SlotId(0);
    assert_eq!(
        Sequence::start(descriptor(source()), duplicated, StartAt::Beginning, None),
        Err(InvalidSequence::Invalid(CorruptState::DuplicateSlot(
            SlotId(0)
        )))
    );
}

// Property tests (M2 exit): every shuffle is a permutation of the unconsumed
// slots and leaves the consumed prefix alone, and every state restores to an
// equal sequence.
#[test]
fn every_shuffle_permutes_only_the_unconsumed_slots() {
    for seed in 0..200 {
        let mut random = Random::new(seed);
        let count = 1 + random.below(400);
        let start = random.below(count);
        let mut sequence = playlist(count, StartAt::Slot(SlotId(start)), None);
        for _ in 0..random.below(20) {
            sequence.advance();
        }
        for step in 0..10 {
            let before = order(&sequence);
            let consumed = sequence.state().cursor.map_or(0, |cursor| cursor + 1);
            let shuffle = (random.below(3) != 0).then(|| random.below(u64::MAX));
            sequence.set_shuffle(shuffle);

            let after = order(&sequence);
            assert_eq!(
                after[..consumed],
                before[..consumed],
                "seed {seed} step {step}"
            );
            let mut upcoming_before = before[consumed..].to_vec();
            let mut upcoming_after = after[consumed..].to_vec();
            upcoming_before.sort_unstable();
            upcoming_after.sort_unstable();
            assert_eq!(upcoming_after, upcoming_before, "seed {seed} step {step}");

            let restored = Sequence::restore(sequence.state().clone()).unwrap();
            assert_eq!(restored, sequence, "seed {seed} step {step}");
            match random.below(4) {
                0 => {
                    sequence.go_back();
                }
                1 => sequence.restart(),
                _ => {
                    sequence.advance();
                }
            }
        }
    }
}

// Shuffle Play over unresolved positions: the slot that plays first is the
// first one hydration reads, so it is never played without being resolved.
#[test]
fn the_current_slot_is_hydrated_first_after_a_start() {
    let liked = SequenceSource::Collection(CollectionDescriptor {
        reference: CollectionRef {
            connection: "spotify-1".into(),
            id: "liked".into(),
        },
        addressing: Addressing::Offset,
        total: None,
        revision: None,
        endless: false,
    });
    let mut positions = slots(120);
    for slot in &mut positions {
        slot.track = None;
    }
    let shuffled = Sequence::start(
        descriptor(vec![liked.clone()]),
        positions.clone(),
        StartAt::Beginning,
        Some(4),
    )
    .unwrap();
    let plan = shuffled.hydration_plan(10);
    assert_eq!(plan.first().copied(), shuffled.current());
    assert_eq!(plan.len(), 11);

    let in_order = Sequence::start(
        descriptor(vec![liked]),
        positions,
        StartAt::Slot(SlotId(119)),
        None,
    )
    .unwrap();
    assert_eq!(in_order.hydration_plan(10), [SlotId(119)]);
}
