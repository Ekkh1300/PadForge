//! Sends rumble through the real path and times how long it takes to come back.
//!
//! Rumble cannot be confirmed from software: the pad gives no acknowledgement,
//! and the output path has no read side for it. What can be measured is whether
//! the call is accepted and whether the driver stays usable afterwards, so this
//! reports those and asks the person running it to confirm the sensation.
//!
//! Run:
//!     cargo run -p padcore --bin probe-rumble --release --target x86_64-pc-windows-gnu

use std::thread::sleep;
use std::time::{Duration, Instant};

use padcore::device::DeviceReader;
use padcore::report::{self, Transport};

fn main() {
    let reader = match DeviceReader::start(None, 1000) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("could not start the reader: {e}");
            std::process::exit(1);
        }
    };

    println!("waiting for the pad...");
    let mut ready = false;
    for _ in 0..40 {
        sleep(Duration::from_millis(250));
        let s = reader.snapshot();
        if s.connected && s.packets > 5 {
            println!(
                "  {} via {}, {} reports",
                s.info.label(),
                s.info.transport.label(),
                s.packets
            );
            ready = true;
            break;
        }
    }
    if !ready {
        eprintln!("  the pad never reported");
        std::process::exit(1);
    }

    let transport = reader.snapshot().info.transport;

    println!("\nEach burst lasts 700 ms with a 500 ms gap. Four kinds:");
    println!("  heavy only, light only, both, then a fade.");
    println!("If you feel nothing at all, the output handle is not reaching the pad");
    println!("even though the pad is reading fine, which is a different fault.");

    let cases: [(&str, (u8, u8)); 4] = [
        ("heavy motor only", (255, 0)),
        ("light motor only", (0, 255)),
        ("both together", (255, 255)),
        ("half strength", (128, 128)),
    ];

    for (name, (heavy, light)) in cases {
        // A lightbar colour is sent alongside every rumble so the pad is
        // demonstrably still being written to at that moment. Some pads ignore a
        // rumble report that arrives with no colour change to piggyback on.
        let mut buf = report::output_report(transport, 0, 120, 255, Some((heavy, light)));
        // Turn the lightbar off first, so the rumble report is not identical to
        // the previous one and the driver has a reason to send it.
        let _ = reader.send_output(&report::output_report(transport, 0, 0, 0, None));
        sleep(Duration::from_millis(250));

        let started = Instant::now();
        let accepted = reader.send_output(&buf);
        let elapsed = started.elapsed();

        println!("\n  {name}");
        println!("    magnitude heavy={heavy} light={light}");
        println!("    accepted in {elapsed:?}  ({accepted})");
        println!("    -> feel it now, 700 ms");
        sleep(Duration::from_millis(700));

        // Stop, and confirm the pad is still reporting afterwards. A driver that
        // wedges on a rumble report stops the input side too, and that is the
        // failure this check exists to catch.
        let stopped = reader.send_output(&report::output_report(transport, 0, 0, 0, Some((0, 0))));
        sleep(Duration::from_millis(500));
        let after = reader.snapshot();
        println!(
            "    stop accepted={stopped}, pad still reporting: connected={} packets={}",
            after.connected, after.packets
        );

        buf[0] = 0;
    }

    println!("\n=== transport detail ===");
    println!("  report id for rumble is 0x05, the standard DS4 output report");
    println!(
        "  length {} bytes",
        match transport {
            Transport::Usb => 64,
            Transport::Bluetooth => 78,
        }
    );
    let frame_ms = reader.snapshot().frame_ms;
    println!("  the pad reports at {frame_ms:.2} ms per frame right now");
}
