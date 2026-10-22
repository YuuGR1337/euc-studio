//! Firmware selection for the desktop simulator. This module never writes to hardware.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static RESOURCE_FIRMWARE: OnceLock<PathBuf> = OnceLock::new();

pub fn set_resource_directory(directory: PathBuf) {
    let _ = RESOURCE_FIRMWARE.set(directory.join("firmware"));
}

use euc_profile::Sha256;
use euc_sim::config::StockMap;
use euc_sim::sil::SilLibrary;
use euc_sim::{CpuFactory, ExceptionTiming, WheelFiles};
use serde::Serialize;

/// The firmware choice that runs our firmware as the host SIL library with a quirk mask:
/// `ours-custom` (no quirk) or `ours-custom:NAME,NAME` (those quirks on).
pub const OURS_CUSTOM: &str = "ours-custom";

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
    /// For `ours-custom`: the SIL library and its quirk mask (`image` is then only the vector
    /// table that routes interrupts to it).
    pub sil: Option<SilRun>,
}

/// Our firmware as the host SIL library, with these quirks.
pub struct SilRun {
    pub library: SilLibrary,
    pub mask: u64,
}

impl Loaded {
    /// The CPU the firmware runs on: the SIL library, or this build's backend.
    pub fn factory(&self) -> Result<Box<CpuFactory<'static>>, String> {
        match &self.sil {
            Some(sil) => Ok(sil.library.factory(sil.mask)),
            None => super::cpu::factory(),
        }
    }

    /// Exception switch costs: none for the SIL library, which has no instruction timing.
    pub fn exception_timing(&self) -> ExceptionTiming {
        if self.sil.is_some() { euc_sim::sil::EXCEPTION_TIMING } else { ExceptionTiming::default() }
    }

    /// `sil`, or the backend's name, for recordings and the status bar.
    pub fn cpu_name(&self) -> &'static str {
        if self.sil.is_some() { "sil" } else { super::cpu::backend().name() }
    }

    /// The fixed cost per instruction a recording names, if the run has one.
    pub fn cpi(&self) -> Option<f64> {
        self.sil.is_none().then_some(euc_sim::DEFAULT_CPI)
    }
}

/// The quirk names of an `ours-custom` choice, or `None` for any other choice.
pub fn custom_quirks(choice: &str) -> Option<Vec<&str>> {
    let rest = choice.strip_prefix(OURS_CUSTOM)?;
    match rest.strip_prefix(':') {
        Some(list) => Some(list.split(',').map(str::trim).filter(|n| !n.is_empty()).collect()),
        None if rest.is_empty() => Some(Vec::new()),
        None => None,
    }
}

pub fn validate_choice(choice: &str) -> Result<(), String> {
    match choice {
        "stock" | "ours-compat" | "ours-clean" => Ok(()),
        _ => match custom_quirks(choice) {
            Some(names) if names.iter().all(|n| n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')) => Ok(()),
            _ => Err(format!("Unknown firmware selection: {choice}")),
        },
    }
}

pub fn catalogue(wheel: &str) -> Vec<FirmwareInfo> {
    [
        ("stock", "Stock", "Official pinned firmware", None),
        ("ours-compat", "Ours-compat", "Our firmware with stock behavior", Some("compat")),
        ("ours-clean", "Ours-clean", "Our firmware with documented fixes", Some("clean")),
        (OURS_CUSTOM, "Ours-custom", "Ours (custom quirks): our firmware on the host, quirks switched in Tune", None),
    ]
    .into_iter()
    .map(|(id, label, detail, profile)| FirmwareInfo {
        id,
        label,
        detail,
        disabled_reason: match profile {
            Some(p) => own_build(p, wheel).err(),
            None if id == OURS_CUSTOM => sil_library().err(),
            None => None,
        },
    })
    .collect()
}

/// Where our builds are looked for: `EUC_FIRMWARE_DIR`, the bundle's resources, and in development
/// builds this checkout's `firmware/build`.
fn build_roots() -> Vec<PathBuf> {
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
    roots
}

/// Our firmware as the host SIL library (`cmake --workflow --preset host-sil`).
pub(super) fn sil_library() -> Result<PathBuf, String> {
    if let Some(why) = euc_sim::sil::UNSUPPORTED {
        return Err(why.to_owned());
    }
    let file = euc_sim::sil::library_file_name();
    for root in build_roots() {
        for path in [root.join("host-sil").join(file), root.join("sil").join(file)] {
            if path.is_file() {
                return Ok(path);
            }
        }
    }
    Err("Build the SIL library (cmake --workflow --preset host-sil in firmware/), then set EUC_FIRMWARE_DIR to \
         firmware/build and restart the app"
        .into())
}

/// Our firmware as the SIL library with the quirks `names` on.
fn load_sil(files: &WheelFiles, names: &[&str], progress: &mut dyn FnMut(&str)) -> Result<Loaded, String> {
    progress("Loading our firmware as the host SIL library");
    let path = sil_library()?;
    let library = SilLibrary::open(&path)?;
    let spec: Vec<String> = std::iter::once("none".to_owned()).chain(names.iter().map(|n| format!("+{n}"))).collect();
    let mask = library.parse_quirks(&spec.join(","))?;
    let bytes = read_bounded(&path, 64 * 1024 * 1024)?;
    let label = format!("ours-sil ({}) · {}", library.describe(mask), Sha256::digest(&bytes).short());
    let image = library.image(files.profile.board.app.base);
    let map = Some(library.map(&label));
    Ok(Loaded { image, label, map, sil: Some(SilRun { library, mask }) })
}

/// Finds original project firmware builds, never an unverified vendor image.
pub(super) fn own_build(profile: &str, wheel: &str) -> Result<PathBuf, String> {
    let filename = format!("euc-fw-{wheel}.bin");
    for root in build_roots() {
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
    if let Some(names) = custom_quirks(choice) {
        return load_sil(files, &names, progress);
    }
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
    Ok(Loaded { image: image.bytes().to_vec(), label: format!("{label} · {}", hash.short()), map, sil: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_quirk_choices_parse_and_others_are_refused() {
        assert_eq!(custom_quirks("ours-custom"), Some(vec![]));
        assert_eq!(custom_quirks("ours-custom:"), Some(vec![]));
        assert_eq!(custom_quirks("ours-custom:ASIN_ACCEL, KEIL_FTZ"), Some(vec!["ASIN_ACCEL", "KEIL_FTZ"]));
        assert_eq!(custom_quirks("ours-customs"), None);
        assert_eq!(custom_quirks("ours-clean"), None);
        assert!(validate_choice("ours-custom:ASIN_ACCEL,NO_WATCHDOG").is_ok());
        assert!(validate_choice("ours-custom:ASIN ACCEL").is_err(), "names are identifiers");
        assert!(validate_choice("ours-custom:../x").is_err());
        assert!(validate_choice("stock").is_ok() && validate_choice("ours-sil").is_err());
    }

    #[test]
    fn the_catalogue_offers_the_custom_quirks_choice() {
        let custom = catalogue("begode-extreme").into_iter().find(|f| f.id == OURS_CUSTOM).expect("listed");
        assert_eq!(custom.label, "Ours-custom");
        assert_eq!(custom.disabled_reason.is_none(), sil_library().is_ok());
        if cfg!(windows) {
            assert_eq!(custom.disabled_reason.as_deref(), Some("ours-sil is not supported on Windows yet"));
        }
    }

    /// Needs `cmake --workflow --preset host-sil` in firmware/.
    #[test]
    #[ignore = "needs firmware/build/host-sil"]
    fn the_custom_choice_loads_the_sil_library_with_its_mask() {
        let files = WheelFiles::load(&euc_profile::ProfileSource::Builtin, "begode-extreme").unwrap();
        let loaded = load(&files, "ours-custom:ASIN_ACCEL", &mut |_| {}).unwrap();
        let sil = loaded.sil.as_ref().expect("a SIL run");
        assert_eq!(sil.library.describe(sil.mask), "none+ASIN_ACCEL");
        assert!(loaded.label.starts_with("ours-sil (none+ASIN_ACCEL)"), "{}", loaded.label);
        assert!(loaded.map.as_ref().is_some_and(|m| m.ram.iter().any(|v| v.name == "speed_cms")));
        assert_eq!((loaded.cpu_name(), loaded.cpi()), ("sil", None));
    }
}
