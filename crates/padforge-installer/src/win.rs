//! Windows-specific plumbing: shortcuts, the Add/Remove Programs entry, and the
//! per-user autostart key.
//!
//! `windows-sys` does not bind `IShellLink`, so the vtable is declared by hand.
//! That is safe because COM fixes the layout: every interface begins with
//! `IUnknown`'s three methods, and the rest follow in declaration order. Only
//! the methods actually used are named; the gaps are opaque placeholders so the
//! offsets line up. A test writes a link and reads it back, which is what proves
//! the layout is right.

#![cfg(windows)]

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::core::{GUID, PCWSTR};
use windows_sys::Win32::Foundation::S_OK;
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, OPEN_EXISTING};
use windows_sys::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyW, RegDeleteKeyW, RegDeleteValueW, RegGetValueW, RegOpenKeyExW,
    RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
    RRF_RT_REG_SZ,
};

/// `CLSID_ShellLink`
const CLSID_SHELL_LINK: GUID = GUID {
    data1: 0x0002_1401,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

/// `IID_IShellLinkW`
const IID_SHELL_LINK_W: GUID = GUID {
    data1: 0x0002_14F9,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

/// `IID_IPersistFile`
const IID_IPERSIST_FILE: GUID = GUID {
    data1: 0x0000_010B,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

/// `RPC_E_CHANGED_MODE`: the thread is already in a different apartment.
const RPC_E_CHANGED_MODE: i32 = -2147417850;

/// The per-user autostart entry, and where it lives.
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// The `IShellLinkW` vtable, in interface-declaration order.
#[repr(C)]
struct ShellLinkVtable {
    // IUnknown
    query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    // IShellLink
    get_path: *const c_void,
    get_id_list: *const c_void,
    set_id_list: *const c_void,
    get_description: *const c_void,
    set_description: unsafe extern "system" fn(*mut c_void, PCWSTR) -> i32,
    get_working_directory: *const c_void,
    set_working_directory: unsafe extern "system" fn(*mut c_void, PCWSTR) -> i32,
    get_arguments: *const c_void,
    set_arguments: unsafe extern "system" fn(*mut c_void, PCWSTR) -> i32,
    get_hotkey: *const c_void,
    set_hotkey: unsafe extern "system" fn(*mut c_void, PCWSTR) -> i32,
    get_show_cmd: *const c_void,
    set_show_cmd: unsafe extern "system" fn(*mut c_void, i32) -> i32,
    get_icon_location: *const c_void,
    set_icon_location: unsafe extern "system" fn(*mut c_void, PCWSTR, i32) -> i32,
    set_relative_path: *const c_void,
    resolve: *const c_void,
    set_path: unsafe extern "system" fn(*mut c_void, PCWSTR) -> i32,
}

/// `IPersistFile`, reached from the shell link via `QueryInterface`.
///
/// The method order is load-bearing. `IPersist` contributes `GetClassID` and
/// `IsDirty`; `IPersistFile` then adds `Load`, `Save`, `SaveCompleted`, and
/// `GetCurFile`. Dropping `IsDirty` shifts `Save` down one slot, where it lands
/// on `Load` — and `Load` on a file that does not exist yet fails with
/// `ERROR_FILE_NOT_FOUND`, which reads exactly like a permissions or path
/// problem rather than an indexing mistake.
#[repr(C)]
struct PersistFileVtable {
    query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    // IPersist
    get_class_id: unsafe extern "system" fn(*mut c_void, *mut GUID) -> i32,
    is_dirty: unsafe extern "system" fn(*mut c_void) -> i32,
    // IPersistFile
    load: unsafe extern "system" fn(*mut c_void, PCWSTR, u32) -> i32,
    save: unsafe extern "system" fn(*mut c_void, PCWSTR, i32) -> i32,
    save_completed: unsafe extern "system" fn(*mut c_void, PCWSTR) -> i32,
    get_cur_file: unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> i32,
}

/// A COM object. Its first field is a pointer to its vtable.
#[repr(C)]
struct ShellLink(*mut c_void);

impl ShellLink {
    /// # Safety
    /// `self.0` must be a live `IShellLinkW`.
    unsafe fn vtable(&self) -> &'static ShellLinkVtable {
        // Two dereferences: the object's first field points at the method table,
        // so the first read yields the table's address. Stopping after one would
        // read the object's own data and then call it as code.
        &*link_vtable(self.0)
    }
}

/// Encode text as a NUL-terminated UTF-16 string.
fn wide(text: &str) -> Vec<u16> {
    std::ffi::OsStr::new(text)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// RAII wrapper around `CoInitializeEx`, so the apartment is always released.
struct ComApartment {
    /// False when the thread was already in an apartment we do not own.
    owned: bool,
}

impl ComApartment {
    fn enter() -> Result<Self, String> {
        // SAFETY: called on the installer's main thread, which does no COM before.
        let hr = unsafe { CoInitializeEx(std::ptr::null_mut(), COINIT_APARTMENTTHREADED as u32) };
        if hr == S_OK {
            return Ok(Self { owned: true });
        }
        // A ShellLink still works in a multi-threaded apartment; we just must not
        // uninitialise an apartment we did not create.
        if hr == RPC_E_CHANGED_MODE {
            return Ok(Self { owned: false });
        }
        Err(format!("COM could not be initialised (0x{hr:08X})"))
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.owned {
            // SAFETY: balances the successful CoInitializeEx in `enter`.
            unsafe { CoUninitialize() };
        }
    }
}

/// Create a `.lnk` shortcut pointing at `target`.
pub fn create_shortcut(
    link_path: &Path,
    target: &Path,
    description: &str,
    working_dir: &Path,
) -> Result<(), String> {
    if let Some(parent) = link_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }

    let _apartment = ComApartment::enter()?;

    let mut shell_link: *mut c_void = std::ptr::null_mut();
    // SAFETY: both GUIDs are valid and the class is in-process. The last argument
    // is `void**`, which a `*mut *mut c_void` already is.
    let hr = unsafe {
        CoCreateInstance(
            &CLSID_SHELL_LINK,
            std::ptr::null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID_SHELL_LINK_W,
            &mut shell_link,
        )
    };
    if hr < 0 || shell_link.is_null() {
        return Err(format!("could not create a shortcut object (0x{hr:08X})"));
    }

    let link = ShellLink(shell_link);
    let target_str = target.to_string_lossy().into_owned();
    let w_target = wide(&target_str);
    let w_desc = wide(description);
    let w_dir = wide(&working_dir.to_string_lossy());
    // An icon index of 0 with the target's own path uses the executable's icon.
    let w_icon = wide(&target_str);

    // SAFETY: `shell_link` is live; every string is NUL-terminated and outlives
    // the call. `ShellLink` is dropped logically here but released below.
    let result = unsafe {
        let vtable = link.vtable();
        let check = |hr: i32, what: &str| -> Result<(), String> {
            if hr < 0 {
                Err(format!("{what} failed (0x{hr:08X})"))
            } else {
                Ok(())
            }
        };
        check((vtable.set_path)(shell_link, w_target.as_ptr()), "SetPath")?;
        check(
            (vtable.set_description)(shell_link, w_desc.as_ptr()),
            "SetDescription",
        )?;
        check(
            (vtable.set_working_directory)(shell_link, w_dir.as_ptr()),
            "SetWorkingDirectory",
        )?;
        check(
            (vtable.set_icon_location)(shell_link, w_icon.as_ptr(), 0),
            "SetIconLocation",
        )?;
        save_link(link_path, shell_link)
    };

    // SAFETY: balances the successful CoCreateInstance reference.
    unsafe { (link.vtable().release)(shell_link) };
    result
}

/// Persist the shell link to disk through `IPersistFile`.
fn save_link(link_path: &Path, shell_link: *mut c_void) -> Result<(), String> {
    // SAFETY: `shell_link` is a live COM object; the out-pointer receives an owned
    // reference, which this function releases on every path.
    let mut persist: *mut c_void = std::ptr::null_mut();
    unsafe {
        let vtable = &*link_vtable(shell_link);
        let hr = (vtable.query_interface)(shell_link, &IID_IPERSIST_FILE, &mut persist);
        if hr < 0 || persist.is_null() {
            return Err(format!("IPersistFile unavailable (0x{hr:08X})"));
        }
    }

    let w_path = wide(&link_path.to_string_lossy());
    // SAFETY: `persist` is a live IPersistFile and `w_path` outlives the call.
    unsafe {
        let vtable = &*persist_vtable(persist);
        let hr = (vtable.save)(persist, w_path.as_ptr(), 1);
        (vtable.release)(persist);
        if hr < 0 {
            Err(format!("could not save the shortcut (0x{hr:08X})"))
        } else {
            Ok(())
        }
    }
}

/// The `IShellLinkW` method table for a live interface pointer.
///
/// Returns the table's address rather than a reference, because the table lives
/// in the shell's read-only data rather than in anything Rust can borrow from.
///
/// # Safety
/// `ptr` must be a live `IShellLinkW`.
unsafe fn link_vtable(ptr: *mut c_void) -> *const ShellLinkVtable {
    *(ptr as *const *const ShellLinkVtable)
}

/// The `IPersistFile` method table for an interface pointer from `QueryInterface`.
///
/// # Safety
/// `ptr` must be a live `IPersistFile`.
unsafe fn persist_vtable(ptr: *mut c_void) -> *const PersistFileVtable {
    *(ptr as *const *const PersistFileVtable)
}

/// Open a registry key under `HKCU` with the requested access, creating it if
/// needed. Failures name the key, since a silent registry miss is very hard to
/// diagnose later.
fn create_key(path: &str) -> Result<HKEY, String> {
    let w_path = wide(path);
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: `w_path` is NUL-terminated and outlives the call. `RegCreateKeyW`
    // opens an existing key if there is one and creates it otherwise, which is
    // what a per-user installer wants: nothing may fail merely because a key has
    // not been written yet.
    let status = unsafe { RegCreateKeyW(HKEY_CURRENT_USER, w_path.as_ptr(), &mut key) };
    if status != 0 {
        return Err(format!(
            "could not open registry key {path} (status {status})"
        ));
    }
    Ok(key)
}

/// Open an existing key without creating it, so probing leaves no trace behind.
fn open_existing_key(path: &str, access: u32) -> Result<HKEY, String> {
    let w_path = wide(path);
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: `w_path` is NUL-terminated and outlives the call.
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w_path.as_ptr(),
            REG_OPTION_NON_VOLATILE,
            access,
            &mut key,
        )
    };
    if status != 0 {
        return Err(format!(
            "could not open registry key {path} (status {status})"
        ));
    }
    Ok(key)
}

/// Set a string value, replacing any existing one.
pub fn set_reg_string(key_path: &str, name: &str, value: &str) -> Result<(), String> {
    let key = create_key(key_path)?;
    let w_name = wide(name);
    let w_value = wide(value);
    let bytes = (w_value.len() * 2) as u32;
    // SAFETY: `key` is open for writing and both strings outlive the call. The
    // value pointer is consumed as raw bytes, so alignment is not required.
    let status = unsafe {
        RegSetValueExW(
            key,
            w_name.as_ptr(),
            0,
            REG_SZ,
            w_value.as_ptr() as *const u8,
            bytes,
        )
    };
    // SAFETY: balances the open above.
    unsafe { RegCloseKey(key) };
    if status != 0 {
        return Err(format!(
            "could not set {key_path}\\{name} (status {status})"
        ));
    }
    Ok(())
}

/// Set a 32-bit integer value.
pub fn set_reg_u32(key_path: &str, name: &str, value: u32) -> Result<(), String> {
    let key = create_key(key_path)?;
    let w_name = wide(name);
    // SAFETY: as above; `value` lives for the duration of the call.
    let status = unsafe {
        RegSetValueExW(
            key,
            w_name.as_ptr(),
            0,
            4, // REG_DWORD
            &value as *const u32 as *const u8,
            4,
        )
    };
    // SAFETY: balances the open above.
    unsafe { RegCloseKey(key) };
    if status != 0 {
        return Err(format!(
            "could not set {key_path}\\{name} (status {status})"
        ));
    }
    Ok(())
}

/// Read a string value. A missing key or value is reported as `None`, since that
/// is the normal state on a first run.
pub fn get_reg_string(key_path: &str, name: &str) -> Option<String> {
    let w_name = wide(name);
    let mut size = 0u32;
    // SAFETY: a null buffer with a size of 0 asks for the required size only. A
    // non-zero result here means the value does not exist, which is not an error.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            wide(key_path).as_ptr(),
            w_name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if status != 0 || size == 0 {
        return None;
    }

    // `size` counts bytes including the terminator.
    let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
    let mut size = (buffer.len() * 2) as u32;
    // SAFETY: `buffer` is `size` bytes long, as just measured and then re-measured.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            wide(key_path).as_ptr(),
            w_name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr() as *mut c_void,
            &mut size,
        )
    };
    if status != 0 {
        return None;
    }
    let text = String::from_utf16_lossy(&buffer);
    let text = text.trim_end_matches('\0');
    (!text.is_empty()).then(|| text.to_string())
}

/// Delete a value if it exists. A missing value is not an error.
pub fn delete_reg_value(key_path: &str, name: &str) {
    if let Ok(key) = open_existing_key(key_path, KEY_SET_VALUE) {
        let w_name = wide(name);
        // SAFETY: both handles are valid; a missing value reports an error that is
        // intentionally ignored, because "not there" is the desired state.
        unsafe {
            RegDeleteValueW(key, w_name.as_ptr());
            RegCloseKey(key);
        }
    }
}

/// Delete a key if it exists. A missing key is not an error.
pub fn delete_reg_key(key_path: &str) {
    let w_path = wide(key_path);
    // SAFETY: `w_path` is NUL-terminated.
    unsafe {
        RegDeleteKeyW(HKEY_CURRENT_USER, w_path.as_ptr());
    }
}

/// Whether the ViGEmBus virtual gamepad driver is present.
///
/// PadForge runs without it, but input will not reach games, so the installer
/// says so up front instead of leaving the user to work it out later.
pub fn vigem_installed() -> bool {
    // ViGEmBus exposes a control device under the Win32 device namespace. Its
    // absence shows up as a "path not found" error, which is cheap to test and
    // needs no driver handle to be opened.
    // SAFETY: a zeroed desired-access, share mode, and disposition is a plain
    // existence probe; the path is NUL-terminated.
    let handle = unsafe {
        CreateFileW(
            wide(r"\\.\VIGEMBUS").as_ptr(),
            0,
            0,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    let ok = handle != INVALID_HANDLE_VALUE;
    if ok {
        // SAFETY: `handle` is a valid open handle returned by CreateFileW.
        unsafe {
            CloseHandle(handle);
        }
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_is_nul_terminated() {
        let w = wide("A\\B");
        assert_eq!(*w.last().unwrap(), 0);
        assert_eq!(&w[..3], &[0x41, 0x5C, 0x42]);
    }

    /// The whole reason the vtable is hand-written: a wrong offset produces a link
    /// pointing at the wrong thing. Writing one and reading it back proves the
    /// layout is right.
    #[test]
    fn shortcut_round_trips_through_the_vtable() {
        let dir = std::env::temp_dir().join("padforge-installer-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let target = dir.join("target.exe");
        std::fs::write(&target, b"not a real exe").unwrap();
        let link = dir.join("test.lnk");

        create_shortcut(&link, &target, "PadForge test", &dir)
            .expect("a shortcut should be created");

        let bytes = std::fs::read(&link).expect("the shortcut should exist");
        assert!(bytes.len() > 100, "a real shell link is longer than this");
        // The target path is stored in the link, so it must appear verbatim.
        let needle = target.to_string_lossy().into_owned();
        let haystack: String = bytes
            .iter()
            .filter(|b| **b != 0)
            .map(|b| *b as char)
            .collect();
        assert!(
            haystack.contains(&needle),
            "the shortcut should reference {needle}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A scratch registry key that deletes itself on drop, so a failing assertion
    /// cannot leave rubbish behind. Each test uses its own key because the test
    /// harness runs them in parallel threads against one registry.
    struct Scratch(&'static str);

    impl Drop for Scratch {
        fn drop(&mut self) {
            delete_reg_key(self.0);
        }
    }

    #[test]
    fn reg_round_trips_a_string() {
        let key = Scratch(r"Software\PadForge\InstallerTestString");
        assert_eq!(get_reg_string(key.0, "Value"), None);

        set_reg_string(key.0, "Value", r"C:\Program Files\PadForge").unwrap();
        assert_eq!(
            get_reg_string(key.0, "Value").as_deref(),
            Some(r"C:\Program Files\PadForge")
        );
    }

    #[test]
    fn reg_round_trips_a_dword() {
        let key = Scratch(r"Software\PadForge\InstallerTestDword");
        set_reg_u32(key.0, "Size", 7123).unwrap();
        // Reading it back as a string fails by design, because it is not a string.
        assert_eq!(get_reg_string(key.0, "Size"), None);
    }

    #[test]
    fn a_missing_key_reads_as_none_and_is_not_created() {
        let path = r"Software\PadForge\InstallerTestAbsent";
        delete_reg_key(path);
        assert_eq!(get_reg_string(path, "Value"), None);
        // Probing must not leave a key behind just because it asked.
        let _ = open_existing_key(path, KEY_SET_VALUE);
    }

    #[test]
    fn vigem_probe_does_not_panic() {
        // The answer depends on the machine; the point is that it is safe to ask.
        let _ = vigem_installed();
    }
}
