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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError, RwLock};

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
    /// Whether its previous generation is cleaned up. Until it is, no call
    /// is admitted and nothing stored for it is read or written.
    cleanup: Cleanup,
}

/// The cleanup of a connection's previous generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cleanup {
    Done,
    Running,
    /// It failed; the connection stays unavailable until registering it
    /// again succeeds in cleaning up.
    Failed,
}

/// Whether a call may run for a connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Admission {
    /// Under this generation.
    Ready(u64),
    /// The host does not know the connection.
    Unknown,
    /// Its previous generation could not be cleaned up.
    Unavailable,
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
    /// Every read and write of a connection's or the Plugin's stored state
    /// (data, credential, cache, storage) holds this for reading and checks
    /// the generation and the retirement count; a generation change, a
    /// removal and a retirement hold it for writing. So once either moves,
    /// nothing of the old one is written or read.
    commits: RwLock<()>,
    /// Moves when a version of the Plugin is installed or it is disabled.
    retirements: AtomicU64,
    /// Woken when a connection's cleanup ends.
    ready: tokio::sync::Notify,
    /// Marked when a generation ends or the Plugin is retired, for I/O
    /// that must stop with its call.
    changed: tokio::sync::watch::Sender<()>,
}

impl Connections {
    pub(crate) fn new(plugin: PluginId) -> Self {
        Self {
            plugin,
            entries: Mutex::new(HashMap::new()),
            commits: RwLock::new(()),
            retirements: AtomicU64::new(0),
            ready: tokio::sync::Notify::new(),
            changed: tokio::sync::watch::Sender::new(()),
        }
    }

    /// How often the Plugin was retired.
    pub(crate) fn retirements(&self) -> u64 {
        self.retirements.load(Ordering::SeqCst)
    }

    /// Retires every running call: no commit made under the old count
    /// lands after this returns.
    pub(crate) fn retire(&self) {
        let _change = self.commits.write().unwrap_or_else(PoisonError::into_inner);
        self.retirements.fetch_add(1, Ordering::SeqCst);
        self.changed.send_replace(());
    }

    /// Resolves once a call tagged with `generation` and `retirements` is
    /// no longer current: its generation ended or the Plugin was retired.
    pub(crate) async fn ended(&self, connection: &ConnectionId, generation: u64, retirements: u64) {
        let mut changes = self.changed.subscribe();
        loop {
            changes.borrow_and_update();
            let current = lock(&self.entries)
                .get(connection)
                .is_some_and(|entry| self.current(entry, generation, retirements));
            if !current || changes.changed().await.is_err() {
                return;
            }
        }
    }

    /// Whether `entry` is still current for a call tagged with `generation`
    /// and `retirements`.
    fn current(&self, entry: &Entry, generation: u64, retirements: u64) -> bool {
        entry.info.generation == generation
            && entry.cleanup == Cleanup::Done
            && self.retirements() == retirements
    }

    /// Runs `f` if `generation` is still the connection's current one, it
    /// is not being cleaned up and the Plugin was not retired since
    /// `retirements`, with none of that able to change meanwhile.
    pub(crate) fn commit<R>(
        &self,
        connection: &ConnectionId,
        generation: u64,
        retirements: u64,
        f: impl FnOnce() -> R,
    ) -> Option<R> {
        let _commit = self.commits.read().unwrap_or_else(PoisonError::into_inner);
        let current = lock(&self.entries)
            .get(connection)
            .is_some_and(|entry| self.current(entry, generation, retirements));
        current.then(f)
    }

    /// Like [`Self::commit`], with the connection as registered, so a check
    /// against it holds for as long as `f` runs.
    pub(crate) fn commit_with<R>(
        &self,
        connection: &ConnectionId,
        generation: u64,
        retirements: u64,
        f: impl FnOnce(&ConnectionInfo) -> R,
    ) -> Option<R> {
        let _commit = self.commits.read().unwrap_or_else(PoisonError::into_inner);
        let info = lock(&self.entries)
            .get(connection)
            .filter(|entry| self.current(entry, generation, retirements))
            .map(|entry| entry.info.clone())?;
        Some(f(&info))
    }

    /// Runs `f` if the Plugin was not retired since `retirements`, for its
    /// connection-independent state (`storage`).
    pub(crate) fn commit_plugin<R>(&self, retirements: u64, f: impl FnOnce() -> R) -> Option<R> {
        let _commit = self.commits.read().unwrap_or_else(PoisonError::into_inner);
        (self.retirements() == retirements).then(f)
    }

    /// The connection as registered, if a call tagged with `generation`
    /// and `retirements` is still current.
    pub(crate) fn info_at(
        &self,
        connection: &ConnectionId,
        generation: u64,
        retirements: u64,
    ) -> Option<ConnectionInfo> {
        let _commit = self.commits.read().unwrap_or_else(PoisonError::into_inner);
        lock(&self.entries)
            .get(connection)
            .filter(|entry| self.current(entry, generation, retirements))
            .map(|entry| entry.info.clone())
    }

    /// Ends the cleanup of a connection's previous generation. A failed one
    /// leaves the connection unavailable.
    pub(crate) fn finish_cleaning(&self, connection: &ConnectionId, succeeded: bool) {
        if let Some(entry) = lock(&self.entries).get_mut(connection) {
            entry.cleanup = if succeeded {
                Cleanup::Done
            } else {
                Cleanup::Failed
            };
        }
        self.ready.notify_waiters();
    }

    /// Whether a new call may run, once any cleanup running for the
    /// connection has ended.
    pub(crate) async fn admit(&self, connection: &ConnectionId) -> Admission {
        loop {
            let notified = self.ready.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            match lock(&self.entries).get(connection) {
                None => return Admission::Unknown,
                Some(entry) => match entry.cleanup {
                    Cleanup::Done => return Admission::Ready(entry.info.generation),
                    Cleanup::Failed => return Admission::Unavailable,
                    Cleanup::Running => {}
                },
            }
            notified.await;
        }
    }

    /// The vault key of a connection's Credential Record. Plugin ids are
    /// reverse DNS and may contain dots; the connection id is hex after the
    /// last dot, so no two Plugin and connection pairs share a key.
    pub(crate) fn vault_key(&self, connection: &ConnectionId) -> String {
        let hex: String = connection
            .as_str()
            .bytes()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        format!("ristle.credential.{}.{hex}", self.plugin)
    }

    /// Registers or updates a connection.
    pub(crate) fn set(&self, info: ConnectionInfo) -> Registered {
        let _change = self.commits.write().unwrap_or_else(PoisonError::into_inner);
        let registered = self.register(info);
        if registered == Registered::NewGeneration {
            self.changed.send_replace(());
        }
        registered
    }

    fn register(&self, info: ConnectionInfo) -> Registered {
        let mut entries = lock(&self.entries);
        match entries.get_mut(&info.id) {
            Some(entry)
                if entry.info.generation == info.generation && entry.cleanup == Cleanup::Done =>
            {
                entry.info = info;
                Registered::Unchanged
            }
            // A cleanup that failed is retried.
            Some(entry) if entry.info.generation == info.generation => {
                entry.info = info;
                entry.cleanup = Cleanup::Running;
                Registered::NewGeneration
            }
            Some(entry) => {
                entry.info = info;
                // A new generation reads its record afresh, once the old
                // one is cleaned up.
                entry.credential = Slot::default();
                entry.cleanup = Cleanup::Running;
                Registered::NewGeneration
            }
            None => {
                entries.insert(
                    info.id.clone(),
                    Entry {
                        info,
                        credential: Slot::default(),
                        cleanup: Cleanup::Done,
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
        let _change = self.commits.write().unwrap_or_else(PoisonError::into_inner);
        let removed = lock(&self.entries).remove(connection).is_some();
        self.changed.send_replace(());
        removed
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
        retirements: u64,
    ) -> Result<Option<(Option<CredentialRecord>, u64)>, VaultError> {
        let key = self.vault_key(connection);
        let _commit = self.commits.read().unwrap_or_else(PoisonError::into_inner);
        let mut entries = lock(&self.entries);
        let Some(entry) = entries.get_mut(connection) else {
            return Ok(None);
        };
        if !self.current(entry, generation, retirements) {
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
    /// version the writer last read, by a call tagged with `generation`
    /// and `retirements` that is still current. Writes are serialized by
    /// the lock.
    pub(crate) fn write_credential(
        &self,
        vault: &dyn SecretVault,
        connection: &ConnectionId,
        (generation, retirements): (u64, u64),
        base: u64,
        record: CredentialRecord,
        encoded: &[u8],
    ) -> Result<u64, WriteRefused> {
        let key = self.vault_key(connection);
        let _commit = self.commits.read().unwrap_or_else(PoisonError::into_inner);
        let mut entries = lock(&self.entries);
        let Some(entry) = entries.get_mut(connection) else {
            return Err(WriteRefused::Retired);
        };
        if !self.current(entry, generation, retirements) {
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
