//! Virtual gamepad output.
//!
//! Games on Windows overwhelmingly speak XInput, not HID. So the whole point of
//! this app is to turn the DS4 report into an `XINPUT_GAMEPAD` and hand it to a
//! virtual driver that the OS presents as a real controller.
//!
//! [`XButtons`] is defined here (rather than imported) so that the mapping layer
//! can reference button bits without pulling in a platform-specific type, and so
//! the same values are used by the unit tests.

/// XInput-compatible button flags, matching the `wButtons` layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(transparent)]
pub struct XButtons(pub u16);

impl XButtons {
    pub const DPAD_UP: u16 = 0x0001;
    pub const DPAD_DOWN: u16 = 0x0002;
    pub const DPAD_LEFT: u16 = 0x0004;
    pub const DPAD_RIGHT: u16 = 0x0008;
    pub const START: u16 = 0x0010;
    pub const BACK: u16 = 0x0020;
    pub const LEFT_THUMB: u16 = 0x0040;
    pub const RIGHT_THUMB: u16 = 0x0080;
    pub const LEFT_SHOULDER: u16 = 0x0100;
    pub const RIGHT_SHOULDER: u16 = 0x0200;
    pub const GUIDE: u16 = 0x0400;
    pub const A: u16 = 0x1000;
    pub const B: u16 = 0x2000;
    pub const X: u16 = 0x4000;
    pub const Y: u16 = 0x8000;

    /// Names as games and existing muscle memory use them.
    pub const L3: u16 = Self::LEFT_THUMB;
    pub const R3: u16 = Self::RIGHT_THUMB;
    pub const LB: u16 = Self::LEFT_SHOULDER;
    pub const RB: u16 = Self::RIGHT_SHOULDER;

    /// Is every bit in `mask` set?
    pub fn has(self, mask: u16) -> bool {
        self.0 & mask == mask
    }

    /// Is any bit in `mask` set?
    pub fn any(self, mask: u16) -> bool {
        self.0 & mask != 0
    }

    pub fn insert(&mut self, mask: u16) {
        self.0 |= mask;
    }

    pub fn clear(&mut self, mask: u16) {
        self.0 &= !mask;
    }

    /// Human-readable button list, for the UI's activity monitor.
    pub fn names(self) -> Vec<&'static str> {
        let table: [(u16, &str); 14] = [
            (Self::DPAD_UP, "DPadUp"),
            (Self::DPAD_DOWN, "DPadDown"),
            (Self::DPAD_LEFT, "DPadLeft"),
            (Self::DPAD_RIGHT, "DPadRight"),
            (Self::START, "Start"),
            (Self::BACK, "Back"),
            (Self::LEFT_THUMB, "L3"),
            (Self::RIGHT_THUMB, "R3"),
            (Self::LEFT_SHOULDER, "LB"),
            (Self::RIGHT_SHOULDER, "RB"),
            (Self::GUIDE, "Guide"),
            (Self::A, "A"),
            (Self::B, "B"),
            (Self::X, "X"),
        ];
        table
            .into_iter()
            .filter(|(bit, _)| self.has(*bit))
            .map(|(_, name)| name)
            .collect()
    }
}

/// A fully-formed XInput gamepad state, laid out exactly like `XINPUT_GAMEPAD`
/// so it can be sent without further marshalling.
#[derive(Debug, Clone, Copy, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct GamepadState {
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub thumb_lx: i16,
    pub thumb_ly: i16,
    pub thumb_rx: i16,
    pub thumb_ry: i16,
}

impl GamepadState {
    /// Release everything.
    pub fn neutral() -> Self {
        Self::default()
    }

    /// Signed axis float to XInput's signed 16-bit range.
    pub fn quantise(v: f32) -> i16 {
        (v.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16
    }

    /// 0..=1 float to a trigger byte.
    pub fn quantise_trigger(v: f32) -> u8 {
        (v.clamp(0.0, 1.0) * 255.0).round() as u8
    }
}

/// What the engine talks to, so the rest of the codebase does not care whether a
/// real virtual driver is present.
pub trait OutputBackend: Send {
    /// Human-readable name, shown in the UI.
    fn name(&self) -> String;
    /// True when reports are actually reaching the OS.
    fn is_connected(&self) -> bool;
    /// Publish one gamepad state.
    fn submit(&mut self, state: GamepadState);
    /// Most recently published state, for the UI's output monitor.
    fn last_state(&self) -> GamepadState;
    /// Force feedback the OS most recently asked for, or `None` if there is
    /// nothing new.
    ///
    /// This is the only direction the virtual pad can speak in: a game sets the
    /// motors through XInput, the driver passes it to the target, and without
    /// something reading it here the value exists nowhere in the process. Taken
    /// rather than polled because a vibration that stops is a real event too,
    /// and holding on to it would leave the pad buzzing after the game stopped.
    fn take_rumble(&mut self) -> Option<(u8, u8)> {
        None
    }
    /// Tear the virtual device down cleanly.
    fn shutdown(&mut self);
}

/// Used when the virtual driver is not installed. Keeps the whole app running
/// (device discovery, profile editing, the visualizer) instead of hard-failing,
/// which means a missing driver degrades to a viewer rather than a crash.
#[derive(Debug, Default)]
pub struct NullBackend {
    /// Last state submitted, so the UI can still show what would be sent.
    pub last: GamepadState,
}

impl OutputBackend for NullBackend {
    fn name(&self) -> String {
        "none (virtual driver not detected)".into()
    }
    fn is_connected(&self) -> bool {
        false
    }
    fn submit(&mut self, state: GamepadState) {
        self.last = state;
    }
    fn last_state(&self) -> GamepadState {
        self.last
    }
    fn shutdown(&mut self) {}
}

#[cfg(target_os = "windows")]
mod vigem {
    use super::*;
    use std::sync::Arc;
    use vigem_client::{Client, TargetId, Xbox360Wired};

    /// The virtual pad is exposed as a wired Xbox 360 controller, which is what
    /// every Windows game already knows how to talk to.
    type Pad = Xbox360Wired<Arc<Client>>;

    /// Real output through the ViGEmBus virtual driver.
    ///
    /// The driver has to be installed and this process has to be able to open
    /// `\\.\VIGEMBUS`. When it cannot we report disconnected rather than
    /// panicking, so the app degrades to a viewer instead of vanishing.
    pub struct VigemBackend {
        // Held so the target cannot outlive the bus connection.
        _client: Option<Arc<Client>>,
        target: Option<Pad>,
        connected: bool,
        last_error: Option<String>,
        last: GamepadState,
        /// Latest motor state the game asked for, shared with the notification
        /// thread. Written there, taken here, so neither side ever blocks the
        /// other for longer than the lock.
        feedback: Arc<parking_lot::Mutex<Option<(u8, u8)>>>,
        /// The thread draining notifications. Joined once the target is gone.
        notify: Option<std::thread::JoinHandle<()>>,
    }

    impl VigemBackend {
        /// Try to bring up a virtual pad.
        pub fn connect() -> Self {
            let client = match Client::connect() {
                Ok(c) => Arc::new(c),
                Err(e) => {
                    tracing::warn!("vigembus unavailable: {e:?}");
                    return Self::failed(format!("{e:?}"));
                }
            };

            let mut target = Xbox360Wired::new(Arc::clone(&client), TargetId::XBOX360_WIRED);

            if let Err(e) = target.plugin() {
                tracing::warn!("could not plug virtual pad: {e:?}");
                return Self {
                    _client: Some(client),
                    target: None,
                    connected: false,
                    last_error: Some(format!("{e:?}")),
                    last: GamepadState::neutral(),
                    feedback: Arc::new(parking_lot::Mutex::new(None)),
                    notify: None,
                };
            }

            // The device is enumerated asynchronously, so give it a moment
            // before the first report or the update races device startup.
            let _ = target.wait_ready();

            let feedback = Arc::new(parking_lot::Mutex::new(None));
            let notify = match target.request_notification() {
                Ok(request) => {
                    let slot = Arc::clone(&feedback);
                    Some(request.spawn_thread(move |_request, data| {
                        // XInput calls the low-frequency motor "large" and the
                        // high-frequency one "small", while the DualShock report
                        // wants them as (heavy, fast). They line up in that order.
                        *slot.lock() = Some((data.large_motor, data.small_motor));
                    }))
                }
                Err(e) => {
                    // The pad still maps and the lightbar still works without
                    // this, so the missing half is worth a warning, not a
                    // disconnected backend.
                    tracing::warn!("force feedback notifications unavailable: {e:?}");
                    None
                }
            };

            Self {
                _client: Some(client),
                target: Some(target),
                connected: true,
                last_error: None,
                last: GamepadState::neutral(),
                feedback,
                notify,
            }
        }

        /// Build a disconnected backend, remembering why.
        fn failed(reason: String) -> Self {
            Self {
                _client: None,
                target: None,
                connected: false,
                last_error: Some(reason),
                last: GamepadState::neutral(),
                feedback: Arc::new(parking_lot::Mutex::new(None)),
                notify: None,
            }
        }

        /// Why the driver is unavailable, for display in the UI.
        pub fn last_error(&self) -> Option<&str> {
            self.last_error.as_deref()
        }
    }

    impl OutputBackend for VigemBackend {
        fn name(&self) -> String {
            if self.connected {
                "ViGEmBus · virtual Xbox 360 pad".into()
            } else {
                format!(
                    "ViGEmBus unavailable · {}",
                    self.last_error.as_deref().unwrap_or("unknown")
                )
            }
        }

        fn is_connected(&self) -> bool {
            self.connected
        }

        fn submit(&mut self, state: GamepadState) {
            self.last = state;
            let Some(target) = self.target.as_mut() else {
                return;
            };
            let pad = vigem_client::XGamepad {
                buttons: vigem_client::XButtons(state.buttons),
                left_trigger: state.left_trigger,
                right_trigger: state.right_trigger,
                thumb_lx: state.thumb_lx,
                thumb_ly: state.thumb_ly,
                thumb_rx: state.thumb_rx,
                thumb_ry: state.thumb_ry,
            };
            // `TargetNotReady` for the first few milliseconds after plugging in is
            // expected, so nothing here is worth warning about.
            if let Err(e) = target.update(&pad) {
                tracing::trace!("report dropped: {e:?}");
            }
        }

        fn last_state(&self) -> GamepadState {
            self.last
        }

        fn take_rumble(&mut self) -> Option<(u8, u8)> {
            self.feedback.lock().take()
        }

        fn shutdown(&mut self) {
            if let Some(mut t) = self.target.take() {
                let _ = t.unplug();
            }
            self.connected = false;
        }
    }

    impl Drop for VigemBackend {
        fn drop(&mut self) {
            // The notification thread only returns once its target is gone, so
            // the target goes first. Joining while it is still alive would
            // block here until a notification nobody is going to send arrives.
            self.target = None;
            if let Some(handle) = self.notify.take() {
                let _ = handle.join();
            }
        }
    }
}

#[cfg(target_os = "windows")]
pub use vigem::VigemBackend;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantise_clamps_and_rounds() {
        assert_eq!(GamepadState::quantise(0.0), 0);
        assert_eq!(GamepadState::quantise(1.0), i16::MAX);
        assert_eq!(GamepadState::quantise(-1.0), -i16::MAX);
        assert_eq!(GamepadState::quantise(5.0), i16::MAX);
        assert_eq!(GamepadState::quantise(-5.0), -i16::MAX);
        assert_eq!(GamepadState::quantise_trigger(1.0), 255);
        assert_eq!(GamepadState::quantise_trigger(-3.0), 0);
    }

    #[test]
    fn button_bit_toggle() {
        let mut b = XButtons::default();
        assert!(!b.any(XButtons::A));
        b.insert(XButtons::A);
        assert!(b.has(XButtons::A));
        b.clear(XButtons::A);
        assert!(!b.any(XButtons::A));
    }

    #[test]
    fn names_lists_active_buttons() {
        let mut b = XButtons::default();
        b.insert(XButtons::A);
        b.insert(XButtons::DPAD_UP);
        let names = b.names();
        assert!(names.contains(&"A"));
        assert!(names.contains(&"DPadUp"));
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn neutral_releases_everything() {
        let s = GamepadState::neutral();
        assert_eq!(s.buttons, 0);
        assert_eq!(s.left_trigger, 0);
        assert_eq!(s.thumb_lx, 0);
    }
}
