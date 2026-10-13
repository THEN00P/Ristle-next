//! Spike D core stub: the shapes the real core will expose through UniFFI.
//!
//! - `Core::load_track` is an async function that may fail with a typed error and
//!   awaits a foreign async callback (`TokenProvider`).
//! - `StateObserver` is a foreign callback interface, pushed from a core thread.
//! - A fake playhead advances every 100 ms. It is delivered both by push
//!   (`subscribe`) and by pull (`next_state`, an async long-poll).
//! - `stats` exposes counters so the platform tests can check for leaked
//!   subscriptions and uncancelled futures from the Rust side.

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

uniffi::setup_scaffolding!();

const TICK: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PlaybackStatus {
    Idle,
    Loading,
    Playing,
    Paused,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct PlayerState {
    pub status: PlaybackStatus,
    pub track_id: Option<String>,
    pub title: String,
    pub position_ms: u64,
    pub duration_ms: u64,
    /// Increases with every published state, so pull consumers can resume.
    pub seq: u64,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TrackInfo {
    pub id: String,
    pub title: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct CoreStats {
    pub live_subscriptions: u64,
    pub ticks: u64,
    pub callbacks_delivered: u64,
    /// Slowest single foreign `on_state` call since `reset_max_callback`, in µs.
    pub max_callback_us: u64,
    /// Total time spent in foreign `on_state` calls, in µs.
    pub total_callback_us: u64,
    pub futures_started: u64,
    pub futures_completed: u64,
    /// Futures dropped before completion, i.e. cancelled by the foreign side.
    pub futures_cancelled: u64,
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CoreError {
    #[error("track {id} not found")]
    NotFound { id: String },
    #[error("provider failed: {reason}")]
    Provider { reason: String },
    #[error("cancelled")]
    Cancelled,
}

// A foreign callback that throws something other than CoreError arrives here.
impl From<uniffi::UnexpectedUniFFICallbackError> for CoreError {
    fn from(e: uniffi::UnexpectedUniFFICallbackError) -> Self {
        CoreError::Provider { reason: format!("unexpected callback error: {}", e.reason) }
    }
}

/// Pushed from a core worker thread, never from the platform's main thread.
#[uniffi::export(with_foreign)]
pub trait StateObserver: Send + Sync {
    fn on_state(&self, state: PlayerState);
}

/// A foreign async capability the core awaits (stands in for credential storage).
#[uniffi::export(with_foreign)]
#[async_trait::async_trait]
pub trait TokenProvider: Send + Sync {
    async fn token(&self) -> Result<String, CoreError>;
}

#[derive(Default)]
struct Counters {
    live_subscriptions: AtomicU64,
    ticks: AtomicU64,
    callbacks_delivered: AtomicU64,
    max_callback_us: AtomicU64,
    total_callback_us: AtomicU64,
    futures_started: AtomicU64,
    futures_completed: AtomicU64,
    futures_cancelled: AtomicU64,
}

struct Inner {
    state: tokio::sync::watch::Sender<PlayerState>,
    observers: Mutex<Vec<(u64, Arc<dyn StateObserver>)>>,
    next_observer_id: AtomicU64,
    token_provider: Mutex<Option<Arc<dyn TokenProvider>>>,
    counters: Counters,
}

impl Inner {
    fn update(&self, f: impl FnOnce(&mut PlayerState)) {
        self.state.send_modify(|s| {
            f(s);
            s.seq += 1;
        });
        self.broadcast();
    }

    fn broadcast(&self) {
        let state = self.state.borrow().clone();
        // Snapshot so observers may subscribe or cancel from inside on_state.
        let observers: Vec<_> = self.observers.lock().unwrap().iter().map(|(_, o)| o.clone()).collect();
        for o in observers {
            let started = Instant::now();
            o.on_state(state.clone());
            let us = started.elapsed().as_micros() as u64;
            self.counters.max_callback_us.fetch_max(us, Ordering::Relaxed);
            self.counters.total_callback_us.fetch_add(us, Ordering::Relaxed);
            self.counters.callbacks_delivered.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn tick(&self) {
        self.counters.ticks.fetch_add(1, Ordering::Relaxed);
        if self.state.borrow().status != PlaybackStatus::Playing {
            return;
        }
        self.update(|s| {
            s.position_ms += TICK.as_millis() as u64;
            if s.duration_ms > 0 && s.position_ms >= s.duration_ms {
                s.position_ms = 0; // loop the fake track so soaks keep streaming
            }
        });
    }
}

#[derive(uniffi::Object)]
pub struct Core {
    inner: Arc<Inner>,
    // Option so Drop can shut it down without blocking (see Drop below).
    rt: Option<tokio::runtime::Runtime>,
}

#[uniffi::export]
impl Core {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("ristle-core")
            .enable_time()
            .build()
            .expect("tokio runtime");
        let (state, _) = tokio::sync::watch::channel(PlayerState {
            status: PlaybackStatus::Idle,
            track_id: None,
            title: String::new(),
            position_ms: 0,
            duration_ms: 0,
            seq: 0,
        });
        let inner = Arc::new(Inner {
            state,
            observers: Mutex::new(Vec::new()),
            next_observer_id: AtomicU64::new(1),
            token_provider: Mutex::new(None),
            counters: Counters::default(),
        });
        let weak = Arc::downgrade(&inner);
        rt.spawn(async move {
            let mut interval = tokio::time::interval(TICK);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                let Some(inner) = weak.upgrade() else { break };
                inner.tick();
            }
        });
        Arc::new(Core { inner, rt: Some(rt) })
    }

    /// Resolves a track (after asking the platform for a token) and starts it.
    pub async fn load_track(&self, id: String) -> Result<TrackInfo, CoreError> {
        let inner = self.inner.clone();
        self.run(async move {
            inner.update(|s| {
                s.status = PlaybackStatus::Loading;
                s.track_id = Some(id.clone());
            });
            let provider = inner.token_provider.lock().unwrap().clone();
            if let Some(p) = provider {
                let token = p.token().await?;
                if token.is_empty() {
                    return Err(CoreError::Provider { reason: "empty token".into() });
                }
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
            if id.starts_with("missing") {
                inner.update(|s| s.status = PlaybackStatus::Idle);
                return Err(CoreError::NotFound { id });
            }
            let info = TrackInfo { title: format!("Fake track {id}"), id, duration_ms: 180_000 };
            inner.update(|s| {
                s.status = PlaybackStatus::Playing;
                s.track_id = Some(info.id.clone());
                s.title = info.title.clone();
                s.position_ms = 0;
                s.duration_ms = info.duration_ms;
            });
            Ok(info)
        })
        .await
    }

    /// Sleeps on the core runtime. Used to observe cancellation from the platform.
    pub async fn wait_ms(&self, ms: u64) -> u64 {
        self.run(async move {
            tokio::time::sleep(Duration::from_millis(ms)).await;
            ms
        })
        .await
    }

    /// Like `wait_ms`, but also stops when `token` is cancelled. Swift's generated
    /// async wrappers do not forward Task cancellation, so long operations take
    /// an explicit token there.
    pub async fn wait_ms_cancellable(&self, ms: u64, token: Arc<CancelToken>) -> Result<u64, CoreError> {
        let mut rx = token.tx.subscribe();
        self.run(async move {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(ms)) => Ok(ms),
                _ = rx.wait_for(|c| *c) => Err(CoreError::Cancelled),
            }
        })
        .await
    }

    /// Awaits the platform's TokenProvider with a timeout. On timeout the core
    /// drops the foreign future, which must cancel the platform task.
    pub async fn probe_token(&self, timeout_ms: u64) -> Result<String, CoreError> {
        let provider = self.inner.token_provider.lock().unwrap().clone();
        let Some(p) = provider else {
            return Err(CoreError::Provider { reason: "no token provider".into() });
        };
        self.run(async move {
            match tokio::time::timeout(Duration::from_millis(timeout_ms), p.token()).await {
                Ok(r) => r,
                Err(_) => Err(CoreError::Cancelled),
            }
        })
        .await
    }

    /// Long-poll for the next state after `after_seq`. Pull-style alternative
    /// to `subscribe`, maps onto a Kotlin Flow or a Swift AsyncStream.
    pub async fn next_state(&self, after_seq: u64) -> PlayerState {
        let mut rx = self.inner.state.subscribe();
        self.run(async move {
            loop {
                {
                    let s = rx.borrow_and_update();
                    if s.seq > after_seq {
                        return s.clone();
                    }
                }
                if rx.changed().await.is_err() {
                    return rx.borrow().clone();
                }
            }
        })
        .await
    }

    pub fn play(&self) {
        self.inner.update(|s| {
            if s.status == PlaybackStatus::Paused {
                s.status = PlaybackStatus::Playing;
            }
        });
    }

    pub fn pause(&self) {
        self.inner.update(|s| {
            if s.status == PlaybackStatus::Playing {
                s.status = PlaybackStatus::Paused;
            }
        });
    }

    pub fn seek(&self, position_ms: u64) {
        self.inner.update(|s| s.position_ms = position_ms.min(s.duration_ms));
    }

    pub fn state(&self) -> PlayerState {
        self.inner.state.borrow().clone()
    }

    pub fn subscribe(&self, observer: Arc<dyn StateObserver>) -> Arc<Subscription> {
        let id = self.inner.next_observer_id.fetch_add(1, Ordering::Relaxed);
        self.inner.observers.lock().unwrap().push((id, observer));
        self.inner.counters.live_subscriptions.fetch_add(1, Ordering::Relaxed);
        Arc::new(Subscription { id, inner: Arc::downgrade(&self.inner), cancelled: Mutex::new(false) })
    }

    pub fn set_token_provider(&self, provider: Option<Arc<dyn TokenProvider>>) {
        *self.inner.token_provider.lock().unwrap() = provider;
    }

    pub fn stats(&self) -> CoreStats {
        let c = &self.inner.counters;
        CoreStats {
            live_subscriptions: c.live_subscriptions.load(Ordering::Relaxed),
            ticks: c.ticks.load(Ordering::Relaxed),
            callbacks_delivered: c.callbacks_delivered.load(Ordering::Relaxed),
            max_callback_us: c.max_callback_us.load(Ordering::Relaxed),
            total_callback_us: c.total_callback_us.load(Ordering::Relaxed),
            futures_started: c.futures_started.load(Ordering::Relaxed),
            futures_completed: c.futures_completed.load(Ordering::Relaxed),
            futures_cancelled: c.futures_cancelled.load(Ordering::Relaxed),
        }
    }

    pub fn reset_max_callback(&self) {
        self.inner.counters.max_callback_us.store(0, Ordering::Relaxed);
    }

    /// Checks how a Rust panic surfaces on the platform (non-throwing function).
    pub fn trigger_panic(&self) {
        panic!("deliberate panic from the core stub");
    }

    /// The same panic from a function that can return an error.
    pub fn try_panic(&self) -> Result<(), CoreError> {
        panic!("deliberate panic from a fallible function");
    }
}

impl Core {
    /// Runs `fut` on the core runtime and awaits it from whatever executor the
    /// foreign side polls with. tokio timers need a runtime context, which the
    /// foreign poller does not provide. Dropping the returned future (foreign
    /// cancellation) aborts the task.
    fn run<T: Send + 'static>(
        &self,
        fut: impl Future<Output = T> + Send + 'static,
    ) -> impl Future<Output = T> + Send + 'static {
        self.inner.counters.futures_started.fetch_add(1, Ordering::Relaxed);
        let handle = self.rt.as_ref().unwrap().spawn(fut);
        let mut guard = CancelGuard { inner: Arc::downgrade(&self.inner), handle: Some(handle) };
        async move {
            let out = guard.handle.as_mut().unwrap().await;
            guard.handle = None;
            if let Some(inner) = guard.inner.upgrade() {
                inner.counters.futures_completed.fetch_add(1, Ordering::Relaxed);
            }
            match out {
                Ok(v) => v,
                Err(e) if e.is_panic() => std::panic::resume_unwind(e.into_panic()),
                Err(e) => panic!("core task failed: {e}"),
            }
        }
    }
}

struct CancelGuard<T> {
    inner: Weak<Inner>,
    handle: Option<tokio::task::JoinHandle<T>>,
}

impl<T> Drop for CancelGuard<T> {
    fn drop(&mut self) {
        if let Some(h) = self.handle.take() {
            h.abort();
            if let Some(inner) = self.inner.upgrade() {
                inner.counters.futures_cancelled.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        // The last reference may be released on a core worker thread (from inside
        // a callback). A blocking runtime drop would panic there.
        if let Some(rt) = self.rt.take() {
            rt.shutdown_background();
        }
    }
}

/// Explicit cancellation for long async operations, set from the platform.
#[derive(uniffi::Object)]
pub struct CancelToken {
    tx: tokio::sync::watch::Sender<bool>,
}

#[uniffi::export]
impl CancelToken {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(CancelToken { tx: tokio::sync::watch::channel(false).0 })
    }

    pub fn cancel(&self) {
        self.tx.send_replace(true);
    }
}

/// Handle returned by `subscribe`. `cancel` (or dropping it) unregisters the observer.
#[derive(uniffi::Object)]
pub struct Subscription {
    id: u64,
    inner: Weak<Inner>,
    cancelled: Mutex<bool>,
}

#[uniffi::export]
impl Subscription {
    pub fn cancel(&self) {
        let mut cancelled = self.cancelled.lock().unwrap();
        if *cancelled {
            return;
        }
        *cancelled = true;
        if let Some(inner) = self.inner.upgrade() {
            inner.observers.lock().unwrap().retain(|(id, _)| *id != self.id);
            inner.counters.live_subscriptions.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Count(AtomicU64);
    impl StateObserver for Count {
        fn on_state(&self, _: PlayerState) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn block_on<F: Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(f)
    }

    #[test]
    fn streams_ticks_and_unsubscribes() {
        let core = Core::new();
        let obs = Arc::new(Count(AtomicU64::new(0)));
        let sub = core.subscribe(obs.clone());
        block_on(core.load_track("t1".into())).unwrap();
        std::thread::sleep(Duration::from_millis(1050));
        let n = obs.0.load(Ordering::Relaxed);
        assert!((9..=14).contains(&n), "got {n} updates");
        drop(sub);
        assert_eq!(core.stats().live_subscriptions, 0);
        let before = obs.0.load(Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(obs.0.load(Ordering::Relaxed), before);
    }

    #[test]
    fn typed_error_and_cancellation() {
        let core = Core::new();
        let err = block_on(core.load_track("missing-1".into())).unwrap_err();
        assert!(matches!(err, CoreError::NotFound { .. }));
        // An async fn body only starts on first poll, so poll it before dropping.
        let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        let timed_out = rt.block_on(async { tokio::time::timeout(Duration::from_millis(50), core.wait_ms(10_000)).await });
        assert!(timed_out.is_err());
        assert_eq!(core.stats().futures_cancelled, 1);
    }

    #[test]
    fn cancel_token_stops_wait() {
        let core = Core::new();
        let token = CancelToken::new();
        let t = token.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            t.cancel();
        });
        let started = Instant::now();
        let r = block_on(core.wait_ms_cancellable(10_000, token));
        assert!(matches!(r, Err(CoreError::Cancelled)));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn next_state_long_polls() {
        let core = Core::new();
        block_on(core.load_track("t1".into())).unwrap();
        let s1 = block_on(core.next_state(0));
        let s2 = block_on(core.next_state(s1.seq));
        assert!(s2.seq > s1.seq);
    }
}
