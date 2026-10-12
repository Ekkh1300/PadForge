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

/// The d-pad, as a DualShock 4 reports it.
///
/// A DualShock 4 reports its d-pad as a *hat*: one value for the eight compass
/// directions plus one for "nothing held", never as four independent bits.
/// XInput is the opposite, so up-and-left is two bits here and one value there.
///
/// A contradictory pair — up **and** down — is reported as nothing rather than
/// resolved in favour of one. A d-pad physically cannot do it, and picking a
/// winner would invent a press that nobody made.
pub const DS4_DPAD_NONE: u16 = 0x8;

/// Four XInput d-pad bits to a DualShock 4 hat value.
pub fn ds4_hat(buttons: u16) -> u16 {
    let up = buttons & XButtons::DPAD_UP != 0;
    let down = buttons & XButtons::DPAD_DOWN != 0;
    let left = buttons & XButtons::DPAD_LEFT != 0;
    let right = buttons & XButtons::DPAD_RIGHT != 0;
    match (up, down, left, right) {
        (true, false, true, false) => 0x7,  // north-west
        (true, false, false, true) => 0x1,  // north-east
        (false, true, true, false) => 0x5,  // south-west
        (false, true, false, true) => 0x3,  // south-east
        (true, false, false, false) => 0x0, // north
        (false, true, false, false) => 0x4, // south
        (false, false, true, false) => 0x6, // west
        (false, false, false, true) => 0x2, // east
        _ => DS4_DPAD_NONE,
    }
}

/// The digital button word of a DualShock 4 report.
///
/// The bits are ViGEmBus's own `DS4_BUTTONS`, which is what the driver reads
/// when this pad is published. The low nibble is the hat and the rest are the
/// face, shoulder, share/options and stick-click buttons.
pub fn ds4_buttons(state: GamepadState) -> u16 {
    let b = state.buttons;
    let mut out = ds4_hat(b);
    if b & XButtons::X != 0 {
        out |= 1 << 4; // square
    }
    if b & XButtons::A != 0 {
        out |= 1 << 5; // cross
    }
    if b & XButtons::B != 0 {
        out |= 1 << 6; // circle
    }
    if b & XButtons::Y != 0 {
        out |= 1 << 7; // triangle
    }
    if b & XButtons::LEFT_SHOULDER != 0 {
        out |= 1 << 8; // L1
    }
    if b & XButtons::RIGHT_SHOULDER != 0 {
        out |= 1 << 9; // R1
    }
    // A DualShock 4 reports each trigger twice: as travel and as a pressed
    // button. Halfway is where "pressed" starts, which is also roughly where
    // the real switch closes — the click is at the end of the travel, not at
    // the start of it.
    if state.left_trigger >= 128 {
        out |= 1 << 10;
    }
    if state.right_trigger >= 128 {
        out |= 1 << 11;
    }
    if b & XButtons::BACK != 0 {
        out |= 1 << 12; // share
    }
    if b & XButtons::START != 0 {
        out |= 1 << 13; // options
    }
    if b & XButtons::LEFT_THUMB != 0 {
        out |= 1 << 14;
    }
    if b & XButtons::RIGHT_THUMB != 0 {
        out |= 1 << 15;
    }
    out
}

/// The "special" byte: the PS button and the touchpad click, which a DualShock
/// 4 keeps outside its button word.
///
/// Only the PS button can be filled. A touchpad click arrives in the pad's own
/// HID report and XInput has nowhere to carry it, so it is gone by the time a
/// state reaches here — and inventing a press for it would be worse than
/// leaving it out.
pub fn ds4_special(state: GamepadState) -> u8 {
    let mut out = 0;
    if state.buttons & XButtons::GUIDE != 0 {
        out |= 1 << 0; // PS
    }
    out
}

/// One stick axis into a DualShock 4's byte, where `0x80` is centre.
///
/// XInput's Y grows upward and the DualShock 4's shrinks, so the two axes are
/// not written the same way; `invert` says which is being written. The result
/// lands on `0`, `0x80` and `0xFF` for full-left, centre and full-right, which
/// is where a real pad reports them.
pub fn ds4_axis(v: i16, invert: bool) -> u8 {
    let t = (v as f32 + 32767.0) / 65534.0 * 255.0;
    let t = if invert { 255.0 - t } else { t };
    t.round().clamp(0.0, 255.0) as u8
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
    /// The XInput slot this pad was published into, if it has one.
    ///
    /// Reported rather than assumed because the driver is the one that decides:
    /// a preferred slot is honoured only when it is free, so the number the
    /// player ends up on is whatever came back, and showing them the request
    /// instead of the answer would be the same class of mistake as claiming a
    /// lightbar was set when the write was refused.
    fn slot(&self) -> Option<u32> {
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
    /// No slot exists to report, which is not the same as slot zero: a viewer
    /// that claimed player 1 would look like a pad a game could drive.
    fn slot(&self) -> Option<u32> {
        None
    }
    fn last_state(&self) -> GamepadState {
        self.last
    }
    fn shutdown(&mut self) {}
}

#[cfg(target_os = "windows")]
mod vigem {
    use super::*;
    use crate::settings::OutputKind;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use vigem_client::{Client, DS4Report, DualShock4Wired, TargetId, Xbox360Wired};

    /// The virtual pad is exposed as a wired Xbox 360 controller, which is what
    /// every Windows game already knows how to talk to.
    type X360Pad = Xbox360Wired<Arc<Client>>;
    /// The other shape: a wired DualShock 4, for programs that speak HID.
    type Ds4Pad = DualShock4Wired<Arc<Client>>;

    /// Which device the bus was asked for.
    ///
    /// Two shapes over one backend rather than two backends: the connection,
    /// the reporting of why it is not there, and the teardown are all the same,
    /// and duplicating them would let the two drift apart.
    enum Target {
        X360(X360Pad),
        Ds4(Ds4Pad),
    }

    /// Real output through the ViGEmBus virtual driver.
    ///
    /// The driver has to be installed and this process has to be able to open
    /// `\\.\VIGEMBUS`. When it cannot we report disconnected rather than
    /// panicking, so the app degrades to a viewer instead of vanishing.
    pub struct VigemBackend {
        // Held so the target cannot outlive the bus connection.
        _client: Option<Arc<Client>>,
        target: Option<Target>,
        connected: bool,
        last_error: Option<String>,
        last: GamepadState,
        /// Latest motor state the game asked for, shared with the notification
        /// thread. Written there, taken here, so neither side ever blocks the
        /// other for longer than the lock.
        feedback: Arc<parking_lot::Mutex<Option<(u8, u8)>>>,
        /// The thread draining notifications. Joined once the target is gone.
        notify: Option<std::thread::JoinHandle<()>>,
        /// XInput slot the driver published this pad into.
        ///
        /// Read back from the driver rather than remembered from the request,
        /// because a preferred slot is only honoured when it is free. Always
        /// `None` for a DualShock 4, which is a HID device and never appears on
        /// the XInput bus at all.
        slot: Option<u32>,
        /// Which shape this backend is.
        kind: OutputKind,
    }

    impl VigemBackend {
        /// Try to bring up a virtual pad, taking whichever slot is free.
        pub fn connect() -> Self {
            Self::connect_with_slot(None, OutputKind::Xbox360)
        }

        /// Bring up a virtual pad of the chosen shape, asking for `wanted`, an
        /// XInput slot.
        ///
        /// ViGEmBus has no way to name a slot: `plugin` takes no index and the
        /// bus always hands out the lowest free one. So a request is honoured
        /// only when it agrees with what the bus was going to do anyway, and
        /// this reports back what actually happened rather than what was asked
        /// for.
        ///
        /// Re-plugging to force a slot was tried and removed. XInput keeps
        /// reporting a removed controller as present long after it is gone, so
        /// the bus hands the pad the same slot back every time and the second
        /// attempt lands exactly where the first did.
        ///
        /// `wanted` means nothing to a DualShock 4. That shape is a HID device
        /// and never reaches XInput, so there is no slot for it to be given.
        pub fn connect_with_slot(wanted: Option<usize>, kind: OutputKind) -> Self {
            match kind {
                OutputKind::Xbox360 => Self::x360(wanted),
                OutputKind::DualShock4 => Self::ds4(),
            }
        }

        /// The Xbox 360 shape: an XInput pad, the one every game knows, and the
        /// only one whose driver tells us when a game wants the motors running.
        fn x360(wanted: Option<usize>) -> Self {
            let client = match Client::connect() {
                Ok(c) => Arc::new(c),
                Err(e) => {
                    tracing::warn!("vigembus unavailable: {e:?}");
                    return Self::failed(format!("{e:?}"), OutputKind::Xbox360);
                }
            };

            let mut target = Xbox360Wired::new(Arc::clone(&client), TargetId::XBOX360_WIRED);
            let slot = Self::plug_and_read(&mut target);

            // Not a failure, and deliberately not one: the pad is live and
            // every button still works. It is simply somewhere other than
            // where it was asked to be, which the player needs to know rather
            // than be left believing the request took.
            let note = match (wanted, slot) {
                (Some(w), Some(s)) if w != s as usize => Some(format!(
                    "Player {} is not available to this pad, so it is Player {}",
                    w + 1,
                    s + 1
                )),
                // Nothing to report on either count. An unnamed slot is worth a
                // line of its own: without a number there is no way to tell
                // which player a game will pick this pad as.
                (Some(w), None) => Some(format!(
                    "Player {} was asked for, and the driver did not say where it landed",
                    w + 1
                )),
                _ => None,
            };
            if let Some(note) = &note {
                tracing::info!("{note}");
            }

            let feedback = Arc::new(parking_lot::Mutex::new(None));
            let notify = match target.request_notification() {
                Ok(request) => {
                    let slot = Arc::clone(&feedback);
                    Some(request.spawn_thread(move |_request, data| {
                        // XInput calls the low-frequency motor "large" and the
                        // high-frequency one "small", while the DualShock report
                        // wants them as (heavy, fast). They line up in that order.
                        //
                        // Logged on change rather than on every notification:
                        // a driver calls back continuously, and a line per
                        // callback would bury everything else in the file. A
                        // change is the only value worth a line, and it is
                        // exactly what is missing when a vibration never
                        // arrives — the callback fires, with zeroes, forever.
                        let motors = (data.large_motor, data.small_motor);
                        let mut previous = slot.lock();
                        if *previous != Some(motors) {
                            tracing::debug!(
                                "force feedback from the driver: heavy={} fast={}",
                                motors.0,
                                motors.1
                            );
                        }
                        *previous = Some(motors);
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
                target: Some(Target::X360(target)),
                connected: true,
                last_error: note,
                last: GamepadState::neutral(),
                feedback,
                notify,
                slot,
                kind: OutputKind::Xbox360,
            }
        }

        /// The DualShock 4 shape: a HID device on the bus, with no XInput slot.
        ///
        /// Chosen by programs that speak to a pad directly rather than through
        /// XInput — emulators, Steam's own stack, anything that wants the share
        /// and options buttons by name.
        fn ds4() -> Self {
            let kind = OutputKind::DualShock4;
            let client = match Client::connect() {
                Ok(c) => Arc::new(c),
                Err(e) => {
                    tracing::warn!("vigembus unavailable: {e:?}");
                    return Self::failed(format!("{e:?}"), kind);
                }
            };

            let mut target = DualShock4Wired::new(Arc::clone(&client), TargetId::DUALSHOCK4_WIRED);
            if let Err(e) = target.plugin() {
                tracing::warn!("could not plug the virtual DualShock 4: {e:?}");
                return Self::failed(format!("{e:?}"), kind);
            }
            let _ = target.wait_ready();

            // Said once, here, and deliberately not once per report. A player
            // who picked this shape needs to know what it costs, and a warning
            // buried in a log file is not somewhere anybody looks while
            // wondering why the pad has stopped buzzing.
            tracing::info!(
                "virtual DualShock 4 published. Force feedback does not travel back \
                 through this shape: the driver's DualShock 4 target has no \
                 notification channel, so a game's rumble stops at the bus."
            );

            Self {
                _client: Some(client),
                target: Some(Target::Ds4(target)),
                connected: true,
                last_error: None,
                last: GamepadState::neutral(),
                feedback: Arc::new(parking_lot::Mutex::new(None)),
                notify: None,
                slot: None,
                kind,
            }
        }

        /// How long to wait for the driver to name the slot this pad landed in.
        ///
        /// Enumeration is asynchronous, so the number is not there the moment
        /// the pad is plugged in. This is a ceiling, not a delay: the loop
        /// below returns as soon as the driver answers, and this only bounds
        /// how long it may stay silent.
        const SLOT_WAIT_MS: u64 = 500;

        /// How often to look for the pad's slot to become known.
        const SLOT_POLL_MS: u64 = 10;

        /// Plug the pad in and ask the driver which XInput slot it took.
        ///
        /// Enumeration is asynchronous, so the number is not available the
        /// instant `plugin` returns. Rather than sleep a fixed amount and hope,
        /// the question is asked repeatedly until it is answered, which is
        /// quick on a healthy machine and bounded on one that never answers.
        ///
        /// The driver's own message is what decides whether to ask again.
        /// "Not ready yet" is an answer that will change; anything else is an
        /// answer that will not, and retrying it would only delay finding out.
        fn plug_and_read(target: &mut X360Pad) -> Option<u32> {
            if let Err(e) = target.plugin() {
                tracing::warn!("could not plug virtual pad: {e:?}");
                return None;
            }
            let _ = target.wait_ready();

            let deadline = Instant::now() + Duration::from_millis(Self::SLOT_WAIT_MS);
            loop {
                match target.get_user_index() {
                    Ok(i) => return Some(i),
                    Err(e) => {
                        // WinError 433 is `ERROR_INVALID_DEVICE_OBJECT_PARAMETER`,
                        // which the driver returns while it is still working out
                        // where the pad landed. It is the only refusal that
                        // changes with time, so it is the only one worth
                        // asking again; anything else is a real answer.
                        //
                        // Matched by its meaning rather than by the text of the
                        // message, which is Windows' and could be reworded.
                        let not_ready = matches!(
                            e,
                            vigem_client::Error::UserIndexOutOfRange
                                | vigem_client::Error::WinError(433)
                        );
                        if !not_ready {
                            tracing::warn!("the driver did not report the XInput slot: {e:?}");
                            return None;
                        }
                        if Instant::now() >= deadline {
                            tracing::warn!("the driver never settled on an XInput slot: {e:?}");
                            return None;
                        }
                        std::thread::sleep(Duration::from_millis(Self::SLOT_POLL_MS));
                    }
                }
            }
        }

        /// Build a disconnected backend, remembering why.
        fn failed(reason: String, kind: OutputKind) -> Self {
            Self {
                _client: None,
                target: None,
                connected: false,
                last_error: Some(reason),
                last: GamepadState::neutral(),
                feedback: Arc::new(parking_lot::Mutex::new(None)),
                notify: None,
                slot: None,
                kind,
            }
        }

        /// The report a DualShock 4 would have sent, from the state the engine
        /// keeps.
        ///
        /// Assembled here rather than in the caller so the two axes and the
        /// button word are built the same way every time, and so a test can
        /// hold the result and look at it.
        fn ds4_report(state: GamepadState) -> DS4Report {
            DS4Report {
                thumb_lx: ds4_axis(state.thumb_lx, false),
                // XInput's Y grows upward and a DualShock 4's shrinks, so the
                // two are written opposite ways round.
                thumb_ly: ds4_axis(state.thumb_ly, true),
                thumb_rx: ds4_axis(state.thumb_rx, false),
                thumb_ry: ds4_axis(state.thumb_ry, true),
                buttons: ds4_buttons(state),
                special: ds4_special(state),
                trigger_l: state.left_trigger,
                trigger_r: state.right_trigger,
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
                format!("ViGEmBus · virtual {} pad", self.kind.label())
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
            match target {
                Target::X360(target) => {
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
                Target::Ds4(target) => {
                    let report = Self::ds4_report(state);
                    if let Err(e) = target.update(&report) {
                        tracing::trace!("report dropped: {e:?}");
                    }
                }
            }
        }

        fn last_state(&self) -> GamepadState {
            self.last
        }

        fn take_rumble(&mut self) -> Option<(u8, u8)> {
            self.feedback.lock().take()
        }

        fn slot(&self) -> Option<u32> {
            self.slot
        }

        fn shutdown(&mut self) {
            // Both shapes unplug the same way; the match exists only because
            // they are different types, and `unplug` is what tells the bus the
            // pad has gone rather than leaving it to time out.
            let unplugged = match self.target.take() {
                Some(Target::X360(mut t)) => t.unplug(),
                Some(Target::Ds4(mut t)) => t.unplug(),
                None => Ok(()),
            };
            if let Err(e) = unplugged {
                tracing::debug!("unplug reported: {e:?}");
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

    // --- DualShock 4 output -------------------------------------------------

    /// A state holding nothing, so a test can press one thing and count bits.
    fn held(buttons: u16) -> GamepadState {
        GamepadState {
            buttons,
            ..GamepadState::neutral()
        }
    }

    #[test]
    fn ds4_hat_walks_the_compass_in_the_driver_s_order() {
        // ViGEmBus's DS4_DPAD_DIRECTIONS runs north, north-east, east and so on
        // clockwise, with 8 meaning nothing held. It is the order the driver
        // reads, so it is the one that has to be right.
        assert_eq!(ds4_hat(XButtons::DPAD_UP), 0x0);
        assert_eq!(ds4_hat(XButtons::DPAD_UP | XButtons::DPAD_RIGHT), 0x1);
        assert_eq!(ds4_hat(XButtons::DPAD_RIGHT), 0x2);
        assert_eq!(ds4_hat(XButtons::DPAD_DOWN | XButtons::DPAD_RIGHT), 0x3);
        assert_eq!(ds4_hat(XButtons::DPAD_DOWN), 0x4);
        assert_eq!(ds4_hat(XButtons::DPAD_DOWN | XButtons::DPAD_LEFT), 0x5);
        assert_eq!(ds4_hat(XButtons::DPAD_LEFT), 0x6);
        assert_eq!(ds4_hat(XButtons::DPAD_UP | XButtons::DPAD_LEFT), 0x7);
        assert_eq!(ds4_hat(0), DS4_DPAD_NONE);
    }

    #[test]
    fn ds4_hat_refuses_a_direction_a_d_pad_cannot_make() {
        // Up and down at once is two XInput bits, which a d-pad physically
        // cannot produce. Reporting either way round would invent a press that
        // nobody made, so the answer is nothing held.
        assert_eq!(
            ds4_hat(XButtons::DPAD_UP | XButtons::DPAD_DOWN),
            DS4_DPAD_NONE
        );
        assert_eq!(
            ds4_hat(XButtons::DPAD_LEFT | XButtons::DPAD_RIGHT),
            DS4_DPAD_NONE
        );
        assert_eq!(
            ds4_hat(XButtons::DPAD_UP | XButtons::DPAD_DOWN | XButtons::DPAD_LEFT),
            DS4_DPAD_NONE
        );
    }

    #[test]
    fn ds4_face_buttons_land_on_their_own_bits() {
        // The bits are ViGEmBus's DS4_BUTTONS: square, cross, circle, triangle
        // in that order above the hat nibble. Each face button is checked
        // against every other, because a shifted mapping would still pass a
        // test that only looked at one of them.
        //
        // Every expectation carries the hat's idle value as well: the d-pad
        // always occupies the low nibble, and "no d-pad pressed" is 8 rather
        // than 0. Reading these as bare bit numbers is how the mapping would
        // pass by accident.
        assert_eq!(ds4_buttons(held(XButtons::X)), DS4_DPAD_NONE | (1 << 4));
        assert_eq!(ds4_buttons(held(XButtons::A)), DS4_DPAD_NONE | (1 << 5));
        assert_eq!(ds4_buttons(held(XButtons::B)), DS4_DPAD_NONE | (1 << 6));
        assert_eq!(ds4_buttons(held(XButtons::Y)), DS4_DPAD_NONE | (1 << 7));
    }

    #[test]
    fn ds4_shoulders_share_and_stick_clicks_keep_their_order() {
        assert_eq!(
            ds4_buttons(held(XButtons::LEFT_SHOULDER)),
            DS4_DPAD_NONE | (1 << 8)
        );
        assert_eq!(
            ds4_buttons(held(XButtons::RIGHT_SHOULDER)),
            DS4_DPAD_NONE | (1 << 9)
        );
        assert_eq!(ds4_buttons(held(XButtons::BACK)), DS4_DPAD_NONE | (1 << 12));
        assert_eq!(
            ds4_buttons(held(XButtons::START)),
            DS4_DPAD_NONE | (1 << 13)
        );
        assert_eq!(
            ds4_buttons(held(XButtons::LEFT_THUMB)),
            DS4_DPAD_NONE | (1 << 14)
        );
        assert_eq!(
            ds4_buttons(held(XButtons::RIGHT_THUMB)),
            DS4_DPAD_NONE | (1 << 15)
        );
    }

    #[test]
    fn ds4_triggers_report_travel_and_a_press() {
        // A DualShock 4 sends each trigger twice: as how far it is pulled and
        // as whether it counts as pressed. Halfway is where that starts, so a
        // feather touch is only travel and a pull past the middle is both.
        let light = GamepadState {
            left_trigger: 40,
            ..GamepadState::neutral()
        };
        assert_eq!(ds4_buttons(light), DS4_DPAD_NONE);

        let pulled = GamepadState {
            left_trigger: 40,
            right_trigger: 200,
            ..GamepadState::neutral()
        };
        assert_eq!(ds4_buttons(pulled), DS4_DPAD_NONE | (1 << 11));

        // Exactly on the line counts as pressed, so the boundary is not a
        // place where the button can vanish.
        let edge = GamepadState {
            right_trigger: 128,
            ..GamepadState::neutral()
        };
        assert_eq!(ds4_buttons(edge), DS4_DPAD_NONE | (1 << 11));
        let below = GamepadState {
            right_trigger: 127,
            ..GamepadState::neutral()
        };
        assert_eq!(ds4_buttons(below), DS4_DPAD_NONE);
    }

    #[test]
    fn ds4_hat_and_buttons_coexist_in_one_word() {
        // The hat occupies the low nibble, so pressing up *and* a face button
        // has to keep both. This is the case a test of either half alone would
        // miss.
        let both = GamepadState {
            buttons: XButtons::DPAD_UP | XButtons::Y | XButtons::BACK,
            ..GamepadState::neutral()
        };
        assert_eq!(ds4_buttons(both), (1 << 7) | (1 << 12));
    }

    #[test]
    fn ds4_ps_button_lives_in_the_special_byte() {
        assert_eq!(ds4_special(held(XButtons::GUIDE)), 1 << 0);
        assert_eq!(ds4_special(held(XButtons::A)), 0);
    }

    #[test]
    fn ds4_axis_anchors_left_centre_and_right() {
        // A DualShock 4's own encoding: 0x80 at rest, 0 at full travel one way
        // and 0xFF at the other. Landing exactly on those three is what stops
        // a virtual pad from sitting a hair off-centre forever.
        assert_eq!(ds4_axis(-i16::MAX, false), 0x00);
        assert_eq!(ds4_axis(0, false), 0x80);
        assert_eq!(ds4_axis(i16::MAX, false), 0xFF);
        // The trigger byte is deliberately past centre: 0x80 *is* the middle,
        // and rounding to the nearest count cannot put it anywhere else.
        assert_eq!(ds4_axis(0, true), 0x80);
    }

    #[test]
    fn ds4_y_axis_runs_the_opposite_way_to_xinput() {
        // XInput's Y grows upward. A DualShock 4's shrinks. Getting this wrong
        // inverts the stick in every game and no other test would notice,
        // because both halves of the pair are self-consistent.
        assert_eq!(ds4_axis(i16::MAX, true), 0x00); // up
        assert_eq!(ds4_axis(-i16::MAX, true), 0xFF); // down
                                                     // The reading a real DualShock 4 sends for "pushed straight up" is 0,
                                                     // which is what an inverted full-up becomes — and full-down is its
                                                     // mirror, so the pair is not the same value twice.
        assert_eq!(ds4_axis(i16::MAX, true), 0x00);
        assert_ne!(ds4_axis(i16::MAX, true), ds4_axis(-i16::MAX, true));
    }

    #[test]
    fn neutral_releases_everything() {
        let s = GamepadState::neutral();
        assert_eq!(s.buttons, 0);
        assert_eq!(s.left_trigger, 0);
        assert_eq!(s.thumb_lx, 0);
    }
}
