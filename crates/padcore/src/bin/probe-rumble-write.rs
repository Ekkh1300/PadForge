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
///
/// Longer than it needs to be on hardware. A burst short enough to miss is
/// indistinguishable, when somebody is holding the pad and not watching a
/// console, from motors that never fired at all — and that difference is the
/// entire question this probe asks.
const BURST: Duration = Duration::from_millis(2000);
/// How long between rewrites of the held report.
///
/// Close to the interval a driver uses, and comfortably inside anything the
/// pad would time out on, so a pad that needs refreshing stays refreshed.
const TICK: Duration = Duration::from_millis(8);
/// How long the pad is left alone before each of the two cases, so the second
/// one cannot be reading state the first one left behind.
const RESET: Duration = Duration::from_millis(700);
/// How long the "write once" case runs before the pad is asked to stop.
const HOLD_FOR: Duration = Duration::from_secs(2);
/// How long the motors are given to be silent afterwards.
const QUIET_FOR: Duration = Duration::from_millis(1200);

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

        // Two passes, because they answer different questions.
        //
        // The first sends once and waits. If the motors run, the report is
        // right and the fault is upstream, in the application.
        //
        // The second rewrites the same report for the whole burst, the way a
        // driver does. A pad that drops its output state shortly after a write
        // is silent under the first and loud under the second — which would be
        // a real difference, and one that no amount of reading the layout will
        // tell you.
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

            // `write`, not `send_output_report`: the latter sends through the
            // 64 byte feature report buffer, which cannot hold this report, so
            // every write is refused before it leaves the process.
            match device.write(&report) {
                Ok(_) => println!("accepted, held with a single write"),
                Err(e) => println!("REJECTED: {e}"),
            }
            println!("     -> feel it now, {BURST:?}");
            sleep(BURST);

            let stop = report::output_report(transport, 0, 0, 0, Some((0, 0)));
            match device.write(&stop) {
                Ok(_) => println!("     stop accepted"),
                Err(e) => println!("     stop REJECTED: {e}"),
            }
            sleep(Duration::from_millis(400));
        }

        println!("\n  held by rewriting, once every {TICK:?}");
        println!("  hold the pad for {BURST:?} at a time");
        for (name, heavy, light) in [
            ("both motors", 255u8, 255u8),
            ("heavy only", 255, 0),
            ("light only", 0, 255),
        ] {
            let report = report::output_report(transport, 0, 140, 255, Some((heavy, light)));
            print!("  {name:<12} -> ");
            std::io::Write::flush(&mut std::io::stdout()).ok();

            let until = Instant::now() + BURST;
            let mut written = 0usize;
            let mut rejected = None;
            while Instant::now() < until {
                if let Err(e) = device.write(&report) {
                    rejected = Some(e.to_string());
                    break;
                }
                written += 1;
                sleep(TICK);
            }

            match rejected {
                None => println!("{written} writes, all accepted"),
                Some(e) => println!("{written} writes accepted, then REJECTED: {e}"),
            }

            let stop = report::output_report(transport, 0, 0, 0, Some((0, 0)));
            device.write(&stop).ok();
            sleep(Duration::from_millis(400));
        }

        println!("\n  a rejected write means the report itself is wrong, since the");
        println!("  interface is the one that reports. An accepted write that produces");
        println!("  no sensation means the pad received it and chose to ignore it,");
        println!("  which is a different fault and a different fix.");

        // One question no amount of reading the layout can answer: does the pad
        // keep the state it was given?
        //
        // A DS4 clears its motors on its own shortly after the last write, which
        // a single report cannot tell you anything about. Under that behaviour a
        // write is always accepted, the report is always correct, and the motors
        // still never run — because the only thing that would have held them was
        // the next write, and there wasn't one. Rewriting and then reading the
        // pad back is the difference between a pad that was told to buzz and a
        // pad that is buzzing.
        println!("\n  does the pad keep what it was told?");
        let steady = report::output_report(transport, 0, 140, 255, Some((255, 255)));
        let quiet = report::output_report(transport, 0, 140, 255, Some((0, 0)));

        for (name, report, held) in [
            ("write once", &steady, false),
            ("write, keep writing", &steady, true),
        ] {
            let stop = report::output_report(transport, 0, 140, 255, Some((0, 0)));
            device.write(&stop).ok();
            sleep(RESET);

            let mut writes = 0usize;
            let start = Instant::now();
            if held {
                let until = start + HOLD_FOR;
                while Instant::now() < until {
                    if device.write(report).is_ok() {
                        writes += 1;
                    }
                    sleep(TICK);
                }
            } else if device.write(report).is_ok() {
                writes = 1;
            }

            let quiet_writes = {
                let until = Instant::now() + QUIET_FOR;
                let mut n = 0usize;
                while Instant::now() < until {
                    if device.write(&quiet).is_ok() {
                        n += 1;
                    }
                    sleep(TICK);
                }
                n
            };

            println!("  {name:<18} {writes} motor writes, {quiet_writes} stopped writes");
            println!("     feel it through the {QUIET_FOR:?} after, {name} — the motors should be");
            println!("     dead by the end of it either way, so what matters is the first half");
            sleep(Duration::from_millis(600));
        }

        println!("\n  If neither half buzzed, the pad is discarding report 0x11's motor");
        println!("  bytes no matter how they arrive, which is a pad-side setting or");
        println!("  firmware question rather than a bug in this code.");
        println!("  If the second buzzed and the first did not, the motors need writing");
        println!("  at a steady rate, and one report is not enough to hold them.");
    }
}
