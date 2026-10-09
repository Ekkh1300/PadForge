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
pub fn read_json<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> Option<T> {
    let raw = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&raw) {
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