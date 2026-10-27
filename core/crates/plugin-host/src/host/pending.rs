//! `types`, `http.rate-limit`, and the imports that arrive in M5 part 4b:
//! `files`, `socket` and `live`. Until then they trap, so a call that uses
//! one fails with `error.internal` and nothing pretends to work.

use std::time::Duration;

use wasmtime::component::Resource;

use super::{ConnectionHandle, HostState, TcpHandle, strings_fit, trap};
use crate::bindings::ristle::plugin::types::{Error, FilePath};
use crate::bindings::ristle::plugin::{connection, files, http, live, socket, types};

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

impl files::Host for HostState {
    fn list(
        &mut self,
        _: Resource<ConnectionHandle>,
        _: FilePath,
        _: Option<String>,
    ) -> wasmtime::Result<Result<Vec<files::Entry>, Error>> {
        Err(not_yet("files.list"))
    }

    fn read(
        &mut self,
        _: Resource<ConnectionHandle>,
        _: FilePath,
        _: u64,
        _: u32,
    ) -> wasmtime::Result<Result<Vec<u8>, Error>> {
        Err(not_yet("files.read"))
    }
}

impl socket::Host for HostState {}

impl socket::HostTcp for HostState {
    fn connect(
        &mut self,
        _: Resource<ConnectionHandle>,
        _: String,
        _: u16,
        _: bool,
    ) -> wasmtime::Result<Result<Resource<TcpHandle>, Error>> {
        Err(not_yet("socket.connect"))
    }

    fn send(&mut self, _: Resource<TcpHandle>, _: Vec<u8>) -> wasmtime::Result<Result<(), Error>> {
        Err(not_yet("socket.send"))
    }

    fn receive(
        &mut self,
        _: Resource<TcpHandle>,
        _: u32,
        _: u32,
    ) -> wasmtime::Result<Result<Vec<u8>, Error>> {
        Err(not_yet("socket.receive"))
    }

    fn drop(&mut self, rep: Resource<TcpHandle>) -> wasmtime::Result<()> {
        self.table.delete(rep)?;
        Ok(())
    }
}

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
