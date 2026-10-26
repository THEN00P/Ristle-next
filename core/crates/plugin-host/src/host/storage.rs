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
            if !state.in_call {
                return Ok(None);
            }
            let services = state.services();
            let now = services.clock.now_ms();
            services
                .store
                .plugin_kv(&state.plugin.id)
                .get(&key, now)
                .map_err(|error| {
                    tracing::warn!(plugin = %state.plugin.id, %error, "storage.get failed");
                    trap("storage is unavailable")
                })
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
            if !state.in_call {
                return Err(Error::Unauthorized(
                    "storage is reachable only in a call".to_owned(),
                ));
            }
            let services = state.services();
            let now = services.clock.now_ms();
            let expires = ttl_seconds.map(|ttl| now.saturating_add(u64::from(ttl) * 1000));
            let quota = u64::from(state.plugin.manifest.permissions.storage_mib) * 1024 * 1024;
            match services
                .store
                .plugin_kv(&state.plugin.id)
                .set(&key, &value, expires, quota, now)
            {
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
            if !state.in_call {
                return Ok(());
            }
            state
                .services()
                .store
                .plugin_kv(&state.plugin.id)
                .delete(&key)
                .map_err(|error| {
                    tracing::warn!(plugin = %state.plugin.id, %error, "storage.delete failed");
                    trap("storage is unavailable")
                })
        })
    }
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use std::sync::{Arc, Mutex};

    use ristle_domain::ManualClock;

    use super::*;
    use crate::host::{PluginContext, Services};
    use crate::test_support::{MemoryPluginStore, Publisher, mock_package};

    #[test]
    fn storage_is_unreachable_outside_a_call() {
        let clock = Arc::new(ManualClock::new(0));
        let store = Arc::new(MemoryPluginStore::new());
        let services = Arc::new(Services {
            clock: clock.clone(),
            store: store.clone(),
            net: ristle_net::Client::new(ristle_net::ClientConfig::default(), clock, None).unwrap(),
            locale: Mutex::new("en".to_owned()),
        });
        let manifest = mock_package(&Publisher::new(1), "").manifest().clone();
        let mut state = HostState::new(Arc::new(PluginContext::new(manifest, services)));
        // As during instantiation, an install's trial included.
        assert!(matches!(
            state.set("k".into(), b"v".to_vec(), None).unwrap(),
            Err(Error::Unauthorized(_))
        ));
        assert_eq!(state.get("k".into()).unwrap(), None);
        state.in_call = true;
        state.set("k".into(), b"v".to_vec(), None).unwrap().unwrap();
        assert_eq!(state.get("k".into()).unwrap(), Some(b"v".to_vec()));
        state.in_call = false;
        state.delete("k".into()).unwrap();
        state.in_call = true;
        assert_eq!(state.get("k".into()).unwrap(), Some(b"v".to_vec()));
    }
}
