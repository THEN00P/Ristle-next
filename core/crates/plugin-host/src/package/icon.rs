//! The package icon: a square PNG of 256 to 1024 pixels.
//!
//! The icon is shown before anything is accepted, so its dimensions are read
//! from the PNG header and checked before anything decodes it.

use super::PackageError;

/// The smallest icon side.
pub const MIN_ICON_SIDE: u32 = 256;
/// The largest icon side.
pub const MAX_ICON_SIDE: u32 = 1024;

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// The side of a valid icon.
pub(crate) fn check(bytes: &[u8]) -> Result<u32, PackageError> {
    let invalid = |why: &str| PackageError::Icon(why.to_owned());
    if bytes.get(..8) != Some(&PNG_SIGNATURE[..]) {
        return Err(invalid("not a PNG"));
    }
    // The first chunk must be IHDR: length 13, type, width, height.
    let header = bytes
        .get(8..24)
        .ok_or_else(|| invalid("truncated header"))?;
    if header[..4] != [0, 0, 0, 13] || &header[4..8] != b"IHDR" {
        return Err(invalid("no IHDR chunk"));
    }
    let width = u32::from_be_bytes([header[8], header[9], header[10], header[11]]);
    let height = u32::from_be_bytes([header[12], header[13], header[14], header[15]]);
    if width != height {
        return Err(invalid("not square"));
    }
    if !(MIN_ICON_SIDE..=MAX_ICON_SIDE).contains(&width) {
        return Err(invalid("not between 256 and 1024 pixels"));
    }
    Ok(width)
}

/// The start of a PNG of the given size, enough for [`check`]. For tests.
#[cfg(any(test, feature = "test-support"))]
pub fn png_header(width: u32, height: u32) -> Vec<u8> {
    let mut out = PNG_SIGNATURE.to_vec();
    out.extend_from_slice(&[0, 0, 0, 13]);
    out.extend_from_slice(b"IHDR");
    out.extend_from_slice(&width.to_be_bytes());
    out.extend_from_slice(&height.to_be_bytes());
    out.extend_from_slice(&[8, 6, 0, 0, 0]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_square_pngs_of_the_right_size_pass() {
        assert_eq!(check(&png_header(256, 256)).unwrap(), 256);
        assert_eq!(check(&png_header(1024, 1024)).unwrap(), 1024);
        for (width, height) in [(255, 255), (1025, 1025), (512, 256), (u32::MAX, u32::MAX)] {
            assert!(
                check(&png_header(width, height)).is_err(),
                "{width}x{height}"
            );
        }
        assert!(check(b"<svg/>").is_err());
        assert!(check(&png_header(256, 256)[..20]).is_err());
    }
}
