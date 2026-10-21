use std::ops::Range;

use euc_profile::WheelProfile;
use serde::Serialize;
use thiserror::Error;

/// Synthetic storage geometry; not a discovered vendor flash layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    /// First flash address.
    pub flash_base: u32,
    /// Total flash size, at most 16 MiB.
    pub flash_size: u32,
    /// Erase page size; also the modeled settings page size.
    pub page_size: u32,
}

/// Checked address ranges with exclusive ends. Only `application` is writable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Regions {
    /// Entire synthetic storage.
    pub flash: Range<u32>,
    /// Protected storage below the application.
    pub bootloader: Range<u32>,
    /// The single writable application slot.
    pub application: Range<u32>,
    /// Protected settings pages from the wheel profile.
    pub settings: Vec<Range<u32>>,
}

/// A profile or geometry that cannot define a safe simulation.
#[derive(Debug, Error)]
pub enum SetupError {
    /// The wheel profile failed its existing validation rules.
    #[error(transparent)]
    Profile(#[from] euc_profile::Problem),
    /// The supplied geometry breaks the model's region rules.
    #[error("invalid simulated flash geometry: {0}")]
    Geometry(&'static str),
}

impl Geometry {
    pub(crate) fn regions(self, wheel: &WheelProfile) -> Result<Regions, SetupError> {
        let invalid = SetupError::Geometry;
        if self.flash_size == 0 || self.flash_size > 16 * 1024 * 1024 || self.page_size == 0 {
            return Err(invalid("flash must contain 1..=16 MiB of bytes; page size must be nonzero"));
        }
        let flash_end = self.flash_base.checked_add(self.flash_size).ok_or(invalid("flash address overflow"))?;
        let app = &wheel.board.app;
        let app_end = app.base.checked_add(app.max_size).ok_or(invalid("application address overflow"))?;
        if app.max_size == 0 || app.base <= self.flash_base || app_end > flash_end {
            return Err(invalid("application must follow a nonempty bootloader and fit inside flash"));
        }
        if ![self.flash_base, self.flash_size, app.base, app.max_size]
            .into_iter()
            .all(|value| value.is_multiple_of(self.page_size))
        {
            return Err(invalid("flash and application bounds must align to erase pages"));
        }
        // Check geometry before profile validation: the bounded app size also bounds its vector table.
        wheel.validate()?;
        let mut regions = Regions {
            flash: self.flash_base..flash_end,
            bootloader: self.flash_base..app.base,
            application: app.base..app_end,
            settings: Vec::new(),
        };
        for &start in &app.storage_pages {
            let end = start.checked_add(self.page_size).ok_or(invalid("settings address overflow"))?;
            let page = start..end;
            if !start.is_multiple_of(self.page_size)
                || start < self.flash_base
                || end > flash_end
                || overlaps(&page, &regions.bootloader)
                || overlaps(&page, &regions.application)
                || regions.settings.iter().any(|other| overlaps(&page, other))
            {
                return Err(invalid("settings pages must be aligned, inside flash, and disjoint from other regions"));
            }
            regions.settings.push(page);
        }
        Ok(regions)
    }
}

fn overlaps(left: &Range<u32>, right: &Range<u32>) -> bool {
    left.start < right.end && right.start < left.end
}
