//! Ristle Party logic on the Party Host and on guests (ADR 0007, ADR 0012).
//!
//! - **Host:** applies each member's Party Commands at most once
//!   (`applied_through` persisted in the same transaction as the queue
//!   mutation), turns answers for the guest into rejections in Party State,
//!   and publishes a bounded queue window with only the fields a Plugin's
//!   `[party] share` allows.
//! - **Guest:** keeps unacknowledged commands and resends those above its
//!   acknowledgement after a reconnect.
//! - **File Share policy:** expiry, departure and removal rules for guest
//!   files.
//!
//! The Relay connection is a port. Phase 2 tests run against an in-memory
//! Relay; the network client and ICE arrive in phase 5.
//!
//! Milestone M8 (`docs/plan/phase-2.md`). Ported legacy tests: the Party
//! cases of `app-core/tests`.
