//! Runs the real reader against the real pad and reports what it sees.
//!
//! Every probe so far has exercised one layer at a time. This one drives
//! `DeviceReader`, which is what the application actually uses, because the
//! interface-selection fix lives there and a decoder-only test would pass with
//! the reader still blocking on the silent collection.
//!
//! Run:
//!     cargo run -p padcore --bin probe-reader --release --target x86_64-pc-windows-gnu

use std::time::{Duration, Instant};

use padcore::device::DeviceReader;

/// How long to let the reader run. Long enough that the probe phase has finished
/// and reports have been flowing for a while.
const RUN: Duration = Duration::from_secs(8);

fn main() {
    let reader = match DeviceReader::start(None, 1000) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("could not start the reader: {e}");
            std::process::exit(1);
        }
    };

    println!("reader started, waiting {RUN:?}...");
    let started = Instant::now();
    let mut was_connected = false;
    let mut reported_connect = false;

    while started.elapsed() < RUN {
        std::thread::sleep(Duration::from_millis(200));
        let s = reader.snapshot();

        if s.connected && !was_connected {
            println!(
                "  connected after {:?}: {} via {}",
                started.elapsed(),
                s.info.label(),
                s.info.transport.label()
            );
            was_connected = true;
            continue;
        }

        if !s.connected {
            if was_connected {
                println!("  disconnected");
                was_connected = false;
            }
            continue;
        }

        // Wait for the packet counter to actually move. That is the difference
        // between "the reader opened a handle" and "reports are arriving", and
        // it is precisely what was broken: a reader blocked on the silent
        // interface reports connected with a packet count frozen at zero.
        if s.packets > 0 && !reported_connect {
            println!("  first decoded report after {:?}", started.elapsed());
            reported_connect = true;
        }

        if reported_connect && started.elapsed() > RUN / 2 {
            let s = reader.snapshot();
            println!("\n  telemetry");
            println!("    reports decoded : {}", s.packets);
            println!("    frame interval  : {:.2} ms", s.frame_ms);
            let rate = if s.frame_ms > 0.0 {
                1000.0 / s.frame_ms
            } else {
                0.0
            };
            println!("    implied rate    : {rate:.0} Hz");
            println!(
                "    sticks          : L({:+.3}, {:+.3}) R({:+.3}, {:+.3})",
                s.report.left_x, s.report.left_y, s.report.right_x, s.report.right_y
            );
            println!(
                "    accelerometer   : {:.3} g",
                (s.report.accel.x.powi(2) + s.report.accel.y.powi(2) + s.report.accel.z.powi(2))
                    .sqrt()
            );
            println!(
                "    gyro            : ({:.1}, {:.1}, {:.1}) deg/s",
                s.report.gyro.yaw, s.report.gyro.pitch, s.report.gyro.roll
            );
            println!("    battery reads   : {}", reader.battery_reads());

            if s.packets < 100 {
                println!("\n  the reader is alive but almost nothing is arriving, which is");
                println!("  what a reader stuck on the wrong interface looks like");
                std::process::exit(1);
            }
            if s.frame_ms <= 0.0 || rate < 20.0 {
                println!("\n  reports arrive far too slowly to be a 1000 Hz pad");
                std::process::exit(1);
            }
            println!("\n  the reader is decoding live reports at a plausible rate");
            return;
        }
    }

    eprintln!("\nno reports arrived within {RUN:?}");
    eprintln!("the reader is still opening a handle, so the interface selection or the");
    eprintln!("report layout is still wrong");
    std::process::exit(1);
}
