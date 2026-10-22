-- cache.sqlite, version 1: data Ristle can fetch again.
--
-- Never migrated. A cache database of any other version, or one that cannot
-- be opened, is deleted and created again empty (src/migrate.rs).
-- `size` is the bytes a row holds, for the cache budgets.
--
-- Rows carry their Provider Connection's generation and are read only with
-- the current one. In WAL mode `cache.sqlite` and `ristle.sqlite` do not
-- commit atomically together, so a crash may keep a new generation without
-- the deletion of the old one's rows; those rows are then never served.

-- HTTP responses (`ristle-net`'s `HttpCacheStore`). `key` and `response` are
-- opaque here: the net crate builds and encodes them.
CREATE TABLE http_cache (
  plugin_id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  generation INTEGER NOT NULL CHECK (generation >= 0),
  key TEXT NOT NULL,
  response BLOB NOT NULL,
  size INTEGER NOT NULL CHECK (size >= 0),
  fetched_ms INTEGER NOT NULL,
  accessed_ms INTEGER NOT NULL,
  PRIMARY KEY (plugin_id, connection_id, generation, key)
) STRICT;

CREATE INDEX http_cache_fetched ON http_cache (fetched_ms);
CREATE INDEX http_cache_accessed ON http_cache (plugin_id, accessed_ms);

-- Provider pages, listings and library snapshots, by namespace.
CREATE TABLE page_cache (
  provider_id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  generation INTEGER NOT NULL CHECK (generation >= 0),
  namespace TEXT NOT NULL,
  key TEXT NOT NULL,
  revision TEXT,
  payload BLOB NOT NULL,
  size INTEGER NOT NULL CHECK (size >= 0),
  fetched_ms INTEGER NOT NULL,
  accessed_ms INTEGER NOT NULL,
  PRIMARY KEY (provider_id, connection_id, generation, namespace, key)
) STRICT;

CREATE INDEX page_cache_fetched ON page_cache (fetched_ms);
CREATE INDEX page_cache_accessed ON page_cache (provider_id, accessed_ms);
