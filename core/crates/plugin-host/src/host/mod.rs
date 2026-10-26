//! The store data of an instance and the imports it calls.
//!
//! Every import measures the time it spends, so the epoch callback can
//! count only guest time against a call's budget, and checks the bounds of
//! `plugin-api/wit/host.wit` on its arguments: a string over 64 KiB or byte
//! payloads over 16 MiB fail the call with `error.internal`, or trap for an
//! import without an error result.

mod connection;
mod crypto;
mod files;
mod http;
mod pending;
mod socket;
mod storage;
mod system;

// Named by the generated bindings, which re-export it.
#[allow(unreachable_pub)]
pub use socket::TcpHandle;

use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use ristle_domain::{Clock, ConnectionId, PluginId};
use wasmtime::component::ResourceTable;

use crate::bindings::ristle::plugin::types::Error as WitError;
use crate::connections::Connections;
use crate::limits::{self, Limiter};
use crate::package::Manifest;
use crate::ports::{FolderAccess, PluginStore, SecretVault};

/// The largest string a Plugin may pass to an import.
pub(crate) const MAX_STRING: usize = 64 * 1024;
/// The largest byte payload of one import call.
pub(crate) const MAX_PAYLOAD: usize = 16 * 1024 * 1024;

/// What every instance of every Plugin shares.
pub(crate) struct Services {
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) store: Arc<dyn PluginStore>,
    pub(crate) vault: Arc<dyn SecretVault>,
    pub(crate) net: ristle_net::Client,
    pub(crate) locale: Mutex<String>,
    /// The core runtime: imports block on it from the call's thread.
    pub(crate) runtime: tokio::runtime::Handle,
    /// The platform's access to granted folders, where there is one.
    pub(crate) folders: Option<Arc<dyn FolderAccess>>,
    /// Socket destinations tests confirm as if granted.
    #[cfg(feature = "test-support")]
    pub(crate) socket_origins: Vec<ristle_net::GrantedOrigin>,
}

impl Services {
    /// What a socket to `target` may reach beyond public addresses:
    /// nothing, except in tests.
    pub(crate) fn socket_origin(
        &self,
        target: &crate::package::SocketTarget,
    ) -> Option<ristle_net::GrantedOrigin> {
        #[cfg(feature = "test-support")]
        if let Some(origin) = self
            .socket_origins
            .iter()
            .find(|origin| origin.host == target.host && origin.port == target.port)
        {
            return Some(origin.clone());
        }
        let _ = target;
        None
    }
}

/// What every instance of one Plugin shares.
pub(crate) struct PluginContext {
    pub(crate) id: PluginId,
    pub(crate) manifest: Manifest,
    pub(crate) services: Arc<Services>,
    pub(crate) log: Mutex<LogBucket>,
    pub(crate) connections: Connections,
    /// Open sockets across the Plugin's instances.
    pub(crate) open_sockets: AtomicUsize,
}

impl PluginContext {
    pub(crate) fn new(manifest: Manifest, services: Arc<Services>) -> Self {
        Self {
            id: manifest.id.clone(),
            connections: Connections::new(manifest.id.clone()),
            manifest,
            services,
            log: Mutex::new(LogBucket::new()),
            open_sockets: AtomicUsize::new(0),
        }
    }

    pub(crate) fn retirements(&self) -> u64 {
        self.connections.retirements()
    }
}

/// The call an instance is running: which connection, under which
/// generation, and what it read.
#[derive(Clone, Debug)]
pub(crate) struct CallContext {
    pub(crate) connection: ConnectionId,
    pub(crate) generation: u64,
    pub(crate) retirements: u64,
    /// The credential version this call last read, or the one it started
    /// with, for `set-credential`'s compare-and-set.
    pub(crate) credential_base: u64,
    /// The user asked to refresh: cached responses are revalidated.
    pub(crate) refresh: bool,
}

/// A token bucket for `system.log`.
pub(crate) struct LogBucket {
    tokens: f64,
    refilled: Instant,
    /// Messages dropped since the last one that got through.
    pub(crate) dropped: u64,
}

impl LogBucket {
    fn new() -> Self {
        Self {
            tokens: f64::from(limits::LOG_BURST),
            refilled: Instant::now(),
            dropped: 0,
        }
    }

    /// Takes a token, if one is left.
    pub(crate) fn take(&mut self) -> bool {
        let now = Instant::now();
        let earned =
            now.duration_since(self.refilled).as_secs_f64() * f64::from(limits::LOG_PER_SECOND);
        self.tokens = (self.tokens + earned).min(f64::from(limits::LOG_BURST));
        self.refilled = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            self.dropped += 1;
            false
        }
    }
}

/// Which budget a call ran out of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Exceeded {
    /// Guest time.
    GuestTime,
    /// Total time, imports included.
    CallTime,
}

/// Time accounting for the call an instance is running.
pub(crate) struct CallClock {
    started: Instant,
    in_imports: Duration,
    guest_budget: Duration,
    total_budget: Duration,
    pub(crate) exceeded: Option<Exceeded>,
}

impl CallClock {
    pub(crate) fn idle() -> Self {
        Self::start(Duration::MAX, Duration::MAX)
    }

    pub(crate) fn start(guest_budget: Duration, total_budget: Duration) -> Self {
        Self {
            started: Instant::now(),
            in_imports: Duration::ZERO,
            guest_budget,
            total_budget,
            exceeded: None,
        }
    }

    /// How long the call may still take, for timeouts inside imports.
    pub(crate) fn remaining(&self) -> Duration {
        self.total_budget.saturating_sub(self.started.elapsed())
    }

    /// Called on every epoch tick while guest code runs: which budget is
    /// spent, if any.
    pub(crate) fn check(&mut self) -> Option<Exceeded> {
        let elapsed = self.started.elapsed();
        let exceeded = if elapsed >= self.total_budget {
            Some(Exceeded::CallTime)
        } else if elapsed.saturating_sub(self.in_imports) >= self.guest_budget {
            Some(Exceeded::GuestTime)
        } else {
            None
        };
        self.exceeded = self.exceeded.or(exceeded);
        exceeded
    }
}

/// The data of one instance's store.
pub(crate) struct HostState {
    pub(crate) limiter: Limiter,
    pub(crate) table: ResourceTable,
    pub(crate) clock: CallClock,
    pub(crate) plugin: Arc<PluginContext>,
    pub(crate) call: Option<CallContext>,
}

impl HostState {
    pub(crate) fn new(plugin: Arc<PluginContext>) -> Self {
        Self {
            limiter: Limiter::default(),
            table: ResourceTable::new(),
            clock: CallClock::idle(),
            plugin,
            call: None,
        }
    }

    /// The retirement count of a running call that is not retired, for
    /// commits of the Plugin's own state.
    pub(crate) fn live_retirements(&self) -> Option<u64> {
        if self.retired() {
            return None;
        }
        self.call.as_ref().map(|call| call.retirements)
    }

    /// Whether the running call is retired: its connection's generation
    /// moved or ended, or the Plugin was replaced or disabled since it
    /// started. A retired call reaches nothing outside its instance.
    pub(crate) fn retired(&self) -> bool {
        let Some(call) = &self.call else {
            return true;
        };
        self.plugin.retirements() != call.retirements
            || self.plugin.connections.generation(&call.connection) != Some(call.generation)
    }

    /// Runs an import's body, counting its time as import time.
    pub(crate) fn timed<R>(&mut self, body: impl FnOnce(&mut Self) -> R) -> R {
        let started = Instant::now();
        let result = body(self);
        self.clock.in_imports += started.elapsed();
        result
    }

    pub(crate) fn services(&self) -> &Services {
        &self.plugin.services
    }
}

/// The host side of a `connection` resource: which Provider Connection a
/// call runs for.
// `pub` because the generated bindings re-export the types they map
// resources to; the module itself is private.
#[allow(unreachable_pub)]
#[derive(Debug)]
pub struct ConnectionHandle {
    pub(crate) connection: ristle_domain::ConnectionId,
}

/// An argument over a bound, for imports with an error result.
pub(crate) fn too_large(what: &str) -> WitError {
    WitError::Internal(format!("{what} exceeds the host's bound"))
}

/// Checks strings an import received.
pub(crate) fn strings_fit(strings: &[&str]) -> bool {
    strings.iter().all(|text| text.len() <= MAX_STRING)
}

/// A trap for an import without an error result.
pub(crate) fn trap(message: &str) -> wasmtime::Error {
    wasmtime::Error::msg(message.to_owned())
}

/// Locks a mutex, ignoring poisoning: the data stays consistent because no
/// lock is held across a fallible step.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
