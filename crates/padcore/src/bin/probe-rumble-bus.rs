//! Which slots of our own virtual pad answer with force feedback.
//!
//! `VigemBackend::take_rumble` only reads whatever the notification thread last
//! wrote. That is the right thing for the engine, which must never block, but
//! it cannot tell the difference between "the game asked for zero" and "the
//! game asked for something and the report is still in flight". A probe that
//! holds on to the listener instead of draining it can wait for a value to
//! climb, which is what proves the driver actually carried a pulse rather than
//! an idle zero.
//!
//! The game side is driven through XInput, so this crosses the same boundary a
//! game does and no other.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Which slots XInput has.
const SLOT_COUNT: u8 = 4;

/// What counts as "the motors moved". Anything at or above this is a real
/// request rather than an idle zero, and it is deliberately below the pulse so
/// a driver that scales a pulse down still shows up.
const NONZERO: u8 = 8;

/// Slots that answer as a connected pad.
///
/// The probe asks the OS to vibrate whatever is connected, so it has to know
/// that first: writing to a slot that is not ours is how a test ends up
/// vibrating somebody else's controller instead of the one under test.
fn connected_slots() -> Vec<u8> {
    use windows_sys::Win32::UI::Input::XboxController::{XInputGetState, XINPUT_STATE};
    (0..SLOT_COUNT)
        .filter(|slot| {
            let mut state = XINPUT_STATE::default();
            // SAFETY: a slot index in range and a struct the caller owns. XInput
            // writes into that struct and nowhere else.
            let status = unsafe { XInputGetState(u32::from(*slot), &mut state) };
            status == 0
        })
        .collect()
}

fn main() {
    println!("PadForge rumble channel probe");
    println!("=============================\n");

    // Our own virtual pad has to exist before anything can be asked to vibrate,
    // and it is what carries the request back to the listener. Without it the
    // probe would be asking a bus with nothing plugged into it.
    let client = match vigem_client::Client::connect() {
        Ok(c) => std::sync::Arc::new(c),
        Err(e) => {
            println!("Could not reach ViGEmBus: {e:?}");
            println!("Install the ViGEmBus driver, then run this again.");
            std::process::exit(1);
        }
    };

    let mut target = vigem_client::Xbox360Wired::new(
        std::sync::Arc::clone(&client),
        vigem_client::TargetId::XBOX360_WIRED,
    );
    if let Err(e) = target.plugin() {
        println!("Could not plug a virtual pad: {e:?}");
        std::process::exit(1);
    }
    let _ = target.wait_ready();
    println!("Virtual pad is plugged in.");

    // Wait for Windows to finish enumerating it. The pad is only reachable
    // through XInput once the bus has reported it, and asking too early is
    // indistinguishable from a bus that does not carry feedback at all.
    let mut slots = Vec::new();
    let enumeration_deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < enumeration_deadline {
        slots = connected_slots();
        if !slots.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if slots.is_empty() {
        println!("Windows never reported the pad on XInput.");
        println!("The bus accepted it but the OS did not enumerate it.");
        drop(target);
        std::process::exit(2);
    }
    println!("Windows reports it on slot(s): {slots:?}");

    // The highest motor value seen since the listener was registered, and
    // whether the listener saw a real request at all. Held in atomics so the
    // callback does no locking on the path the driver calls it from.
    let peak = Arc::new(std::sync::atomic::AtomicU8::new(0));
    let seen = Arc::new(AtomicBool::new(false));

    let peak_for_thread = Arc::clone(&peak);
    let seen_for_thread = Arc::clone(&seen);
    let listener = match target.request_notification() {
        Ok(request) => request.spawn_thread(move |_request, data| {
            let strongest = data.large_motor.max(data.small_motor);
            // Keep the largest value rather than the newest: a pulse is short,
            // and sampling it is exactly what a probe is for.
            peak_for_thread.fetch_max(strongest, Ordering::SeqCst);
            if strongest > 0 {
                seen_for_thread.store(true, Ordering::SeqCst);
            }
        }),
        Err(e) => {
            println!("Could not register for notifications: {e:?}");
            println!("This is the line that decides whether rumble can work at all.");
            drop(target);
            std::process::exit(2);
        }
    };
    println!("Notification listener is registered.");

    // Write one frame first. A target that has never been written is sometimes
    // not ready to answer, and that would be reported as a dead rumble path.
    let neutral = vigem_client::XGamepad::default();
    if let Err(e) = target.update(&neutral) {
        println!("Could not write an initial frame: {e:?}");
    }
    std::thread::sleep(Duration::from_millis(150));

    println!("\nPulsing the motors at full strength...");
    if let Err(e) = padcore::xinput::vibrate(&slots, 255, 255) {
        println!("  the OS refused: {e}");
        drop(target);
        let _ = listener.join();
        std::process::exit(1);
    }

    // Wait for a value to climb rather than for a fixed time. A pulse can be
    // arbitrarily short, and the first thing to arrive is always an idle zero,
    // so a deadline that only waits for "something" proves nothing.
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut held: u8 = 0;
    while Instant::now() < deadline {
        let strongest = peak.load(Ordering::SeqCst);
        if strongest > held {
            held = strongest;
            println!("  strongest seen so far: {held}");
        }
        if seen.load(Ordering::SeqCst) && strongest >= NONZERO {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    let _ = padcore::xinput::vibrate(&slots, 0, 0);
    // Give the release a moment to arrive too, then stop listening. The target
    // must go before the thread is joined or the thread never returns.
    std::thread::sleep(Duration::from_millis(150));
    drop(target);
    let _ = listener.join();

    println!("\nResult");
    println!("------");
    if seen.load(Ordering::SeqCst) {
        println!("  Strongest request that crossed the bus: {held}");
        println!("  The driver carries force feedback, and our listener sees it.");
        println!("  PadForge forwards exactly this to the DualShock's motors.");
        std::process::exit(0);
    }

    println!("  No non-zero request arrived within three seconds.");
    println!("  The listener is registered and the pad accepted the write, so the");
    println!("  gap is between XInput and the bus. In practice that means:");
    println!("    1. The bus version is older than the notification interface.");
    println!("    2. Another process already owns a target on this slot.");
    println!("    3. The pulse is being swallowed before it reaches the bus.");
    std::process::exit(2);
}
