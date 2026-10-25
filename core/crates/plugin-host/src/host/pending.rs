//! `types`, `http.rate-limit`, and `live`, which arrives with the
//! `queue-projection` export (M5, part 5). Until then `live` traps, so a
//! call that uses it fails with `error.internal` and nothing pretends to
//! work.

use std::time::Duration;

use super::{HostState, strings_fit, trap};
use crate::bindings::ristle::plugin::types::Error;
use crate::bindings::ristle::plugin::{connection, http, live, types};

fn not_yet(import: &str) -> wasmtime::Error {
    trap(&format!(
        "{import} is not available in this version of the host"
    ))
}

impl types::Host for HostState {}

impl http::Host for HostState {
    fn rate_limit(&mut self, host: String, retry_after_ms: u32) -> wasmtime::Result<()> {
        if !strings_fit(&[&host]) {
            return Err(trap("http.rate-limit: host exceeds 64 KiB"));
        }
        self.timed(|state| {
            // A retired call reaches nothing outside its instance.
            if state.retired() {
                return;
            }
            let host = host.to_ascii_lowercase();
            // Only a host the Plugin may reach: a Plugin cannot hold back
            // another Plugin's Provider.
            let permissions = &state.plugin.manifest.permissions;
            let declared = permissions
                .hosts
                .iter()
                .chain(&permissions.media_hosts)
                .any(|pattern| pattern.matches(&host));
            if declared {
                state
                    .services()
                    .net
                    .report_rate_limit(&host, Duration::from_millis(u64::from(retry_after_ms)));
            } else {
                tracing::warn!(plugin = %state.plugin.id, %host, "ignoring a rate limit for an undeclared host");
            }
        });
        Ok(())
    }
}

impl connection::Host for HostState {}

impl live::Host for HostState {
    fn open(&mut self, _: live::ChannelRequest) -> wasmtime::Result<Result<String, Error>> {
        Err(not_yet("live.open"))
    }

    fn reopen(
        &mut self,
        _: String,
        _: live::ChannelRequest,
    ) -> wasmtime::Result<Result<(), Error>> {
        Err(not_yet("live.reopen"))
    }

    fn send(&mut self, _: String, _: live::Frame) -> wasmtime::Result<Result<(), Error>> {
        Err(not_yet("live.send"))
    }

    fn close(&mut self, _: String) -> wasmtime::Result<()> {
        Err(not_yet("live.close"))
    }
}
