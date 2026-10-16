use std::fmt;

use serde::Serialize;
use thiserror::Error;

use crate::elf;

/// How the image was supplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Format {
    /// Raw flash bytes (`.bin`), assumed to load at the profile's app base.
    Raw,
    /// An ELF file; its loadable segments are flattened at their load addresses.
    Elf,
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Raw => "raw binary",
            Self::Elf => "ELF",
        })
    }
}

/// Flash bytes ready for the gate, plus where they were linked when the file says so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    bytes: Vec<u8>,
    format: Format,
    load_addr: Option<u32>,
}

impl Image {
    /// Detects the format of `data`: ELF by its magic, anything else as a raw binary.
    pub fn from_bytes(data: &[u8]) -> Result<Self, ImageError> {
        if data.starts_with(elf::MAGIC) {
            let (load_addr, bytes) = elf::flatten(data)?;
            Ok(Self { bytes, format: Format::Elf, load_addr: Some(load_addr) })
        } else {
            Ok(Self::raw(data.to_vec()))
        }
    }

    /// Raw flash bytes.
    pub fn raw(bytes: Vec<u8>) -> Self {
        Self { bytes, format: Format::Raw, load_addr: None }
    }

    /// The flash bytes, from the load address up.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// How the image was supplied.
    pub fn format(&self) -> Format {
        self.format
    }

    /// Where the first byte loads, when the file records it (ELF). `None` for raw binaries.
    pub fn load_addr(&self) -> Option<u32> {
        self.load_addr
    }
}

/// A file that cannot be turned into flash bytes.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ImageError {
    /// The ELF headers do not parse.
    #[error("not a readable ELF file: {0}")]
    Elf(String),
    /// Firmware for this project is 32-bit little-endian ARM.
    #[error("ELF is not 32-bit little-endian ARM")]
    NotArm,
    /// Nothing to flash.
    #[error("ELF has no loadable bytes")]
    Empty,
    /// The segments are spread too far apart to be one flash image, usually a section whose load
    /// address (LMA) was left in RAM by the linker script.
    #[error("ELF segments span 0x{start:08X}..0x{end:08X}, more than a flash image can")]
    Sparse {
        /// Lowest load address.
        start: u32,
        /// End of the highest segment.
        end: u64,
    },
}
