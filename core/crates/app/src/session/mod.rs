//! The Playback Session: the Canonical Queue, the sequence its continuation
//! comes from, where the current Track stands, and whether it plays, kept
//! in memory and in `ristle.sqlite` together. Ristle Playlists are edited
//! here too, because editing the playlist that plays changes the queue.
//!
//! Every change runs as one mutation: it works on a copy, the copy is saved
//! in one storage transaction (with the playlists, settings and command
//! receipt the mutation wrote), and only then replaces the session in memory
//! and is published to observers. A mutation that fails, or whose save
//! fails, leaves memory, the database and the receipts as they were.
//! Mutations run one at a time.
//!
//! Each mutation runs as its own task on the core runtime, and its caller
//! only waits for the result. A caller that is dropped, such as a cancelled
//! platform call, therefore cannot stop a mutation between its save and
//! the swap in memory: once the save is submitted, memory and the snapshot
//! follow it.
//!
//! A cold start restores the saved session paused (CONTEXT.md, Playback
//! Session).

use std::future::Future;
use std::sync::Arc;

use ristle_domain::{Clock, PlaylistId, RistlePlaylist, Track};
use ristle_queue::sequence::{SequenceId, SequenceSource};
use ristle_queue::{
    Advance, CanonicalQueue, CollectionEntry, EntryId, QueueEntry, QueueState, Rejection, Removed,
    Submitter,
};
use ristle_storage::Storage;
use ristle_storage::repo::{Intent, PlaybackSession, RecentlyPlayedEntry};
use tokio::runtime::Handle;
use tokio::sync::Mutex;

use crate::error::Error;
use crate::model::session::{CoreSnapshot, LibraryChange, RepeatMode};
use crate::observe::Observers;
use crate::services::SeedSource;

pub(crate) mod sequence;
pub(crate) mod view;

use sequence::Active;

/// Previous restarts the Track instead once it has played this long.
pub const PREVIOUS_RESTARTS_AFTER_MS: u64 = 3_000;

/// The session's state.
struct State {
    queue: CanonicalQueue,
    /// The sequence the continuation comes from, if any.
    active: Option<Active>,
    position_ms: u64,
    intent: Intent,
    /// The latest removal, for `undo_removal`. Not persisted: undo is for
    /// the moment, not across restarts.
    removed: Option<Removed>,
}

impl State {
    fn shuffled(&self, queue: &QueueState) -> bool {
        self.active
            .as_ref()
            .map_or(queue.shuffle.is_some(), Active::shuffled)
    }
}

/// A command receipt to record with a mutation (`apply_once`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    /// Who issued the command, such as a Ristle Party.
    pub scope: String,
    /// The command's key within the scope, such as a member's sequence
    /// number.
    pub key: String,
    /// What the command came to, such as `applied` or `rejected: ...`.
    pub outcome: String,
}

enum PlaylistWrite {
    Save(RistlePlaylist),
    Delete(PlaylistId),
}

/// A mutation's working copy of the session. Changes reach the session
/// only if the mutation returns `Ok` and its save succeeds.
pub struct Edit {
    queue: CanonicalQueue,
    active: Option<Active>,
    position_ms: u64,
    intent: Intent,
    removed: Option<Removed>,
    /// When the mutation runs, by the core's clock.
    now_ms: u64,
    /// The sequence the session named before, deleted if it no longer does.
    previous_sequence: Option<SequenceId>,
    playlists: Vec<PlaylistWrite>,
    settings: Vec<(String, String)>,
    changes: Vec<LibraryChange>,
}

impl Edit {
    /// The Canonical Queue.
    pub fn queue(&mut self) -> &mut CanonicalQueue {
        &mut self.queue
    }

    /// Where the current Track stands.
    pub fn set_position(&mut self, position_ms: u64) {
        self.position_ms = position_ms;
    }

    /// Whether playback should run.
    pub fn set_intent(&mut self, intent: Intent) {
        self.intent = intent;
    }

    /// Replaces the collection with `tracks` and plays the first one that
    /// can play at or after `start`, and plays. Fails, changing nothing,
    /// when none can. The queue no longer follows a sequence.
    pub fn start_tracks(&mut self, tracks: Vec<Track>, start: usize) -> Result<(), Error> {
        let outcome = self
            .queue
            .start_collection(CollectionEntry::numbered(tracks, 0), start);
        if !outcome.started {
            return Err(Error::InvalidArgument {
                reason: "nothing at or after the start can play".to_owned(),
            });
        }
        self.active = None;
        self.removed = None;
        self.position_ms = 0;
        self.intent = Intent::Playing;
        Ok(())
    }

    /// Writes a setting in the mutation's transaction.
    pub fn put_setting(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.settings.push((key.into(), value.into()));
    }

    /// Saves a Ristle Playlist, and reconciles the queue if it is the one
    /// playing (legacy `ActiveRistlePlaylistEditsReconcileThePersistedUpcomingQueue`).
    fn save_playlist(&mut self, playlist: RistlePlaylist) -> Result<(), Error> {
        let playing = self
            .active
            .as_ref()
            .is_some_and(|active| active.playlist_id() == playlist.id.as_str());
        if playing && let Some(active) = self.active.take() {
            self.active = active.reconcile(Arc::new(playlist.clone()), &mut self.queue)?;
            self.removed = None;
        }
        self.playlists.push(PlaylistWrite::Save(playlist));
        self.changes.push(LibraryChange::RistlePlaylists);
        Ok(())
    }
}

impl std::fmt::Debug for Edit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Edit").finish_non_exhaustive()
    }
}

fn storage_error(error: ristle_storage::Error) -> Error {
    Error::Storage {
        reason: error.to_string(),
    }
}

fn rejected(rejection: Rejection) -> Error {
    Error::InvalidArgument {
        reason: rejection.to_string(),
    }
}

fn no_playlist(id: &PlaylistId) -> Error {
    Error::InvalidArgument {
        reason: format!("there is no Ristle Playlist {id}"),
    }
}

impl Inner {
    /// One mutation, start to end; see the module documentation. `prepare`
    /// runs first, under the session lock, so what it reads cannot change
    /// before the mutation is saved.
    async fn commit<P, T>(
        &self,
        receipt: Option<Receipt>,
        prepare: impl Future<Output = Result<P, Error>>,
        change: impl FnOnce(&mut Edit, P) -> Result<T, Error>,
    ) -> Result<Option<T>, Error> {
        let mut state = self.state.lock().await;
        // Checked under the session lock, so no other mutation can record
        // the same receipt between the check and the save.
        if let Some(receipt) = &receipt
            && read_receipt(&self.storage, &receipt.scope, &receipt.key)
                .await?
                .is_some()
        {
            return Ok(None);
        }
        let prepared = prepare.await?;
        let mut edit = Edit {
            queue: state.queue.clone(),
            active: state.active.clone(),
            position_ms: state.position_ms,
            intent: state.intent,
            removed: state.removed.clone(),
            now_ms: self.clock.now_ms(),
            previous_sequence: state.active.as_ref().map(|active| active.id().clone()),
            playlists: Vec::new(),
            settings: Vec::new(),
            changes: Vec::new(),
        };
        let value = change(&mut edit, prepared)?;
        if let Some(active) = edit.active.as_mut() {
            active.sync(&mut edit.queue);
        }

        let queue: QueueState = edit.queue.snapshot();
        let sequence = edit
            .active
            .as_ref()
            .map(|active| active.sequence.state().clone());
        let session = PlaybackSession {
            sequence: sequence.as_ref().map(|state| state.descriptor.id.clone()),
            queue,
            position_ms: edit.position_ms,
            intent: edit.intent,
        };
        let shuffled = edit
            .active
            .as_ref()
            .map_or(session.queue.shuffle.is_some(), Active::shuffled);
        let unused = edit
            .previous_sequence
            .take()
            .filter(|previous| session.sequence.as_ref() != Some(previous));
        let playlists = std::mem::take(&mut edit.playlists);
        let settings = std::mem::take(&mut edit.settings);
        let changes = std::mem::take(&mut edit.changes);
        self.storage
            .write(move |tx| {
                for write in &playlists {
                    match write {
                        PlaylistWrite::Save(playlist) => {
                            tx.playlists().save(playlist)?;
                        }
                        PlaylistWrite::Delete(id) => {
                            tx.playlists().delete(id)?;
                        }
                    }
                }
                if let Some(sequence) = &sequence {
                    tx.sequences().save(sequence)?;
                }
                tx.session().save(&session)?;
                if let Some(unused) = &unused {
                    tx.sequences().delete(unused)?;
                }
                for (key, value) in &settings {
                    tx.settings().put(key, value)?;
                }
                if let Some(receipt) = &receipt {
                    tx.receipts()
                        .put(&receipt.scope, &receipt.key, &receipt.outcome)?;
                }
                Ok(())
            })
            .await
            .map_err(storage_error)?;

        *state = State {
            queue: edit.queue,
            active: edit.active,
            position_ms: edit.position_ms,
            intent: edit.intent,
            removed: edit.removed,
        };
        // Published under the session lock, so revisions follow the order
        // of mutations.
        self.observers.publish_snapshot(|snapshot| {
            view::fill_snapshot(
                snapshot,
                &state.queue,
                shuffled,
                state.intent,
                state.position_ms,
                state.active.as_ref(),
            );
        });
        for change in changes {
            self.observers.publish_library_change(change);
        }
        Ok(Some(value))
    }
}

async fn read_receipt(storage: &Storage, scope: &str, key: &str) -> Result<Option<String>, Error> {
    let (scope, key) = (scope.to_owned(), key.to_owned());
    storage
        .read(move |tx| tx.receipts().get(&scope, &key))
        .await
        .map_err(storage_error)
}

async fn read_playlist(storage: Arc<Storage>, id: PlaylistId) -> Result<RistlePlaylist, Error> {
    let wanted = id.clone();
    storage
        .read(move |tx| tx.playlists().load(&wanted))
        .await
        .map_err(storage_error)?
        .ok_or_else(|| no_playlist(&id))
}

/// The Playback Session.
pub struct Session {
    inner: Arc<Inner>,
    /// Where mutations run (see the module documentation).
    runtime: Handle,
}

struct Inner {
    state: Mutex<State>,
    storage: Arc<Storage>,
    observers: Arc<Observers>,
    seeds: Arc<dyn SeedSource>,
    clock: Arc<dyn Clock>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").finish_non_exhaustive()
    }
}

/// Reads the saved session and the sequence it follows. A sequence that
/// cannot be restored is let go: the queue keeps what it materialized.
fn restore_state(storage: &Storage) -> State {
    let mut state = State {
        queue: CanonicalQueue::new(),
        active: None,
        position_ms: 0,
        intent: Intent::Paused,
        removed: None,
    };
    let saved = storage
        .read_blocking(|tx| tx.session().load_for_cold_start())
        .map_err(|error| tracing::error!(%error, "the saved Playback Session cannot be read"))
        .ok()
        .flatten();
    let Some(saved) = saved else {
        return state;
    };
    match CanonicalQueue::restore(saved.session.queue) {
        Ok(queue) => state.queue = queue,
        Err(error) => {
            tracing::error!(%error, "the saved Canonical Queue is corrupt; starting empty");
            return state;
        }
    }
    state.position_ms = saved.session.position_ms;
    // `load_for_cold_start` always restores paused.
    state.intent = saved.session.intent;
    if let Some(id) = saved.session.sequence {
        let restored = storage.read_blocking(move |tx| {
            let Some(sequence) = tx.sequences().load(&id)? else {
                return Ok(None);
            };
            let playlist = match sequence.descriptor.sources.as_slice() {
                [SequenceSource::RistlePlaylist(playlist)] => tx.playlists().load(playlist)?,
                _ => None,
            };
            Ok(playlist.map(|playlist| (sequence, playlist)))
        });
        state.active = match restored {
            Ok(Some((sequence, playlist))) => Active::restore(sequence, Arc::new(playlist))
                .map_err(|error| tracing::error!(%error, "the saved sequence is let go"))
                .ok(),
            Ok(None) => {
                tracing::warn!("the saved sequence or its playlist is gone; let go");
                None
            }
            Err(error) => {
                tracing::error!(%error, "the saved sequence cannot be read; let go");
                None
            }
        };
    }
    state
}

impl Session {
    /// Restores the saved session, paused. A session that cannot be read
    /// starts empty; the reason is logged. Blocks on storage, so it runs
    /// before the runtime takes commands.
    pub(crate) fn restore(
        storage: Arc<Storage>,
        seeds: Arc<dyn SeedSource>,
        clock: Arc<dyn Clock>,
        runtime: Handle,
    ) -> Self {
        let state = restore_state(&storage);
        let mut snapshot = CoreSnapshot::empty();
        let queue = state.queue.snapshot();
        view::fill_snapshot(
            &mut snapshot,
            &state.queue,
            state.shuffled(&queue),
            state.intent,
            state.position_ms,
            state.active.as_ref(),
        );
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(state),
                storage,
                observers: Arc::new(Observers::new(snapshot)),
                seeds,
                clock,
            }),
            runtime,
        }
    }

    pub(crate) fn observers(&self) -> &Arc<Observers> {
        &self.inner.observers
    }

    pub(crate) fn storage(&self) -> &Arc<Storage> {
        &self.inner.storage
    }

    /// Runs one mutation; see the module documentation.
    async fn mutate<T: Send + 'static>(
        &self,
        change: impl FnOnce(&mut Edit) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, Error> {
        self.mutate_with(std::future::ready(Ok(())), move |edit, ()| change(edit))
            .await
    }

    /// Runs one mutation after `prepare`, which reads what it needs under
    /// the session lock.
    async fn mutate_with<P, T: Send + 'static>(
        &self,
        prepare: impl Future<Output = Result<P, Error>> + Send + 'static,
        change: impl FnOnce(&mut Edit, P) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, Error> {
        // Without a receipt there is nothing to skip.
        self.mutate_once(None, prepare, change)
            .await?
            .ok_or_else(|| Error::Internal {
                reason: "a mutation without a receipt was skipped".to_owned(),
            })
    }

    /// Runs one mutation as its own task, unless `receipt` was recorded
    /// before (`None`). Dropping the returned future does not stop it.
    async fn mutate_once<P, T: Send + 'static>(
        &self,
        receipt: Option<Receipt>,
        prepare: impl Future<Output = Result<P, Error>> + Send + 'static,
        change: impl FnOnce(&mut Edit, P) -> Result<T, Error> + Send + 'static,
    ) -> Result<Option<T>, Error> {
        let inner = Arc::clone(&self.inner);
        self.runtime
            .spawn(async move { inner.commit(receipt, prepare, change).await })
            .await
            // A panicking mutation changed nothing: it never got to the
            // swap. Its error must not unwind into the caller.
            .map_err(|error| Error::Internal {
                reason: format!("a session mutation did not finish: {error}"),
            })?
    }

    /// Applies a command at most once: if `receipt`'s scope and key were
    /// recorded before, nothing happens and `None` is returned. Otherwise
    /// the mutation and the receipt are saved together, so a command is
    /// never applied without its receipt or recorded without its effect
    /// (legacy `PlaybackCommandReceiptSurvivesRestart...`).
    pub async fn apply_once<T: Send + 'static>(
        &self,
        receipt: Receipt,
        change: impl FnOnce(&mut Edit) -> Result<T, Error> + Send + 'static,
    ) -> Result<Option<T>, Error> {
        self.mutate_once(
            Some(receipt),
            std::future::ready(Ok(())),
            move |edit, ()| change(edit),
        )
        .await
    }

    /// The outcome recorded for a command, if any.
    pub async fn receipt(&self, scope: &str, key: &str) -> Result<Option<String>, Error> {
        read_receipt(&self.inner.storage, scope, key).await
    }

    /// Forgets every receipt of a scope, such as a Party that ended.
    /// Returns how many there were.
    pub async fn clear_receipts(&self, scope: &str) -> Result<u64, Error> {
        let scope = scope.to_owned();
        self.inner
            .storage
            .write(move |tx| tx.receipts().clear_scope(&scope))
            .await
            .map_err(storage_error)
    }

    /// A setting's value.
    pub async fn setting(&self, key: &str) -> Result<Option<String>, Error> {
        let key = key.to_owned();
        self.inner
            .storage
            .read(move |tx| tx.settings().get(&key))
            .await
            .map_err(storage_error)
    }

    /// Writes a setting outside any mutation.
    pub async fn put_setting(&self, key: &str, value: &str) -> Result<(), Error> {
        let (key, value) = (key.to_owned(), value.to_owned());
        self.inner
            .storage
            .write(move |tx| tx.settings().put(&key, &value))
            .await
            .map_err(storage_error)
    }

    /// The Canonical Queue as it is now.
    pub async fn queue(&self) -> QueueState {
        self.inner.state.lock().await.queue.snapshot()
    }

    /// The state of the sequence the continuation comes from, if any.
    pub async fn sequence(&self) -> Option<ristle_queue::SequenceState> {
        self.inner
            .state
            .lock()
            .await
            .active
            .as_ref()
            .map(|active| active.sequence.state().clone())
    }

    /// Where the current Track stands, and whether it should play.
    pub async fn transport(&self) -> (Intent, u64) {
        let state = self.inner.state.lock().await;
        (state.intent, state.position_ms)
    }

    /// Replaces the collection with `tracks` and plays the first one that
    /// can play at or after `start`. When none can, the session is left as
    /// it was and the call fails, so a start that cannot happen never stops
    /// what is playing.
    pub async fn start_tracks(&self, tracks: Vec<Track>, start: usize) -> Result<(), Error> {
        self.mutate(move |edit| edit.start_tracks(tracks, start))
            .await
    }

    /// Plays a Ristle Playlist: from the Track at index `start`, or from its
    /// beginning (shuffled: from a random Track). Only a window of it is
    /// materialized into the queue. When nothing in it can play, the session
    /// is left as it was and the call fails.
    pub async fn start_playlist(
        &self,
        playlist: &PlaylistId,
        start: Option<usize>,
        shuffle: bool,
    ) -> Result<(), Error> {
        let seed = shuffle.then(|| self.inner.seeds.next_seed());
        let load = read_playlist(Arc::clone(&self.inner.storage), playlist.clone());
        self.mutate_with(load, move |edit, playlist| {
            let active = Active::start(Arc::new(playlist), start, seed, &mut edit.queue)?;
            edit.active = Some(active);
            edit.removed = None;
            edit.position_ms = 0;
            edit.intent = Intent::Playing;
            Ok(())
        })
        .await
    }

    /// Every Ristle Playlist, most recently changed first.
    pub async fn playlists(&self) -> Result<Vec<ristle_storage::repo::PlaylistSummary>, Error> {
        self.inner
            .storage
            .read(|tx| tx.playlists().list())
            .await
            .map_err(storage_error)
    }

    /// A Ristle Playlist with its Tracks.
    pub async fn playlist(&self, id: &PlaylistId) -> Result<RistlePlaylist, Error> {
        read_playlist(Arc::clone(&self.inner.storage), id.clone()).await
    }

    /// Saves a Ristle Playlist whole, creating it or replacing what was
    /// saved under its id. Editing the playlist that plays reconciles the
    /// queue (legacy `SaveRistlePlaylist`).
    pub async fn save_playlist(&self, playlist: RistlePlaylist) -> Result<(), Error> {
        self.mutate(move |edit| edit.save_playlist(playlist)).await
    }

    /// A new, empty Ristle Playlist with a fresh id, not saved yet.
    pub fn new_playlist(&self, title: String) -> RistlePlaylist {
        let now_ms = self.inner.clock.now_ms();
        RistlePlaylist {
            id: PlaylistId::new(format!("{:016x}", self.inner.seeds.next_seed())),
            title,
            tracks: Vec::new(),
            created_ms: now_ms,
            updated_ms: now_ms,
        }
    }

    /// Creates an empty Ristle Playlist and returns it.
    pub async fn create_playlist(&self, title: String) -> Result<RistlePlaylist, Error> {
        let playlist = self.new_playlist(title);
        self.save_playlist(playlist.clone()).await?;
        Ok(playlist)
    }

    /// Renames a Ristle Playlist.
    pub async fn rename_playlist(&self, id: &PlaylistId, title: String) -> Result<(), Error> {
        let load = read_playlist(Arc::clone(&self.inner.storage), id.clone());
        self.mutate_with(load, move |edit, mut playlist| {
            playlist.title = title;
            playlist.updated_ms = edit.now_ms;
            edit.save_playlist(playlist)
        })
        .await
    }

    /// Appends Tracks to a Ristle Playlist. Duplicates and Disabled Tracks
    /// are kept: a playlist holds what the user put in it.
    pub async fn add_to_playlist(&self, id: &PlaylistId, tracks: Vec<Track>) -> Result<(), Error> {
        let load = read_playlist(Arc::clone(&self.inner.storage), id.clone());
        self.mutate_with(load, move |edit, mut playlist| {
            playlist.tracks.extend(tracks);
            playlist.updated_ms = edit.now_ms;
            edit.save_playlist(playlist)
        })
        .await
    }

    /// Deletes a Ristle Playlist. If it plays, the queue keeps its entries
    /// and no longer follows it (legacy
    /// `DeletingTheActiveRistlePlaylistDetachesItsQueueSource`). Returns
    /// whether there was one.
    pub async fn delete_playlist(&self, id: &PlaylistId) -> Result<bool, Error> {
        let wanted = id.clone();
        let exists = {
            let storage = Arc::clone(&self.inner.storage);
            async move {
                storage
                    .read(move |tx| tx.playlists().load(&wanted))
                    .await
                    .map(|playlist| playlist.is_some())
                    .map_err(storage_error)
            }
        };
        let id = id.clone();
        self.mutate_with(exists, move |edit, exists| {
            if !exists {
                return Ok(false);
            }
            if edit
                .active
                .as_ref()
                .is_some_and(|active| active.playlist_id() == id.as_str())
            {
                edit.active = None;
            }
            edit.playlists.push(PlaylistWrite::Delete(id));
            edit.changes.push(LibraryChange::RistlePlaylists);
            Ok(true)
        })
        .await
    }

    /// Records a Track that has played for `listened_ms`, if that qualifies
    /// it for Recently Played (CONTEXT.md: 30 seconds or half its duration,
    /// whichever comes first; Live Tracks after 30 seconds). Returns whether
    /// it was recorded.
    pub async fn record_played(&self, track: Track, listened_ms: u64) -> Result<bool, Error> {
        let recorded = self
            .inner
            .storage
            .write(move |tx| tx.recently_played().record(&track, listened_ms))
            .await
            .map_err(storage_error)?;
        if recorded {
            self.inner
                .observers
                .publish_library_change(LibraryChange::RecentlyPlayed);
        }
        Ok(recorded)
    }

    /// Recently Played, latest first.
    pub async fn recently_played(&self) -> Result<Vec<RecentlyPlayedEntry>, Error> {
        self.inner
            .storage
            .read(|tx| tx.recently_played().list())
            .await
            .map_err(storage_error)
    }

    /// Adds a Queued Request after the others. A Disabled Track is refused.
    pub async fn add_to_queue(
        &self,
        track: Track,
        by: Option<Submitter>,
    ) -> Result<EntryId, Error> {
        self.mutate(move |edit| edit.queue.add_to_queue(track, by).map_err(rejected))
            .await
    }

    /// Adds a Queued Request after the other Play Next requests.
    pub async fn play_next(&self, track: Track, by: Option<Submitter>) -> Result<EntryId, Error> {
        self.mutate(move |edit| edit.queue.play_next(track, by).map_err(rejected))
            .await
    }

    /// Moves on: `Skip` for Next, `TrackEnded` when the Track finished (it
    /// replays with repeat Track). Returns whether something is playing.
    pub async fn advance(&self, how: Advance) -> Result<bool, Error> {
        self.mutate(move |edit| {
            let playing = edit.queue.advance(how);
            edit.position_ms = 0;
            Ok(playing)
        })
        .await
    }

    /// Goes back to the entry that played before, from its start. Returns
    /// whether there was one.
    pub async fn go_back(&self) -> Result<bool, Error> {
        self.mutate(|edit| {
            let moved = edit.queue.go_back();
            edit.position_ms = 0;
            Ok(moved)
        })
        .await
    }

    /// Previous: restarts the Track once it has played for
    /// [`PREVIOUS_RESTARTS_AFTER_MS`], otherwise goes back.
    pub async fn previous(&self) -> Result<(), Error> {
        self.mutate(|edit| {
            if edit.position_ms < PREVIOUS_RESTARTS_AFTER_MS {
                edit.queue.go_back();
            }
            edit.position_ms = 0;
            Ok(())
        })
        .await
    }

    /// Plays an entry of the queue now. Returns whether it is playing.
    pub async fn jump_to(&self, entry: EntryId) -> Result<bool, Error> {
        self.mutate(move |edit| {
            let playing = edit.queue.advance_to(entry);
            if playing {
                edit.position_ms = 0;
            }
            Ok(playing)
        })
        .await
    }

    /// Removes an upcoming entry. Returns whether there was one.
    pub async fn remove(&self, entry: EntryId) -> Result<bool, Error> {
        self.mutate(move |edit| {
            let removed = edit.queue.remove(entry);
            let found = removed.is_some();
            if found {
                edit.removed = removed;
            }
            Ok(found)
        })
        .await
    }

    /// Puts the latest removed entry back. Returns whether it was.
    pub async fn undo_removal(&self) -> Result<bool, Error> {
        self.mutate(|edit| {
            let Some(removed) = edit.removed.take() else {
                return Ok(false);
            };
            Ok(edit.queue.undo_remove(removed))
        })
        .await
    }

    /// Moves a Queued Request to a place among the requests.
    pub async fn move_request(&self, entry: EntryId, to: usize) -> Result<bool, Error> {
        self.mutate(move |edit| Ok(edit.queue.move_request(entry, to)))
            .await
    }

    /// Removes everything upcoming. The queue no longer follows its
    /// sequence, which would otherwise fill it again. Returns how many
    /// entries went.
    pub async fn clear_upcoming(&self) -> Result<usize, Error> {
        self.mutate(|edit| {
            edit.removed = None;
            edit.active = None;
            Ok(edit.queue.clear_upcoming())
        })
        .await
    }

    /// Sets what repeats.
    pub async fn set_repeat(&self, mode: RepeatMode) -> Result<(), Error> {
        self.mutate(move |edit| {
            edit.queue.set_repeat(view::queue_repeat(mode));
            Ok(())
        })
        .await
    }

    /// Shuffles what is still to come with a new seed, or restores its
    /// order: the whole sequence the queue follows, or else the queue's own
    /// collection. Turning on shuffle when it is on keeps the order.
    pub async fn set_shuffle(&self, enabled: bool) -> Result<(), Error> {
        let seed = self.inner.seeds.next_seed();
        self.mutate(move |edit| {
            if let Some(active) = edit.active.as_mut() {
                if active.shuffled() != enabled {
                    active.set_shuffle(enabled.then_some(seed), &mut edit.queue);
                    edit.removed = None;
                }
                return Ok(());
            }
            let shuffled = edit.queue.snapshot().shuffle.is_some();
            match (enabled, shuffled) {
                (true, false) => edit.queue.shuffle_upcoming(seed),
                (false, true) => edit.queue.restore_upcoming_order(),
                _ => {}
            }
            Ok(())
        })
        .await
    }

    /// Records whether playback runs and where the Track stands.
    pub async fn set_transport(&self, intent: Intent, position_ms: u64) -> Result<(), Error> {
        self.mutate(move |edit| {
            edit.intent = intent;
            edit.position_ms = position_ms;
            Ok(())
        })
        .await
    }

    /// Moves the playhead.
    pub async fn seek(&self, position_ms: u64) -> Result<(), Error> {
        self.mutate(move |edit| {
            edit.position_ms = position_ms;
            Ok(())
        })
        .await
    }

    /// The entry Next (`Skip`) or the end of the Track (`TrackEnded`) would
    /// play, without changing the session.
    pub async fn peek_next(&self, how: Advance) -> Option<QueueEntry> {
        let mut queue = self.inner.state.lock().await.queue.clone();
        queue
            .advance(how)
            .then(|| queue.current().cloned())
            .flatten()
    }

    /// The entry going back would play, without changing the session.
    pub async fn peek_previous(&self) -> Option<QueueEntry> {
        let mut queue = self.inner.state.lock().await.queue.clone();
        queue.go_back().then(|| queue.current().cloned()).flatten()
    }
}
