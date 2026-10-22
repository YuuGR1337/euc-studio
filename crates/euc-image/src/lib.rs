//! The firmware image gate.
//!
//! Before an image goes near a wheel (or the flasher, or the studio's "run on hardware" path) it
//! must pass this gate for the wheel's profile: it fits the app slot, its vector table is sane,
//! and it is either a pinned stock build or one of ours carrying a valid `.euc_info` block
//! (`docs/spec/euc-info.md`). Everything here is pure: callers read the file, this crate only
//! looks at bytes and returns a typed [`Report`].
//!
//! ```no_run
//! # fn demo(bytes: &[u8], wheel: &euc_profile::WheelProfile) -> Result<(), euc_image::ImageError> {
//! let image = euc_image::Image::from_bytes(bytes)?;
//! let report = euc_image::validate(&image, wheel);
//! for check in &report.checks {
//!     println!("{}: {}", check.gate, check.outcome);
//! }
//! assert!(report.passed);
//! # Ok(())
//! # }
//! ```

mod elf;
mod gate;
mod image;
mod info;
mod seal;

pub use gate::{Check, Gate, Identity, Outcome, Problem, Report, validate};
pub use image::{Format, Image, ImageError};
pub use info::{BuildProfile, EucInfo, INFO_LEN, InfoError, Version, info_crc32};
pub use seal::{SealError, seal};
