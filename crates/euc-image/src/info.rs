//! The `.euc_info` block, layout 1 (`docs/spec/euc-info.md`).

use std::fmt;

use serde::Serialize;
use thiserror::Error;

/// Size of the block in bytes.
pub const INFO_LEN: usize = 128;

const MAGIC: [u8; 4] = *b"EUCF";
const LAYOUT: u16 = 1;
const FLAG_DEV: u16 = 1;

// Field offsets inside the block.
const LAYOUT_AT: usize = 0x04;
const SIZE_AT: usize = 0x06;
const BOARD: (usize, usize) = (0x08, 16);
const PROFILE: (usize, usize) = (0x18, 8);
const VERSION_AT: usize = 0x20;
const FLAGS_AT: usize = 0x26;
const GIT_SHA: (usize, usize) = (0x28, 12);
pub(crate) const IMAGE_LEN_AT: usize = 0x34;
pub(crate) const CRC_AT: usize = 0x38;
const SIGNATURE: (usize, usize) = (0x3C, 64);
const RESERVED_AT: usize = 0x7C;

/// The identity block our firmware builds carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EucInfo {
    /// Board id the image was built for.
    pub board: String,
    /// Which firmware profile was built.
    pub profile: BuildProfile,
    /// Firmware version.
    pub version: Version,
    /// Built as a development image (unsigned or signed with a dev key).
    pub dev: bool,
    /// Short git sha of the source.
    pub git_sha: String,
    /// Image length written by `euc image seal`; 0 while unsealed.
    pub image_len: u32,
    /// CRC-32 written by `euc image seal`.
    pub crc32: u32,
    /// True when the signature field is not all zero.
    pub signed: bool,
}

impl EucInfo {
    /// Reads the block at `offset` in `image`.
    ///
    /// Returns `Ok(None)` when there is no block: the image is too short or the magic is absent
    /// (stock images). A block with the magic but bad contents is an error, never "no block".
    pub fn find(image: &[u8], offset: usize) -> Result<Option<Self>, InfoError> {
        let Some(block) = image.get(offset..offset + INFO_LEN) else { return Ok(None) };
        if block[..4] != MAGIC {
            return Ok(None);
        }
        let layout = u16_at(block, LAYOUT_AT);
        if layout != LAYOUT {
            return Err(InfoError::Layout(layout));
        }
        let size = u16_at(block, SIZE_AT);
        if usize::from(size) != INFO_LEN {
            return Err(InfoError::Size(size));
        }
        let flags = u16_at(block, FLAGS_AT);
        if flags & !FLAG_DEV != 0 {
            return Err(InfoError::Flags(flags));
        }
        if u32_at(block, RESERVED_AT) != 0 {
            return Err(InfoError::Reserved);
        }
        let profile = match text(block, PROFILE, "profile")?.as_str() {
            "compat" => BuildProfile::Compat,
            "clean" => BuildProfile::Clean,
            other => return Err(InfoError::Profile(other.to_owned())),
        };
        Ok(Some(Self {
            board: text(block, BOARD, "board")?,
            profile,
            version: Version {
                major: u16_at(block, VERSION_AT),
                minor: u16_at(block, VERSION_AT + 2),
                patch: u16_at(block, VERSION_AT + 4),
            },
            dev: flags & FLAG_DEV != 0,
            git_sha: text(block, GIT_SHA, "git_sha")?,
            image_len: u32_at(block, IMAGE_LEN_AT),
            crc32: u32_at(block, CRC_AT),
            signed: block[SIGNATURE.0..SIGNATURE.0 + SIGNATURE.1].iter().any(|&b| b != 0),
        }))
    }

    /// True once `euc image seal` has written the length and CRC.
    pub const fn is_sealed(&self) -> bool {
        self.image_len != 0
    }
}

/// CRC-32/ISO-HDLC of `image` with the block's `crc32` and `signature` fields read as zero.
///
/// This is the value a sealed block stores. Computed in place, without copying the image.
pub fn info_crc32(image: &[u8], info_offset: usize) -> u32 {
    let crc_at = info_offset + CRC_AT;
    let signed_end = info_offset + SIGNATURE.0 + SIGNATURE.1;
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&image[..crc_at]);
    hasher.update(&[0; 4 + SIGNATURE.1]);
    hasher.update(&image[signed_end..]);
    hasher.finalize()
}

/// The two firmware profiles our build produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BuildProfile {
    /// Behaves exactly like the stock firmware, quirks included.
    Compat,
    /// Stock behavior with the known defects fixed.
    Clean,
}

impl fmt::Display for BuildProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Compat => "compat",
            Self::Clean => "clean",
        })
    }
}

/// Semantic version of a firmware build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Version {
    /// Major.
    pub major: u16,
    /// Minor.
    pub minor: u16,
    /// Patch.
    pub patch: u16,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// A block that has the magic but breaks the layout.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "kebab-case")]
pub enum InfoError {
    /// Unknown layout version.
    #[error("layout {0} is not supported (expected {LAYOUT})")]
    Layout(u16),
    /// The size field is not 128.
    #[error("size field is {0} (expected {INFO_LEN})")]
    Size(u16),
    /// Flag bits other than bit 0 are set.
    #[error("flags 0x{0:04X} set bits that layout 1 does not define")]
    Flags(u16),
    /// The reserved word is not zero.
    #[error("reserved field is not zero")]
    Reserved,
    /// A text field is not printable ASCII followed by NUL padding.
    #[error("{0} field is not NUL-padded printable ASCII")]
    Text(&'static str),
    /// The profile is neither `compat` nor `clean`.
    #[error("profile \"{0}\" is not compat or clean")]
    Profile(String),
}

fn u16_at(block: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([block[at], block[at + 1]])
}

fn u32_at(block: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([block[at], block[at + 1], block[at + 2], block[at + 3]])
}

/// A NUL-padded ASCII field: printable characters, then only NULs.
fn text(block: &[u8], (at, len): (usize, usize), name: &'static str) -> Result<String, InfoError> {
    let field = &block[at..at + len];
    let end = field.iter().position(|&b| b == 0).unwrap_or(len);
    let (value, padding) = field.split_at(end);
    if value.iter().all(u8::is_ascii_graphic) && padding.iter().all(|&b| b == 0) {
        Ok(String::from_utf8_lossy(value).into_owned())
    } else {
        Err(InfoError::Text(name))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Writes a layout-1 block into `image` at `offset`, as the firmware build does (unsealed).
    pub(crate) fn write_block(image: &mut [u8], offset: usize, board: &str, profile: &str) {
        let block = &mut image[offset..offset + INFO_LEN];
        block.fill(0);
        block[..4].copy_from_slice(&MAGIC);
        block[LAYOUT_AT..LAYOUT_AT + 2].copy_from_slice(&LAYOUT.to_le_bytes());
        block[SIZE_AT..SIZE_AT + 2].copy_from_slice(&(INFO_LEN as u16).to_le_bytes());
        block[BOARD.0..BOARD.0 + board.len()].copy_from_slice(board.as_bytes());
        block[PROFILE.0..PROFILE.0 + profile.len()].copy_from_slice(profile.as_bytes());
        for (i, part) in [0_u16, 3, 1].into_iter().enumerate() {
            block[VERSION_AT + 2 * i..VERSION_AT + 2 * i + 2].copy_from_slice(&part.to_le_bytes());
        }
        block[FLAGS_AT..FLAGS_AT + 2].copy_from_slice(&FLAG_DEV.to_le_bytes());
        block[GIT_SHA.0..GIT_SHA.0 + 7].copy_from_slice(b"abc1234");
    }

    /// Fills `image_len` and `crc32` the way `euc image seal` will.
    pub(crate) fn seal(image: &mut [u8], offset: usize) {
        let len = image.len() as u32;
        image[offset + IMAGE_LEN_AT..offset + IMAGE_LEN_AT + 4].copy_from_slice(&len.to_le_bytes());
        let crc = info_crc32(image, offset);
        image[offset + CRC_AT..offset + CRC_AT + 4].copy_from_slice(&crc.to_le_bytes());
    }

    #[test]
    fn reads_an_unsealed_block() {
        let mut image = vec![0xA5; 0x400];
        write_block(&mut image, 0x140, "begode-extreme", "clean");
        let info = EucInfo::find(&image, 0x140).unwrap().unwrap();
        assert_eq!(info.board, "begode-extreme");
        assert_eq!(info.profile, BuildProfile::Clean);
        assert_eq!(info.version.to_string(), "0.3.1");
        assert_eq!(info.git_sha, "abc1234");
        assert!(info.dev && !info.signed && !info.is_sealed());
    }

    #[test]
    fn no_magic_or_short_image_means_no_block() {
        assert_eq!(EucInfo::find(&[0xA5; 0x400], 0x140), Ok(None));
        assert_eq!(EucInfo::find(&[0xA5; 0x150], 0x140), Ok(None));
    }

    #[test]
    fn bad_fields_are_errors() {
        let mut image = vec![0; 0x400];
        let mut broken = |edit: &dyn Fn(&mut [u8])| {
            write_block(&mut image, 0x140, "begode-extreme", "compat");
            edit(&mut image[0x140..0x1C0]);
            EucInfo::find(&image, 0x140).unwrap_err()
        };
        assert_eq!(broken(&|b| b[LAYOUT_AT] = 2), InfoError::Layout(2));
        assert_eq!(broken(&|b| b[SIZE_AT] = 64), InfoError::Size(64));
        assert_eq!(broken(&|b| b[FLAGS_AT] = 3), InfoError::Flags(3));
        assert_eq!(broken(&|b| b[RESERVED_AT] = 1), InfoError::Reserved);
        assert_eq!(broken(&|b| b[BOARD.0 + 15] = b'x'), InfoError::Text("board"));
        assert_eq!(broken(&|b| b[GIT_SHA.0] = 0x80), InfoError::Text("git_sha"));
        assert_eq!(
            broken(&|b| b[PROFILE.0..PROFILE.0 + 6].copy_from_slice(b"turbo\0")),
            InfoError::Profile("turbo".into())
        );
    }

    #[test]
    fn crc_ignores_the_crc_and_signature_fields() {
        let mut image: Vec<u8> = (0..0x300_u32).map(|i| (i * 7) as u8).collect();
        write_block(&mut image, 0x140, "begode-extreme", "clean");
        let before = info_crc32(&image, 0x140);
        image[0x140 + CRC_AT] ^= 0xFF;
        image[0x140 + SIGNATURE.0 + 63] ^= 0xFF;
        assert_eq!(info_crc32(&image, 0x140), before);
        image[0x2FF] ^= 1;
        assert_ne!(info_crc32(&image, 0x140), before);
    }

    #[test]
    fn crc_is_iso_hdlc() {
        // Zeroing the two fields of an all-zero block changes nothing, so this is the plain CRC.
        let image = vec![0_u8; 0x200];
        assert_eq!(info_crc32(&image, 0x140), crc32fast::hash(&image));
        // CRC-32/ISO-HDLC check value.
        assert_eq!(crc32fast::hash(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn seal_round_trips() {
        let mut image = vec![0x11; 0x400];
        write_block(&mut image, 0x140, "begode-extreme", "clean");
        seal(&mut image, 0x140);
        let info = EucInfo::find(&image, 0x140).unwrap().unwrap();
        assert!(info.is_sealed());
        assert_eq!(info.image_len, 0x400);
        assert_eq!(info.crc32, info_crc32(&image, 0x140));
    }
}
