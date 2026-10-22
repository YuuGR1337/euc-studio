//! `ours-sil`: our firmware as the host SIL library (`firmware/build/host-sil/`), with its quirks
//! chosen at run time (euc-sim `sil`, docs/sil.md).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use euc_profile::Sha256;
use euc_sim::WheelFiles;
use euc_sim::sil::{self, SilLibrary};

use crate::sim::Firmware;
use crate::ui;

/// The firmware value that runs the SIL library this checkout built.
pub(crate) const OURS_SIL: &str = "ours-sil";

/// A SIL run's library and quirk mask.
pub(crate) struct SilChoice {
    pub(crate) library: SilLibrary,
    pub(crate) mask: u64,
}

/// The library `spec` names, with the quirks written into it: `ours-sil` (the build in
/// `firmware/build/host-sil/`, here or above), `ours-sil:QUIRKS`, or a library file
/// (`.dylib`, `.so`, `.dll`). `None` for any other firmware.
pub(crate) fn library(spec: &str) -> anyhow::Result<Option<(PathBuf, Option<&str>)>> {
    let (head, inline) = match spec.split_once(':') {
        Some((head, quirks)) if head == OURS_SIL => (head, Some(quirks)),
        _ => (spec, None),
    };
    let path = Path::new(spec);
    let is_library = path.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e, "dylib" | "so" | "dll"));
    if let Some(why) = sil::UNSUPPORTED.filter(|_| head == OURS_SIL || is_library) {
        anyhow::bail!("{why}");
    }
    if head == OURS_SIL {
        let cwd = std::env::current_dir()?;
        let path = sil::find_library(&cwd).with_context(|| {
            format!(
                "no firmware/build/host-sil/{} here or above; build it with \
                 `(cd firmware && cmake --workflow --preset host-sil)`",
                sil::library_file_name()
            )
        })?;
        return Ok(Some((path, inline)));
    }
    Ok(is_library.then(|| (path.to_path_buf(), None)))
}

/// The SIL firmware at `path` with `quirks` (compat when `None`).
pub(crate) fn load(files: &WheelFiles, path: &Path, quirks: Option<&str>) -> anyhow::Result<Firmware> {
    let library = SilLibrary::open(path).map_err(anyhow::Error::msg)?;
    let mask = match quirks {
        Some(spec) => library.parse_quirks(spec).map_err(|why| anyhow::anyhow!("--quirks {spec}: {why}"))?,
        None => library.compat_mask(),
    };
    let bytes = fs::read(path).with_context(|| format!("cannot read {}", ui::path(path)))?;
    let label = format!("{OURS_SIL} ({})", library.describe(mask));
    let image = library.image(files.profile.board.app.base);
    let map = Some(library.map(&label));
    Ok(Firmware {
        image,
        label,
        sha256: Sha256::digest(&bytes),
        stock: false,
        map,
        sil: Some(SilChoice { library, mask }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sil_values_are_refused_on_windows_and_others_pass_through() {
        for spec in ["ours-sil", "ours-sil:clean", "build/libeuc_sil.so", "x/euc_sil.dll"] {
            let result = library(spec);
            if cfg!(windows) {
                assert_eq!(result.unwrap_err().to_string(), "ours-sil is not supported on Windows yet");
            } else if spec.contains('/') {
                assert_eq!(result.unwrap().map(|(path, _)| path), Some(PathBuf::from(spec)));
            }
        }
        for spec in ["stock", "compat", "image.bin"] {
            assert!(library(spec).unwrap().is_none(), "{spec} is not a SIL value");
        }
    }
}
