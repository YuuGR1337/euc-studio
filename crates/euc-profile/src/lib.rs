//! Wheel profiles: the data that describes one wheel model.
//!
//! Each wheel lives in `profiles/<wheel>/wheel.toml`. A new wheel of a known kind is new data,
//! not new engine code. This crate owns the schema, parses it with errors that point at the bad
//! line, and checks the cross-field rules TOML cannot express (the app must not reach the storage
//! pages, pins must be unique, ...). It does no I/O beyond reading the profile files.
//!
//! The sha256 pins in a profile decide which firmware counts as official, so where a profile
//! came from matters. [`ProfileSource::Builtin`] holds the repo's profiles, compiled into the
//! binary; a folder on disk is used only when the caller names it ([`ProfileSource::Override`]),
//! and every loaded profile records its [`Origin`].
//!
//! ```
//! let profile = euc_profile::WheelProfile::parse(r#"
//!     schema = 1
//!     id = "demo-wheel"
//!     name = "Demo Wheel"
//!     brand = "Demo"
//!
//!     [board]
//!     id = "demo-board"
//!     mcu = "STM32F103RC-class"
//!     core = "Cortex-M3"
//!     hclk_hz = 72_000_000
//!     app = { base = 0x0800_3000, max_size = 49_152, vectors = 76, info_offset = 0x140,
//!             storage_pages = [0x0800_F000] }
//!     ram = { base = 0x2000_0000, size = 0xC000 }
//! "#)?;
//! assert_eq!(profile.board.app.end(), 0x0800_F000);
//! # Ok::<(), euc_profile::ParseError>(())
//! ```

mod builtin;
mod date;
mod dir;
mod sha;
mod source;
mod text;
mod validate;

use std::path::PathBuf;

use serde::Deserialize;

pub use date::Date;
pub use dir::{ProfileDir, ProfileError, WHEEL_FILE};
pub use sha::{BadSha256, Sha256};
pub use source::{Origin, ProfileSource};
pub use text::{is_unsafe_display_char, sanitize_display};
pub use validate::{ParseError, Problem};

/// The schema version this build reads.
pub const SCHEMA: u32 = 1;

/// One wheel model.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WheelProfile {
    /// Schema version of the file; must be [`SCHEMA`].
    pub schema: u32,
    /// Stable id in kebab case; also the profile's folder name (`begode-extreme`).
    pub id: String,
    /// Display name (`Begode Extreme`).
    pub name: String,
    /// Maker (`Begode`).
    pub brand: String,
    /// The main controller board.
    pub board: Board,
    /// Known firmware builds and where to get them.
    #[serde(default)]
    pub firmware: Vec<FirmwareSource>,
    /// Where this profile was loaded from; set by the loader, not part of the file.
    #[serde(skip)]
    pub origin: Origin,
}

impl WheelProfile {
    /// Every pinned build (sources with a sha256), in file order.
    pub fn pins(&self) -> impl Iterator<Item = Pin<'_>> {
        self.firmware.iter().filter_map(FirmwareSource::pin)
    }

    /// The pinned build with this digest, if any.
    pub fn find_pin(&self, sha256: &Sha256) -> Option<Pin<'_>> {
        self.pins().find(|pin| pin.sha256 == *sha256)
    }
}

/// Summary of the main controller board: enough to identify it and to gate an image for it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Board {
    /// Board id; images built for this board carry it in their `.euc_info` block (16 bytes max).
    pub id: String,
    /// Microcontroller (`STM32F103RC-class`).
    pub mcu: String,
    /// CPU core (`Cortex-M3`).
    pub core: String,
    /// Core clock in Hz.
    pub hclk_hz: u32,
    /// Where the controller app lives in flash.
    pub app: AppLayout,
    /// SRAM; a valid initial stack pointer lies inside it.
    pub ram: Ram,
}

/// The app slot in flash, after the vendor bootloader and before the settings pages.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppLayout {
    /// Load address of the image and of its vector table.
    pub base: u32,
    /// Largest image that fits without touching `storage_pages`.
    pub max_size: u32,
    /// Entries in the vector table, including the initial stack pointer.
    pub vectors: u32,
    /// Offset of the `.euc_info` block from the image start (see `docs/spec/euc-info.md`).
    pub info_offset: u32,
    /// Flash pages the firmware keeps settings in. An image must never overlap them.
    #[serde(default)]
    pub storage_pages: Vec<u32>,
}

impl AppLayout {
    /// First address past the app slot.
    pub const fn end(&self) -> u32 {
        self.base + self.max_size
    }
}

/// A RAM region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ram {
    /// First address.
    pub base: u32,
    /// Size in bytes.
    pub size: u32,
}

impl Ram {
    /// First address past the region.
    pub const fn end(&self) -> u32 {
        self.base + self.size
    }
}

/// Where one firmware build comes from.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum FirmwareSource {
    /// An official build on Begode's public update server.
    BegodeApi(BegodeBuild),
    /// A known build that cannot be downloaded; the user imports it and its sha256 is checked.
    Manual(ManualBuild),
    /// An image built from this repo. Not pinned: it changes with every build.
    Local(LocalBuild),
}

impl FirmwareSource {
    /// The pin for this source, if it has one.
    pub fn pin(&self) -> Option<Pin<'_>> {
        match self {
            Self::BegodeApi(b) => Some(Pin { sha256: b.sha256, label: &b.label, date: b.date }),
            Self::Manual(b) => Some(Pin { sha256: b.sha256, label: &b.label, date: b.date }),
            Self::Local(_) => None,
        }
    }

    /// Human label of the build.
    pub fn label(&self) -> &str {
        match self {
            Self::BegodeApi(b) => &b.label,
            Self::Manual(b) => &b.label,
            Self::Local(b) => &b.label,
        }
    }
}

/// A build served by Begode's update API (`kind = "begode-api"`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BegodeBuild {
    /// Product code the API lists the build under (`firmwareTypeCode`). Required for the
    /// current channel, which `euc fetch` checks against the server.
    pub type_code: Option<String>,
    /// File name on the server (`pwm_<upload ms>.bin`).
    pub file: String,
    /// Pinned digest of the official bytes.
    pub sha256: Sha256,
    /// Firmware name shown to people.
    pub label: String,
    /// Release date from the server notes.
    pub date: Option<Date>,
    /// Whether the server still offers this build.
    #[serde(default)]
    pub channel: Channel,
}

/// A pinned build the user supplies by hand (`kind = "manual"`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManualBuild {
    /// Pinned digest.
    pub sha256: Sha256,
    /// Firmware name shown to people.
    pub label: String,
    /// Release date, if known.
    pub date: Option<Date>,
    /// Where to find the file, shown when it is missing.
    pub hint: Option<String>,
}

/// An image built in this repo (`kind = "local"`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalBuild {
    /// Path relative to the repo root.
    pub path: PathBuf,
    /// Name shown to people.
    pub label: String,
}

/// Release channel of a vendor build.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Channel {
    /// The build the server offers today. `euc fetch` downloads it by default.
    #[default]
    Current,
    /// An older build the server no longer lists but still serves. Pinned so it is recognized.
    Archive,
}

/// A pinned firmware build: the digest that identifies it and its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pin<'a> {
    /// Digest of the official bytes.
    pub sha256: Sha256,
    /// Firmware name.
    pub label: &'a str,
    /// Release date, if known.
    pub date: Option<Date>,
}
