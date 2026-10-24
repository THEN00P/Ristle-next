//! The WASM Plugin host (ADR 0003).
//!
//! - **Packages:** parse the manifest custom section, verify the `wasmsign2`
//!   signature, and decide install, update or takeover by Publisher
//!   (`docs/plugin-api/packaging.md`).
//! - **Engine:** one wasmtime configuration on every platform: the small
//!   memory settings (`memory_reservation(0)`, no guard region), Cranelift
//!   where the OS allows a JIT and Pulley where it does not, epoch
//!   interruption, and a compiled-artifact cache keyed by package hash and
//!   engine configuration.
//! - **Instances:** one per Provider Connection, recycled after a trap or when
//!   linear memory has grown. Plugin state lives in host storage only.
//! - **Imports:** `http`, `connection`, `storage`, `crypto`, `system`,
//!   `files`, `socket` and `live` from `plugin-api/wit/host.wit`, each checked
//!   against the manifest and the user's consent at call time.
//! - **Exports:** typed async wrappers for `catalog` and every optional
//!   interface. WIT types are converted to `ristle-domain` types here, so no
//!   other crate sees generated bindings.
//!
//! Persistence and secrets are ports this crate declares and `ristle-app`
//! implements. Milestone M5 (`docs/plan/phase-2.md`).

mod engine;
mod exports;
mod inspect;
pub mod limits;
pub mod package;
mod ports;
mod runtime;
#[cfg(feature = "test-support")]
pub mod test_support;

pub use engine::{Backend, Engine, EngineConfig, TICK};
pub use exports::{MAX_RESULT_BYTES, MAX_RESULT_STRING, PluginError};
pub use inspect::{API, API_VERSION};
pub use ports::{KvScope, PluginStore, StoreError, Stored};
pub use runtime::{CallError, HostPorts, HostSettings, LoadedPlugin, PluginHost};

/// Every error this crate returns.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A package was refused.
    #[error(transparent)]
    Package(#[from] package::PackageError),
    /// The engine could not be set up.
    #[error("plugin engine: {0}")]
    Engine(String),
    /// A call failed before the Plugin could answer.
    #[error(transparent)]
    Call(#[from] CallError),
}

mod bindings;
mod host;
