//! The top-level sections of a Plugin Package.
//!
//! Read with a plain section parser before anything is compiled
//! (`docs/plugin-api/packaging.md`, "Layout"). Every length is checked
//! against the file and the section limits before anything is copied.

use super::PackageError;

/// The largest package: 20 MiB.
pub const MAX_PACKAGE_BYTES: usize = 20 * 1024 * 1024;

/// The signature section's name, as `wasmsign2` writes it.
pub(crate) const SIGNATURE: &str = "signature";
/// The manifest section's name.
pub(crate) const MANIFEST: &str = "ristle-manifest";
/// The icon section's name.
pub(crate) const ICON: &str = "ristle-icon";
/// The handover section's name.
pub(crate) const HANDOVER: &str = "ristle-handover";

const SIGNATURE_LIMIT: usize = 4 * 1024;
const MANIFEST_LIMIT: usize = 64 * 1024;
const ICON_LIMIT: usize = 64 * 1024;
const HANDOVER_LIMIT: usize = 1024;

/// The preamble of a WebAssembly component (layer 1, version 0x0d).
const COMPONENT_HEADER: [u8; 8] = [0x00, 0x61, 0x73, 0x6d, 0x0d, 0x00, 0x01, 0x00];

/// The Ristle sections of a package, borrowed from its bytes.
#[derive(Debug)]
pub(crate) struct Sections<'a> {
    pub(crate) manifest: &'a [u8],
    pub(crate) icon: Option<&'a [u8]>,
    pub(crate) handover: Option<&'a [u8]>,
}

/// Reads the sections. The `signature` section must come first, then the
/// manifest, then the optional icon and handover, in that order, each at
/// most once and within its limit. Other custom sections with a `ristle-`
/// name are refused, so nothing can pose as a Ristle section later.
pub(crate) fn read(bytes: &[u8]) -> Result<Sections<'_>, PackageError> {
    if bytes.len() > MAX_PACKAGE_BYTES {
        return Err(PackageError::TooLarge("package"));
    }
    if bytes.get(..8) != Some(&COMPONENT_HEADER[..]) {
        return Err(PackageError::NotAComponent);
    }
    let mut reader = Reader { bytes, position: 8 };
    let mut index = 0usize;
    let mut manifest = None;
    let mut icon = None;
    let mut handover = None;
    // The order of the last Ristle section seen: manifest 1, icon 2,
    // handover 3. They form one run right after the signature.
    let mut last_ristle = 0u8;
    let mut in_ristle_run = true;
    while let Some((id, payload)) = reader.section()? {
        let custom = if id == 0 {
            Some(custom_section(payload)?)
        } else {
            None
        };
        if index == 0 {
            match custom {
                Some((SIGNATURE, data)) if data.len() <= SIGNATURE_LIMIT => {}
                Some((SIGNATURE, _)) => return Err(PackageError::TooLarge("signature section")),
                _ => return Err(PackageError::Unsigned),
            }
            index += 1;
            continue;
        }
        index += 1;
        let Some((name, data)) = custom else {
            in_ristle_run = false;
            continue;
        };
        let (order, limit, slot, what) = match name {
            MANIFEST => (1, MANIFEST_LIMIT, &mut manifest, "manifest section"),
            ICON => (2, ICON_LIMIT, &mut icon, "icon section"),
            HANDOVER => (3, HANDOVER_LIMIT, &mut handover, "handover section"),
            SIGNATURE => return Err(PackageError::Layout("a second signature section")),
            other if other.starts_with("ristle-") => {
                return Err(PackageError::Layout("an unknown ristle- section"));
            }
            _ => {
                in_ristle_run = false;
                continue;
            }
        };
        // Ristle sections directly follow the signature, manifest first,
        // each at most once.
        if slot.is_some()
            || !in_ristle_run
            || order <= last_ristle
            || (last_ristle == 0 && order != 1)
        {
            return Err(PackageError::Layout("Ristle sections out of order"));
        }
        if data.len() > limit {
            return Err(PackageError::TooLarge(what));
        }
        *slot = Some(data);
        last_ristle = order;
    }
    if index == 0 {
        return Err(PackageError::Unsigned);
    }
    let manifest = manifest.ok_or(PackageError::Layout("no manifest section"))?;
    Ok(Sections {
        manifest,
        icon,
        handover,
    })
}

/// Splits a custom section's payload into its name and data.
fn custom_section(payload: &[u8]) -> Result<(&str, &[u8]), PackageError> {
    let mut reader = Reader {
        bytes: payload,
        position: 0,
    };
    let length = reader.u32()? as usize;
    let name_end = reader
        .position
        .checked_add(length)
        .filter(|end| *end <= payload.len())
        .ok_or(PackageError::Malformed)?;
    let name = std::str::from_utf8(&payload[reader.position..name_end])
        .map_err(|_| PackageError::Malformed)?;
    Ok((name, &payload[name_end..]))
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    /// The next section's id and payload, or `None` at the end.
    fn section(&mut self) -> Result<Option<(u8, &'a [u8])>, PackageError> {
        let Some(&id) = self.bytes.get(self.position) else {
            return Ok(None);
        };
        self.position += 1;
        let length = self.u32()? as usize;
        let end = self
            .position
            .checked_add(length)
            .filter(|end| *end <= self.bytes.len())
            .ok_or(PackageError::Malformed)?;
        let payload = &self.bytes[self.position..end];
        self.position = end;
        Ok(Some((id, payload)))
    }

    /// An unsigned LEB128 `u32`, at most five bytes.
    fn u32(&mut self) -> Result<u32, PackageError> {
        let mut value = 0u32;
        for shift in 0..5 {
            let byte = *self
                .bytes
                .get(self.position)
                .ok_or(PackageError::Malformed)?;
            self.position += 1;
            if shift == 4 && byte > 0x0f {
                return Err(PackageError::Malformed);
            }
            value |= u32::from(byte & 0x7f) << (7 * shift);
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(PackageError::Malformed)
    }
}
