//! The `connection` resource: data, account, grants, the Credential
//! Record and requests.
//!
//! A retired call (see `HostState::retired`) reaches nothing: `get-data`,
//! `account`, `server` and `credential` return `none`, `folders` is empty,
//! and everything with an error result fails with `error.unauthorized`.

use wasmtime::component::Resource;

use super::http::{self, BATCH_BODY_BYTES, MAX_BATCH};
use super::{ConnectionHandle, HostState, MAX_PAYLOAD, strings_fit, too_large, trap};
use crate::bindings::ristle::plugin::connection::HostConnection;
use crate::bindings::ristle::plugin::http as wit_http;
use crate::bindings::ristle::plugin::types::{
    Account, Attach, Credential, Error, Image, MediaServer, MusicFolder, SettingValue,
};
use crate::connections::{AttachRule, CredentialRecord, WriteRefused};
use crate::ports::Stored;

fn retired() -> Error {
    Error::Unauthorized("this call was retired".to_owned())
}

impl HostState {
    /// The call's context, unless it is retired.
    fn live_call(&self) -> Option<super::CallContext> {
        (!self.retired()).then(|| self.call.clone()).flatten()
    }

    fn quota(&self) -> u64 {
        u64::from(self.plugin.manifest.permissions.storage_mib) * 1024 * 1024
    }
}

impl HostConnection for HostState {
    fn id(&mut self, self_: Resource<ConnectionHandle>) -> wasmtime::Result<String> {
        Ok(self.table.get(&self_)?.connection.as_str().to_owned())
    }

    fn get_data(
        &mut self,
        _: Resource<ConnectionHandle>,
        key: String,
    ) -> wasmtime::Result<Option<Vec<u8>>> {
        if !strings_fit(&[&key]) {
            return Err(trap("connection.get-data: key exceeds 64 KiB"));
        }
        self.timed(|state| {
            let Some(call) = state.live_call() else {
                return Ok(None);
            };
            let services = state.services();
            services
                .store
                .connection_kv(&call.connection)
                .get(&key, services.clock.now_ms())
                .map_err(|error| {
                    tracing::warn!(plugin = %state.plugin.id, %error, "connection.get-data failed");
                    trap("connection data is unavailable")
                })
        })
    }

    fn set_data(
        &mut self,
        _: Resource<ConnectionHandle>,
        key: String,
        value: Vec<u8>,
        ttl_seconds: Option<u32>,
    ) -> wasmtime::Result<Result<(), Error>> {
        if !strings_fit(&[&key]) {
            return Ok(Err(too_large("data key")));
        }
        if value.len() > MAX_PAYLOAD {
            return Ok(Err(too_large("data value")));
        }
        Ok(self.timed(|state| {
            let Some(call) = state.live_call() else {
                return Err(retired());
            };
            let services = state.services();
            let now = services.clock.now_ms();
            let expires = ttl_seconds.map(|ttl| now.saturating_add(u64::from(ttl) * 1000));
            match services.store.connection_kv(&call.connection).set(
                &key,
                &value,
                expires,
                state.quota(),
                now,
            ) {
                Ok(Stored::Yes) => Ok(()),
                Ok(Stored::OverQuota) => Err(Error::Internal("storage quota exceeded".to_owned())),
                Err(error) => {
                    tracing::warn!(plugin = %state.plugin.id, %error, "connection.set-data failed");
                    Err(Error::Transient(
                        "connection data is unavailable".to_owned(),
                    ))
                }
            }
        }))
    }

    fn delete_data(
        &mut self,
        _: Resource<ConnectionHandle>,
        key: String,
    ) -> wasmtime::Result<Result<(), Error>> {
        if !strings_fit(&[&key]) {
            return Ok(Err(too_large("data key")));
        }
        Ok(self.timed(|state| {
            let Some(call) = state.live_call() else {
                return Err(retired());
            };
            state
                .services()
                .store
                .connection_kv(&call.connection)
                .delete(&key)
                .map_err(|error| Error::Transient(format!("connection data: {error}")))
        }))
    }

    fn account(&mut self, _: Resource<ConnectionHandle>) -> wasmtime::Result<Option<Account>> {
        Ok(self.timed(|state| {
            let call = state.live_call()?;
            let account = state.plugin.connections.info(&call.connection)?.account?;
            Some(Account {
                id: account.id,
                name: account.name,
                avatar: account
                    .avatar
                    .into_iter()
                    .map(|image| Image {
                        url: image.url,
                        width: image.width,
                        height: image.height,
                    })
                    .collect(),
            })
        }))
    }

    fn server(&mut self, _: Resource<ConnectionHandle>) -> wasmtime::Result<Option<MediaServer>> {
        Ok(self.timed(|state| {
            let call = state.live_call()?;
            let server = state.plugin.connections.info(&call.connection)?.server?;
            Some(MediaServer {
                name: server.name,
                urls: server.urls,
            })
        }))
    }

    fn folders(&mut self, _: Resource<ConnectionHandle>) -> wasmtime::Result<Vec<MusicFolder>> {
        Ok(self.timed(|state| {
            let Some(call) = state.live_call() else {
                return Vec::new();
            };
            state
                .plugin
                .connections
                .info(&call.connection)
                .map(|info| {
                    info.folders
                        .into_iter()
                        .map(|folder| MusicFolder {
                            id: folder.id,
                            name: folder.name,
                        })
                        .collect()
                })
                .unwrap_or_default()
        }))
    }

    fn credential(
        &mut self,
        _: Resource<ConnectionHandle>,
    ) -> wasmtime::Result<Option<Credential>> {
        self.timed(|state| {
            let Some(call) = state.live_call() else {
                return Ok(None);
            };
            let plugin = &state.plugin;
            let read = plugin
                .connections
                .credential(
                    plugin.services.vault.as_ref(),
                    &call.connection,
                    call.generation,
                )
                .map_err(|error| {
                    tracing::warn!(plugin = %plugin.id, %error, "reading a credential failed");
                    trap("credentials are unavailable")
                })?;
            let Some((record, version)) = read else {
                return Ok(None);
            };
            if let Some(call) = &mut state.call {
                call.credential_base = version;
            }
            Ok(record.map(to_wit))
        })
    }

    fn set_credential(
        &mut self,
        _: Resource<ConnectionHandle>,
        credential: Credential,
    ) -> wasmtime::Result<Result<(), Error>> {
        Ok(self.timed(|state| {
            let Some(call) = state.live_call() else {
                return Err(retired());
            };
            let record = from_wit(credential);
            let Some(encoded) = record.encode() else {
                return Err(Error::Internal("the credential exceeds 64 KiB".to_owned()));
            };
            if record.parsed_rules().is_none() {
                return Err(Error::Internal(
                    "an attach rule names an invalid host".to_owned(),
                ));
            }
            let plugin = &state.plugin;
            match plugin.connections.write_credential(
                plugin.services.vault.as_ref(),
                &call.connection,
                call.generation,
                call.credential_base,
                record,
                &encoded,
            ) {
                Ok(version) => {
                    if let Some(call) = &mut state.call {
                        call.credential_base = version;
                    }
                    Ok(())
                }
                Err(WriteRefused::Conflict) => Err(Error::Conflict(
                    "the credential changed since this call read it".to_owned(),
                )),
                Err(WriteRefused::Retired) => Err(retired()),
                Err(WriteRefused::Vault(error)) => {
                    tracing::warn!(plugin = %plugin.id, %error, "storing a credential failed");
                    Err(Error::Transient(
                        "the credential store is unavailable".to_owned(),
                    ))
                }
            }
        }))
    }

    fn fetch(
        &mut self,
        _: Resource<ConnectionHandle>,
        request: wit_http::Request,
    ) -> wasmtime::Result<Result<wit_http::Response, Error>> {
        Ok(self.timed(|state| {
            let Some(call) = state.live_call() else {
                return Err(retired());
            };
            let prepared = state.prepare(&call, request, None).inspect_err(|error| {
                log_refusal(state, error);
            })?;
            let services = state.services();
            let response = services.runtime.block_on(http::send(
                services.net.clone(),
                services.store.clone(),
                call.connection.clone(),
                prepared,
            ));
            // A call retired while it waited keeps nothing it received.
            if state.retired() {
                return Err(retired());
            }
            response
        }))
    }

    fn fetch_all(
        &mut self,
        _: Resource<ConnectionHandle>,
        requests: Vec<wit_http::Request>,
    ) -> wasmtime::Result<Result<Vec<Result<wit_http::Response, Error>>, Error>> {
        Ok(self.timed(|state| {
            let Some(call) = state.live_call() else {
                return Err(retired());
            };
            if requests.len() > MAX_BATCH {
                return Err(Error::Internal(format!("more than {MAX_BATCH} requests")));
            }
            let bodies: usize = requests.iter().map(|request| request.body.len()).sum();
            if bodies > MAX_PAYLOAD {
                return Err(too_large("request bodies"));
            }
            let budget = ristle_net::BodyBudget::new(BATCH_BODY_BYTES);
            let services = state.services();
            let tasks: Vec<_> = requests
                .into_iter()
                .map(
                    |request| match state.prepare(&call, request, Some(budget.clone())) {
                        Ok(prepared) => Ok(services.runtime.spawn(http::send(
                            services.net.clone(),
                            services.store.clone(),
                            call.connection.clone(),
                            prepared,
                        ))),
                        Err(error) => {
                            log_refusal(state, &error);
                            Err(error)
                        }
                    },
                )
                .collect();
            let results = services.runtime.block_on(async {
                let mut results = Vec::with_capacity(tasks.len());
                for task in tasks {
                    results.push(match task {
                        Ok(handle) => handle.await.unwrap_or_else(|error| {
                            Err(Error::Internal(format!("request lost: {error}")))
                        }),
                        Err(error) => Err(error),
                    });
                }
                results
            });
            if state.retired() {
                return Err(retired());
            }
            Ok(results)
        }))
    }

    fn setting(
        &mut self,
        _: Resource<ConnectionHandle>,
        _: String,
    ) -> wasmtime::Result<Option<SettingValue>> {
        Err(trap(
            "connection.setting is not available in this version of the host",
        ))
    }

    fn drop(&mut self, rep: Resource<ConnectionHandle>) -> wasmtime::Result<()> {
        // The host owns connection resources and removes them after the
        // call; a guest drop of its borrow needs nothing.
        let _ = rep;
        Ok(())
    }
}

/// An undeclared host or a forbidden header is the Plugin's mistake: it
/// fails with `error.internal` and is logged.
fn log_refusal(state: &HostState, error: &Error) {
    if let Error::Internal(why) = error {
        tracing::warn!(plugin = %state.plugin.id, "refused a request: {why}");
    }
}

fn to_wit(record: CredentialRecord) -> Credential {
    Credential {
        scheme: record.scheme,
        access_token: record.access_token,
        refresh_token: record.refresh_token,
        expires_at: record.expires_at_ms,
        scopes: record.scopes,
        username: record.username,
        password: record.password,
        custom: record.custom,
        attach: record
            .attach
            .into_iter()
            .map(|rule| Attach {
                hosts: rule.hosts,
                headers: rule.headers,
                query: rule.query,
            })
            .collect(),
    }
}

fn from_wit(credential: Credential) -> CredentialRecord {
    CredentialRecord {
        scheme: credential.scheme,
        access_token: credential.access_token,
        refresh_token: credential.refresh_token,
        expires_at_ms: credential.expires_at,
        scopes: credential.scopes,
        username: credential.username,
        password: credential.password,
        custom: credential.custom,
        attach: credential
            .attach
            .into_iter()
            .map(|rule| AttachRule {
                hosts: rule.hosts,
                headers: rule.headers,
                query: rule.query,
            })
            .collect(),
    }
}
