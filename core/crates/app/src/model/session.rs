//! The Playback Session as UIs see it, and the commands that change it.

use super::items::{ItemRef, TrackView};
use super::pages::CollectionRef;

/// Where the core keeps its files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreConfig {
    /// Directory for `ristle.sqlite` and installed Plugin Packages. Backed up
    /// with the app's data.
    pub data_dir: String,
    /// Directory for `cache.sqlite` and compiled Plugins. The system may
    /// clear it.
    pub cache_dir: String,
}

/// What the player is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlaybackState {
    /// Nothing is loaded.
    Idle,
    /// Playback is wanted but waiting for audio: opening a Track or
    /// refilling the buffer.
    Buffering,
    /// Audio is playing.
    Playing,
    /// Paused by the user, by a transient failure, or after a cold start.
    Paused,
}

/// Repeat state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RepeatMode {
    /// Play to the end and stop.
    Off,
    /// Replay the current Track when it ends. Next still moves on.
    Track,
    /// Start the queue again after the last Track.
    Queue,
}

/// The transport: state and playhead.
#[derive(Debug, Clone, PartialEq)]
pub struct Transport {
    /// What the player is doing.
    pub state: PlaybackState,
    /// The playhead when the snapshot was published. While `state` is
    /// `Playing`, UIs advance it themselves from the moment the snapshot
    /// arrived; the core publishes again only when the transport changes.
    pub position_ms: u64,
    /// The current Track's duration, when known.
    pub duration_ms: Option<u64>,
    /// Whether `seek` is possible now.
    pub seekable: bool,
    /// Output volume, 0.0 to 1.0.
    pub volume: f32,
}

/// One entry of the Canonical Queue.
#[derive(Debug, Clone, PartialEq)]
pub struct QueueEntryView {
    /// Stable for the entry's life, and the id queue commands take. Two
    /// entries of the same Track have different ids.
    pub entry: u64,
    /// The Track.
    pub track: TrackView,
    /// Who asked for it, in a Ristle Party.
    pub submitter: Option<String>,
}

/// Where the queue's continuation comes from, for display and navigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueSource {
    /// The title, such as an album or playlist name, or "Liked Songs".
    pub title: String,
    /// What the user opened to start it, when it can be opened again.
    pub opened: Option<ItemRef>,
}

/// The part of the Canonical Queue UIs show. The core decides how much
/// history and lookahead fit; the rest of a long collection is not
/// materialized.
#[derive(Debug, Clone, PartialEq)]
pub struct QueueWindow {
    /// The latest played entries, oldest first.
    pub history: Vec<QueueEntryView>,
    /// The current entry.
    pub current: Option<QueueEntryView>,
    /// Queued Requests, in the order they will play.
    pub requests: Vec<QueueEntryView>,
    /// The collection's continuation after the requests.
    pub upcoming: Vec<QueueEntryView>,
    /// The length of the whole continuation, including the entries in
    /// `upcoming`, when known.
    pub upcoming_total: Option<u64>,
    /// Where the continuation comes from.
    pub source: Option<QueueSource>,
}

/// Everything a now-playing screen, a queue screen and a platform media
/// session render. Published whole, with a revision that only grows.
#[derive(Debug, Clone, PartialEq)]
pub struct CoreSnapshot {
    /// Increases with every published snapshot. A UI drops a snapshot whose
    /// revision is not higher than the last one it applied.
    pub revision: u64,
    /// State and playhead.
    pub transport: Transport,
    /// The queue.
    pub queue: QueueWindow,
    /// Repeat state.
    pub repeat: RepeatMode,
    /// Whether the continuation is shuffled.
    pub shuffle: bool,
}

impl CoreSnapshot {
    /// The state of a core with nothing loaded.
    pub(crate) fn empty() -> Self {
        Self {
            revision: 0,
            transport: Transport {
                state: PlaybackState::Idle,
                position_ms: 0,
                duration_ms: None,
                seekable: false,
                volume: 1.0,
            },
            queue: QueueWindow {
                history: Vec::new(),
                current: None,
                requests: Vec::new(),
                upcoming: Vec::new(),
                upcoming_total: None,
                source: None,
            },
            repeat: RepeatMode::Off,
            shuffle: false,
        }
    }
}

/// Where `add_to_queue` puts items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Placement {
    /// Right after the current Track, before other Queued Requests.
    Next,
    /// After the other Queued Requests (Add to queue).
    Last,
}

/// What to start playing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaySource {
    /// Items in the given order: an album, a playlist, an artist's
    /// discography, or a selection of Tracks.
    Items {
        /// The items.
        items: Vec<ItemRef>,
    },
    /// A Provider collection, such as a library shelf.
    Collection {
        /// The collection.
        collection: CollectionRef,
    },
    /// Liked Songs across every Provider Connection.
    LikedTracks,
    /// The Tracks of a Unified Search snapshot, in its order.
    SearchResults {
        /// The snapshot's id.
        search: u64,
    },
}

/// Where in a [`PlaySource`] to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartAt {
    /// The first entry, or a random one when shuffling.
    Beginning,
    /// The entry at this position, counted from zero in source order. With
    /// shuffle on, it plays first and the rest is shuffled.
    Position {
        /// The position.
        position: u64,
    },
}

/// A command from a platform media session: lock screen, notification,
/// headset, Android Auto or CarPlay.
#[derive(Debug, Clone, PartialEq)]
pub enum MediaCommand {
    /// Play.
    Play,
    /// Pause.
    Pause,
    /// Play when paused, pause when playing.
    PlayPause,
    /// Stop and release audio focus. The session stays.
    Stop,
    /// The next Track.
    Next,
    /// Restart the Track, or the previous one near its start.
    Previous,
    /// Move the playhead.
    SeekTo {
        /// The new position.
        position_ms: u64,
    },
    /// Move the playhead by an offset.
    SeekBy {
        /// Milliseconds forward, or back when negative.
        offset_ms: i64,
    },
    /// Change repeat.
    SetRepeat {
        /// The mode.
        mode: RepeatMode,
    },
    /// Turn shuffle on or off.
    SetShuffle {
        /// Whether to shuffle.
        enabled: bool,
    },
    /// Like or unlike the current Track.
    SetCurrentLiked {
        /// Whether to like it.
        liked: bool,
    },
    /// Jump to a queue entry the session listed.
    JumpTo {
        /// The entry.
        entry: u64,
    },
}

/// A message above the seek bar when playback is delayed or interrupted.
/// Never a dialog; the transport stays usable.
#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackNotice {
    /// Audio is taking longer than expected to start.
    Delayed {
        /// The Track that is loading.
        track: TrackView,
    },
    /// A Track could not play and was skipped.
    Skipped {
        /// The Track.
        track: TrackView,
        /// Why, when known.
        reason: Option<String>,
    },
    /// Playback stopped on a Track after a transient failure, such as the
    /// network. Play retries.
    Stopped {
        /// The Track.
        track: TrackView,
        /// Why, when known.
        reason: Option<String>,
    },
    /// A Provider Connection must sign in again before its Tracks play.
    SignInNeeded {
        /// The connection.
        connection: String,
    },
    /// The previous notice no longer applies.
    Cleared,
}

/// Something outside the Playback Session changed; UIs reload what shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryChange {
    /// Ristle Playlists were created, renamed, deleted or edited.
    RistlePlaylists,
    /// A like changed, here or on another device.
    Liked {
        /// The item.
        item: ItemRef,
        /// Whether it is liked now.
        liked: bool,
    },
    /// Recently Played changed.
    RecentlyPlayed,
    /// Plugins or Provider Connections changed.
    Plugins,
    /// A Provider Connection's library was read again.
    LibraryView {
        /// The connection.
        connection: String,
    },
}
