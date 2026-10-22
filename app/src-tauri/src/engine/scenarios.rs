//! The scenarios the studio offers: the repository's `scenarios/*.toml`. A dev build reads the
//! folder when the web view connects, so a new scenario shows without a rebuild; all shipped scenarios are
//! built in. The ride and balance scenarios come first.

use std::path::Path;
use std::sync::{Mutex, OnceLock};

use euc_sim::scenario::Scenario;
use serde::Serialize;

const BUILT_IN: &[(&str, &str, &str)] = &[
    ("ride-20kmh-stop", "Ride to 20 km/h and stop", include_str!("../../../../scenarios/ride-20kmh-stop.toml")),
    ("balance-60s", "Balance for 60 s", include_str!("../../../../scenarios/balance-60s.toml")),
    (
        "ble-service-cut-standing",
        "Ble service cut standing",
        include_str!("../../../../scenarios/ble-service-cut-standing.toml"),
    ),
    ("coasting-stop", "Coasting stop", include_str!("../../../../scenarios/coasting-stop.toml")),
    ("downhill-cruise-2pct", "Downhill cruise 2pct", include_str!("../../../../scenarios/downhill-cruise-2pct.toml")),
    (
        "full-effort-70kmh-stop-soc100",
        "70 km/h acceleration and braking · 100% battery",
        include_str!("../../../../scenarios/full-effort-70kmh-stop-soc100.toml"),
    ),
    (
        "full-effort-70kmh-stop-soc50",
        "70 km/h acceleration and braking · 50% battery",
        include_str!("../../../../scenarios/full-effort-70kmh-stop-soc50.toml"),
    ),
    (
        "full-effort-70kmh-stop-soc20",
        "70 km/h acceleration and braking · 20% battery",
        include_str!("../../../../scenarios/full-effort-70kmh-stop-soc20.toml"),
    ),
    (
        "hall-loss-while-riding",
        "Hall loss while riding",
        include_str!("../../../../scenarios/hall-loss-while-riding.toml"),
    ),
    (
        "hall-single-line-loss",
        "Hall single line loss",
        include_str!("../../../../scenarios/hall-single-line-loss.toml"),
    ),
    ("hard-braking-30kmh", "Hard braking 30kmh", include_str!("../../../../scenarios/hard-braking-30kmh.toml")),
    (
        "hill-climb-low-battery",
        "Hill climb low battery",
        include_str!("../../../../scenarios/hill-climb-low-battery.toml"),
    ),
    ("low-battery-tiltback", "Low battery tiltback", include_str!("../../../../scenarios/low-battery-tiltback.toml")),
    (
        "overlean-no-pwm-tiltback",
        "Overlean no pwm tiltback",
        include_str!("../../../../scenarios/overlean-no-pwm-tiltback.toml"),
    ),
    ("overlean-top-speed", "Overlean top speed", include_str!("../../../../scenarios/overlean-top-speed.toml")),
    (
        "overlean-top-speed-30pct",
        "Overlean top speed 30pct",
        include_str!("../../../../scenarios/overlean-top-speed-30pct.toml"),
    ),
    ("pothole-20kmh", "Pothole 20kmh", include_str!("../../../../scenarios/pothole-20kmh.toml")),
    ("pothole-5cm-20kmh", "5 cm pothole at 20 km/h", include_str!("../../../../scenarios/pothole-5cm-20kmh.toml")),
    (
        "rider-steps-off-15kmh",
        "Rider steps off 15kmh",
        include_str!("../../../../scenarios/rider-steps-off-15kmh.toml"),
    ),
    ("rider-steps-off-5kmh", "Rider steps off 5kmh", include_str!("../../../../scenarios/rider-steps-off-5kmh.toml")),
];

/// A scenario as the run bar lists it.
#[derive(Debug, Clone, Serialize)]
pub struct ScenarioInfo {
    pub id: String,
    pub label: String,
    pub detail: String,
}

/// `(id, label, text)` of every scenario file, built-in ones first.
fn repository_files() -> Vec<(String, String, String)> {
    let mut files: Vec<_> =
        BUILT_IN.iter().map(|&(id, label, text)| (id.to_owned(), label.to_owned(), text.to_owned())).collect();
    if !cfg!(dev) {
        return files;
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenarios");
    let Ok(entries) = std::fs::read_dir(&dir) else { return files };
    let mut found: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    found.sort();
    for path in found {
        let (Some(id), Ok(text)) = (path.file_stem().and_then(|s| s.to_str()), std::fs::read_to_string(&path)) else {
            continue;
        };
        match files.iter_mut().find(|f| f.0 == id) {
            // The file on disk is newer than the copy built in.
            Some(file) => file.2 = text,
            None => files.push((id.to_owned(), label(id), text)),
        }
    }
    files
}

type ScenarioFile = (String, String, String);
static IMPORTED: OnceLock<Mutex<Vec<ScenarioFile>>> = OnceLock::new();

fn files() -> Vec<ScenarioFile> {
    let mut files = repository_files();
    if let Ok(imported) = IMPORTED.get_or_init(Mutex::default).lock() {
        files.extend(imported.iter().cloned());
    }
    files
}

/// Imports scenario text into this app session without changing repository files.
pub fn import(text: String) -> Result<ScenarioInfo, String> {
    if text.len() > 1_048_576 {
        return Err("Scenario files must be at most 1 MiB".into());
    }
    let scenario = Scenario::from_toml(&text).map_err(|e| e.to_string())?;
    scenario.terrain.validate()?;
    if !scenario.seconds.is_finite() || scenario.seconds > 3600.0 || scenario.terrain.length_m > 5000.0 {
        return Err("Desktop scenarios must last at most 3600 s and span at most 5000 m".into());
    }
    if scenario.expects.len() > 1000 || scenario.faults.len() > 1000 || scenario.terrain.features.len() > 1000 {
        return Err("A desktop scenario permits at most 1000 checks, faults, or terrain features".into());
    }
    euc_sim::WheelFiles::load(&euc_profile::ProfileSource::Builtin, &scenario.wheel).map_err(|e| e.to_string())?;
    let mut imported = IMPORTED.get_or_init(Mutex::default).lock().map_err(|_| "Scenario storage is unavailable")?;
    if imported.len() >= 64 {
        return Err("This app session already contains 64 imported scenarios".into());
    }
    let id = format!("custom-{}", imported.len() + 1);
    let info = ScenarioInfo { id: id.clone(), label: scenario.name.clone(), detail: scenario.description };
    imported.push((id, info.label.clone(), text));
    Ok(info)
}

/// "low-battery-tiltback" -> "Low battery tiltback", "hard-braking-30kmh" -> "Hard braking 30 km/h".
fn label(id: &str) -> String {
    let words: Vec<String> = id
        .split('-')
        .map(|w| {
            let digits = w.trim_end_matches(|c: char| c.is_ascii_alphabetic());
            match &w[digits.len()..] {
                "kmh" if !digits.is_empty() => format!("{digits} km/h"),
                "pct" if !digits.is_empty() => format!("{digits} %"),
                _ => w.to_owned(),
            }
        })
        .collect();
    let text = words.join(" ");
    let mut chars = text.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// Every scenario that parses, built-in ones first. Drafts (description "draft") are left out.
pub fn infos() -> Vec<ScenarioInfo> {
    files()
        .into_iter()
        .filter_map(|(id, label, text)| {
            let scenario = Scenario::from_toml(&text).ok()?;
            (scenario.description != "draft").then_some(ScenarioInfo { id, label, detail: scenario.description })
        })
        .collect()
}

/// Scenario `id`'s file as it is read (for recording headers).
pub fn text(id: &str) -> Option<String> {
    files().into_iter().find(|f| f.0 == id).map(|f| f.2)
}

/// Scenario `id`, parsed.
pub fn load(id: &str) -> Result<Scenario, String> {
    let (_, _, text) = files().into_iter().find(|f| f.0 == id).ok_or_else(|| format!("no scenario {id}"))?;
    Scenario::from_toml(&text).map_err(|e| format!("scenario {id}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_scenarios_parse_and_come_first() {
        let infos = infos();
        for (i, &(id, _, text)) in BUILT_IN.iter().enumerate() {
            assert_eq!(infos[i].id, id);
            assert_eq!(Scenario::from_toml(text).expect(id).name, id);
            assert_eq!(load(id).expect(id).name, id);
        }
        for info in &infos {
            // Imported IDs are session keys such as custom-1, not the scenario's name.
            load(&info.id).expect(&info.id);
        }
    }

    #[test]
    fn every_shipped_repository_scenario_is_embedded_for_release() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenarios");
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if !path.extension().is_some_and(|extension| extension == "toml") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let scenario = Scenario::from_toml(&text).unwrap();
            if scenario.description == "draft" {
                continue;
            }
            let id = path.file_stem().unwrap().to_str().unwrap();
            let embedded = BUILT_IN.iter().find(|entry| entry.0 == id);
            assert!(embedded.is_some(), "The release catalogue omits {id}");
            assert_eq!(embedded.unwrap().2, text, "The release copy of {id} is stale");
        }
    }

    #[test]
    fn labels_read_like_words() {
        assert_eq!(label("low-battery-tiltback"), "Low battery tiltback");
        assert_eq!(label("hard-braking-30kmh"), "Hard braking 30 km/h");
        assert_eq!(label("overlean-top-speed-30pct"), "Overlean top speed 30 %");
    }
}
