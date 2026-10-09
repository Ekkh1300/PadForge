//! Global application settings.
//!
//! These are the things that are *not* per-game: which pad to watch, how fast to
//! poll, window behaviour, startup, and so on.

use serde::{Deserialize, Serialize};

use crate::paths;

/// How the lightbar should react when no specific profile colour is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OutputMode {
    /// Publish a virtual Xbox 360 pad.
    #[default]
    Xbox360,
    /// Watch and display only; never touch the virtual bus.
    MonitorOnly,
}

/// Which pad to bind to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeviceSelection {
    /// Whatever connects first.
    #[default]
    Auto,
    /// One specific pad, by serial number.
    Serial(String),
}

impl DeviceSelection {
    pub fn serial(&self) -> Option<&str> {
        match self {
            DeviceSelection::Auto => None,
            DeviceSelection::Serial(s) => Some(s.as_str()),
        }
    }
}

/// Polling rate presets, in Hz. Higher means lower latency and more CPU.
pub const POLL_RATES: &[u32] = &[125, 250, 500, 1000];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub device: DeviceSelection,
    pub poll_rate_hz: u32,
    pub output_mode: OutputMode,

    /// Stop publishing reports without unloading anything.
    pub paused: bool,

    /// Start hidden in the notification area.
    pub start_minimized: bool,
    /// Keep running after the window is closed.
    pub minimize_on_close: bool,
    /// Add a Run-at-login entry.
    pub autostart: bool,

    /// Poll the foreground process to drive auto-profiles.
    pub auto_profiles_enabled: bool,
    /// How often to check the foreground process, in milliseconds.
    pub auto_profile_interval_ms: u64,

    /// Write a rolling log file.
    pub logging: bool,
    /// Verbose logging.
    pub debug_logging: bool,

    /// Interval for the battery refresh, in seconds.
    pub battery_interval_secs: u64,

    /// Re-apply the active profile when the pad reconnects.
    pub auto_reapply_profile: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            device: DeviceSelection::Auto,
            poll_rate_hz: 250,
            output_mode: OutputMode::Xbox360,
            paused: false,
            start_minimized: false,
            minimize_on_close: true,
            autostart: false,
            auto_profiles_enabled: true,
            auto_profile_interval_ms: 750,
            logging: true,
            debug_logging: false,
            battery_interval_secs: 30,
            auto_reapply_profile: true,
        }
    }
}

impl Settings {
    /// Clamp anything that could have been hand-edited into an invalid value.
    pub fn sanitised(mut self) -> Self {
        if !POLL_RATES.contains(&self.poll_rate_hz) {
            self.poll_rate_hz = 250;
        }
        self.auto_profile_interval_ms = self.auto_profile_interval_ms.clamp(100, 10_000);
        self.battery_interval_secs = self.battery_interval_secs.clamp(5, 3600);
        self
    }

    pub fn load() -> Self {
        paths::read_json(&paths::settings_file())
            .map(Settings::sanitised)
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        paths::write_json(&paths::settings_file(), self)
    }

    /// Human-readable summary of the device binding, for the status bar.
    pub fn device_label(&self) -> String {
        match &self.device {
            DeviceSelection::Auto => "First pad to connect".into(),
            DeviceSelection::Serial(s) => format!("Serial {s}"),
        }
    }
}

#[cfg(target_os = "windows")]
mod autostart {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_FILE_EXISTS, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
        KEY_READ, KEY_WRITE, REG_SZ,
    };

    /// `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
    const SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    const VALUE: &str = "PadForge";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn set(command: &str) -> std::io::Result<()> {
        unsafe {
            let mut key: HKEY = std::ptr::null_mut();
            let subkey = wide(SUBKEY);
            let status = RegOpenKeyExW(
                std::ptr::null_mut(),
                subkey.as_ptr(),
                0,
                KEY_WRITE,
                &mut key,
            );
            if status != ERROR_SUCCESS {
                return Err(std::io::Error::from_raw_os_error(status as i32));
            }
            let name = wide(VALUE);
            let data = wide(command);
            let bytes = (data.len() * std::mem::size_of::<u16>()) as u32;
            let rc = RegSetValueExW(key, name.as_ptr(), 0, REG_SZ, data.as_ptr().cast(), bytes);
            let _ = RegCloseKey(key);
            if rc != ERROR_SUCCESS {
                return Err(std::io::Error::from_raw_os_error(rc as i32));
            }
            Ok(())
        }
    }

    pub fn clear() -> std::io::Result<()> {
        unsafe {
            let mut key: HKEY = std::ptr::null_mut();
            let subkey = wide(SUBKEY);
            let status =
                RegOpenKeyExW(std::ptr::null_mut(), subkey.as_ptr(), 0, KEY_WRITE, &mut key);
            if status != ERROR_SUCCESS {
                return Err(std::io::Error::from_raw_os_error(status as i32));
            }
            let name = wide(VALUE);
            let rc = RegDeleteValueW(key, name.as_ptr());
            let _ = RegCloseKey(key);
            match rc {
                ERROR_SUCCESS => Ok(()),
                ERROR_FILE_EXISTS => Ok(()),
                other => Err(std::io::Error::from_raw_os_error(other as i32)),
            }
        }
    }

    pub fn is_enabled() -> bool {
        unsafe {
            let mut key: HKEY = std::ptr::null_mut();
            let subkey = wide(SUBKEY);
            let status =
                RegOpenKeyExW(std::ptr::null_mut(), subkey.as_ptr(), 0, KEY_READ, &mut key);
            if status != ERROR_SUCCESS {
                return false;
            }
            let name = wide(VALUE);
            let mut size: u32 = 0;
            let rc = RegQueryValueExW(
                key,
                name.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            );
            let _error = GetLastError();
            let _ = RegCloseKey(key);
            rc == ERROR_SUCCESS && size > 0
        }
    }
}

/// Path of the running executable, quoted for a Run-key command.
#[cfg(target_os = "windows")]
pub fn autostart_command() -> String {
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "padforge.exe".into());
    format!("\"{exe}\" --tray")
}

#[cfg(target_os = "windows")]
pub fn set_autostart(enabled: bool) -> std::io::Result<()> {
    if enabled {
        autostart::set(&autostart_command())
    } else {
        // Clearing an absent entry is not an error worth surfacing.
        match autostart::clear() {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}

#[cfg(target_os = "windows")]
pub fn autostart_enabled() -> bool {
    autostart::is_enabled()
}

#[cfg(not(target_os = "windows"))]
pub fn set_autostart(enabled: bool) -> std::io::Result<()> {
    let _ = enabled;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn autostart_enabled() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let s = Settings::default();
        assert!(POLL_RATES.contains(&s.poll_rate_hz));
        assert!(s.minimize_on_close);
        assert!(s.auto_profiles_enabled);
    }

    #[test]
    fn sanitise_repairs_bad_values() {
        let s = Settings {
            poll_rate_hz: 999,
            auto_profile_interval_ms: 5,
            battery_interval_secs: 0,
            ..Default::default()
        }
        .sanitised();
        assert_eq!(s.poll_rate_hz, 250);
        assert_eq!(s.auto_profile_interval_ms, 100);
        assert_eq!(s.battery_interval_secs, 5);
    }

    #[test]
    fn device_selection_serial_accessor() {
        assert!(DeviceSelection::Auto.serial().is_none());
        let d = DeviceSelection::Serial("abc".into());
        assert_eq!(d.serial(), Some("abc"));
    }

    #[test]
    fn json_round_trip() {
        let s = Settings {
            poll_rate_hz: 500,
            paused: true,
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn autostart_command_is_quoted() {
        let cmd = autostart_command();
        assert!(cmd.starts_with('"'), "command must be quoted: {cmd}");
    }
}