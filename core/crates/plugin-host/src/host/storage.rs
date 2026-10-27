//! `storage`: the Plugin's key-value store with the manifest's quota.

use super::{HostState, MAX_PAYLOAD, strings_fit, too_large, trap};
use crate::bindings::ristle::plugin::storage::Host;
use crate::bindings::ristle::plugin::types::Error;
use crate::ports::Stored;

impl Host for HostState {
    fn get(&mut self, key: String) -> wasmtime::Result<Option<Vec<u8>>> {
        if !strings_fit(&[&key]) {
            return Err(trap("storage.get: key exceeds 64 KiB"));
        }
        self.timed(|state| {
            let Some(retirements) = state.live_retirements() else {
                return Ok(None);
            };
            let plugin = &state.plugin;
            let services = &plugin.services;
            let now = services.clock.now_ms();
            match plugin.connections.commit_plugin(retirements, || {
                services.store.plugin_kv(&plugin.id).get(&key, now)
            }) {
                None => Ok(None),
                Some(read) => read.map_err(|error| {
                    tracing::warn!(plugin = %plugin.id, %error, "storage.get failed");
                    trap("storage is unavailable")
                }),
            }
        })
    }

    fn set(
        &mut self,
        key: String,
        value: Vec<u8>,
        ttl_seconds: Option<u32>,
    ) -> wasmtime::Result<Result<(), Error>> {
        if !strings_fit(&[&key]) {
            return Ok(Err(too_large("storage key")));
        }
        if value.len() > MAX_PAYLOAD {
            return Ok(Err(too_large("storage value")));
        }
        Ok(self.timed(|state| {
            let retired = || Error::Unauthorized("this call was retired".to_owned());
            let Some(retirements) = state.live_retirements() else {
                return Err(retired());
            };
            let plugin = &state.plugin;
            let services = &plugin.services;
            let now = services.clock.now_ms();
            let expires = ttl_seconds.map(|ttl| now.saturating_add(u64::from(ttl) * 1000));
            let quota = u64::from(plugin.manifest.permissions.storage_mib) * 1024 * 1024;
            let written = plugin.connections.commit_plugin(retirements, || {
                services
                    .store
                    .plugin_kv(&plugin.id)
                    .set(&key, &value, expires, quota, now)
            });
            let Some(written) = written else {
                return Err(retired());
            };
            match written {
                Ok(Stored::Yes) => Ok(()),
                Ok(Stored::OverQuota) => Err(Error::Internal("storage quota exceeded".to_owned())),
                Err(error) => {
                    tracing::warn!(plugin = %state.plugin.id, %error, "storage.set failed");
                    Err(Error::Transient("storage is unavailable".to_owned()))
                }
            }
        }))
    }

    fn delete(&mut self, key: String) -> wasmtime::Result<()> {
        if !strings_fit(&[&key]) {
            return Err(trap("storage.delete: key exceeds 64 KiB"));
        }
        self.timed(|state| {
            let Some(retirements) = state.live_retirements() else {
                return Ok(());
            };
            let plugin = &state.plugin;
            match plugin.connections.commit_plugin(retirements, || {
                plugin.services.store.plugin_kv(&plugin.id).delete(&key)
            }) {
                None => Ok(()),
                Some(deleted) => deleted.map_err(|error| {
                    tracing::warn!(plugin = %plugin.id, %error, "storage.delete failed");
                    trap("storage is unavailable")
                }),
            }
        })
    }
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use std::sync::{Arc, Mutex};

    use ristle_domain::{ConnectionId, ManualClock};

    use super::*;
    use crate::host::{CallContext, PluginContext, Services};
    use crate::test_support::{
        MemoryPluginStore, MemoryVault, Publisher, connection_info, mock_package,
    };

    #[tokio::test]
    async fn storage_is_unreachable_outside_a_call() {
        let clock = Arc::new(ManualClock::new(0));
        let services = Arc::new(Services {
            clock: clock.clone(),
            store: Arc::new(MemoryPluginStore::new()),
            vault: Arc::new(MemoryVault::new()),
            net: ristle_net::Client::new(ristle_net::ClientConfig::default(), clock, None).unwrap(),
            locale: Mutex::new("en".to_owned()),
            runtime: tokio::runtime::Handle::current(),
            folders: None,
            socket_origins: Vec::new(),
        });
        let manifest = mock_package(&Publisher::new(1), "").manifest().clone();
        let context = Arc::new(PluginContext::new(manifest, services));
        context.connections.set(connection_info("c1", 1));
        let mut state = HostState::new(context);
        // As during instantiation, an install's trial included.
        assert!(matches!(
            state.set("k".into(), b"v".to_vec(), None).unwrap(),
            Err(Error::Unauthorized(_))
        ));
        assert_eq!(state.get("k".into()).unwrap(), None);
        let call = CallContext {
            connection: ConnectionId::new("c1"),
            generation: 1,
            retirements: 0,
            credential_base: 0,
            refresh: false,
        };
        state.call = Some(call.clone());
        state.set("k".into(), b"v".to_vec(), None).unwrap().unwrap();
        assert_eq!(state.get("k".into()).unwrap(), Some(b"v".to_vec()));
        state.call = None;
        state.delete("k".into()).unwrap();
        state.call = Some(call);
        assert_eq!(state.get("k".into()).unwrap(), Some(b"v".to_vec()));
    }
}
