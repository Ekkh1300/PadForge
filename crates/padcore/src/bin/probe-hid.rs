//! Prints what the HID layer sees, with the fields PadForge filters on.
//!
//! The application filters on vendor id and usage, so a pad the HID layer will
//! not enumerate can never appear however the filter is written. Running the
//! same enumeration the engine uses separates "the filter is wrong" from "the
//! device is not being enumerated at all", which have nothing in common as fixes.
//!
//! Run:
//!     cargo run -p padcore --bin probe-hid --release --target x86_64-pc-windows-gnu

use padcore::device::{probe_all_devices, probe_matching_devices};

/// Sony's vendor id.
const SONY: u16 = 0x054C;
/// The product ids the DS4 uses: v1 over USB, v2 over Bluetooth.
const DS4_PIDS: (u16, u16) = (0x05C4, 0x09CC);

fn main() {
    let all = probe_all_devices();
    println!("the HID layer reports {} device(s) in total\n", all.len());

    if all.is_empty() {
        println!("none at all, which points at the runtime rather than at any filter.");
        return;
    }

    for d in all.iter().take(15) {
        println!(
            "  vid={:04x} pid={:04x} iface={} usagePage={:04x} usage={:04x} '{}'",
            d.vendor_id, d.product_id, d.interface_number, d.usage_page, d.usage, d.product
        );
    }
    if all.len() > 15 {
        println!("  ... and {} more", all.len() - 15);
    }

    let sony: Vec<_> = all.iter().filter(|d| d.vendor_id == SONY).collect();
    println!("\nSony devices: {}", sony.len());

    for d in &sony {
        let kind = if d.product_id == DS4_PIDS.0 || d.product_id == DS4_PIDS.1 {
            "DS4 pid"
        } else {
            "other   "
        };
        let iface = if d.usage_page == 0x01 && d.usage == 0x05 {
            "gamepad"
        } else if d.usage_page == 0x01 && d.usage == 0x12 {
            "pointer "
        } else if d.usage_page == 0x01 && d.usage == 0x09 {
            "keyboard"
        } else if d.usage_page == 0xFF {
            "vendor  "
        } else {
            "other   "
        };
        println!(
            "  {kind} {iface} pid={:04x} iface={} usagePage={:04x} usage={:04x}",
            d.product_id, d.interface_number, d.usage_page, d.usage
        );
        println!("      product      : {}", d.product);
        println!("      manufacturer : {}", d.manufacturer);
        println!("      serial       : {}", d.serial);
        println!("      path         : {}", d.path);
    }

    let matching = probe_matching_devices();
    println!("\n--- what PadForge would connect to ---");
    println!("{} device(s) pass the full filter", matching.len());
    for d in &matching {
        println!("  {}  {}", d.product, d.path);
    }

    if matching.is_empty() {
        println!("\nnothing passed. Two different problems look identical here:");
        let sony_any = !sony.is_empty();
        let sony_ds4 = sony
            .iter()
            .any(|d| d.product_id == DS4_PIDS.0 || d.product_id == DS4_PIDS.1);
        if !sony_any {
            println!("  The pad is not being enumerated at all. Windows can see it in");
            println!("  Device Manager, but the generic HID layer does not, which means the");
            println!("  interface has no usable driver binding. That is fixed in Windows:");
            println!("    1. Settings > Bluetooth & devices: remove the Wireless Controller");
            println!("    2. Pair it again with the SHARE + PS button held down");
            println!("    3. Wait for it to appear under Human Interface Devices, not just");
            println!("       under Bluetooth");
            println!("  A USB cable is worth trying as well: it proves whether the");
            println!("  application itself is correct, independent of the Bluetooth pairing.");
        } else if !sony_ds4 {
            println!("  A Sony device is present but with a different product id, so the");
            println!("  filter rejects it. The expected ids are 05C4 and 09CC.");
        } else {
            println!("  The DS4 is enumerated, but no interface passes the usage filter.");
            println!("  The engine wants usage page 0x01 usage 0x05, which is the gamepad");
            println!("  collection. The listing above shows which interfaces it did find.");
        }
    }
}
