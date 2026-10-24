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
