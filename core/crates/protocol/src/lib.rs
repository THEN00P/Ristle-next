//! Client-server protocol types (ADR 0001, ADR 0007).
//!
//! The one crate shared by the app core and `ristle-server`: Party Commands
//! with their per-member sequence numbers, Party State with its revision,
//! acknowledgements and rejections, transport-only updates, File Share
//! signaling, and the ephemeral search request and reply. Serialization only;
//! no behavior.
//!
//! Licensed GPL-3.0-or-later like the rest of `core/`. The AGPL-3.0 server
//! may link it (GPL-3.0 section 13).
//!
//! Milestone M8 (`docs/plan/phase-2.md`).
