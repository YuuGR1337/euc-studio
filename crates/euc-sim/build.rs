//! Lets this crate's binaries and tests find the MuJoCo library at run time (see `tools/fetch-mujoco`),
//! and records the git commit the engine is built from (`EUC_GIT_REV`, for recording headers).

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo::rerun-if-env-changed=MUJOCO_DYNAMIC_LINK_DIR");
    // mujoco-rs links the library from MUJOCO_DYNAMIC_LINK_DIR; on Unix the loader also needs an rpath to it.
    // Windows finds mujoco.dll on PATH instead.
    if let Ok(dir) = std::env::var("MUJOCO_DYNAMIC_LINK_DIR")
        && std::env::var_os("CARGO_CFG_UNIX").is_some()
    {
        println!("cargo::rustc-link-arg=-Wl,-rpath,{dir}");
    }
    git_rev();
}

/// Sets `EUC_GIT_REV` to the commit checked out, unless the environment already sets it (a
/// packaged build without `.git`). Reruns when HEAD or the branch it names moves.
fn git_rev() {
    println!("cargo::rerun-if-env-changed=EUC_GIT_REV");
    if std::env::var_os("EUC_GIT_REV").is_some() {
        return;
    }
    let dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let git = |args: &[&str]| -> Option<String> {
        let out = Command::new("git").args(args).current_dir(&dir).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    };
    let Some(rev) = git(&["rev-parse", "HEAD"]) else { return };
    println!("cargo::rustc-env=EUC_GIT_REV={rev}");
    let mut watched = vec!["HEAD".to_owned(), "packed-refs".to_owned()];
    watched.extend(git(&["symbolic-ref", "-q", "HEAD"]));
    for name in watched {
        if let Some(path) = git(&["rev-parse", "--git-path", &name]) {
            let path = Path::new(&path);
            let path = if path.is_absolute() { path.to_owned() } else { dir.join(path) };
            if path.exists() {
                println!("cargo::rerun-if-changed={}", path.display());
            }
        }
    }
}
