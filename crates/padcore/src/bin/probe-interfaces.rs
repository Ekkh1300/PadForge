//! Reads from every matching interface at once, so a silent one cannot hide a
//! working one.
//!
//! A DS4 over Bluetooth enumerates as more than one HID collection, and only one
//! of them carries reports. Opening the others is legal, they simply never
//! deliver, and a blocking read on such a handle never returns. That is what
//! makes this a probe worth having: a single-interface test can report "the pad
//! is silent" when the truth is "the pad is on interface 2 and the code opened
//! interface 1".
//!
//! Run:
//!     cargo run -p padcore --bin probe-interfaces --release --target x86_64-pc-windows-gnu

use std::sync::mpsc;
use std::time::{Duration, Instant};

use padcore::device::probe_matching_devices;
use padcore::report;

/// How long one interface gets before it is declared silent.
const PER_INTERFACE: Duration = Duration::from_secs(3);
/// How many decoded frames count as "working".
const ENOUGH: usize = 20;

fn main() {
    let devices = probe_matching_devices();
    if devices.is_empty() {
        eprintln!("no DS4 passed the filter");
        std::process::exit(1);
    }

    let api = match hidapi::HidApi::new() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("could not initialise the HID API: {e}");
            std::process::exit(1);
        }
    };

    // Each interface gets its own thread, so a handle that blocks forever costs
    // one timeout rather than the whole run.
    let mut results = Vec::new();

    for (index, info) in devices.iter().enumerate() {
        let path = info.path.clone();
        let label = format!(
            "iface {} serial={} path={}",
            info.interface_number,
            if info.serial.is_empty() {
                "(none)"
            } else {
                &info.serial
            },
            short(&info.path)
        );

        let Ok(c_path) = std::ffi::CString::new(path.as_str()) else {
            continue;
        };

        match api.open_path(&c_path) {
            Ok(device) => {
                let (tx, rx) = mpsc::channel();
                let deadline = Instant::now() + PER_INTERFACE;
                std::thread::spawn(move || {
                    let mut buffer = [0u8; 256];
                    let mut decoded = 0usize;
                    let mut total = 0usize;
                    let mut first: Option<Vec<u8>> = None;
                    let mut last: Option<padcore::report::Ds4Report> = None;

                    while Instant::now() < deadline && decoded < ENOUGH {
                        match device.read(&mut buffer) {
                            Ok(n) if n > 0 => {
                                total += 1;
                                if let Some(d) = report::parse(&buffer[..n]) {
                                    decoded += 1;
                                    if first.is_none() {
                                        first = Some(buffer[..n].to_vec());
                                    }
                                    last = Some(d);
                                }
                            }
                            Ok(_) => {}
                            Err(e) => {
                                let _ = tx.send(Outcome::Error(e.to_string()));
                                return;
                            }
                        }
                    }
                    let _ = tx.send(Outcome::Done {
                        decoded,
                        total,
                        first,
                        last,
                    });
                });

                match rx.recv_timeout(PER_INTERFACE + Duration::from_secs(2)) {
                    Ok(outcome) => results.push((index, label, outcome)),
                    Err(_) => results.push((index, label, Outcome::Blocked)),
                }
            }
            Err(e) => results.push((index, label, Outcome::Error(format!("open: {e}")))),
        }
    }

    let mut working = 0;
    for (index, label, outcome) in &results {
        println!("=== interface {index} ===");
        println!("  {label}");
        match outcome {
            Outcome::Blocked => {
                println!("  BLOCKED: read() never returned, so no report ever arrives");
                println!("  Opening this interface and then waiting on it stalls whatever");
                println!("  thread does so, which is a bug in the reader, not the pad.");
            }
            Outcome::Error(e) => println!("  error: {e}"),
            Outcome::Done {
                decoded,
                total,
                first,
                last,
            } => {
                if *decoded == 0 {
                    println!("  silent: {total} reads, none decoded as a DS4 report");
                    if let Some(f) = first {
                        println!("  first read was {} bytes starting 0x{:02x}", f.len(), f[0]);
                    }
                } else {
                    working += 1;
                    println!("  WORKS: {decoded} of {total} reads decoded");
                    if let Some(d) = last {
                        println!(
                            "  sticks L({:+.3}, {:+.3}) R({:+.3}, {:+.3})",
                            d.left_x, d.left_y, d.right_x, d.right_y
                        );
                        let mag =
                            (d.accel.x.powi(2) + d.accel.y.powi(2) + d.accel.z.powi(2)).sqrt();
                        println!(
                            "  accel {:.3} g, gyro ({:.1}, {:.1}, {:.1}) deg/s",
                            mag, d.gyro.yaw, d.gyro.pitch, d.gyro.roll
                        );
                    }
                }
            }
        }
    }

    println!(
        "\n{} of {} interfaces deliver reports",
        working,
        results.len()
    );
    if working == 0 {
        println!("  no interface works, so this is not a selection problem");
    } else if working < results.len() {
        println!("  picking a working interface rather than the first one is what keeps");
        println!("  the reader from blocking on a silent handle forever");
    } else {
        println!("  every interface works, so the bug is elsewhere");
    }
}

fn short(p: &str) -> String {
    if p.chars().count() <= 70 {
        return p.to_string();
    }
    let tail: String = p.chars().skip(p.chars().count() - 50).collect();
    format!("...{tail}")
}

enum Outcome {
    Done {
        decoded: usize,
        total: usize,
        first: Option<Vec<u8>>,
        last: Option<padcore::report::Ds4Report>,
    },
    Error(String),
    Blocked,
}
