//! `connection.fetch` and `fetch-all` on `ristle-net`.
//!
//! What a Plugin may reach is its manifest's `hosts` and the origins of the
//! connection's Server Grant; the client enforces HTTPS, public addresses,
//! the redirect rules and the header bounds. The host adds the Credential
//! Record's `attach` rules per hop, keys the cache by Plugin, connection
//! and generation, numbers every response that reached the Provider, and
//! turns a 401 from a host the credential covers into `error.unauthorized`.

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use ristle_domain::ConnectionId;
use ristle_net::{
    Attach, BodyBudget, CacheGate, CacheOverride, CachePolicy, CacheScope, Header, Reach, Request,
    Sequencer,
};
use url::Url;

use super::{CallContext, HostState, MAX_PAYLOAD, MAX_STRING};
use crate::bindings::ristle::plugin::http;
use crate::bindings::ristle::plugin::types::Error;
use crate::connections::ParsedRule;

/// Requests per `fetch-all`.
pub(crate) const MAX_BATCH: usize = 16;
/// Response bodies of one `fetch-all` together.
pub(crate) const BATCH_BODY_BYTES: u64 = 32 * 1024 * 1024;

/// Lets a response into the cache only while its call's generation is
/// current, under the lock a generation change takes.
struct GenerationGate {
    plugin: Arc<super::PluginContext>,
    connection: ConnectionId,
    generation: u64,
    retirements: u64,
}

impl CacheGate for GenerationGate {
    fn commit(&self, write: &mut dyn FnMut()) {
        self.plugin
            .connections
            .commit(&self.connection, self.generation, self.retirements, write);
    }
}

/// Numbers a connection's responses as their headers arrive.
struct ResponseSequence {
    store: Arc<dyn crate::ports::PluginStore>,
    plugin: ristle_domain::PluginId,
    connection: ConnectionId,
}

impl Sequencer for ResponseSequence {
    fn next(&self) -> Result<u64, String> {
        self.store
            .next_response_sequence(&self.plugin, &self.connection)
            .map_err(|error| error.to_string())
    }
}

/// A request ready to send, or why it is refused before anything is sent.
pub(crate) struct Prepared {
    request: Request,
    rules: Arc<Vec<ParsedRule>>,
}

/// The Credential Record's `attach` rules, applied to each hop's host.
struct CredentialAttach {
    rules: Arc<Vec<ParsedRule>>,
}

impl CredentialAttach {
    fn matching<'a>(&'a self, url: &'a Url) -> impl Iterator<Item = &'a ParsedRule> + 'a {
        let host = url.host_str().unwrap_or_default();
        self.rules.iter().filter(move |rule| rule.covers(host))
    }
}

impl Attach for CredentialAttach {
    fn headers(&self, url: &Url) -> Vec<Header> {
        self.matching(url)
            .flat_map(|rule| rule.headers.iter().cloned())
            .collect()
    }

    fn query(&self, url: &Url) -> Vec<(String, String)> {
        self.matching(url)
            .flat_map(|rule| rule.query.iter().cloned())
            .collect()
    }
}

impl HostState {
    /// Checks a Plugin's request and turns it into a client request.
    pub(crate) fn prepare(
        &self,
        call: &CallContext,
        request: http::Request,
        budget: Option<BodyBudget>,
    ) -> Result<Prepared, Error> {
        let internal = |why: &str| Error::Internal(why.to_owned());
        let strings_fit = request.url.len() <= MAX_STRING
            && request
                .headers
                .iter()
                .all(|(name, value)| name.len() <= MAX_STRING && value.len() <= MAX_STRING)
            && match &request.cache {
                http::Cache::Override(rule) => {
                    rule.key.as_ref().is_none_or(|key| key.len() <= MAX_STRING)
                }
                _ => true,
            };
        if !strings_fit {
            return Err(internal("a request string exceeds 64 KiB"));
        }
        if request.body.len() > MAX_PAYLOAD {
            return Err(internal("a request body exceeds 16 MiB"));
        }
        let url = Url::parse(&request.url).map_err(|_| internal("the request URL is invalid"))?;
        let plugin = &self.plugin;
        let info = plugin
            .connections
            .info_at(&call.connection, call.generation, call.retirements)
            .ok_or_else(|| Error::Unauthorized("this call was retired".to_owned()))?;
        let mut reach = Reach::hosts(plugin.manifest.permissions.hosts.clone());
        if let Some(server) = &info.server {
            reach = reach.with_granted(server.origins.clone());
        }
        let credential = plugin
            .connections
            .credential(
                plugin.services.vault.as_ref(),
                &call.connection,
                call.generation,
                call.retirements,
            )
            .map_err(|error| Error::Transient(format!("credentials unavailable: {error}")))?
            // `None` is a retired call, which sends nothing.
            .ok_or_else(|| Error::Unauthorized("this call was retired".to_owned()))?
            .0;
        let rules = Arc::new(
            credential
                .and_then(|record| record.parsed_rules())
                .unwrap_or_default(),
        );
        let scope = CacheScope {
            plugin: plugin.id.clone(),
            connection: call.connection.clone(),
            generation: call.generation,
        };
        let cache = match request.cache {
            http::Cache::Http => CachePolicy::Http(scope),
            http::Cache::Bypass => CachePolicy::Bypass,
            http::Cache::Override(rule) => CachePolicy::Override(
                scope,
                CacheOverride {
                    ttl: Duration::from_secs(u64::from(rule.ttl_seconds)),
                    stale: Duration::from_secs(u64::from(rule.stale_seconds)),
                    key: rule.key,
                },
            ),
        };
        let mut timeout = ristle_net::MAX_TIMEOUT.min(self.clock.remaining());
        if let Some(ms) = request.timeout_ms {
            timeout = timeout.min(Duration::from_millis(u64::from(ms)));
        }
        if timeout.is_zero() {
            return Err(Error::Transient("the call has no time left".to_owned()));
        }
        let mut prepared = Request::new(method(request.method), url, Arc::new(reach));
        prepared.headers = request.headers;
        prepared.body = Bytes::from(request.body);
        prepared.attach = Some(Arc::new(CredentialAttach {
            rules: rules.clone(),
        }));
        prepared.cache = cache;
        prepared.revalidate = call.refresh;
        prepared.timeout = timeout;
        prepared.budget = budget;
        prepared.gate = Some(Arc::new(GenerationGate {
            plugin: self.plugin.clone(),
            connection: call.connection.clone(),
            generation: call.generation,
            retirements: call.retirements,
        }));
        prepared.sequencer = Some(Arc::new(ResponseSequence {
            store: plugin.services.store.clone(),
            plugin: plugin.id.clone(),
            connection: call.connection.clone(),
        }));
        Ok(Prepared {
            request: prepared,
            rules,
        })
    }
}

/// Sends a prepared request.
pub(crate) async fn send(
    net: ristle_net::Client,
    prepared: Prepared,
) -> Result<http::Response, Error> {
    let Prepared { request, rules } = prepared;
    let response = net.send(request).await.map_err(error)?;
    let host = response.url.host_str().unwrap_or_default().to_owned();
    if response.status == 401 && rules.iter().any(|rule| rule.covers(&host)) {
        return Err(Error::Unauthorized(format!("{host} answered 401")));
    }
    // Numbered when its headers arrived; a response served from the cache
    // did not reach the Provider and has none.
    let sequence = response.sequence.unwrap_or(0);
    Ok(http::Response {
        status: response.status,
        headers: response.headers,
        body: response.body.to_vec(),
        cached: response.cached,
        received: response.received,
        sequence,
    })
}

fn method(method: http::Method) -> ristle_net::Method {
    match method {
        http::Method::Get => ristle_net::Method::Get,
        http::Method::Head => ristle_net::Method::Head,
        http::Method::Post => ristle_net::Method::Post,
        http::Method::Put => ristle_net::Method::Put,
        http::Method::Patch => ristle_net::Method::Patch,
        http::Method::Delete => ristle_net::Method::Delete,
    }
}

/// A client error as the WIT `error`.
fn error(error: ristle_net::Error) -> Error {
    match error {
        ristle_net::Error::NotAllowed(why) => Error::Internal(why),
        ristle_net::Error::RateLimited { retry_after_ms } => {
            Error::RateLimited(retry_after_ms.map(|ms| u32::try_from(ms).unwrap_or(u32::MAX)))
        }
        ristle_net::Error::Timeout => Error::Transient("timed out".to_owned()),
        // A body over the batch's budget is not kept.
        ristle_net::Error::BudgetSpent => {
            Error::Internal("the batch's response bodies exceed 32 MiB".to_owned())
        }
        other => Error::Transient(other.to_string()),
    }
}
