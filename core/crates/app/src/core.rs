//! [`Core`], the composition root, and every command a UI can send.

use std::sync::Arc;

use tokio::runtime::{Builder, Handle, Runtime};

use crate::error::Error;
use crate::model::items::{ItemRef, TrackView};
use crate::model::pages::{CollectionRef, LibraryView, ListingPage, Page, PageTarget};
use crate::model::playlists::{
    Duplicates, PlaylistAddOutcome, PlaylistDestination, PlaylistSummary,
};
use crate::model::plugins::{Consent, InstallReview, PluginView, SettingValue, SettingsGroup};
use crate::model::search::{SearchScope, SearchSnapshot};
use crate::model::session::{
    CoreConfig, CoreSnapshot, MediaCommand, Placement, PlaySource, RepeatMode, StartAt,
};
use crate::model::sign_in::{SignInAnswer, SignInStep};
use crate::observe::{Observer, Observers, Subscription};
use crate::platform::Platform;

/// Name of every runtime worker thread. Observers are called on these.
pub const CORE_THREAD_NAME: &str = "ristle-core";

/// Worker threads of the core runtime. The audio engine and storage have
/// their own threads; this runtime runs commands, Plugin calls and
/// observers.
const WORKER_THREADS: usize = 2;

/// The application core. One per process.
///
/// Commands that change the Playback Session return at once; their effects
/// arrive through [`Observer`]s. Reads and starts are `async` and run on any
/// executor, but are meant to be spawned on [`runtime`](Self::runtime).
/// Dropping one of their futures cancels it.
pub struct Core {
    shared: Arc<Shared>,
    runtime_handle: Handle,
    // Only `None` while dropping.
    runtime: Option<Runtime>,
}

struct Shared {
    observers: Arc<Observers>,
    #[allow(dead_code, reason = "first callers: Plugin secrets (M7b)")]
    config: CoreConfig,
    #[allow(dead_code, reason = "first callers: Plugin secrets (M7b)")]
    platform: Arc<dyn Platform>,
}

impl Core {
    /// Starts the core: its runtime now, storage and Plugins as their
    /// milestones land.
    pub fn new(config: CoreConfig, platform: Arc<dyn Platform>) -> Result<Self, Error> {
        for (name, dir) in [
            ("data_dir", &config.data_dir),
            ("cache_dir", &config.cache_dir),
        ] {
            if dir.is_empty() {
                return Err(Error::InvalidArgument {
                    reason: format!("{name} is empty"),
                });
            }
        }
        let runtime = Builder::new_multi_thread()
            .worker_threads(WORKER_THREADS)
            .thread_name(CORE_THREAD_NAME)
            .enable_all()
            .build()
            .map_err(|error| Error::Internal {
                reason: format!("cannot start the core runtime: {error}"),
            })?;
        Ok(Self {
            shared: Arc::new(Shared {
                observers: Arc::new(Observers::new(CoreSnapshot::empty())),
                config,
                platform,
            }),
            runtime_handle: runtime.handle().clone(),
            runtime: Some(runtime),
        })
    }

    /// The core's runtime. Facades spawn async calls on it, so they run on
    /// core threads whichever executor awaits them.
    pub fn runtime(&self) -> &Handle {
        &self.runtime_handle
    }

    /// Subscribes an observer. It receives the current snapshot first, then
    /// every change, on core threads.
    pub fn subscribe(&self, observer: Arc<dyn Observer>) -> Subscription {
        self.shared
            .observers
            .subscribe(observer, &self.runtime_handle)
    }

    /// The current snapshot.
    pub fn snapshot(&self) -> CoreSnapshot {
        self.shared.observers.snapshot()
    }

    // Transport and queue. Each returns at once; effects arrive through
    // observers.

    /// Plays.
    pub fn play(&self) -> Result<(), Error> {
        Err(Error::not_ready("play"))
    }

    /// Pauses.
    pub fn pause(&self) -> Result<(), Error> {
        Err(Error::not_ready("pause"))
    }

    /// Moves to the next Track.
    pub fn next(&self) -> Result<(), Error> {
        Err(Error::not_ready("next"))
    }

    /// Restarts the Track, or moves to the previous one near its start.
    pub fn previous(&self) -> Result<(), Error> {
        Err(Error::not_ready("previous"))
    }

    /// Moves the playhead.
    pub fn seek(&self, _position_ms: u64) -> Result<(), Error> {
        Err(Error::not_ready("seek"))
    }

    /// Sets repeat.
    pub fn set_repeat(&self, _mode: RepeatMode) -> Result<(), Error> {
        Err(Error::not_ready("set_repeat"))
    }

    /// Shuffles the continuation, keeping the current Track, or restores
    /// its order.
    pub fn set_shuffle(&self, _enabled: bool) -> Result<(), Error> {
        Err(Error::not_ready("set_shuffle"))
    }

    /// Sets the output volume, 0.0 to 1.0.
    pub fn set_volume(&self, _volume: f32) -> Result<(), Error> {
        Err(Error::not_ready("set_volume"))
    }

    /// Adds items as Queued Requests. Albums and playlists are read in the
    /// background and arrive as one ordered group.
    pub fn add_to_queue(&self, _items: Vec<ItemRef>, _placement: Placement) -> Result<(), Error> {
        Err(Error::not_ready("add_to_queue"))
    }

    /// Removes a queue entry. `undo_removal` puts it back.
    pub fn remove_entry(&self, _entry: u64) -> Result<(), Error> {
        Err(Error::not_ready("remove_entry"))
    }

    /// Undoes the latest `remove_entry`.
    pub fn undo_removal(&self) -> Result<(), Error> {
        Err(Error::not_ready("undo_removal"))
    }

    /// Moves a Queued Request to a position among the requests.
    pub fn move_request(&self, _entry: u64, _to: u32) -> Result<(), Error> {
        Err(Error::not_ready("move_request"))
    }

    /// Removes every upcoming entry and detaches the queue's source.
    pub fn clear_upcoming(&self) -> Result<(), Error> {
        Err(Error::not_ready("clear_upcoming"))
    }

    /// Plays a queue entry now.
    pub fn jump_to(&self, _entry: u64) -> Result<(), Error> {
        Err(Error::not_ready("jump_to"))
    }

    /// Applies a command from a platform media session.
    pub fn handle_media_command(&self, _command: MediaCommand) -> Result<(), Error> {
        Err(Error::not_ready("handle_media_command"))
    }

    // Reading and starting.

    /// Replaces the queue's continuation with a source and plays it.
    pub async fn play_source(
        &self,
        _source: PlaySource,
        _start: StartAt,
        _shuffle: bool,
    ) -> Result<(), Error> {
        Err(Error::not_ready("play_source"))
    }

    /// Runs a Unified Search and returns its stable snapshot.
    pub async fn search(
        &self,
        _query: String,
        _scope: SearchScope,
    ) -> Result<SearchSnapshot, Error> {
        Err(Error::not_ready("search"))
    }

    /// Opens a page, from cache unless `refresh`.
    pub async fn open_page(&self, _target: PageTarget, _refresh: bool) -> Result<Page, Error> {
        Err(Error::not_ready("open_page"))
    }

    /// Reads one page of a collection. `cursor` is `None` for the first.
    pub async fn list(
        &self,
        _collection: CollectionRef,
        _cursor: Option<String>,
    ) -> Result<ListingPage, Error> {
        Err(Error::not_ready("list"))
    }

    /// The library across every Provider Connection, from cache unless
    /// `refresh`.
    pub async fn library(&self, _refresh: bool) -> Result<LibraryView, Error> {
        Err(Error::not_ready("library"))
    }

    /// Likes or unlikes an item on its Provider.
    pub async fn set_liked(&self, _item: ItemRef, _liked: bool) -> Result<(), Error> {
        Err(Error::not_ready("set_liked"))
    }

    // Ristle Playlists and history.

    /// Every Ristle Playlist, most recently changed first.
    pub fn ristle_playlists(&self) -> Result<Vec<PlaylistSummary>, Error> {
        Err(Error::not_ready("ristle_playlists"))
    }

    /// Creates an empty Ristle Playlist.
    pub fn create_playlist(&self, _title: String) -> Result<PlaylistSummary, Error> {
        Err(Error::not_ready("create_playlist"))
    }

    /// Renames a Ristle Playlist.
    pub fn rename_playlist(&self, _playlist: String, _title: String) -> Result<(), Error> {
        Err(Error::not_ready("rename_playlist"))
    }

    /// Deletes a Ristle Playlist. If it is playing, the queue keeps its
    /// entries and detaches from it.
    pub fn delete_playlist(&self, _playlist: String) -> Result<(), Error> {
        Err(Error::not_ready("delete_playlist"))
    }

    /// Appends the Tracks of items to a playlist (Playlist Add).
    pub async fn playlist_add(
        &self,
        _destination: PlaylistDestination,
        _items: Vec<ItemRef>,
        _duplicates: Duplicates,
    ) -> Result<PlaylistAddOutcome, Error> {
        Err(Error::not_ready("playlist_add"))
    }

    /// Recently Played, latest first.
    pub fn recently_played(&self) -> Result<Vec<TrackView>, Error> {
        Err(Error::not_ready("recently_played"))
    }

    // Plugins, Provider Connections and sign-in.

    /// Every installed Plugin with its Provider Connections.
    pub fn plugins(&self) -> Result<Vec<PluginView>, Error> {
        Err(Error::not_ready("plugins"))
    }

    /// Verifies a Plugin Package and describes what installing it would do.
    pub async fn review_package(&self, _package: Vec<u8>) -> Result<InstallReview, Error> {
        Err(Error::not_ready("review_package"))
    }

    /// Installs a reviewed package with the user's consent.
    pub async fn install(&self, _review: String, _consent: Consent) -> Result<PluginView, Error> {
        Err(Error::not_ready("install"))
    }

    /// Starts an Authorization Session for a new Provider Connection of a
    /// Plugin.
    pub async fn begin_sign_in(&self, _plugin: String) -> Result<SignInStep, Error> {
        Err(Error::not_ready("begin_sign_in"))
    }

    /// Answers the latest step of an Authorization Session.
    pub async fn continue_sign_in(
        &self,
        _session: String,
        _answer: SignInAnswer,
    ) -> Result<SignInStep, Error> {
        Err(Error::not_ready("continue_sign_in"))
    }

    /// Removes a Provider Connection and its credential.
    pub async fn sign_out(&self, _connection: String) -> Result<(), Error> {
        Err(Error::not_ready("sign_out"))
    }

    /// A Provider Connection's Plugin settings.
    pub fn plugin_settings(&self, _connection: String) -> Result<Vec<SettingsGroup>, Error> {
        Err(Error::not_ready("plugin_settings"))
    }

    /// Changes a Plugin setting. Returns a reason to show, if the Plugin
    /// has one.
    pub async fn apply_setting(
        &self,
        _connection: String,
        _key: String,
        _value: SettingValue,
    ) -> Result<Option<String>, Error> {
        Err(Error::not_ready("apply_setting"))
    }

    #[cfg(test)]
    pub(crate) fn observers(&self) -> &Observers {
        &self.shared.observers
    }

    #[cfg(test)]
    pub(crate) fn observers_arc(&self) -> Arc<Observers> {
        Arc::clone(&self.shared.observers)
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        // The last reference may go away on a core thread, inside a task or
        // an observer callback, where a blocking runtime drop would panic.
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

impl std::fmt::Debug for Core {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Core").finish_non_exhaustive()
    }
}
