//! `euc flash`: the simulator, the dry-run and the campaign through the real binary. No vendor
//! image is needed: `--synthetic` brings its own stock image.

use std::path::Path;
use std::process::{Command, Output};

fn euc(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_euc"))
        .args(args)
        .current_dir(dir)
        .env_remove("EUC_PROFILES")
        .env_remove("EUC_BLE_REAL_WRITES")
        .env("EUC_CACHE_DIR", dir)
        .env("EUC_FLASH_LEDGER", dir.join("ledger.json"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn a_real_flash_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let output = euc(tmp.path(), &["flash"]);
    assert!(!output.status.success());
    assert!(stdout(&output).contains("this build cannot flash a real wheel"), "{}", stdout(&output));
}

#[test]
fn synthetic_sim_flash_succeeds_and_logs() {
    let tmp = tempfile::tempdir().unwrap();
    let log = tmp.path().join("flash.jsonl");
    let output = euc(tmp.path(), &["--json", "flash", "--sim", "--synthetic", "--log", log.to_str().unwrap()]);
    assert!(output.status.success(), "{}", stdout(&output));
    let json: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(json["report"]["outcome"]["kind"], "flashed");
    assert_eq!(json["report"]["reentry"], true);
    assert_eq!(json["report"]["guard_refusals"], 0);
    let lines = std::fs::read_to_string(&log).unwrap();
    assert!(lines.lines().count() > 100);
    assert!(lines.lines().all(|line| serde_json::from_str::<serde_json::Value>(line).is_ok()));
    assert!(!tmp.path().join("ledger.json").exists(), "synthetic runs record no dry-run");
}

#[test]
fn injected_power_loss_ends_restored() {
    let tmp = tempfile::tempdir().unwrap();
    let output = euc(tmp.path(), &["flash", "--sim", "--synthetic", "--inject", "power-loss-erase", "--seed", "3"]);
    let text = stdout(&output);
    assert!(!output.status.success());
    assert!(text.contains("RESTORED"), "{text}");
    assert!(text.contains("the wheel lost power"), "{text}");

    let output = euc(tmp.path(), &["flash", "--sim", "--synthetic", "--inject", "meteor"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown fault"));
}

#[test]
fn dry_run_needs_a_pinned_recovery_image_and_connects_to_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    // A plausible app that no profile pins: gate 2 and the recovery gate refuse it.
    let mut app = vec![0_u8; 0x1000];
    for (index, value) in [(0_usize, 0x2000_2000_u32), (1, 0x0800_3201), (3, 0x0800_3205)] {
        app[4 * index..4 * index + 4].copy_from_slice(&value.to_le_bytes());
    }
    let file = tmp.path().join("app.bin");
    std::fs::write(&file, &app).unwrap();
    let path = file.to_str().unwrap();
    let output = euc(tmp.path(), &["--json", "flash", "--dry-run", path, "--recovery", path]);
    assert!(!output.status.success());
    let json: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(json["allowed"], false);
    let gates = json["gates"].as_array().unwrap();
    assert_eq!(gates[0]["status"], "pass", "gate 1: the structure is fine");
    assert_eq!(gates[1]["status"], "fail", "gate 2: unknown image");
    assert_eq!(gates[3]["status"], "fail", "the recovery image is not stock");
    assert!(json["note"].as_str().unwrap().contains("nothing was connected or written"));
}

#[test]
fn the_campaign_runs_from_the_command_line() {
    let tmp = tempfile::tempdir().unwrap();
    let output = euc(tmp.path(), &["flash", "--campaign", "12", "--seed", "9"]);
    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("PASS      12 runs"), "{text}");
}
