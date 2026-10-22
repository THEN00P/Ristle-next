//! Installed Plugins, their grants, Provider Connections, the Plugin
//! key-value stores and response sequences.
//!
//! These tables hold what the Plugin host needs to persist. The host owns
//! the rules (consent, quotas, generations); this module stores the values
//! and keeps the pieces that must change together in one statement or one
//! transaction.

use ristle_domain::{Artwork, ConnectionId, PluginId};
use serde::{Deserialize, Serialize};

use crate::sql::{flag, int, optional, read_flag, to_u64, uint};
use crate::{Error, Tx};

const PLUGINS: &str = "plugins";
const CONNECTIONS: &str = "provider_connections";
const GRANTS: &str = "plugin_grants";

/// An installed Plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRecord {
    /// Its manifest id.
    pub id: PluginId,
    /// The installed version, as the manifest states it.
    pub version: String,
    /// The hash of the installed package, which names its files on disk.
    pub package_hash: String,
    /// The Publisher key that signed it.
    pub publisher_key: String,
    /// Whether it runs. A disabled Plugin's Tracks are Disabled Tracks.
    pub enabled: bool,
    /// When it was first installed.
    pub installed_ms: u64,
    /// When the installed package last changed.
    pub updated_ms: u64,
}

/// The `plugins` table.
pub struct Plugins<'a> {
    tx: &'a Tx<'a>,
}

impl<'a> Plugins<'a> {
    pub(crate) fn new(tx: &'a Tx<'a>) -> Self {
        Self { tx }
    }

    /// Records an install or an update. Grants, connections and data stay;
    /// the host removes what an update or takeover invalidates.
    pub fn put(&self, plugin: &PluginRecord) -> Result<(), Error> {
        if plugin.id.is_empty() {
            return Err(Error::invalid("Plugin", "the id is empty"));
        }
        self.tx
            .write_conn()?
            .prepare_cached(
                "INSERT INTO plugins (plugin_id, version, package_hash, publisher_key, enabled,
                     installed_ms, updated_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT (plugin_id) DO UPDATE SET version = excluded.version,
                     package_hash = excluded.package_hash, publisher_key = excluded.publisher_key,
                     enabled = excluded.enabled, installed_ms = excluded.installed_ms,
                     updated_ms = excluded.updated_ms",
            )?
            .execute(rusqlite::params![
                plugin.id.as_str(),
                plugin.version,
                plugin.package_hash,
                plugin.publisher_key,
                flag(plugin.enabled),
                int(plugin.installed_ms, "install time")?,
                int(plugin.updated_ms, "update time")?,
            ])?;
        Ok(())
    }

    /// One Plugin.
    pub fn get(&self, id: &PluginId) -> Result<Option<PluginRecord>, Error> {
        optional(
            self.tx.conn(),
            "SELECT plugin_id, version, package_hash, publisher_key, enabled, installed_ms, updated_ms
             FROM plugins WHERE plugin_id = ?1",
            [id.as_str()],
            raw_plugin,
        )?
        .map(plugin_record)
        .transpose()
    }

    /// Every installed Plugin, by id.
    pub fn list(&self) -> Result<Vec<PluginRecord>, Error> {
        let mut select = self.tx.conn().prepare_cached(
            "SELECT plugin_id, version, package_hash, publisher_key, enabled, installed_ms, updated_ms
             FROM plugins ORDER BY plugin_id",
        )?;
        let raw = select
            .query_map([], raw_plugin)?
            .collect::<Result<Vec<_>, _>>()?;
        raw.into_iter().map(plugin_record).collect()
    }

    /// Uninstalls a Plugin with its Plugin Permissions, Provider
    /// Connections, their grants and data, and its key-value store. Response
    /// sequences stay, so they never go back if it is installed again.
    pub fn remove(&self, id: &PluginId) -> Result<bool, Error> {
        Ok(self
            .tx
            .write_conn()?
            .prepare_cached("DELETE FROM plugins WHERE plugin_id = ?1")?
            .execute([id.as_str()])?
            > 0)
    }
}

type RawPlugin = (String, String, String, String, i64, i64, i64);

fn raw_plugin(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawPlugin> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
    ))
}

fn plugin_record(raw: RawPlugin) -> Result<PluginRecord, Error> {
    let (id, version, package_hash, publisher_key, enabled, installed, updated) = raw;
    Ok(PluginRecord {
        id: PluginId::new(id),
        version,
        package_hash,
        publisher_key,
        enabled: read_flag(enabled, PLUGINS, "enabled")?,
        installed_ms: uint(installed, PLUGINS, "installed_ms")?,
        updated_ms: uint(updated, PLUGINS, "updated_ms")?,
    })
}

/// The account a Provider Connection signed in to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    /// The Provider's account id.
    pub id: String,
    /// The name to show.
    pub name: String,
    /// The account's picture, in the renditions the Provider offers.
    #[serde(default)]
    pub avatar: Vec<Artwork>,
}

/// A Provider Connection's stored metadata. Its Credential Record is in the
/// platform's secret store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionRecord {
    /// Its id, assigned by the host and unique across Plugins.
    pub id: ConnectionId,
    /// The Plugin it belongs to.
    pub plugin: PluginId,
    /// Changes with every accepted authorization (WIT `http`). Cached
    /// results and connection data of an older generation are gone.
    pub generation: u64,
    /// The account of the last accepted authorization.
    pub account: Option<Account>,
    /// When it was created.
    pub created_ms: u64,
    /// When its account or generation last changed.
    pub updated_ms: u64,
}

/// The `provider_connections` and `response_sequences` tables.
pub struct Connections<'a> {
    tx: &'a Tx<'a>,
}

impl<'a> Connections<'a> {
    pub(crate) fn new(tx: &'a Tx<'a>) -> Self {
        Self { tx }
    }

    /// Creates a connection at generation 0, without an account, stamped
    /// with the clock.
    pub fn create(&self, id: &ConnectionId, plugin: &PluginId) -> Result<ConnectionRecord, Error> {
        if id.is_empty() {
            return Err(Error::invalid("Provider Connection", "the id is empty"));
        }
        let now = self.tx.now_ms();
        self.tx
            .write_conn()?
            .prepare_cached(
                "INSERT INTO provider_connections
                     (connection_id, plugin_id, generation, account, created_ms, updated_ms)
                 VALUES (?1, ?2, 0, NULL, ?3, ?3)",
            )?
            .execute(rusqlite::params![
                id.as_str(),
                plugin.as_str(),
                int(now, "creation time")?
            ])?;
        Ok(ConnectionRecord {
            id: id.clone(),
            plugin: plugin.clone(),
            generation: 0,
            account: None,
            created_ms: now,
            updated_ms: now,
        })
    }

    /// One connection.
    pub fn get(&self, id: &ConnectionId) -> Result<Option<ConnectionRecord>, Error> {
        optional(
            self.tx.conn(),
            "SELECT connection_id, plugin_id, generation, account, created_ms, updated_ms
             FROM provider_connections WHERE connection_id = ?1",
            [id.as_str()],
            raw_connection,
        )?
        .map(connection_record)
        .transpose()
    }

    /// Every connection, in creation order.
    pub fn list(&self) -> Result<Vec<ConnectionRecord>, Error> {
        let mut select = self.tx.conn().prepare_cached(
            "SELECT connection_id, plugin_id, generation, account, created_ms, updated_ms
             FROM provider_connections ORDER BY created_ms, connection_id",
        )?;
        let raw = select
            .query_map([], raw_connection)?
            .collect::<Result<Vec<_>, _>>()?;
        raw.into_iter().map(connection_record).collect()
    }

    /// Starts a new generation after an accepted authorization: stores the
    /// account and deletes the connection's data (WIT `connection.set-data`)
    /// in the same transaction. Cached results in `cache.sqlite` are the
    /// caller's to clear ([`crate::repo::HttpCache::clear_connection`],
    /// [`crate::repo::PageCache::clear_connection`]). Returns the new
    /// generation.
    pub fn begin_generation(
        &self,
        id: &ConnectionId,
        account: Option<&Account>,
    ) -> Result<u64, Error> {
        let conn = self.tx.write_conn()?;
        let account = account
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| Error::invalid("account", error.to_string()))?;
        let generation: Option<i64> = optional(
            conn,
            "UPDATE provider_connections SET generation = generation + 1, account = ?2, updated_ms = ?3
             WHERE connection_id = ?1 RETURNING generation",
            rusqlite::params![id.as_str(), account, int(self.tx.now_ms(), "update time")?],
            |row| row.get(0),
        )?;
        let generation = generation
            .ok_or_else(|| Error::invalid("Provider Connection", format!("{id} does not exist")))?;
        conn.prepare_cached("DELETE FROM connection_data WHERE connection_id = ?1")?
            .execute([id.as_str()])?;
        uint(generation, CONNECTIONS, "generation")
    }

    /// Removes a connection with its grants, data and Local Track index.
    /// Its response sequence stays.
    pub fn remove(&self, id: &ConnectionId) -> Result<bool, Error> {
        Ok(self
            .tx
            .write_conn()?
            .prepare_cached("DELETE FROM provider_connections WHERE connection_id = ?1")?
            .execute([id.as_str()])?
            > 0)
    }

    /// Reserves the next `count` response sequence numbers of a connection
    /// (WIT `response.sequence`) and returns the first. They are durable
    /// once the transaction commits, so a caller may hand them out from
    /// memory without a write per response, and they never repeat, even for
    /// a connection id used again. The anonymous connection is the empty
    /// id, one per Plugin.
    pub fn reserve_response_sequences(
        &self,
        plugin: &PluginId,
        connection: &ConnectionId,
        count: u64,
    ) -> Result<u64, Error> {
        if count == 0 {
            return Err(Error::invalid(
                "response sequence reservation",
                "the count is 0",
            ));
        }
        let reserved: i64 = self
            .tx
            .write_conn()?
            .prepare_cached(
                "INSERT INTO response_sequences (plugin_id, connection_id, reserved)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT (plugin_id, connection_id) DO UPDATE SET reserved = reserved + ?3
                 RETURNING reserved",
            )?
            .query_row(
                rusqlite::params![
                    plugin.as_str(),
                    connection.as_str(),
                    int(count, "reservation")?
                ],
                |row| row.get(0),
            )?;
        // Numbers start at 1; the stored value is the last one reserved.
        Ok(uint(reserved, "response_sequences", "reserved")? - count + 1)
    }
}

type RawConnection = (String, String, i64, Option<String>, i64, i64);

fn raw_connection(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawConnection> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
    ))
}

fn connection_record(raw: RawConnection) -> Result<ConnectionRecord, Error> {
    let (id, plugin, generation, account, created, updated) = raw;
    Ok(ConnectionRecord {
        id: ConnectionId::new(id),
        plugin: PluginId::new(plugin),
        generation: uint(generation, CONNECTIONS, "generation")?,
        account: account
            .map(|json| serde_json::from_str(&json))
            .transpose()
            .map_err(|error| {
                Error::corrupt(CONNECTIONS, format!("account does not decode: {error}"))
            })?,
        created_ms: uint(created, CONNECTIONS, "created_ms")?,
        updated_ms: uint(updated, CONNECTIONS, "updated_ms")?,
    })
}

/// A folder of a Folder Grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderGrant {
    /// Host-assigned, stable while the grant lasts (WIT `music-folder.id`).
    pub id: String,
    /// The folder's name as the system shows it.
    pub name: String,
    /// What the platform needs to open it again: a path on desktop, a
    /// document tree URI on Android, a security-scoped bookmark on iOS.
    pub locator: String,
}

/// An origin of a Server Grant and what the user confirmed for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmedOrigin {
    /// `scheme://host:port`.
    pub origin: String,
    /// The user accepted that it resolves to public addresses.
    pub public: bool,
    /// The local network or same-device addresses the user confirmed.
    #[serde(default)]
    pub addresses: Vec<String>,
}

/// A Server Grant (WIT `media-server`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerGrant {
    /// The server's own name.
    pub name: String,
    /// Base URLs that reach it.
    pub urls: Vec<String>,
    /// What the user confirmed, per origin.
    #[serde(default)]
    pub confirmed: Vec<ConfirmedOrigin>,
}

const SERVER_KEY: &str = "server";

/// The `plugin_grants` table: Plugin Permissions (Plugin-wide), Folder
/// Grants and Server Grants (per Provider Connection).
pub struct Grants<'a> {
    tx: &'a Tx<'a>,
}

impl<'a> Grants<'a> {
    pub(crate) fn new(tx: &'a Tx<'a>) -> Self {
        Self { tx }
    }

    /// Replaces a Plugin's Plugin Permissions with `permissions`, in order.
    pub fn set_permissions(&self, plugin: &PluginId, permissions: &[String]) -> Result<(), Error> {
        let conn = self.tx.write_conn()?;
        if permissions.iter().any(String::is_empty) {
            return Err(Error::invalid("Plugin Permission", "a permission is empty"));
        }
        conn.prepare_cached("DELETE FROM plugin_grants WHERE plugin_id = ?1 AND kind = 0")?
            .execute([plugin.as_str()])?;
        let mut insert = conn.prepare_cached(
            "INSERT INTO plugin_grants (plugin_id, connection_id, kind, position, grant_key, detail)
             VALUES (?1, NULL, 0, ?2, ?3, NULL)",
        )?;
        for (position, permission) in permissions.iter().enumerate() {
            insert
                .execute(rusqlite::params![
                    plugin.as_str(),
                    crate::sql::index(position, "position")?,
                    permission
                ])
                .map_err(|error| duplicate(error, "Plugin Permission", permission))?;
        }
        Ok(())
    }

    /// A Plugin's Plugin Permissions, in the order they were granted.
    pub fn permissions(&self, plugin: &PluginId) -> Result<Vec<String>, Error> {
        let mut select = self.tx.conn().prepare_cached(
            "SELECT grant_key FROM plugin_grants WHERE plugin_id = ?1 AND kind = 0 ORDER BY position",
        )?;
        let permissions = select
            .query_map([plugin.as_str()], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        Ok(permissions)
    }

    /// Every Plugin's Plugin Permissions, by Plugin id. Plugins without any
    /// are left out.
    pub fn all_permissions(&self) -> Result<Vec<(PluginId, Vec<String>)>, Error> {
        let mut select = self.tx.conn().prepare_cached(
            "SELECT plugin_id, grant_key FROM plugin_grants WHERE kind = 0
             ORDER BY plugin_id, position",
        )?;
        let mut rows = select.query([])?;
        let mut all: Vec<(PluginId, Vec<String>)> = Vec::new();
        while let Some(row) = rows.next()? {
            let plugin = PluginId::new(row.get::<_, String>(0)?);
            let permission: String = row.get(1)?;
            match all.last_mut() {
                Some((last, permissions)) if *last == plugin => permissions.push(permission),
                _ => all.push((plugin, vec![permission])),
            }
        }
        Ok(all)
    }

    /// Replaces a connection's Folder Grant.
    pub fn set_folders(
        &self,
        connection: &ConnectionId,
        folders: &[FolderGrant],
    ) -> Result<(), Error> {
        let conn = self.tx.write_conn()?;
        let plugin = self.plugin_of(connection)?;
        conn.prepare_cached("DELETE FROM plugin_grants WHERE connection_id = ?1 AND kind = 1")?
            .execute([connection.as_str()])?;
        let mut insert = conn.prepare_cached(
            "INSERT INTO plugin_grants (plugin_id, connection_id, kind, position, grant_key, detail)
             VALUES (?1, ?2, 1, ?3, ?4, ?5)",
        )?;
        for (position, folder) in folders.iter().enumerate() {
            if folder.id.is_empty() {
                return Err(Error::invalid("Folder Grant", "a folder id is empty"));
            }
            let detail = serde_json::to_string(folder)
                .map_err(|error| Error::invalid("Folder Grant", error.to_string()))?;
            insert
                .execute(rusqlite::params![
                    plugin,
                    connection.as_str(),
                    crate::sql::index(position, "position")?,
                    folder.id,
                    detail
                ])
                .map_err(|error| duplicate(error, "Folder Grant", &folder.id))?;
        }
        Ok(())
    }

    /// A connection's Folder Grant, in the order the user chose the folders.
    pub fn folders(&self, connection: &ConnectionId) -> Result<Vec<FolderGrant>, Error> {
        let mut select = self.tx.conn().prepare_cached(
            "SELECT detail FROM plugin_grants WHERE connection_id = ?1 AND kind = 1 ORDER BY position",
        )?;
        let details = select
            .query_map([connection.as_str()], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        details.iter().map(|json| decode_detail(json)).collect()
    }

    /// Sets or, with `None`, removes a connection's Server Grant.
    pub fn set_server(
        &self,
        connection: &ConnectionId,
        server: Option<&ServerGrant>,
    ) -> Result<(), Error> {
        let conn = self.tx.write_conn()?;
        let plugin = self.plugin_of(connection)?;
        conn.prepare_cached("DELETE FROM plugin_grants WHERE connection_id = ?1 AND kind = 2")?
            .execute([connection.as_str()])?;
        if let Some(server) = server {
            let detail = serde_json::to_string(server)
                .map_err(|error| Error::invalid("Server Grant", error.to_string()))?;
            conn.prepare_cached(
                "INSERT INTO plugin_grants (plugin_id, connection_id, kind, position, grant_key, detail)
                 VALUES (?1, ?2, 2, 0, ?3, ?4)",
            )?
            .execute(rusqlite::params![plugin, connection.as_str(), SERVER_KEY, detail])?;
        }
        Ok(())
    }

    /// A connection's Server Grant.
    pub fn server(&self, connection: &ConnectionId) -> Result<Option<ServerGrant>, Error> {
        optional(
            self.tx.conn(),
            "SELECT detail FROM plugin_grants WHERE connection_id = ?1 AND kind = 2",
            [connection.as_str()],
            |row| row.get::<_, String>(0),
        )?
        .map(|json| decode_detail(&json))
        .transpose()
    }

    fn plugin_of(&self, connection: &ConnectionId) -> Result<String, Error> {
        optional(
            self.tx.conn(),
            "SELECT plugin_id FROM provider_connections WHERE connection_id = ?1",
            [connection.as_str()],
            |row| row.get(0),
        )?
        .ok_or_else(|| {
            Error::invalid(
                "grant",
                format!("Provider Connection {connection} does not exist"),
            )
        })
    }
}

fn decode_detail<T: for<'de> Deserialize<'de>>(json: &str) -> Result<T, Error> {
    serde_json::from_str(json)
        .map_err(|error| Error::corrupt(GRANTS, format!("grant does not decode: {error}")))
}

fn duplicate(error: rusqlite::Error, what: &'static str, key: &str) -> Error {
    match error.sqlite_error_code() {
        Some(rusqlite::ErrorCode::ConstraintViolation) => {
            Error::invalid(what, format!("{key} appears twice"))
        }
        _ => Error::Database(error),
    }
}

/// Which key-value store: a Plugin's `storage`, shared by all its
/// connections, or one connection's data.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum KvScope {
    /// The WIT `storage` interface of a Plugin.
    Plugin(PluginId),
    /// The WIT `connection.get-data` and `set-data` of a connection.
    Connection(ConnectionId),
}

/// The `plugin_kv` and `connection_data` tables.
///
/// Expired values read as absent and are deleted by
/// [`PluginKv::purge_expired`]. A Plugin's quota covers its `storage` and
/// the data of all its connections, counted as key plus value bytes.
pub struct PluginKv<'a> {
    tx: &'a Tx<'a>,
}

impl<'a> PluginKv<'a> {
    pub(crate) fn new(tx: &'a Tx<'a>) -> Self {
        Self { tx }
    }

    /// The live value under `key`.
    pub fn get(&self, scope: &KvScope, key: &str) -> Result<Option<Vec<u8>>, Error> {
        let (table, column, owner) = scope.parts();
        optional(
            self.tx.conn(),
            &format!(
                "SELECT value FROM {table} WHERE {column} = ?1 AND key = ?2
                 AND (expires_ms IS NULL OR expires_ms > ?3)"
            ),
            rusqlite::params![owner, key, int(self.tx.now_ms(), "time")?],
            |row| row.get(0),
        )
    }

    /// Stores `value` under `key`, expiring after `ttl_seconds` if given.
    /// Fails with [`Error::QuotaExceeded`], writing nothing, when the
    /// Plugin would then hold more than `quota_bytes`.
    pub fn set(
        &self,
        scope: &KvScope,
        key: &str,
        value: &[u8],
        ttl_seconds: Option<u32>,
        quota_bytes: u64,
    ) -> Result<(), Error> {
        let conn = self.tx.write_conn()?;
        let now = self.tx.now_ms();
        let plugin = match scope {
            KvScope::Plugin(plugin) => plugin.as_str().to_owned(),
            KvScope::Connection(connection) => optional(
                conn,
                "SELECT plugin_id FROM provider_connections WHERE connection_id = ?1",
                [connection.as_str()],
                |row| row.get(0),
            )?
            .ok_or_else(|| {
                Error::invalid(
                    "connection data",
                    format!("Provider Connection {connection} does not exist"),
                )
            })?,
        };
        let (table, column, owner) = scope.parts();
        let replaced: i64 = optional(
            conn,
            &format!(
                "SELECT length(CAST(key AS BLOB)) + length(value) FROM {table}
                 WHERE {column} = ?1 AND key = ?2 AND (expires_ms IS NULL OR expires_ms > ?3)"
            ),
            rusqlite::params![owner, key, int(now, "time")?],
            |row| row.get(0),
        )?
        .unwrap_or(0);
        let used = self.usage_of(&plugin, now)?;
        let needed = (used + key.len() as u64 + value.len() as u64)
            .saturating_sub(uint(replaced, table, "size")?);
        if needed > quota_bytes {
            return Err(Error::QuotaExceeded {
                quota: quota_bytes,
                needed,
            });
        }
        let expires = ttl_seconds
            .map(|ttl| int(now.saturating_add(u64::from(ttl) * 1000), "expiry"))
            .transpose()?;
        conn.prepare_cached(&format!(
            "INSERT INTO {table} ({column}, key, value, expires_ms) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT ({column}, key) DO UPDATE SET value = excluded.value,
                 expires_ms = excluded.expires_ms"
        ))?
        .execute(rusqlite::params![owner, key, value, expires])?;
        Ok(())
    }

    /// Removes `key`. `false` when nothing was stored under it.
    pub fn delete(&self, scope: &KvScope, key: &str) -> Result<bool, Error> {
        let (table, column, owner) = scope.parts();
        Ok(self
            .tx
            .write_conn()?
            .prepare_cached(&format!(
                "DELETE FROM {table} WHERE {column} = ?1 AND key = ?2"
            ))?
            .execute([owner, key])?
            > 0)
    }

    /// Removes every value of a scope.
    pub fn clear(&self, scope: &KvScope) -> Result<u64, Error> {
        let (table, column, owner) = scope.parts();
        Ok(to_u64(
            self.tx
                .write_conn()?
                .prepare_cached(&format!("DELETE FROM {table} WHERE {column} = ?1"))?
                .execute([owner])?,
        ))
    }

    /// The bytes a Plugin holds in live values, its connections' data
    /// included.
    pub fn usage(&self, plugin: &PluginId) -> Result<u64, Error> {
        self.usage_of(plugin.as_str(), self.tx.now_ms())
    }

    /// Deletes every expired value. Returns how many.
    pub fn purge_expired(&self) -> Result<u64, Error> {
        let conn = self.tx.write_conn()?;
        let now = int(self.tx.now_ms(), "time")?;
        let mut purged = 0;
        for table in ["plugin_kv", "connection_data"] {
            purged += to_u64(
                conn.prepare_cached(&format!("DELETE FROM {table} WHERE expires_ms <= ?1"))?
                    .execute([now])?,
            );
        }
        Ok(purged)
    }

    fn usage_of(&self, plugin: &str, now: u64) -> Result<u64, Error> {
        let used: i64 = self
            .tx
            .conn()
            .prepare_cached(
                "SELECT
                 (SELECT coalesce(sum(length(CAST(key AS BLOB)) + length(value)), 0) FROM plugin_kv
                  WHERE plugin_id = ?1 AND (expires_ms IS NULL OR expires_ms > ?2))
               + (SELECT coalesce(sum(length(CAST(d.key AS BLOB)) + length(d.value)), 0)
                  FROM connection_data d JOIN provider_connections c USING (connection_id)
                  WHERE c.plugin_id = ?1 AND (d.expires_ms IS NULL OR d.expires_ms > ?2))",
            )?
            .query_row(rusqlite::params![plugin, int(now, "time")?], |row| {
                row.get(0)
            })?;
        uint(used, "plugin_kv", "usage")
    }
}

impl KvScope {
    fn parts(&self) -> (&'static str, &'static str, &str) {
        match self {
            Self::Plugin(plugin) => ("plugin_kv", "plugin_id", plugin.as_str()),
            Self::Connection(connection) => {
                ("connection_data", "connection_id", connection.as_str())
            }
        }
    }
}

/// The `command_receipts` table: outcomes of commands that must apply once,
/// such as Party commands, so a command sent again is answered from its
/// receipt instead of being applied twice.
pub struct Receipts<'a> {
    tx: &'a Tx<'a>,
}

impl<'a> Receipts<'a> {
    /// How many receipts are kept in all. At the limit new commands are
    /// refused rather than old receipts evicted, since an evicted receipt
    /// would let its command apply again.
    pub const LIMIT: u64 = 10_000;
    /// The longest scope or key, in bytes.
    pub const MAX_KEY_BYTES: usize = 512;
    /// The longest outcome, in bytes.
    pub const MAX_OUTCOME_BYTES: usize = 4096;

    pub(crate) fn new(tx: &'a Tx<'a>) -> Self {
        Self { tx }
    }

    /// The outcome recorded for a command.
    pub fn get(&self, scope: &str, key: &str) -> Result<Option<String>, Error> {
        optional(
            self.tx.conn(),
            "SELECT outcome FROM command_receipts WHERE scope = ?1 AND key = ?2",
            [scope, key],
            |row| row.get(0),
        )
    }

    /// Records a command's outcome, stamped with the clock.
    pub fn put(&self, scope: &str, key: &str, outcome: &str) -> Result<(), Error> {
        let conn = self.tx.write_conn()?;
        if scope.is_empty() || key.is_empty() {
            return Err(Error::invalid(
                "command receipt",
                "the scope or key is empty",
            ));
        }
        if scope.len() > Self::MAX_KEY_BYTES || key.len() > Self::MAX_KEY_BYTES {
            return Err(Error::invalid(
                "command receipt",
                "the scope or key is too long",
            ));
        }
        if outcome.len() > Self::MAX_OUTCOME_BYTES {
            return Err(Error::invalid("command receipt", "the outcome is too long"));
        }
        if self.get(scope, key)?.is_some() {
            return Err(Error::ReceiptExists {
                scope: scope.to_owned(),
                key: key.to_owned(),
            });
        }
        let count: i64 = conn
            .prepare_cached("SELECT count(*) FROM command_receipts")?
            .query_row([], |row| row.get(0))?;
        if uint(count, "command_receipts", "count")? >= Self::LIMIT {
            return Err(Error::ReceiptLimit { limit: Self::LIMIT });
        }
        conn.prepare_cached(
            "INSERT INTO command_receipts (scope, key, outcome, created_ms) VALUES (?1, ?2, ?3, ?4)",
        )?
        .execute(rusqlite::params![scope, key, outcome, int(self.tx.now_ms(), "time")?])?;
        Ok(())
    }

    /// Removes every receipt of a scope, such as an ended Party.
    pub fn clear_scope(&self, scope: &str) -> Result<u64, Error> {
        Ok(to_u64(
            self.tx
                .write_conn()?
                .prepare_cached("DELETE FROM command_receipts WHERE scope = ?1")?
                .execute([scope])?,
        ))
    }
}
