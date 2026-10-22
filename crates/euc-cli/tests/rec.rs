//! `euc sim run --record`, `euc rec info` and `euc rec export` on a synthetic image: the recording
//! describes the run and never carries the image.

use std::path::Path;
use std::process::{Command, Output};

use euc_profile::Sha256;
use euc_telemetry::recording::Reader;

const BASE: u32 = 0x0800_3000;

/// Bytes only the image holds: a recording must not contain them.
const MARK: &[u8; 48] = b"code flash only: never in a recording (canary 7)";

fn euc(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_euc"))
        .args(args)
        .current_dir(dir)
        .env_remove("EUC_PROFILES")
        .env("EUC_CACHE_DIR", dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}

/// An app that spins in its reset handler (`b .`), with the canary in its flash.
fn spinning_app() -> Vec<u8> {
    let mut image = vec![0; 0x1000];
    for (index, value) in [(0, 0x2000_2000), (1, BASE + 0x201), (3, BASE + 0x205)] {
        image[4 * index..4 * index + 4].copy_from_slice(&u32::to_le_bytes(value));
    }
    image[0x200..0x202].copy_from_slice(&[0xFE, 0xE7]);
    image[0x204..0x206].copy_from_slice(&[0xFE, 0xE7]);
    image[0x400..0x400 + MARK.len()].copy_from_slice(MARK);
    image
}

const SCENARIO: &str = r#"
schema = 1
name = "rec-test"
description = "A short stand-still run to record."
wheel = "begode-extreme"
seconds = 1.2
release_s = 1.0
speed_kmh = [[0, 0]]
"#;

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn records_a_run_and_reads_it_back() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let image = spinning_app();
    std::fs::write(dir.join("app.bin"), &image).unwrap();
    std::fs::write(dir.join("rec-test.toml"), SCENARIO).unwrap();
    let out =
        euc(dir, &["sim", "run", "--scenario", "rec-test.toml", "--firmware", "app.bin", "--record", "run.eucrec"]);
    assert!(dir.join("run.eucrec").is_file(), "{}", String::from_utf8_lossy(&out.stderr));

    let mut reader = Reader::open(&dir.join("run.eucrec")).unwrap();
    let h = reader.header().clone();
    assert_eq!(reader.truncated(), None);
    assert_eq!((h.wheel.as_str(), h.firmware.as_str()), ("begode-extreme", "app.bin"));
    assert_eq!(h.firmware_sha256, Some(Sha256::digest(&image).to_string()));
    assert_eq!(h.scenario.as_deref(), Some("rec-test"));
    assert_eq!(h.scenario_toml.as_deref(), Some(SCENARIO));
    assert_eq!((h.cpu.as_deref(), h.sample_hz.round()), (Some("own"), 100.0));
    assert!(h.profile_sha256.is_some() && h.seed.is_some());
    assert!(h.bodies.iter().any(|b| b.name == "wheel"));
    let samples = reader.read_all().unwrap();
    assert!((115..=121).contains(&samples.len()), "{} samples", samples.len());
    assert!(reader.events().iter().any(|e| e.text == "stand released"));
    assert!(reader.events().iter().any(|e| e.text.starts_with("Scenario rec-test")), "the verdict is on the timeline");

    // Nothing of the image: not in the header, the events, the frames or the outputs.
    let mut text = serde_json::to_vec(&h).unwrap();
    text.extend(serde_json::to_vec(reader.events()).unwrap());
    text.extend(serde_json::to_vec(reader.outputs()).unwrap());
    assert!(!contains(&text, &MARK[..16]));
    assert!(reader.frames().iter().all(|f| !contains(&f.bytes, &MARK[..16])));
    assert!(!contains(&std::fs::read(dir.join("run.eucrec")).unwrap(), &image[0x400..0x420]));

    let info = euc(dir, &["--json", "rec", "info", "run.eucrec"]);
    assert!(info.status.success(), "{}", String::from_utf8_lossy(&info.stderr));
    let json: serde_json::Value = serde_json::from_slice(&info.stdout).unwrap();
    assert_eq!(json["samples"], samples.len());
    assert_eq!(json["header"]["scenario"], "rec-test");

    let export = euc(dir, &["rec", "export", "run.eucrec", "--csv", "csv"]);
    assert!(export.status.success(), "{}", String::from_utf8_lossy(&export.stderr));
    let motion = std::fs::read_to_string(dir.join("csv/motion.csv")).unwrap();
    assert!(motion.starts_with("t_s,speed_kmh,"));
    assert_eq!(motion.lines().count(), samples.len() + 1);
    for file in ["drive", "battery", "attitude", "channels", "poses", "events", "frames", "outputs"] {
        assert!(dir.join(format!("csv/{file}.csv")).is_file(), "{file}.csv");
    }
}

#[test]
fn info_refuses_a_file_that_is_not_a_recording() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("x.eucrec"), b"EUCR\x01\0\0\0 the old format").unwrap();
    let out = euc(tmp.path(), &["rec", "info", "x.eucrec"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not an .eucrec recording"));
}
