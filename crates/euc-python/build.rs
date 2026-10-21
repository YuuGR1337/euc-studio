//! Adds the user-supplied MuJoCo library directory to the extension's runtime search path.

fn main() {
    println!("cargo::rerun-if-env-changed=MUJOCO_DYNAMIC_LINK_DIR");
    if let Ok(dir) = std::env::var("MUJOCO_DYNAMIC_LINK_DIR")
        && std::env::var_os("CARGO_CFG_UNIX").is_some()
    {
        println!("cargo::rustc-link-arg=-Wl,-rpath,{dir}");
    }
}
