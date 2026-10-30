//! What the core takes from its environment besides the platform: time and
//! randomness. Both are injected so that tests are deterministic
//! (`docs/plan/phase-2.md`, "Time and randomness").

use std::sync::Arc;

use ristle_domain::{Clock, SystemClock};

/// Produces the seed of every random order. The seed is persisted with the
/// order it produced, so restoring never needs the source again.
pub trait SeedSource: Send + Sync + 'static {
    /// A new seed.
    fn next_seed(&self) -> u64;
}

/// Seeds from the operating system.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemSeeds;

impl SeedSource for SystemSeeds {
    fn next_seed(&self) -> u64 {
        // A failing OS generator still leaves a usable, if predictable,
        // shuffle: the seed only orders music.
        getrandom::u64().unwrap_or_else(|error| {
            tracing::warn!(%error, "no OS randomness; shuffling with a time-based seed");
            SystemClock.now_ms().rotate_left(17) ^ 0x9E37_79B9_7F4A_7C15
        })
    }
}

/// The core's clock and seed source.
#[derive(Clone)]
pub struct Services {
    /// Wall-clock time.
    pub clock: Arc<dyn Clock>,
    /// Shuffle seeds.
    pub seeds: Arc<dyn SeedSource>,
}

impl Default for Services {
    fn default() -> Self {
        Self {
            clock: Arc::new(SystemClock),
            seeds: Arc::new(SystemSeeds),
        }
    }
}

impl std::fmt::Debug for Services {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Services").finish_non_exhaustive()
    }
}
