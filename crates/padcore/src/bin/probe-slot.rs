//! Asks the virtual pad to move to a chosen XInput slot, and reports where it
//! actually landed.
//!
//! The driver is the one that decides, so a request and its answer are two
//! different things: a slot that is already taken leaves the pad somewhere else,
//! and showing the request instead of the answer would be a report of a pad
//! that is not there. This writes the setting the app writes and then reads the
//! slot back the same way the app does, which is the only way to tell a move
//! that worked from one that quietly did not.
//!
//! Run:
//!     cargo run -p padcore --bin probe-slot --release --target x86_64-pc-windows-gnu -- 2
//!
//! The argument is the player number to ask for, 1 to 4. Omit it to take
//! whichever slot is free.

use std::time::{Duration, Instant};

use padcore::output::{OutputBackend, VigemBackend};

/// How long to wait for the driver to name the slot.
const SLOT_WAIT: Duration = Duration::from_millis(1500);

fn main() {
    let wanted = std::env::args()
        .nth(1)
        .and_then(|a| a.parse::<usize>().ok())
        .map(|n| n.saturating_sub(1));

    match wanted {
        Some(i) if i >= 4 => {
            eprintln!("XInput has slots 1 to 4; {i} is not one of them");
            std::process::exit(2);
        }
        Some(i) => println!("asking for Player {}", i + 1),
        None => println!("asking for whichever slot is free"),
    }

    // Same call the engine makes, so a difference here is a difference in the
    // app rather than in something this probe invented.
    let backend = VigemBackend::connect_with_slot(wanted);

    if !backend.is_connected() {
        eprintln!(
            "no virtual pad: {}",
            backend.last_error().unwrap_or("the driver did not say why")
        );
        std::process::exit(1);
    }

    // The slot is read back after the pad is up, which is where the app reads
    // it from too.
    let deadline = Instant::now() + SLOT_WAIT;
    let mut slot = backend.slot();
    while slot.is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
        slot = backend.slot();
    }

    match (wanted, slot) {
        (_, Some(s)) => {
            println!("landed on Player {}", s + 1);
            match wanted {
                // Zero-based: a request for player 2 is slot index 1.
                Some(w) if w as u32 == s => println!("the request was honoured"),
                Some(w) => println!(
                    "the request for Player {} was not, so the pad is Player {}",
                    w + 1,
                    s + 1
                ),
                None => {}
            }
        }
        (w, None) => {
            println!("the driver did not report a slot");
            if let Some(w) = w {
                println!("so the request for Player {} cannot be confirmed", w + 1);
            }
            std::process::exit(3);
        }
    }
}
