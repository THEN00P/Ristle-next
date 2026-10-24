//! The engine, instances and the first imports, against the mock Plugin
//! (`plugin-api/examples/mock-provider`), under Cranelift and with Pulley
//! forced.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use ristle_domain::{
    AttributeValue, Availability, Clock, ConnectionId, ManualClock, MediaDate, SystemClock, Track,
    TrackRef,
};
use ristle_net::{Client, ClientConfig};
use ristle_plugin_host::package::PackageError;
use ristle_plugin_host::test_support::{
    MemoryPluginStore, Publisher, Unsigned, mock_manifest, mock_package, mock_provider_component,
};
use ristle_plugin_host::{
    Backend, Engine, EngineConfig, Error, HostPorts, HostSettings, LoadedPlugin, PluginError,
    PluginHost,
};

const NOW: u64 = 1_800_000_000_000;

fn host_with(backend: Backend, artifacts: Option<std::path::PathBuf>) -> PluginHost {
    host_on(backend, artifacts, Arc::new(ManualClock::new(NOW)), 4)
}

/// A host on another clock: `spin:` waits on `system.now`, so it needs one
/// that moves.
fn host_on(
    backend: Backend,
    artifacts: Option<std::path::PathBuf>,
    clock: Arc<dyn Clock>,
    concurrent_calls: usize,
) -> PluginHost {
    let net = Client::new(ClientConfig::default(), clock.clone(), None).unwrap();
    let engine = Engine::new(EngineConfig { backend, artifacts }).unwrap();
    PluginHost::new(
        engine,
        HostPorts {
            clock,
            store: Arc::new(MemoryPluginStore::new()),
            net,
        },
        HostSettings {
            locale: "de-DE".to_owned(),
            concurrent_calls,
            ..HostSettings::default()
        },
    )
    .unwrap()
}

fn spinning_plugin(backend: Backend, concurrent_calls: usize) -> LoadedPlugin {
    host_on(backend, None, Arc::new(SystemClock), concurrent_calls)
        .load(&mock_package(&Publisher::new(1), ""))
        .unwrap()
}

fn plugin(backend: Backend) -> LoadedPlugin {
    host_with(backend, None)
        .load(&mock_package(&Publisher::new(1), ""))
        .unwrap()
}

fn connection(id: &str) -> ConnectionId {
    ConnectionId::new(id)
}

async fn titles(
    plugin: &LoadedPlugin,
    connection: &ConnectionId,
    ids: &[&str],
) -> Result<Vec<String>, PluginError> {
    let tracks = plugin
        .tracks(connection, ids.iter().map(|id| (*id).to_owned()).collect())
        .await?;
    tracks
        .into_iter()
        .map(|track| track.map(|track| track.title))
        .collect()
}

const BACKENDS: [Backend; 2] = [Backend::Cranelift, Backend::Pulley];

#[tokio::test]
async fn tracks_come_back_as_domain_tracks() {
    for backend in BACKENDS {
        let plugin = plugin(backend);
        let tracks = plugin
            .tracks(
                &connection("c1"),
                vec!["a".into(), "gone-1".into(), "b".into()],
            )
            .await
            .unwrap();
        assert_eq!(tracks.len(), 3);
        let first = tracks[0].as_ref().unwrap();
        assert_eq!(first.title, "Track a");
        assert_eq!(
            first.reference,
            TrackRef::catalog("mock".into(), "a".into()).unwrap()
        );
        assert_eq!(first.duration_ms, Some(180_000));
        assert!(matches!(tracks[1], Err(PluginError::Unavailable(_))));
        assert_eq!(tracks[2].as_ref().unwrap().title, "Track b");
    }
}

#[tokio::test]
async fn the_host_batches_by_the_manifests_limit() {
    let plugin = plugin(Backend::Cranelift);
    let ids: Vec<String> = (0..120).map(|index| format!("id{index}")).collect();
    let tracks = plugin.tracks(&connection("c1"), ids.clone()).await.unwrap();
    assert_eq!(tracks.len(), 120);
    assert_eq!(tracks[119].as_ref().unwrap().title, "Track id119");
    // Three batches of at most 50 ran on the same instance.
    assert_eq!(
        titles(&plugin, &connection("c1"), &["count"])
            .await
            .unwrap(),
        vec!["4"]
    );
}

#[tokio::test]
async fn fields_are_checked_on_the_way_in() {
    let plugin = plugin(Backend::Cranelift);
    let tracks = plugin
        .tracks(&connection("c1"), vec!["rich".into(), "link".into()])
        .await
        .unwrap();
    let rich: &Track = tracks[0].as_ref().unwrap();
    // Only HTTPS artwork on image-hosts.
    assert_eq!(rich.artwork.len(), 1);
    assert_eq!(rich.artwork[0].url, "https://i.mock.example/a.jpg");
    assert_eq!(rich.url.as_deref(), Some("https://example.com/track/rich"));
    // 2024-02-30 does not exist and is dropped, not invented.
    assert_eq!(rich.released, None);
    assert_eq!(
        rich.uploaded,
        Some(MediaDate::new(2024, Some(5), None).unwrap())
    );
    assert!(rich.explicit);
    assert!(!rich.capabilities.seekable);
    assert!(!rich.capabilities.live);
    assert_eq!(rich.availability, Availability::Available);
    let keys: Vec<&str> = rich
        .attributes
        .iter()
        .map(|attribute| attribute.key.as_str())
        .collect();
    assert_eq!(keys, vec!["genre", "mock:plays"]);
    assert_eq!(rich.attributes[1].value, AttributeValue::Number(12.0));
    let link: &Track = tracks[1].as_ref().unwrap();
    assert_eq!(link.url, None, "a non-HTTPS link is dropped");
    assert_eq!(
        link.availability,
        Availability::Unavailable {
            reason: Some("region".into())
        }
    );
}

#[tokio::test]
async fn results_over_the_bounds_fail_the_call() {
    let plugin = plugin(Backend::Cranelift);
    assert!(matches!(
        titles(&plugin, &connection("c1"), &["huge-title"]).await,
        Err(PluginError::Internal(_))
    ));
}

#[tokio::test]
async fn copying_a_result_is_bounded_before_it_happens() {
    let plugin = plugin(Backend::Cranelift);
    // 150 Tracks with 120 KiB of names are 18 MiB: over the hostcall fuel, so wasmtime
    // stops lifting before the host holds them, not the 4 MiB check after.
    let error = titles(&plugin, &connection("c1"), &["many:150"])
        .await
        .unwrap_err();
    assert!(
        matches!(error, PluginError::Internal(ref text) if text.contains("too much data")),
        "{error:?}"
    );
    // Within the fuel, the 4 MiB result bound still applies.
    // 50 of them (one batch) are 6 MiB.
    let mut ids = vec!["many:50".to_owned()];
    ids.extend((1..50).map(|index| format!("id{index}")));
    let error = plugin.tracks(&connection("c1"), ids).await.unwrap_err();
    assert!(
        matches!(error, PluginError::Internal(ref text) if text.contains("4 MiB")),
        "{error:?}"
    );
}

#[tokio::test]
async fn grant_holding_plugins_scope_their_tracks_to_the_connection() {
    let host = host_with(Backend::Cranelift, None);
    let plugin = host
        .load(&mock_package(&Publisher::new(1), "servers = true"))
        .unwrap();
    let tracks = plugin
        .tracks(&connection("c1"), vec!["a".into()])
        .await
        .unwrap();
    assert_eq!(
        tracks[0].as_ref().unwrap().reference,
        TrackRef::server_scoped("mock".into(), "a".into(), connection("c1")).unwrap()
    );
}

// Instances -------------------------------------------------------------------

#[tokio::test]
async fn an_instance_serves_its_connection_and_keeps_memory_between_calls() {
    for backend in BACKENDS {
        let plugin = plugin(backend);
        let c1 = connection("c1");
        assert_eq!(titles(&plugin, &c1, &["count"]).await.unwrap(), vec!["1"]);
        assert_eq!(titles(&plugin, &c1, &["count"]).await.unwrap(), vec!["2"]);
        assert_eq!(plugin.idle_instances(&c1), 1);
        // Another connection gets its own instance.
        assert_eq!(
            titles(&plugin, &connection("c2"), &["count"])
                .await
                .unwrap(),
            vec!["1"]
        );
        plugin.discard_connection_instances(&c1);
        assert_eq!(titles(&plugin, &c1, &["count"]).await.unwrap(), vec!["1"]);
    }
}

#[tokio::test]
async fn a_trap_fails_one_call_and_the_next_runs_on_a_fresh_instance() {
    for backend in BACKENDS {
        let plugin = plugin(backend);
        let c1 = connection("c1");
        assert_eq!(titles(&plugin, &c1, &["count"]).await.unwrap(), vec!["1"]);
        let error = titles(&plugin, &c1, &["trap"]).await.unwrap_err();
        assert!(matches!(error, PluginError::Internal(_)), "{error:?}");
        assert_eq!(plugin.idle_instances(&c1), 0);
        assert_eq!(titles(&plugin, &c1, &["count"]).await.unwrap(), vec!["1"]);
    }
}

#[tokio::test]
async fn an_endless_loop_is_stopped_by_the_epoch_deadline() {
    for backend in BACKENDS {
        let plugin = plugin(backend);
        let c1 = connection("c1");
        let started = Instant::now();
        let error = titles(&plugin, &c1, &["loop"]).await.unwrap_err();
        let elapsed = started.elapsed();
        assert!(
            matches!(error, PluginError::Internal(ref text) if text.contains("too long")),
            "{error:?}"
        );
        let budget = match backend {
            Backend::Cranelift => Duration::from_secs(2),
            Backend::Pulley => Duration::from_secs(20),
        };
        assert!(elapsed >= budget, "{backend:?} stopped after {elapsed:?}");
        assert!(
            elapsed < budget + Duration::from_secs(2),
            "{backend:?} took {elapsed:?}"
        );
        assert_eq!(titles(&plugin, &c1, &["count"]).await.unwrap(), vec!["1"]);
    }
}

#[tokio::test]
async fn guest_time_under_the_budget_passes() {
    let plugin = spinning_plugin(Backend::Cranelift, 4);
    assert_eq!(
        titles(&plugin, &connection("c1"), &["spin:1500"])
            .await
            .unwrap(),
        vec!["spun"]
    );
    assert!(matches!(
        titles(&plugin, &connection("c1"), &["spin:2500"]).await,
        Err(PluginError::Internal(_))
    ));
}

#[tokio::test]
async fn pulley_gets_ten_times_the_guest_time() {
    let plugin = spinning_plugin(Backend::Pulley, 4);
    assert_eq!(
        titles(&plugin, &connection("c1"), &["spin:2500"])
            .await
            .unwrap(),
        vec!["spun"]
    );
}

#[tokio::test]
async fn an_instance_whose_memory_grew_is_recycled() {
    let plugin = plugin(Backend::Cranelift);
    let c1 = connection("c1");
    assert_eq!(
        titles(&plugin, &c1, &["grow:2"]).await.unwrap(),
        vec!["grown"]
    );
    // A little growth keeps the instance.
    assert_eq!(titles(&plugin, &c1, &["count"]).await.unwrap(), vec!["2"]);
    assert_eq!(
        titles(&plugin, &c1, &["grow:9"]).await.unwrap(),
        vec!["grown"]
    );
    assert_eq!(plugin.idle_instances(&c1), 0);
    assert_eq!(titles(&plugin, &c1, &["count"]).await.unwrap(), vec!["1"]);
}

#[tokio::test]
async fn memory_beyond_the_limit_is_refused() {
    let plugin = plugin(Backend::Cranelift);
    let c1 = connection("c1");
    // 70 MiB is over the 64 MiB limit: the allocation fails and the Plugin
    // aborts.
    assert!(matches!(
        titles(&plugin, &c1, &["grow:70"]).await,
        Err(PluginError::Internal(_))
    ));
    assert_eq!(titles(&plugin, &c1, &["count"]).await.unwrap(), vec!["1"]);
}

// Imports ---------------------------------------------------------------------

#[tokio::test]
async fn system_reports_the_clock_and_locale() {
    let plugin = plugin(Backend::Cranelift);
    assert_eq!(
        titles(&plugin, &connection("c1"), &["system", "locale"])
            .await
            .unwrap(),
        vec![NOW.to_string(), "de-DE".to_owned()]
    );
    // Logging works and is bounded; a flood never fails the call.
    assert_eq!(
        titles(&plugin, &connection("c1"), &["log:1000"])
            .await
            .unwrap(),
        vec!["logged"]
    );
}

#[tokio::test]
async fn crypto_runs_on_the_host() {
    let plugin = plugin(Backend::Pulley);
    assert_eq!(
        titles(&plugin, &connection("c1"), &["crypto"])
            .await
            .unwrap(),
        vec!["ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"]
    );
    let random = titles(&plugin, &connection("c1"), &["random"])
        .await
        .unwrap();
    assert_eq!(random[0].len(), 32);
    assert!(matches!(
        titles(&plugin, &connection("c1"), &["too-random"]).await,
        Err(PluginError::Internal(_))
    ));
}

#[tokio::test]
async fn storage_outlives_instances_and_honours_the_quota() {
    let plugin = plugin(Backend::Cranelift);
    let c1 = connection("c1");
    assert_eq!(
        titles(&plugin, &c1, &["store:client-id=abc"])
            .await
            .unwrap(),
        vec!["stored"]
    );
    plugin.discard_instances();
    // Another connection's instance sees the same Plugin-wide storage.
    assert_eq!(
        titles(&plugin, &connection("c2"), &["store:client-id"])
            .await
            .unwrap(),
        vec!["abc"]
    );
    // 5 MiB by default: four values of 1 MiB fit, the sixth does not.
    assert_eq!(
        titles(&plugin, &c1, &["fill:4"]).await.unwrap(),
        vec!["filled"]
    );
    assert!(matches!(
        titles(&plugin, &c1, &["fill:6"]).await,
        Err(PluginError::Internal(ref text)) if text.contains("quota")
    ));
}

#[tokio::test]
async fn imports_still_to_come_fail_the_call_without_pretending() {
    let plugin = plugin(Backend::Cranelift);
    assert!(matches!(
        titles(&plugin, &connection("c1"), &["fetch"]).await,
        Err(PluginError::Internal(_))
    ));
}

#[tokio::test]
async fn byte_sources_are_read_under_their_budget() {
    let plugin = plugin(Backend::Cranelift);
    let bytes = plugin
        .read_bytes(&connection("c1"), "t1".into(), 1000, 100)
        .await
        .unwrap();
    // The mock's streams are 1024 bytes of `i mod 251`.
    assert_eq!(bytes.len(), 24);
    assert_eq!(bytes[0], (1000 % 251) as u8);
}

// Loading ---------------------------------------------------------------------

#[tokio::test]
async fn loading_reports_the_exports() {
    let plugin = plugin(Backend::Cranelift);
    let exports = plugin.exports();
    assert!(exports.accounts && exports.byte_source && exports.queue_projection);
    assert_eq!(plugin.id().as_str(), "dev.ristle.mock");
}

#[tokio::test]
async fn a_manifest_without_the_limits_its_exports_need_is_refused() {
    let publisher = Publisher::new(1);
    let manifest = mock_manifest(&publisher, "1.0.0", "").replace("liked-states = 40\n", "");
    let mut pieces = Unsigned::new(manifest);
    pieces.component = mock_provider_component();
    let package = ristle_plugin_host::package::Package::read(pieces.signed(&[&publisher])).unwrap();
    let error = host_with(Backend::Cranelift, None)
        .load(&package)
        .unwrap_err();
    assert!(
        matches!(error, Error::Package(PackageError::Manifest(ref text)) if text.contains("liked-states")),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_package_that_is_no_plugin_is_refused() {
    let publisher = Publisher::new(1);
    // The empty component exports nothing.
    let package = ristle_plugin_host::package::Package::read(
        Unsigned::new(mock_manifest(&publisher, "1.0.0", "")).signed(&[&publisher]),
    )
    .unwrap();
    let error = host_with(Backend::Cranelift, None)
        .load(&package)
        .unwrap_err();
    assert!(
        matches!(error, Error::Package(PackageError::Component(_))),
        "{error:?}"
    );
}

#[tokio::test]
async fn compiled_artifacts_are_cached_and_verified() {
    let directory = std::env::temp_dir().join(format!("ristle-artifacts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    let package = mock_package(&Publisher::new(1), "");
    for backend in BACKENDS {
        let host = host_with(backend, Some(directory.clone()));
        host.load(&package).unwrap();
        let fingerprint = host.engine().fingerprint().to_owned();
        let files: Vec<_> = std::fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.to_string_lossy().contains(&fingerprint))
            .collect();
        assert_eq!(files.len(), 1, "{backend:?}");
        // A second host loads the artifact and it runs.
        let cached = host_with(backend, Some(directory.clone()))
            .load(&package)
            .unwrap();
        assert_eq!(
            titles(&cached, &connection("c1"), &["count"])
                .await
                .unwrap(),
            vec!["1"]
        );
        // A corrupted artifact is discarded and compiled again.
        let mut bytes = std::fs::read(&files[0]).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        std::fs::write(&files[0], &bytes).unwrap();
        let recompiled = host_with(backend, Some(directory.clone()))
            .load(&package)
            .unwrap();
        assert_eq!(
            titles(&recompiled, &connection("c1"), &["count"])
                .await
                .unwrap(),
            vec!["1"]
        );
        assert_ne!(std::fs::read(&files[0]).unwrap(), bytes);
    }
    let _ = std::fs::remove_dir_all(&directory);
}

#[tokio::test]
async fn calls_beyond_the_concurrency_limit_wait() {
    let plugin = spinning_plugin(Backend::Cranelift, 1);
    let (c1, c2) = (connection("c1"), connection("c2"));
    let started = Instant::now();
    let (a, b) = tokio::join!(
        titles(&plugin, &c1, &["spin:400"]),
        titles(&plugin, &c2, &["spin:400"]),
    );
    a.unwrap();
    b.unwrap();
    assert!(started.elapsed() >= Duration::from_millis(800));
}

#[tokio::test]
async fn idle_instances_are_released_without_further_calls() {
    let clock = Arc::new(ManualClock::new(NOW));
    let host = PluginHost::new(
        Engine::new(EngineConfig {
            backend: Backend::Cranelift,
            artifacts: None,
        })
        .unwrap(),
        HostPorts {
            clock: clock.clone(),
            store: Arc::new(MemoryPluginStore::new()),
            net: Client::new(ClientConfig::default(), clock, None).unwrap(),
        },
        HostSettings {
            idle_instance: Duration::from_millis(200),
            sweep_interval: Duration::from_millis(50),
            ..HostSettings::default()
        },
    )
    .unwrap();
    let plugin = host.load(&mock_package(&Publisher::new(1), "")).unwrap();
    let c1 = connection("c1");
    titles(&plugin, &c1, &["count"]).await.unwrap();
    assert_eq!(plugin.idle_instances(&c1), 1);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(plugin.idle_instances(&c1), 0);
}

#[test]
fn a_host_needs_the_runtime() {
    let clock = Arc::new(ManualClock::new(NOW));
    let result = PluginHost::new(
        Engine::new(EngineConfig::default()).unwrap(),
        HostPorts {
            clock: clock.clone(),
            store: Arc::new(MemoryPluginStore::new()),
            net: Client::new(ClientConfig::default(), clock, None).unwrap(),
        },
        HostSettings::default(),
    );
    assert!(matches!(result, Err(Error::Engine(_))));
}
