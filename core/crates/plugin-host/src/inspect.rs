//! Install step 5: the component's imports and exports against the
//! `ristle:plugin` version this host implements, and the `limits` the
//! exports need (`docs/plugin-api/packaging.md`).

use wasmtime::component::Component;

use crate::package::{Exports, Manifest, PackageError};

/// The `ristle:plugin` version this host implements.
pub const API_VERSION: &str = "0.1.0";

/// The same, as update feeds write it (`api = "0.1"`).
pub const API: &str = "0.1";

const IMPORTS: [&str; 9] = [
    "types",
    "http",
    "connection",
    "storage",
    "crypto",
    "system",
    "socket",
    "live",
    "files",
];

/// The optional exports a component offers, after checking every import
/// and export it declares.
pub(crate) fn exports(
    component: &Component,
    engine: &wasmtime::Engine,
    manifest: &Manifest,
) -> Result<Exports, PackageError> {
    let ty = component.component_type();
    for (name, _) in ty.imports(engine) {
        let interface = interface(name)?;
        if !IMPORTS.contains(&interface) {
            return Err(PackageError::Component(format!(
                "imports {name}, which Ristle does not provide"
            )));
        }
    }
    let mut catalog = false;
    let mut exports = Exports::default();
    for (name, _) in ty.exports(engine) {
        let flag = match interface(name)? {
            "catalog" => &mut catalog,
            "accounts" => &mut exports.accounts,
            "likes" => &mut exports.likes,
            "like-lookup" => &mut exports.like_lookup,
            "playlist-editing" => &mut exports.playlist_editing,
            "reposts" => &mut exports.reposts,
            "feed" => &mut exports.feed,
            "comments" => &mut exports.comments,
            "settings" => &mut exports.settings,
            "queue-projection" => &mut exports.queue_projection,
            "byte-source" => &mut exports.byte_source,
            // Types-only interfaces may be re-exported; they add nothing.
            "types" => continue,
            other => {
                return Err(PackageError::Component(format!(
                    "exports ristle:plugin/{other}, which this Ristle does not know"
                )));
            }
        };
        *flag = true;
    }
    if !catalog {
        return Err(PackageError::Component(
            "does not export ristle:plugin/catalog".to_owned(),
        ));
    }
    let limits = &manifest.limits;
    let missing = [
        (limits.tracks.is_none(), "limits.tracks (for catalog)"),
        (
            exports.like_lookup && limits.liked_states.is_none(),
            "limits.liked-states (for like-lookup)",
        ),
        (
            exports.playlist_editing && limits.playlist_add.is_none(),
            "limits.playlist-add (for playlist-editing)",
        ),
        (
            exports.playlist_editing && limits.playlist_remove.is_none(),
            "limits.playlist-remove (for playlist-editing)",
        ),
    ];
    if let Some((_, what)) = missing.iter().find(|(missing, _)| *missing) {
        return Err(PackageError::Manifest(format!("{what} is missing")));
    }
    Ok(exports)
}

/// `ristle:plugin/<interface>@0.1.0` to `<interface>`.
fn interface(name: &str) -> Result<&str, PackageError> {
    let Some(rest) = name.strip_prefix("ristle:plugin/") else {
        return Err(PackageError::Component(format!(
            "uses {name}, which is not part of ristle:plugin"
        )));
    };
    let (interface, version) = rest.split_once('@').unwrap_or((rest, ""));
    if version != API_VERSION {
        return Err(PackageError::NeedsNewerRistle(format!(
            "built for ristle:plugin@{version}; this Ristle implements {API_VERSION}"
        )));
    }
    Ok(interface)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interface_names_need_this_version() {
        assert_eq!(interface("ristle:plugin/catalog@0.1.0"), Ok("catalog"));
        assert!(matches!(
            interface("ristle:plugin/catalog@0.2.0"),
            Err(PackageError::NeedsNewerRistle(_))
        ));
        assert!(matches!(
            interface("wasi:cli/environment@0.2.0"),
            Err(PackageError::Component(_))
        ));
    }
}
