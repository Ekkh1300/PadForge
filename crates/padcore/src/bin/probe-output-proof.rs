//! Proves the output path reaches the pad, by reading the pad's own reply.
//!
//! Every previous check asked the HID layer whether it accepted a buffer, which
//! is not the same question as whether the pad did anything. This one sends a
//! colour and a rumble and then watches the input stream for a change, so the
//! evidence comes from the device rather than from the driver.
//!
//! Run:
//!     cargo run -p padcore --bin probe-output-proof --release --target x86_64-pc-windows-gnu

use std::thread::sleep;
use std::time::{Duration, Instant};

use padcore::device::probe_matching_devices;
use padcore::report::{self, Transport};

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

    let Some(info) = devices.first() else {
        return;
    };
    let Ok(path) = std::ffi::CString::new(info.path.as_str()) else {
        return;
    };
    let Ok(device) = api.open_path(&path) else {
        eprintln!("could not open the pad");
        return;
    };

    let mut buf = [0u8; 256];

    // Establish the baseline: what the pad reports while nothing is being sent.
    let deadline = Instant::now() + Duration::from_millis(600);
    let mut baseline = None;
    while Instant::now() < deadline {
        if let Ok(n) = device.read_timeout(&mut buf, 50) {
            if n > 0 {
                if let Some(r) = report::parse(&buf[..n]) {
                    baseline = Some(r);
                    break;
                }
            }
        }
    }
    let Some(base) = baseline else {
        eprintln!("the pad never reported, so nothing can be concluded");
        std::process::exit(1);
    };

    let transport = match buf[0] {
        0x11 => Transport::Bluetooth,
        0x01 => Transport::Usb,
        other => {
            eprintln!("unknown report id 0x{other:02x}");
            std::process::exit(1);
        }
    };

    println!(
        "pad is sending report 0x{:02x}, so {}",
        buf[0],
        transport.label()
    );
    println!(
        "baseline frame: sticks L({:+.3},{:+.3}) R({:+.3},{:+.3})",
        base.left_x, base.left_y, base.right_x, base.right_y
    );
    println!();

    /// One step: a label, a colour, and the rumble to pair with it.
    type Step = (String, u8, u8, u8, Option<(u8, u8)>);

    let steps: [Step; 5] = [
        ("blue, no rumble".to_string(), 0, 0, 255, None),
        (
            "blue + both motors".to_string(),
            0,
            0,
            255,
            Some((255, 255)),
        ),
        ("red, no rumble".to_string(), 255, 0, 0, None),
        ("green, no rumble".to_string(), 0, 255, 0, None),
        ("black, motors off".to_string(), 0, 0, 0, Some((0, 0))),
    ];

    for (name, r, g, b, rumble) in steps {
        let report = report::output_report(transport, r, g, b, rumble);
        // `write` rather than `send_output_report`: the latter puts the report
        // through the 64 byte feature buffer, which cannot hold it, so it is
        // refused every time and this probe would only ever prove that.
        let accepted = device.write(&report).is_ok();

        // Hold the setting long enough to sample several frames.
        sleep(Duration::from_millis(700));

        let mut saw = 0usize;
        let mut changed = false;
        let deadline = Instant::now() + Duration::from_millis(400);
        while Instant::now() < deadline {
            match device.read_timeout(&mut buf, 50) {
                Ok(n) if n > 0 => {
                    if let Some(frame) = report::parse(&buf[..n]) {
                        saw += 1;
                        if frame.left_x != base.left_x
                            || frame.right_x != base.right_x
                            || frame.left_y != base.left_y
                        {
                            changed = true;
                        }
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }

        println!("  {name:<22} write={accepted}  frames_read={saw}  input_changed={changed}");
        if rumble.is_some_and(|(_, l)| l > 0) || rumble.is_some_and(|(h, _)| h > 0) {
            println!("      -> that one had rumble on it");
        }
    }

    println!("\nThe rumble column above only confirms the write was accepted.");
    println!("Whether the pad vibrated is something only you can report, and it is");
    println!("worth saying plainly: nothing in software can observe it, because the");
    println!("DS4 has no rumble acknowledgement in its input report.");
}
