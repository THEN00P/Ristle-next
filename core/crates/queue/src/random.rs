//! Seeded randomness.
//!
//! Seeds are persisted with the orders they produced, so the same seed must
//! give the same order in every later version of Ristle. That is why this
//! module draws from ChaCha8, whose output stream is specified, and does its
//! own range reduction and Fisher-Yates instead of relying on the `rand`
//! crate's algorithms, which may change between releases. Changing anything
//! here changes persisted orders.

use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{RngCore, SeedableRng};

/// A deterministic random source for one shuffle.
pub(crate) struct Random(ChaCha8Rng);

impl Random {
    pub(crate) fn new(seed: u64) -> Self {
        Self(ChaCha8Rng::seed_from_u64(seed))
    }

    /// A uniform integer in `0..bound`. `bound` must not be zero.
    pub(crate) fn below(&mut self, bound: u64) -> u64 {
        debug_assert!(bound > 0);
        // Rejection sampling: accept only draws below the largest multiple of
        // `bound`, so every residue is equally likely.
        let zone = u64::MAX - (u64::MAX % bound + 1) % bound;
        loop {
            let draw = self.0.next_u64();
            if draw <= zone {
                return draw % bound;
            }
        }
    }

    /// Shuffles `items` in place (Fisher-Yates, from the back).
    pub(crate) fn shuffle<T>(&mut self, items: &mut [T]) {
        for last in (1..items.len()).rev() {
            // `last` fits in u64 and the result is at most `last`.
            let pick = self.below(last as u64 + 1) as usize;
            items.swap(last, pick);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_order() {
        let order = |seed| {
            let mut items: Vec<u32> = (0..20).collect();
            Random::new(seed).shuffle(&mut items);
            items
        };
        assert_eq!(order(42), order(42));
        assert_ne!(order(42), order(43));
        let mut sorted = order(42);
        sorted.sort_unstable();
        assert_eq!(sorted, (0..20).collect::<Vec<_>>());
    }

    // Pins the algorithm: if this fails, persisted shuffles would replay in
    // a different order after an upgrade.
    #[test]
    fn the_order_for_a_seed_never_changes() {
        let mut items: Vec<u32> = (0..10).collect();
        Random::new(7).shuffle(&mut items);
        assert_eq!(items, PINNED_SEED_7);
    }

    const PINNED_SEED_7: [u32; 10] = [2, 7, 0, 6, 3, 4, 8, 1, 9, 5];

    #[test]
    fn below_stays_in_range() {
        let mut random = Random::new(1);
        for bound in [1, 2, 3, 7, 1000, u64::MAX] {
            for _ in 0..100 {
                assert!(random.below(bound) < bound);
            }
        }
    }
}
