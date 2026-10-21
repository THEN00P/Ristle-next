//! The application core: the composition root and every behavior that spans
//! crates.
//!
//! - **Playback session:** applies Queue Mutations to the Canonical Queue and
//!   the active sequence and persists them in one transaction, restores the
//!   Playback Session paused on a cold start, and applies idempotent command
//!   receipts.
//! - **Playback control:** opens the current Track through the Plugin host and
//!   the audio engine. An unplayable Track is reported and skipped; a
//!   transient failure stops on it, paused.
//! - **Sequences:** walks collections through the Plugin host, hydrates the
//!   lookahead in bounded batches, refills in the background, and keeps
//!   positional collections and composites (Liked Songs, search results) on
//!   their routing ids.
//! - **Library:** Ristle Playlists, Liked Songs, Recently Played, Unified
//!   Search, and Local Track grouping.
//! - **Observers:** every state change is published through the core's
//!   runtime, never inline on the caller's thread (ADR 0001).
//! - **Ports:** implements the storage, cache and secret ports of the other
//!   crates over `ristle-storage` and the platform.
//!
//! Milestone M7 (`docs/plan/phase-2.md`). Ported legacy tests:
//! `application_core_test.cpp`, `unified_search_test.cpp` and the non-Party
//! cases of `app-core/tests`.
//!
//! Until a behavior's milestone lands, its [`Core`] method has its final
//! signature and returns [`Error::NotReady`].

mod core;
mod error;
pub mod model;
mod observe;
mod platform;

pub use crate::core::{CORE_THREAD_NAME, Core};
pub use crate::error::{Error, ProviderErrorKind};
pub use crate::model::items::*;
pub use crate::model::pages::*;
pub use crate::model::playlists::*;
pub use crate::model::plugins::*;
pub use crate::model::search::*;
pub use crate::model::session::*;
pub use crate::model::sign_in::*;
pub use crate::observe::{Observer, Subscription};
pub use crate::platform::{Platform, PlatformError};

#[cfg(test)]
mod tests;
