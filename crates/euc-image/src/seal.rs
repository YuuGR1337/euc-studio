//! Unsigned development image sealing for layout 1.

use euc_profile::WheelProfile;
use serde::Serialize;
use thiserror::Error;

use crate::info::{CRC_AT, IMAGE_LEN_AT};
use crate::{Format, Identity, Image, Report, info_crc32, validate};

/// Seals a raw, unsigned development image for `wheel`.
///
/// Both the input and the result must pass [`validate`]. The input must carry our valid
/// layout-1 `.euc_info` block, set the development flag, and have an all-zero signature.
/// ELF files and images pinned as stock for this wheel are refused.
///
/// Returns a new image, changing only `image_len` and `crc32`. Metadata, the development flag,
/// the zero signature, and every payload byte are preserved. An already sealed image must
/// pass its existing length and CRC checks; sealing it again returns identical bytes.
/// This checksum detects corruption; it provides no cryptographic authenticity.
pub fn seal(image: &Image, wheel: &WheelProfile) -> Result<Image, SealError> {
    // Check the magic too: Image::raw must not provide a way to seal an ELF container.
    if image.format() != Format::Raw || image.bytes().starts_with(crate::elf::MAGIC) {
        return Err(SealError::NotRaw);
    }
    let report = validate(image, wheel);
    if !report.passed {
        return Err(SealError::InvalidInput { report: Box::new(report) });
    }
    // Check pins independently of identity: a pinned image must never become a seal input,
    // even if its bytes also happen to contain a valid info block.
    if wheel.find_pin(&report.sha256).is_some() {
        return Err(SealError::Stock);
    }
    let Identity::Ours { info } = report.identity else {
        return Err(SealError::MissingInfo);
    };
    if info.signed {
        return Err(SealError::Signed);
    }
    if !info.dev {
        return Err(SealError::NotDevelopment);
    }

    // The gate bounds the length by the profile's u32 app slot and checks the whole block.
    let mut bytes = image.bytes().to_vec();
    let len = bytes.len() as u32;
    let offset = wheel.board.app.info_offset as usize;
    bytes[offset + IMAGE_LEN_AT..offset + IMAGE_LEN_AT + 4].copy_from_slice(&len.to_le_bytes());
    let crc = info_crc32(&bytes, offset);
    bytes[offset + CRC_AT..offset + CRC_AT + 4].copy_from_slice(&crc.to_le_bytes());

    let sealed = Image::raw(bytes);
    let report = validate(&sealed, wheel);
    if !report.passed {
        return Err(SealError::InvalidOutput { report: Box::new(report) });
    }
    Ok(sealed)
}

/// Why unsigned development sealing was refused.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SealError {
    /// The input is an ELF container rather than raw flash bytes.
    #[error("sealing requires a raw BIN image; ELF inputs are refused")]
    NotRaw,
    /// The input failed the wheel's image gate. Existing seals are never repaired silently.
    #[error("the input image failed validation")]
    InvalidInput {
        /// All input gate checks, including the failures.
        report: Box<Report>,
    },
    /// The input matches a stock pin for the selected wheel.
    #[error("pinned stock images cannot be sealed")]
    Stock,
    /// The input does not carry our own valid identity block.
    #[error("sealing requires our valid layout-1 .euc_info block")]
    MissingInfo,
    /// Any nonzero signature byte prevents sealing.
    #[error("signed images cannot be sealed; sealing would change signed bytes")]
    Signed,
    /// Unsigned sealing must preserve an existing development flag.
    #[error("unsigned sealing requires the development flag")]
    NotDevelopment,
    /// The result failed the wheel's image gate.
    #[error("the sealed image failed validation")]
    InvalidOutput {
        /// All result gate checks, including the failures.
        report: Box<Report>,
    },
}
