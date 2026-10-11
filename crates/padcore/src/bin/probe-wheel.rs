//! Shows what the generic HID parser believes about a wheel — or about the
//! DualShock, which is the more useful thing to point it at first.
//!
//! Nothing about a steering wheel's report layout is standard. The same model
//! differs between its USB and Bluetooth firmware, an axis is eight bits in one
//! report and sixteen in the next, and no table of known wheels survives that.
//! So this crate does not guess at bytes: it asks Windows, through
//! `HidP_GetUsageValue` and its neighbours, the same machinery a game would use
//! to read the wheel itself.
//!
//! That moves the question from "are the bytes being decoded correctly" to "is
//! the parser being asked correctly", and the second question is answerable
//! without a wheel. Run this against the DualShock: the parser should report an
//! X axis declared 0..=255, and moving the left stick should take it from 0 at
//! left lock to 255 at right lock. If it does, the parser reads reports
//! correctly, and a wheel is only a different layout of the same reports.
//!
//! With a wheel attached, the same run shows which axis each pedal reports on,
//! which is the number a binding needs and the only thing worth looking up by
//! hand.
//!
//! Run:
//!     cargo run -p padcore --bin probe-wheel --release --target x86_64-pc-windows-gnu
//!
//! Pass an index to open one and stream it:
//!     cargo run -p padcore --bin probe-wheel --release --target x86_64-pc-windows-gnu -- 3
//!
//! And `--all` to list every HID interface rather than only the candidates,
//! which is what puts the DualShock on the list.

use std::time::{Duration, Instant};

use padcore::wheel::{self, ControlKind, WheelDevice};

/// Sony's vendor id, and the product ids a DualShock 4 uses.
const SONY: u16 = 0x054C;
const DS4_PIDS: [u16; 3] = [0x05C4, 0x09CC, 0x0BA0];

/// How long a stream runs before it reports a control that never moved.
const STREAM: Duration = Duration::from_secs(6);

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let want_all = args.iter().any(|a| a == "--all");
    // `--` is cargo's own separator between the binary's arguments and ours,
    // so it has to be stepped over rather than mistaken for a device index.
    let target = args
        .iter()
        .find(|a| *a != "--all" && **a != "--")
        .and_then(|a| a.parse::<usize>().ok());

    let candidates = if want_all {
        every_hid()
    } else {
        wheel::devices()
    };

    if candidates.is_empty() {
        println!(
            "no candidate devices.\n\n{}",
            if want_all {
                "the HID layer reported nothing at all, which points at the runtime."
            } else {
                "run with --all to list every HID interface; a wheel reports itself as a\n\
                 joystick (usage 04), game pad (05) or multi-axis controller (08), and\n\
                 anything that does not will not appear on this list."
            }
        );
        return;
    }

    println!("{} candidate device(s)\n", candidates.len());
    for (index, device) in candidates.iter().enumerate() {
        println!(
            "  [{index}] {:>9}  usage {:02x}  '{}'",
            device.ids(),
            device.usage,
            device.label
        );
    }

    let Some(index) = target else {
        println!(
            "\nrun again with an index to open one and watch it move, e.g. `-- 0`.\n\
             point this at the DualShock with --all to check the parser on hardware\n\
             this app already knows how to read."
        );
        return;
    };

    let Some(device) = candidates.get(index) else {
        println!("\nno device at index {index}");
        return;
    };
    stream(device);
}

/// Every HID interface the layer will enumerate, including the ones the picker
/// deliberately leaves out.
fn every_hid() -> Vec<WheelDevice> {
    let Ok(api) = hidapi::HidApi::new() else {
        return Vec::new();
    };
    api.device_list()
        .filter(|d| d.usage_page() == 0x01)
        .map(|d| WheelDevice {
            path: d.path().to_string_lossy().into_owned(),
            label: d
                .product_string()
                .filter(|s| !s.trim().is_empty())
                .or_else(|| d.manufacturer_string().filter(|s| !s.trim().is_empty()))
                .unwrap_or("unnamed")
                .to_string(),
            vendor_id: d.vendor_id(),
            product_id: d.product_id(),
            usage: d.usage(),
        })
        .collect()
}

/// Open one device, print the layout the driver declared, then stream it.
fn stream(device: &WheelDevice) {
    println!("\nopening {} '{}' ...", device.ids(), device.label);

    let mut reader = match wheel::WheelReader::open(device) {
        Ok(reader) => reader,
        Err(err) => {
            println!("  refused: {err}");
            if device.vendor_id == SONY && DS4_PIDS.contains(&device.product_id) {
                println!(
                    "\n  the pad is already open by PadForge or by the game. A DualShock hands\n\
                     out only one interface, and this is only worth doing to check the parser."
                );
            }
            return;
        }
    };

    let controls = reader.controls().to_vec();
    println!(
        "\n{} control(s), as the driver describes them\n",
        controls.len()
    );
    println!(
        "  {:<14} {:<12} {:<24} kind",
        "label", "usage", "declared range"
    );
    println!("  {}", "-".repeat(60));
    for control in &controls {
        let range = match control.kind {
            ControlKind::Axis { min, max, bits } => {
                format!("{min} ..= {max}  ({bits}-bit)")
            }
            ControlKind::Button => "0 or 1".to_string(),
        };
        println!(
            "  {:<14} {:<12} {:<24} {}",
            control.label,
            format!("{:02x}:{:02x}", control.id.page, control.id.usage),
            range,
            if control.is_axis() { "axis" } else { "button" }
        );
    }

    // The check worth doing before trusting any of this on a wheel: an axis the
    // driver declared as spanning the whole of `i32` is a driver that is not
    // describing anything usable, and it would normalise every report to zero.
    for control in &controls {
        if let Some((min, max)) = control.range() {
            if max <= min {
                println!(
                    "\n  '{}' is declared {} ..= {} and has no travel to report. That is a\n\
                     driver problem, not a parsing one.",
                    control.label, min, max
                );
            }
        }
    }

    println!(
        "\nmove something. streaming for {}s ...\n",
        STREAM.as_secs()
    );
    let started = Instant::now();
    let mut frames = 0u32;
    while started.elapsed() < STREAM {
        match reader.poll() {
            Ok(Some(values)) => {
                frames += 1;
                print_frame(&controls, &values);
            }
            Ok(None) => {}
            Err(err) => {
                println!("  read refused: {err}");
                return;
            }
        }
        // A wheel reports on its own schedule and the loop is only here to keep
        // the screen from filling faster than it can be read.
        std::thread::sleep(Duration::from_millis(8));
    }

    println!("\n{} frame(s) in {}s", frames, STREAM.as_secs());
    if frames == 0 {
        println!(
            "nothing was reported. Either the interface is quiet — some wheels only\n\
             report once a control has moved — or the report is not reaching the parser."
        );
    }
}

/// One line per frame, with every control that moved.
///
/// Buttons are shown as `#` and axes as a bar, because the question this
/// answers is "is this the control I am looking for", and that is answered by
/// watching something move rather than by reading a number.
fn print_frame(controls: &[padcore::wheel::Control], values: &[(padcore::wheel::ControlId, f32)]) {
    let mut line = String::new();
    for control in controls {
        let Some((_, value)) = values.iter().find(|(id, _)| *id == control.id) else {
            continue;
        };
        if !control.is_axis() {
            if *value > 0.5 {
                line.push_str(&format!("  {}=#", control.label));
            }
            continue;
        }
        if *value < 0.002 || *value > 0.998 {
            // Rest and full travel both look identical on every wheel, so
            // showing them would fill the screen with bars that say nothing.
            // A control in between is the one being moved.
            if *value > 0.998 {
                line.push_str(&format!("  {}=FULL", control.label));
            }
            continue;
        }
        let filled = (*value * 24.0).round() as usize;
        line.push_str(&format!(
            "  {}=[{}{}] {:.2}",
            control.label,
            "#".repeat(filled),
            "-".repeat(24 - filled),
            value
        ));
    }
    if !line.is_empty() {
        println!("{line}");
    }
}
