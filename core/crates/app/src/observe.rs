//! Observers and how they are notified.
//!
//! Every notification runs on the core's runtime, never inline on the thread
//! that caused it (ADR 0001). A UI transport action therefore returns at
//! once even when an observer is slow, and observers can rely on being
//! called from a core thread, which platform code hops off on its own.
//!
//! Each subscription has its own queue and delivery task, so it sees events
//! in publication order and one slow observer does not hold up another.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use tokio::runtime::Handle;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use crate::model::session::{CoreSnapshot, LibraryChange, PlaybackNotice};

/// Receives the core's state. Called on core threads, one call at a time
/// per subscription, in publication order.
pub trait Observer: Send + Sync + 'static {
    /// A new snapshot. Its revision is higher than every earlier one.
    fn on_snapshot(&self, snapshot: CoreSnapshot);
    /// A Playback Notice.
    fn on_notice(&self, notice: PlaybackNotice);
    /// Something outside the Playback Session changed.
    fn on_library_changed(&self, change: LibraryChange);
}

#[derive(Debug)]
enum Event {
    // Shared: one event goes to every subscriber.
    Snapshot(Arc<CoreSnapshot>),
    Notice(Arc<PlaybackNotice>),
    LibraryChanged(Arc<LibraryChange>),
}

impl Event {
    fn deliver(self, observer: &dyn Observer) {
        match self {
            Self::Snapshot(snapshot) => observer.on_snapshot(Arc::unwrap_or_clone(snapshot)),
            Self::Notice(notice) => observer.on_notice(Arc::unwrap_or_clone(notice)),
            Self::LibraryChanged(change) => {
                observer.on_library_changed(Arc::unwrap_or_clone(change))
            }
        }
    }
}

struct Subscriber {
    id: u64,
    events: UnboundedSender<Event>,
}

struct Registry {
    next_id: u64,
    snapshot: CoreSnapshot,
    subscribers: Vec<Subscriber>,
}

/// The current snapshot and everyone subscribed to it.
pub(crate) struct Observers {
    registry: Mutex<Registry>,
}

impl Observers {
    pub(crate) fn new(snapshot: CoreSnapshot) -> Self {
        Self {
            registry: Mutex::new(Registry {
                next_id: 1,
                snapshot,
                subscribers: Vec::new(),
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Registry> {
        // Nothing panics while holding the lock, and the registry stays
        // consistent at every step, so a poisoned lock is still usable.
        self.registry.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn snapshot(&self) -> CoreSnapshot {
        self.lock().snapshot.clone()
    }

    /// Registers `observer` and queues the current snapshot for it, so a new
    /// subscriber needs no separate read.
    pub(crate) fn subscribe(
        self: &Arc<Self>,
        observer: Arc<dyn Observer>,
        runtime: &Handle,
    ) -> Subscription {
        let (events, mut queue) = unbounded_channel();
        let active = Arc::new(AtomicBool::new(true));
        let id = {
            let mut registry = self.lock();
            let id = registry.next_id;
            registry.next_id += 1;
            // The receiver is alive, so this cannot fail.
            let _ = events.send(Event::Snapshot(Arc::new(registry.snapshot.clone())));
            registry.subscribers.push(Subscriber { id, events });
            id
        };
        let delivering = Arc::clone(&active);
        runtime.spawn(async move {
            while let Some(event) = queue.recv().await {
                if !delivering.load(Ordering::Acquire) {
                    break;
                }
                // A panicking Rust observer, or a foreign one that threw from
                // a non-throwing callback, loses that one event and keeps its
                // subscription.
                if catch_unwind(AssertUnwindSafe(|| event.deliver(&*observer))).is_err() {
                    tracing::error!(subscription = id, "observer panicked; event dropped");
                }
            }
        });
        Subscription {
            id,
            active,
            observers: Arc::downgrade(self),
        }
    }

    /// Changes the snapshot, raises its revision and queues it for every
    /// subscriber. Returns the new revision.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "first caller: the session (M7a)")
    )]
    pub(crate) fn publish_snapshot(&self, change: impl FnOnce(&mut CoreSnapshot)) -> u64 {
        let mut registry = self.lock();
        change(&mut registry.snapshot);
        registry.snapshot.revision += 1;
        let snapshot = Arc::new(registry.snapshot.clone());
        let revision = snapshot.revision;
        registry.broadcast(|| Event::Snapshot(Arc::clone(&snapshot)));
        revision
    }

    /// Queues a Playback Notice for every subscriber.
    #[allow(dead_code, reason = "first caller: playback control (M7b)")]
    pub(crate) fn publish_notice(&self, notice: PlaybackNotice) {
        let notice = Arc::new(notice);
        self.lock().broadcast(|| Event::Notice(Arc::clone(&notice)));
    }

    /// Queues a library change for every subscriber.
    #[allow(dead_code, reason = "first caller: Ristle Playlists (M7a)")]
    pub(crate) fn publish_library_change(&self, change: LibraryChange) {
        let change = Arc::new(change);
        self.lock()
            .broadcast(|| Event::LibraryChanged(Arc::clone(&change)));
    }

    fn unsubscribe(&self, id: u64) {
        self.lock()
            .subscribers
            .retain(|subscriber| subscriber.id != id);
    }

    #[cfg(test)]
    pub(crate) fn subscriber_count(&self) -> usize {
        self.lock().subscribers.len()
    }
}

impl Registry {
    /// Sends under the registry lock, so every subscriber receives events in
    /// the same order as the revisions were assigned.
    fn broadcast(&mut self, event: impl Fn() -> Event) {
        // A send fails only when the delivery task has ended, which happens
        // once the runtime shuts down; the subscriber is dropped then.
        self.subscribers
            .retain(|subscriber| subscriber.events.send(event()).is_ok());
    }
}

/// Keeps an observer subscribed. Dropping it, or [`cancel`](Self::cancel),
/// unsubscribes.
pub struct Subscription {
    id: u64,
    active: Arc<AtomicBool>,
    observers: Weak<Observers>,
}

impl Subscription {
    /// Unsubscribes and drops the events still queued for this observer. A
    /// callback already under way on a core thread may still finish.
    /// Calling it again does nothing.
    pub fn cancel(&self) {
        if self.active.swap(false, Ordering::AcqRel)
            && let Some(observers) = self.observers.upgrade()
        {
            observers.unsubscribe(self.id);
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl std::fmt::Debug for Subscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Subscription")
            .field("id", &self.id)
            .field("active", &self.active.load(Ordering::Relaxed))
            .finish()
    }
}
