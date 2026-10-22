-- ristle.sqlite at schema v1, written by `write_schema_v1_fixture`.
-- Never regenerate it: it stands for the databases users have.
CREATE TABLE command_receipts (
  scope TEXT NOT NULL,
  key TEXT NOT NULL,
  outcome TEXT NOT NULL,
  created_ms INTEGER NOT NULL,
  PRIMARY KEY (scope, key)
) STRICT, WITHOUT ROWID;
CREATE TABLE connection_data (
  connection_id TEXT NOT NULL REFERENCES provider_connections (connection_id) ON DELETE CASCADE,
  key TEXT NOT NULL,
  value BLOB NOT NULL,
  expires_ms INTEGER,
  PRIMARY KEY (connection_id, key)
) STRICT;
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
CREATE TABLE local_tracks (
  connection_id TEXT NOT NULL REFERENCES provider_connections (connection_id) ON DELETE CASCADE,
  content_id TEXT NOT NULL,
  fingerprint TEXT NOT NULL,
  fingerprint_hash INTEGER NOT NULL,
  duration_ms INTEGER CHECK (duration_ms >= 0),
  indexed_ms INTEGER NOT NULL,
  PRIMARY KEY (connection_id, content_id)
) STRICT;
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
CREATE TABLE plugin_kv (
  plugin_id TEXT NOT NULL REFERENCES plugins (plugin_id) ON DELETE CASCADE,
  key TEXT NOT NULL,
  value BLOB NOT NULL,
  expires_ms INTEGER,
  PRIMARY KEY (plugin_id, key)
) STRICT;
CREATE TABLE plugins (
  plugin_id TEXT PRIMARY KEY NOT NULL,
  version TEXT NOT NULL,
  package_hash TEXT NOT NULL,
  publisher_key TEXT NOT NULL,
  enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
  installed_ms INTEGER NOT NULL,
  updated_ms INTEGER NOT NULL
) STRICT, WITHOUT ROWID;
CREATE TABLE provider_connections (
  connection_id TEXT PRIMARY KEY NOT NULL,
  plugin_id TEXT NOT NULL REFERENCES plugins (plugin_id) ON DELETE CASCADE,
  generation INTEGER NOT NULL CHECK (generation >= 0),
  -- `Account` as JSON, after the first accepted authorization.
  account TEXT,
  created_ms INTEGER NOT NULL,
  updated_ms INTEGER NOT NULL
) STRICT, WITHOUT ROWID;
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
CREATE TABLE recently_played (
  entry_id INTEGER PRIMARY KEY NOT NULL,
  provider_id TEXT NOT NULL,
  track_id TEXT NOT NULL,
  connection_id TEXT,
  track TEXT NOT NULL,
  played_ms INTEGER NOT NULL
) STRICT;
CREATE TABLE response_sequences (
  plugin_id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  reserved INTEGER NOT NULL CHECK (reserved >= 0),
  PRIMARY KEY (plugin_id, connection_id)
) STRICT, WITHOUT ROWID;
CREATE TABLE ristle_playlist_entries (
  playlist_id TEXT NOT NULL REFERENCES ristle_playlists (playlist_id) ON DELETE CASCADE,
  position INTEGER NOT NULL CHECK (position >= 0),
  provider_id TEXT NOT NULL,
  track_id TEXT NOT NULL,
  connection_id TEXT,
  track TEXT NOT NULL,
  PRIMARY KEY (playlist_id, position)
) STRICT;
CREATE TABLE ristle_playlists (
  playlist_id TEXT PRIMARY KEY NOT NULL,
  title TEXT NOT NULL,
  created_ms INTEGER NOT NULL,
  updated_ms INTEGER NOT NULL
) STRICT, WITHOUT ROWID;
CREATE TABLE sequence_progress (
  sequence_id TEXT PRIMARY KEY NOT NULL REFERENCES sequences (sequence_id) ON DELETE CASCADE,
  cursor INTEGER CHECK (cursor >= 0),
  materialized_end INTEGER NOT NULL CHECK (materialized_end >= 0),
  shuffle_seed INTEGER,
  revision INTEGER NOT NULL CHECK (revision >= 0)
) STRICT, WITHOUT ROWID;
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
CREATE TABLE sequences (
  sequence_id TEXT PRIMARY KEY NOT NULL,
  title TEXT NOT NULL,
  -- `ristle_queue::sequence::Opened` as JSON. Display only.
  opened TEXT NOT NULL,
  next_slot INTEGER NOT NULL CHECK (next_slot >= 0)
) STRICT, WITHOUT ROWID;
CREATE TABLE settings (
  key TEXT PRIMARY KEY NOT NULL,
  value TEXT NOT NULL
) STRICT, WITHOUT ROWID;
CREATE INDEX local_files_track ON local_files (connection_id, content_id);
CREATE INDEX local_tracks_fingerprint ON local_tracks (fingerprint_hash, duration_ms);
CREATE UNIQUE INDEX plugin_grants_connection ON plugin_grants (connection_id, kind, grant_key) WHERE kind <> 0;
CREATE UNIQUE INDEX plugin_grants_permission ON plugin_grants (plugin_id, grant_key) WHERE kind = 0;
CREATE INDEX provider_connections_plugin ON provider_connections (plugin_id);
CREATE INDEX recently_played_latest ON recently_played (played_ms DESC, entry_id DESC);
CREATE INDEX ristle_playlist_entries_track ON ristle_playlist_entries (provider_id, track_id);
INSERT INTO command_receipts VALUES ('party-1', 'member-1:1', 'applied', 1760000000000);
INSERT INTO command_receipts VALUES ('party-1', 'member-2:1', 'rejected: unavailable', 1760000000000);
INSERT INTO connection_data VALUES ('spotify-1', 'user', X'009FFF', 1760003600000);
INSERT INTO local_files VALUES ('local-1', 'f1/a.flac', 'content-1', 'flac', 900, 20000000, 1759999999991, 1, '{"reference":{"provider":"local-files","id":"content-1"},"title":"Title content-1","artists":[{"id":"a1","name":"Artist One"},{"id":null,"name":"Guest"}],"album":{"id":"al1","name":"Album"},"duration_ms":215000,"artwork":[{"url":"https://img.example/1.jpg","width":300,"height":null}],"released":{"year":1999,"month":12,"day":31},"uploaded":{"year":2001},"explicit":true,"capabilities":{"seekable":false,"live":false},"availability":"Unknown","url":"https://local-files.example/track/content-1","isrc":"GBAYE0000001","attributes":[{"key":"genre","value":{"Text":"ambient"}},{"key":"bpm","value":{"Number":92.5}},{"key":"soundcloud:reposted","value":{"YesNo":false}}]}');
INSERT INTO local_files VALUES ('local-1', 'f1/a.mp3', 'content-1', 'mp3', 320, 7000000, 1759999999992, 0, '{"reference":{"provider":"local-files","id":"content-1"},"title":"Title content-1","artists":[{"id":"a1","name":"Artist One"},{"id":null,"name":"Guest"}],"album":{"id":"al1","name":"Album"},"duration_ms":215000,"artwork":[{"url":"https://img.example/1.jpg","width":300,"height":null}],"released":{"year":1999,"month":12,"day":31},"uploaded":{"year":2001},"explicit":true,"capabilities":{"seekable":false,"live":false},"availability":"Unknown","url":"https://local-files.example/track/content-1","isrc":"GBAYE0000001","attributes":[{"key":"genre","value":{"Text":"ambient"}},{"key":"bpm","value":{"Number":92.5}},{"key":"soundcloud:reposted","value":{"YesNo":false}}]}');
INSERT INTO local_tracks VALUES ('local-1', 'content-1', 'AQAAE0mUaEkSRZEGAA', 4276993775, 180000, 1760000000000);
INSERT INTO playback_session VALUES (1, 2, 1, 2, 2, -2, 9, 'liked:spotify-1+soundcloud-1', 61000, 1, 1760000000000);
INSERT INTO plugin_grants VALUES ('local-files', 'local-1', 1, 0, 'f1', '{"id":"f1","name":"Music","locator":"/home/ada/Music"}');
INSERT INTO plugin_grants VALUES ('spotify', NULL, 0, 0, 'credentials', NULL);
INSERT INTO plugin_grants VALUES ('spotify', NULL, 0, 1, 'hosts:api.spotify.com', NULL);
INSERT INTO plugin_grants VALUES ('spotify', 'spotify-1', 2, 0, 'server', '{"name":"Living Room","urls":["http://192.168.1.20:8096","https://music.example/jellyfin"],"confirmed":[{"origin":"http://192.168.1.20:8096","public":false,"addresses":["192.168.1.20"]},{"origin":"https://music.example","public":true,"addresses":[]}]}');
INSERT INTO plugin_kv VALUES ('spotify', 'client-id', X'616263', NULL);
INSERT INTO plugins VALUES ('local-files', '0.3.1', 'sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08', 'ed25519:3fa1', 1, 1759999999000, 1759999999500);
INSERT INTO plugins VALUES ('spotify', '0.3.1', 'sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08', 'ed25519:3fa1', 1, 1759999999000, 1759999999500);
INSERT INTO provider_connections VALUES ('local-1', 'local-files', 0, NULL, 1760000000000, 1760000000000);
INSERT INTO provider_connections VALUES ('spotify-1', 'spotify', 1, '{"id":"user-1","name":"Ada L.","avatar":[{"url":"https://img.example/a.jpg","width":64,"height":64}]}', 1760000000000, 1760000000000);
INSERT INTO queue_entries VALUES (1, 0, 0, 0, 0, 0, NULL, NULL, 'spotify', 's1', NULL, '{"reference":{"provider":"spotify","id":"s1"},"title":"Title s1","artists":[{"id":"a1","name":"Artist One"},{"id":null,"name":"Guest"}],"album":{"id":"al1","name":"Album"},"duration_ms":215000,"artwork":[{"url":"https://img.example/1.jpg","width":300,"height":null}],"released":{"year":1999,"month":12,"day":31},"uploaded":{"year":2001},"explicit":true,"capabilities":{"seekable":false,"live":false},"availability":"Available","url":"https://spotify.example/track/s1","isrc":"GBAYE0000001","attributes":[{"key":"genre","value":{"Text":"ambient"}},{"key":"bpm","value":{"Number":92.5}},{"key":"soundcloud:reposted","value":{"YesNo":false}}]}');
INSERT INTO queue_entries VALUES (2, 0, 2, 0, 3, 1, NULL, NULL, 'spotify', 's1', NULL, '{"reference":{"provider":"spotify","id":"s1"},"title":"Title s1","artists":[{"id":"a1","name":"Artist One"},{"id":null,"name":"Guest"}],"album":{"id":"al1","name":"Album"},"duration_ms":215000,"artwork":[{"url":"https://img.example/1.jpg","width":300,"height":null}],"released":{"year":1999,"month":12,"day":31},"uploaded":{"year":2001},"explicit":true,"capabilities":{"seekable":false,"live":false},"availability":"Available","url":"https://spotify.example/track/s1","isrc":"GBAYE0000001","attributes":[{"key":"genre","value":{"Text":"ambient"}},{"key":"bpm","value":{"Number":92.5}},{"key":"soundcloud:reposted","value":{"YesNo":false}}]}');
INSERT INTO queue_entries VALUES (3, 0, 3, 0, 4, NULL, NULL, NULL, 'jellyfin', 'item-9', 'jellyfin-1', '{"reference":{"provider":"jellyfin","id":"item-9","connection":"jellyfin-1"},"title":"On my server","artists":[],"album":null,"duration_ms":null,"artwork":[],"released":null,"uploaded":null,"explicit":false,"capabilities":{"seekable":true,"live":false},"availability":{"Unavailable":{"reason":"Server unreachable"}},"url":null,"isrc":null,"attributes":[]}');
INSERT INTO queue_entries VALUES (5, 0, 1, 1, NULL, NULL, NULL, NULL, 'soundcloud', 'passed', NULL, '{"reference":{"provider":"soundcloud","id":"passed"},"title":"Title passed","artists":[{"id":"a1","name":"Artist One"},{"id":null,"name":"Guest"}],"album":{"id":"al1","name":"Album"},"duration_ms":215000,"artwork":[{"url":"https://img.example/1.jpg","width":300,"height":null}],"released":{"year":1999,"month":12,"day":31},"uploaded":{"year":2001},"explicit":true,"capabilities":{"seekable":false,"live":false},"availability":"Available","url":"https://soundcloud.example/track/passed","isrc":"GBAYE0000001","attributes":[{"key":"genre","value":{"Text":"ambient"}},{"key":"bpm","value":{"Number":92.5}},{"key":"soundcloud:reposted","value":{"YesNo":false}}]}');
INSERT INTO queue_entries VALUES (6, 2, 0, 1, NULL, NULL, NULL, NULL, 'soundcloud', 'playing', NULL, '{"reference":{"provider":"soundcloud","id":"playing"},"title":"Title playing","artists":[{"id":"a1","name":"Artist One"},{"id":null,"name":"Guest"}],"album":{"id":"al1","name":"Album"},"duration_ms":215000,"artwork":[{"url":"https://img.example/1.jpg","width":300,"height":null}],"released":{"year":1999,"month":12,"day":31},"uploaded":{"year":2001},"explicit":true,"capabilities":{"seekable":false,"live":false},"availability":"Available","url":"https://soundcloud.example/track/playing","isrc":"GBAYE0000001","attributes":[{"key":"genre","value":{"Text":"ambient"}},{"key":"bpm","value":{"Number":92.5}},{"key":"soundcloud:reposted","value":{"YesNo":false}}]}');
INSERT INTO queue_entries VALUES (7, 1, 0, 1, NULL, NULL, NULL, NULL, 'spotify', 'next', NULL, '{"reference":{"provider":"spotify","id":"next"},"title":"Title next","artists":[{"id":"a1","name":"Artist One"},{"id":null,"name":"Guest"}],"album":{"id":"al1","name":"Album"},"duration_ms":215000,"artwork":[{"url":"https://img.example/1.jpg","width":300,"height":null}],"released":{"year":1999,"month":12,"day":31},"uploaded":{"year":2001},"explicit":true,"capabilities":{"seekable":false,"live":false},"availability":"Available","url":"https://spotify.example/track/next","isrc":"GBAYE0000001","attributes":[{"key":"genre","value":{"Text":"ambient"}},{"key":"bpm","value":{"Number":92.5}},{"key":"soundcloud:reposted","value":{"YesNo":false}}]}');
INSERT INTO queue_entries VALUES (8, 1, 1, 1, NULL, NULL, 'member-1', 'Ada', 'soundcloud', 'later', NULL, '{"reference":{"provider":"soundcloud","id":"later"},"title":"Title later","artists":[{"id":"a1","name":"Artist One"},{"id":null,"name":"Guest"}],"album":{"id":"al1","name":"Album"},"duration_ms":215000,"artwork":[{"url":"https://img.example/1.jpg","width":300,"height":null}],"released":{"year":1999,"month":12,"day":31},"uploaded":{"year":2001},"explicit":true,"capabilities":{"seekable":false,"live":false},"availability":"Available","url":"https://soundcloud.example/track/later","isrc":"GBAYE0000001","attributes":[{"key":"genre","value":{"Text":"ambient"}},{"key":"bpm","value":{"Number":92.5}},{"key":"soundcloud:reposted","value":{"YesNo":false}}]}');
INSERT INTO recently_played VALUES (1, 'spotify', 's1', NULL, '{"reference":{"provider":"spotify","id":"s1"},"title":"Title s1","artists":[{"id":"a1","name":"Artist One"},{"id":null,"name":"Guest"}],"album":{"id":"al1","name":"Album"},"duration_ms":215000,"artwork":[{"url":"https://img.example/1.jpg","width":300,"height":null}],"released":{"year":1999,"month":12,"day":31},"uploaded":{"year":2001},"explicit":true,"capabilities":{"seekable":false,"live":false},"availability":"Available","url":"https://spotify.example/track/s1","isrc":"GBAYE0000001","attributes":[{"key":"genre","value":{"Text":"ambient"}},{"key":"bpm","value":{"Number":92.5}},{"key":"soundcloud:reposted","value":{"YesNo":false}}]}', 1760000001000);
INSERT INTO recently_played VALUES (2, 'soundcloud', 'playing', NULL, '{"reference":{"provider":"soundcloud","id":"playing"},"title":"Title playing","artists":[{"id":"a1","name":"Artist One"},{"id":null,"name":"Guest"}],"album":{"id":"al1","name":"Album"},"duration_ms":215000,"artwork":[{"url":"https://img.example/1.jpg","width":300,"height":null}],"released":{"year":1999,"month":12,"day":31},"uploaded":{"year":2001},"explicit":true,"capabilities":{"seekable":false,"live":false},"availability":"Available","url":"https://soundcloud.example/track/playing","isrc":"GBAYE0000001","attributes":[{"key":"genre","value":{"Text":"ambient"}},{"key":"bpm","value":{"Number":92.5}},{"key":"soundcloud:reposted","value":{"YesNo":false}}]}', 1760000002000);
INSERT INTO response_sequences VALUES ('spotify', '', 10);
INSERT INTO response_sequences VALUES ('spotify', 'spotify-1', 1000);
INSERT INTO ristle_playlist_entries VALUES ('road', 0, 'spotify', 's1', NULL, '{"reference":{"provider":"spotify","id":"s1"},"title":"Title s1","artists":[{"id":"a1","name":"Artist One"},{"id":null,"name":"Guest"}],"album":{"id":"al1","name":"Album"},"duration_ms":215000,"artwork":[{"url":"https://img.example/1.jpg","width":300,"height":null}],"released":{"year":1999,"month":12,"day":31},"uploaded":{"year":2001},"explicit":true,"capabilities":{"seekable":false,"live":false},"availability":"Available","url":"https://spotify.example/track/s1","isrc":"GBAYE0000001","attributes":[{"key":"genre","value":{"Text":"ambient"}},{"key":"bpm","value":{"Number":92.5}},{"key":"soundcloud:reposted","value":{"YesNo":false}}]}');
INSERT INTO ristle_playlist_entries VALUES ('road', 1, 'soundcloud', 'gone', NULL, '{"reference":{"provider":"soundcloud","id":"gone"},"title":"Title gone","artists":[{"id":"a1","name":"Artist One"},{"id":null,"name":"Guest"}],"album":{"id":"al1","name":"Album"},"duration_ms":215000,"artwork":[{"url":"https://img.example/1.jpg","width":300,"height":null}],"released":{"year":1999,"month":12,"day":31},"uploaded":{"year":2001},"explicit":true,"capabilities":{"seekable":false,"live":false},"availability":{"Unavailable":{"reason":null}},"url":"https://soundcloud.example/track/gone","isrc":"GBAYE0000001","attributes":[{"key":"genre","value":{"Text":"ambient"}},{"key":"bpm","value":{"Number":92.5}},{"key":"soundcloud:reposted","value":{"YesNo":false}}]}');
INSERT INTO ristle_playlist_entries VALUES ('road', 2, 'spotify', 's1', NULL, '{"reference":{"provider":"spotify","id":"s1"},"title":"Title s1","artists":[{"id":"a1","name":"Artist One"},{"id":null,"name":"Guest"}],"album":{"id":"al1","name":"Album"},"duration_ms":215000,"artwork":[{"url":"https://img.example/1.jpg","width":300,"height":null}],"released":{"year":1999,"month":12,"day":31},"uploaded":{"year":2001},"explicit":true,"capabilities":{"seekable":false,"live":false},"availability":"Available","url":"https://spotify.example/track/s1","isrc":"GBAYE0000001","attributes":[{"key":"genre","value":{"Text":"ambient"}},{"key":"bpm","value":{"Number":92.5}},{"key":"soundcloud:reposted","value":{"YesNo":false}}]}');
INSERT INTO ristle_playlist_entries VALUES ('road', 3, 'jellyfin', 'item-9', 'jellyfin-1', '{"reference":{"provider":"jellyfin","id":"item-9","connection":"jellyfin-1"},"title":"On my server","artists":[],"album":null,"duration_ms":null,"artwork":[],"released":null,"uploaded":null,"explicit":false,"capabilities":{"seekable":true,"live":false},"availability":{"Unavailable":{"reason":"Server unreachable"}},"url":null,"isrc":null,"attributes":[]}');
INSERT INTO ristle_playlists VALUES ('road', 'Road trip', 1759913600000, 1759996400000);
INSERT INTO sequence_progress VALUES ('liked:spotify-1+soundcloud-1', 2, 4, -9223372036854775808, 3);
INSERT INTO sequence_routes VALUES ('liked:spotify-1+soundcloud-1', 0, 0, 'spotify-1', 'liked', 1, 4, 0, 'rev-1', 0, NULL);
INSERT INTO sequence_routes VALUES ('liked:spotify-1+soundcloud-1', 1, 0, 'soundcloud-1', 'stream', 0, NULL, NULL, NULL, 1, NULL);
INSERT INTO sequence_routes VALUES ('liked:spotify-1+soundcloud-1', 2, 1, NULL, NULL, NULL, NULL, NULL, NULL, NULL, 'road');
INSERT INTO sequence_routes VALUES ('liked:spotify-1+soundcloud-1', 3, 2, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL);
INSERT INTO sequence_slots VALUES ('liked:spotify-1+soundcloud-1', 0, 0, 1, 0, 0, 'spotify', 's1', NULL);
INSERT INTO sequence_slots VALUES ('liked:spotify-1+soundcloud-1', 1, 1, 2, 0, 3, 'spotify', 's1', NULL);
INSERT INTO sequence_slots VALUES ('liked:spotify-1+soundcloud-1', 2, 2, 4, 0, 1, NULL, NULL, NULL);
INSERT INTO sequence_slots VALUES ('liked:spotify-1+soundcloud-1', 3, 4, 0, 3, 0, 'jellyfin', 'item-9', 'jellyfin-1');
INSERT INTO sequence_slots VALUES ('liked:spotify-1+soundcloud-1', 4, 3, 3, 1, 0, 'soundcloud', 'x', NULL);
INSERT INTO sequences VALUES ('liked:spotify-1+soundcloud-1', 'Liked Songs', '"LikedTracks"', 5);
INSERT INTO settings VALUES ('audio.volume', '0.8');
INSERT INTO settings VALUES ('ui.theme', 'dark');
PRAGMA user_version = 1;
