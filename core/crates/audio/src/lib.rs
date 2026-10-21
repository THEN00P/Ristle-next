//! The audio engine and output.
//!
//! The engine pulls PCM from `MediaStream`s on its decode thread, maps
//! channels and resamples to the output rate (rubato), and keeps two slots,
//! each with its own lock-free ring: the current Track and the prepared next
//! one. The render callback moves from one to the other at the exact frame
//! where the current Track ends (Gapless Transition), so the next Track is
//! decoded ahead and Next can promote it without reopening it. The engine
//! owns the transport: play, pause, seek with a sequence number that makes
//! stale positions harmless, one core-owned gain, a terminal ended state, and
//! stream failures kept for diagnostics. It never overlaps Tracks; a later
//! crossfade must honour `overlap = false` (ADR 0012).
//!
//! Output goes through the [`AudioOutput`] port: cpal on desktop, AAudio
//! with PowerSaving and the music content type on Android, and a capturing
//! output for tests (`test_support`, behind the `test-support` feature).
//!
//! Milestone M6b (`docs/plan/phase-2.md`). Ported legacy tests:
//! `audio-engine/tests/audio_engine_test.cpp`, in `tests/engine.rs`.

mod convert;
mod engine;
mod feed;
pub mod output;
mod render;
mod ring;
mod shared;
pub mod stream;
#[cfg(feature = "test-support")]
pub mod test_support;

pub use engine::{
    Engine, EngineConfig, EngineEvent, EngineObserver, EngineState, Error, TrackToken,
};
pub use output::{AudioOutput, OutputError, OutputErrors, OutputInfo, OutputRequest, Render};
pub use stream::{MediaError, MediaStream, Read, StreamFormat};
