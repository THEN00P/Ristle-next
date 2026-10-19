//! Domain types shared by every core crate.
//!
//! This crate holds the vocabulary of `CONTEXT.md` as plain data: Track
//! identity (`TrackRef`: Provider, Provider-local id and, for Server Grants
//! only, the Provider Connection), `Track` with its playback capabilities and
//! availability, Media Date with its original precision, artwork, Ristle
//! Playlists, collection descriptors (offset or cursor addressing, revision,
//! total) and the injected `Clock`.
//!
//! It has no I/O, no async and no dependency on another core crate. Anything
//! that needs a database, the network or a runtime belongs elsewhere.
//!
//! Milestone M1 (`docs/plan/phase-2.md`) fills this crate. Ported legacy
//! tests: `track_test.cpp`.
