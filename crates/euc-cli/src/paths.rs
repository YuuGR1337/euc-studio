//! Where the profiles and the firmware cache live.

use std::env;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};
use euc_fwsource::FirmwareCache;
use euc_profile::{ProfileDir, WheelProfile};

use crate::Context;

impl Context {
    /// The profiles folder: `--profiles` / `EUC_PROFILES`, else the nearest `profiles/` above
    /// the working directory, else above the `euc` binary (so `target/debug/euc` works from
    /// anywhere in a checkout).
    pub(crate) fn profiles(&self) -> anyhow::Result<ProfileDir> {
        if let Some(dir) = &self.profiles {
            return Ok(ProfileDir::new(dir));
        }
        let exe_dir = env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf));
        for start in [env::current_dir().ok(), exe_dir].into_iter().flatten() {
            for dir in start.ancestors() {
                let candidate = ProfileDir::new(dir.join("profiles"));
                if candidate.ids().is_ok_and(|ids| !ids.is_empty()) {
                    return Ok(candidate);
                }
            }
        }
        bail!("cannot find the profiles folder; run inside the euc-studio checkout or set EUC_PROFILES")
    }

    /// Loads one wheel profile.
    pub(crate) fn wheel(&self, id: &str) -> anyhow::Result<WheelProfile> {
        Ok(self.profiles()?.load(id)?)
    }

    /// The firmware cache: `--cache-dir` / `EUC_CACHE_DIR`, else the per-user cache.
    pub(crate) fn cache(&self) -> anyhow::Result<FirmwareCache> {
        let root: Option<PathBuf> = self.cache_dir.clone().or_else(FirmwareCache::default_root);
        Ok(FirmwareCache::new(root.context("this system reports no per-user cache folder; set EUC_CACHE_DIR")?))
    }
}
