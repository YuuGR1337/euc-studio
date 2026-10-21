//! Firmware selection for the desktop simulator. This module never writes to hardware.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static RESOURCE_FIRMWARE: OnceLock<PathBuf> = OnceLock::new();

pub fn set_resource_directory(directory: PathBuf) {
    let _ = RESOURCE_FIRMWARE.set(directory.join("firmware"));
}

use euc_profile::Sha256;
use euc_sim::WheelFiles;
use euc_sim::config::StockMap;
use serde::Serialize;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirmwareInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub detail: &'static str,
    pub disabled_reason: Option<String>,
}

pub struct Loaded {
    pub image: Vec<u8>,
    pub label: String,
    pub map: Option<StockMap>,
}

pub fn validate_choice(choice: &str) -> Result<(), String> {
    match choice {
        "stock" | "ours-compat" | "ours-clean" => Ok(()),
        _ => Err(format!("Unknown firmware selection: {choice}")),
    }
}

pub fn catalogue(wheel: &str) -> Vec<FirmwareInfo> {
    [
        ("stock", "Stock", "Official pinned firmware", None),
        ("ours-compat", "Ours-compat", "Our firmware with stock behavior", Some("compat")),
        ("ours-clean", "Ours-clean", "Our firmware with documented fixes", Some("clean")),
    ]
    .into_iter()
    .map(|(id, label, detail, profile)| FirmwareInfo {
        id,
        label,
        detail,
        disabled_reason: profile.and_then(|p| own_build(p, wheel).err()),
    })
    .collect()
}

/// Finds original project firmware builds, never an unverified vendor image.
pub(super) fn own_build(profile: &str, wheel: &str) -> Result<PathBuf, String> {
    let filename = format!("euc-fw-{wheel}.bin");
    let mut roots = Vec::new();
    if let Some(dir) = std::env::var_os("EUC_FIRMWARE_DIR") {
        roots.push(PathBuf::from(dir));
    }
    if let Some(root) = RESOURCE_FIRMWARE.get() {
        roots.push(root.clone());
    }
    // Explicitly chosen development inputs. Bundled builds can use EUC_FIRMWARE_DIR.
    if cfg!(dev) {
        roots.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../firmware/build"));
        if let Ok(cwd) = std::env::current_dir() {
            roots.extend(cwd.ancestors().map(|p| p.join("firmware/build")));
        }
    }
    for root in roots {
        for path in [root.join(format!("target-{profile}")).join(&filename), root.join(profile).join(&filename)] {
            if path.is_file() && path.with_extension("elf").is_file() {
                return Ok(path);
            }
        }
    }
    Err(format!("Build {profile} with its ELF, then set EUC_FIRMWARE_DIR to firmware/build and restart the app"))
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err(format!("{} exceeds its file size limit", path.display()));
    }
    Ok(bytes)
}

pub fn load(files: &WheelFiles, choice: &str, progress: &mut dyn FnMut(&str)) -> Result<Loaded, String> {
    validate_choice(choice)?;
    let wheel = &files.profile;
    let (bytes, label, elf) = if choice == "stock" {
        let (bytes, label) = super::firmware::stock_image(wheel, progress)?;
        (bytes, label, None)
    } else {
        let profile = choice.trim_start_matches("ours-");
        progress("Loading our firmware and its probe map");
        let path = own_build(profile, &wheel.id)?;
        let bytes = read_bounded(&path, u64::from(wheel.board.app.max_size))?;
        let elf = read_bounded(&path.with_extension("elf"), 16 * 1024 * 1024)?;
        (bytes, format!("{profile} ({})", Sha256::digest(&elf).short()), Some(elf))
    };
    let image = euc_image::Image::from_bytes(&bytes).map_err(|e| e.to_string())?;
    let report = euc_image::validate(&image, wheel);
    if !report.passed {
        let failures = report.checks.iter().map(|c| format!("{}: {}", c.gate, c.outcome)).collect::<Vec<_>>();
        return Err(format!("Image validation failed: {}", failures.join("; ")));
    }
    if choice != "stock" {
        let expected = choice.trim_start_matches("ours-");
        match &report.identity {
            euc_image::Identity::Ours { info } if info.profile.to_string() == expected => {}
            _ => return Err(format!("The selected {expected} image contains another firmware profile")),
        }
    }
    let hash = Sha256::digest(image.bytes());
    let stock = wheel.find_pin(&hash).is_some_and(|p| p.label == files.stockmap.build);
    let map = if stock {
        Some(files.stockmap.clone())
    } else if let Some(elf) = elf {
        let flattened = euc_image::Image::from_bytes(&elf).map_err(|e| format!("Firmware ELF: {e}"))?;
        if flattened.load_addr() != Some(wheel.board.app.base) || flattened.bytes() != image.bytes() {
            return Err("The BIN and ELF do not contain the same application".into());
        }
        let probes = files.probes.as_ref().ok_or("This wheel has no own-firmware probe map")?;
        Some(probes.resolve(&elf, &label)?)
    } else {
        None
    };
    Ok(Loaded { image: image.bytes().to_vec(), label: format!("{label} · {}", hash.short()), map })
}
