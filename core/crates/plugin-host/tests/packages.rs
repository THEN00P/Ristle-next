//! Package reading, signature verification and install review
//! (`docs/plugin-api/packaging.md`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use ristle_plugin_host::package::{
    Change, ConsentExport, Exports, Installed, MAX_PACKAGE_BYTES, Package, PackageError,
    PermissionDiff, Review, ShareKind, png_header, review,
};
use ristle_plugin_host::test_support::{Publisher, Unsigned, custom_section, manifest, sign};

const ID: &str = "dev.example.music";

fn unsigned(version: &str, publisher: &Publisher, extra: &str) -> Unsigned {
    Unsigned::new(manifest(
        ID,
        version,
        "Example Developer",
        &publisher.key(),
        extra,
    ))
}

fn package(version: &str, publisher: &Publisher, extra: &str) -> Package {
    Package::read(unsigned(version, publisher, extra).signed(&[publisher])).unwrap()
}

fn installed(package: &Package, exports: Exports) -> Installed {
    Installed {
        manifest: package.manifest().clone(),
        exports,
        publisher_key: package.manifest().publisher_key,
    }
}

// Reading -------------------------------------------------------------------

#[test]
fn a_signed_package_reads() {
    let publisher = Publisher::new(1);
    let mut pieces = unsigned("1.0.0", &publisher, "");
    pieces.icon = Some(png_header(512, 512));
    let bytes = pieces.signed(&[&publisher]);
    let package = Package::read(bytes.clone()).unwrap();
    assert_eq!(package.manifest().id.as_str(), ID);
    assert_eq!(package.icon(), Some(&png_header(512, 512)[..]));
    assert_eq!(package.bytes().as_ref(), &bytes[..]);
    assert!(package.signed_by(&publisher.key()));
    assert!(!package.signed_by(&Publisher::new(2).key()));
}

#[test]
fn every_flipped_byte_after_the_signature_is_rejected() {
    let publisher = Publisher::new(1);
    let bytes = unsigned("1.0.0", &publisher, "").signed(&[&publisher]);
    // The signature section: id 0, a one-byte length, then its payload.
    let signed_from = 2 + usize::from(bytes[9]) + 8;
    for index in signed_from..bytes.len() {
        let mut tampered = bytes.clone();
        tampered[index] ^= 0x01;
        assert!(
            Package::read(tampered).is_err(),
            "byte {index} flipped was accepted"
        );
    }
}

#[test]
fn a_tampered_signature_is_rejected() {
    let publisher = Publisher::new(1);
    let bytes = unsigned("1.0.0", &publisher, "").signed(&[&publisher]);
    let mut tampered = bytes.clone();
    // The last byte of the signature section is part of the signature.
    let end = 10 + usize::from(bytes[9]);
    tampered[end - 1] ^= 0x01;
    assert!(Package::read(tampered).is_err());
}

#[test]
fn a_section_appended_after_signing_is_rejected() {
    let publisher = Publisher::new(1);
    let mut bytes = unsigned("1.0.0", &publisher, "").signed(&[&publisher]);
    custom_section(&mut bytes, "extra", b"added later");
    assert_eq!(Package::read(bytes), Err(PackageError::BadSignature));
}

#[test]
fn a_package_signed_by_another_key_than_its_manifest_names_is_rejected() {
    let named = Publisher::new(1);
    let other = Publisher::new(2);
    let bytes = unsigned("1.0.0", &named, "").signed(&[&other]);
    assert_eq!(Package::read(bytes), Err(PackageError::BadSignature));
}

#[test]
fn unsigned_and_foreign_files_are_rejected() {
    let publisher = Publisher::new(1);
    assert_eq!(
        Package::read(unsigned("1.0.0", &publisher, "").bytes()),
        Err(PackageError::Unsigned)
    );
    let mut module = unsigned("1.0.0", &publisher, "").signed(&[&publisher]);
    module[4] = 0x01; // a core module's version
    module[6] = 0x00;
    assert_eq!(Package::read(module), Err(PackageError::NotAComponent));
    assert_eq!(Package::read(Vec::new()), Err(PackageError::NotAComponent));
    let mut truncated = unsigned("1.0.0", &publisher, "").signed(&[&publisher]);
    truncated.truncate(truncated.len() - 3);
    assert!(Package::read(truncated).is_err());
}

#[test]
fn the_ristle_sections_must_follow_the_signature_in_order() {
    let publisher = Publisher::new(1);
    let manifest_text = manifest(ID, "1.0.0", "Example Developer", &publisher.key(), "");
    let layouts: Vec<Vec<(&str, Vec<u8>)>> = vec![
        // No manifest.
        vec![("ristle-icon", png_header(256, 256))],
        // Another section before the manifest.
        vec![
            ("name", b"x".to_vec()),
            ("ristle-manifest", manifest_text.clone().into_bytes()),
        ],
        // Icon before manifest.
        vec![
            ("ristle-icon", png_header(256, 256)),
            ("ristle-manifest", manifest_text.clone().into_bytes()),
        ],
        // Two manifests.
        vec![
            ("ristle-manifest", manifest_text.clone().into_bytes()),
            ("ristle-manifest", manifest_text.clone().into_bytes()),
        ],
        // An unknown Ristle section.
        vec![
            ("ristle-manifest", manifest_text.clone().into_bytes()),
            ("ristle-extra", b"x".to_vec()),
        ],
        // An oversized manifest.
        vec![(
            "ristle-manifest",
            format!("{manifest_text}#{}", "x".repeat(64 * 1024)).into_bytes(),
        )],
        // An oversized icon.
        vec![
            ("ristle-manifest", manifest_text.clone().into_bytes()),
            (
                "ristle-icon",
                [png_header(256, 256), vec![0; 64 * 1024]].concat(),
            ),
        ],
    ];
    for (index, sections) in layouts.into_iter().enumerate() {
        let mut bytes = ristle_plugin_host::test_support::EMPTY_COMPONENT.to_vec();
        for (name, data) in &sections {
            custom_section(&mut bytes, name, data);
        }
        let signed = sign(bytes, &[&publisher]);
        let result = Package::read(signed);
        assert!(
            matches!(
                result,
                Err(PackageError::Layout(_) | PackageError::TooLarge(_))
            ),
            "layout {index}: {result:?}"
        );
    }
}

#[test]
fn oversized_packages_and_bad_icons_are_rejected() {
    let publisher = Publisher::new(1);
    let mut big = unsigned("1.0.0", &publisher, "");
    let mut component = ristle_plugin_host::test_support::EMPTY_COMPONENT.to_vec();
    custom_section(&mut component, "padding", &vec![0; MAX_PACKAGE_BYTES]);
    big.component = component;
    assert_eq!(
        Package::read(big.signed(&[&publisher])),
        Err(PackageError::TooLarge("package"))
    );
    for icon in [
        png_header(128, 128),
        png_header(512, 256),
        b"<svg/>".to_vec(),
    ] {
        let mut pieces = unsigned("1.0.0", &publisher, "");
        pieces.icon = Some(icon);
        assert!(matches!(
            Package::read(pieces.signed(&[&publisher])),
            Err(PackageError::Icon(_))
        ));
    }
}

// Review --------------------------------------------------------------------

#[test]
fn an_unknown_id_is_a_new_install() {
    let publisher = Publisher::new(1);
    let package = package("1.0.0", &publisher, "");
    assert_eq!(
        review(&package, &Exports::default(), None, &[]),
        Ok(Review::New)
    );
    assert_eq!(
        review(&package, &Exports::default(), None, &[publisher.key()]),
        Err(PackageError::Revoked)
    );
}

#[test]
fn a_higher_version_by_the_same_key_is_an_update() {
    let publisher = Publisher::new(1);
    let old = installed(&package("1.0.0", &publisher, ""), Exports::default());
    let new = package("1.1.0", &publisher, "");
    match review(&new, &Exports::default(), Some(&old), &[]).unwrap() {
        Review::Update(change) => {
            assert_eq!(change.from, semver::Version::new(1, 0, 0));
            assert!(change.added.is_empty());
            assert_eq!(change.handover, None);
        }
        other => panic!("{other:?}"),
    }
    let lower = package("0.9.0", &publisher, "");
    assert!(matches!(
        review(&lower, &Exports::default(), Some(&old), &[]),
        Ok(Review::Downgrade(_))
    ));
    let same = package("1.0.0", &publisher, "");
    assert_eq!(
        review(&same, &Exports::default(), Some(&old), &[]),
        Err(PackageError::AlreadyInstalled)
    );
    // Build metadata does not order releases.
    let rebuilt = package("1.0.0+build.2", &publisher, "");
    assert_eq!(
        review(&rebuilt, &Exports::default(), Some(&old), &[]),
        Err(PackageError::AlreadyInstalled)
    );
    let built = installed(
        &package("1.0.0+build.2", &publisher, ""),
        Exports::default(),
    );
    assert_eq!(
        review(&same, &Exports::default(), Some(&built), &[]),
        Err(PackageError::AlreadyInstalled)
    );
}

#[test]
fn new_permissions_are_listed_and_removed_ones_never_ask() {
    let publisher = Publisher::new(1);
    let old_extra = r#"updates = "https://feeds.example.com/music.json"

[permissions]
hosts = ["api.example.com"]
media-hosts = ["*.cdn.example.com"]
sockets = ["ap.example.com:4070"]
web-capture = ["example.com", "*.example.com", "accounts.google.com"]
web-capture-observe = ["example.com"]

[party]
share = ["title"]

[playback]
overlap = false"#;
    let new_extra = r#"updates = "https://other.example.net/music.json"

[permissions]
hosts = ["api.example.com", "api2.example.com"]
media-hosts = ["a.cdn.example.com", "*.media.example.org"]
sockets = ["ap.example.com:4070", "ap2.example.com:443"]
web-capture = ["example.com", "*.example.com", "id.example.net"]
web-capture-observe = ["example.com", "login.example.com"]
storage-mib = 20
servers = true

[auth]
app-schemes = ["music"]

[party]
share = ["title", "artists", "description"]"#;
    let old_exports = Exports {
        accounts: true,
        ..Exports::default()
    };
    let new_exports = Exports {
        accounts: true,
        likes: true,
        playlist_editing: true,
        feed: true,
        ..Exports::default()
    };
    let old = installed(&package("1.0.0", &publisher, old_extra), old_exports);
    let new = package("1.1.0", &publisher, new_extra);
    let Review::Update(change) = review(&new, &new_exports, Some(&old), &[]).unwrap() else {
        panic!("not an update");
    };
    assert_eq!(
        change.added,
        PermissionDiff {
            hosts: vec!["api2.example.com".into(), "*.media.example.org".into()],
            update_host: Some("other.example.net".into()),
            sockets: vec!["ap2.example.com:443".into()],
            web_capture: vec!["id.example.net".into()],
            web_capture_observe: vec!["login.example.com".into()],
            party: vec![
                (ShareKind::Track, vec!["artists".into()]),
                (ShareKind::Album, vec!["artists".into()]),
                (ShareKind::Playlist, vec!["description".into()]),
            ],
            overlap: true,
            app_schemes: vec!["music".into()],
            storage_mib: Some(20),
            servers: true,
            folders: false,
            exports: vec![ConsentExport::Likes, ConsentExport::PlaylistEditing],
        }
    );
    // Back again: what was removed or narrowed asks nothing. What the old
    // version had beyond the new one (a wider media wildcard, its feed
    // host, a loaded identity provider) is new again.
    let reverse =
        PermissionDiff::between(new.manifest(), &new_exports, &old.manifest, &old_exports);
    assert_eq!(
        reverse,
        PermissionDiff {
            hosts: vec!["*.cdn.example.com".into()],
            update_host: Some("feeds.example.com".into()),
            web_capture: vec!["accounts.google.com".into()],
            ..PermissionDiff::default()
        }
    );
}

#[test]
fn sharing_every_field_again_is_a_new_permission() {
    let publisher = Publisher::new(1);
    let old = installed(
        &package("1.0.0", &publisher, "[party]\nshare = [\"title\"]"),
        Exports::default(),
    );
    let new = package("1.1.0", &publisher, "");
    let Review::Update(change) = review(&new, &Exports::default(), Some(&old), &[]).unwrap() else {
        panic!("not an update");
    };
    let kinds: Vec<ShareKind> = change.added.party.iter().map(|(kind, _)| *kind).collect();
    assert_eq!(
        kinds,
        vec![
            ShareKind::Track,
            ShareKind::Album,
            ShareKind::Profile,
            ShareKind::Playlist
        ]
    );
}

#[test]
fn dropping_servers_or_folders_ends_those_grants() {
    let publisher = Publisher::new(1);
    let old = installed(
        &package(
            "1.0.0",
            &publisher,
            "[permissions]\nservers = true\nfolders = true",
        ),
        Exports::default(),
    );
    let new = package("1.1.0", &publisher, "");
    let Review::Update(change) = review(&new, &Exports::default(), Some(&old), &[]).unwrap() else {
        panic!("not an update");
    };
    assert!(change.ends_server_grants && change.ends_folder_grants);
    assert!(change.added.is_empty());
}

#[test]
fn another_key_is_a_takeover() {
    let original = Publisher::new(1);
    let other = Publisher::new(2);
    let old = installed(&package("1.0.0", &original, ""), Exports::default());
    let replacement = Package::read(
        Unsigned::new(manifest(ID, "2.0.0", "Other Person", &other.key(), "")).signed(&[&other]),
    )
    .unwrap();
    assert_eq!(
        review(&replacement, &Exports::default(), Some(&old), &[]),
        Ok(Review::Takeover {
            handover: None,
            from: "Example Developer".into(),
            to: "Other Person".into(),
        })
    );
    // Lower or equal versions do not make a takeover less of one.
    let same_version = Package::read(
        Unsigned::new(manifest(ID, "1.0.0", "Other Person", &other.key(), "")).signed(&[&other]),
    )
    .unwrap();
    assert!(matches!(
        review(&same_version, &Exports::default(), Some(&old), &[]),
        Ok(Review::Takeover { .. })
    ));
}

#[test]
fn a_takeover_with_a_handover_from_the_installed_key_says_so() {
    let original = Publisher::new(1);
    let successor = Publisher::new(2);
    let old = installed(&package("1.0.0", &original, ""), Exports::default());
    let mut pieces = Unsigned::new(manifest(ID, "2.0.0", "Successor", &successor.key(), ""));
    pieces.handover = Some(original.handover(ID, &successor.key(), 1_760_000_000_000));
    let handed_over = Package::read(pieces.signed(&[&successor])).unwrap();
    match review(&handed_over, &Exports::default(), Some(&old), &[]).unwrap() {
        Review::Takeover { handover, .. } => {
            assert_eq!(handover.unwrap().new_key, successor.key());
        }
        other => panic!("{other:?}"),
    }
    // Revoked: the handover is ignored, the takeover stays a plain one.
    assert_eq!(
        review(
            &handed_over,
            &Exports::default(),
            Some(&old),
            &[original.key()]
        ),
        Ok(Review::Takeover {
            handover: None,
            from: "Example Developer".into(),
            to: "Successor".into(),
        })
    );
    // A handover the successor signed itself counts for nothing.
    let mut forged = Unsigned::new(manifest(ID, "2.0.0", "Successor", &successor.key(), ""));
    forged.handover = Some(successor.handover(ID, &successor.key(), 1_760_000_000_000));
    let forged = Package::read(forged.signed(&[&successor])).unwrap();
    assert!(matches!(
        review(&forged, &Exports::default(), Some(&old), &[]),
        Ok(Review::Takeover { handover: None, .. })
    ));
}

#[test]
fn a_key_rotation_goes_through_a_handover_the_user_accepts() {
    let old_key = Publisher::new(1);
    let new_key = Publisher::new(2);
    let mut current = installed(&package("1.0.0", &old_key, ""), Exports::default());
    // The rotation: an ordinary update by the old key carrying the handover.
    let mut pieces = unsigned("1.1.0", &old_key, "");
    pieces.handover = Some(old_key.handover(ID, &new_key.key(), 1_760_000_000_000));
    let rotation = Package::read(pieces.signed(&[&old_key])).unwrap();
    match review(&rotation, &Exports::default(), Some(&current), &[]).unwrap() {
        Review::Update(change) => assert_eq!(change.handover.unwrap().new_key, new_key.key()),
        other => panic!("{other:?}"),
    }
    // Until the user accepts, the next package from the new key is a takeover.
    current.manifest = rotation.manifest().clone();
    let next = package("1.2.0", &new_key, "");
    assert!(matches!(
        review(&next, &Exports::default(), Some(&current), &[]),
        Ok(Review::Takeover { .. })
    ));
    // Accepted: the new key is the Publisher key, and it updates.
    current.publisher_key = new_key.key();
    assert!(matches!(
        review(&next, &Exports::default(), Some(&current), &[]),
        Ok(Review::Update(_))
    ));
}

#[test]
fn an_extra_signature_never_changes_the_publisher() {
    let original = Publisher::new(1);
    let stranger = Publisher::new(2);
    let old = installed(&package("1.0.0", &original, ""), Exports::default());
    // Anyone can append a signature to a genuine update; it is ignored.
    let genuine = unsigned("1.1.0", &original, "").signed(&[&original]);
    let with_stranger = Package::read(sign(genuine, &[&stranger])).unwrap();
    assert!(matches!(
        review(&with_stranger, &Exports::default(), Some(&old), &[]),
        Ok(Review::Update(Change { handover: None, .. }))
    ));
    // A package naming the stranger's key but also signed by the installed
    // key is still the installed Publisher's update.
    let dual = Package::read(
        Unsigned::new(manifest(ID, "1.2.0", "Stranger", &stranger.key(), ""))
            .signed(&[&stranger, &original]),
    )
    .unwrap();
    assert!(matches!(
        review(&dual, &Exports::default(), Some(&old), &[]),
        Ok(Review::Update(_))
    ));
}

#[test]
fn nothing_signed_by_a_revoked_installed_key_is_accepted() {
    let publisher = Publisher::new(1);
    let old = installed(&package("1.0.0", &publisher, ""), Exports::default());
    let update = package("1.1.0", &publisher, "");
    assert_eq!(
        review(&update, &Exports::default(), Some(&old), &[publisher.key()]),
        Err(PackageError::Revoked)
    );
    // A genuine revocation from a feed names the key.
    assert_eq!(
        publisher.revocation("2026-11-02T10:00:00Z").verified(),
        Some(publisher.key())
    );
}

#[test]
fn a_package_for_another_id_is_not_reviewed_against_this_one() {
    let publisher = Publisher::new(1);
    let old = installed(&package("1.0.0", &publisher, ""), Exports::default());
    let other = Package::read(
        Unsigned::new(manifest(
            "dev.example.other",
            "2.0.0",
            "X",
            &publisher.key(),
            "",
        ))
        .signed(&[&publisher]),
    )
    .unwrap();
    assert!(matches!(
        review(&other, &Exports::default(), Some(&old), &[]),
        Err(PackageError::Manifest(_))
    ));
}
