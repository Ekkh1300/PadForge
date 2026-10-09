//! Compiles the Windows resource script and links it into the binary.
//!
//! Without an `RT_GROUP_ICON` in the resource section, Explorer and the Start
//! Menu show a generic executable glyph no matter how good the artwork is, and
//! the Properties dialog reports the file as having no version. Rust has no
//! built-in way to attach either, so `windres` compiles `padforge.rc` into a COFF
//! object and the linker is told to include it.
//!
//! On a non-Windows host there is nothing to do, and the script is skipped
//! rather than failing, so `cargo check` still works for documentation builds.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));

    if !cfg!(target_os = "windows") {
        return;
    }

    // tools/ is at the workspace root, two levels up from crates/padforge.
    let tools_dir = manifest_dir
        .parent()
        .and_then(Path::parent)
        .map(|root| root.join("tools"))
        .expect("could not locate the tools directory");

    let rc = tools_dir.join("padforge.rc");
    if !rc.is_file() {
        println!(
            "cargo:warning={} not found; the binary will have no icon",
            rc.display()
        );
        return;
    }

    // windres resolves ICON paths relative to the working directory, so run it
    // from tools/ and give it bare filenames.
    let icon = tools_dir.join("padforge.ico");
    println!("cargo:rerun-if-changed={}", rc.display());
    println!("cargo:rerun-if-changed={}", icon.display());

    let object = out_dir.join("padforge_resources.o");
    let status = Command::new("windres")
        .arg("-i")
        .arg("padforge.rc")
        .arg("-O")
        .arg("coff")
        .arg("-o")
        .arg(&object)
        .current_dir(&tools_dir)
        .status();

    // A missing icon is cosmetic and a missing windres is a machine difference,
    // so neither is worth failing the build over. Panicking here broke CI: the
    // ICO had been left out of the repository because it is generated, and the
    // check step could not build anything at all over a cosmetic resource.
    match status {
        Ok(s) if s.success() => {
            // The documented way to get a non-Rust object into the link.
            println!("cargo:rustc-link-arg={}", object.display());
        }
        Ok(s) => println!("cargo:warning=windres failed with {s}; the binary will have no icon"),
        Err(e) => {
            println!("cargo:warning=windres could not be run ({e}); the binary will have no icon")
        }
    }
}
