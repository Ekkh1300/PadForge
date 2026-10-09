//! Exercises the pad's output path on real hardware: lightbar colour and rumble.
//!
//! Rumble is the part that cannot be verified any other way. It is not a return
//! value — the pad gives no acknowledgement — so the only evidence that a rumble
//! arrived is the user feeling it, or the caller reporting it. This prints the
//! sequence clearly enough that the person running it can confirm each step, and
//! records whether the calls were accepted, which is a different and weaker
//! thing but still worth knowing.
//!
//! Run:
//!     cargo run -p padcore --bin probe-output --release --target x86_64-pc-windows-gnu
//!     (or with --guide to send a Guide press, which Windows owns)

use std::thread::sleep;
use std::time::Duration;

use padcore::device::DeviceReader;
use padcore::report::{self, Transport};

/// How long each rumble burst lasts.
const BURST: Duration = Duration::from_millis(600);

fn main() {
    let wants_guide = std::env::args().any(|a| a == "--guide");

    let reader = match DeviceReader::start(None, 1000) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("could not start the reader: {e}");
            std::process::exit(1);
        }
    };

    // Wait for reports, so the pad is known to be live before anything is sent.
    println!("waiting for the pad to report...");
    let mut connected = false;
    for _ in 0..40 {
        sleep(Duration::from_millis(250));
        let s = reader.snapshot();
        if s.connected && s.packets > 5 {
            connected = true;
            println!(
                "  {} via {}, {} reports decoded",
                s.info.label(),
                s.info.transport.label(),
                s.packets
            );
            break;
        }
    }
    if !connected {
        eprintln!("  the pad never reported, so there is nothing to drive");
        std::process::exit(1);
    }

    let transport = reader.snapshot().info.transport;
    let output_len = match transport {
        Transport::Usb => 64,
        Transport::Bluetooth => 78,
    };
    println!(
        "  output report is {output_len} bytes on {}",
        transport.label()
    );

    println!("\n=== lightbar ===");
    for (name, r, g, b) in [
        ("red", 255u8, 0u8, 0u8),
        ("green", 0, 255, 0),
        ("blue", 0, 0, 255),
        ("white", 255, 255, 255),
        ("off", 0, 0, 0),
    ] {
        let sent = send(&reader, report::output_report(transport, r, g, b, None));
        println!("  {name:<6} rgb({r:>3},{g:>3},{b:>3})  sent={sent}");
        sleep(Duration::from_millis(700));
    }

    println!("\n=== rumble ===");
    println!("  You should feel two distinct bursts, one heavy motor and one light.");
    println!("  If you feel nothing, that is the result, not a bug in this print.");
    for (name, heavy, light) in [("heavy (right)", 255u8, 0u8), ("light (left)", 0, 255)] {
        let sent = send(
            &reader,
            report::output_report(transport, 0, 0, 255, Some((heavy, light))),
        );
        println!("  {name:<14} magnitude {heavy:>3}/{light:>3}  sent={sent}");
        sleep(BURST);
        // Stop, then pause, so the two bursts are separated rather than merging.
        let stopped = send(
            &reader,
            report::output_report(transport, 0, 0, 0, Some((0, 0))),
        );
        println!("  stopped        sent={stopped}");
        sleep(Duration::from_millis(500));
    }

    if wants_guide {
        println!("\n=== Guide press (only if you were asked to test this) ===");
        println!("  Windows reserves this button, so expect the Game Bar to open");
        println!("  and take focus. That is the reason it is not bound by default.");
        // The Guide bit is set directly rather than through a profile mapping,
        // because no profile should carry this binding after the repair.
        let mut buf = report::output_report(transport, 0, 0, 0, None);
        // Button state lives at a fixed offset in the XInput report the pad
        // echoes; see output.rs for the shared layout.
        set_button_bit(&mut buf, 0x0400);
        let sent = send(&reader, buf);
        println!("  guide pressed, sent={sent}");
        sleep(Duration::from_secs(2));
        let released = send(&reader, report::output_report(transport, 0, 0, 0, None));
        println!("  guide released, sent={released}");
    }

    println!("\ndone. Anything printed as sent=false was refused by Windows, which usually");
    println!("means the output handle is not open rather than that the pad refused.");
}

fn send(reader: &DeviceReader, buf: Vec<u8>) -> bool {
    reader.send_output(&buf)
}

/// Set one button bit in the XInput portion of an output report.
///
/// The button word sits at the same offset the input report uses for its digital
/// cluster, which is what makes this round-trippable: the pad reflects its own
/// output state back in the next input report.
fn set_button_bit(buf: &mut [u8], bit: u16) {
    const BUTTON_OFFSET: usize = 9;
    let idx = BUTTON_OFFSET + (bit as usize / 8);
    if idx < buf.len() {
        buf[idx] |= 1 << (bit % 8);
    }
}
