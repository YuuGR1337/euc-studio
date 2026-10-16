//! Where the profiles and the firmware cache come from.

use std::path::PathBuf;

use anyhow::Context as _;
use euc_fwsource::FirmwareCache;
use euc_profile::{ProfileDir, ProfileSource, WheelProfile};

use crate::Context;
use crate::ui;

impl Context {
    /// The built-in profiles, unless `--profiles` / `EUC_PROFILES` names a folder.
    ///
    /// Nothing is ever picked up from the working directory: profile pins decide which firmware
    /// counts as official, so an override must be explicit and is announced on stderr once.
    pub(crate) fn profiles(&self) -> ProfileSource {
        let Some(dir) = &self.profiles else { return ProfileSource::Builtin };
        if !self.warned.replace(true) {
            ui::warn(format!("using profiles from {}, not the built-in set", ui::path(dir)));
        }
        ProfileSource::Override(ProfileDir::new(dir))
    }

    /// Loads one wheel profile from [`Self::profiles`].
    pub(crate) fn wheel(&self, id: &str) -> anyhow::Result<WheelProfile> {
        Ok(self.profiles().load(id)?)
    }

    /// The firmware cache: `--cache-dir` / `EUC_CACHE_DIR`, else the per-user cache.
    pub(crate) fn cache(&self) -> anyhow::Result<FirmwareCache> {
        let root: Option<PathBuf> = self.cache_dir.clone().or_else(FirmwareCache::default_root);
        Ok(FirmwareCache::new(root.context("this system reports no per-user cache folder; set EUC_CACHE_DIR")?))
    }
}
