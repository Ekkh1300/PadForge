//! Drives the real output backend and reports what a game would see.
//!
//! `send_output` returning true only proves Windows accepted a buffer. This runs
//! the same publishing path the application uses and then reads the state back,
//! so a virtual pad that failed to appear is visible as a failure rather than as
//! silence.
//!
//! Run:
//!     cargo run -p padcore --bin probe-vigem --release --target x86_64-pc-windows-gnu

use std::thread::sleep;
use std::time::Duration;

use padcore::output::{GamepadState, OutputBackend, VigemBackend, XButtons};

fn main() {
    println!("=== opening the virtual driver ===");
    let mut backend = VigemBackend::connect();
    println!("  opened: {}", backend.name());
    println!("  connected = {}", backend.is_connected());
    if !backend.is_connected() {
        eprintln!("\n  the virtual driver did not come up.");
        eprintln!("  Without it the pad is read but no game can see it, because");
        eprintln!("  there is nothing to publish to. ViGEmBus from");
        eprintln!("  https://github.com/nefarius/ViGEmBus/releases, then reboot.");
        std::process::exit(1);
    }

    println!("\n=== publishing states a game can read ===");
    println!("  Run a game or the Windows Game Controllers panel alongside this if");
    println!("  you want to watch them move; the numbers here only prove the path");
    println!("  accepted the data.");

    let states: [(&str, GamepadState); 5] = [
        ("neutral", GamepadState::neutral()),
        (
            "A held, right stick full right",
            state(|s| {
                s.buttons = XButtons::A;
                s.thumb_rx = 32767;
            }),
        ),
        (
            "both triggers fully pressed",
            state(|s| {
                s.left_trigger = 255;
                s.right_trigger = 255;
            }),
        ),
        (
            "D-pad up, right stick full down",
            state(|s| {
                s.buttons = XButtons::DPAD_UP;
                s.thumb_ry = -32767;
            }),
        ),
        ("neutral again", GamepadState::neutral()),
    ];

    for (name, s) in states {
        backend.submit(s);
        let readback = backend.last_state();
        // `buttons` is a raw mask rather than the newtype the mapping layer uses,
        // so the A button is a mask test rather than a method call.
        println!(
            "  {name:<34} buttons=0x{:04x} rx={} ry={} triggers={}/{}",
            readback.buttons,
            readback.thumb_rx,
            readback.thumb_ry,
            readback.left_trigger,
            readback.right_trigger
        );
        sleep(Duration::from_millis(900));
    }

    println!("\n=== teardown ===");
    backend.shutdown();
    println!("  shut down; the virtual pad should disappear from the device list");
}

fn state(f: impl FnOnce(&mut GamepadState)) -> GamepadState {
    let mut s = GamepadState::neutral();
    f(&mut s);
    s
}
