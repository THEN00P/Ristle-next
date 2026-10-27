//! The reproducible caches in `cache.sqlite`: HTTP responses and Provider
//! pages.
//!
//! This module stores and evicts; the policies (freshness, the 30-day
//! retention, the per-Plugin and total budgets) belong to `ristle-net` and
//! the Plugin host, which call [`HttpCache::evict_fetched_before`],
//! [`HttpCache::evict_to_budget`] and their page cache counterparts. Reads
//! do not write, so a hit is recorded for the least-recently-used order
//! with `touch`.
//!
//! Keys include the Provider Connection's generation. A generation change
//! in `ristle.sqlite` and the deletion of the old generation's rows here do
//! not commit atomically (WAL does not make attached databases atomic), so
//! a row of an old generation may survive a crash. It is never read, since
//! callers ask with the current generation, and it ages out or is cleared
//! with the connection.

use ristle_domain::{ConnectionId, PluginId, ProviderId};
use rusqlite::types::Value;

use crate::sql::{int, optional, to_u64, uint};
use crate::{Error, Tx};

const HTTP: &str = "http_cache";
const PAGES: &str = "page_cache";

/// Where a cached HTTP response is kept. Keys always include the Plugin and
/// the Provider Connection (WIT `http`); `key` is built by `ristle-net`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HttpKey {
    /// The Plugin the request was made for.
    pub plugin: PluginId,
    /// Its connection; empty for the anonymous connection.
    pub connection: ConnectionId,
    /// The connection's generation when the response was fetched; 0 for the
    /// anonymous connection.
    pub generation: u64,
    /// `ristle-net`'s key for the request.
    pub key: String,
}

impl HttpKey {
    const WHERE: &str = "plugin_id = ?1 AND connection_id = ?2 AND generation = ?3 AND key = ?4";

    fn values(&self) -> Result<[Value; 4], Error> {
        Ok([
            Value::Text(self.plugin.as_str().to_owned()),
            Value::Text(self.connection.as_str().to_owned()),
            Value::Integer(int(self.generation, "generation")?),
            Value::Text(self.key.clone()),
        ])
    }
}

/// A cached HTTP response, encoded by `ristle-net`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedResponse {
    /// The encoded response.
    pub response: Vec<u8>,
    /// When it was last fetched from the Provider.
    pub fetched_ms: u64,
}

/// The `cache.http_cache` table.
pub struct HttpCache<'a> {
    tx: &'a Tx<'a>,
}

impl<'a> HttpCache<'a> {
    pub(crate) fn new(tx: &'a Tx<'a>) -> Self {
        Self { tx }
    }

    /// The cached response.
    pub fn get(&self, key: &HttpKey) -> Result<Option<CachedResponse>, Error> {
        optional(
            self.tx.conn(),
            &format!(
                "SELECT response, fetched_ms FROM cache.http_cache WHERE {}",
                HttpKey::WHERE
            ),
            rusqlite::params_from_iter(key.values()?),
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?)),
        )?
        .map(|(response, fetched)| {
            Ok(CachedResponse {
                response,
                fetched_ms: uint(fetched, HTTP, "fetched_ms")?,
            })
        })
        .transpose()
    }

    /// Stores a response, used now.
    pub fn put(&self, key: &HttpKey, response: &CachedResponse) -> Result<(), Error> {
        let mut values = key.values()?.to_vec();
        values.extend([
            Value::Blob(response.response.clone()),
            Value::Integer(size(key.key.len() + response.response.len())?),
            Value::Integer(int(response.fetched_ms, "fetch time")?),
            Value::Integer(int(self.tx.now_ms(), "time")?),
        ]);
        self.tx
            .write_conn()?
            .prepare_cached(
                "INSERT INTO cache.http_cache (plugin_id, connection_id, generation, key, response,
                     size, fetched_ms, accessed_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT (plugin_id, connection_id, generation, key) DO UPDATE SET
                     response = excluded.response, size = excluded.size,
                     fetched_ms = excluded.fetched_ms, accessed_ms = excluded.accessed_ms",
            )?
            .execute(rusqlite::params_from_iter(values))?;
        Ok(())
    }

    /// Records a hit for the least-recently-used order.
    pub fn touch(&self, key: &HttpKey) -> Result<bool, Error> {
        let mut values = key.values()?.to_vec();
        values.push(Value::Integer(int(self.tx.now_ms(), "time")?));
        Ok(self
            .tx
            .write_conn()?
            .prepare_cached(&format!(
                "UPDATE cache.http_cache SET accessed_ms = ?5 WHERE {}",
                HttpKey::WHERE
            ))?
            .execute(rusqlite::params_from_iter(values))?
            > 0)
    }

    /// Removes one response.
    pub fn remove(&self, key: &HttpKey) -> Result<bool, Error> {
        Ok(self
            .tx
            .write_conn()?
            .prepare_cached(&format!(
                "DELETE FROM cache.http_cache WHERE {}",
                HttpKey::WHERE
            ))?
            .execute(rusqlite::params_from_iter(key.values()?))?
            > 0)
    }

    /// Removes every response of a connection, of every generation, as a
    /// generation change requires.
    pub fn clear_connection(
        &self,
        plugin: &PluginId,
        connection: &ConnectionId,
    ) -> Result<u64, Error> {
        Ok(to_u64(
            self.tx
                .write_conn()?
                .prepare_cached(
                    "DELETE FROM cache.http_cache WHERE plugin_id = ?1 AND connection_id = ?2",
                )?
                .execute([plugin.as_str(), connection.as_str()])?,
        ))
    }

    /// Removes every response last fetched before `fetched_before_ms`.
    pub fn evict_fetched_before(&self, fetched_before_ms: u64) -> Result<u64, Error> {
        evict_fetched_before(self.tx, HTTP, fetched_before_ms)
    }

    /// Removes the least recently used responses of one Plugin, or of all
    /// Plugins with `None`, until the rest take at most `max_bytes`.
    pub fn evict_to_budget(&self, plugin: Option<&PluginId>, max_bytes: u64) -> Result<u64, Error> {
        evict_to_budget(
            self.tx,
            HTTP,
            "plugin_id",
            plugin.map(PluginId::as_str),
            max_bytes,
        )
    }

    /// The bytes held, by one Plugin or by all.
    pub fn size(&self, plugin: Option<&PluginId>) -> Result<u64, Error> {
        total_size(self.tx, HTTP, "plugin_id", plugin.map(PluginId::as_str))
    }
}

/// Where a cached Provider page is kept.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PageKey {
    /// The Provider.
    pub provider: ProviderId,
    /// The connection it was read through; empty for the anonymous one.
    pub connection: ConnectionId,
    /// The connection's generation when it was read; 0 for the anonymous
    /// connection.
    pub generation: u64,
    /// What kind of result, such as `playlist` or `library`, so one kind
    /// can be cleared alone.
    pub namespace: String,
    /// Which result of that kind.
    pub key: String,
}

impl PageKey {
    const WHERE: &str = "provider_id = ?1 AND connection_id = ?2 AND generation = ?3 AND namespace = ?4 AND key = ?5";

    fn values(&self) -> Result<[Value; 5], Error> {
        Ok([
            Value::Text(self.provider.as_str().to_owned()),
            Value::Text(self.connection.as_str().to_owned()),
            Value::Integer(int(self.generation, "generation")?),
            Value::Text(self.namespace.clone()),
            Value::Text(self.key.clone()),
        ])
    }
}

/// A cached Provider page, encoded by its user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedPage {
    /// The Provider's revision of the content, when it has one.
    pub revision: Option<String>,
    /// The encoded result.
    pub payload: Vec<u8>,
    /// When its oldest input was fetched from the Provider.
    pub fetched_ms: u64,
}

/// The `cache.page_cache` table.
pub struct PageCache<'a> {
    tx: &'a Tx<'a>,
}

impl<'a> PageCache<'a> {
    pub(crate) fn new(tx: &'a Tx<'a>) -> Self {
        Self { tx }
    }

    /// The cached page.
    pub fn get(&self, key: &PageKey) -> Result<Option<CachedPage>, Error> {
        optional(
            self.tx.conn(),
            &format!(
                "SELECT revision, payload, fetched_ms FROM cache.page_cache WHERE {}",
                PageKey::WHERE
            ),
            rusqlite::params_from_iter(key.values()?),
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )?
        .map(|(revision, payload, fetched)| {
            Ok(CachedPage {
                revision,
                payload,
                fetched_ms: uint(fetched, PAGES, "fetched_ms")?,
            })
        })
        .transpose()
    }

    /// Stores a page, used now.
    pub fn put(&self, key: &PageKey, page: &CachedPage) -> Result<(), Error> {
        let mut values = key.values()?.to_vec();
        values.extend([
            page.revision.clone().map_or(Value::Null, Value::Text),
            Value::Blob(page.payload.clone()),
            Value::Integer(size(
                key.namespace.len()
                    + key.key.len()
                    + page.revision.as_ref().map_or(0, String::len)
                    + page.payload.len(),
            )?),
            Value::Integer(int(page.fetched_ms, "fetch time")?),
            Value::Integer(int(self.tx.now_ms(), "time")?),
        ]);
        self.tx
            .write_conn()?
            .prepare_cached(
                "INSERT INTO cache.page_cache (provider_id, connection_id, generation, namespace, key,
                     revision, payload, size, fetched_ms, accessed_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT (provider_id, connection_id, generation, namespace, key) DO UPDATE SET
                     revision = excluded.revision, payload = excluded.payload, size = excluded.size,
                     fetched_ms = excluded.fetched_ms, accessed_ms = excluded.accessed_ms",
            )?
            .execute(rusqlite::params_from_iter(values))?;
        Ok(())
    }

    /// Records a hit for the least-recently-used order.
    pub fn touch(&self, key: &PageKey) -> Result<bool, Error> {
        let mut values = key.values()?.to_vec();
        values.push(Value::Integer(int(self.tx.now_ms(), "time")?));
        Ok(self
            .tx
            .write_conn()?
            .prepare_cached(&format!(
                "UPDATE cache.page_cache SET accessed_ms = ?6 WHERE {}",
                PageKey::WHERE
            ))?
            .execute(rusqlite::params_from_iter(values))?
            > 0)
    }

    /// Removes one page.
    pub fn remove(&self, key: &PageKey) -> Result<bool, Error> {
        Ok(self
            .tx
            .write_conn()?
            .prepare_cached(&format!(
                "DELETE FROM cache.page_cache WHERE {}",
                PageKey::WHERE
            ))?
            .execute(rusqlite::params_from_iter(key.values()?))?
            > 0)
    }

    /// Removes every page of one namespace of one connection, of every
    /// generation.
    pub fn clear_namespace(
        &self,
        provider: &ProviderId,
        connection: &ConnectionId,
        namespace: &str,
    ) -> Result<u64, Error> {
        Ok(to_u64(
            self.tx
                .write_conn()?
                .prepare_cached(
                    "DELETE FROM cache.page_cache
                     WHERE provider_id = ?1 AND connection_id = ?2 AND namespace = ?3",
                )?
                .execute([provider.as_str(), connection.as_str(), namespace])?,
        ))
    }

    /// Removes every page of a connection, of every generation, as a
    /// generation change requires.
    pub fn clear_connection(
        &self,
        provider: &ProviderId,
        connection: &ConnectionId,
    ) -> Result<u64, Error> {
        Ok(to_u64(
            self.tx
                .write_conn()?
                .prepare_cached(
                    "DELETE FROM cache.page_cache WHERE provider_id = ?1 AND connection_id = ?2",
                )?
                .execute([provider.as_str(), connection.as_str()])?,
        ))
    }

    /// Removes every page whose oldest input was fetched before
    /// `fetched_before_ms`.
    pub fn evict_fetched_before(&self, fetched_before_ms: u64) -> Result<u64, Error> {
        evict_fetched_before(self.tx, PAGES, fetched_before_ms)
    }

    /// Removes the least recently used pages of one Provider, or of all with
    /// `None`, until the rest take at most `max_bytes`.
    pub fn evict_to_budget(
        &self,
        provider: Option<&ProviderId>,
        max_bytes: u64,
    ) -> Result<u64, Error> {
        evict_to_budget(
            self.tx,
            PAGES,
            "provider_id",
            provider.map(ProviderId::as_str),
            max_bytes,
        )
    }

    /// The bytes held, by one Provider or by all.
    pub fn size(&self, provider: Option<&ProviderId>) -> Result<u64, Error> {
        total_size(
            self.tx,
            PAGES,
            "provider_id",
            provider.map(ProviderId::as_str),
        )
    }
}

impl Tx<'_> {
    /// Empties `cache.sqlite`. Nothing in `ristle.sqlite` is touched, so
    /// Ristle Playlists, the session and every grant survive.
    pub fn clear_caches(&self) -> Result<(), Error> {
        let conn = self.write_conn()?;
        conn.execute("DELETE FROM cache.http_cache", [])?;
        conn.execute("DELETE FROM cache.page_cache", [])?;
        Ok(())
    }
}

fn size(bytes: usize) -> Result<i64, Error> {
    crate::sql::index(bytes, "cache entry size")
}

fn evict_fetched_before(tx: &Tx<'_>, table: &'static str, before: u64) -> Result<u64, Error> {
    Ok(to_u64(
        tx.write_conn()?
            .prepare_cached(&format!("DELETE FROM cache.{table} WHERE fetched_ms < ?1"))?
            .execute([int(before, "time")?])?,
    ))
}

fn total_size(
    tx: &Tx<'_>,
    table: &'static str,
    owner_column: &str,
    owner: Option<&str>,
) -> Result<u64, Error> {
    let total: i64 = tx
        .conn()
        .prepare_cached(&format!(
            "SELECT coalesce(sum(size), 0) FROM cache.{table} WHERE ?1 IS NULL OR {owner_column} = ?1"
        ))?
        .query_row([owner], |row| row.get(0))?;
    uint(total, table, "size")
}

fn evict_to_budget(
    tx: &Tx<'_>,
    table: &'static str,
    owner_column: &str,
    owner: Option<&str>,
    max_bytes: u64,
) -> Result<u64, Error> {
    let conn = tx.write_conn()?;
    let mut excess = total_size(tx, table, owner_column, owner)?.saturating_sub(max_bytes);
    if excess == 0 {
        return Ok(0);
    }
    let victims = {
        let mut select = conn.prepare_cached(&format!(
            "SELECT rowid, size FROM cache.{table} WHERE ?1 IS NULL OR {owner_column} = ?1
             ORDER BY accessed_ms, rowid"
        ))?;
        let mut rows = select.query([owner])?;
        let mut victims = Vec::new();
        while excess > 0 {
            let Some(row) = rows.next()? else { break };
            victims.push(row.get::<_, i64>(0)?);
            excess = excess.saturating_sub(uint(row.get(1)?, table, "size")?);
        }
        victims
    };
    let mut delete = conn.prepare_cached(&format!("DELETE FROM cache.{table} WHERE rowid = ?1"))?;
    let mut evicted = 0;
    for rowid in victims {
        evicted += to_u64(delete.execute([rowid])?);
    }
    Ok(evicted)
}
