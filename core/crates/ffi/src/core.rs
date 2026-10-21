//! `RistleCore`, the one object UIs hold, and `Subscription`.
//!
//! Conversion only: each method calls the `ristle_app::Core` method of the
//! same name. Async methods run on the core's runtime, so tokio works inside
//! them whichever executor the platform polls with, and dropping the
//! platform's future aborts the core task.

use std::future::Future;
use std::sync::Arc;

use tokio::task::JoinHandle;

use crate::cancel::CancelToken;
use crate::error::CoreError;
use crate::platform::{CoreObserver, ForeignObserver, ForeignPlatform, PlatformHost};
use crate::types::{
    CollectionRef, Consent, CoreConfig, CoreSnapshot, Duplicates, InstallReview, ItemRef,
    LibraryView, ListingPage, MediaCommand, Page, PageTarget, Placement, PlaySource,
    PlaylistAddOutcome, PlaylistDestination, PlaylistSummary, PluginView, RepeatMode, SearchScope,
    SearchSnapshot, SettingValue, SettingsGroup, SignInAnswer, SignInStep, StartAt, TrackView,
};

/// The Rust core. Create one per process, off the main thread (JNA's first
/// use touches disk), and keep it for the process's life.
///
/// Synchronous commands return at once; their effects arrive through
/// `CoreObserver`. Async calls that read or start something take a
/// `CancelToken`.
#[derive(Debug, uniffi::Object)]
pub struct RistleCore {
    core: Arc<ristle_app::Core>,
}

#[uniffi::export]
impl RistleCore {
    /// Starts the core.
    #[uniffi::constructor]
    pub fn new(
        config: CoreConfig,
        platform: Arc<dyn PlatformHost>,
    ) -> Result<Arc<Self>, CoreError> {
        let core = ristle_app::Core::new(config, Arc::new(ForeignPlatform(platform)))?;
        Ok(Arc::new(Self {
            core: Arc::new(core),
        }))
    }

    /// Subscribes an observer. It receives the current snapshot first, then
    /// every change. Keep the returned `Subscription`; dropping or
    /// cancelling it unsubscribes.
    pub fn subscribe(&self, observer: Arc<dyn CoreObserver>) -> Arc<Subscription> {
        let inner = self.core.subscribe(Arc::new(ForeignObserver(observer)));
        Arc::new(Subscription { inner })
    }

    /// The current snapshot: now playing, queue window and transport.
    pub fn snapshot(&self) -> CoreSnapshot {
        self.core.snapshot()
    }

    /// Plays.
    pub fn play(&self) -> Result<(), CoreError> {
        Ok(self.core.play()?)
    }

    /// Pauses.
    pub fn pause(&self) -> Result<(), CoreError> {
        Ok(self.core.pause()?)
    }

    /// Moves to the next Track.
    pub fn next(&self) -> Result<(), CoreError> {
        Ok(self.core.next()?)
    }

    /// Restarts the Track, or moves to the previous one near its start.
    pub fn previous(&self) -> Result<(), CoreError> {
        Ok(self.core.previous()?)
    }

    /// Moves the playhead.
    pub fn seek(&self, position_ms: u64) -> Result<(), CoreError> {
        Ok(self.core.seek(position_ms)?)
    }

    /// Sets repeat.
    pub fn set_repeat(&self, mode: RepeatMode) -> Result<(), CoreError> {
        Ok(self.core.set_repeat(mode)?)
    }

    /// Shuffles the continuation, keeping the current Track, or restores
    /// its order.
    pub fn set_shuffle(&self, enabled: bool) -> Result<(), CoreError> {
        Ok(self.core.set_shuffle(enabled)?)
    }

    /// Sets the output volume, 0.0 to 1.0.
    pub fn set_volume(&self, volume: f32) -> Result<(), CoreError> {
        Ok(self.core.set_volume(volume)?)
    }

    /// Adds items as Queued Requests.
    pub fn add_to_queue(&self, items: Vec<ItemRef>, placement: Placement) -> Result<(), CoreError> {
        Ok(self.core.add_to_queue(items, placement)?)
    }

    /// Removes a queue entry. `undo_removal` puts it back.
    pub fn remove_entry(&self, entry: u64) -> Result<(), CoreError> {
        Ok(self.core.remove_entry(entry)?)
    }

    /// Undoes the latest `remove_entry`.
    pub fn undo_removal(&self) -> Result<(), CoreError> {
        Ok(self.core.undo_removal()?)
    }

    /// Moves a Queued Request to a position among the requests.
    pub fn move_request(&self, entry: u64, to: u32) -> Result<(), CoreError> {
        Ok(self.core.move_request(entry, to)?)
    }

    /// Removes every upcoming entry and detaches the queue's source.
    pub fn clear_upcoming(&self) -> Result<(), CoreError> {
        Ok(self.core.clear_upcoming()?)
    }

    /// Plays a queue entry now.
    pub fn jump_to(&self, entry: u64) -> Result<(), CoreError> {
        Ok(self.core.jump_to(entry)?)
    }

    /// Applies a command from a platform media session.
    pub fn handle_media_command(&self, command: MediaCommand) -> Result<(), CoreError> {
        Ok(self.core.handle_media_command(command)?)
    }

    /// Replaces the queue's continuation with a source and plays it.
    pub async fn play_source(
        &self,
        source: PlaySource,
        start: StartAt,
        shuffle: bool,
        cancel: Arc<CancelToken>,
    ) -> Result<(), CoreError> {
        let core = Arc::clone(&self.core);
        self.run(Some(cancel), async move {
            core.play_source(source, start, shuffle).await
        })
        .await
    }

    /// Runs a Unified Search and returns its stable snapshot.
    pub async fn search(
        &self,
        query: String,
        scope: SearchScope,
        cancel: Arc<CancelToken>,
    ) -> Result<SearchSnapshot, CoreError> {
        let core = Arc::clone(&self.core);
        self.run(Some(cancel), async move { core.search(query, scope).await })
            .await
    }

    /// Opens a page, from cache unless `refresh`.
    pub async fn open_page(
        &self,
        target: PageTarget,
        refresh: bool,
        cancel: Arc<CancelToken>,
    ) -> Result<Page, CoreError> {
        let core = Arc::clone(&self.core);
        self.run(Some(cancel), async move {
            core.open_page(target, refresh).await
        })
        .await
    }

    /// Reads one page of a collection. `cursor` is `None` for the first.
    pub async fn list(
        &self,
        collection: CollectionRef,
        cursor: Option<String>,
        cancel: Arc<CancelToken>,
    ) -> Result<ListingPage, CoreError> {
        let core = Arc::clone(&self.core);
        self.run(
            Some(cancel),
            async move { core.list(collection, cursor).await },
        )
        .await
    }

    /// The library across every Provider Connection, from cache unless
    /// `refresh`.
    pub async fn library(
        &self,
        refresh: bool,
        cancel: Arc<CancelToken>,
    ) -> Result<LibraryView, CoreError> {
        let core = Arc::clone(&self.core);
        self.run(Some(cancel), async move { core.library(refresh).await })
            .await
    }

    /// Likes or unlikes an item on its Provider.
    pub async fn set_liked(&self, item: ItemRef, liked: bool) -> Result<(), CoreError> {
        let core = Arc::clone(&self.core);
        self.run(None, async move { core.set_liked(item, liked).await })
            .await
    }

    /// Every Ristle Playlist, most recently changed first.
    pub fn ristle_playlists(&self) -> Result<Vec<PlaylistSummary>, CoreError> {
        Ok(self.core.ristle_playlists()?)
    }

    /// Creates an empty Ristle Playlist.
    pub fn create_playlist(&self, title: String) -> Result<PlaylistSummary, CoreError> {
        Ok(self.core.create_playlist(title)?)
    }

    /// Renames a Ristle Playlist.
    pub fn rename_playlist(&self, playlist: String, title: String) -> Result<(), CoreError> {
        Ok(self.core.rename_playlist(playlist, title)?)
    }

    /// Deletes a Ristle Playlist.
    pub fn delete_playlist(&self, playlist: String) -> Result<(), CoreError> {
        Ok(self.core.delete_playlist(playlist)?)
    }

    /// Appends the Tracks of items to a playlist (Playlist Add).
    pub async fn playlist_add(
        &self,
        destination: PlaylistDestination,
        items: Vec<ItemRef>,
        duplicates: Duplicates,
        cancel: Arc<CancelToken>,
    ) -> Result<PlaylistAddOutcome, CoreError> {
        let core = Arc::clone(&self.core);
        self.run(Some(cancel), async move {
            core.playlist_add(destination, items, duplicates).await
        })
        .await
    }

    /// Recently Played, latest first.
    pub fn recently_played(&self) -> Result<Vec<TrackView>, CoreError> {
        Ok(self.core.recently_played()?)
    }

    /// Every installed Plugin with its Provider Connections.
    pub fn plugins(&self) -> Result<Vec<PluginView>, CoreError> {
        Ok(self.core.plugins()?)
    }

    /// Verifies a Plugin Package and describes what installing it would do.
    pub async fn review_package(&self, package: Vec<u8>) -> Result<InstallReview, CoreError> {
        let core = Arc::clone(&self.core);
        self.run(None, async move { core.review_package(package).await })
            .await
    }

    /// Installs a reviewed package with the user's consent.
    pub async fn install(&self, review: String, consent: Consent) -> Result<PluginView, CoreError> {
        let core = Arc::clone(&self.core);
        self.run(None, async move { core.install(review, consent).await })
            .await
    }

    /// Starts an Authorization Session for a new Provider Connection.
    pub async fn begin_sign_in(
        &self,
        plugin: String,
        cancel: Arc<CancelToken>,
    ) -> Result<SignInStep, CoreError> {
        let core = Arc::clone(&self.core);
        self.run(
            Some(cancel),
            async move { core.begin_sign_in(plugin).await },
        )
        .await
    }

    /// Answers the latest step of an Authorization Session.
    pub async fn continue_sign_in(
        &self,
        session: String,
        answer: SignInAnswer,
        cancel: Arc<CancelToken>,
    ) -> Result<SignInStep, CoreError> {
        let core = Arc::clone(&self.core);
        self.run(Some(cancel), async move {
            core.continue_sign_in(session, answer).await
        })
        .await
    }

    /// Removes a Provider Connection and its credential.
    pub async fn sign_out(&self, connection: String) -> Result<(), CoreError> {
        let core = Arc::clone(&self.core);
        self.run(None, async move { core.sign_out(connection).await })
            .await
    }

    /// A Provider Connection's Plugin settings.
    pub fn plugin_settings(&self, connection: String) -> Result<Vec<SettingsGroup>, CoreError> {
        Ok(self.core.plugin_settings(connection)?)
    }

    /// Changes a Plugin setting. Returns a reason to show, if the Plugin
    /// has one.
    pub async fn apply_setting(
        &self,
        connection: String,
        key: String,
        value: SettingValue,
    ) -> Result<Option<String>, CoreError> {
        let core = Arc::clone(&self.core);
        self.run(None, async move {
            core.apply_setting(connection, key, value).await
        })
        .await
    }
}

impl RistleCore {
    /// Runs `task` on the core runtime and waits for it, for `cancel`, or
    /// for the caller to drop this future, whichever comes first. The last
    /// two abort the task. A token cancelled before the call wins over a
    /// result that is ready at once.
    async fn run<T: Send + 'static>(
        &self,
        cancel: Option<Arc<CancelToken>>,
        task: impl Future<Output = Result<T, ristle_app::Error>> + Send + 'static,
    ) -> Result<T, CoreError> {
        run_on(self.core.runtime(), cancel, task).await
    }
}

pub(crate) async fn run_on<T: Send + 'static>(
    runtime: &tokio::runtime::Handle,
    cancel: Option<Arc<CancelToken>>,
    task: impl Future<Output = Result<T, ristle_app::Error>> + Send + 'static,
) -> Result<T, CoreError> {
    let mut task = AbortOnDrop(runtime.spawn(task));
    let cancelled = async {
        match &cancel {
            Some(token) => token.cancelled().await,
            None => std::future::pending().await,
        }
    };
    tokio::select! {
        biased;
        () = cancelled => Err(CoreError::Cancelled),
        joined = &mut task.0 => match joined {
            Ok(result) => result.map_err(CoreError::from),
            Err(error) if error.is_cancelled() => Err(CoreError::Cancelled),
            // Never resumed: a panic must not cross the boundary (ADR 0001).
            Err(_) => Err(CoreError::Internal {
                reason: "a core task panicked".to_owned(),
            }),
        },
    }
}

/// Aborts the task when the platform drops the future that awaits it.
struct AbortOnDrop<T>(JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Keeps a `CoreObserver` subscribed. `cancel`, or releasing the object,
/// unsubscribes.
#[derive(Debug, uniffi::Object)]
pub struct Subscription {
    inner: ristle_app::Subscription,
}

#[uniffi::export]
impl Subscription {
    /// Unsubscribes and drops events not yet delivered. A callback already
    /// under way may still finish. Calling it again does nothing.
    pub fn cancel(&self) {
        self.inner.cancel();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use super::*;

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_time()
            .build()
            .unwrap()
    }

    /// Sets its flag when dropped, which is how an aborted task shows.
    struct DropFlag(Arc<AtomicBool>);

    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    async fn forever(flag: DropFlag) -> Result<(), ristle_app::Error> {
        let _flag = flag;
        std::future::pending().await
    }

    fn wait_for(flag: &AtomicBool) {
        for _ in 0..500 {
            if flag.load(Ordering::SeqCst) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the core task was not aborted");
    }

    #[test]
    fn cancelling_the_token_ends_the_call_and_aborts_the_task() {
        let core = runtime();
        let caller = runtime();
        let token = CancelToken::new();
        let aborted = Arc::new(AtomicBool::new(false));

        let canceller = Arc::clone(&token);
        let task = forever(DropFlag(Arc::clone(&aborted)));
        let result = caller.block_on(async {
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(50)).await;
                canceller.cancel();
            });
            run_on(core.handle(), Some(token), task).await
        });

        assert_eq!(result, Err(CoreError::Cancelled));
        wait_for(&aborted);
    }

    #[test]
    fn dropping_the_call_aborts_the_task() {
        let core = runtime();
        let caller = runtime();
        let aborted = Arc::new(AtomicBool::new(false));

        let task = forever(DropFlag(Arc::clone(&aborted)));
        let timed_out = caller.block_on(async {
            tokio::time::timeout(Duration::from_millis(50), run_on(core.handle(), None, task)).await
        });

        assert!(timed_out.is_err());
        wait_for(&aborted);
    }

    #[test]
    fn a_token_cancelled_before_the_call_wins() {
        let core = runtime();
        let token = CancelToken::new();
        token.cancel();
        let result = runtime().block_on(run_on(core.handle(), Some(token), async { Ok(1) }));
        assert_eq!(result, Err(CoreError::Cancelled));
    }

    #[test]
    fn a_panicking_task_is_an_internal_error() {
        let core = runtime();
        let result: Result<(), CoreError> =
            runtime().block_on(run_on(core.handle(), None, async { panic!("core bug") }));
        assert!(matches!(result, Err(CoreError::Internal { .. })));
    }

    #[test]
    fn app_errors_map_to_core_errors() {
        use ristle_app::{Error, ProviderErrorKind};
        let reason = || "m".to_owned();
        let cases = [
            (
                Error::NotReady {
                    feature: "f".into(),
                },
                CoreError::NotReady {
                    feature: "f".into(),
                },
            ),
            (Error::Cancelled, CoreError::Cancelled),
            (
                Error::InvalidArgument { reason: reason() },
                CoreError::InvalidArgument { reason: reason() },
            ),
            (
                Error::Provider {
                    kind: ProviderErrorKind::Transient,
                    reason: reason(),
                },
                CoreError::Provider {
                    kind: ProviderErrorKind::Transient,
                    reason: reason(),
                },
            ),
            (
                Error::Storage { reason: reason() },
                CoreError::Storage { reason: reason() },
            ),
            (
                Error::Platform { reason: reason() },
                CoreError::Platform { reason: reason() },
            ),
            (
                Error::Internal { reason: reason() },
                CoreError::Internal { reason: reason() },
            ),
        ];
        for (app, ffi) in cases {
            assert_eq!(CoreError::from(app), ffi);
        }
    }
}
