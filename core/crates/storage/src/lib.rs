//! SQLite persistence through `rusqlite`.
//!
//! Two databases: `ristle.sqlite` for state the user cannot recreate
//! (Playback Session, Ristle Playlists, Recently Played, settings, Plugin
//! Permissions and grants, Provider Connections, Local Track index, sequence
//! state, command receipts) and `cache.sqlite` for reproducible data (the
//! HTTP cache, Provider page caches, compiled Plugin artifacts index). Clearing
//! the cache never touches the first.
//!
//! Migrations are forward-only and run in one transaction. A failed
//! migration leaves the prior database untouched, and a database written by a
//! newer schema is opened in recovery mode without modification.
//!
//! The crate exposes concrete repositories. Other crates do not depend on it:
//! they declare the storage ports they need, and `ristle-app` implements
//! those ports over these repositories.
//!
//! Milestone M3 (`docs/plan/phase-2.md`). Ported legacy tests:
//! `storage_test.cpp`.
