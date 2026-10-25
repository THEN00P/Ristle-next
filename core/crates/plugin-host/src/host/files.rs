//! `files`: reading the folders the user granted a connection (its Folder
//! Grant). The only file access a Plugin has, read-only.

use wasmtime::component::Resource;

use super::{ConnectionHandle, HostState, strings_fit, too_large};
use crate::bindings::ristle::plugin::files;
use crate::bindings::ristle::plugin::types::{Error, FilePath};
use crate::ports::{FileEntry, FolderError};

/// Entries per `files.list` call.
pub(crate) const MAX_ENTRIES: usize = 1000;
/// Bytes per `files.read` call.
pub(crate) const MAX_READ: u32 = 1024 * 1024;

/// A Plugin's path as the host passes it on: `/`-separated, relative, with
/// no empty, `.` or `..` component and no backslash or NUL. `""` is the
/// granted folder itself.
fn normalized(path: &str) -> Option<String> {
    if path.is_empty() {
        return Some(String::new());
    }
    let valid = path.split('/').all(|part| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && !part.contains('\\')
            && !part.contains('\0')
    });
    valid.then(|| path.to_owned())
}

fn folder_error(error: FolderError) -> Error {
    match error {
        FolderError::NotFound => Error::Unavailable("no such file".to_owned()),
        FolderError::AccessLost => {
            Error::Unauthorized("the system no longer lets Ristle read this folder".to_owned())
        }
        other => Error::Transient(other.to_string()),
    }
}

impl HostState {
    /// Checks a path against the connection's Folder Grant and runs `f`
    /// with the platform's access, under the commit gate.
    fn in_grant<R>(
        &self,
        path: &FilePath,
        f: impl FnOnce(&dyn crate::ports::FolderAccess, &str) -> Result<R, FolderError>,
    ) -> Result<R, Error> {
        let Some(call) = self.live_call() else {
            return Err(Error::Unauthorized("this call was retired".to_owned()));
        };
        let plugin = &self.plugin;
        if !plugin.manifest.permissions.folders {
            return Err(Error::Unsupported(
                "the manifest does not ask for folders".to_owned(),
            ));
        }
        let granted = plugin
            .connections
            .info(&call.connection)
            .is_some_and(|info| info.folders.iter().any(|folder| folder.id == path.folder));
        if !granted {
            return Err(Error::Unavailable(
                "the folder is not granted to this connection".to_owned(),
            ));
        }
        let relative = normalized(&path.path)
            .ok_or_else(|| Error::Unavailable("the path leaves the granted folder".to_owned()))?;
        let Some(access) = plugin.services.folders.as_deref() else {
            return Err(Error::Unsupported(
                "this platform offers no folder access".to_owned(),
            ));
        };
        plugin
            .connections
            .commit(&call.connection, call.generation, || f(access, &relative))
            .ok_or_else(|| Error::Unauthorized("this call was retired".to_owned()))?
            .map_err(folder_error)
    }
}

impl files::Host for HostState {
    fn list(
        &mut self,
        _: Resource<ConnectionHandle>,
        dir: FilePath,
        after: Option<String>,
    ) -> wasmtime::Result<Result<Vec<files::Entry>, Error>> {
        if !strings_fit(&[&dir.folder, &dir.path, after.as_deref().unwrap_or_default()]) {
            return Ok(Err(too_large("a path")));
        }
        Ok(self.timed(|state| {
            let entries = state.in_grant(&dir, |access, path| {
                access.list(&dir.folder, path, after.as_deref(), MAX_ENTRIES)
            })?;
            if entries.len() > MAX_ENTRIES {
                return Err(Error::Internal("the folder listing is too long".to_owned()));
            }
            Ok(entries
                .into_iter()
                .map(|entry: FileEntry| files::Entry {
                    name: entry.name,
                    folder: entry.folder,
                    size: entry.size,
                    modified: entry.modified_ms,
                })
                .collect())
        }))
    }

    fn read(
        &mut self,
        _: Resource<ConnectionHandle>,
        file: FilePath,
        offset: u64,
        length: u32,
    ) -> wasmtime::Result<Result<Vec<u8>, Error>> {
        if !strings_fit(&[&file.folder, &file.path]) {
            return Ok(Err(too_large("a path")));
        }
        let length = length.min(MAX_READ);
        Ok(self.timed(|state| {
            let bytes = state.in_grant(&file, |access, path| {
                access.read(&file.folder, path, offset, length)
            })?;
            if bytes.len() > length as usize {
                return Err(Error::Internal(
                    "the read returned more than asked".to_owned(),
                ));
            }
            Ok(bytes)
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::normalized;

    #[test]
    fn paths_stay_inside_their_folder() {
        assert_eq!(normalized(""), Some(String::new()));
        assert_eq!(
            normalized("Album/01.flac").as_deref(),
            Some("Album/01.flac")
        );
        for bad in [
            "/etc/passwd",
            "../x",
            "a/../b",
            "a/./b",
            "a//b",
            "a\\..\\b",
            "a/",
            "x\0y",
        ] {
            assert_eq!(normalized(bad), None, "{bad:?}");
        }
    }
}
