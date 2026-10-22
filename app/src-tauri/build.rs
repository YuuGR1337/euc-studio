fn main() {
    // Bundles find MuJoCo beside the executable, independently of the build machine's cache.
    // macos-package.mjs copies and signs the framework. Development still uses the fetched cache.
    println!("cargo::rerun-if-env-changed=MUJOCO_DYNAMIC_LINK_DIR");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo::rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
    }
    if (tauri_build::is_dev() || std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos"))
        && let Ok(dir) = std::env::var("MUJOCO_DYNAMIC_LINK_DIR")
        && std::env::var_os("CARGO_CFG_UNIX").is_some()
    {
        println!("cargo::rustc-link-arg=-Wl,-rpath,{dir}");
    }
    tauri_build::build()
}
