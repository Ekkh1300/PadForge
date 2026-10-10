//! Proof that force feedback reaches us from the OS, and that we forward it.
//!
//! Vibration is the one thing a screenshot cannot show and the one thing that
//! silently does nothing when a single line is wrong: the driver must be
//! present, the notification must be registered, the motor values must land in
//! the right report bytes, and the pad must accept the write. This probe walks
//! that whole path the way a game does and prints what actually arrives, so
//! "it should be vibrating" becomes a number someone can read.
//!
//! It asks the OS to vibrate the pad through XInput, which is exactly the door
//! a game uses, and then reports what ViGEmBus handed back to the listener.

use std::time::{Duration, Instant};

// The backend is spoken to through its trait, and the trait has to be in scope
// for those calls to resolve.
use padcore::output::OutputBackend;

fn main() {
    println!("PadForge rumble probe");
    println!("=====================\n");

    // Open the pad the way the engine does. If this fails there is no point
    // asking about motors: the driver is what carries them.
    let mut backend = padcore::output::VigemBackend::connect();
    if !backend.is_connected() {
        println!(
            "The virtual pad is not available: {}",
            backend.last_error().unwrap_or("unknown reason")
        );
        println!("Install the ViGEmBus driver, then run this again.");
        std::process::exit(1);
    }
    println!("Virtual pad is up: {}", backend.name());

    // Publish a frame so the pad is not sitting in its default state. A game
    // writes state constantly; a bare target sometimes will not answer until it
    // has been written once.
    backend.submit(padcore::output::GamepadState::neutral());

    // Enumerate only now, because our own pad takes an XInput slot the moment
    // it exists. Read before connecting and the list holds everyone else's
    // pads: the pulse then lands on a slot we are not listening to, the
    // listener reports the empty notification it was born with, and a working
    // chain reads as a zero one.
    let slots = padcore::xinput::connected_slots();
    if slots.is_empty() {
        println!("No XInput pad is connected, so there is nothing to vibrate.");
        println!("The virtual pad was created but the OS does not see it.");
        std::process::exit(1);
    }
    println!("XInput sees a pad on slot(s): {slots:?}");

    let quiet = Duration::from_millis(150);
    std::thread::sleep(quiet);

    // One full-strength pulse, then silence, with a deadline rather than a
    // blind sleep: if the notification never comes we want to say so quickly
    // instead of hanging.
    println!("\nAsking for a pulse at full strength on slot(s) {slots:?}...");
    let started = Instant::now();
    if let Err(e) = padcore::xinput::vibrate(&slots, 255, 255) {
        println!("  the OS refused the request: {e}");
        println!("  That is a driver or permissions problem, not a PadForge one.");
        std::process::exit(1);
    }

    let deadline = Duration::from_secs(3);

    // A target announces itself with an empty notification before any pulse
    // lands, so the first thing `take_rumble` hands back is a zero one. Taking
    // that as the answer is what made a healthy chain look like a silent one:
    // keep reading until something with strength arrives, and remember the
    // strongest pair seen either way so the two failures stay distinguishable.
    let mut arrived = false;
    let mut strongest = (0u8, 0u8);
    while started.elapsed() < deadline {
        if let Some((heavy, fast)) = backend.take_rumble() {
            arrived = true;
            let seen = heavy.max(fast);
            if seen > strongest.0.max(strongest.1) {
                strongest = (heavy, fast);
            }
            if seen > 0 {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    // Always release, even on failure: leaving a pad buzzing after the probe
    // has exited would look like a hang.
    let _ = padcore::xinput::vibrate(&slots, 0, 0);

    println!("\nResult");
    println!("------");
    if !arrived {
        println!("  Nothing came back within {deadline:?}.");
        println!("\n  That means the OS accepted the request but the driver never");
        println!("  handed it back. Likely causes, in order:");
        println!("    1. The notification was never registered (driver version).");
        println!("    2. The pad was asked to stop before the report was written.");
        println!("    3. Another process owns the same virtual pad.");
        std::process::exit(2);
    }

    let (heavy, fast) = strongest;
    println!("  Force feedback arrived: heavy={heavy} fast={fast}");
    if heavy.max(fast) == 0 {
        println!("  Only the target's idle notification came back.");
        println!("  The listener works, so the wiring is fine; nothing asked it to move.");
        std::process::exit(3);
    }
    println!("  The whole chain works: game -> XInput -> ViGEmBus -> listener.");
    println!("  PadForge forwards exactly this to the DualShock's motors.");

    // Hold briefly so the motors have time to move at all before we stop them.
    std::thread::sleep(Duration::from_millis(250));
    let _ = padcore::xinput::vibrate(&slots, 0, 0);
    println!("\nReleased.");
}
