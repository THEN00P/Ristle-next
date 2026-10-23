//! Walking the granted folders and telling what changed since the last scan.
//!
//! A scan lists every folder of the Folder Grant through `files.list`, keeps
//! audio files by name, and compares each file's size and modification time
//! with the previous scan. Only new and changed files are probed again; a
//! file that is gone is reported so its Track becomes a Disabled Track. A
//! directory that cannot be read right now hides nothing: its files count
//! as unchanged until a scan reads it again.

use std::collections::{BTreeMap, BTreeSet};

use crate::source::SourceError;

/// The most `files.list` returns per call.
pub const MAX_LIST: usize = 1000;

/// One entry of a directory listing (the WIT's `files.entry`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// The file or directory name, without a path.
    pub name: String,
    /// A directory.
    pub folder: bool,
    /// Size in bytes.
    pub size: u64,
    /// Last modification, in milliseconds since the Unix epoch.
    pub modified_ms: Option<u64>,
}

/// Directory listings of the granted folders (the WIT's `files.list`).
pub trait Folders {
    /// The entries of `dir` in granted folder `folder`, sorted by name, at
    /// most [`MAX_LIST`], starting after the name `after`. `dir` is
    /// `/`-separated and relative to the folder; empty for the folder itself.
    fn list(
        &self,
        folder: &str,
        dir: &str,
        after: Option<&str>,
    ) -> Result<Vec<DirEntry>, SourceError>;
}

/// A file in a granted folder (the WIT's `file-path`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileKey {
    /// The granted folder's id.
    pub folder: String,
    /// `/`-separated path relative to the folder.
    pub path: String,
}

impl FileKey {
    /// The file name: the last path segment.
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    fn is_under(&self, folder: &str, dir: &str) -> bool {
        self.folder == folder
            && (dir.is_empty()
                || self
                    .path
                    .strip_prefix(dir)
                    .is_some_and(|rest| rest.starts_with('/')))
    }
}

/// What tells a file's content may have changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    /// Size in bytes.
    pub size: u64,
    /// Last modification, in milliseconds since the Unix epoch.
    pub modified_ms: Option<u64>,
}

impl Stamp {
    /// Whether a file with this stamp needs no new probe. Without a
    /// modification time a same-size edit is invisible, so such a file is
    /// always probed again.
    fn unchanged_since(&self, earlier: &Stamp) -> bool {
        self.size == earlier.size
            && self.modified_ms.is_some()
            && self.modified_ms == earlier.modified_ms
    }
}

/// Bounds on one scan, so a huge or looping tree cannot run forever.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Directory levels below a granted folder.
    pub max_depth: usize,
    /// Audio files in all.
    pub max_files: usize,
    /// Directories in all, below the granted folders.
    pub max_dirs: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_depth: 32,
            max_files: 200_000,
            max_dirs: 50_000,
        }
    }
}

/// The result of walking the granted folders.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Walk {
    /// Every audio file found, sorted.
    pub files: BTreeMap<FileKey, Stamp>,
    /// Directories that could not be read now, as `(folder, dir)`.
    pub unreadable: Vec<(String, String)>,
    /// The scan stopped at one of its [`Limits`]; what it did not reach is
    /// kept like an unreadable directory.
    pub truncated: bool,
}

/// Why a scan failed as a whole.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScanError {
    /// A granted folder is no longer readable: the user must choose the
    /// folders again.
    #[error("folder {0} is no longer accessible")]
    Revoked(String),
}

/// File extensions of the formats the host plays.
const AUDIO_EXTENSIONS: [&str; 8] = ["flac", "mp3", "m4a", "aac", "ogg", "oga", "opus", "wav"];

/// Whether a file name looks like audio worth probing. Hidden files (such as
/// macOS `._` resource forks) are skipped.
pub fn is_audio_name(name: &str) -> bool {
    !name.starts_with('.')
        && name.rsplit_once('.').is_some_and(|(_, extension)| {
            AUDIO_EXTENSIONS
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        })
}

/// Walks every granted folder, one listing page at a time, so memory and
/// host calls stay within `limits` however large a directory is.
pub fn walk<F: Folders + ?Sized>(
    folders: &F,
    roots: &[String],
    limits: Limits,
) -> Result<Walk, ScanError> {
    let mut walk = Walk::default();
    let mut directories = 0usize;
    for (index, root) in roots.iter().enumerate() {
        // Depth-first, in name order: (dir, depth).
        let mut pending = vec![(String::new(), 0usize)];
        while let Some((dir, depth)) = pending.pop() {
            match walk_dir(
                folders,
                root,
                &dir,
                depth,
                limits,
                &mut walk,
                &mut directories,
            ) {
                Ok(Listed::Complete(subdirs)) => {
                    pending.extend(subdirs.into_iter().rev().map(|path| (path, depth + 1)));
                }
                Ok(Listed::Unreadable) => walk.unreadable.push((root.clone(), dir)),
                Ok(Listed::LimitReached) => {
                    // Everything not reached is kept as it was.
                    walk.truncated = true;
                    walk.unreadable.push((root.clone(), dir));
                    walk.unreadable
                        .extend(pending.into_iter().map(|(dir, _)| (root.clone(), dir)));
                    walk.unreadable.extend(
                        roots[index + 1..]
                            .iter()
                            .map(|root| (root.clone(), String::new())),
                    );
                    return Ok(walk);
                }
                Err(ScanError::Revoked(folder)) => return Err(ScanError::Revoked(folder)),
            }
        }
    }
    Ok(walk)
}

enum Listed {
    /// Every page was read; these subdirectories remain.
    Complete(Vec<String>),
    /// The directory could not be read, or not to its end.
    Unreadable,
    /// The scan reached its limits inside this directory.
    LimitReached,
}

/// Lists one directory page by page, recording its audio files.
fn walk_dir<F: Folders + ?Sized>(
    folders: &F,
    root: &str,
    dir: &str,
    depth: usize,
    limits: Limits,
    walk: &mut Walk,
    directories: &mut usize,
) -> Result<Listed, ScanError> {
    let mut subdirs = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let page = match folders.list(root, dir, after.as_deref()) {
            Ok(page) => page,
            Err(SourceError::Revoked) => return Err(ScanError::Revoked(root.to_owned())),
            // Files already recorded stay; the rest is kept by the
            // unreadable mark.
            Err(SourceError::Unavailable(_)) => return Ok(Listed::Unreadable),
        };
        // A page that does not move past `after` would repeat forever.
        let advances = match (page.last(), after.as_deref()) {
            (None, _) => false,
            (Some(last), Some(after)) => last.name.as_str() > after,
            (Some(_), None) => true,
        };
        if !advances {
            return Ok(Listed::Complete(subdirs));
        }
        after = page.last().map(|entry| entry.name.clone());
        for entry in page {
            if entry.name.is_empty() || entry.name.contains('/') || entry.name.starts_with('.') {
                continue;
            }
            let path = if dir.is_empty() {
                entry.name.clone()
            } else {
                format!("{dir}/{}", entry.name)
            };
            if entry.folder {
                if depth >= limits.max_depth {
                    walk.truncated = true;
                    walk.unreadable.push((root.to_owned(), path));
                } else if *directories >= limits.max_dirs {
                    return Ok(Listed::LimitReached);
                } else {
                    *directories += 1;
                    subdirs.push(path);
                }
            } else if is_audio_name(&entry.name) {
                if walk.files.len() >= limits.max_files {
                    return Ok(Listed::LimitReached);
                }
                walk.files.insert(
                    FileKey {
                        folder: root.to_owned(),
                        path,
                    },
                    Stamp {
                        size: entry.size,
                        modified_ms: entry.modified_ms,
                    },
                );
            }
        }
    }
}

/// What a scan found compared with the previous one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
    /// Files not seen before. Probe them.
    pub added: Vec<FileKey>,
    /// Files whose stamp changed. Probe them again.
    pub changed: Vec<FileKey>,
    /// Files that are gone: their Tracks become Disabled Tracks.
    pub removed: Vec<FileKey>,
    /// Files kept as they were, including those in unreadable directories.
    pub unchanged: Vec<FileKey>,
}

/// Compares a walk with the stamps of the previous scan. `roots` are the
/// folders granted now; files of any other folder are removed.
pub fn diff(previous: &BTreeMap<FileKey, Stamp>, walk: &Walk, roots: &[String]) -> Changes {
    let granted: BTreeSet<&str> = roots.iter().map(String::as_str).collect();
    let mut changes = Changes::default();
    for (key, stamp) in &walk.files {
        match previous.get(key) {
            None => changes.added.push(key.clone()),
            Some(earlier) if stamp.unchanged_since(earlier) => changes.unchanged.push(key.clone()),
            Some(_) => changes.changed.push(key.clone()),
        }
    }
    for key in previous.keys() {
        if walk.files.contains_key(key) {
            continue;
        }
        let hidden = walk
            .unreadable
            .iter()
            .any(|(folder, dir)| key.is_under(folder, dir));
        if hidden && granted.contains(key.folder.as_str()) {
            changes.unchanged.push(key.clone());
        } else {
            changes.removed.push(key.clone());
        }
    }
    changes.unchanged.sort();
    changes
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    /// An in-memory tree: folder id → path → entry.
    #[derive(Default)]
    struct Tree {
        entries: BTreeMap<(String, String), DirEntry>,
        unavailable: Vec<(String, String)>,
        revoked: Vec<String>,
        page: usize,
        calls: Cell<usize>,
    }

    impl Tree {
        fn file(mut self, folder: &str, path: &str, size: u64, modified: Option<u64>) -> Self {
            let parts: Vec<&str> = path.split('/').collect();
            for i in 1..parts.len() {
                let dir = parts[..i].join("/");
                let name = parts[i - 1].to_owned();
                self.entries
                    .entry((folder.into(), dir))
                    .or_insert(DirEntry {
                        name,
                        folder: true,
                        size: 0,
                        modified_ms: None,
                    });
            }
            self.entries.insert(
                (folder.into(), path.into()),
                DirEntry {
                    name: parts[parts.len() - 1].to_owned(),
                    folder: false,
                    size,
                    modified_ms: modified,
                },
            );
            self
        }
    }

    impl Folders for Tree {
        fn list(
            &self,
            folder: &str,
            dir: &str,
            after: Option<&str>,
        ) -> Result<Vec<DirEntry>, SourceError> {
            self.calls.set(self.calls.get() + 1);
            if self.revoked.iter().any(|f| f == folder) {
                return Err(SourceError::Revoked);
            }
            if self
                .unavailable
                .iter()
                .any(|(f, d)| f == folder && d == dir)
            {
                return Err(SourceError::Unavailable("busy".into()));
            }
            let mut children: Vec<DirEntry> = self
                .entries
                .iter()
                .filter(|((f, path), _)| {
                    f == folder
                        && match path.rsplit_once('/') {
                            Some((parent, _)) => parent == dir,
                            None => dir.is_empty(),
                        }
                })
                .map(|(_, entry)| entry.clone())
                .collect();
            children.sort_by(|a, b| a.name.cmp(&b.name));
            let page = if self.page == 0 { MAX_LIST } else { self.page };
            Ok(children
                .into_iter()
                .filter(|entry| after.is_none_or(|after| entry.name.as_str() > after))
                .take(page)
                .collect())
        }
    }

    fn key(folder: &str, path: &str) -> FileKey {
        FileKey {
            folder: folder.into(),
            path: path.into(),
        }
    }

    fn roots(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn finds_audio_files_in_nested_folders() {
        let tree = Tree::default()
            .file("music", "a.flac", 10, Some(1))
            .file("music", "Artist/Album/01 One.mp3", 20, Some(2))
            .file("music", "Artist/Album/cover.jpg", 30, Some(3))
            .file("music", "Artist/.hidden/x.mp3", 40, Some(4))
            .file("music", "._b.mp3", 50, Some(5))
            .file("phone", "Voice.M4A", 60, None);

        let walk = walk(&tree, &roots(&["music", "phone"]), Limits::default()).unwrap();

        let found: Vec<FileKey> = walk.files.keys().cloned().collect();
        assert_eq!(
            found,
            [
                key("music", "Artist/Album/01 One.mp3"),
                key("music", "a.flac"),
                key("phone", "Voice.M4A"),
            ]
        );
        assert!(walk.unreadable.is_empty());
        assert!(!walk.truncated);
    }

    #[test]
    fn pages_through_large_directories() {
        let mut tree = Tree {
            page: 3,
            ..Tree::default()
        };
        for i in 0..10 {
            tree = tree.file("music", &format!("{i:02}.mp3"), i, Some(i));
        }

        let walk = walk(&tree, &roots(&["music"]), Limits::default()).unwrap();

        assert_eq!(walk.files.len(), 10);
        // Four full or partial pages and the empty one that ends the listing.
        assert_eq!(tree.calls.get(), 5);
    }

    #[test]
    fn a_revoked_folder_fails_the_scan() {
        let mut tree = Tree::default().file("music", "a.mp3", 1, Some(1));
        tree.revoked.push("music".into());
        assert_eq!(
            walk(&tree, &roots(&["music"]), Limits::default()),
            Err(ScanError::Revoked("music".into()))
        );
    }

    #[test]
    fn stops_at_the_limits() {
        let tree = Tree::default()
            .file("music", "a.mp3", 1, Some(1))
            .file("music", "b.mp3", 1, Some(1))
            .file("music", "deep/er/c.mp3", 1, Some(1));
        let limits = Limits {
            max_depth: 1,
            ..Limits::default()
        };

        let walk = walk(&tree, &roots(&["music"]), limits).unwrap();

        assert_eq!(walk.files.len(), 2);
        assert!(walk.truncated);
        assert_eq!(
            walk.unreadable,
            [("music".to_owned(), "deep/er".to_owned())]
        );
    }

    #[test]
    fn stops_listing_at_the_file_limit() {
        let mut tree = Tree {
            page: 2,
            ..Tree::default()
        };
        for i in 0..100 {
            tree = tree.file("music", &format!("big/{i:03}.mp3"), 1, Some(1));
        }
        tree = tree
            .file("music", "later/a.mp3", 1, Some(1))
            .file("other", "b.mp3", 1, Some(1));
        let limits = Limits {
            max_files: 3,
            ..Limits::default()
        };

        let walk = walk(&tree, &roots(&["music", "other"]), limits).unwrap();

        assert_eq!(walk.files.len(), 3);
        assert!(walk.truncated);
        // The root's page and the empty one that ends it, then two pages of
        // `big`, the second of which reaches the limit. `big`'s other pages,
        // `later` and `other` are never listed.
        assert_eq!(tree.calls.get(), 4);
        assert_eq!(
            walk.unreadable,
            [
                ("music".to_owned(), "big".to_owned()),
                ("music".to_owned(), "later".to_owned()),
                ("other".to_owned(), String::new()),
            ]
        );
        // Nothing beyond the limit counts as removed.
        let mut previous = walk.files.clone();
        previous.insert(
            key("music", "big/099.mp3"),
            Stamp {
                size: 1,
                modified_ms: Some(1),
            },
        );
        previous.insert(
            key("other", "b.mp3"),
            Stamp {
                size: 1,
                modified_ms: Some(1),
            },
        );
        let changes = diff(&previous, &walk, &roots(&["music", "other"]));
        assert!(changes.removed.is_empty());
    }

    /// Legacy `LocalProviderSkipsUnchangedFilesAndDisablesMissingTracks`,
    /// the Plugin's half: unchanged files are not probed again and missing
    /// ones are reported.
    #[test]
    fn skips_unchanged_files_and_reports_missing_ones() {
        let before = Tree::default()
            .file("music", "same.flac", 100, Some(1))
            .file("music", "edited.mp3", 200, Some(2))
            .file("music", "gone.mp3", 300, Some(3))
            .file("music", "unstamped.mp3", 400, None);
        let roots = roots(&["music"]);
        let previous = walk(&before, &roots, Limits::default()).unwrap().files;

        let after = Tree::default()
            .file("music", "same.flac", 100, Some(1))
            .file("music", "edited.mp3", 210, Some(9))
            .file("music", "new.opus", 50, Some(4))
            .file("music", "unstamped.mp3", 400, None);
        let changes = diff(
            &previous,
            &walk(&after, &roots, Limits::default()).unwrap(),
            &roots,
        );

        assert_eq!(changes.added, [key("music", "new.opus")]);
        // Without a modification time a file is always probed again.
        assert_eq!(
            changes.changed,
            [key("music", "edited.mp3"), key("music", "unstamped.mp3")]
        );
        assert_eq!(changes.removed, [key("music", "gone.mp3")]);
        assert_eq!(changes.unchanged, [key("music", "same.flac")]);
    }

    #[test]
    fn an_unreadable_directory_hides_nothing() {
        let full = Tree::default()
            .file("music", "Live/a.mp3", 1, Some(1))
            .file("music", "Livestock/b.mp3", 1, Some(1));
        let roots = roots(&["music"]);
        let previous = walk(&full, &roots, Limits::default()).unwrap().files;

        let mut busy = Tree::default()
            .file("music", "Live/a.mp3", 1, Some(1))
            .file("music", "Livestock/b.mp3", 1, Some(1));
        busy.unavailable.push(("music".into(), "Live".into()));
        let walk = walk(&busy, &roots, Limits::default()).unwrap();
        let changes = diff(&previous, &walk, &roots);

        assert!(changes.removed.is_empty());
        assert_eq!(
            changes.unchanged,
            [key("music", "Live/a.mp3"), key("music", "Livestock/b.mp3")]
        );
    }

    #[test]
    fn files_of_a_folder_no_longer_granted_are_removed() {
        let before =
            Tree::default()
                .file("music", "a.mp3", 1, Some(1))
                .file("old", "b.mp3", 1, Some(1));
        let previous = walk(&before, &roots(&["music", "old"]), Limits::default())
            .unwrap()
            .files;

        let now = roots(&["music"]);
        let changes = diff(
            &previous,
            &walk(&before, &now, Limits::default()).unwrap(),
            &now,
        );

        assert_eq!(changes.removed, [key("old", "b.mp3")]);
        assert_eq!(changes.unchanged, [key("music", "a.mp3")]);
    }

    #[test]
    fn recognizes_audio_names() {
        for name in [
            "a.flac", "B.MP3", "c.m4a", "d.Opus", "e.ogg", "f.oga", "g.aac", "h.wav",
        ] {
            assert!(is_audio_name(name), "{name}");
        }
        for name in ["cover.jpg", "notes", ".x.mp3", "mp3", "a.mp3.part"] {
            assert!(!is_audio_name(name), "{name}");
        }
    }

    #[test]
    fn names_the_file() {
        assert_eq!(key("m", "A/B/c.mp3").name(), "c.mp3");
        assert_eq!(key("m", "c.mp3").name(), "c.mp3");
    }
}
