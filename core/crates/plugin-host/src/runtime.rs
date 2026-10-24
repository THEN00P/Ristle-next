//! Loading Plugins and running calls (ADR 0003).
//!
//! - One pool of instances per Provider Connection; an instance runs one
//!   call at a time and is kept a few minutes after its last call.
//! - A call runs synchronously on a blocking thread, so the same code runs
//!   under Cranelift and Pulley without fibers; imports that wait on the
//!   network block that thread, never the runtime's workers.
//! - Budgets: guest time (2 s, 250 ms for `byte-source.read`, ten times that
//!   under Pulley) counted on every epoch tick without the time spent in
//!   imports, and 60 s in total. A call over budget traps.
//! - After a trap, or once linear memory has grown by more than
//!   [`limits::RECYCLE_GROWTH_BYTES`], the instance is discarded; the next
//!   call gets a fresh one. Plugin state therefore lives in host storage
//!   only.
//! - At most [`limits::PHONE_CONCURRENT_CALLS`] calls run at once across all
//!   Plugins (more on desktop).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use ristle_domain::{Clock, ConnectionId, PluginId};
use wasmtime::component::{InstancePre, Linker, Resource};
use wasmtime::{Store, UpdateDeadline};

use crate::Error;
use crate::bindings::FullProvider;
use crate::bindings::exports::ristle::plugin as guest;
use crate::connections::{ConnectionInfo, Registered};
use crate::engine::{Backend, Engine};
use crate::host::{
    CallContext, ConnectionHandle, Exceeded, HostState, PluginContext, Services, lock,
};
use crate::limits;
use crate::package::{Exports, Package, PackageError};
use crate::ports::{PluginStore, SecretVault};

/// What the Plugin host needs from the rest of the core.
pub struct HostPorts {
    /// Wall-clock time for Plugins and storage expiry.
    pub clock: Arc<dyn Clock>,
    /// Plugin state.
    pub store: Arc<dyn PluginStore>,
    /// The platform credential store, for Credential Records.
    pub vault: Arc<dyn SecretVault>,
    /// The core's one HTTP client.
    pub net: ristle_net::Client,
}

/// Settings of a [`PluginHost`].
#[derive(Clone, Debug)]
pub struct HostSettings {
    /// Plugin calls running at once across all Plugins.
    pub concurrent_calls: usize,
    /// The user's locale as a BCP 47 tag.
    pub locale: String,
    /// How long an idle instance is kept.
    pub idle_instance: Duration,
    /// How often idle instances are released.
    pub sweep_interval: Duration,
}

impl Default for HostSettings {
    fn default() -> Self {
        Self {
            concurrent_calls: limits::PHONE_CONCURRENT_CALLS,
            locale: "en-US".to_owned(),
            idle_instance: limits::IDLE_INSTANCE,
            sweep_interval: limits::SWEEP_INTERVAL,
        }
    }
}

/// Why a Plugin call failed before the Plugin could answer.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CallError {
    /// The Plugin trapped; its instance was discarded.
    #[error("the plugin trapped: {0}")]
    Trapped(String),
    /// The call spent its guest-time budget; its instance was discarded.
    #[error("the plugin ran too long")]
    GuestTime,
    /// The call spent its total time budget; its instance was discarded.
    #[error("the plugin call took too long")]
    CallTime,
    /// No instance could be made.
    #[error("the plugin could not start: {0}")]
    Instantiate(String),
    /// The Plugin does not export the interface the call needs.
    #[error("the plugin does not export {0}")]
    NotExported(&'static str),
    /// The connection is not registered with the host.
    #[error("unknown provider connection")]
    UnknownConnection,
    /// The call was retired while it ran: its connection's generation
    /// moved or ended, or the Plugin was replaced or disabled. Its result
    /// is discarded.
    #[error("the call was retired")]
    Retired,
    /// The call's thread ended abnormally.
    #[error("the plugin call was lost: {0}")]
    Lost(String),
}

/// Which budget a call gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CallKind {
    /// Every export but `byte-source.read`.
    Normal,
    /// `byte-source.read`, which playback waits on.
    ByteSource,
}

/// Runs Plugins: one engine, one linker with every import, the shared
/// services and the call limit.
pub struct PluginHost {
    engine: Engine,
    linker: Arc<Linker<HostState>>,
    services: Arc<Services>,
    calls: Arc<tokio::sync::Semaphore>,
    idle_instance: Duration,
    /// Every loaded Plugin, for the sweeper.
    loaded: Arc<Mutex<Vec<Weak<PluginInner>>>>,
}

impl std::fmt::Debug for PluginHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginHost")
            .field("engine", &self.engine)
            .finish_non_exhaustive()
    }
}

impl PluginHost {
    /// A host on `engine`. Must be created inside the core's tokio runtime,
    /// which runs the sweeper that releases idle instances.
    pub fn new(engine: Engine, ports: HostPorts, settings: HostSettings) -> Result<Self, Error> {
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| Error::Engine("the plugin host needs a tokio runtime".to_owned()))?;
        let mut linker = Linker::new(engine.wasmtime());
        FullProvider::add_to_linker::<HostState, wasmtime::component::HasSelf<HostState>>(
            &mut linker,
            |state| state,
        )
        .map_err(|error| Error::Engine(format!("{error:#}")))?;
        Ok(Self {
            engine,
            linker: Arc::new(linker),
            services: Arc::new(Services {
                clock: ports.clock,
                store: ports.store,
                vault: ports.vault,
                net: ports.net,
                locale: Mutex::new(settings.locale),
                runtime: runtime.clone(),
            }),
            calls: Arc::new(tokio::sync::Semaphore::new(
                settings.concurrent_calls.max(1),
            )),
            idle_instance: settings.idle_instance,
            loaded: start_sweeper(&runtime, settings.sweep_interval),
        })
    }

    /// The engine.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Sets the locale Plugins see from their next call on.
    pub fn set_locale(&self, locale: &str) {
        *lock(&self.services.locale) = locale.to_owned();
    }

    /// Compiles a verified package and checks it can run here (install
    /// step 5): its imports and exports against the `ristle:plugin`
    /// version this host implements, the `limits` its exports need, and a
    /// trial instantiation within the instance limits.
    pub fn load(&self, package: &Package) -> Result<LoadedPlugin, Error> {
        let component = self.engine.compile(package)?;
        let manifest = package.manifest().clone();
        let exports = crate::inspect::exports(&component, self.engine.wasmtime(), &manifest)?;
        let pre = self
            .linker
            .instantiate_pre(&component)
            .map_err(|error| PackageError::Component(format!("{error:#}")))?;
        let indices = Indices::new(&pre, &exports)
            .map_err(|error| PackageError::Component(format!("{error:#}")))?;
        let context = Arc::new(PluginContext::new(manifest, self.services.clone()));
        let inner = Arc::new(PluginInner {
            context,
            pre,
            indices,
            exports,
            backend: self.engine.backend(),
            engine: self.engine.wasmtime().clone(),
            pools: Mutex::new(HashMap::new()),
            calls: self.calls.clone(),
            idle_instance: self.idle_instance,
        });
        // A component that cannot be instantiated within the limits, such
        // as one declaring more initial memory, is refused now.
        let trial = inner
            .instantiate()
            .map_err(|error| PackageError::Component(format!("cannot start: {error}")))?;
        drop(trial);
        let mut loaded = lock(&self.loaded);
        loaded.retain(|plugin| plugin.strong_count() > 0);
        loaded.push(Arc::downgrade(&inner));
        Ok(LoadedPlugin { inner })
    }
}

/// Releases idle instances past their time every `interval`, until the
/// host is dropped.
fn start_sweeper(
    runtime: &tokio::runtime::Handle,
    interval: Duration,
) -> Arc<Mutex<Vec<Weak<PluginInner>>>> {
    let loaded: Arc<Mutex<Vec<Weak<PluginInner>>>> = Arc::default();
    let registry = Arc::downgrade(&loaded);
    runtime.spawn(async move {
        let mut ticks = tokio::time::interval(interval);
        ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticks.tick().await;
            let Some(loaded) = registry.upgrade() else {
                return;
            };
            let plugins: Vec<Arc<PluginInner>> =
                lock(&loaded).iter().filter_map(Weak::upgrade).collect();
            drop(loaded);
            for plugin in plugins {
                plugin.sweep();
            }
        }
    });
    loaded
}

/// A Plugin ready to be called.
#[derive(Clone)]
pub struct LoadedPlugin {
    inner: Arc<PluginInner>,
}

impl std::fmt::Debug for LoadedPlugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedPlugin")
            .field("id", &self.inner.context.id)
            .finish_non_exhaustive()
    }
}

struct PluginInner {
    context: Arc<PluginContext>,
    pre: InstancePre<HostState>,
    indices: Indices,
    exports: Exports,
    backend: Backend,
    engine: wasmtime::Engine,
    pools: Mutex<HashMap<ConnectionId, Vec<Instance>>>,
    calls: Arc<tokio::sync::Semaphore>,
    idle_instance: Duration,
}

/// Where each export this host calls lives in the component, found once
/// at load. The other optional exports are wrapped as their callers land
/// (M5, part 5); `exports` already reports them.
struct Indices {
    catalog: guest::catalog::GuestIndices,
    byte_source: Option<guest::byte_source::GuestIndices>,
}

impl Indices {
    fn new(pre: &InstancePre<HostState>, exports: &Exports) -> wasmtime::Result<Self> {
        // Every export the component advertises must have its interface's
        // exact type, since the advertised ones decide consent and
        // permission differences, whether or not the host calls them yet.
        if exports.accounts {
            guest::accounts::GuestIndices::new(pre)?;
        }
        if exports.likes {
            guest::likes::GuestIndices::new(pre)?;
        }
        if exports.like_lookup {
            guest::like_lookup::GuestIndices::new(pre)?;
        }
        if exports.playlist_editing {
            guest::playlist_editing::GuestIndices::new(pre)?;
        }
        if exports.reposts {
            guest::reposts::GuestIndices::new(pre)?;
        }
        if exports.feed {
            guest::feed::GuestIndices::new(pre)?;
        }
        if exports.comments {
            guest::comments::GuestIndices::new(pre)?;
        }
        if exports.settings {
            guest::settings::GuestIndices::new(pre)?;
        }
        if exports.queue_projection {
            guest::queue_projection::GuestIndices::new(pre)?;
        }
        Ok(Self {
            catalog: guest::catalog::GuestIndices::new(pre)?,
            byte_source: exports
                .byte_source
                .then(|| guest::byte_source::GuestIndices::new(pre))
                .transpose()?,
        })
    }
}

/// The loaded exports of one instance.
pub(crate) struct Guests {
    pub(crate) catalog: guest::catalog::Guest,
    pub(crate) byte_source: Option<guest::byte_source::Guest>,
}

struct Instance {
    store: Store<HostState>,
    guests: Guests,
    baseline_memory: usize,
    last_used: Instant,
}

impl PluginInner {
    /// A fresh instance.
    fn instantiate(&self) -> Result<Instance, CallError> {
        let failed = |error: wasmtime::Error| CallError::Instantiate(format!("{error:#}"));
        let mut store = Store::new(&self.engine, HostState::new(self.context.clone()));
        store.limiter(|state| &mut state.limiter);
        store.set_hostcall_fuel(limits::HOSTCALL_FUEL_BYTES);
        store.epoch_deadline_callback(|mut context| {
            Ok(match context.data_mut().clock.check() {
                Some(_) => UpdateDeadline::Interrupt,
                None => UpdateDeadline::Continue(1),
            })
        });
        // Instantiation runs guest start code under the normal budget.
        store.data_mut().clock = self.clock(CallKind::Normal);
        store.set_epoch_deadline(1);
        let instance = self.pre.instantiate(&mut store).map_err(failed)?;
        let guests = (|| {
            Ok::<_, wasmtime::Error>(Guests {
                catalog: self.indices.catalog.load(&mut store, &instance)?,
                byte_source: self
                    .indices
                    .byte_source
                    .as_ref()
                    .map(|indices| indices.load(&mut store, &instance))
                    .transpose()?,
            })
        })()
        .map_err(failed)?;
        let baseline_memory = store.data().limiter.memory_bytes;
        Ok(Instance {
            store,
            guests,
            baseline_memory,
            last_used: Instant::now(),
        })
    }

    fn clock(&self, kind: CallKind) -> crate::host::CallClock {
        let guest = match kind {
            CallKind::Normal => limits::GUEST_TIME,
            CallKind::ByteSource => limits::BYTE_SOURCE_GUEST_TIME,
        };
        let guest = match self.backend {
            Backend::Cranelift => guest,
            Backend::Pulley => guest * limits::INTERPRETER_FACTOR,
        };
        crate::host::CallClock::start(guest, limits::CALL_TIME)
    }

    /// An idle instance of the connection, if one is still fresh.
    fn checkout(&self, connection: &ConnectionId) -> Option<Instance> {
        let mut pools = lock(&self.pools);
        let pool = pools.get_mut(connection)?;
        let idle = self.idle_instance;
        pool.retain(|instance| instance.last_used.elapsed() < idle);
        pool.pop()
    }

    fn give_back(&self, connection: ConnectionId, mut instance: Instance) {
        instance.last_used = Instant::now();
        let mut pools = lock(&self.pools);
        Self::expire(&mut pools, self.idle_instance);
        let pool = pools.entry(connection).or_default();
        if pool.len() < limits::IDLE_PER_CONNECTION {
            pool.push(instance);
        }
    }

    /// Releases every idle instance past its time, in every pool.
    fn sweep(&self) {
        let expired = {
            let mut pools = lock(&self.pools);
            Self::expire(&mut pools, self.idle_instance)
        };
        // Stores are dropped outside the lock.
        drop(expired);
    }

    fn expire(pools: &mut HashMap<ConnectionId, Vec<Instance>>, idle: Duration) -> Vec<Instance> {
        let mut expired = Vec::new();
        for pool in pools.values_mut() {
            let (keep, old): (Vec<Instance>, Vec<Instance>) = std::mem::take(pool)
                .into_iter()
                .partition(|instance| instance.last_used.elapsed() < idle);
            *pool = keep;
            expired.extend(old);
        }
        pools.retain(|_, pool| !pool.is_empty());
        expired
    }
}

impl LoadedPlugin {
    /// The Plugin's id.
    pub fn id(&self) -> &PluginId {
        &self.inner.context.id
    }

    /// Its manifest.
    pub fn manifest(&self) -> &crate::package::Manifest {
        &self.inner.context.manifest
    }

    /// The optional interfaces it exports.
    pub fn exports(&self) -> Exports {
        self.inner.exports
    }

    /// Discards every idle instance.
    pub fn discard_instances(&self) {
        lock(&self.inner.pools).clear();
    }

    /// Registers a Provider Connection or updates its account and grants.
    ///
    /// When its generation moved, the old generation ends first: every
    /// running call for it is retired, its idle instances are discarded,
    /// and its cached responses and connection data are deleted. At app
    /// start the host learns each connection with its current generation,
    /// which keeps everything stored for it.
    pub async fn set_connection(&self, info: ConnectionInfo) -> Result<(), crate::Error> {
        let connection = info.id.clone();
        let connections = &self.inner.context.connections;
        if connections.set(info) == Registered::NewGeneration {
            // Calls for the new generation wait until this is done.
            let cleaned = self.end_generation(&connection).await;
            connections.finish_cleaning(&connection);
            cleaned?;
        }
        Ok(())
    }

    /// Ends a connection for good: the user signed out or removed it. Its
    /// running calls are retired, and its instances, cached responses,
    /// connection data and Credential Record are deleted.
    pub async fn remove_connection(&self, connection: &ConnectionId) -> Result<(), crate::Error> {
        let context = &self.inner.context;
        context.connections.remove(connection);
        self.end_generation(connection).await?;
        context
            .services
            .vault
            .delete(&context.connections.vault_key(connection))
            .map_err(|error| crate::Error::Store(error.to_string()))
    }

    /// Retires every running call and discards every instance: a version of
    /// the Plugin was installed, or it was disabled.
    pub fn retire_calls(&self) {
        self.inner
            .context
            .retirements
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.discard_instances();
    }

    async fn end_generation(&self, connection: &ConnectionId) -> Result<(), crate::Error> {
        let context = &self.inner.context;
        self.discard_connection_instances(connection);
        context
            .services
            .net
            .remove_cached_connection(&context.id, connection)
            .await
            .map_err(|error| crate::Error::Store(error.to_string()))?;
        context
            .services
            .store
            .connection_kv(&context.id, connection)
            .clear()
            .map_err(|error| crate::Error::Store(error.to_string()))
    }

    /// Discards the idle instances of one connection.
    pub fn discard_connection_instances(&self, connection: &ConnectionId) {
        lock(&self.inner.pools).remove(connection);
    }

    /// Idle instances kept for a connection, for tests and diagnostics.
    pub fn idle_instances(&self, connection: &ConnectionId) -> usize {
        lock(&self.inner.pools).get(connection).map_or(0, Vec::len)
    }

    /// Runs one call on an instance of the connection. `body` gets the
    /// store, the loaded exports and the call's `connection` resource.
    pub(crate) async fn call<R: Send + 'static>(
        &self,
        connection: &ConnectionId,
        kind: CallKind,
        refresh: bool,
        body: impl FnOnce(
            &mut Store<HostState>,
            &Guests,
            Resource<ConnectionHandle>,
        ) -> wasmtime::Result<R>
        + Send
        + 'static,
    ) -> Result<R, CallError> {
        let permit = self
            .inner
            .calls
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| CallError::Lost("the host is shutting down".to_owned()))?;
        let context = &self.inner.context;
        let generation = context
            .connections
            .admit(connection)
            .await
            .ok_or(CallError::UnknownConnection)?;
        let call = CallContext {
            connection: connection.clone(),
            generation,
            retirements: context.retirements(),
            credential_base: context.connections.version(connection).unwrap_or(0),
            refresh,
        };
        let inner = self.inner.clone();
        let connection = connection.clone();
        let joined = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let mut instance = match inner.checkout(&connection) {
                Some(instance) => instance,
                None => inner.instantiate()?,
            };
            instance.store.data_mut().clock = inner.clock(kind);
            instance.store.data_mut().call = Some(call);
            instance.store.set_epoch_deadline(1);
            let handle = instance
                .store
                .data_mut()
                .table
                .push(ConnectionHandle {
                    connection: connection.clone(),
                })
                .map_err(|error| CallError::Lost(format!("{error}")))?;
            let rep = handle.rep();
            let result = body(
                &mut instance.store,
                &instance.guests,
                Resource::new_own(rep),
            );
            let _ = instance
                .store
                .data_mut()
                .table
                .delete::<ConnectionHandle>(Resource::new_own(rep));
            let exceeded = instance.store.data().clock.exceeded;
            let retired = instance.store.data().retired();
            instance.store.data_mut().call = None;
            // An instance that served a retired call never serves another:
            // nothing it kept for the old generation may reach the next.
            if retired {
                return Err(CallError::Retired);
            }
            match result {
                Ok(value) => {
                    let grown = instance
                        .store
                        .data()
                        .limiter
                        .memory_bytes
                        .saturating_sub(instance.baseline_memory);
                    if grown <= limits::RECYCLE_GROWTH_BYTES {
                        inner.give_back(connection, instance);
                    }
                    Ok(value)
                }
                // The instance is in an unknown state after any trap.
                Err(error) => Err(match exceeded {
                    Some(Exceeded::GuestTime) => CallError::GuestTime,
                    Some(Exceeded::CallTime) => CallError::CallTime,
                    None => CallError::Trapped(trap_message(&error)),
                }),
            }
        })
        .await;
        joined.unwrap_or_else(|error| Err(CallError::Lost(error.to_string())))
    }
}

/// A trap's message, without the guest backtrace's addresses.
fn trap_message(error: &wasmtime::Error) -> String {
    match error.downcast_ref::<wasmtime::Trap>() {
        Some(trap) => trap.to_string(),
        None => error.to_string(),
    }
}
