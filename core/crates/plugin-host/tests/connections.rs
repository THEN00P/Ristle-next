//! Provider Connections: data, account and grants, the Credential Record,
//! requests through `ristle-net`, and generations (M5, part 4a). HTTP goes
//! to the local test server, reached through a Server Grant as a
//! self-hosted server would be.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use ristle_domain::{ConnectionId, ManualClock};
use ristle_net::test_support::{MemoryCacheStore, TestResponse, TestServer};
use ristle_net::{Client, ClientConfig};
use ristle_plugin_host::test_support::{
    MemoryPluginStore, MemoryVault, Publisher, connection_info, mock_package,
};
use ristle_plugin_host::{
    Account, Backend, ConnectionInfo, Engine, EngineConfig, HostPorts, HostSettings, LoadedPlugin,
    PluginError, PluginHost, ServerGrant,
};

const NOW: u64 = 1_800_000_000_000;

struct Harness {
    plugin: LoadedPlugin,
    vault: MemoryVault,
    cache: Arc<MemoryCacheStore>,
}

fn harness(permissions: &str) -> Harness {
    let clock = Arc::new(ManualClock::new(NOW));
    let cache = Arc::new(MemoryCacheStore::new());
    let vault = MemoryVault::new();
    let net = Client::new(ClientConfig::default(), clock.clone(), Some(cache.clone())).unwrap();
    let host = PluginHost::new(
        Engine::new(EngineConfig {
            backend: Backend::Cranelift,
            artifacts: None,
        })
        .unwrap(),
        HostPorts {
            clock,
            store: Arc::new(MemoryPluginStore::new()),
            vault: Arc::new(vault.clone()),
            net,
        },
        HostSettings::default(),
    )
    .unwrap();
    let plugin = host
        .load(&mock_package(&Publisher::new(1), permissions))
        .unwrap();
    Harness {
        plugin,
        vault,
        cache,
    }
}

fn c(id: &str) -> ConnectionId {
    ConnectionId::new(id)
}

/// A connection granted the test server.
fn granted(id: &str, generation: u64, server: &TestServer) -> ConnectionInfo {
    ConnectionInfo {
        server: Some(ServerGrant {
            name: "Home".into(),
            urls: vec![server.origin()],
            origins: vec![server.granted()],
        }),
        ..connection_info(id, generation)
    }
}

async fn run(
    plugin: &LoadedPlugin,
    connection: &str,
    command: &str,
) -> Result<Vec<String>, PluginError> {
    plugin
        .tracks(&c(connection), vec![command.to_owned()])
        .await?
        .into_iter()
        .map(|track| track.map(|track| track.title))
        .collect()
}

/// A command whose result has `count` Tracks: the ids after the first are
/// padding.
async fn run_n(
    plugin: &LoadedPlugin,
    connection: &str,
    command: &str,
    count: usize,
) -> Result<Vec<String>, PluginError> {
    let ids = std::iter::once(command.to_owned())
        .chain((1..count).map(|index| format!("pad{index}")))
        .collect();
    plugin
        .tracks(&c(connection), ids)
        .await?
        .into_iter()
        .map(|track| track.map(|track| track.title))
        .collect()
}

async fn one(
    plugin: &LoadedPlugin,
    connection: &str,
    command: &str,
) -> Result<String, PluginError> {
    Ok(run(plugin, connection, command).await?.remove(0))
}

#[tokio::test]
async fn calls_need_a_registered_connection() {
    let h = harness("");
    let error = one(&h.plugin, "unknown", "count").await.unwrap_err();
    assert!(
        matches!(error, PluginError::Internal(ref text) if text.contains("unknown")),
        "{error:?}"
    );
    h.plugin
        .set_connection(connection_info("", 0))
        .await
        .unwrap();
    assert_eq!(one(&h.plugin, "", "id").await.unwrap(), "");
}

#[tokio::test]
async fn the_connection_reports_its_account_and_grants() {
    let h = harness("servers = true\nfolders = true");
    let server = TestServer::start(|_| TestResponse::ok("")).await.unwrap();
    let mut info = granted("c1", 1, &server);
    info.account = Some(Account {
        id: "u1".into(),
        name: "Ada".into(),
        avatar: Vec::new(),
    });
    info.folders = vec![ristle_plugin_host::MusicFolder {
        id: "f1".into(),
        name: "Music".into(),
    }];
    h.plugin.set_connection(info).await.unwrap();
    assert_eq!(one(&h.plugin, "c1", "account").await.unwrap(), "Ada");
    assert_eq!(one(&h.plugin, "c1", "server").await.unwrap(), "Home");
    assert_eq!(one(&h.plugin, "c1", "folders").await.unwrap(), "1");
    assert_eq!(one(&h.plugin, "c1", "id").await.unwrap(), "c1");
}

#[tokio::test]
async fn connection_data_is_per_connection() {
    let h = harness("");
    for id in ["c1", "c2"] {
        h.plugin
            .set_connection(connection_info(id, 1))
            .await
            .unwrap();
    }
    assert_eq!(
        one(&h.plugin, "c1", "data:ids=a,b").await.unwrap(),
        "stored"
    );
    assert_eq!(one(&h.plugin, "c1", "data:ids").await.unwrap(), "a,b");
    assert_eq!(one(&h.plugin, "c2", "data:ids").await.unwrap(), "none");
}

#[tokio::test]
async fn a_new_generation_starts_clean() {
    let h = harness("");
    h.plugin
        .set_connection(connection_info("c1", 1))
        .await
        .unwrap();
    one(&h.plugin, "c1", "data:ids=a").await.unwrap();
    one(&h.plugin, "c1", "set-credential:v1").await.unwrap();
    assert_eq!(one(&h.plugin, "c1", "count").await.unwrap(), "3");
    // Registering again with the same generation, as at app start, keeps it all.
    h.plugin
        .set_connection(connection_info("c1", 1))
        .await
        .unwrap();
    assert_eq!(one(&h.plugin, "c1", "data:ids").await.unwrap(), "a");
    // A new generation: data gone, instance fresh; the record is the
    // accounts flow's to replace.
    h.plugin
        .set_connection(connection_info("c1", 2))
        .await
        .unwrap();
    assert_eq!(one(&h.plugin, "c1", "data:ids").await.unwrap(), "none");
    assert_eq!(one(&h.plugin, "c1", "count").await.unwrap(), "2");
}

#[tokio::test]
async fn credentials_live_in_the_vault_and_writes_compare_and_set() {
    let h = harness("");
    h.plugin
        .set_connection(connection_info("c1", 1))
        .await
        .unwrap();
    assert_eq!(one(&h.plugin, "c1", "credential").await.unwrap(), "none");
    assert_eq!(
        one(&h.plugin, "c1", "set-credential:v1").await.unwrap(),
        "set"
    );
    assert_eq!(one(&h.plugin, "c1", "credential").await.unwrap(), "v1");
    assert_eq!(
        h.vault.keys(),
        vec!["ristle.credential.dev.ristle.mock.c1".to_owned()]
    );
    // Signing out deletes it, with the connection's data.
    h.plugin.remove_connection(&c("c1")).await.unwrap();
    assert!(h.vault.keys().is_empty());
}

#[tokio::test]
async fn a_stale_read_modify_write_fails_with_conflict() {
    let h = harness("servers = true");
    let server = TestServer::start(|request| {
        if request.path == "/slow" {
            TestResponse {
                delay: Duration::from_millis(800),
                ..TestResponse::ok("")
            }
        } else {
            TestResponse::ok("")
        }
    })
    .await
    .unwrap();
    h.plugin
        .set_connection(granted("c1", 1, &server))
        .await
        .unwrap();
    let slow = format!("slow-write:stale:{}/slow", server.origin());
    let plugin = h.plugin.clone();
    let writer = tokio::spawn(async move { one(&plugin, "c1", &slow).await });
    tokio::time::sleep(Duration::from_millis(200)).await;
    // Another call writes while the slow one waits.
    assert_eq!(
        one(&h.plugin, "c1", "set-credential:fresh").await.unwrap(),
        "set"
    );
    let error = writer.await.unwrap().unwrap_err();
    assert!(matches!(error, PluginError::Conflict(_)), "{error:?}");
    assert_eq!(one(&h.plugin, "c1", "credential").await.unwrap(), "fresh");
}

#[tokio::test]
async fn requests_reach_a_granted_server_with_the_credential_attached() {
    let h = harness("servers = true");
    let server = TestServer::start(|request| {
        TestResponse::ok(
            request
                .header("authorization")
                .unwrap_or("anonymous")
                .to_owned(),
        )
    })
    .await
    .unwrap();
    h.plugin
        .set_connection(granted("c1", 1, &server))
        .await
        .unwrap();
    let url = format!("{}/me", server.origin());
    assert_eq!(
        one(&h.plugin, "c1", &format!("fetch-bypass:{url}"))
            .await
            .unwrap(),
        "200 anonymous cached=false sequence=1"
    );
    one(&h.plugin, "c1", "set-credential:token:127.0.0.1:Bearer abc")
        .await
        .unwrap();
    assert_eq!(
        one(&h.plugin, "c1", &format!("fetch-bypass:{url}"))
            .await
            .unwrap(),
        "200 Bearer abc cached=false sequence=2"
    );
}

#[tokio::test]
async fn a_401_from_a_credentialed_host_is_unauthorized() {
    let h = harness("servers = true");
    let server = TestServer::start(|_| TestResponse::status(401))
        .await
        .unwrap();
    h.plugin
        .set_connection(granted("c1", 1, &server))
        .await
        .unwrap();
    let url = format!("{}/me", server.origin());
    // Without a credential for the host, a 401 is just a response.
    assert_eq!(
        one(&h.plugin, "c1", &format!("fetch-bypass:{url}"))
            .await
            .unwrap(),
        "401  cached=false sequence=1"
    );
    one(&h.plugin, "c1", "set-credential:token:127.0.0.1:Bearer abc")
        .await
        .unwrap();
    assert!(matches!(
        one(&h.plugin, "c1", &format!("fetch-bypass:{url}")).await,
        Err(PluginError::Unauthorized(_))
    ));
}

#[tokio::test]
async fn undeclared_hosts_are_internal_errors() {
    let h = harness("");
    let server = TestServer::start(|_| TestResponse::ok("")).await.unwrap();
    // No grant for this connection: the server is not reachable.
    h.plugin
        .set_connection(connection_info("c1", 1))
        .await
        .unwrap();
    let error = one(&h.plugin, "c1", &format!("fetch:{}/x", server.origin()))
        .await
        .unwrap_err();
    assert!(matches!(error, PluginError::Internal(_)), "{error:?}");
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn cached_responses_are_scoped_to_the_generation() {
    let h = harness("servers = true");
    let server =
        TestServer::start(|_| TestResponse::ok("x").header("Cache-Control", "max-age=600"))
            .await
            .unwrap();
    h.plugin
        .set_connection(granted("c1", 1, &server))
        .await
        .unwrap();
    let fetch = format!("fetch:{}/a", server.origin());
    assert_eq!(
        one(&h.plugin, "c1", &fetch).await.unwrap(),
        "200 x cached=false sequence=1"
    );
    // Served from the cache: it did not reach the Provider, so no sequence.
    assert_eq!(
        one(&h.plugin, "c1", &fetch).await.unwrap(),
        "200 x cached=true sequence=0"
    );
    assert_eq!(h.cache.len(), 1);
    // A new generation drops the old generation's cache.
    h.plugin
        .set_connection(granted("c1", 2, &server))
        .await
        .unwrap();
    assert!(h.cache.is_empty());
    assert_eq!(
        one(&h.plugin, "c1", &fetch).await.unwrap(),
        "200 x cached=false sequence=2"
    );
}

#[tokio::test]
async fn fetch_all_runs_concurrently_and_is_bounded() {
    let h = harness("servers = true");
    let server = TestServer::start(|request| {
        if request.path == "/big" {
            TestResponse::ok(Bytes::from(vec![b'x'; 12 * 1024 * 1024]))
        } else {
            TestResponse {
                delay: Duration::from_millis(300),
                ..TestResponse::ok("ok")
            }
        }
    })
    .await
    .unwrap();
    h.plugin
        .set_connection(granted("c1", 1, &server))
        .await
        .unwrap();
    let started = std::time::Instant::now();
    let titles = run_n(
        &h.plugin,
        "c1",
        &format!("fetch-all:4:{}/slow", server.origin()),
        4,
    )
    .await
    .unwrap();
    assert_eq!(titles.len(), 4);
    assert!(
        titles.iter().all(|title| title.starts_with("200 ok")),
        "{titles:?}"
    );
    assert!(
        started.elapsed() < Duration::from_millis(1_000),
        "ran one by one"
    );
    // More than 16 fail the whole call.
    assert!(matches!(
        run_n(
            &h.plugin,
            "c1",
            &format!("fetch-all:17:{}/slow", server.origin()),
            17
        )
        .await,
        Err(PluginError::Internal(_))
    ));
    // Bodies beyond 32 MiB together fail individually, the first ones kept.
    let titles = run_n(
        &h.plugin,
        "c1",
        &format!("fetch-all:3:{}/big", server.origin()),
        3,
    )
    .await
    .unwrap();
    let failed = titles
        .iter()
        .filter(|title| title.contains("exceed 32 MiB"))
        .count();
    assert_eq!(failed, 1, "{titles:?}");
}

#[tokio::test]
async fn a_call_retired_by_a_new_generation_keeps_nothing() {
    let h = harness("servers = true");
    let server = TestServer::start(|_| TestResponse {
        delay: Duration::from_millis(600),
        ..TestResponse::ok("old account")
    })
    .await
    .unwrap();
    h.plugin
        .set_connection(granted("c1", 1, &server))
        .await
        .unwrap();
    let plugin = h.plugin.clone();
    let url = format!("fetch-bypass:{}/me", server.origin());
    let call = tokio::spawn(async move { one(&plugin, "c1", &url).await });
    tokio::time::sleep(Duration::from_millis(200)).await;
    h.plugin
        .set_connection(granted("c1", 2, &server))
        .await
        .unwrap();
    let error = call.await.unwrap().unwrap_err();
    assert!(
        matches!(
            error,
            PluginError::Unauthorized(_) | PluginError::Internal(_)
        ),
        "{error:?}"
    );
    // The instance that served the old generation is gone.
    assert_eq!(h.plugin.idle_instances(&c("c1")), 0);
}

#[tokio::test]
async fn retiring_the_plugin_retires_running_calls() {
    let h = harness("servers = true");
    let server = TestServer::start(|_| TestResponse {
        delay: Duration::from_millis(600),
        ..TestResponse::ok("")
    })
    .await
    .unwrap();
    h.plugin
        .set_connection(granted("c1", 1, &server))
        .await
        .unwrap();
    let plugin = h.plugin.clone();
    let url = format!("fetch-bypass:{}/x", server.origin());
    let call = tokio::spawn(async move { one(&plugin, "c1", &url).await });
    tokio::time::sleep(Duration::from_millis(200)).await;
    h.plugin.retire_calls();
    assert!(call.await.unwrap().is_err());
    // Calls after it run normally.
    assert_eq!(one(&h.plugin, "c1", "count").await.unwrap(), "1");
}
