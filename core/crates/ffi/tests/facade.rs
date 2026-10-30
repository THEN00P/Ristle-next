//! The facade's conventions (ADR 0001), through its public API: observer
//! callbacks arrive on core threads, cancellation ends a call, and errors
//! arrive typed. Plus the list of methods whose milestone has not landed.

// clippy.toml exempts `#[test]` functions only; the helpers here are test
// code too.
#![allow(clippy::unwrap_used)]

use std::future::Future;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use ristle_core::types::*;
use ristle_core::{CancelToken, CoreError, CoreObserver, PlatformError, PlatformHost, RistleCore};

const WAIT: Duration = Duration::from_secs(5);

struct NoSecrets;

impl PlatformHost for NoSecrets {
    fn read_secret(&self, _key: String) -> Result<Option<Vec<u8>>, PlatformError> {
        Ok(None)
    }
    fn write_secret(&self, _key: String, _value: Vec<u8>) -> Result<(), PlatformError> {
        Ok(())
    }
    fn delete_secret(&self, _key: String) -> Result<(), PlatformError> {
        Ok(())
    }
}

/// A core over a temporary directory, which must outlive it.
fn core() -> (tempfile::TempDir, Arc<RistleCore>) {
    let dir = tempfile::tempdir().unwrap();
    let core = RistleCore::new(
        CoreConfig {
            data_dir: dir.path().join("data").to_string_lossy().into_owned(),
            cache_dir: dir.path().join("cache").to_string_lossy().into_owned(),
        },
        Arc::new(NoSecrets),
    )
    .unwrap();
    (dir, core)
}

/// Polls a facade future the way a platform does: from its own thread,
/// outside any tokio runtime.
fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(future)
}

/// Reports the thread each snapshot arrives on.
struct ThreadRecorder(Mutex<Sender<(thread::ThreadId, Option<String>, u64)>>);

impl CoreObserver for ThreadRecorder {
    fn on_snapshot(&self, snapshot: CoreSnapshot) {
        let current = thread::current();
        let _ = self.0.lock().unwrap().send((
            current.id(),
            current.name().map(str::to_owned),
            snapshot.revision,
        ));
    }
    fn on_notice(&self, _notice: PlaybackNotice) {}
    fn on_library_changed(&self, _change: LibraryChange) {}
}

#[test]
fn a_callback_from_a_synchronous_command_arrives_on_a_core_thread() {
    let (_dir, core) = core();
    let (sender, calls) = channel();

    let _subscription = core.subscribe(Arc::new(ThreadRecorder(Mutex::new(sender))));

    let (thread, name, revision) = calls.recv_timeout(WAIT).unwrap();
    assert_ne!(thread, thread::current().id());
    assert_eq!(name.as_deref(), Some(ristle_app::CORE_THREAD_NAME));
    assert_eq!(revision, core.snapshot().revision);
}

#[test]
fn a_cancelled_subscription_receives_nothing() {
    let (_dir, core) = core();
    let (sender, calls) = channel();
    let subscription = core.subscribe(Arc::new(ThreadRecorder(Mutex::new(sender))));
    calls.recv_timeout(WAIT).unwrap();

    subscription.cancel();
    drop(subscription);

    assert!(calls.recv_timeout(Duration::from_millis(200)).is_err());
}

#[test]
fn a_cancelled_token_ends_an_async_call_with_cancelled() {
    let (_dir, core) = core();
    let token = CancelToken::new();
    assert!(!token.is_cancelled());
    token.cancel();
    assert!(token.is_cancelled());

    let result = block_on(core.search("query".into(), SearchScope::Everywhere, token));

    assert_eq!(result, Err(CoreError::Cancelled));
}

#[test]
fn an_invalid_config_is_a_typed_error() {
    let error = RistleCore::new(
        CoreConfig {
            data_dir: "data".into(),
            cache_dir: String::new(),
        },
        Arc::new(NoSecrets),
    )
    .unwrap_err();
    assert!(matches!(error, CoreError::InvalidArgument { .. }));
}

fn item() -> ItemId {
    ItemId {
        provider: "mock".into(),
        id: "1".into(),
        connection: None,
    }
}

fn token() -> Arc<CancelToken> {
    CancelToken::new()
}

/// Queue and playlist commands are accepted and return at once; their
/// effects reach observers (covered in `ristle-app`'s tests).
#[test]
fn queue_commands_are_accepted() {
    let (_dir, core) = core();
    let results = [
        core.next(),
        core.previous(),
        core.set_repeat(RepeatMode::Queue),
        core.set_shuffle(true),
        core.remove_entry(1),
        core.undo_removal(),
        core.move_request(1, 0),
        core.clear_upcoming(),
        core.jump_to(1),
        core.rename_playlist("p".into(), "t".into()),
        core.delete_playlist("p".into()),
    ];
    assert!(results.iter().all(Result::is_ok), "{results:?}");
}

/// Ristle Playlists and Recently Played read through the facade.
#[test]
fn playlists_and_recently_played_read_through_the_facade() {
    let (_dir, core) = core();
    assert_eq!(core.recently_played().unwrap(), []);
    let created = core.create_playlist("Morning".into()).unwrap();
    assert_eq!(created.title, "Morning");
    assert_eq!(created.track_count, 0);
    // The create is a queued command; the listing sees it once it ran.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while core.ristle_playlists().unwrap().is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "the playlist never appeared"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(core.ristle_playlists().unwrap(), [created]);
}

/// Every method that still returns `NotReady`. Remove a line when its
/// milestone lands; phase 2 ends with this list empty (M9).
#[test]
fn methods_not_implemented_yet_say_so() {
    let (_dir, core) = core();
    let results: Vec<(&str, Result<(), CoreError>)> = vec![
        ("play", core.play()),
        ("pause", core.pause()),
        ("seek", core.seek(1_000)),
        ("set_volume", core.set_volume(0.5)),
        (
            "add_to_queue",
            core.add_to_queue(vec![ItemRef::Track { item: item() }], Placement::Last),
        ),
        (
            "handle_media_command",
            core.handle_media_command(MediaCommand::PlayPause),
        ),
        (
            "play_source",
            block_on(core.play_source(PlaySource::LikedTracks, StartAt::Beginning, false, token())),
        ),
        (
            "search",
            block_on(core.search("q".into(), SearchScope::MyLibrary, token())).map(drop),
        ),
        (
            "open_page",
            block_on(core.open_page(PageTarget::Album { item: item() }, false, token())).map(drop),
        ),
        (
            "list",
            block_on(core.list(
                CollectionRef {
                    connection: "c".into(),
                    id: "liked".into(),
                },
                None,
                token(),
            ))
            .map(drop),
        ),
        ("library", block_on(core.library(true, token())).map(drop)),
        (
            "set_liked",
            block_on(core.set_liked(ItemRef::Track { item: item() }, true)),
        ),
        (
            "playlist_add",
            block_on(core.playlist_add(
                PlaylistDestination::NewRistle { title: "t".into() },
                vec![ItemRef::Track { item: item() }],
                Duplicates::Ask,
                token(),
            ))
            .map(drop),
        ),
        ("plugins", core.plugins().map(drop)),
        (
            "review_package",
            block_on(core.review_package(vec![0, 97, 115, 109])).map(drop),
        ),
        (
            "install",
            block_on(core.install("r".into(), Consent::Install)).map(drop),
        ),
        (
            "begin_sign_in",
            block_on(core.begin_sign_in("mock".into(), token())).map(drop),
        ),
        (
            "continue_sign_in",
            block_on(core.continue_sign_in("s".into(), SignInAnswer::Continue, token())).map(drop),
        ),
        ("sign_out", block_on(core.sign_out("c".into()))),
        (
            "plugin_settings",
            core.plugin_settings("c".into()).map(drop),
        ),
        (
            "apply_setting",
            block_on(core.apply_setting("c".into(), "k".into(), SettingValue::Pressed)).map(drop),
        ),
    ];

    for (method, result) in results {
        assert_eq!(
            result,
            Err(CoreError::NotReady {
                feature: method.to_owned()
            }),
            "{method}"
        );
    }
}
