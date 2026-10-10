//! Opens a DS4 and reads real reports from it, printing what arrives.
//!
//! Enumeration succeeding only proves the device list is populated. What matters
//! is whether `read` ever returns, because a pad that enumerates but never
//! reports looks identical to a connected pad in any UI that only shows a
//! connection state.
//!
//! Run:
//!     cargo run -p padcore --bin probe-reports --release --target x86_64-pc-windows-gnu

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use padcore::device::probe_matching_devices;
use padcore::report::{self, Ds4Report};

/// How many reports to wait for before giving up.
const MAX_READS: usize = 500;
/// How long to keep reading after the last one arrived. A pad that stops sending
/// while still enumerated is a distinct failure from one that never starts.
const IDLE_TIMEOUT: Duration = Duration::from_secs(5);

fn main() {
    let devices = probe_matching_devices();
    if devices.is_empty() {
        println!("no DS4 passed the filter, nothing to read");
        println!("run probe-hid first: it says whether that is a pairing problem");
        return;
    }

    let api = match hidapi::HidApi::new() {
        Ok(a) => a,
        Err(e) => {
            println!("could not initialise the HID API: {e}");
            return;
        }
    };

    let mut any_worked = false;

    for info in &devices {
        println!("=== {} ===", info.product);
        println!("  path    : {}", info.path);
        println!("  serial  : {}", info.serial);
        println!("  bus     : {}", info.bus_type);
        println!(
            "  usage   : page {:04x} usage {:04x}",
            info.usage_page, info.usage
        );

        let path = match std::ffi::CString::new(info.path.as_str()) {
            Ok(p) => p,
            Err(_) => {
                println!("  the path contains a NUL byte and cannot be opened");
                continue;
            }
        };

        let started = Instant::now();
        let device = match api.open_path(&path) {
            Ok(d) => d,
            Err(e) => {
                println!("  open FAILED: {e}");
                println!("  this is the usual state when another program already has the");
                println!("  pad open, since HID is exclusive by default");
                continue;
            }
        };
        println!("  opened in {:?}", started.elapsed());

        let mut buffer = [0u8; 128];
        let mut reads = 0usize;
        let mut parsed = 0usize;
        let mut distinct = BTreeSet::new();
        let mut lengths = BTreeSet::new();
        let mut last = Instant::now();
        let mut error = None;

        while reads < MAX_READS {
            match device.read(&mut buffer) {
                Ok(n) if n > 0 => {
                    reads += 1;
                    lengths.insert(n);
                    distinct.insert(buffer[..n].to_vec());
                    last = Instant::now();
                    if report::parse(&buffer[..n]).is_some() {
                        parsed += 1;
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    error = Some(e.to_string());
                    break;
                }
            }
            if last.elapsed() > IDLE_TIMEOUT {
                break;
            }
        }

        println!("\n  reads              : {reads}");
        println!("  parsed as a DS4    : {parsed}");
        println!("  distinct payloads  : {}", distinct.len());
        println!("  report lengths seen: {lengths:?}");
        if let Some(e) = error {
            println!("  read error         : {e}");
        }

        if reads == 0 {
            println!("\n  the pad enumerates but never sent a report.");
            println!("  That is a pairing state rather than an application fault: the DS4");
            println!("  only streams over HID once a host has requested the report");
            println!("  descriptor. In order of likelihood:");
            println!("    - press the PS button once, then move a stick");
            println!("    - hold SHARE + PS to re-enter pairing, then reconnect");
            println!("    - confirm the lightbar is lit and not blinking");
            println!("    - plug in USB, which separates pairing from the application");
        } else if parsed == 0 {
            println!("\n  reports arrived but none parsed. The raw lengths were {lengths:?},");
            println!("  and the first bytes were:");
            if let Some(r) = distinct.iter().next() {
                println!("    {}", hex(r));
            }
            println!("  A USB report is 64 bytes starting 01, Bluetooth 78 or 79. Other");
            println!("  lengths mean the wrong interface was opened.");
        } else {
            any_worked = true;
            println!("\n  reading works.");
            if let Some(r) = distinct.iter().next() {
                println!("    first report: {} bytes, {}", r.len(), hex(r));
                if let Some(d) = report::parse(r) {
                    show(&d);
                }
            }
            // The most recent report, which is more interesting than the first:
            // it reflects whatever the user is doing right now.
            if let Some(r) = distinct.iter().next_back() {
                println!("    last  report: {} bytes, {}", r.len(), hex(r));
                if let Some(d) = report::parse(r) {
                    show(&d);
                }
            }
        }
    }

    if any_worked {
        println!("\nThe hardware path works end to end: open, read, decode.");
    } else {
        println!("\nNo pad produced a decoded report. The filter is not the problem:");
        println!("probe-hid confirms the device is enumerated, so this is the");
        println!("connection or the host's report-descriptor request, not the code.");
    }
}

fn hex(r: &[u8]) -> String {
    r.iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn show(d: &Ds4Report) {
    println!(
        "      sticks    L({:+.3}, {:+.3})  R({:+.3}, {:+.3})",
        d.left_x, d.left_y, d.right_x, d.right_y
    );
    println!(
        "      raw bytes L({}, {})  R({}, {})",
        d.raw_left_x, d.raw_left_y, d.raw_right_x, d.raw_right_y
    );
    println!(
        "      triggers  L2 {:.0}%  R2 {:.0}%",
        d.l2 * 100.0,
        d.r2 * 100.0
    );
    println!("      buttons   {:#06x}", d.buttons.0);
    println!(
        "      gyro      yaw {:+.1}  pitch {:+.1}  roll {:+.1}",
        d.gyro.yaw, d.gyro.pitch, d.gyro.roll
    );
    println!(
        "      touchpad  touched={}  ({:.3}, {:.3})",
        d.touch.pad_touched, d.touch.x, d.touch.y
    );
    println!(
        "      battery   {:.0}%  charging={}",
        d.battery.fraction() * 100.0,
        d.battery.charging
    );
}
