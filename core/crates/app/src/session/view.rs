//! What UIs see of the session: domain and queue state turned into the
//! records of `crate::model`.

use ristle_domain::{Track, TrackRef};
use ristle_queue::{CanonicalQueue, QueueEntry};
use ristle_storage::repo::Intent;

use super::sequence::Active;
use crate::model::items::{
    ArtworkImage, Attribute, AttributeValue, Availability, ItemId, MediaDate, Named, TrackView,
};
use crate::model::session::{
    CoreSnapshot, PlaybackState, QueueEntryView, QueueWindow, RepeatMode, Transport,
};

/// Played entries a snapshot carries. Older history stays in the session.
pub(crate) const HISTORY_WINDOW: usize = 50;
/// Upcoming collection entries a snapshot carries. Queued Requests are
/// always all there.
pub(crate) const UPCOMING_WINDOW: usize = 100;

/// The facade's identity of a Track.
pub(crate) fn item_id(reference: &TrackRef) -> ItemId {
    ItemId {
        provider: reference.provider().as_str().to_owned(),
        id: reference.id().to_owned(),
        connection: reference.connection().map(|c| c.as_str().to_owned()),
    }
}

fn named(named: &ristle_domain::Named) -> Named {
    Named {
        id: named.id.clone(),
        name: named.name.clone(),
    }
}

fn media_date(date: &ristle_domain::MediaDate) -> MediaDate {
    MediaDate {
        year: date.year,
        month: date.month,
        day: date.day,
    }
}

/// A Track as UIs show it.
pub(crate) fn track_view(track: &Track) -> TrackView {
    TrackView {
        item: item_id(&track.reference),
        title: track.title.clone(),
        artists: track.artists.iter().map(named).collect(),
        album: track.album.as_ref().map(named),
        duration_ms: track.duration_ms,
        artwork: track
            .artwork
            .iter()
            .map(|artwork| ArtworkImage {
                url: artwork.url.clone(),
                width: artwork.width,
                height: artwork.height,
            })
            .collect(),
        explicit: track.explicit,
        live: track.capabilities.live,
        seekable: track.capabilities.seekable,
        availability: match &track.availability {
            ristle_domain::Availability::Available => Availability::Available,
            ristle_domain::Availability::Unavailable { reason } => Availability::Unavailable {
                reason: reason.clone(),
            },
            ristle_domain::Availability::Unknown => Availability::Unknown,
        },
        released: track.released.as_ref().map(media_date),
        uploaded: track.uploaded.as_ref().map(media_date),
        url: track.url.clone(),
        attributes: track
            .attributes
            .iter()
            .map(|attribute| Attribute {
                key: attribute.key.clone(),
                value: match &attribute.value {
                    ristle_domain::AttributeValue::Text(value) => AttributeValue::Text {
                        value: value.clone(),
                    },
                    ristle_domain::AttributeValue::Number(value) => {
                        AttributeValue::Number { value: *value }
                    }
                    ristle_domain::AttributeValue::YesNo(value) => {
                        AttributeValue::YesNo { value: *value }
                    }
                },
            })
            .collect(),
    }
}

fn entry_view(entry: &QueueEntry) -> QueueEntryView {
    QueueEntryView {
        entry: entry.id.0,
        track: track_view(&entry.track),
        submitter: entry
            .submitter
            .as_ref()
            .map(|submitter| submitter.display_name.clone()),
    }
}

pub(crate) fn repeat_mode(mode: ristle_queue::RepeatMode) -> RepeatMode {
    match mode {
        ristle_queue::RepeatMode::Off => RepeatMode::Off,
        ristle_queue::RepeatMode::Track => RepeatMode::Track,
        ristle_queue::RepeatMode::Queue => RepeatMode::Queue,
    }
}

pub(crate) fn queue_repeat(mode: RepeatMode) -> ristle_queue::RepeatMode {
    match mode {
        RepeatMode::Off => ristle_queue::RepeatMode::Off,
        RepeatMode::Track => ristle_queue::RepeatMode::Track,
        RepeatMode::Queue => ristle_queue::RepeatMode::Queue,
    }
}

/// The queue window: recent history, the current entry, every Queued
/// Request and the next part of the collection.
pub(crate) fn queue_window(queue: &CanonicalQueue) -> QueueWindow {
    let history = queue.history();
    let requests = queue.requests();
    let upcoming = queue.upcoming();
    // `upcoming` lists the requests first, then the collection.
    let continuation = &upcoming[requests.len().min(upcoming.len())..];
    QueueWindow {
        history: history[history.len().saturating_sub(HISTORY_WINDOW)..]
            .iter()
            .map(entry_view)
            .collect(),
        current: queue.current().map(entry_view),
        requests: requests.iter().map(entry_view).collect(),
        upcoming: continuation
            .iter()
            .take(UPCOMING_WINDOW)
            .map(|entry| entry_view(entry))
            .collect(),
        upcoming_total: Some(continuation.len() as u64),
        source: None,
    }
}

/// Fills `snapshot` from the session, keeping its revision and volume.
pub(crate) fn fill_snapshot(
    snapshot: &mut CoreSnapshot,
    queue: &CanonicalQueue,
    shuffled: bool,
    intent: Intent,
    position_ms: u64,
    active: Option<&Active>,
) {
    let current = queue.current();
    snapshot.transport = Transport {
        state: match (current, intent) {
            (None, _) => PlaybackState::Idle,
            (Some(_), Intent::Playing) => PlaybackState::Playing,
            (Some(_), Intent::Paused) => PlaybackState::Paused,
        },
        position_ms: if current.is_some() { position_ms } else { 0 },
        duration_ms: current.and_then(|entry| entry.track.duration_ms),
        seekable: current.is_some_and(|entry| entry.track.capabilities.seekable),
        volume: snapshot.transport.volume,
    };
    snapshot.queue = queue_window(queue);
    if let Some(active) = active {
        snapshot.queue.source = Some(active.source());
        // The whole continuation, not only what is materialized.
        snapshot.queue.upcoming_total = Some(active.remaining());
    }
    snapshot.repeat = repeat_mode(queue.repeat());
    snapshot.shuffle = shuffled;
}
