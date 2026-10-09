//! Foreground process detection.
//!
//! Auto-profiles key off *which game is in front*, which means asking Windows
//! for the foreground window's owning process and then resolving its image path.
//! Both of those are cheap enough to poll a couple of times a second.

/// Identity of a running program, used to match auto-profile rules.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProcessInfo {
    /// File name without extension, lowercased, e.g. `cyberpunk2077`.
    pub name: String,
    /// Full image path when it could be resolved.
    pub path: Option<String>,
}

impl ProcessInfo {
    /// Normalise a process name for matching: lowercase, and without the `.exe`
    /// suffix, so auto-profile rules can be written either way.
    pub fn new(name: String, path: Option<String>) -> Self {
        let trimmed = name.trim();
        let stem = trimmed
            .strip_suffix(".exe")
            .or_else(|| trimmed.strip_suffix(".EXE"))
            .unwrap_or(trimmed);
        Self {
            name: stem.to_ascii_lowercase(),
            path,
        }
    }
}

/// Query the foreground process. Returns `None` if there is no foreground window
/// or the process could not be resolved.
#[cfg(target_os = "windows")]
pub fn foreground_process() -> Option<ProcessInfo> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return None;
        }

        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }

        let mut buf = [0u16; 32768 / 2];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len);
        let _ = windows_sys::Win32::Foundation::CloseHandle(handle);
        if ok == 0 {
            return None;
        }

        let path = std::ffi::OsString::from_wide(&buf[..len as usize])
            .to_string_lossy()
            .into_owned();
        let name = std::path::Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();

        if name.is_empty() {
            return None;
        }
        Some(ProcessInfo::new(name, Some(path)))
    }
}

/// Non-Windows stub so the crate still builds (the UI just shows "unsupported").
#[cfg(not(target_os = "windows"))]
pub fn foreground_process() -> Option<ProcessInfo> {
    None
}

/// List the full paths of running executables, for the auto-profile editor's
/// "pick an app" picker.
#[cfg(target_os = "windows")]
pub fn running_processes() -> Vec<ProcessInfo> {
    let mut out = Vec::new();
    for (name, path) in process_list() {
        out.push(ProcessInfo::new(name, Some(path)));
    }
    out
}

#[cfg(not(target_os = "windows"))]
pub fn running_processes() -> Vec<ProcessInfo> {
    Vec::new()
}

#[cfg(target_os = "windows")]
fn process_list() -> Vec<(String, String)> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Vec::new();
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        let mut out = Vec::new();
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let raw = std::ffi::OsString::from_wide(&entry.szExeFile)
                    .to_string_lossy()
                    .into_owned();
                let name = raw.trim_end_matches(".exe").to_ascii_lowercase();

                if let Some(path) = query_path(entry.th32ProcessID) {
                    out.push((name, path));
                }

                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
        out
    }
}

#[cfg(target_os = "windows")]
fn query_path(pid: u32) -> Option<String> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = [0u16; 32768 / 2];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
        let _ = CloseHandle(h);
        if ok == 0 {
            return None;
        }
        Some(
            std::ffi::OsString::from_wide(&buf[..len as usize])
                .to_string_lossy()
                .into_owned(),
        )
    }
}

#[cfg(target_os = "windows")]
const INVALID_HANDLE_VALUE: *mut core::ffi::c_void = -1isize as *mut core::ffi::c_void;



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_lowercased_and_extension_stripped() {
        let p = ProcessInfo::new("Cyberpunk2077.EXE".into(), None);
        assert_eq!(p.name, "cyberpunk2077");
    }

    #[test]
    fn foreground_is_optional() {
        // Under a test harness there may or may not be a foreground window; the
        // contract is simply that it never panics.
        let _ = foreground_process();
    }

    #[test]
    fn running_processes_is_readable() {
        // Must not panic even with an empty snapshot.
        let list = running_processes();
        assert!(list.iter().all(|p| !p.name.is_empty()));
    }
}