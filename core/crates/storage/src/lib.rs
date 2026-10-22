//! SQLite persistence through `rusqlite`.
//!
//! Two databases: `ristle.sqlite` for state the user cannot recreate
//! (Playback Session, sequences, Ristle Playlists, Recently Played, settings,
//! Plugins with their Plugin Permissions and grants, Provider Connections,
//! Plugin key-value data, response sequences, command receipts, the Local
//! Track index) and `cache.sqlite` for what can be fetched again (HTTP
//! responses, Provider pages). Clearing the cache never touches the first.
//! The cache is attached to the same connections as schema `cache`, so one
//! [`Tx`] reaches both.
//!
//! [`Storage::open`] migrates `ristle.sqlite` forward in one transaction. A
//! failed migration leaves the database as it was, and a database written
//! by a newer schema is not modified at all; both open in recovery mode
//! ([`OpenStatus::Recovery`]). See "Migrations" below.
//!
//! All work runs on the storage threads: [`Storage::write`] and
//! [`Storage::read`] send a closure that receives a [`Tx`], and the
//! repositories hang off it (`tx.session()`, `tx.sequences()`, ...). Saves of
//! keyed rows (queue entries, sequence slots and routes, playlist entries)
//! rewrite only the rows that changed and report it as [`Changes`].
//!
//! # Encoding
//!
//! Tracks are stored as `ristle_domain::Track` JSON beside their identity
//! columns, and a few small values (a sequence's `Opened`, an account, grant
//! details) are JSON too. So the serde shape of those types is part of
//! schema v1: the fixture test reads a committed v1 database and fails if a
//! change in `ristle-domain` or `ristle-queue` stops it from decoding.
//!
//! # Migrations
//!
//! Ristle starts at schema v1 and imports nothing from Ristle legacy
//! (`docs/plan/phase-2.md`, open question 3). A later schema version adds a
//! migration and a test that applies it to the committed v1 fixture
//! (`tests/fixtures/schema-v1.sql`).
//!
//! The crate exposes concrete repositories. Other crates do not depend on it:
//! they declare the storage ports they need, and `ristle-app` implements
//! those ports over these repositories.
//!
//! Milestone M3 (`docs/plan/phase-2.md`). Ported legacy tests:
//! `storage_test.cpp` and two `application_core_test.cpp` cases.

mod error;
#[cfg(test)]
mod fixture;
mod migrate;
pub mod repo;
mod sql;
mod storage;
mod tx;

pub use error::Error;
pub use migrate::SCHEMA_VERSION;
pub use sql::Changes;
pub use storage::{OpenStatus, Storage, StoragePaths};
pub use tx::Tx;
