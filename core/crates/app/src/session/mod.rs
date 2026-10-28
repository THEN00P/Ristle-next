//! The Playback Session: the Canonical Queue, where the current Track
//! stands, and whether it plays, kept in memory and in `ristle.sqlite`
//! together.
//!
//! Every change runs as one mutation: it works on a copy, the copy is saved
//! in one storage transaction (with the settings and command receipt the
//! mutation wrote), and only then replaces the session in memory and is
//! published to observers. A mutation that fails, or whose save fails,
//! leaves memory, the database and the receipts as they were. Mutations run
//! one at a time.
//!
//! A cold start restores the saved session paused (CONTEXT.md, Playback
//! Session).

use std::sync::Arc;

use ristle_domain::Track;
use ristle_queue::sequence::SequenceId;
use ristle_queue::{
    Advance, CanonicalQueue, CollectionEntry, EntryId, QueueEntry, QueueState, Rejection, Removed,
    Submitter,
};
use ristle_storage::Storage;
use ristle_storage::repo::{Intent, PlaybackSession};
use tokio::sync::Mutex;

use crate::error::Error;
use crate::model::session::{CoreSnapshot, RepeatMode};
use crate::observe::Observers;
use crate::services::SeedSource;

pub(crate) mod view;

/// Previous restarts the Track instead once it has played this long.
pub const PREVIOUS_RESTARTS_AFTER_MS: u64 = 3_000;

/// The session's state.
struct State {
    queue: CanonicalQueue,
    shuffled: bool,
    sequence: Option<SequenceId>,
    position_ms: u64,
    intent: Intent,
    /// The latest removal, for `undo_removal`. Not persisted: undo is for
    /// the moment, not across restarts.
    removed: Option<Removed>,
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

/// A mutation's working copy of the session. Changes reach the session
/// only if the mutation returns `Ok` and its save succeeds.
pub struct Edit {
    queue: CanonicalQueue,
    sequence: Option<SequenceId>,
    position_ms: u64,
    intent: Intent,
    removed: Option<Removed>,
    shuffled: bool,
    settings: Vec<(String, String)>,
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
    /// can play at or after `start`. Fails, changing nothing, when none can.
    pub fn start_tracks(&mut self, tracks: Vec<Track>, start: usize) -> Result<(), Error> {
        let outcome = self
            .queue
            .start_collection(CollectionEntry::numbered(tracks, 0), start);
        if !outcome.started {
            return Err(Error::InvalidArgument {
                reason: "nothing at or after the start can play".to_owned(),
            });
        }
        self.sequence = None;
        self.removed = None;
        self.position_ms = 0;
        Ok(())
    }

    /// Writes a setting in the mutation's transaction.
    pub fn put_setting(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.settings.push((key.into(), value.into()));
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

/// The Playback Session.
pub struct Session {
    state: Mutex<State>,
    storage: Arc<Storage>,
    observers: Arc<Observers>,
    seeds: Arc<dyn SeedSource>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").finish_non_exhaustive()
    }
}

impl Session {
    /// Restores the saved session, paused. A session that cannot be read
    /// starts empty; the reason is logged. Blocks on storage, so it runs
    /// before the runtime takes commands.
    pub(crate) fn restore(storage: Arc<Storage>, seeds: Arc<dyn SeedSource>) -> Self {
        let saved = storage
            .read_blocking(|tx| tx.session().load_for_cold_start())
            .map_err(|error| tracing::error!(%error, "the saved Playback Session cannot be read"))
            .ok()
            .flatten();
        let mut state = State {
            queue: CanonicalQueue::new(),
            shuffled: false,
            sequence: None,
            position_ms: 0,
            intent: Intent::Paused,
            removed: None,
        };
        if let Some(saved) = saved {
            let shuffled = saved.session.queue.shuffle.is_some();
            match CanonicalQueue::restore(saved.session.queue) {
                Ok(queue) => {
                    state.queue = queue;
                    state.shuffled = shuffled;
                    state.sequence = saved.session.sequence;
                    state.position_ms = saved.session.position_ms;
                    // `load_for_cold_start` always restores paused.
                    state.intent = saved.session.intent;
                }
                Err(error) => {
                    tracing::error!(%error, "the saved Canonical Queue is corrupt; starting empty");
                }
            }
        }
        let mut snapshot = CoreSnapshot::empty();
        view::fill_snapshot(
            &mut snapshot,
            &state.queue,
            state.shuffled,
            state.intent,
            state.position_ms,
        );
        Self {
            state: Mutex::new(state),
            storage,
            observers: Arc::new(Observers::new(snapshot)),
            seeds,
        }
    }

    pub(crate) fn observers(&self) -> &Arc<Observers> {
        &self.observers
    }

    /// Runs one mutation; see the module documentation.
    async fn mutate<T>(
        &self,
        change: impl FnOnce(&mut Edit) -> Result<T, Error>,
    ) -> Result<T, Error> {
        // Without a receipt there is nothing to skip.
        self.mutate_once(None, change)
            .await?
            .ok_or_else(|| Error::Internal {
                reason: "a mutation without a receipt was skipped".to_owned(),
            })
    }

    /// Runs one mutation, unless `receipt` was recorded before (`None`).
    async fn mutate_once<T>(
        &self,
        receipt: Option<Receipt>,
        change: impl FnOnce(&mut Edit) -> Result<T, Error>,
    ) -> Result<Option<T>, Error> {
        let mut state = self.state.lock().await;
        // Checked under the session lock, so no other mutation can record
        // the same receipt between the check and the save.
        if let Some(receipt) = &receipt
            && self.receipt(&receipt.scope, &receipt.key).await?.is_some()
        {
            return Ok(None);
        }
        let mut edit = Edit {
            queue: state.queue.clone(),
            sequence: state.sequence.clone(),
            position_ms: state.position_ms,
            intent: state.intent,
            removed: state.removed.clone(),
            shuffled: state.shuffled,
            settings: Vec::new(),
        };
        let value = change(&mut edit)?;

        let queue: QueueState = edit.queue.snapshot();
        let shuffled = queue.shuffle.is_some();
        let session = PlaybackSession {
            queue,
            sequence: edit.sequence.clone(),
            position_ms: edit.position_ms,
            intent: edit.intent,
        };
        let settings = std::mem::take(&mut edit.settings);
        self.storage
            .write(move |tx| {
                tx.session().save(&session)?;
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
            shuffled,
            sequence: edit.sequence,
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
                state.shuffled,
                state.intent,
                state.position_ms,
            );
        });
        Ok(Some(value))
    }

    /// Applies a command at most once: if `receipt`'s scope and key were
    /// recorded before, nothing happens and `None` is returned. Otherwise
    /// the mutation and the receipt are saved together, so a command is
    /// never applied without its receipt or recorded without its effect
    /// (legacy `PlaybackCommandReceiptSurvivesRestart...`).
    pub async fn apply_once<T>(
        &self,
        receipt: Receipt,
        change: impl FnOnce(&mut Edit) -> Result<T, Error>,
    ) -> Result<Option<T>, Error> {
        self.mutate_once(Some(receipt), change).await
    }

    /// The outcome recorded for a command, if any.
    pub async fn receipt(&self, scope: &str, key: &str) -> Result<Option<String>, Error> {
        let (scope, key) = (scope.to_owned(), key.to_owned());
        self.storage
            .read(move |tx| tx.receipts().get(&scope, &key))
            .await
            .map_err(storage_error)
    }

    /// Forgets every receipt of a scope, such as a Party that ended.
    /// Returns how many there were.
    pub async fn clear_receipts(&self, scope: &str) -> Result<u64, Error> {
        let scope = scope.to_owned();
        self.storage
            .write(move |tx| tx.receipts().clear_scope(&scope))
            .await
            .map_err(storage_error)
    }

    /// A setting's value.
    pub async fn setting(&self, key: &str) -> Result<Option<String>, Error> {
        let key = key.to_owned();
        self.storage
            .read(move |tx| tx.settings().get(&key))
            .await
            .map_err(storage_error)
    }

    /// Writes a setting outside any mutation.
    pub async fn put_setting(&self, key: &str, value: &str) -> Result<(), Error> {
        let (key, value) = (key.to_owned(), value.to_owned());
        self.storage
            .write(move |tx| tx.settings().put(&key, &value))
            .await
            .map_err(storage_error)
    }

    /// The Canonical Queue as it is now.
    pub async fn queue(&self) -> QueueState {
        self.state.lock().await.queue.snapshot()
    }

    /// Where the current Track stands, and whether it should play.
    pub async fn transport(&self) -> (Intent, u64) {
        let state = self.state.lock().await;
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

    /// Removes everything upcoming. Returns how many entries went.
    pub async fn clear_upcoming(&self) -> Result<usize, Error> {
        self.mutate(|edit| {
            edit.removed = None;
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

    /// Shuffles the upcoming collection with a new seed, or restores its
    /// order. Turning on shuffle when it is on keeps the order.
    pub async fn set_shuffle(&self, enabled: bool) -> Result<(), Error> {
        let seed = self.seeds.next_seed();
        self.mutate(move |edit| {
            match (enabled, edit.shuffled) {
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
        let mut queue = self.state.lock().await.queue.clone();
        queue
            .advance(how)
            .then(|| queue.current().cloned())
            .flatten()
    }

    /// The entry going back would play, without changing the session.
    pub async fn peek_previous(&self) -> Option<QueueEntry> {
        let mut queue = self.state.lock().await.queue.clone();
        queue.go_back().then(|| queue.current().cloned()).flatten()
    }
}
