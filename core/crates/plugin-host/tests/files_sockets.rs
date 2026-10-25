//! `files` within Folder Grants and `socket` within the manifest's
//! `sockets` (M5, part 4b).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use ristle_domain::{ConnectionId, ManualClock};
use ristle_net::{Client, ClientConfig, GrantedOrigin, Scheme};
use ristle_plugin_host::test_support::{
    MemoryFolders, MemoryPluginStore, MemoryVault, Publisher, connection_info, mock_package,
};
use ristle_plugin_host::{
    Backend, ConnectionInfo, Engine, EngineConfig, FolderAccess, HostPorts, HostSettings,
    LoadedPlugin, MusicFolder, PluginError, PluginHost,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const NOW: u64 = 1_800_000_000_000;

fn host(folders: Option<Arc<dyn FolderAccess>>, socket_origins: Vec<GrantedOrigin>) -> PluginHost {
    let clock = Arc::new(ManualClock::new(NOW));
    PluginHost::new(
        Engine::new(EngineConfig {
            backend: Backend::Cranelift,
            artifacts: None,
        })
        .unwrap(),
        HostPorts {
            clock: clock.clone(),
            store: Arc::new(MemoryPluginStore::new()),
            vault: Arc::new(MemoryVault::new()),
            net: Client::new(ClientConfig::default(), clock, None).unwrap(),
            folders,
        },
        HostSettings {
            socket_origins,
            ..HostSettings::default()
        },
    )
    .unwrap()
}

async fn one(
    plugin: &LoadedPlugin,
    connection: &str,
    command: &str,
) -> Result<Vec<String>, PluginError> {
    plugin
        .tracks(&ConnectionId::new(connection), vec![command.to_owned()])
        .await?
        .into_iter()
        .map(|track| track.map(|track| track.title))
        .collect()
}

/// A command that returns `count` Tracks.
async fn many(
    plugin: &LoadedPlugin,
    connection: &str,
    command: &str,
    count: usize,
) -> Result<Vec<String>, PluginError> {
    let ids = std::iter::once(command.to_owned())
        .chain((1..count).map(|index| format!("pad{index}")))
        .collect();
    plugin
        .tracks(&ConnectionId::new(connection), ids)
        .await?
        .into_iter()
        .map(|track| track.map(|track| track.title))
        .collect()
}

fn with_folder(id: &str) -> ConnectionInfo {
    ConnectionInfo {
        folders: vec![MusicFolder {
            id: "f1".into(),
            name: "Music".into(),
        }],
        ..connection_info(id, 1)
    }
}

// files -----------------------------------------------------------------------

async fn files_plugin() -> (LoadedPlugin, MemoryFolders) {
    let folders = MemoryFolders::new();
    folders.add("f1", "Album/01.flac", b"fLaC-one");
    folders.add("f1", "Album/02.flac", b"fLaC-two");
    folders.add("f1", "cover.jpg", b"jpeg");
    folders.add("f2", "secret.flac", b"not yours");
    let plugin = host(Some(Arc::new(folders.clone())), Vec::new())
        .load(&mock_package(&Publisher::new(1), "folders = true"))
        .unwrap();
    plugin.set_connection(with_folder("c1")).await.unwrap();
    (plugin, folders)
}

#[tokio::test]
async fn granted_folders_can_be_listed_and_read() {
    let (plugin, _) = files_plugin().await;
    assert_eq!(
        many(&plugin, "c1", "list:f1:", 2).await.unwrap(),
        vec!["Album/", "cover.jpg"]
    );
    assert_eq!(
        many(&plugin, "c1", "list:f1:Album", 2).await.unwrap(),
        vec!["01.flac", "02.flac"]
    );
    assert_eq!(
        one(&plugin, "c1", "readfile:f1:Album/02.flac:5:100")
            .await
            .unwrap(),
        vec!["two"]
    );
}

#[tokio::test]
async fn nothing_outside_the_grant_is_reachable() {
    let (plugin, _) = files_plugin().await;
    for command in [
        "readfile:f2:secret.flac:0:10",
        "readfile:f1:../f2/secret.flac:0:10",
        "readfile:f1:/etc/passwd:0:10",
        "readfile:f1:Album/../../f2/secret.flac:0:10",
        "readfile:f1:missing.flac:0:10",
    ] {
        assert!(
            matches!(
                one(&plugin, "c1", command).await,
                Err(PluginError::Unavailable(_))
            ),
            "{command}"
        );
    }
    // Another connection of the same Plugin has no grant.
    plugin
        .set_connection(connection_info("c2", 1))
        .await
        .unwrap();
    assert!(matches!(
        one(&plugin, "c2", "readfile:f1:cover.jpg:0:10").await,
        Err(PluginError::Unavailable(_))
    ));
}

#[tokio::test]
async fn files_need_the_manifest_and_the_platform() {
    // Without `folders = true` in the manifest.
    let plugin = host(Some(Arc::new(MemoryFolders::new())), Vec::new())
        .load(&mock_package(&Publisher::new(1), ""))
        .unwrap();
    plugin.set_connection(with_folder("c1")).await.unwrap();
    assert!(matches!(
        one(&plugin, "c1", "readfile:f1:cover.jpg:0:10").await,
        Err(PluginError::Unsupported(_))
    ));
    // Without folder access on this platform.
    let plugin = host(None, Vec::new())
        .load(&mock_package(&Publisher::new(1), "folders = true"))
        .unwrap();
    plugin.set_connection(with_folder("c1")).await.unwrap();
    assert!(matches!(
        one(&plugin, "c1", "readfile:f1:cover.jpg:0:10").await,
        Err(PluginError::Unsupported(_))
    ));
}

// socket ----------------------------------------------------------------------

/// A TCP echo server on 127.0.0.1.
async fn echo() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buffer = [0u8; 64];
                while let Ok(read) = stream.read(&mut buffer).await {
                    if read == 0 || stream.write_all(&buffer[..read]).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    port
}

fn loopback(port: u16) -> GrantedOrigin {
    GrantedOrigin {
        scheme: Scheme::Http,
        host: "127.0.0.1".into(),
        port,
        public: false,
        confirmed: vec!["127.0.0.1".parse().unwrap()],
    }
}

async fn socket_plugin(port: u16, confirm: bool) -> LoadedPlugin {
    let origins = if confirm {
        vec![loopback(port)]
    } else {
        Vec::new()
    };
    let plugin = host(None, origins)
        .load(&mock_package(
            &Publisher::new(1),
            &format!("sockets = [\"127.0.0.1:{port}\"]"),
        ))
        .unwrap();
    plugin
        .set_connection(connection_info("c1", 1))
        .await
        .unwrap();
    plugin
}

#[tokio::test]
async fn a_declared_socket_talks() {
    let port = echo().await;
    let plugin = socket_plugin(port, true).await;
    assert_eq!(
        one(&plugin, "c1", &format!("socket:127.0.0.1:{port}:ping"))
            .await
            .unwrap(),
        vec!["ping"]
    );
}

#[tokio::test]
async fn sockets_reach_only_declared_destinations_at_public_addresses() {
    let port = echo().await;
    let plugin = socket_plugin(port, false).await;
    // Declared but loopback: refused by the address rules.
    assert!(matches!(
        one(&plugin, "c1", &format!("socket:127.0.0.1:{port}:ping")).await,
        Err(PluginError::Transient(_))
    ));
    // Not declared at all.
    assert!(matches!(
        one(
            &plugin,
            "c1",
            &format!("socket:127.0.0.1:{}:ping", port.wrapping_add(1))
        )
        .await,
        Err(PluginError::Internal(_))
    ));
}

#[tokio::test]
async fn a_plugin_holds_at_most_four_sockets_until_its_instance_goes() {
    let port = echo().await;
    let plugin = socket_plugin(port, true).await;
    let hold = |count: u32| format!("hold-sockets:{count}:127.0.0.1:{port}");
    assert_eq!(one(&plugin, "c1", &hold(4)).await.unwrap(), vec!["held"]);
    assert!(matches!(
        one(&plugin, "c1", &hold(1)).await,
        Err(PluginError::Internal(_))
    ));
    // Discarding the instance closes its sockets.
    plugin.discard_instances();
    assert_eq!(one(&plugin, "c1", &hold(4)).await.unwrap(), vec!["held"]);
}
