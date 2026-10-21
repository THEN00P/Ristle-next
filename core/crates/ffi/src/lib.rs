//! The UniFFI facade for Kotlin and Swift (ADR 0001).
//!
//! A thin layer over `ristle-app`: one `RistleCore` object, records and enums
//! for what UIs render, a `CoreObserver` callback interface, typed
//! `CoreError`s, and a `CancelToken` on every long-running async call. It
//! holds no behavior. Conventions from spike D apply: no panic crosses the
//! boundary, every fallible function returns `Result`, callbacks arrive on
//! core threads, and bindings are generated in library mode from an
//! unstripped build.
//!
//! The API shape is fixed in milestone M1 (`docs/plan/phase-2.md`) so phase 3
//! can start against it; its implementation fills in as M7 lands.
//!
//! The data types are `ristle-app`'s (see [`types`]); this crate adds the
//! objects (`RistleCore`, `CancelToken`, `Subscription`), the platform's
//! callback interfaces and the two error types.

mod cancel;
mod core;
mod error;
mod platform;
pub mod types;

pub use crate::cancel::CancelToken;
pub use crate::core::{RistleCore, Subscription};
pub use crate::error::{CoreError, PlatformError};
pub use crate::platform::{CoreObserver, PlatformHost};

uniffi::setup_scaffolding!();
