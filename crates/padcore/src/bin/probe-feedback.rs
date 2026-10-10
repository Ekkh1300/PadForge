//! Follows one vibration along the exact path the application uses.
//!
//! The application does not write rumble to the pad. It asks the virtual Xbox
//! 360 controller to vibrate and then waits for the driver to say what the game
//! asked for, which is the same thing a game does — so the value only ever
//! exists if every link holds:
//!
//!     XInputSetState -> ViGEmBus -> notification thread -> backend -> report
//!
//! Each link has already been proved on its own somewhere else, which is
//! precisely why this probe is worth having: a chain with every link working
//! and one missing link still reads as "the motors do not work", and the only
//! thing that separates those is watching the value travel.
//!
//! The virtual pad is driven, not just observed. A listener that attaches and
//! reads a vibration somebody else started proves nothing about the request
//! this process just made.
//!
//! Run:
//!     cargo run -p padcore --bin probe-feedback --release --target x86_64-pc-windows-gnu

use std::thread::sleep;
use std::time::{Duration, Instant};

use padcore::output::{OutputBackend, VigemBackend};
use padcore::xinput;

/// How long the request is held, long enough to feel and to sample repeatedly.
const HOLD: Duration = Duration::from_secs(3);

fn main() {
    // Same order as the app: the backend first, because the pad has to exist
    // before XInput has anything to report as connected.
    let mut backend = VigemBackend::connect();
    println!("virtual pad: {}", backend.name());
    if !backend.is_connected() {
        eprintln!("\nwithout a virtual pad there is nothing to vibrate, and that");
        eprintln!("is a different problem: install ViGEmBus.");
        std::process::exit(1);
    }

    // The app gives the device a moment before its first report, and the same
    // pause here keeps enumeration from racing the request.
    sleep(Duration::from_millis(600));

    let slots = xinput::connected_slots();
    println!("XInput reports slots {slots:?} as connected");
    if slots.is_empty() {
        eprintln!("\nWindows does not see the pad this process just plugged in,");
        eprintln!("so nothing will reach the driver and the motors stay silent");
        eprintln!("whatever the report says.");
        std::process::exit(2);
    }

    println!("\nvibrating for {HOLD:?} — hold the pad now");
    if let Err(e) = xinput::vibrate(&slots, 255, 255) {
        eprintln!("  XInputSetState failed: {e}");
        std::process::exit(3);
    }
    println!("  XInputSetState accepted");

    // Sample continuously: the notification carries a change, so a listener
    // that sleeps between reads can miss one entirely and report silence.
    let deadline = Instant::now() + HOLD;
    let mut seen: Option<(u8, u8)> = None;
    let mut samples = 0usize;
    while Instant::now() < deadline {
        if let Some(m) = backend.take_rumble() {
            seen = Some(m);
        }
        samples += 1;
        sleep(Duration::from_millis(5));
    }

    let _ = xinput::vibrate(&slots, 0, 0);

    println!("\n  {samples} samples taken");
    match seen {
        Some((heavy, fast)) => {
            println!("  feedback arrived: heavy={heavy} fast={fast}");
            if heavy == 0 && fast == 0 {
                println!("\n  The driver answered, but with both motors off. That is a");
                println!("  real answer: the value reaching the engine is zero, so the");
                println!("  report written to the pad asks for nothing and the motors");
                println!("  correctly stay still.");
                std::process::exit(4);
            }
            println!("\n  The value reached the engine. The app turns it into a report");
            println!("  and writes it, which the lightbar already proves works.");
            println!("  If the pad did not buzz during that window, the report is");
            println!("  being accepted and ignored, which points at the report itself.");
        }
        None => {
            println!("  no feedback ever arrived");
            println!("\n  The request left this process and nothing came back, so the");
            println!("  break is between XInput and the engine — not in the report and");
            println!("  not in the pad.");
            std::process::exit(5);
        }
    }
}
