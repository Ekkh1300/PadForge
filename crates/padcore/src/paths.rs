//! Platform paths for on-disk state.
//!
//! Everything PadForge writes lives under `%APPDATA%\PadForge` so that uninstall
//! is a single folder delete.

use std::path::PathBuf;

/// `%APPDATA%\PadForge`
pub fn config_dir() -> PathBuf {
    base_dir().join(crate::APP_NAME)
}

/// `%APPDATA%\PadForge\profiles`
pub fn profiles_dir() -> PathBuf {
    config_dir().join("profiles")
}

/// `%APPDATA%\PadForge\logs`
pub fn logs_dir() -> PathBuf {
    config_dir().join("logs")
}

/// `%APPDATA%\PadForge\settings.json`
pub fn settings_file() -> PathBuf {
    config_dir().join("settings.json")
}

/// `%LOCALAPPDATA%\PadForge`
pub fn cache_dir() -> PathBuf {
    base_local().join(crate::APP_NAME)
}

fn base_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(fallback_home)
}

fn base_local() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(fallback_home)
}

fn fallback_home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Create a directory (and parents), ignoring "already exists".
pub fn ensure_dir(path: &std::path::Path) -> std::io::Result<()> {
    if path.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(path)
}

/// Read + parse a JSON file, returning `None` on any failure.
///
/// A leading byte order mark is stripped first. Windows editors leave one
/// behind — Notepad always does, and so does PowerShell's `Set-Content` — and
/// `serde_json` will not skip it, so the parse fails and `None` sends the app
/// back to defaults. A user who hand-edits their settings loses every setting
/// with no warning at all, and the file they wrote is still on disk looking
/// perfectly fine, which makes it the worst kind of failure to have.
pub fn read_json<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> Option<T> {
    let raw = std::fs::read_to_string(path).ok()?;
    let body = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
    match serde_json::from_str(body) {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::warn!(?path, %e, "could not parse json, falling back to defaults");
            None
        }
    }
}

/// Serialise `value` as pretty JSON, writing via a temp file + rename so a crash
/// mid-write can never leave a half-written config behind.
pub fn write_json<T: serde::Serialize>(path: &std::path::Path, value: &T) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    let body = serde_json::to_string_pretty(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, body)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            // Windows rename fails if the destination exists; fall back to copy.
            std::fs::copy(&tmp, path)?;
            let _ = std::fs::remove_file(&tmp);
            Err(e).or(Ok(()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A settings file that a Windows editor has touched.
    ///
    /// Notepad writes a byte order mark by default and so does PowerShell's
    /// `Set-Content`, so this is not an exotic state: it is what happens the
    /// first time anybody hand-edits their configuration. Losing every setting
    /// over it, silently, is the worst kind of failure — the file on disk still
    /// looks perfectly fine.
    #[test]
    fn a_byte_order_mark_does_not_reset_the_settings() {
        #[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
        struct Small {
            rate: u32,
        }

        let dir = std::env::temp_dir().join(format!("padforge-bom-{}", std::process::id()));
        ensure_dir(&dir).expect("temp dir");
        let path = dir.join("settings.json");

        std::fs::write(&path, "\u{feff}{\"rate\":250}").expect("write bom file");
        let with_bom: Option<Small> = read_json(&path);
        assert_eq!(with_bom, Some(Small { rate: 250 }));

        // And a file without one still reads, so stripping has not become a new
        // way to break the ordinary case.
        std::fs::write(&path, "{\"rate\":500}").expect("write plain file");
        let plain: Option<Small> = read_json(&path);
        assert_eq!(plain, Some(Small { rate: 500 }));

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }
}
