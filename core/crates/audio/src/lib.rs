//! The audio engine and output.
//!
//! The engine pulls PCM from `ristle-media` `MediaStream`s, resamples to the
//! device rate (rubato), and writes one lock-free ring across Track
//! boundaries, so a prepared next stream joins at the exact frame (Gapless
//! Transition). It owns the transport: play, pause, seek with a sequence
//! number that makes stale positions harmless, one core-owned gain, a
//! terminal ended state, and failure reporting for diagnostics. It never
//! overlaps Tracks; a later crossfade must honour `overlap = false`
//! (ADR 0012).
//!
//! Output goes through the `AudioOutput` port: cpal on desktop, AAudio with
//! PowerSaving and the music content type on Android (through `ndk`, after the
//! `ndk-context` bootstrap), and a capturing output for tests.
//!
//! Milestone M6 (`docs/plan/phase-2.md`). Ported legacy tests:
//! `audio-engine/tests/audio_engine_test.cpp`.
