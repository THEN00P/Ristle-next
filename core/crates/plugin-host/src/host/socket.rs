//! `socket`: raw TCP, optionally with TLS, for protocols that are not
//! HTTP. Only to the manifest's `sockets` (exact `host:port`), only to
//! public addresses, at most [`MAX_SOCKETS`] per Plugin across its
//! instances. A socket belongs to its instance and its connection's
//! generation: it closes with the instance, serves only calls for its own
//! connection, and is closed once that generation ends or the Plugin is
//! retired, even in the middle of a send or receive.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use wasmtime::component::Resource;

use super::{ConnectionHandle, HostState, MAX_PAYLOAD, PluginContext, strings_fit, too_large};
use crate::bindings::ristle::plugin::socket;
use crate::bindings::ristle::plugin::types::Error;
use crate::package::SocketTarget;

/// Open sockets per Plugin.
pub(crate) const MAX_SOCKETS: usize = 4;
const CONNECT_TIME: Duration = Duration::from_secs(15);
const SEND_TIME: Duration = Duration::from_secs(30);
const RECEIVE_TIME: Duration = Duration::from_secs(10);
const RECEIVE_BYTES: u32 = 1024 * 1024;

/// Counts a socket against its Plugin's limit for as long as it exists.
struct Slot(Arc<PluginContext>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.open_sockets.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Slot {
    fn take(plugin: &Arc<PluginContext>) -> Option<Self> {
        plugin
            .open_sockets
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |open| {
                (open < MAX_SOCKETS).then_some(open + 1)
            })
            .ok()
            .map(|_| Self(plugin.clone()))
    }
}

/// An open stream and the slot it holds: closing it frees the slot.
struct Open {
    stream: Box<dyn ristle_net::Socket>,
    _slot: Slot,
}

/// The host side of a `socket.tcp` resource.
// `pub` because the generated bindings re-export the types they map
// resources to; the module itself is private.
#[allow(unreachable_pub)]
pub struct TcpHandle {
    open: Option<Open>,
    connection: ristle_domain::ConnectionId,
    generation: u64,
    retirements: u64,
}

impl std::fmt::Debug for TcpHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TcpHandle")
            .field("connection", &self.connection)
            .field("open", &self.open.is_some())
            .finish_non_exhaustive()
    }
}

fn retired() -> Error {
    Error::Unauthorized("this call was retired".to_owned())
}

impl HostState {
    /// The socket, if the running call may use it: a call for its own
    /// connection, under its generation. A socket whose generation ended is
    /// closed.
    fn usable_socket(&mut self, socket: &Resource<TcpHandle>) -> Result<&mut TcpHandle, Error> {
        let call = self.live_call().ok_or_else(retired)?;
        let current = self.plugin.connections.generation(&call.connection);
        let handle = self
            .table
            .get_mut(socket)
            .map_err(|_| Error::Internal("no such socket".to_owned()))?;
        if handle.connection != call.connection {
            return Err(Error::Internal(
                "a socket serves only calls for its own connection".to_owned(),
            ));
        }
        if current != Some(handle.generation) || handle.retirements != call.retirements {
            handle.open = None;
            return Err(retired());
        }
        if handle.open.is_none() {
            return Err(Error::Transient("the socket is closed".to_owned()));
        }
        Ok(handle)
    }
}

impl socket::Host for HostState {}

impl socket::HostTcp for HostState {
    fn connect(
        &mut self,
        _: Resource<ConnectionHandle>,
        host: String,
        port: u16,
        tls: bool,
    ) -> wasmtime::Result<Result<Resource<TcpHandle>, Error>> {
        if !strings_fit(&[&host]) {
            return Ok(Err(too_large("a host")));
        }
        self.timed(|state| {
            let Some(call) = state.live_call() else {
                return Ok(Err(retired()));
            };
            let plugin = state.plugin.clone();
            let target = SocketTarget {
                host: host.to_ascii_lowercase(),
                port,
            };
            if !plugin.manifest.permissions.sockets.contains(&target) {
                tracing::warn!(plugin = %plugin.id, %target, "refused an undeclared socket");
                return Ok(Err(Error::Internal(format!(
                    "{target} is not a declared socket"
                ))));
            }
            let Some(slot) = Slot::take(&plugin) else {
                return Ok(Err(Error::Internal(format!(
                    "a plugin may hold at most {MAX_SOCKETS} sockets"
                ))));
            };
            let services = &plugin.services;
            let time = CONNECT_TIME.min(state.clock.remaining());
            let origin = services.socket_origin(&target);
            let connected = services.runtime.block_on(tokio::time::timeout(
                time,
                services
                    .net
                    .connect_socket(&target.host, port, tls, origin.as_ref()),
            ));
            let stream = match connected {
                Ok(Ok(stream)) => stream,
                Ok(Err(ristle_net::Error::NotAllowed(why))) => {
                    return Ok(Err(Error::Internal(why)));
                }
                Ok(Err(error)) => return Ok(Err(Error::Transient(error.to_string()))),
                Err(_) => return Ok(Err(Error::Transient("connecting timed out".to_owned()))),
            };
            if state.retired() {
                return Ok(Err(retired()));
            }
            let handle = TcpHandle {
                open: Some(Open {
                    stream,
                    _slot: slot,
                }),
                connection: call.connection,
                generation: call.generation,
                retirements: call.retirements,
            };
            Ok(Ok(state.table.push(handle)?))
        })
    }

    fn send(
        &mut self,
        self_: Resource<TcpHandle>,
        data: Vec<u8>,
    ) -> wasmtime::Result<Result<(), Error>> {
        if data.len() > MAX_PAYLOAD {
            return Ok(Err(too_large("socket data")));
        }
        Ok(self.timed(|state| {
            let plugin = state.plugin.clone();
            let time = SEND_TIME.min(state.clock.remaining());
            let handle = state.usable_socket(&self_)?;
            let ended =
                plugin
                    .connections
                    .ended(&handle.connection, handle.generation, handle.retirements);
            let Some(open) = handle.open.as_mut() else {
                return Err(Error::Transient("the socket is closed".to_owned()));
            };
            let stream = &mut open.stream;
            let sent = plugin.services.runtime.block_on(async {
                tokio::select! {
                    sent = tokio::time::timeout(time, async {
                        stream.write_all(&data).await?;
                        stream.flush().await
                    }) => Some(sent),
                    () = ended => None,
                }
            });
            match sent {
                Some(Ok(Ok(()))) => Ok(()),
                // How much was sent is unknown: the socket is closed.
                Some(Ok(Err(error))) => {
                    handle.open = None;
                    Err(Error::Transient(format!("sending failed: {error}")))
                }
                Some(Err(_)) => {
                    handle.open = None;
                    Err(Error::Transient("sending timed out".to_owned()))
                }
                None => {
                    handle.open = None;
                    Err(retired())
                }
            }
        }))
    }

    fn receive(
        &mut self,
        self_: Resource<TcpHandle>,
        max: u32,
        timeout_ms: u32,
    ) -> wasmtime::Result<Result<Vec<u8>, Error>> {
        // Capped before anything is allocated.
        let max = max.min(RECEIVE_BYTES) as usize;
        Ok(self.timed(|state| {
            let plugin = state.plugin.clone();
            let time = Duration::from_millis(u64::from(timeout_ms))
                .min(RECEIVE_TIME)
                .min(state.clock.remaining());
            let handle = state.usable_socket(&self_)?;
            let ended =
                plugin
                    .connections
                    .ended(&handle.connection, handle.generation, handle.retirements);
            let Some(open) = handle.open.as_mut() else {
                return Err(Error::Transient("the socket is closed".to_owned()));
            };
            let stream = &mut open.stream;
            let mut buffer = vec![0u8; max];
            let received = plugin.services.runtime.block_on(async {
                tokio::select! {
                    read = tokio::time::timeout(time, stream.read(&mut buffer)) => Some(read),
                    () = ended => None,
                }
            });
            match received {
                Some(Ok(Ok(0))) if max > 0 => {
                    handle.open = None;
                    Err(Error::Transient("the peer closed the socket".to_owned()))
                }
                Some(Ok(Ok(read))) => {
                    buffer.truncate(read);
                    Ok(buffer)
                }
                Some(Ok(Err(error))) => {
                    handle.open = None;
                    Err(Error::Transient(format!("receiving failed: {error}")))
                }
                // Nothing within the timeout.
                Some(Err(_)) => Ok(Vec::new()),
                None => {
                    handle.open = None;
                    Err(retired())
                }
            }
        }))
    }

    fn drop(&mut self, rep: Resource<TcpHandle>) -> wasmtime::Result<()> {
        self.table.delete(rep)?;
        Ok(())
    }
}
