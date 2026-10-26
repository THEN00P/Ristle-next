-- ristle.sqlite, schema version 1: state the user cannot recreate.
--
-- Frozen since the fixture `tests/fixtures/schema-v1.sql` was committed.
-- Change the schema only by adding a migration (src/migrate.rs); the
-- fixture test fails if this file changes.
--
-- Conventions: every table is STRICT. Times are milliseconds since the Unix
-- epoch. Tracks are stored as `ristle_domain::Track` JSON in a `track`
-- column, with their identity repeated in `provider_id`, `track_id` and
-- `connection_id` for queries. Seeds are u64 values stored bit for bit as
-- INTEGER, so they may read negative in SQL.

-- Key-value settings of the app.
CREATE TABLE settings (
  key TEXT PRIMARY KEY NOT NULL,
  value TEXT NOT NULL
) STRICT, WITHOUT ROWID;

-- Playable sequences (`ristle_queue::SequenceState`). The descriptor is
-- written once; routes, slots and progress are rewritten only where they
-- changed.
CREATE TABLE sequences (
  sequence_id TEXT PRIMARY KEY NOT NULL,
  title TEXT NOT NULL,
  -- `ristle_queue::sequence::Opened` as JSON. Display only.
  opened TEXT NOT NULL,
  next_slot INTEGER NOT NULL CHECK (next_slot >= 0)
) STRICT, WITHOUT ROWID;

-- A sequence's sources, stored once and named by index from every slot
-- (legacy trap 9). kind: 0 Provider collection, 1 Ristle Playlist, 2 fixed.
CREATE TABLE sequence_routes (
  sequence_id TEXT NOT NULL REFERENCES sequences (sequence_id) ON DELETE CASCADE,
  source_index INTEGER NOT NULL CHECK (source_index >= 0),
  kind INTEGER NOT NULL CHECK (kind IN (0, 1, 2)),
  connection_id TEXT,
  collection_id TEXT,
  -- 0 cursor, 1 offset.
  addressing INTEGER CHECK (addressing IN (0, 1)),
  total INTEGER CHECK (total >= 0),
  total_exact INTEGER CHECK (total_exact IN (0, 1)),
  revision TEXT,
  endless INTEGER CHECK (endless IN (0, 1)),
  playlist_id TEXT,
  PRIMARY KEY (sequence_id, source_index),
  CHECK ((kind = 0) = (connection_id IS NOT NULL AND collection_id IS NOT NULL
                       AND addressing IS NOT NULL AND endless IS NOT NULL)),
  CHECK ((kind = 1) = (playlist_id IS NOT NULL)),
  CHECK ((total IS NULL) = (total_exact IS NULL))
) STRICT, WITHOUT ROWID;

-- One row per occurrence. `natural_index` is its place in natural order,
-- `play_index` its place in the play order.
CREATE TABLE sequence_slots (
  sequence_id TEXT NOT NULL REFERENCES sequences (sequence_id) ON DELETE CASCADE,
  slot_id INTEGER NOT NULL CHECK (slot_id >= 0),
  natural_index INTEGER NOT NULL CHECK (natural_index >= 0),
  play_index INTEGER NOT NULL CHECK (play_index >= 0),
  source_index INTEGER NOT NULL,
  source_offset INTEGER NOT NULL CHECK (source_offset >= 0),
  -- NULL while the slot is only a position.
  provider_id TEXT,
  track_id TEXT,
  connection_id TEXT,
  PRIMARY KEY (sequence_id, slot_id),
  FOREIGN KEY (sequence_id, source_index)
    REFERENCES sequence_routes (sequence_id, source_index) DEFERRABLE INITIALLY DEFERRED,
  CHECK ((provider_id IS NULL) = (track_id IS NULL)),
  CHECK (connection_id IS NULL OR track_id IS NOT NULL)
) STRICT, WITHOUT ROWID;

-- Where a sequence stands. Rewritten on every advance, alone.
CREATE TABLE sequence_progress (
  sequence_id TEXT PRIMARY KEY NOT NULL REFERENCES sequences (sequence_id) ON DELETE CASCADE,
  cursor INTEGER CHECK (cursor >= 0),
  materialized_end INTEGER NOT NULL CHECK (materialized_end >= 0),
  shuffle_seed INTEGER,
  revision INTEGER NOT NULL CHECK (revision >= 0)
) STRICT, WITHOUT ROWID;

-- The Playback Session: the Canonical Queue's scalars, the active sequence
-- and the transport. One row at most.
CREATE TABLE playback_session (
  id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
  cursor INTEGER CHECK (cursor >= 0),
  play_next_count INTEGER NOT NULL CHECK (play_next_count >= 0),
  -- 0 nothing, 1 the collection entry at the cursor, 2 a detached request.
  current INTEGER NOT NULL CHECK (current IN (0, 1, 2)),
  -- 0 off, 1 Track, 2 queue.
  repeat INTEGER NOT NULL CHECK (repeat IN (0, 1, 2)),
  shuffle_seed INTEGER,
  next_entry_id INTEGER NOT NULL CHECK (next_entry_id >= 0),
  sequence_id TEXT REFERENCES sequences (sequence_id) DEFERRABLE INITIALLY DEFERRED,
  position_ms INTEGER NOT NULL CHECK (position_ms >= 0),
  -- 0 paused, 1 playing.
  intent INTEGER NOT NULL CHECK (intent IN (0, 1)),
  updated_ms INTEGER NOT NULL
) STRICT;

-- Canonical Queue entries. lane: 0 collection, 1 request lane, 2 the
-- detached current request. origin: 0 collection, 1 request. Every
-- collection entry keeps its `collection_position` (legacy trap 6).
CREATE TABLE queue_entries (
  entry_id INTEGER PRIMARY KEY NOT NULL CHECK (entry_id >= 0),
  lane INTEGER NOT NULL CHECK (lane IN (0, 1, 2)),
  position INTEGER NOT NULL CHECK (position >= 0),
  origin INTEGER NOT NULL CHECK (origin IN (0, 1)),
  collection_position INTEGER CHECK (collection_position >= 0),
  slot_id INTEGER CHECK (slot_id >= 0),
  submitter_id TEXT,
  submitter_name TEXT,
  provider_id TEXT NOT NULL,
  track_id TEXT NOT NULL,
  connection_id TEXT,
  track TEXT NOT NULL,
  CHECK ((origin = 0) = (collection_position IS NOT NULL)),
  CHECK ((submitter_id IS NULL) = (submitter_name IS NULL))
) STRICT;

CREATE TABLE ristle_playlists (
  playlist_id TEXT PRIMARY KEY NOT NULL,
  title TEXT NOT NULL,
  created_ms INTEGER NOT NULL,
  updated_ms INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

-- Duplicates and Disabled Tracks are kept, with their metadata.
CREATE TABLE ristle_playlist_entries (
  playlist_id TEXT NOT NULL REFERENCES ristle_playlists (playlist_id) ON DELETE CASCADE,
  position INTEGER NOT NULL CHECK (position >= 0),
  provider_id TEXT NOT NULL,
  track_id TEXT NOT NULL,
  connection_id TEXT,
  track TEXT NOT NULL,
  PRIMARY KEY (playlist_id, position)
) STRICT;

CREATE INDEX ristle_playlist_entries_track ON ristle_playlist_entries (provider_id, track_id);

CREATE TABLE recently_played (
  entry_id INTEGER PRIMARY KEY NOT NULL,
  provider_id TEXT NOT NULL,
  track_id TEXT NOT NULL,
  connection_id TEXT,
  track TEXT NOT NULL,
  played_ms INTEGER NOT NULL
) STRICT;

CREATE INDEX recently_played_latest ON recently_played (played_ms DESC, entry_id DESC);

-- Installed Plugins.
CREATE TABLE plugins (
  plugin_id TEXT PRIMARY KEY NOT NULL,
  version TEXT NOT NULL,
  package_hash TEXT NOT NULL,
  publisher_key TEXT NOT NULL,
  enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
  installed_ms INTEGER NOT NULL,
  updated_ms INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

-- Provider Connections. The Credential Record lives in the platform's
-- secret store; this is its metadata.
CREATE TABLE provider_connections (
  connection_id TEXT PRIMARY KEY NOT NULL,
  plugin_id TEXT NOT NULL REFERENCES plugins (plugin_id) ON DELETE CASCADE,
  generation INTEGER NOT NULL CHECK (generation >= 0),
  -- `Account` as JSON, after the first accepted authorization.
  account TEXT,
  created_ms INTEGER NOT NULL,
  updated_ms INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

CREATE INDEX provider_connections_plugin ON provider_connections (plugin_id);

-- kind: 0 Plugin Permission (Plugin-wide), 1 Folder Grant, 2 Server Grant
-- (both per Provider Connection, with JSON `detail`).
CREATE TABLE plugin_grants (
  plugin_id TEXT NOT NULL REFERENCES plugins (plugin_id) ON DELETE CASCADE,
  connection_id TEXT REFERENCES provider_connections (connection_id) ON DELETE CASCADE,
  kind INTEGER NOT NULL CHECK (kind IN (0, 1, 2)),
  position INTEGER NOT NULL CHECK (position >= 0),
  grant_key TEXT NOT NULL,
  detail TEXT,
  CHECK ((kind = 0) = (connection_id IS NULL)),
  CHECK ((kind = 0) = (detail IS NULL))
) STRICT;

CREATE UNIQUE INDEX plugin_grants_permission ON plugin_grants (plugin_id, grant_key) WHERE kind = 0;
CREATE UNIQUE INDEX plugin_grants_connection ON plugin_grants (connection_id, kind, grant_key) WHERE kind <> 0;

-- The WIT `connection.set-data`, deleted with every generation change.
CREATE TABLE connection_data (
  connection_id TEXT NOT NULL REFERENCES provider_connections (connection_id) ON DELETE CASCADE,
  key TEXT NOT NULL,
  value BLOB NOT NULL,
  expires_ms INTEGER,
  PRIMARY KEY (connection_id, key)
) STRICT;

-- The WIT `storage` interface, shared by all of a Plugin's connections.
CREATE TABLE plugin_kv (
  plugin_id TEXT NOT NULL REFERENCES plugins (plugin_id) ON DELETE CASCADE,
  key TEXT NOT NULL,
  value BLOB NOT NULL,
  expires_ms INTEGER,
  PRIMARY KEY (plugin_id, key)
) STRICT;

-- The WIT `response.sequence`: never goes back for a connection, so rows
-- outlive the connection and its Plugin, and this is not cache.
CREATE TABLE response_sequences (
  plugin_id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  reserved INTEGER NOT NULL CHECK (reserved >= 0),
  PRIMARY KEY (plugin_id, connection_id)
) STRICT, WITHOUT ROWID;

-- The highest generation each Provider Connection id has reached. Outlives
-- the connection, so an id used again continues above it, and nothing cached
-- under the previous connection's generations is ever read for the new one.
CREATE TABLE connection_generations (
  connection_id TEXT PRIMARY KEY NOT NULL,
  generation INTEGER NOT NULL CHECK (generation >= 0)
) STRICT, WITHOUT ROWID;

-- Outcomes of commands that must apply once, such as Party commands.
CREATE TABLE command_receipts (
  scope TEXT NOT NULL,
  key TEXT NOT NULL,
  outcome TEXT NOT NULL,
  created_ms INTEGER NOT NULL,
  PRIMARY KEY (scope, key)
) STRICT, WITHOUT ROWID;

-- Local Tracks: audio content found under a Folder Grant, grouped by
-- fingerprint.
CREATE TABLE local_tracks (
  connection_id TEXT NOT NULL REFERENCES provider_connections (connection_id) ON DELETE CASCADE,
  content_id TEXT NOT NULL,
  fingerprint TEXT NOT NULL,
  fingerprint_hash INTEGER NOT NULL,
  duration_ms INTEGER CHECK (duration_ms >= 0),
  indexed_ms INTEGER NOT NULL,
  PRIMARY KEY (connection_id, content_id)
) STRICT;

CREATE INDEX local_tracks_fingerprint ON local_tracks (fingerprint_hash, duration_ms);

-- Every file of a Local Track, each a hidden playback candidate, with the
-- metadata read from it. The Local Track shows its preferred file's.
CREATE TABLE local_files (
  connection_id TEXT NOT NULL,
  location TEXT NOT NULL,
  content_id TEXT NOT NULL,
  format TEXT NOT NULL,
  bitrate_kbps INTEGER NOT NULL CHECK (bitrate_kbps >= 0),
  size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
  modified_ms INTEGER NOT NULL,
  available INTEGER NOT NULL CHECK (available IN (0, 1)),
  track TEXT NOT NULL,
  PRIMARY KEY (connection_id, location),
  FOREIGN KEY (connection_id, content_id)
    REFERENCES local_tracks (connection_id, content_id) ON DELETE CASCADE
) STRICT;

CREATE INDEX local_files_track ON local_files (connection_id, content_id);
