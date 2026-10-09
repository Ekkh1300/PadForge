//! Finds the output report length this HID interface actually accepts.
//!
//! hidapi pads a short write out to the device's declared output report length,
//! and it only does so when the buffer it was given is shorter than that length.
//! If the padding is wrong for this device, every write fails with
//! ERROR_INVALID_PARAMETER and nothing is ever sent.
//!
//! So the length is searched rather than assumed. A Bluetooth DS4 v2 takes a 78
//! byte output report, but the interface hidapi opened may declare something
//! else, and the only way to know is to try.
//!
//! Run:
//!     cargo run -p padcore --bin probe-rumble-length --release --target x86_64-pc-windows-gnu

use std::thread::sleep;
use std::time::Duration;

use padcore::device::probe_matching_devices;

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

        println!(
            "=== {} on {}, interface {} ===",
            info.product, info.bus_type, info.interface_number
        );

        // Only the interface that actually reports is worth writing to. The
        // others accept bytes and drive nothing, which is indistinguishable
        // from a rumble that did not work.
        let mut alive = false;
        {
            let mut buf = [0u8; 256];
            for _ in 0..10 {
                if device.read_timeout(&mut buf, 50).unwrap_or(0) > 0 {
                    alive = true;
                    break;
                }
            }
        }
        if !alive {
            println!("  this interface never reports, so writing to it proves nothing");
            println!();
            continue;
        }
        println!("  this interface is live, so a result here is meaningful");

        println!("\n  trying report lengths, full rumble, 250 ms apart:");
        // Lengths covering every plausible case, including the ones Windows pads
        // to. A padded write succeeds only when the padding lands right, so a
        // length that fails is not merely a wrong guess, it is evidence.
        let lengths = [32u32, 64, 78, 96, 97, 128, 256];

        let mut accepted = Vec::new();
        for len in lengths {
            let mut buf = vec![0u8; len as usize];
            buf[0] = 0x05;
            // Byte 6 is the fast motor, byte 7 the slow one.
            if len > 7 {
                buf[6] = 255;
                buf[7] = 255;
            }
            match device.send_output_report(&buf) {
                Ok(()) => {
                    println!("    {len:>3} bytes  ACCEPTED   <- feel it");
                    accepted.push(len);
                    sleep(Duration::from_millis(250));
                }
                Err(e) => println!("    {len:>3} bytes  rejected: {e}"),
            }
        }

        // Stop whatever was left running, using whichever length worked.
        if let Some(len) = accepted.last() {
            let mut stop = vec![0u8; *len as usize];
            stop[0] = 0x05;
            let _ = device.send_output_report(&stop);
        }

        println!("\n  accepted lengths: {accepted:?}");
        if accepted.is_empty() {
            println!("  nothing was accepted, so the failure is not the length.");
            println!("  On a Bluetooth DS4 this usually means output has to be enabled");
            println!("  first: the pad does not act on report 0x05 until it has been");
            println!("  switched into output mode by a feature report.");
        } else {
            println!("\n  use one of those lengths in output_report() and the rumble");
            println!("  will reach the pad. The 78-byte assumption is the bug.");
        }
    }
}
