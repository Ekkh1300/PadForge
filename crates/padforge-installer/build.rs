//! Embeds the built application into the installer binary.
//!
//! The installer has to be a single self-contained `.exe`, so the payload is
//! baked in at compile time rather than shipped alongside. That introduces an
//! ordering requirement: `padforge` has to be built before `padforge-installer`.
//!
//! Rather than failing the build outright, a missing payload produces an empty
//! one plus a loud warning. An installer that refuses to run and says why is far
//! more useful during development than an unrelated build failure in a crate the
//! caller did not ask to build, and `cargo test --workspace` still works.

use std::path::{Path, PathBuf};

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

/// Copy `src` to `dst`, creating the destination directory.
fn copy(src: &Path, dst: &Path) {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).expect("could not create OUT_DIR subdirectory");
    }
    std::fs::copy(src, dst).unwrap_or_else(|e| panic!("could not embed {}: {e}", src.display()));
}
