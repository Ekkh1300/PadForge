//! Asks the HID interface what a write must look like, then makes it act.
//!
//! `hid_write` pads every buffer to `caps.OutputReportByteLength`, so trying
//! seven buffer sizes tests the same write seven times and reports the same
//! answer seven times. That makes a length sweep look like conclusive evidence
//! that length is not the problem, when all it proves is that hidapi normalises.
//!
//! This opens the pad through the Windows HID API directly, so the size written
//! is exactly the size asked for. Three questions, in order:
//!
//!   1. What does the interface declare? (`HidP_GetCaps`)
//!   2. Which sizes and report ids does the driver accept?
//!   3. Once a write is accepted, does the pad actually move — lightbar first,
//!      because it answers instantly and is visible from across the room, then
//!      the motors.
//!
//! Question 3 is the one that matters. A write the driver accepts and the pad
//! ignores is indistinguishable, from inside the application, from a write that
//! worked, and that is exactly what a lightbar which never changes is.
//!
//! Run:
//!     cargo run -p padcore --bin probe-output-caps --release --target x86_64-pc-windows-gnu

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use std::thread::sleep;
use std::time::Duration;

use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    HidD_FreePreparsedData, HidD_GetPreparsedData, HidP_GetCaps, HIDP_CAPS,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE, TRUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, WriteFile, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::{GetOverlappedResult, OVERLAPPED};

use padcore::device::probe_matching_devices;
use padcore::report::{self, Transport};

/// Windows gives this name to 87, which is the error behind every symptom being
/// investigated here: the lightbar that will not light and the motors that will
/// not turn.
const ERROR_INVALID_PARAMETER: u32 = 87;
/// The write was queued rather than completed, which is normal on an overlapped
/// handle and is not a failure.
const ERROR_IO_PENDING: u32 = 997;

/// Open the pad the way the application does, keeping the handle so writes of an
/// exact size can be attempted against it.
///
/// Returns `INVALID_HANDLE_VALUE` on failure, with `GetLastError` still holding
/// the reason, because the reason is the interesting part.
fn open(path: &str, overlapped: bool) -> *mut c_void {
    let wide: Vec<u16> = std::ffi::OsStr::new(path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null(),
            OPEN_EXISTING,
            if overlapped { FILE_FLAG_OVERLAPPED } else { 0 },
            ptr::null_mut(),
        )
    }
}

/// What this collection says it is and what it says it will take.
fn caps_of(handle: *mut c_void) -> Option<HIDP_CAPS> {
    // windows-sys models the opaque preparsed pointer as `isize`.
    let mut preparsed: isize = 0;
    if !unsafe { HidD_GetPreparsedData(handle, &mut preparsed) } {
        eprintln!("  HidD_GetPreparsedData failed, win32 error {}", unsafe {
            GetLastError()
        });
        return None;
    }
    let mut caps: HIDP_CAPS = unsafe { std::mem::zeroed() };
    let status = unsafe { HidP_GetCaps(preparsed, &mut caps) };
    unsafe { HidD_FreePreparsedData(preparsed) };
    // HIDP_STATUS_SUCCESS is 0x00110000, not zero, which is the usual NTSTATUS
    // convention and the reason a naive `!= 0` check reports failure here.
    const HIDP_STATUS_SUCCESS: i32 = 0x0011_0000;
    if status != HIDP_STATUS_SUCCESS {
        eprintln!("  HidP_GetCaps failed, ntstatus {status:#010x}");
        return None;
    }
    Some(caps)
}

/// One synchronous `WriteFile`, returning either the byte count or the error.
fn try_write(handle: *mut c_void, buf: &[u8]) -> Result<u32, u32> {
    let mut written = 0u32;
    let ok = unsafe {
        WriteFile(
            handle,
            buf.as_ptr(),
            buf.len() as u32,
            &mut written,
            ptr::null_mut(),
        )
    };
    if ok == TRUE {
        Ok(written)
    } else {
        Err(unsafe { GetLastError() })
    }
}

/// The same write against an overlapped handle, which is what hidapi opens.
///
/// Worth isolating because it is the one difference between this probe, where a
/// write of the right size goes through, and `hid_write`, where every write
/// comes back as error 87.
fn try_write_overlapped(handle: *mut c_void, buf: &[u8]) -> Result<u32, u32> {
    let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
    let mut written = 0u32;
    let ok = unsafe {
        WriteFile(
            handle,
            buf.as_ptr(),
            buf.len() as u32,
            &mut written,
            &mut ov,
        )
    };
    if ok == TRUE {
        return Ok(written);
    }
    let e = unsafe { GetLastError() };
    if e != ERROR_IO_PENDING {
        return Err(e);
    }
    // `&raw mut` rather than `&mut`: the driver writes into this struct through
    // the pointer the OS was handed, so no Rust reference to it should exist at
    // the time it does.
    if unsafe { GetOverlappedResult(handle, &raw mut ov, &mut written, TRUE) } == TRUE {
        Ok(written)
    } else {
        Err(unsafe { GetLastError() })
    }
}

/// A short name for the error codes worth telling apart.
fn why(code: u32) -> &'static str {
    match code {
        ERROR_INVALID_PARAMETER => "refused as shaped",
        5 => "no write access on the handle",
        31 => "the device refused the report itself",
        121 => "the device did not answer in time",
        1784 => "empty user buffer",
        _ => "unrecognised",
    }
}

fn main() {
    let devices = probe_matching_devices();
    if devices.is_empty() {
        eprintln!("no DS4 is connected");
        std::process::exit(1);
    }

    for info in &devices {
        println!("=== {} on {} ===", info.product, info.bus_type);
        println!("    path: {}\n", info.path);

        let handle = open(&info.path, false);
        if handle == INVALID_HANDLE_VALUE {
            let e = unsafe { GetLastError() };
            println!("    could not open: {e} — {}", why(e));
            println!();
            continue;
        }

        match caps_of(handle) {
            None => println!("    caps unavailable, continuing without them\n"),
            Some(c) => println!(
                "  the interface declares:\n    usage page {:04x} usage {:04x}\n    \
                 input report {} bytes\n    output report {} bytes   <- the write must be at least this\n    \
                 feature report {} bytes\n    output button/value groups: {} / {}\n",
                c.UsagePage,
                c.Usage,
                c.InputReportByteLength,
                c.OutputReportByteLength,
                c.FeatureReportByteLength,
                c.NumberOutputButtonCaps,
                c.NumberOutputValueCaps,
            ),
        }

        // Exact sizes, so the accepted one can be read off rather than inferred.
        // hidapi cannot produce this experiment: it rewrites the length first.
        println!("\n  exact write lengths, report id 0x11:");
        let mut smallest_ok = None;
        for len in [0usize, 1, 77, 78, 79, 128, 256, 334, 512] {
            let mut buf = vec![0u8; len];
            if len > 0 {
                buf[0] = 0x11;
            }
            match try_write(handle, &buf) {
                Ok(_) => {
                    println!("    {len:>3} bytes  accepted");
                    smallest_ok.get_or_insert(len);
                }
                Err(e) => println!("    {len:>3} bytes  {e} — {}", why(e)),
            }
        }
        let Some(least) = smallest_ok else {
            println!("\n  nothing at any size, so the failure is not the length.");
            unsafe { CloseHandle(handle) };
            continue;
        };

        // Now that a working size is known, ask which report id the collection
        // declares. The id is byte zero and is the only part of a write hidapi
        // cannot have rearranged.
        println!("\n  report ids, at {least} bytes:");
        for id in [0x00u8, 0x01, 0x04, 0x05, 0x11, 0x12, 0x15, 0x80, 0xC0] {
            let mut buf = vec![0u8; least];
            buf[0] = id;
            match try_write(handle, &buf) {
                Ok(_) => println!("    id {id:02x}  accepted"),
                Err(e) => println!("    id {id:02x}  {e} — {}", why(e)),
            }
        }

        // The one difference from hidapi's path: an overlapped handle.
        println!("\n  the same write on an overlapped handle, which is what hidapi opens:");
        let oh = open(&info.path, true);
        if oh == INVALID_HANDLE_VALUE {
            let e = unsafe { GetLastError() };
            println!("    could not open overlapped: {e} — {}", why(e));
        } else {
            let mut buf = vec![0u8; least];
            buf[0] = 0x11;
            match try_write_overlapped(oh, &buf) {
                Ok(w) => println!("    {least} bytes  accepted ({w} written)"),
                Err(e) => println!("    {least} bytes  {e} — {}", why(e)),
            }
            unsafe { CloseHandle(oh) };
        }

        // The only result that settles anything. The lightbar answers within a
        // frame and needs no cooperation from the reader; the motors are the
        // thing the user actually asked about.
        println!("\n  does the pad act? Watch the lightbar, then hold the pad.");
        let transport = if info.bus_type.eq_ignore_ascii_case("Bluetooth") {
            Transport::Bluetooth
        } else {
            Transport::Usb
        };

        let send = |r: u8, g: u8, b: u8, rumble: Option<(u8, u8)>| {
            // A CRC the pad can verify, so a report it drops is reported here as
            // dropped rather than quietly accepted.
            let buf = report::output_report_len(transport, least, r, g, b, rumble);
            try_write(handle, &buf)
        };

        for (label, r, g, b) in [
            ("blue", 0, 0, 255),
            ("green", 0, 255, 0),
            ("red", 255, 0, 0),
        ] {
            match send(r, g, b, None) {
                Ok(_) => {
                    println!("    lightbar {label:<5} sent  <- is the pad showing {label}?");
                    sleep(Duration::from_millis(1200));
                }
                Err(e) => println!("    lightbar {label:<5} {e} — {}", why(e)),
            }
        }

        println!("    full rumble for 1.5 s... <- is it buzzing?");
        match send(255, 255, 255, Some((255, 255))) {
            Ok(_) => {
                sleep(Duration::from_millis(1500));
                let _ = send(0, 0, 0, Some((0, 0)));
                println!("    rumble sent and stopped");
            }
            Err(e) => println!("    rumble {e} — {}", why(e)),
        }

        unsafe { CloseHandle(handle) };
        println!();
    }
}
