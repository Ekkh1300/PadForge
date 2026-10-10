//! Whether the pad's motors answer a report we write, without a game.
//!
//! The path from a game to the DualShock's motors has two halves that fail
//! differently. The first, XInput to the bus, is the part a driver carries. The
//! second, our write to the pad, is the part only the pad can prove. This probe
//! is the second half on its own: it writes the same report the engine writes,
//! in the same order, and asks a person whether anything moved.
//!
//! So "the game's rumble does nothing" splits into "the game never reached us"
//! or "we never reached the pad", which are two different bugs in two different
//! places, and this is what tells them apart.

use std::time::Duration;

fn main() {
    println!("PadForge motor probe");
    println!("====================\n");

    // The pad has to be found the same way the app finds it, including the
    // interface probe: a Bluetooth DS4 exposes several HID collections and only
    // one of them accepts output reports.
    let Some(device) = open_pad() else {
        println!("No DualShock 4 is connected.");
        println!("Connect one over USB or Bluetooth, then run this again.");
        std::process::exit(1);
    };

    let transport = pad_is_bluetooth(&device);
    println!(
        "Pad is open over {}.",
        if transport == padcore::report::Transport::Bluetooth {
            "Bluetooth"
        } else {
            "USB"
        }
    );

    // Start from the same state the engine would leave the pad in: motors off,
    // lightbar at its own colour. A report that starts from a buzz would be
    // measuring nothing.
    println!("\n1. Motors off, lightbar unchanged...");
    write_report(&device, transport, 0, 0, 0, Some((0, 0)));
    std::thread::sleep(Duration::from_millis(150));

    println!("2. Left motor (low frequency) at full strength...");
    write_report(&device, transport, 0, 0, 0, Some((255, 0)));
    std::thread::sleep(Duration::from_millis(900));

    println!("3. Right motor (high frequency) at full strength...");
    write_report(&device, transport, 0, 0, 0, Some((0, 255)));
    std::thread::sleep(Duration::from_millis(900));

    println!("4. Both motors at full strength...");
    write_report(&device, transport, 0, 0, 0, Some((255, 255)));
    std::thread::sleep(Duration::from_millis(900));

    println!("5. Both motors off again...");
    write_report(&device, transport, 0, 0, 0, Some((0, 0)));

    println!("\nThe pad accepted every report.");
    println!("If you felt both steps 2 and 3, the motors and this pad's layout are");
    println!("both correct, and the remaining gap is upstream of the write.");
    println!("If you felt neither, the pad is ignoring our reports entirely.");
    std::process::exit(0);
}

/// Open the first pad that actually delivers reports.
///
/// Every Sony interface is tried in the same order the reader tries them, and
/// the first that stays quiet on a short read is skipped rather than blocking.
/// This is the same decision the app makes, so a probe that succeeds here and a
/// reader that fails to find a pad cannot both be right for different reasons.
fn open_pad() -> Option<hidapi::HidDevice> {
    let api = hidapi::HidApi::new().ok()?;
    let candidates: Vec<_> = api
        .device_list()
        .filter(|d| {
            d.vendor_id() == padcore::report::SONY_VENDOR_ID
                && padcore::report::DS4_PRODUCT_IDS.contains(&d.product_id())
        })
        .collect();

    for info in candidates {
        // `open_path` is on the api rather than the device info, and it takes the
        // path as a CString: the same call the reader makes, so the probe and the
        // app cannot disagree about which pad is reachable.
        let Ok(path) = std::ffi::CString::new(info.path().to_string_lossy().into_owned()) else {
            continue;
        };
        let Ok(device) = api.open_path(&path) else {
            continue;
        };
        let mut buf = [0u8; 128];
        // A collection that carries no gamepad reports opens fine and then says
        // nothing, so silence is the test rather than an error code.
        if device.read_timeout(&mut buf, 120).unwrap_or(0) > 0 {
            return Some(device);
        }
    }
    None
}

/// Whether this pad is on Bluetooth, from the width of what it sent.
///
/// A Bluetooth DS4 reports on 0x11 and a USB one on 0x01, and the two take
/// completely different output layouts. Sending a USB report to a Bluetooth pad
/// is rejected outright, which reads as "the pad refuses everything".
fn pad_is_bluetooth(device: &hidapi::HidDevice) -> padcore::report::Transport {
    let mut buf = [0u8; 128];
    match device.read_timeout(&mut buf, 150) {
        Ok(n) if n >= 78 => padcore::report::Transport::Bluetooth,
        _ => padcore::report::Transport::Usb,
    }
}

/// Write the report the engine would write, and say whether the pad took it.
///
/// The refusal is printed rather than swallowed, because it is the whole point
/// of the probe: a pad that turns the report down says so in the error, and
/// that is a different bug from a report that is quietly ignored.
fn write_report(
    device: &hidapi::HidDevice,
    transport: padcore::report::Transport,
    red: u8,
    green: u8,
    blue: u8,
    rumble: Option<(u8, u8)>,
) {
    let buf = padcore::report::output_report(transport, red, green, blue, rumble);
    match device.send_output_report(&buf) {
        Ok(()) => {}
        Err(e) => {
            println!("      the pad refused: {e}");
            std::process::exit(3);
        }
    }
}
