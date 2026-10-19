//! The Canonical Queue and playable sequences.
//!
//! Two modules, both pure and deterministic (every random choice takes a
//! seed):
//!
//! - `queue`: the Canonical Queue. Current entry, collection with its cursor
//!   and history, Queued Requests (FIFO, with Play Next ahead of them), repeat
//!   modes, shuffle of the upcoming collection, removal with undo, pruning,
//!   and a serializable snapshot that restores exactly.
//! - `sequence`: a collection the host walks without fetching it whole. Its
//!   order is a list of slots, each either resolved to a Track or still a
//!   position (offset-addressed collections such as Spotify Liked Songs). It
//!   owns the shuffle permutation, the bounded window of hydrated history and
//!   lookahead, the hydration plan, and re-basing positions after drift
//!   without reshuffling (`docs/plugin-api/walkthrough.md`, "Spotify Liked
//!   Songs: positional handles").
//!
//! Depends on `ristle-domain` only. Persistence is `ristle-storage`'s job;
//! this crate only exposes state that round-trips.
//!
//! Milestone M2 (`docs/plan/phase-2.md`). Ported legacy tests:
//! `canonical_queue_test.cpp`, `playable_sequence_test.cpp`.
