//! Runs the real `euc` binary against the repo's profiles and synthetic images.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BASE: u32 = 0x0800_3000;

fn profiles() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../profiles")
}

fn euc(cache: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_euc"))
        .args(args)
        .env("EUC_PROFILES", profiles())
        .env("EUC_CACHE_DIR", cache)
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

/// A 4 KiB app for the Begode Extreme slot: SP inside RAM, reset and a fault handler inside.
fn app() -> Vec<u8> {
    let mut image = vec![0; 0x1000];
    for (index, value) in [(0, 0x2000_2000), (1, BASE + 0x201), (3, BASE + 0x205)] {
        image[4 * index..4 * index + 4].copy_from_slice(&u32::to_le_bytes(value));
    }
    image
}

/// Adds an unsealed layout-1 `.euc_info` block, as our firmware build writes it.
fn with_info(mut image: Vec<u8>, board: &str) -> Vec<u8> {
    let block = &mut image[0x140..0x1C0];
    block[..4].copy_from_slice(b"EUCF");
    block[4..6].copy_from_slice(&1_u16.to_le_bytes());
    block[6..8].copy_from_slice(&128_u16.to_le_bytes());
    block[8..8 + board.len()].copy_from_slice(board.as_bytes());
    block[0x18..0x1D].copy_from_slice(b"clean");
    block[0x26..0x28].copy_from_slice(&1_u16.to_le_bytes());
    block[0x28..0x2F].copy_from_slice(b"0000000");
    image
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn validate_passes_our_dev_image() {
    let tmp = tempfile::tempdir().unwrap();
    let file = write(tmp.path(), "ours.bin", &with_info(app(), "begode-extreme"));
    let output = euc(tmp.path(), &["image", "validate", file.to_str().unwrap()]);
    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("PASS      begode-extreme clean 0.0.0 (0000000), dev unsealed"), "{text}");
}

#[test]
fn validate_refuses_a_bad_image() {
    let tmp = tempfile::tempdir().unwrap();
    let mut image = with_info(app(), "begode-extreme");
    image[4..8].copy_from_slice(&(BASE + 0x200).to_le_bytes()); // even reset vector
    let file = write(tmp.path(), "bad.bin", &image);
    let output = euc(tmp.path(), &["image", "validate", "--wheel", "begode-extreme", file.to_str().unwrap()]);
    let text = stdout(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("vector 1 (Reset) = 0x08003200 is even, so it is not a Thumb address"), "{text}");
    assert!(text.contains("FAIL      1 problem; this image must not be flashed"), "{text}");
}

#[test]
fn validate_refuses_an_unknown_stock_like_image() {
    let tmp = tempfile::tempdir().unwrap();
    let file = write(tmp.path(), "mystery.bin", &app());
    let output = euc(tmp.path(), &["--json", "image", "validate", file.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["passed"], false);
    assert_eq!(json["file"], file.to_str().unwrap());
    let provenance = &json["checks"][5];
    assert_eq!(provenance["problem"]["kind"], "unpinned");
}

#[test]
fn import_refuses_unpinned_files() {
    let tmp = tempfile::tempdir().unwrap();
    let file = write(tmp.path(), "fw.bin", b"not a pinned build");
    let output = euc(tmp.path(), &["firmware", "import", "begode-extreme", file.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).contains("is not a pinned build of Begode Extreme"));

    let output = euc(tmp.path(), &["firmware", "list"]);
    assert!(output.status.success());
    assert!(stdout(&output).starts_with("no cached firmware in "));
}

#[test]
fn unknown_wheel_lists_the_known_ones() {
    let tmp = tempfile::tempdir().unwrap();
    let output = euc(tmp.path(), &["fetch", "no-such-wheel"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("no wheel profile \"no-such-wheel\""), "{stderr}");
    assert!(stderr.contains("begode-extreme"), "{stderr}");
}
