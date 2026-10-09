//! Embeds the built application into the installer binary, and gives the
//! installer itself an icon and a version resource.
//!
//! The installer has to be a single self-contained `.exe`, so the payload is
//! baked in at compile time rather than shipped alongside. That introduces an
//! ordering requirement: `padforge` has to be built before `padforge-installer`.
//!
//! Rather than failing the build outright, a missing payload produces an empty
//! one plus a loud warning. An installer that refuses to run and says why is far
//! more useful during development than an unrelated build failure in a crate the
//! caller did not ask to build, and `cargo test --workspace` still works.
//!
//! The resources come from `tools/padforge-installer.rc`, compiled by `windres`
//! into an object that the linker is told to include. Without them Explorer shows a
//! generic glyph for the setup file and Properties reports no version, which is
//! the first thing anyone notices about a downloaded installer.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let crate_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let workspace_root = crate_dir
        .parent()
        .and_then(Path::parent)
        .unwrap_or(&crate_dir);

    // OUT_DIR is <target>/<triple>/<profile>/build/<pkg>-<hash>/out
    let target_dir = out_dir
        .ancestors()
        .nth(4)
        .map(Path::to_path_buf)
        .expect("could not locate the target directory from OUT_DIR");

    println!("cargo:rerun-if-changed=build.rs");

    embed_resources(&workspace_root.join("tools"), &out_dir);

    // Prefer release over debug, and the newest build within each.
    let mut candidates: Vec<PathBuf> = Vec::new();
    for profile in ["release", "debug"] {
        let exe = target_dir.join(profile).join("padforge.exe");
        if exe.is_file() {
            candidates.push(exe);
        }
    }

    match candidates.first() {
        Some(app) => {
            copy(app, &out_dir.join("padforge.exe"));
            // Rebuild the installer whenever the application changes.
            println!("cargo:rerun-if-changed={}", app.display());
        }
        None => {
            std::fs::write(out_dir.join("padforge.exe"), b"").expect("could not stage payload");
            println!(
                "cargo:warning=padforge.exe was not found, so the installer will be empty.\n\
                 Build the application first:\n    \
                 cargo build -p padforge --release --target x86_64-pc-windows-gnu"
            );
        }
    }

    // Documentation ships alongside the app. Both are optional, but the paths are
    // watched unconditionally: cargo only re-runs a build script when it sees a
    // *change*, and a file that does not exist yet cannot have changed. Watching
    // the path means creating it later still triggers a rebuild.
    for name in ["README.md", "LICENSE"] {
        let src = workspace_root.join(name);
        println!("cargo:rerun-if-changed={}", src.display());
        if src.is_file() {
            copy(&src, &out_dir.join(name));
        } else {
            println!(
                "cargo:warning={} was not found, so it will not be installed.",
                src.display()
            );
            std::fs::write(out_dir.join(name), b"").expect("could not stage docs");
        }
    }
}

/// Compile `tools/padforge-installer.rc` with windres and link the result in.
///
/// A missing icon is not worth failing the build over, since it affects
/// appearance only and the application itself is unaffected. Every failure here
/// is a warning rather than an error.
fn embed_resources(tools_dir: &Path, out_dir: &Path) {
    let rc = tools_dir.join("padforge-installer.rc");
    if !rc.is_file() {
        println!(
            "cargo:warning={} not found; the installer will have no icon",
            rc.display()
        );
        return;
    }

    // windres resolves ICON paths relative to the working directory, so it is run
    // from tools/ with bare filenames.
    println!("cargo:rerun-if-changed={}", rc.display());
    println!(
        "cargo:rerun-if-changed={}",
        tools_dir.join("padforge.ico").display()
    );

    let object = out_dir.join("padforge_installer_resources.o");
    let status = Command::new("windres")
        .arg("-i")
        .arg("padforge-installer.rc")
        .arg("-O")
        .arg("coff")
        .arg("-o")
        .arg(&object)
        .current_dir(tools_dir)
        .status();

    match status {
        Ok(s) if s.success() => {
            // The documented way to get a non-Rust object into the link.
            println!("cargo:rustc-link-arg={}", object.display());
        }
        Ok(s) => println!("cargo:warning=windres failed with {s}; the installer will have no icon"),
        Err(e) => println!(
            "cargo:warning=windres could not be run ({e}); the installer will have no icon"
        ),
    }
}

/// Copy `src` to `dst`, creating the destination directory.
fn copy(src: &Path, dst: &Path) {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).expect("could not create OUT_DIR subdirectory");
    }
    std::fs::copy(src, dst).unwrap_or_else(|e| panic!("could not embed {}: {e}", src.display()));
}
