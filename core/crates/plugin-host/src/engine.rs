//! The one wasmtime configuration (ADR 0003, verified in spike C).
//!
//! - The small-memory settings on every platform: no up-front reservation,
//!   no guard region, 1 MiB reserved for growth, memories that may move.
//!   iOS needs them, and they bound address space on arm64 Android too.
//! - Cranelift where the OS allows a JIT, Pulley where it does not (iOS).
//! - Epoch interruption, ticked every [`TICK`] by a thread that ends with
//!   the engine.
//! - Compiled artifacts cached as files keyed by package hash and engine
//!   configuration, since loading one takes about a millisecond against
//!   100–300 ms to compile.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};
use wasmtime::component::Component;

use crate::Error;
use crate::package::Package;

/// How often the epoch advances. Guest-time budgets are checked this often.
pub const TICK: Duration = Duration::from_millis(10);

/// How Plugin code runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Backend {
    /// The Cranelift JIT.
    Cranelift,
    /// The Pulley interpreter, for platforms without a JIT.
    Pulley,
}

impl Backend {
    /// Pulley on iOS, Cranelift everywhere else.
    pub fn for_this_platform() -> Self {
        if cfg!(target_os = "ios") {
            Backend::Pulley
        } else {
            Backend::Cranelift
        }
    }
}

/// Engine settings.
#[derive(Clone, Debug)]
pub struct EngineConfig {
    /// How Plugin code runs.
    pub backend: Backend,
    /// Where compiled artifacts are cached; `None` compiles every time.
    pub artifacts: Option<PathBuf>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            backend: Backend::for_this_platform(),
            artifacts: None,
        }
    }
}

/// The wasmtime engine every Plugin runs on.
#[derive(Clone)]
pub struct Engine {
    inner: wasmtime::Engine,
    backend: Backend,
    fingerprint: String,
    artifacts: Option<PathBuf>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("backend", &self.backend)
            .field("fingerprint", &self.fingerprint)
            .finish_non_exhaustive()
    }
}

/// The magic at the start of a cached artifact, then the SHA-256 of the
/// rest.
const ARTIFACT_MAGIC: &[u8; 8] = b"RSTLART1";

impl Engine {
    /// An engine with the given settings, and its epoch thread.
    pub fn new(config: EngineConfig) -> Result<Self, Error> {
        let mut wasm = wasmtime::Config::new();
        wasm.wasm_component_model(true);
        wasm.epoch_interruption(true);
        wasm.memory_reservation(0);
        wasm.memory_guard_size(0);
        wasm.memory_reservation_for_growth(1 << 20);
        wasm.memory_may_move(true);
        if config.backend == Backend::Pulley {
            let target = if cfg!(target_pointer_width = "64") {
                "pulley64"
            } else {
                "pulley32"
            };
            wasm.target(target).map_err(engine_error)?;
            wasm.signals_based_traps(false);
        }
        let inner = wasmtime::Engine::new(&wasm).map_err(engine_error)?;
        let mut hasher = DefaultHasher::new();
        inner.precompile_compatibility_hash().hash(&mut hasher);
        let fingerprint = format!(
            "{}-{:016x}",
            match config.backend {
                Backend::Cranelift => "cranelift",
                Backend::Pulley => "pulley",
            },
            hasher.finish()
        );
        start_ticker(&inner)?;
        Ok(Self {
            inner,
            backend: config.backend,
            fingerprint,
            artifacts: config.artifacts,
        })
    }

    /// How Plugin code runs on this engine.
    pub fn backend(&self) -> Backend {
        self.backend
    }

    /// Identifies the engine configuration in artifact cache keys.
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub(crate) fn wasmtime(&self) -> &wasmtime::Engine {
        &self.inner
    }

    /// The package compiled, from the artifact cache when it holds a valid
    /// artifact for this package and configuration.
    pub(crate) fn compile(&self, package: &Package) -> Result<Component, Error> {
        let Some(directory) = &self.artifacts else {
            return Component::new(&self.inner, package.bytes().as_ref()).map_err(compile_error);
        };
        let path = directory.join(format!(
            "{}-{}.cwasm",
            crate::package::hex_digest(package.sha256()),
            self.fingerprint
        ));
        if let Some(component) = self.load_artifact(&path) {
            return Ok(component);
        }
        let component =
            Component::new(&self.inner, package.bytes().as_ref()).map_err(compile_error)?;
        match component.serialize() {
            Ok(serialized) => {
                if let Err(error) = store_artifact(directory, &path, &serialized) {
                    tracing::warn!(%error, "could not cache a compiled Plugin");
                }
            }
            Err(error) => tracing::warn!(%error, "could not serialize a compiled Plugin"),
        }
        Ok(component)
    }

    /// A cached artifact, if one exists, is intact and fits this engine.
    #[allow(unsafe_code)]
    fn load_artifact(&self, path: &Path) -> Option<Component> {
        let bytes = std::fs::read(path).ok()?;
        let payload = verified_artifact(&bytes)?;
        // SAFETY: `Component::deserialize` runs the bytes as machine code
        // (or Pulley bytecode), so they must be an artifact this engine
        // produced. They are: the file lives in the app's private cache
        // directory, which only Ristle writes, under a name made of the
        // package hash and this engine's configuration; its SHA-256 (checked
        // above) rules out truncation and corruption; and wasmtime itself
        // refuses artifacts from another version or configuration.
        match unsafe { Component::deserialize(&self.inner, payload) } {
            Ok(component) => Some(component),
            Err(error) => {
                tracing::warn!(%error, "discarding a cached Plugin artifact");
                let _ = std::fs::remove_file(path);
                None
            }
        }
    }
}

/// The payload of an artifact file whose magic and hash are right.
fn verified_artifact(bytes: &[u8]) -> Option<&[u8]> {
    let rest = bytes.strip_prefix(ARTIFACT_MAGIC.as_slice())?;
    let (hash, payload) = rest.split_at_checked(32)?;
    (Sha256::digest(payload).as_slice() == hash).then_some(payload)
}

/// Writes an artifact atomically: to a temporary file, then renamed.
fn store_artifact(directory: &Path, path: &Path, serialized: &[u8]) -> std::io::Result<()> {
    std::fs::create_dir_all(directory)?;
    let temporary = path.with_extension(format!("tmp{}", std::process::id()));
    {
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(ARTIFACT_MAGIC)?;
        file.write_all(&Sha256::digest(serialized))?;
        file.write_all(serialized)?;
        file.sync_all()?;
    }
    std::fs::rename(&temporary, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temporary);
    })
}

/// Advances the epoch every [`TICK`] until the engine is dropped.
fn start_ticker(engine: &wasmtime::Engine) -> Result<(), Error> {
    let weak = engine.weak();
    std::thread::Builder::new()
        .name("ristle-plugin-epoch".to_owned())
        .spawn(move || {
            while let Some(engine) = weak.upgrade() {
                engine.increment_epoch();
                drop(engine);
                std::thread::sleep(TICK);
            }
        })
        .map(|_| ())
        .map_err(|error| Error::Engine(format!("epoch thread: {error}")))
}

fn engine_error(error: wasmtime::Error) -> Error {
    Error::Engine(format!("{error:#}"))
}

fn compile_error(error: wasmtime::Error) -> Error {
    Error::Package(crate::package::PackageError::Component(format!(
        "{error:#}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifacts_are_checked_before_use() {
        let payload = b"compiled".to_vec();
        let mut file = ARTIFACT_MAGIC.to_vec();
        file.extend_from_slice(&Sha256::digest(&payload));
        file.extend_from_slice(&payload);
        assert_eq!(verified_artifact(&file), Some(&payload[..]));
        let mut corrupt = file.clone();
        *corrupt.last_mut().unwrap() ^= 1;
        assert_eq!(verified_artifact(&corrupt), None);
        assert_eq!(verified_artifact(&file[..20]), None);
        assert_eq!(verified_artifact(b"RSTLART0"), None);
    }

    #[test]
    fn fingerprints_differ_by_backend() {
        let cranelift = Engine::new(EngineConfig {
            backend: Backend::Cranelift,
            artifacts: None,
        })
        .unwrap();
        let pulley = Engine::new(EngineConfig {
            backend: Backend::Pulley,
            artifacts: None,
        })
        .unwrap();
        assert_ne!(cranelift.fingerprint(), pulley.fingerprint());
        assert!(pulley.fingerprint().starts_with("pulley-"));
    }
}
