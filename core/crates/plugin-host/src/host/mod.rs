//! The store data of an instance and the imports it calls.
//!
//! Every import measures the time it spends, so the epoch callback can
//! count only guest time against a call's budget, and checks the bounds of
//! `plugin-api/wit/host.wit` on its arguments: a string over 64 KiB or byte
//! payloads over 16 MiB fail the call with `error.internal`, or trap for an
//! import without an error result.

mod crypto;
mod pending;
mod storage;
mod system;

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use ristle_domain::{Clock, PluginId};
use wasmtime::component::ResourceTable;

use crate::bindings::ristle::plugin::types::Error as WitError;
use crate::limits::{self, Limiter};
use crate::package::Manifest;
use crate::ports::PluginStore;

/// The largest string a Plugin may pass to an import.
pub(crate) const MAX_STRING: usize = 64 * 1024;
/// The largest byte payload of one import call.
pub(crate) const MAX_PAYLOAD: usize = 16 * 1024 * 1024;

/// What every instance of every Plugin shares.
pub(crate) struct Services {
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) store: Arc<dyn PluginStore>,
    pub(crate) net: ristle_net::Client,
    pub(crate) locale: Mutex<String>,
}

/// What every instance of one Plugin shares.
pub(crate) struct PluginContext {
    pub(crate) id: PluginId,
    pub(crate) manifest: Manifest,
    pub(crate) services: Arc<Services>,
    pub(crate) log: Mutex<LogBucket>,
}

impl PluginContext {
    pub(crate) fn new(manifest: Manifest, services: Arc<Services>) -> Self {
        Self {
            id: manifest.id.clone(),
            manifest,
            services,
            log: Mutex::new(LogBucket::new()),
        }
    }
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
}

impl HostState {
    pub(crate) fn new(plugin: Arc<PluginContext>) -> Self {
        Self {
            limiter: Limiter::default(),
            table: ResourceTable::new(),
            clock: CallClock::idle(),
            plugin,
        }
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

/// The host side of a `socket.tcp` resource.
#[allow(unreachable_pub)]
#[derive(Debug)]
pub struct TcpHandle {
    _private: (),
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
