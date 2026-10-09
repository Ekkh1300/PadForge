//! Writes rumble to the interface that actually reports, and reports what the
//! HID layer returned.
//!
//! A Bluetooth DS4 exposes more than one HID collection, and only one of them is
//! the pad. Writing to the others succeeds and drives nothing, which looks
//! exactly like a rumble that did not work, so this skips them rather than
//! burning time proving it.
//!
//! The HID return value is printed rather than discarded. Everything upstream of
//! this is an assumption: a queued buffer says nothing about whether the pad
//! acted on it.
//!
//! Run:
//!     cargo run -p padcore --bin probe-rumble-write --release --target x86_64-pc-windows-gnu

use std::thread::sleep;
use std::time::{Duration, Instant};

use padcore::device::probe_matching_devices;
use padcore::report::{self, Transport};

/// How long an interface gets to prove it is alive.
const LIVENESS: Duration = Duration::from_millis(700);
/// How long each burst runs.
const BURST: Duration = Duration::from_millis(800);

fn main() {
    let devices = probe_matching_devices();
    if devices.is_empty() {
        eprintln!("no DS4 is connected");
        std::process::exit(1);
    }

    let api = match hidapi::HidApi::new() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("could not initialise the HID API: {e}");
            std::process::exit(1);
        }
    };

    for info in &devices {
        let Ok(path) = std::ffi::CString::new(info.path.as_str()) else {
            continue;
        };
        let Ok(device) = api.open_path(&path) else {
            continue;
        };

        // Is this the pad? A single short read settles it.
        let mut buf = [0u8; 256];
        let mut alive = false;
        let deadline = Instant::now() + LIVENESS;
        while Instant::now() < deadline {
            match device.read_timeout(&mut buf, 50) {
                Ok(n) if n > 0 && report::parse(&buf[..n]).is_some() => {
                    alive = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }

        if !alive {
            println!(
                "skipping interface {} on {}: it never reports, so it is not the pad",
                info.interface_number, info.bus_type
            );
            continue;
        }

        // The transport comes from the report id the pad sends, not from the bus
        // type. This pad reports its Bluetooth interface as "Usb" through hidapi
        // while the frames themselves are 0x11, so trusting the bus string picks
        // the wrong output layout and the rumble never works.
        let transport = match buf[0] {
            0x11 => Transport::Bluetooth,
            0x01 => Transport::Usb,
            other => {
                println!("  unknown report id 0x{other:02x}, skipping");
                continue;
            }
        };
        let (fast, slow) = match transport {
            Transport::Usb => (4usize, 5usize),
            Transport::Bluetooth => (6, 7),
        };

        println!("=== the pad, on {} ===", info.bus_type);
        println!(
            "  transport {} sends report 0x{:02x}",
            transport.label(),
            if transport == Transport::Bluetooth {
                0x11
            } else {
                0x05
            }
        );

        for (name, heavy, light) in [
            ("both motors", 255u8, 255u8),
            ("heavy only", 255, 0),
            ("light only", 0, 255),
        ] {
            let report = report::output_report(transport, 0, 140, 255, Some((heavy, light)));
            print!(
                "  {name:<12} {} bytes, id=0x{:02x}, fast={}, slow={} -> ",
                report.len(),
                report[0],
                report[fast],
                report[slow]
            );
            std::io::Write::flush(&mut std::io::stdout()).ok();

            match device.send_output_report(&report) {
                Ok(()) => println!("accepted"),
                Err(e) => println!("REJECTED: {e}"),
            }
            println!("     -> feel it now, {BURST:?}");
            sleep(BURST);

            let stop = report::output_report(transport, 0, 0, 0, Some((0, 0)));
            match device.send_output_report(&stop) {
                Ok(()) => println!("     stop accepted"),
                Err(e) => println!("     stop REJECTED: {e}"),
            }
            sleep(Duration::from_millis(400));
        }

        println!("\n  a rejected write means the report itself is wrong, since the");
        println!("  interface is the one that reports. An accepted write that produces");
        println!("  no sensation means the pad received it and chose to ignore it,");
        println!("  which is a different fault and a different fix.");
    }
}
