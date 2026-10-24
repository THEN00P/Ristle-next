//! Provider Connections as the Plugin host sees them: their generation,
//! account and grants, and their Credential Record
//! (`plugin-api/wit/host.wit`, `http` and `connection`).
//!
//! A connection's generation changes with every accepted authorization and
//! ends at sign-out or removal. Every call is tagged with the generation it
//! started under (and with the Plugin's own retirement count, which moves
//! when a version is installed or the Plugin is disabled); once either
//! moves, the call is retired and its imports reach nothing outside its
//! instance.

use std::collections::HashMap;
use std::sync::Mutex;

use ristle_domain::{Artwork, ConnectionId, PluginId};
use ristle_net::{GrantedOrigin, Header, HostPattern};
use serde::{Deserialize, Serialize};

use crate::host::lock;
use crate::ports::{SecretVault, VaultError};

/// The account from the last successful authorization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    /// The Provider's id for it.
    pub id: String,
    /// Its display name.
    pub name: String,
    /// Its avatar.
    pub avatar: Vec<Artwork>,
}

/// A Server Grant: the server the user chose and confirmed for a
/// connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerGrant {
    /// The server's name.
    pub name: String,
    /// Its URLs, as the Plugin gave them.
    pub urls: Vec<String>,
    /// What the user confirmed for each origin.
    pub origins: Vec<GrantedOrigin>,
}

/// One folder of a Folder Grant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MusicFolder {
    /// The host's id for it.
    pub id: String,
    /// Its name, for display.
    pub name: String,
}

/// What the host knows about one Provider Connection of a Plugin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionInfo {
    /// Its id; empty for the anonymous connection.
    pub id: ConnectionId,
    /// Its generation: changes with every accepted authorization.
    pub generation: u64,
    /// The signed-in account.
    pub account: Option<Account>,
    /// Its Server Grant.
    pub server: Option<ServerGrant>,
    /// Its Folder Grant.
    pub folders: Vec<MusicFolder>,
}

/// One `attach` rule of a Credential Record: headers and query parameters
/// added to every request to matching hosts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachRule {
    /// Host patterns, as in a manifest.
    pub hosts: Vec<String>,
    /// Headers to add.
    pub headers: Vec<Header>,
    /// Query parameters to add.
    pub query: Vec<(String, String)>,
}

/// A Credential Record (`types.credential`). Stored serialized in the
/// platform credential store, at most [`MAX_CREDENTIAL_BYTES`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialRecord {
    /// A versioned auth scheme the Plugin names.
    pub scheme: String,
    /// The access token.
    pub access_token: Option<String>,
    /// The refresh token.
    pub refresh_token: Option<String>,
    /// When the access token expires, in Unix milliseconds.
    pub expires_at_ms: Option<u64>,
    /// Granted scopes.
    pub scopes: Vec<String>,
    /// A username.
    pub username: Option<String>,
    /// A password.
    pub password: Option<String>,
    /// The Plugin's own data.
    pub custom: Option<String>,
    /// How the credential is attached to requests.
    pub attach: Vec<AttachRule>,
}

/// The largest Credential Record, serialized: 64 KiB.
pub const MAX_CREDENTIAL_BYTES: usize = 64 * 1024;

impl CredentialRecord {
    /// The record serialized, if it fits.
    pub(crate) fn encode(&self) -> Option<Vec<u8>> {
        serde_json::to_vec(self)
            .ok()
            .filter(|bytes| bytes.len() <= MAX_CREDENTIAL_BYTES)
    }

    /// The attach rules with their host patterns parsed; `None` when a
    /// pattern is invalid.
    pub(crate) fn parsed_rules(&self) -> Option<Vec<ParsedRule>> {
        self.attach
            .iter()
            .map(|rule| {
                let hosts = rule
                    .hosts
                    .iter()
                    .map(|host| HostPattern::parse(host).ok())
                    .collect::<Option<Vec<_>>>()?;
                Some(ParsedRule {
                    hosts,
                    headers: rule.headers.clone(),
                    query: rule.query.clone(),
                })
            })
            .collect()
    }
}

/// An attach rule ready to apply.
#[derive(Clone, Debug)]
pub(crate) struct ParsedRule {
    pub(crate) hosts: Vec<HostPattern>,
    pub(crate) headers: Vec<Header>,
    pub(crate) query: Vec<(String, String)>,
}

impl ParsedRule {
    pub(crate) fn covers(&self, host: &str) -> bool {
        self.hosts.iter().any(|pattern| pattern.matches(host))
    }
}

/// A connection's stored Credential Record and its write version.
#[derive(Debug, Default)]
struct Slot {
    loaded: bool,
    record: Option<CredentialRecord>,
    version: u64,
}

#[derive(Debug)]
struct Entry {
    info: ConnectionInfo,
    credential: Slot,
}

/// What registering a connection did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Registered {
    /// The host did not know it yet, as at app start.
    New,
    /// Same generation: only its account or grants were updated.
    Unchanged,
    /// Its generation moved: everything of the old one goes.
    NewGeneration,
}

/// Why a credential write was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WriteRefused {
    /// Another write happened since the record was read.
    Conflict,
    /// The connection is gone or its generation moved.
    Retired,
    /// The vault failed.
    Vault(VaultError),
}

/// The connections of one Plugin.
pub(crate) struct Connections {
    plugin: PluginId,
    entries: Mutex<HashMap<ConnectionId, Entry>>,
}

impl Connections {
    pub(crate) fn new(plugin: PluginId) -> Self {
        Self {
            plugin,
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// The vault key of a connection's Credential Record.
    pub(crate) fn vault_key(&self, connection: &ConnectionId) -> String {
        format!("ristle.credential.{}.{}", self.plugin, connection)
    }

    /// Registers or updates a connection.
    pub(crate) fn set(&self, info: ConnectionInfo) -> Registered {
        let mut entries = lock(&self.entries);
        match entries.get_mut(&info.id) {
            Some(entry) if entry.info.generation == info.generation => {
                entry.info = info;
                Registered::Unchanged
            }
            Some(entry) => {
                entry.info = info;
                // A new generation reads its record afresh.
                entry.credential = Slot::default();
                Registered::NewGeneration
            }
            None => {
                entries.insert(
                    info.id.clone(),
                    Entry {
                        info,
                        credential: Slot::default(),
                    },
                );
                Registered::New
            }
        }
    }

    /// The version of the connection's stored record, without reading it.
    pub(crate) fn version(&self, connection: &ConnectionId) -> Option<u64> {
        lock(&self.entries)
            .get(connection)
            .map(|entry| entry.credential.version)
    }

    /// Ends a connection's generation for good.
    pub(crate) fn remove(&self, connection: &ConnectionId) -> bool {
        lock(&self.entries).remove(connection).is_some()
    }

    /// The connection as registered.
    pub(crate) fn info(&self, connection: &ConnectionId) -> Option<ConnectionInfo> {
        lock(&self.entries)
            .get(connection)
            .map(|entry| entry.info.clone())
    }

    /// The connection's current generation, or `None` once it ended.
    pub(crate) fn generation(&self, connection: &ConnectionId) -> Option<u64> {
        lock(&self.entries)
            .get(connection)
            .map(|entry| entry.info.generation)
    }

    /// The stored record and its version, read from the vault the first
    /// time.
    pub(crate) fn credential(
        &self,
        vault: &dyn SecretVault,
        connection: &ConnectionId,
        generation: u64,
    ) -> Result<Option<(Option<CredentialRecord>, u64)>, VaultError> {
        let key = self.vault_key(connection);
        let mut entries = lock(&self.entries);
        let Some(entry) = entries.get_mut(connection) else {
            return Ok(None);
        };
        if entry.info.generation != generation {
            return Ok(None);
        }
        if !entry.credential.loaded {
            let record = vault
                .read(&key)?
                .and_then(|bytes| serde_json::from_slice::<CredentialRecord>(&bytes).ok());
            entry.credential.record = record;
            entry.credential.loaded = true;
        }
        Ok(Some((
            entry.credential.record.clone(),
            entry.credential.version,
        )))
    }

    /// Replaces the stored record if it is still the one at `base`, the
    /// version the writer last read. Writes are serialized by the lock.
    pub(crate) fn write_credential(
        &self,
        vault: &dyn SecretVault,
        connection: &ConnectionId,
        generation: u64,
        base: u64,
        record: CredentialRecord,
        encoded: &[u8],
    ) -> Result<u64, WriteRefused> {
        let key = self.vault_key(connection);
        let mut entries = lock(&self.entries);
        let Some(entry) = entries.get_mut(connection) else {
            return Err(WriteRefused::Retired);
        };
        if entry.info.generation != generation {
            return Err(WriteRefused::Retired);
        }
        if entry.credential.version != base {
            return Err(WriteRefused::Conflict);
        }
        vault.write(&key, encoded).map_err(WriteRefused::Vault)?;
        entry.credential.record = Some(record);
        entry.credential.loaded = true;
        entry.credential.version += 1;
        Ok(entry.credential.version)
    }
}
