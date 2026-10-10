//! Driving XInput, the way a game does.
//!
//! Everything PadForge sends travels one way: the engine writes a report,
//! ViGEmBus turns it into an Xbox 360 pad, and the game reads that pad. The way
//! back is the only route force feedback has, and it arrives as XInput calls
//! against whichever slot the virtual pad ended up in.
//!
//! Nothing else in the app reaches that path, which is why it went unwired for
//! so long: a report could be refused, a notification never registered, and
//! every screen would still look healthy. Driving it deliberately is therefore
//! also the shortest proof that rumble works, because it is exactly what a game
//! does and exactly all a game does.

/// XInput always offers four slots and no more.
pub const SLOT_COUNT: u8 = 4;

/// XInput slots that currently hold a connected pad.
///
/// XInput cannot say which pad is ours, so this returns every slot that answers
/// rather than guessing one. A test that drove a single guessed slot would look
/// like a broken rumble whenever a second controller, or a second virtual pad,
/// happened to take the slot first.
pub fn connected_slots() -> Vec<u8> {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::Input::XboxController::{XInputGetState, XINPUT_STATE};
        (0..SLOT_COUNT)
            .filter(|slot| {
                let mut state = XINPUT_STATE::default();
                // SAFETY: a slot index in range and a struct the caller owns.
                // XInput writes into that struct and nowhere else.
                let status = unsafe { XInputGetState(u32::from(*slot), &mut state) };
                status == 0
            })
            .collect()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Vec::new()
    }
}

/// Ask every pad in `slots` to vibrate at these strengths, zero stopping them.
///
/// XInput takes a 16-bit motor speed and the drivers only look at the high byte,
/// so the shift happens here rather than at every call site.
///
/// Returns the number of pads that accepted the request, which is worth
/// surfacing: an empty list means there is nothing to vibrate at all, and that
/// is a different problem from a vibration nobody felt.
pub fn vibrate(slots: &[u8], heavy: u8, fast: u8) -> Result<usize, String> {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::Input::XboxController::{XInputSetState, XINPUT_VIBRATION};

        let vibration = XINPUT_VIBRATION {
            wLeftMotorSpeed: u16::from(heavy) << 8,
            wRightMotorSpeed: u16::from(fast) << 8,
        };

        let mut accepted = 0;
        let mut last_error = None;
        for slot in slots {
            // SAFETY: the vibration struct outlives the call, which is the only
            // thing the function reads.
            let status = unsafe { XInputSetState(u32::from(*slot), &vibration) };
            if status == 0 {
                accepted += 1;
            } else {
                last_error = Some(format!("slot {slot} returned {status}"));
            }
        }

        if accepted == 0 && slots.is_empty() {
            return Err("no connected pad to vibrate".into());
        }
        if accepted == 0 {
            return Err(format!(
                "no pad accepted the request{}",
                last_error.map(|e| format!(" ({e})")).unwrap_or_default()
            ));
        }
        Ok(accepted)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (slots, heavy, fast);
        Err("XInput only exists on Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_are_exactly_four_slots() {
        assert_eq!(SLOT_COUNT, 4);
    }

    #[test]
    fn nothing_to_vibrate_says_so() {
        // An empty list is a different complaint from a rejected one, and the
        // UI words them differently: "no pad" is a setup problem, "rejected" is
        // a driver problem.
        let err = vibrate(&[], 255, 255).unwrap_err();
        assert!(err.contains("no connected pad"), "{err}");
    }

    #[test]
    fn slots_are_in_range() {
        assert!(connected_slots().iter().all(|s| *s < SLOT_COUNT));
    }
}
