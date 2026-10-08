//! Puts `memory.x` where `cortex-m-rt`'s `link.x` finds it, for a bare-metal build only.

use std::{env, fs, io, path::PathBuf};

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed=memory.x");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("none") {
        return Ok(());
    }
    let out =
        PathBuf::from(env::var_os("OUT_DIR").ok_or_else(|| io::Error::other("OUT_DIR"))?);
    fs::copy("memory.x", out.join("memory.x"))?;
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rustc-link-arg-bins=-Tlink.x");
    Ok(())
}
