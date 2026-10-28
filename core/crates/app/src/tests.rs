//! Observer dispatch and lifecycle of [`Core`].

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::*;

const WAIT: Duration = Duration::from_secs(5);

struct NoPlatform;

impl Platform for NoPlatform {
    fn read_secret(&self, _key: &str) -> Result<Option<Vec<u8>>, PlatformError> {
        Ok(None)
    }
    fn write_secret(&self, _key: &str, _value: &[u8]) -> Result<(), PlatformError> {
        Ok(())
    }
    fn delete_secret(&self, _key: &str) -> Result<(), PlatformError> {
        Ok(())
    }
}

/// A core over a temporary directory, which lives as long as the core.
struct TestCore {
    core: Option<Core>,
    _dir: tempfile::TempDir,
}

impl std::ops::Deref for TestCore {
    type Target = Core;
    fn deref(&self) -> &Core {
        self.core.as_ref().unwrap()
    }
}

impl TestCore {
    fn take(&mut self) -> Core {
        self.core.take().unwrap()
    }
}

fn core() -> TestCore {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::new(
        CoreConfig {
            data_dir: dir.path().join("data").to_string_lossy().into_owned(),
            cache_dir: dir.path().join("cache").to_string_lossy().into_owned(),
        },
        Arc::new(NoPlatform),
    )
    .unwrap();
    TestCore {
        core: Some(core),
        _dir: dir,
    }
}

#[derive(Debug)]
struct Call {
    thread: Option<String>,
    revision: Option<u64>,
    notice: Option<PlaybackNotice>,
}

/// Reports every callback, with the thread it ran on.
struct Recorder(Mutex<Sender<Call>>);

impl Recorder {
    fn new() -> (Arc<Self>, Receiver<Call>) {
        let (sender, receiver) = channel();
        (Arc::new(Self(Mutex::new(sender))), receiver)
    }

    fn record(&self, revision: Option<u64>, notice: Option<PlaybackNotice>) {
        let thread = thread::current().name().map(str::to_owned);
        let _ = self.0.lock().unwrap().send(Call {
            thread,
            revision,
            notice,
        });
    }
}

impl Observer for Recorder {
    fn on_snapshot(&self, snapshot: CoreSnapshot) {
        self.record(Some(snapshot.revision), None);
    }
    fn on_notice(&self, notice: PlaybackNotice) {
        self.record(None, Some(notice));
    }
    fn on_library_changed(&self, _change: LibraryChange) {
        self.record(None, None);
    }
}

#[test]
fn a_new_subscriber_receives_the_current_snapshot_on_a_core_thread() {
    let core = core();
    let (observer, calls) = Recorder::new();
    let _subscription = core.subscribe(observer);

    let call = calls.recv_timeout(WAIT).unwrap();
    assert_eq!(call.revision, Some(0));
    assert_eq!(call.thread.as_deref(), Some(CORE_THREAD_NAME));
    assert_eq!(core.snapshot().revision, 0);
}

#[test]
fn changes_published_on_the_callers_thread_arrive_in_order_on_core_threads() {
    let core = core();
    let (observer, calls) = Recorder::new();
    let _subscription = core.subscribe(observer);
    assert_eq!(calls.recv_timeout(WAIT).unwrap().revision, Some(0));

    for _ in 0..50 {
        core.observers()
            .publish_snapshot(|s| s.shuffle = !s.shuffle);
    }
    core.observers().publish_notice(PlaybackNotice::Cleared);

    for expected in 1..=50 {
        let call = calls.recv_timeout(WAIT).unwrap();
        assert_eq!(call.revision, Some(expected));
        assert_eq!(call.thread.as_deref(), Some(CORE_THREAD_NAME));
    }
    let notice = calls.recv_timeout(WAIT).unwrap();
    assert_eq!(notice.notice, Some(PlaybackNotice::Cleared));
    assert_eq!(core.snapshot().revision, 50);
}

#[test]
fn a_cancelled_subscription_receives_nothing_more() {
    let core = core();
    let (observer, calls) = Recorder::new();
    let subscription = core.subscribe(observer);
    calls.recv_timeout(WAIT).unwrap();

    subscription.cancel();
    subscription.cancel();
    core.observers().publish_snapshot(|_| {});

    assert!(calls.recv_timeout(Duration::from_millis(200)).is_err());
    assert_eq!(core.observers().subscriber_count(), 0);
}

#[test]
fn dropping_the_subscription_unsubscribes() {
    let core = core();
    let (observer, _calls) = Recorder::new();
    drop(core.subscribe(observer));
    assert_eq!(core.observers().subscriber_count(), 0);
}

struct Panicking(Recorder);

impl Observer for Panicking {
    fn on_snapshot(&self, snapshot: CoreSnapshot) {
        self.0.on_snapshot(snapshot);
        panic!("observer failed");
    }
    fn on_notice(&self, notice: PlaybackNotice) {
        self.0.on_notice(notice);
    }
    fn on_library_changed(&self, change: LibraryChange) {
        self.0.on_library_changed(change);
    }
}

#[test]
fn a_panicking_observer_keeps_its_subscription() {
    let core = core();
    let (sender, calls) = channel();
    let _subscription = core.subscribe(Arc::new(Panicking(Recorder(Mutex::new(sender)))));
    calls.recv_timeout(WAIT).unwrap();

    core.observers().publish_snapshot(|_| {});
    core.observers().publish_notice(PlaybackNotice::Cleared);

    assert_eq!(calls.recv_timeout(WAIT).unwrap().revision, Some(1));
    assert_eq!(
        calls.recv_timeout(WAIT).unwrap().notice,
        Some(PlaybackNotice::Cleared)
    );
}

/// Drops the core from inside its own callback, on a core thread.
struct DropsCore {
    core: Mutex<Option<Core>>,
    dropped: Mutex<Sender<()>>,
}

impl Observer for DropsCore {
    fn on_snapshot(&self, _snapshot: CoreSnapshot) {
        if let Some(core) = self.core.lock().unwrap().take() {
            drop(core);
            let _ = self.dropped.lock().unwrap().send(());
        }
    }
    fn on_notice(&self, _notice: PlaybackNotice) {}
    fn on_library_changed(&self, _change: LibraryChange) {}
}

#[test]
fn the_core_can_be_dropped_on_a_core_thread() {
    let (sender, dropped) = channel();
    let observer = Arc::new(DropsCore {
        core: Mutex::new(None),
        dropped: Mutex::new(sender),
    });
    let mut test = core();
    let observers = test.observers_arc();
    let subscription = test.subscribe(Arc::clone(&observer) as Arc<dyn Observer>);
    *observer.core.lock().unwrap() = Some(test.take());

    observers.publish_snapshot(|_| {});

    dropped.recv_timeout(WAIT).unwrap();
    drop(subscription);
}

#[test]
fn rejects_an_empty_data_directory() {
    let error = Core::new(
        CoreConfig {
            data_dir: String::new(),
            cache_dir: "cache".into(),
        },
        Arc::new(NoPlatform),
    )
    .unwrap_err();
    assert!(matches!(error, Error::InvalidArgument { .. }));
}
