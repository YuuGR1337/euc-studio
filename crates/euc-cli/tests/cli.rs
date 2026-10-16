//! Runs the real `euc` binary, with its built-in profiles, against synthetic images.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use euc_profile::Sha256;

const BASE: u32 = 0x0800_3000;

/// Runs `euc` inside `dir`, with `dir` as the cache and no profiles override in the environment.
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

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
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
    assert_eq!(json["profiles_source"], "builtin");
}

/// A `profiles/` folder in the working directory is never read; only `--profiles` uses one, and
/// then the run says so on stderr, in the JSON and in the provenance line.
#[test]
fn planted_profiles_are_ignored_unless_named() {
    let tmp = tempfile::tempdir().unwrap();
    let image = app();
    write(tmp.path(), "fake.bin", &image);
    let real = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../profiles/begode-extreme/wheel.toml");
    let planted = format!(
        "{}\n[[firmware]]\nkind = \"manual\"\nsha256 = \"{}\"\nlabel = \"PLANTED\"\n",
        std::fs::read_to_string(real).unwrap(),
        Sha256::digest(&image)
    );
    let dir = tmp.path().join("profiles");
    std::fs::create_dir_all(dir.join("begode-extreme")).unwrap();
    write(&dir.join("begode-extreme"), "wheel.toml", planted.as_bytes());

    let plain = euc(tmp.path(), &["image", "validate", "fake.bin"]);
    assert_eq!(plain.status.code(), Some(1), "{}", stdout(&plain));
    assert!(stdout(&plain).contains("in the built-in profiles; unknown images are refused"), "{}", stdout(&plain));
    assert_eq!(stderr(&plain), "", "no warning without an override");

    let dir = dir.to_str().unwrap();
    let named = euc(tmp.path(), &["--json", "--profiles", dir, "image", "validate", "fake.bin"]);
    assert_eq!(named.status.code(), Some(0), "{}", stdout(&named));
    assert_eq!(stderr(&named).trim_end(), format!("warning: using profiles from {dir}, not the built-in set"));
    let json: serde_json::Value = serde_json::from_slice(&named.stdout).unwrap();
    assert_eq!(json["profiles_source"], format!("override:{dir}"));
    assert_eq!(json["identity"]["pin_source"], format!("override:{dir}"));
    assert_eq!(
        json["checks"][5]["message"],
        format!("pinned stock build PLANTED; pin from override profiles at {dir}")
    );
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
fn bad_override_profile_reports_once_and_safely() {
    let tmp = tempfile::tempdir().unwrap();
    let real = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../profiles/begode-extreme/wheel.toml");
    let text = std::fs::read_to_string(real).unwrap().replace(r#"name = "Begode Extreme""#, r#"name = "\u001b[2J""#);
    std::fs::create_dir_all(tmp.path().join("p/begode-extreme")).unwrap();
    write(&tmp.path().join("p/begode-extreme"), "wheel.toml", text.as_bytes());
    let output = euc(tmp.path(), &["--profiles", "p", "fetch", "begode-extreme"]);
    assert_eq!(output.status.code(), Some(1));
    let message = stderr(&output);
    assert_eq!(message.matches("`name` contains control or bidi characters").count(), 1, "{message}");
    assert!(!message.contains('\u{1b}'), "{message}");
}

#[test]
fn unknown_wheel_lists_the_known_ones() {
    let tmp = tempfile::tempdir().unwrap();
    let output = euc(tmp.path(), &["fetch", "no-such-wheel"]);
    assert_eq!(output.status.code(), Some(1));
    let message = stderr(&output);
    assert!(message.contains("no wheel profile \"no-such-wheel\" in the built-in profiles"), "{message}");
    assert!(message.contains("(available: begode-extreme)"), "{message}");
}
