-- cache.sqlite, version 1: data Ristle can fetch again.
--
-- Never migrated. A cache database of any other version, or one that cannot
-- be opened, is deleted and created again empty (src/migrate.rs).
-- `size` is the bytes a row holds, for the cache budgets.

-- HTTP responses (`ristle-net`'s `HttpCacheStore`). `key` and `response` are
-- opaque here: the net crate builds and encodes them.
CREATE TABLE http_cache (
  plugin_id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  key TEXT NOT NULL,
  response BLOB NOT NULL,
  size INTEGER NOT NULL CHECK (size >= 0),
  fetched_ms INTEGER NOT NULL,
  accessed_ms INTEGER NOT NULL,
  PRIMARY KEY (plugin_id, connection_id, key)
) STRICT;

CREATE INDEX http_cache_fetched ON http_cache (fetched_ms);
CREATE INDEX http_cache_accessed ON http_cache (plugin_id, accessed_ms);

-- Provider pages, listings and library snapshots, by namespace.
CREATE TABLE page_cache (
  provider_id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  namespace TEXT NOT NULL,
  key TEXT NOT NULL,
  revision TEXT,
  payload BLOB NOT NULL,
  size INTEGER NOT NULL CHECK (size >= 0),
  fetched_ms INTEGER NOT NULL,
  accessed_ms INTEGER NOT NULL,
  PRIMARY KEY (provider_id, connection_id, namespace, key)
) STRICT;

CREATE INDEX page_cache_fetched ON page_cache (fetched_ms);
CREATE INDEX page_cache_accessed ON page_cache (provider_id, accessed_ms);
