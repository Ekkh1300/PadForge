//! Reads a real report from the pad and shows what gyro-to-mouse aiming would do.
//!
//! The point is the magnitude. The report layout had the gyro and accelerometer
//! the wrong way round, which produced gyro readings hundreds of times too large.
//! Fed through the pointer code that is not a subtle aiming error, it is the
//! mouse being driven across the screen by a pad lying still on a desk.
//!
//! Every matching interface is tried, because a Bluetooth DS4 exposes several
//! and only some of them report. A probe that took the first one would measure
//! the selection bug rather than the pointer maths.
//!
//! Run:
//!     cargo run -p padcore --bin probe-pointer --release --target x86_64-pc-windows-gnu

use std::time::{Duration, Instant};

use padcore::device::probe_matching_devices;
use padcore::pointer::GyroPointer;
use padcore::report::{self, Gyro};

/// The report rate a DS4 uses over Bluetooth, in seconds.
const DT: f32 = 0.001;

/// How long to accumulate readings for.
const WINDOW: Duration = Duration::from_secs(2);

fn main() {
    let devices = probe_matching_devices();
    if devices.is_empty() {
        eprintln!("no DS4 is connected, so there is nothing to probe");
        std::process::exit(1);
    }

    let api = match hidapi::HidApi::new() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("could not initialise the HID API: {e}");
            std::process::exit(1);
        }
    };

    let mut any = false;

    for info in &devices {
        let Ok(path) = std::ffi::CString::new(info.path.as_str()) else {
            continue;
        };
        let Ok(device) = api.open_path(&path) else {
            continue;
        };
        // Rebound as mutable so ead_timeout can take &mut self.

        println!(
            "=== {} on {}, interface {} ===",
            info.product, info.bus_type, info.interface_number
        );

        let mut buffer = [0u8; 256];
        let mut pointer = GyroPointer::new(Default::default());
        let started = Instant::now();
        let mut frames = 0u64;
        let mut total = (0i64, 0i64);
        let mut worst = (0i64, 0i64);
        let mut worst_reading = Gyro::default();
        let mut accel_magnitude = 0.0f32;

        while started.elapsed() < WINDOW {
            // A timeout, because a silent interface never returns from a blocking
            // read and that would stall the probe exactly as it stalls the app.
            let Ok(n) = device.read_timeout(&mut buffer, 50) else {
                break;
            };
            if n == 0 {
                continue;
            }
            let Some(frame) = report::parse(&buffer[..n]) else {
                continue;
            };
            frames += 1;
            accel_magnitude =
                (frame.accel.x.powi(2) + frame.accel.y.powi(2) + frame.accel.z.powi(2)).sqrt();

            if frame.gyro.yaw.abs() > worst_reading.yaw.abs()
                || frame.gyro.pitch.abs() > worst_reading.pitch.abs()
            {
                worst_reading = frame.gyro;
            }

            let d = pointer.process(&frame.gyro, DT);
            total.0 += d.0 as i64;
            total.1 += d.1 as i64;
            worst.0 = worst.0.max(d.0 as i64);
            worst.1 = worst.1.max(d.1 as i64);
        }

        if frames == 0 {
            println!("  silent: no frames in {WINDOW:?}, so this is not the reporting interface");
            println!("  the reader has to step past one like this, or it blocks here forever");
            continue;
        }

        any = true;
        println!("  {frames} frames in {WINDOW:?}");
        println!(
            "  worst gyro reading : yaw {:+.1}, pitch {:+.1} deg/s",
            worst_reading.yaw, worst_reading.pitch
        );
        println!("  accelerometer      : {accel_magnitude:.3} g (a still pad reads about 1 g)");
        println!("  pointer drift      : ({}, {}) px", total.0, total.1);
        println!("  worst single step  : ({}, {}) px", worst.0, worst.1);

        let drift = total.0.abs() + total.1.abs();
        if drift > 200 {
            println!("\n  A pad resting on a desk moved the pointer {drift} px.");
            println!("  That is a mouse moving on its own, and it is what a wrong gyro");
            println!("  offset looks like from the user's side.");
        } else {
            println!("\n  The pointer stayed put, which is what a still pad should do.");
        }
    }

    if !any {
        eprintln!("\nno interface delivered a single frame");
        std::process::exit(1);
    }
}
