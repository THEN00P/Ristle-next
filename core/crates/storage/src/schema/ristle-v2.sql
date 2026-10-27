-- ristle.sqlite, migration 2: the collection entries `trim_history` dropped
-- from the current pass (`ristle_queue::QueueState::trimmed`), so that
-- reconciling a shuffled pass after a restart still counts them as played.
-- Frozen once merged, like every migration.

-- One row per trimmed entry, in list order. Saved with the Playback Session.
CREATE TABLE queue_trimmed (
  trim_index INTEGER PRIMARY KEY NOT NULL CHECK (trim_index >= 0),
  collection_position INTEGER NOT NULL CHECK (collection_position >= 0),
  provider_id TEXT NOT NULL,
  track_id TEXT NOT NULL,
  connection_id TEXT
) STRICT;
