//! The sequence shuffle: an exact permutation built so that hydration reads
//! stay page-local.
//!
//! Offset-addressed Providers serve 50-entry pages. A large shuffle should
//! visibly span the source without turning a 50-Track rolling window into
//! fifty upstream reads. So the slots are grouped by source page, the pages
//! are put in random order, and they are taken [`SLICES`] at a time: each
//! cohort of pages is emitted as rounds of [`PAGE`] entries that draw an even
//! slice from every page still holding entries, and every round is
//! interleaved so it does not read as one run per page. Each rolling window
//! then covers that many independently chosen regions of the source, and
//! the pages it read are used up before the next cohort starts.
//!
//! Pages are keyed by source and offset, never by index into the order, so a
//! kept current slot does not shift every page boundary (legacy trap 2).
//!
//! [`SLICES`] is the mixing dial, measured in Ristle legacy over 600 played
//! Tracks: at 300 Spotify and 1,000 SoundCloud Liked Songs, 8 slices cut the
//! mean same-Provider run from 10.5 to 3.3 Tracks for one extra request.

use std::collections::{BTreeMap, HashMap};

use super::{Slot, SlotId};
use crate::random::Random;

/// Entries per Provider page.
pub(crate) const PAGE: u64 = 50;
/// Pages per cohort.
pub(crate) const SLICES: usize = 8;

/// Shuffles `ids` in place. `slots` holds every id's slot.
pub(crate) fn shuffle(ids: &mut [SlotId], slots: &HashMap<SlotId, &Slot>, random: &mut Random) {
    let mut pages: BTreeMap<(usize, u64), Vec<SlotId>> = BTreeMap::new();
    for id in ids.iter() {
        let key = slots
            .get(id)
            .map_or((0, 0), |slot| (slot.source, slot.offset / PAGE));
        pages.entry(key).or_default().push(*id);
    }
    let mut blocks: Vec<Vec<SlotId>> = pages.into_values().collect();
    for block in &mut blocks {
        random.shuffle(block);
    }
    random.shuffle(&mut blocks);

    let mut out = 0;
    let page = usize::try_from(PAGE).unwrap_or(usize::MAX);
    for cohort in blocks.chunks(SLICES) {
        let mut consumed = vec![0; cohort.len()];
        let mut remaining: usize = cohort.iter().map(Vec::len).sum();
        while remaining > 0 {
            let mut lanes: Vec<usize> = (0..cohort.len())
                .filter(|&lane| consumed[lane] < cohort[lane].len())
                .collect();
            random.shuffle(&mut lanes);

            let mut round_left = page.min(remaining);
            let mut round = Vec::with_capacity(round_left);
            for (taken, &lane) in lanes.iter().enumerate() {
                if round_left == 0 {
                    break;
                }
                let lanes_left = lanes.len() - taken;
                let fair = round_left.div_ceil(lanes_left);
                let take = (cohort[lane].len() - consumed[lane]).min(fair);
                round.extend_from_slice(&cohort[lane][consumed[lane]..consumed[lane] + take]);
                consumed[lane] += take;
                round_left -= take;
                remaining -= take;
            }
            // Each slice is one page's run; interleave the round. It holds
            // the same entries from the same pages, so reads do not change.
            random.shuffle(&mut round);
            ids[out..out + round.len()].copy_from_slice(&round);
            out += round.len();
        }
    }
}
