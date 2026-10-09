//! Global hotkeys.
//!
//! Windows delivers `WM_HOTKEY` to a window, so a hotkey manager needs a message
//! pump. We create a hidden window, register the hotkeys against it, and forward
//! everything the engine cares about.
//!
//! Hotkeys are declared as a modifier mask plus a virtual-key code, which is the
//! same representation the UI uses when capturing a combination.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Modifier bits, matching `MOD_ALT`/`MOD_CONTROL`/`MOD_SHIFT`/`MOD_WIN` from
/// Win32 so the same values can be passed straight through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Mods {
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub win: bool,
}

impl Default for Mods {
    fn default() -> Self {
        Self {
            alt: false,
            ctrl: true,
            shift: false,
            win: false,
        }
    }
}

impl Mods {
    pub const NONE: Mods = Mods {
        alt: false,
        ctrl: false,
        shift: false,
        win: false,
    };

    pub fn is_empty(&self) -> bool {
        !self.alt && !self.ctrl && !self.shift && !self.win
    }

    pub fn win32(&self) -> u32 {
        let mut m = 0u32;
        if self.alt {
            m |= 0x0001; // MOD_ALT
        }
        if self.ctrl {
            m |= 0x0002; // MOD_CONTROL
        }
        if self.shift {
            m |= 0x0004; // MOD_SHIFT
        }
        if self.win {
            m |= 0x0008; // MOD_WIN
        }
        m
    }

    pub fn from_win32(m: u32) -> Self {
        Self {
            alt: m & 0x0001 != 0,
            ctrl: m & 0x0002 != 0,
            shift: m & 0x0004 != 0,
            win: m & 0x0008 != 0,
        }
    }

    /// Render as `Ctrl+Shift+Key`.
    pub fn describe(self, key: u32) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".into());
        }
        if self.alt {
            parts.push("Alt".into());
        }
        if self.shift {
            parts.push("Shift".into());
        }
        if self.win {
            parts.push("Win".into());
        }
        parts.push(key_name(key));
        parts.join("+")
    }
}

/// A single global hotkey.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hotkey {
    pub mods: Mods,
    /// Win32 virtual-key code, e.g. `0x70` for F1.
    pub key: u32,
}

impl Hotkey {
    pub fn new(mods: Mods, key: u32) -> Self {
        Self { mods, key }
    }

    pub fn describe(self) -> String {
        self.mods.describe(self.key)
    }
}

/// Human-readable name for a virtual-key code.
pub fn key_name(key: u32) -> String {
    match key {
        0x08 => "Backspace".to_string(),
        0x09 => "Tab".to_string(),
        0x0D => "Enter".to_string(),
        0x1B => "Esc".to_string(),
        0x20 => "Space".to_string(),
        0x21..=0x2E => printable(key),
        0x30..=0x39 => printable(key),
        0x41..=0x5A => printable(key),
        0x6A..=0x6B => format!("Browser {}", key - 0x6A + 1),
        0x60..=0x69 => format!("Num{}", key - 0x60),
        // VK_F1 is 0x70, so F1..=F24 spans 0x70..=0x87.
        0x70..=0x87 => format!("F{}", key - 0x70 + 1),
        0x90..=0x99 => format!("Lock {}", key - 0x90 + 1),
        0xA0..=0xA5 => format!("Num{}", key - 0xA0),
        0xBA => ";".to_string(),
        0xBB => "=".to_string(),
        0xBC => ",".to_string(),
        0xBD => "-".to_string(),
        0xBE => ".".to_string(),
        0xBF => "/".to_string(),
        0xC0 => "`".to_string(),
        0xDB => "[".to_string(),
        0xDC => "\\".to_string(),
        0xDD => "]".to_string(),
        0xDE => "'".to_string(),
        other => format!("Key {other:#04X}"),
    }
}

/// The character a printable virtual-key code represents.
fn printable(key: u32) -> String {
    char::from_u32(key)
        .map(|c| c.to_string())
        .unwrap_or_default()
}

/// What a hotkey does when pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HotkeyAction {
    /// Switch to the profile at this index.
    SelectProfile(usize),
    /// Move to the next profile.
    NextProfile,
    /// Move to the previous profile.
    PrevProfile,
    /// Pause/resume output entirely.
    TogglePause,
    /// Cycle the lightbar preset.
    CycleLightbar,
    /// Recalibrate the current profile from the pad's resting position.
    Recalibrate,
}

/// One configured hotkey row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HotkeyBinding {
    pub action: HotkeyAction,
    /// Disabled bindings are kept but not registered, so toggling is lossless.
    pub enabled: bool,
    pub hotkey: Option<Hotkey>,
}

impl HotkeyBinding {
    pub fn new(action: HotkeyAction, hotkey: Option<Hotkey>) -> Self {
        Self {
            action,
            enabled: hotkey.is_some(),
            hotkey,
        }
    }
}

/// Owns the hidden message window and the registered hotkeys.
#[cfg(target_os = "windows")]
pub struct HotkeyManager {
    window: Option<windows::Window>,
    /// Win32 hotkey id -> action.
    bindings: HashMap<i32, HotkeyAction>,
    next_id: i32,
}

#[cfg(target_os = "windows")]
impl HotkeyManager {
    /// Create the hidden window. Must be called on the engine thread, which is
    /// also where [`Self::poll`] runs, so the message queue belongs to it.
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            window: Some(windows::Window::new()?),
            bindings: HashMap::new(),
            next_id: 0xB000,
        })
    }

    /// Register a set of bindings, replacing anything registered before.
    ///
    /// Returns every binding that could not be taken, usually because another
    /// application already owns that combination.
    pub fn register_all(
        &mut self,
        bindings: &[HotkeyBinding],
    ) -> std::io::Result<Vec<(HotkeyAction, String)>> {
        self.unregister_all();
        let Some(window) = self.window.as_ref() else {
            return Ok(Vec::new());
        };
        let mut failures = Vec::new();
        for b in bindings {
            let Some(hk) = b.hotkey else { continue };
            if !b.enabled {
                continue;
            }
            if hk.mods.is_empty() {
                // A bare key would swallow it system-wide.
                failures.push((b.action, "no modifier keys".into()));
                continue;
            }
            let id = self.next_id;
            self.next_id += 1;
            match windows::register(window.hwnd(), id, hk.mods.win32(), hk.key) {
                Ok(()) => {
                    self.bindings.insert(id, b.action);
                }
                Err(e) => failures.push((b.action, e.to_string())),
            }
        }
        Ok(failures)
    }

    pub fn unregister_all(&mut self) {
        if let Some(window) = self.window.as_ref() {
            for id in self.bindings.keys() {
                let _ = windows::unregister(window.hwnd(), *id);
            }
        }
        self.bindings.clear();
    }

    /// Pump pending messages. Returns every action that fired.
    pub fn poll(&mut self) -> Vec<HotkeyAction> {
        let Some(window) = self.window.as_mut() else {
            return Vec::new();
        };
        window
            .drain_hotkeys()
            .into_iter()
            .filter_map(|id| self.bindings.get(&id).copied())
            .collect()
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use windows_sys::Win32::Foundation::{
        GetLastError, ERROR_CLASS_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey};
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe {
            match msg {
                // Hotkey ids are collected from the message queue directly, so
                // the default handler is all this window needs.
                WM_DESTROY => {
                    PostQuitMessage(0);
                    0
                }
                _ => DefWindowProcW(hwnd, msg, wparam, lparam),
            }
        }
    }

    /// A message-only window used purely as a hotkey sink.
    ///
    /// `HWND_MESSAGE` means it never appears on screen and does not steal focus,
    /// which matters because it will exist for the whole session.
    pub struct Window {
        hwnd: HWND,
    }

    impl Window {
        pub fn hwnd(&self) -> HWND {
            self.hwnd
        }

        pub fn new() -> std::io::Result<Self> {
            unsafe {
                let class_name: Vec<u16> = "PadForgeHotkeySink\0".encode_utf16().collect();
                let instance: HINSTANCE = GetModuleHandleW(std::ptr::null());

                let wc = WNDCLASSW {
                    lpfnWndProc: Some(wnd_proc),
                    hInstance: instance,
                    lpszClassName: class_name.as_ptr(),
                    ..std::mem::zeroed()
                };
                let class = RegisterClassW(&wc);
                // ERROR_CLASS_ALREADY_EXISTS is harmless: another instance in
                // this process already registered the name, and the class is
                // still usable.
                if class == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
                    return Err(std::io::Error::last_os_error());
                }

                let hwnd = CreateWindowExW(
                    // Extended style and window style.
                    WINDOW_EX_STYLE,
                    class_name.as_ptr(),
                    class_name.as_ptr(),
                    WINDOW_STYLE,
                    // Position and size: irrelevant for a message-only window,
                    // but the parameters still have to be present.
                    0,
                    0,
                    0,
                    0,
                    // Message-only parent, no menu, our instance, no param.
                    HWND_MESSAGE,
                    std::ptr::null_mut(),
                    instance,
                    std::ptr::null_mut(),
                );
                if hwnd.is_null() {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(Self { hwnd })
            }
        }

        /// Consume every message queued since the last call, returning the ids of
        /// any `WM_HOTKEY` messages.
        pub fn drain_hotkeys(&self) -> Vec<i32> {
            let mut out = Vec::new();
            unsafe {
                let mut msg: MSG = std::mem::zeroed();
                while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                    if msg.message == WM_HOTKEY {
                        out.push(msg.wParam as i32);
                    }
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            out
        }
    }

    /// A message-only window has no extended style and no window style.
    const WINDOW_EX_STYLE: WINDOW_EX_STYLE = 0;
    const WINDOW_STYLE: WINDOW_STYLE = 0;

    pub fn register(hwnd: HWND, id: i32, mods: u32, key: u32) -> std::io::Result<()> {
        unsafe {
            if RegisterHotKey(hwnd, id, mods, key) == 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        }
    }

    pub fn unregister(hwnd: HWND, id: i32) -> std::io::Result<()> {
        unsafe {
            if UnregisterHotKey(hwnd, id) == 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub struct HotkeyManager;

#[cfg(not(target_os = "windows"))]
impl HotkeyManager {
    pub fn new() -> std::io::Result<Self> {
        Ok(Self)
    }
    pub fn register_all(
        &mut self,
        bindings: &[HotkeyBinding],
    ) -> std::io::Result<Vec<(HotkeyAction, String)>> {
        let _ = bindings;
        Ok(Vec::new())
    }
    pub fn unregister_all(&mut self) {}
    pub fn poll(&mut self) -> Vec<HotkeyAction> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mods_round_trip_through_win32() {
        let m = Mods {
            alt: true,
            ctrl: true,
            shift: false,
            win: true,
        };
        assert_eq!(Mods::from_win32(m.win32()), m);
    }

    #[test]
    fn describe_is_readable() {
        let hk = Hotkey::new(
            Mods {
                ctrl: true,
                shift: true,
                alt: false,
                win: false,
            },
            0x70,
        );
        assert_eq!(hk.describe(), "Ctrl+Shift+F1");
    }

    #[test]
    fn key_names() {
        assert_eq!(key_name(0x41), "A");
        assert_eq!(key_name(0x31), "1");
        assert_eq!(key_name(0x70), "F1");
        assert_eq!(key_name(0x7A), "F11", "F11 must not read as a raw code");
        assert_eq!(key_name(0x7B), "F12");
        assert_eq!(key_name(0x60), "Num0");
        assert_eq!(key_name(0x0D), "Enter");
        assert_eq!(key_name(0x2E), ".");
    }

    #[test]
    fn default_bindings_name_cleanly() {
        // Regression guard: the default hotkeys used Browser-key codes, which
        // rendered as "Key 0x6B" in the UI.
        for binding in default_bindings() {
            if let Some(hk) = binding.hotkey {
                assert!(
                    !hk.describe().contains("0x"),
                    "default hotkey renders as a raw code: {}",
                    hk.describe()
                );
            }
        }
    }

    /// Mirrors the UI's default set, so the guard above is meaningful here too.
    fn default_bindings() -> Vec<HotkeyBinding> {
        let mods = Mods {
            alt: false,
            ctrl: true,
            shift: true,
            win: false,
        };
        vec![
            HotkeyBinding::new(HotkeyAction::NextProfile, Some(Hotkey::new(mods, 0x7A))),
            HotkeyBinding::new(HotkeyAction::PrevProfile, Some(Hotkey::new(mods, 0x79))),
            HotkeyBinding::new(HotkeyAction::TogglePause, Some(Hotkey::new(mods, 0x7B))),
        ]
    }

    #[test]
    fn no_modifiers_is_detected() {
        assert!(Mods::NONE.is_empty());
        assert!(!Mods::default().is_empty());
    }
}
