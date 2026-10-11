//! Steering wheels: reading any HID joystick, and mapping it onto a pad.
//!
//! A DualShock is one device with one known report layout, which is why the
//! rest of this crate can decode it by hand. A wheel is not: there is no
//! standard layout, the same model differs between its USB and Bluetooth
//! firmwares, and an axis may be eight bits in one report and sixteen in the
//! next. So nothing here guesses at bytes.
//!
//! Windows already ships a parser that answers every question a byte-level
//! reader would have to guess its way to — which bits are an axis, how wide it
//! is, what range the driver says it covers, which bits are a button. It is
//! [`HidP_GetUsageValue`] and its neighbours, and it is the same machinery the
//! game itself would use to read the wheel directly. Reading through it means
//! this module needs no table of known wheels, and a wheel nobody has written
//! a profile for works the first time it is plugged in.
//!
//! The module is in two halves on purpose:
//!
//!   * The pure half — targets, bindings, and the fold from a frame to an
//!     XInput state — knows nothing about Windows and is tested here.
//!   * The reading half needs a wheel attached to be proven, so it is kept
//!     small and is exercised by `probe-wheel`, which prints whatever the
//!     parser believes about any connected HID interface. Running it against
//!     the DualShock itself is the check that the generic path is right: if it
//!     reports the same stick positions as the hand-written decoder, the parser
//!     is reading reports correctly, and a wheel is only a different layout.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use tracing::debug;
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    HidD_FreePreparsedData, HidD_GetPreparsedData, HidP_GetButtonCaps, HidP_GetCaps,
    HidP_GetUsageValue, HidP_GetUsages, HidP_GetValueCaps, HidP_MaxUsageListLength,
    HIDP_BUTTON_CAPS, HIDP_CAPS, HIDP_VALUE_CAPS,
};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, INVALID_HANDLE_VALUE};

use crate::filters::{AxisFilter, Curve, Smoothing};
use crate::output::{GamepadState, XButtons};

/// HID usage page for Generic Desktop Controls, which is where sticks and
/// pedals live.
const PAGE_GENERIC_DESKTOP: u16 = 0x01;
/// The usages that mean "this is a thing you steer or press".
const USAGE_JOYSTICK: u16 = 0x04;
const USAGE_GAMEPAD: u16 = 0x05;
const USAGE_MULTI_AXIS: u16 = 0x08;

/// HIDP_STATUS_SUCCESS. Not zero, which is the usual NTSTATUS convention and
/// the reason a naive `!= 0` check calls every successful parse a failure.
const HIDP_STATUS_SUCCESS: i32 = 0x0011_0000;
/// `HidP_Input`, from the `HIDP_REPORT_TYPE` enum in `hidpi.h`.
const REPORT_INPUT: i32 = 0;
/// HID usage page for Button, which is where every face button lives.
const PAGE_BUTTON: u16 = 0x09;

/// Identity of one control on a wheel: its HID usage page and usage.
///
/// Stored rather than an index because an index is an enumeration detail. The
/// same wheel on different firmware — or two of the same wheel — can order
/// their controls differently while keeping the usages the driver assigned, so
/// an index would silently retarget a saved binding while a usage stays put.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ControlId {
    pub page: u16,
    pub usage: u16,
}

/// What a control physically is, and the numbers needed to read it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ControlKind {
    /// A value the driver declares a logical range for. The range is kept
    /// rather than assumed, because a wheel's steering axis is 0..=65535 on
    /// some models and -32768..=32767 on others and only the driver knows.
    Axis { min: i32, max: i32, bits: u16 },
    /// A button. Read through a different call than an axis, because that is
    /// how the parser reports them: a set of pressed usages, not a number.
    Button,
}

/// One control, as the driver describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct Control {
    pub id: ControlId,
    /// Which link collection it belongs to, needed by the parser.
    pub link: u16,
    /// A name worth showing: "X", "Z", "Button 4".
    pub label: String,
    pub kind: ControlKind,
}

impl Control {
    /// Whether this control can hold a value worth shaping, as opposed to being
    /// on or off.
    pub fn is_axis(&self) -> bool {
        matches!(self.kind, ControlKind::Axis { .. })
    }

    /// The declared range of an axis, for anything that needs to show what the
    /// driver said rather than what came out of it.
    pub fn range(&self) -> Option<(i32, i32)> {
        match self.kind {
            ControlKind::Axis { min, max, .. } => Some((min, max)),
            ControlKind::Button => None,
        }
    }
}

/// What the player wants one control on the wheel to do.
///
/// Six, because that is what an XInput pad has to give a racing game: one
/// axis of steering, two pedals, and three buttons' worth of everything else.
/// A wheel with more controls than targets is normal, and the extra controls
/// are simply not bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WheelTarget {
    Steer,
    Throttle,
    Brake,
    Handbrake,
    GearUp,
    GearDown,
}

impl WheelTarget {
    /// Every target, in the order a learner walks through them.
    pub const ALL: &'static [WheelTarget] = &[
        WheelTarget::Steer,
        WheelTarget::Throttle,
        WheelTarget::Brake,
        WheelTarget::Handbrake,
        WheelTarget::GearUp,
        WheelTarget::GearDown,
    ];

    pub fn label(self) -> &'static str {
        match self {
            WheelTarget::Steer => "steering",
            WheelTarget::Throttle => "throttle",
            WheelTarget::Brake => "brake",
            WheelTarget::Handbrake => "handbrake",
            WheelTarget::GearUp => "shift up",
            WheelTarget::GearDown => "shift down",
        }
    }

    /// What the control should read as while it is being learned: the thing to
    /// move, and how far.
    ///
    /// Told separately from [`Self::label`] because the two audiences differ.
    /// A label is read by someone who already knows what they want; an
    /// instruction is read by someone who has been asked to press something and
    /// does not yet know which.
    pub fn prompt(self) -> &'static str {
        match self {
            WheelTarget::Steer => "turn the wheel all the way left, then right",
            WheelTarget::Throttle => "press the throttle pedal fully",
            WheelTarget::Brake => "press the brake pedal fully",
            WheelTarget::Handbrake => "pull the handbrake",
            WheelTarget::GearUp => "pull the up-shift paddle",
            WheelTarget::GearDown => "pull the down-shift paddle",
        }
    }

    /// The XInput button this target publishes, if it is a button target.
    ///
    /// Paddle shifters become the bumpers and the handbrake becomes X, which is
    /// the convention racing titles already use on a gamepad — so a wheel
    /// needs nothing remapped in the game's own settings.
    pub fn button(self) -> Option<u16> {
        match self {
            WheelTarget::Handbrake => Some(XButtons::X),
            WheelTarget::GearUp => Some(XButtons::RIGHT_SHOULDER),
            WheelTarget::GearDown => Some(XButtons::LEFT_SHOULDER),
            WheelTarget::Steer | WheelTarget::Throttle | WheelTarget::Brake => None,
        }
    }
}

/// One learned assignment: a control on the wheel, and where it goes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WheelBinding {
    pub target: WheelTarget,
    pub control: ControlId,
    #[serde(default)]
    pub invert: bool,
    /// Where the control stops being ignored, 0..=0.95.
    #[serde(default)]
    pub deadzone: f32,
    #[serde(default = "unit_gain")]
    pub sensitivity: f32,
    #[serde(default)]
    pub curve: Curve,
    /// Put back the travel the dead zone removed, so everything past the dead
    /// zone behaves exactly as it would without one.
    #[serde(default)]
    pub anti_deadzone: bool,
}

fn unit_gain() -> f32 {
    1.0
}

impl WheelBinding {
    /// A binding with nothing decided yet, for the learner to fill in.
    pub fn new(target: WheelTarget, control: ControlId) -> Self {
        Self {
            target,
            control,
            invert: false,
            deadzone: 0.0,
            sensitivity: 1.0,
            curve: Curve::Linear,
            // On by default for steering and off for pedals, which is the only
            // split worth having: a wheel must not lose any of its travel to a
            // dead zone, while a pedal that jumps off its rest position makes
            // the car lurch. It is a field rather than a rule so a profile says
            // what it did, and can be changed by hand like everything else.
            anti_deadzone: target == WheelTarget::Steer,
        }
    }

    /// Run one raw value through this binding's filter chain.
    ///
    /// `fraction` is how far through its declared travel the control is, 0..=1.
    /// Steering is offered the axis signed, because it is the only target that
    /// is bipolar; everything else is a pedal that goes from rest to full and
    /// is offered the fraction as it stands.
    pub fn shape(&self, fraction: f32) -> f32 {
        let mut filter = AxisFilter::new();
        filter.deadzone = self.deadzone.clamp(0.0, 0.95);
        filter.curve = self.curve;
        filter.anti_deadzone = self.anti_deadzone;
        // A wheel reports in the hundreds of hertz and a filter's smoothing
        // state belongs to the reader, not to the shaping. Leaving it off here
        // keeps the fold a pure function of its inputs, which is what makes the
        // rest of this module testable without a wheel attached.
        filter.smoothing = Smoothing::Off;
        filter.sensitivity = self.sensitivity;
        filter.inverted = self.invert;
        filter.apply(if self.target == WheelTarget::Steer {
            fraction * 2.0 - 1.0
        } else {
            fraction
        })
    }
}

/// What a frame of wheel input wants the virtual pad to do.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Shaped {
    /// -1..=1, into the left thumbstick's X.
    pub steer: f32,
    /// 0..=1, into the right trigger.
    pub throttle: f32,
    /// 0..=1, into the left trigger.
    pub brake: f32,
    pub handbrake: bool,
    pub gear_up: bool,
    pub gear_down: bool,
}

impl Shaped {
    /// Fold into an XInput state.
    ///
    /// A wheel has no stick Y, no face buttons and no dpad, so they are left
    /// where [`GamepadState::neutral`] puts them rather than copied from a pad
    /// that may not be connected. A game that reads a direction it was never
    /// given gets the neutral position, which is the same answer a real wheel
    /// would give it.
    pub fn to_gamepad(&self) -> GamepadState {
        let mut state = GamepadState::neutral();
        state.thumb_lx = GamepadState::quantise(self.steer);
        state.right_trigger = GamepadState::quantise_trigger(self.throttle);
        state.left_trigger = GamepadState::quantise_trigger(self.brake);

        let mut held = 0u16;
        for (pressed, target) in [
            (self.handbrake, WheelTarget::Handbrake),
            (self.gear_up, WheelTarget::GearUp),
            (self.gear_down, WheelTarget::GearDown),
        ] {
            if pressed {
                if let Some(bit) = target.button() {
                    held |= bit;
                }
            }
        }
        state.buttons = held;
        state
    }
}

/// Fold one frame through every binding.
///
/// `values` holds the normalised position of each control the reader could
/// read; a binding whose control is absent contributes nothing rather than a
/// zero, so unplugging a pedal's cable leaves the throttle alone instead of
/// pinning it.
///
/// Where two bindings name the same target, the first one to appear in
/// `bindings` wins. That is a rule rather than an accident: silently taking the
/// last would mean a re-learn moved the target without the interface ever
/// saying so, and taking the highest would be the same problem with different
/// arithmetic. The learner removes any previous binding for a target before
/// adding a new one, so this should not come up in practice — it is a rule
/// because "should not" is not "cannot".
pub fn shape(values: &[(ControlId, f32)], bindings: &[WheelBinding]) -> Shaped {
    let mut out = Shaped::default();
    // Whether a binding has been applied, kept apart from the value because the
    // value can be zero and mean two different things. A wheel sitting at
    // centre and a wheel with no steering bound both produce 0.0, and only one
    // of them should stop a later binding from taking the target.
    let mut steer_set = false;
    let mut throttle_set = false;
    let mut brake_set = false;

    for binding in bindings {
        let Some((_, fraction)) = values.iter().find(|(id, _)| *id == binding.control) else {
            continue;
        };
        let shaped = binding.shape(*fraction);
        match binding.target {
            WheelTarget::Steer if !steer_set => {
                out.steer = shaped;
                steer_set = true;
            }
            WheelTarget::Throttle if !throttle_set => {
                out.throttle = shaped.clamp(0.0, 1.0);
                throttle_set = true;
            }
            WheelTarget::Brake if !brake_set => {
                out.brake = shaped.clamp(0.0, 1.0);
                brake_set = true;
            }
            // A button target reads its own shaped value rather than asking the
            // control what kind of thing it is, so an axis bound to the
            // handbrake works as a paddle and a button bound to the throttle
            // works as a digital pedal. Both are real setups, and a rule that
            // refused either would need the player to know which was which
            // before they could set it up.
            WheelTarget::Handbrake => out.handbrake |= shaped > 0.5,
            WheelTarget::GearUp => out.gear_up |= shaped > 0.5,
            WheelTarget::GearDown => out.gear_down |= shaped > 0.5,
            _ => {}
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Reading: enumeration and the Windows HID parser
// ---------------------------------------------------------------------------

/// Why the reader would not open, in words the interface can show.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("hidapi: {0}")]
    Hid(String),
    #[error("windows error {0}")]
    Win(u32),
    #[error("the interface did not describe its layout")]
    NoLayout,
}

/// One HID interface that declares itself a joystick, game pad or multi-axis
/// controller — which is what a wheel declares itself to be.
#[derive(Debug, Clone, PartialEq)]
pub struct WheelDevice {
    /// The OS path, stable across a session and the thing the reader opens.
    pub path: String,
    /// Whatever the driver says this device is called.
    pub label: String,
    pub vendor_id: u16,
    pub product_id: u16,
    /// The usage that put this device on the list.
    pub usage: u16,
}

impl WheelDevice {
    /// What a USB id looks like to a person checking whether their wheel is the
    /// one on screen.
    pub fn ids(&self) -> String {
        format!("{:04x}:{:04x}", self.vendor_id, self.product_id)
    }
}

/// Every connected interface that could be a wheel.
///
/// A list rather than a guess. These usages are what a wheel reports, and also
/// what a flight stick, a guitar, an arcade pad and several other things
/// report, so the honest answer is to show the player the candidates and let
/// them choose — which is what the Racing page does, with every axis moving
/// live beside its option.
///
/// Two kinds of device are left out, and both are the same kind.
///
/// The DualShock is excluded because it reports the same page and usage as a
/// game pad, because it is one, and listing it here would put the device this
/// app is already driving at the top of the picker.
///
/// XInput devices are excluded for a reason worth stating plainly: this app's
/// own output is one. Letting a player select it would mean the wheel's
/// position was whatever the app last published — a loop that settles on
/// wherever it started and drives the car there. A real Xbox pad is caught by
/// the same rule, and should be: a game pad has no axis worth calling steering,
/// so a player who picks one is being offered something that cannot work.
pub fn devices() -> Vec<WheelDevice> {
    // A fresh `HidApi` answers from the list captured when it was built, so
    // this cannot reuse one held elsewhere — a stale object would report the
    // devices that were attached when it was made, not the ones attached now.
    let Ok(api) = hidapi::HidApi::new() else {
        return Vec::new();
    };
    api.device_list()
        .filter(|d| {
            d.usage_page() == PAGE_GENERIC_DESKTOP
                && matches!(d.usage(), USAGE_JOYSTICK | USAGE_GAMEPAD | USAGE_MULTI_AXIS)
                && !(d.vendor_id() == crate::report::SONY_VENDOR_ID
                    && crate::report::DS4_PRODUCT_IDS.contains(&d.product_id()))
                && !is_xinput(d.path())
        })
        .map(|d| WheelDevice {
            path: d.path().to_string_lossy().into_owned(),
            label: d
                .product_string()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("unnamed controller")
                .to_string(),
            vendor_id: d.vendor_id(),
            product_id: d.product_id(),
            usage: d.usage(),
        })
        .collect()
}

/// Whether a device path is an XInput pad, which Windows marks with `&IG_` in
/// the interface name.
///
/// Matched case-insensitively, against the path rather than the vendor and
/// product ids, and against the marker with its leading separator: ids are
/// shared between a real Xbox pad and a virtual one while the marker is put
/// there by the same code in both cases, and a bare `ig_` would also be found
/// inside a name that merely happens to contain those letters.
fn is_xinput(path: &std::ffi::CStr) -> bool {
    let path = path.to_string_lossy().to_ascii_lowercase();
    path.contains("&ig_0")
}

/// A group of buttons sharing one link collection, so a frame asks the parser
/// once for the whole group instead of once per button.
#[derive(Debug, Clone)]
struct ButtonGroup {
    page: u16,
    link: u16,
    /// Indices into [`WheelReader::controls`].
    controls: Vec<usize>,
    /// How many usages the parser might name, which is the buffer it needs.
    capacity: u32,
}

/// A wheel, opened, with its layout already decoded by the driver.
pub struct WheelReader {
    device: hidapi::HidDevice,
    /// What the driver calls this device. Captured once, because asking again
    /// is a round trip to the driver and the name cannot have changed.
    label: String,
    /// A second handle, opened only to be asked what the layout is and kept
    /// because the parsed form of that answer is read from every frame.
    driver: *mut std::ffi::c_void,
    /// The driver's parsed layout. Owned by this reader until it is freed.
    preparsed: isize,
    caps: HIDP_CAPS,
    /// Axes first, then buttons, in one list — each with its place in the
    /// layout, so nothing has to be looked up again while a frame is read.
    controls: Vec<Control>,
    button_groups: Vec<ButtonGroup>,
    buf: Vec<u8>,
}

// `HidDevice` is `Send` and the handles are plain integers, so a reader can
// live on whatever thread wants it.
unsafe impl Send for WheelReader {}

impl WheelReader {
    /// Open a wheel and ask the driver what its reports look like.
    ///
    /// Two handles rather than one: `hidapi` gives a reader that behaves the
    /// way the rest of this crate already expects — non-blocking, drained per
    /// frame — while the parsed layout comes from the Windows HID API, which
    /// needs its own handle. Both are shareable, so neither closes the other
    /// out, and neither stops the game from opening the wheel too.
    pub fn open(device: &WheelDevice) -> Result<Self, Error> {
        let api = hidapi::HidApi::new().map_err(|e| Error::Hid(e.to_string()))?;
        let path = std::ffi::CString::new(device.path.as_str())
            .map_err(|_| Error::Hid("device path contains a nul".into()))?;
        let reader = api
            .open_path(&path)
            .map_err(|e| Error::Hid(e.to_string()))?;

        let driver = open_driver(&device.path).map_err(Error::Win)?;
        let mut reader = match build(reader, driver, device) {
            Ok(reader) => reader,
            Err(err) => {
                // The handle is the reader's to release, including when the
                // layout it was opened for turns out to be unusable.
                unsafe {
                    CloseHandle(driver);
                }
                return Err(err);
            }
        };

        // One buffer per report, sized by the driver rather than guessed, so a
        // long report can never be truncated mid-field.
        reader.buf = vec![0u8; reader.caps.InputReportByteLength as usize];
        Ok(reader)
    }

    /// The controls this wheel exposes, with the names and ranges the driver
    /// declared. Fixed after `open`, so anything may hold on to the slice.
    pub fn controls(&self) -> &[Control] {
        &self.controls
    }

    /// What the driver called the device, or `unnamed controller`.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Read whatever the wheel has reported since the last call, and answer
    /// with the newest frame.
    ///
    /// A wheel reports on its own schedule, often several times faster than
    /// this app draws, so one read per frame would both lag and discard
    /// reports. Draining to the last one costs nothing and is the only version
    /// that is as fresh as the hardware made it.
    pub fn poll(&mut self) -> Result<Option<Vec<(ControlId, f32)>>, Error> {
        let mut latest = None;
        loop {
            let n = self
                .device
                .read_timeout(&mut self.buf, 0)
                .map_err(|e| Error::Hid(e.to_string()))?;
            if n == 0 {
                break;
            }
            // The parser is handed the report exactly as the driver returned
            // it, which is not always the buffer `hidapi` hands us — see
            // `as_report`.
            let report = self.as_report(&self.buf[..n]);
            latest = Some(self.parse(&report));
        }
        Ok(latest)
    }

    /// The report as the parser expects it, which is not always the buffer
    /// `hidapi` hands back.
    ///
    /// `hidapi` strips a leading `0x00` when the device uses no report ids,
    /// because the driver sticks one on anyway and applications speaking raw
    /// HID do not want it. The parser is the opposite caller: it was handed
    /// that same buffer by `ReadFile` and expects exactly what was read. The
    /// two are told apart by length — one byte short means a byte was taken
    /// off.
    fn as_report<'a>(&self, read: &'a [u8]) -> Cow<'a, [u8]> {
        if read.len() + 1 == self.caps.InputReportByteLength as usize {
            let mut owned = Vec::with_capacity(read.len() + 1);
            owned.push(0);
            owned.extend_from_slice(read);
            Cow::Owned(owned)
        } else {
            Cow::Borrowed(read)
        }
    }

    /// Turn one report into a normalised value per control.
    ///
    /// A control the parser refuses to answer is dropped rather than reported
    /// as centred. A wheel with one dead control should show that control as
    /// dead; showing it as the axis resting is a different statement and a
    /// misleading one.
    fn parse(&self, report: &[u8]) -> Vec<(ControlId, f32)> {
        // Buttons come first, because the parser answers for a whole link
        // collection at once and the answers are needed while the axes are
        // walked.
        let mut down: Vec<u16> = Vec::new();
        for group in &self.button_groups {
            if let Some(pressed) = self.pressed_in(group, report) {
                down.extend(pressed);
            }
        }

        let mut out = Vec::with_capacity(self.controls.len());
        for control in &self.controls {
            match control.kind {
                ControlKind::Axis { min, max, bits } => {
                    let mut raw = 0u32;
                    let status = unsafe {
                        HidP_GetUsageValue(
                            REPORT_INPUT,
                            control.id.page,
                            control.link,
                            control.id.usage,
                            &mut raw,
                            self.preparsed,
                            report.as_ptr(),
                            report.len() as u32,
                        )
                    };
                    if status != HIDP_STATUS_SUCCESS {
                        continue;
                    }
                    out.push((control.id, fraction(sign_extend(raw, bits, min), min, max)));
                }
                ControlKind::Button => {
                    let value = if down.contains(&control.id.usage) {
                        1.0
                    } else {
                        0.0
                    };
                    out.push((control.id, value));
                }
            }
        }
        out
    }

    /// Which buttons are down in one link collection, or nothing if the parser
    /// turned the answer down.
    fn pressed_in(&self, group: &ButtonGroup, report: &[u8]) -> Option<Vec<u16>> {
        let mut list = vec![0u16; group.capacity as usize];
        let mut len = list.len() as u32;
        let status = unsafe {
            HidP_GetUsages(
                REPORT_INPUT,
                group.page,
                group.link,
                list.as_mut_ptr(),
                &mut len,
                self.preparsed,
                report.as_ptr() as *mut u8,
                report.len() as u32,
            )
        };
        if status != HIDP_STATUS_SUCCESS {
            return None;
        }
        list.truncate(len as usize);
        Some(list)
    }
}

impl Drop for WheelReader {
    fn drop(&mut self) {
        if self.preparsed != 0 {
            unsafe {
                HidD_FreePreparsedData(self.preparsed);
            }
        }
        if !self.driver.is_null() && self.driver != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.driver);
            }
        }
    }
}

/// Ask the driver for the parsed layout of an open interface.
fn build(
    reader: hidapi::HidDevice,
    driver: *mut std::ffi::c_void,
    device: &WheelDevice,
) -> Result<WheelReader, Error> {
    let mut preparsed: isize = 0;
    if !unsafe { HidD_GetPreparsedData(driver, &mut preparsed) } {
        return Err(Error::Win(unsafe { GetLastError() }));
    }

    let (caps, controls) = match describe(preparsed) {
        Ok(found) => found,
        Err(err) => {
            unsafe {
                HidD_FreePreparsedData(preparsed);
            }
            return Err(err);
        }
    };

    // A collection with nothing to read is not a wheel, and opening it anyway
    // would give the picker a row that never moves.
    if controls.is_empty() {
        unsafe {
            HidD_FreePreparsedData(preparsed);
        }
        debug!(path = %device.path, "hid interface has no readable controls");
        return Err(Error::NoLayout);
    }

    let button_groups = group_buttons(&controls, preparsed);
    Ok(WheelReader {
        device: reader,
        label: device.label.clone(),
        driver,
        preparsed,
        caps,
        controls,
        button_groups,
        buf: Vec::new(),
    })
}

/// Read the layout: what each axis's declared range is, and which bits are
/// buttons.
fn describe(preparsed: isize) -> Result<(HIDP_CAPS, Vec<Control>), Error> {
    let mut caps: HIDP_CAPS = unsafe { std::mem::zeroed() };
    if unsafe { HidP_GetCaps(preparsed, &mut caps) } != HIDP_STATUS_SUCCESS {
        return Err(Error::NoLayout);
    }

    let mut controls = Vec::new();

    // Axes first. A player scanning the list is looking for the thing they are
    // about to turn, not the thing they are about to press.
    if caps.NumberInputValueCaps > 0 {
        let mut list: Vec<HIDP_VALUE_CAPS> = (0..caps.NumberInputValueCaps)
            .map(|_| HIDP_VALUE_CAPS::default())
            .collect();
        let mut count = list.len() as u16;
        if unsafe { HidP_GetValueCaps(REPORT_INPUT, list.as_mut_ptr(), &mut count, preparsed) }
            == HIDP_STATUS_SUCCESS
        {
            for value in &list[..count as usize] {
                // A cap may name a range of usages and describes one bit field
                // per usage in it, so each is a control of its own.
                let (first, last) = if value.IsRange {
                    unsafe {
                        (
                            value.Anonymous.Range.UsageMin,
                            value.Anonymous.Range.UsageMax,
                        )
                    }
                } else {
                    let usage = unsafe { value.Anonymous.NotRange.Usage };
                    (usage, usage)
                };
                for usage in first..=last {
                    controls.push(Control {
                        id: ControlId {
                            page: value.UsagePage,
                            usage,
                        },
                        link: value.LinkCollection,
                        label: value_label(value.UsagePage, usage),
                        kind: ControlKind::Axis {
                            min: value.LogicalMin,
                            max: value.LogicalMax,
                            bits: value.BitSize,
                        },
                    });
                }
            }
        }
    }

    if caps.NumberInputButtonCaps > 0 {
        let mut list: Vec<HIDP_BUTTON_CAPS> = (0..caps.NumberInputButtonCaps)
            .map(|_| HIDP_BUTTON_CAPS::default())
            .collect();
        let mut count = list.len() as u16;
        if unsafe { HidP_GetButtonCaps(REPORT_INPUT, list.as_mut_ptr(), &mut count, preparsed) }
            == HIDP_STATUS_SUCCESS
        {
            for button in &list[..count as usize] {
                let (first, last) = if button.IsRange {
                    unsafe {
                        (
                            button.Anonymous.Range.UsageMin,
                            button.Anonymous.Range.UsageMax,
                        )
                    }
                } else {
                    let usage = unsafe { button.Anonymous.NotRange.Usage };
                    (usage, usage)
                };
                for usage in first..=last {
                    controls.push(Control {
                        id: ControlId {
                            page: button.UsagePage,
                            usage,
                        },
                        link: button.LinkCollection,
                        label: button_label(button.UsagePage, usage),
                        kind: ControlKind::Button,
                    });
                }
            }
        }
    }

    Ok((caps, controls))
}

/// Group the buttons by the collection they live in.
///
/// The parser answers "which buttons are down" for a whole page and collection
/// at once, never for one button, so asking per button would run the same
/// report walk sixteen times over. Grouping makes a frame cost one call per
/// collection, of which a wheel has one or two.
fn group_buttons(controls: &[Control], preparsed: isize) -> Vec<ButtonGroup> {
    let mut groups: Vec<ButtonGroup> = Vec::new();
    for (index, control) in controls.iter().enumerate() {
        if !matches!(control.kind, ControlKind::Button) {
            continue;
        }
        let key = (control.id.page, control.link);
        match groups.iter_mut().find(|g| (g.page, g.link) == key) {
            Some(group) => group.controls.push(index),
            None => groups.push(ButtonGroup {
                page: key.0,
                link: key.1,
                controls: vec![index],
                capacity: unsafe { HidP_MaxUsageListLength(REPORT_INPUT, key.0, preparsed) }.max(1),
            }),
        }
    }
    groups
}

/// A name for a control, in the vocabulary the HID specification already uses.
///
/// Shown rather than numbered because "Z" and "Rz" are what every wheel's own
/// manual and every forum thread say, so a player looking for the axis their
/// pedal reports on can search for it. Numbering would be tidier and useless.
fn value_label(page: u16, usage: u16) -> String {
    if page == PAGE_GENERIC_DESKTOP {
        let name = match usage {
            0x30 => "X",
            0x31 => "Y",
            0x32 => "Z",
            0x33 => "Rx",
            0x34 => "Ry",
            0x35 => "Rz",
            0x36 => "Slider",
            0x37 => "Dial",
            0x38 => "Wheel",
            0x39 => "Hat switch",
            _ => return format!("axis {usage:#04x}"),
        };
        name.to_string()
    } else {
        format!("axis {usage:#04x}")
    }
}

/// A button's name. Numbered, because buttons carry no standard vocabulary of
/// their own beyond "the one you pressed first".
fn button_label(page: u16, usage: u16) -> String {
    if page == PAGE_BUTTON {
        format!("button {usage}")
    } else {
        format!("button {usage} ({page:#04x})")
    }
}

/// Open a driver handle onto an OS path.
///
/// Read and write are both requested even though nothing is ever written: a
/// wheel's HID interface can refuse a read-only open, and the handle here is
/// used only to be asked what the layout is.
fn open_driver(path: &str) -> Result<*mut std::ffi::c_void, u32> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };

    let wide: Vec<u16> = std::ffi::OsStr::new(path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        Err(unsafe { GetLastError() })
    } else {
        Ok(handle)
    }
}

/// The parser hands back a field's bits zero-extended, which is wrong for an
/// axis the driver declares a negative half. A steering wheel is commonly
/// -32768..=32767, and without this, left would read as a huge positive number.
fn sign_extend(raw: u32, bits: u16, min: i32) -> i32 {
    if min >= 0 {
        // The whole declared range is positive, so the bits already are the value.
        return raw.min(i32::MAX as u32) as i32;
    }
    if bits >= 32 {
        return raw as i32;
    }
    let shift = 32 - bits.max(1) as u32;
    // Shifting left and then back, arithmetically, replicates the sign bit
    // across the widening. Doing it by subtraction instead would need the
    // sign's position calculated twice and would be right in exactly as many
    // cases as this is.
    ((raw as i32) << shift) >> shift
}

/// Where a value sits in the driver's declared travel, 0..=1.
///
/// The range is the driver's and not a constant, and that is the whole point
/// of going through the parser: an axis declared 0..=255 and one declared
/// 0..=65535 both come out of here as the fraction of travel they moved, which
/// is the only unit the rest of this module understands.
fn fraction(raw: i32, min: i32, max: i32) -> f32 {
    if max <= min {
        return 0.0;
    }
    // Widened before subtracting: an axis declared across the whole of `i32`
    // would overflow the subtraction, and `i64` costs nothing to reach for.
    let span = max as i64 - min as i64;
    ((raw as i64 - min as i64) as f32 / span as f32).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEER: ControlId = ControlId {
        page: PAGE_GENERIC_DESKTOP,
        usage: 0x38,
    };
    const THROTTLE: ControlId = ControlId {
        page: PAGE_GENERIC_DESKTOP,
        usage: 0x35,
    };
    const BRAKE: ControlId = ControlId {
        page: PAGE_GENERIC_DESKTOP,
        usage: 0x33,
    };
    const PADDLE_UP: ControlId = ControlId {
        page: PAGE_BUTTON,
        usage: 3,
    };

    /// A binding with nothing tuned, so a test states only the thing it is about.
    fn plain(target: WheelTarget, control: ControlId) -> WheelBinding {
        WheelBinding {
            target,
            control,
            invert: false,
            deadzone: 0.0,
            sensitivity: 1.0,
            curve: Curve::Linear,
            anti_deadzone: false,
        }
    }

    fn axis(min: i32, max: i32, bits: u16) -> ControlKind {
        ControlKind::Axis { min, max, bits }
    }

    // -- the fold from a frame to a pad --------------------------------------

    #[test]
    fn a_steer_binding_reaches_both_locks() {
        let bindings = [plain(WheelTarget::Steer, STEER)];
        for (fraction, want) in [(0.0, -1.0), (0.5, 0.0), (1.0, 1.0)] {
            let out = shape(&[(STEER, fraction)], &bindings);
            assert!(
                (out.steer - want).abs() < 1e-6,
                "{fraction} gave {}",
                out.steer
            );
        }
    }

    #[test]
    fn pedals_stay_in_their_own_range() {
        // A throttle is a trigger: XInput wants 0..=1 and nothing else. A pedal
        // held at rest must read as released, not as half pressed.
        let bindings = [
            plain(WheelTarget::Throttle, THROTTLE),
            plain(WheelTarget::Brake, BRAKE),
        ];
        let out = shape(&[(THROTTLE, 0.0), (BRAKE, 0.0)], &bindings);
        assert_eq!(out.throttle, 0.0);
        assert_eq!(out.brake, 0.0);

        let out = shape(&[(THROTTLE, 1.0), (BRAKE, 1.0)], &bindings);
        assert_eq!(out.throttle, 1.0);
        assert_eq!(out.brake, 1.0);
    }

    #[test]
    fn a_binding_whose_control_is_absent_contributes_nothing() {
        // Unplugging a pedal's cable must leave the throttle alone, not pin it.
        // A missing control is a different statement from a centred one, and
        // reading it as centred would be a lie the driver never told.
        let bindings = [plain(WheelTarget::Throttle, THROTTLE)];
        let out = shape(&[(STEER, 0.5)], &bindings);
        assert_eq!(out.throttle, 0.0);
        assert_eq!(out.steer, 0.0);
    }

    #[test]
    fn the_first_binding_for_a_target_wins() {
        // Two bindings naming one target is a state the learner should never
        // produce, but if it does, the answer must be the same twice over
        // rather than whichever the fold happened to reach last. Both bindings
        // here read the same control, so the only thing that can differ
        // between the two orderings is which one is honoured — and it must
        // always be the first.
        let gentle = WheelBinding {
            sensitivity: 1.0,
            ..plain(WheelTarget::Steer, STEER)
        };
        let harsh = WheelBinding {
            sensitivity: 4.0,
            ..plain(WheelTarget::Steer, STEER)
        };
        for bindings in [[gentle.clone(), harsh.clone()], [harsh, gentle]] {
            let out = shape(&[(STEER, 0.5)], &bindings);
            assert!((out.steer - 0.0).abs() < 1e-6, "gave {}", out.steer);
        }
    }

    #[test]
    fn a_button_target_reads_past_half() {
        // An axis bound to the handbrake is a paddle, and a button bound to the
        // throttle is a digital pedal. Both are real setups, and both work
        // because the fold asks the shaped value and not the control's kind.
        let bindings = [plain(WheelTarget::Handbrake, PADDLE_UP)];
        assert!(!shape(&[(PADDLE_UP, 0.4)], &bindings).handbrake);
        assert!(shape(&[(PADDLE_UP, 1.0)], &bindings).handbrake);
    }

    #[test]
    fn a_bound_button_publishes_its_xinput_bit() {
        assert_eq!(WheelTarget::Handbrake.button(), Some(XButtons::X));
        assert_eq!(WheelTarget::GearUp.button(), Some(XButtons::RIGHT_SHOULDER));
        assert_eq!(
            WheelTarget::GearDown.button(),
            Some(XButtons::LEFT_SHOULDER)
        );
        assert_eq!(WheelTarget::Steer.button(), None);
    }

    #[test]
    fn the_shaped_state_is_a_whole_xinput_state() {
        let out = Shaped {
            steer: 1.0,
            throttle: 1.0,
            gear_up: true,
            ..Shaped::default()
        };
        let state = out.to_gamepad();

        assert_eq!(state.thumb_lx, i16::MAX);
        assert_eq!(state.right_trigger, 255);
        assert_eq!(state.left_trigger, 0);
        assert_eq!(state.buttons, XButtons::RIGHT_SHOULDER);
        // A wheel has no stick Y and no face buttons, and they must read as
        // neutral rather than carry over from whatever was there.
        assert_eq!(state.thumb_ly, 0);
        assert_eq!(state.thumb_rx, 0);
    }

    #[test]
    fn every_target_has_a_name_and_an_instruction() {
        // Shown to a player who does not know which paddle is which yet, so a
        // missing one would leave a row with nowhere to look.
        for target in WheelTarget::ALL {
            assert!(!target.label().is_empty());
            assert!(!target.prompt().is_empty());
        }
        assert_eq!(WheelTarget::ALL.len(), 6);
    }

    // -- the filter a binding applies ---------------------------------------

    #[test]
    fn invert_flips_both_ends_of_the_steer() {
        let mut binding = plain(WheelTarget::Steer, STEER);
        binding.invert = true;
        assert!((binding.shape(1.0) - -1.0).abs() < 1e-6);
        assert!((binding.shape(0.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_deadzone_silences_the_centre() {
        let mut binding = plain(WheelTarget::Steer, STEER);
        binding.deadzone = 0.2;
        assert_eq!(binding.shape(0.5), 0.0, "sitting inside the dead zone");
        // Past it, and the axis must still reach full lock: a dead zone that
        // ate the edge of the wheel would make the car understeer by design.
        assert!((binding.shape(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn anti_deadzone_restores_the_scale_past_the_dead_zone() {
        // Steering wants this: the dead zone silences the centre and changes
        // nothing else, so what the driver declares is what the game gets.
        let mut binding = plain(WheelTarget::Steer, STEER);
        binding.deadzone = 0.1;
        binding.anti_deadzone = true;
        assert!((binding.shape(0.5) - 0.0).abs() < 1e-6);
        assert!((binding.shape(1.0) - 1.0).abs() < 1e-6);

        // And a pedal does not: a throttle that jumps to a tenth at the moment
        // the dead zone ends makes the car lurch off the line.
        let mut pedal = plain(WheelTarget::Throttle, THROTTLE);
        pedal.deadzone = 0.1;
        assert!(pedal.shape(0.1) < 1e-6);
    }

    #[test]
    fn a_steering_binding_is_anti_deadzoned_by_default_and_a_pedal_is_not() {
        assert!(WheelBinding::new(WheelTarget::Steer, STEER).anti_deadzone);
        assert!(!WheelBinding::new(WheelTarget::Throttle, THROTTLE).anti_deadzone);
    }

    #[test]
    fn a_deadzone_cannot_be_set_to_the_whole_axis() {
        // A dead zone of 1.0 would make the control silent at every position
        // including full travel, which is not a dead zone but a disconnection
        // with extra steps.
        let mut binding = plain(WheelTarget::Steer, STEER);
        binding.deadzone = 1.0;
        assert!(binding.shape(1.0).abs() > 0.9, "must still reach lock");
    }

    // -- normalisation --------------------------------------------------------

    #[test]
    fn a_value_normalises_across_what_the_driver_declared() {
        // The same axis declared two ways is the same amount of travel, and the
        // rest of this module has one unit, so this is the boundary it lives on.
        assert_eq!(fraction(0, 0, 255), 0.0);
        assert_eq!(fraction(255, 0, 255), 1.0);
        assert!((fraction(128, 0, 255) - 0.501_960_8).abs() < 1e-5);

        // A signed axis, which is how most wheels declare their steering. The
        // range is asymmetric — 65536 values either side of zero but the
        // positive end stops one short — so the exact midpoint is a hair over
        // a half, and asserting an exact half here would be asserting that the
        // range was symmetric, which it is not.
        assert_eq!(fraction(-32768, -32768, 32767), 0.0);
        assert_eq!(fraction(32767, -32768, 32767), 1.0);
        assert!((fraction(0, -32768, 32767) - 0.5).abs() < 1e-4);
    }

    #[test]
    fn a_value_out_of_its_declared_range_does_not_escape() {
        // A driver that reports one tick past its own maximum — which happens
        // when a pedal is not calibrated — must not push the trigger past full
        // or, worse, wrap it around to the other end.
        assert_eq!(fraction(300, 0, 255), 1.0);
        assert_eq!(fraction(-1, 0, 255), 0.0);
    }

    #[test]
    fn an_axis_with_no_travel_is_centred_rather_than_divided_by_zero() {
        // `max <= min` means the driver declared nothing usable. Reporting a
        // fraction anyway would be a number with no referent, and dividing by
        // the span would be the one way to turn that into a NaN that then
        // survives clamping into a trigger.
        assert_eq!(fraction(5, 5, 5), 0.0);
        assert_eq!(fraction(5, 10, 5), 0.0);
    }

    // -- the parser's arithmetic ---------------------------------------------

    #[test]
    fn a_negative_field_reads_as_negative() {
        // The parser hands back the field's bits zero-extended, so without this
        // a wheel turned left would report a huge positive number and the car
        // would steer right.
        assert_eq!(sign_extend(0xFFFF, 16, -32768), -1);
        assert_eq!(sign_extend(0x8000, 16, -32768), -32768);
        assert_eq!(sign_extend(0x7FFF, 16, -32768), 32767);
        assert_eq!(sign_extend(0xFF, 8, -128), -1);
        assert_eq!(sign_extend(0x80, 8, -128), -128);
    }

    #[test]
    fn a_positive_field_is_left_alone() {
        // Many pedals declare 0..=255 or 0..=65535 and have no negative half,
        // so the bits already are the value and sign-extending would break them.
        assert_eq!(sign_extend(0xFFFF, 16, 0), 65535);
        assert_eq!(sign_extend(255, 8, 0), 255);
    }

    #[test]
    fn a_control_is_an_axis_only_when_the_driver_says_so() {
        let control = Control {
            id: STEER,
            link: 0,
            label: "Wheel".into(),
            kind: axis(-32768, 32767, 16),
        };
        assert!(control.is_axis());
        assert_eq!(control.range(), Some((-32768, 32767)));

        let button = Control {
            id: PADDLE_UP,
            link: 0,
            label: "button 3".into(),
            kind: ControlKind::Button,
        };
        assert!(!button.is_axis());
        assert_eq!(button.range(), None);
    }

    #[test]
    fn the_picker_shows_what_to_move_next_to_what_it_is_called() {
        // A player reading the picker knows their wheel as "the G29" and their
        // pedals as "the axis called Z". Neither name alone finds the other.
        assert_eq!(value_label(PAGE_GENERIC_DESKTOP, 0x38), "Wheel");
        assert_eq!(value_label(PAGE_GENERIC_DESKTOP, 0x35), "Rz");
        assert_eq!(value_label(PAGE_GENERIC_DESKTOP, 0x33), "Rx");
        assert_eq!(button_label(PAGE_BUTTON, 4), "button 4");
        // An unknown usage is named by what it is rather than dropped, because
        // a control nobody can find in the list cannot be bound.
        assert_eq!(value_label(PAGE_GENERIC_DESKTOP, 0x99), "axis 0x99");
    }

    // -- serialisation --------------------------------------------------------

    #[test]
    fn a_binding_survives_a_round_trip_through_the_profile() {
        let mut binding = WheelBinding::new(WheelTarget::Brake, BRAKE);
        binding.invert = true;
        binding.deadzone = 0.15;
        binding.sensitivity = 2.0;
        binding.curve = Curve::exponential(3.0);

        let text = serde_json::to_string(&binding).expect("serialize");
        let back: WheelBinding = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(back, binding);
    }

    #[test]
    fn a_target_names_itself_in_the_profile_as_words() {
        // Written by hand and read by a person, so `Steer` and `throttle` would
        // both be worse than what the tag already gives us.
        let binding = WheelBinding::new(WheelTarget::Throttle, THROTTLE);
        let text = serde_json::to_string(&binding).expect("serialize");
        assert!(text.contains("\"throttle\""), "{text}");
    }

    #[test]
    fn a_binding_written_by_an_older_version_still_loads() {
        // Fields added since the first release have to be optional on the way
        // in, or every profile saved before them becomes unreadable.
        let text = r#"{"target":"steer","control":{"page":1,"usage":56}}"#;
        let binding: WheelBinding = serde_json::from_str(text).expect("defaults");
        assert_eq!(binding.sensitivity, 1.0);
        assert_eq!(binding.deadzone, 0.0);
        assert!(!binding.invert);
        assert!(!binding.anti_deadzone);
        assert_eq!(binding.curve, Curve::Linear);
    }

    #[test]
    fn a_wheel_device_reports_its_usb_id_for_a_person_to_check() {
        let device = WheelDevice {
            path: r"\\?\hid#vid_046d&pid_c24f".into(),
            label: "G29".into(),
            vendor_id: 0x046D,
            product_id: 0xC24F,
            usage: USAGE_JOYSTICK,
        };
        assert_eq!(device.ids(), "046d:c24f");
    }

    #[test]
    fn the_apps_own_output_is_never_offered_back_as_an_input() {
        // The path a driver reports, which is the thing this check reads. Built
        // here rather than matched live so the test cannot be skipped by having
        // no virtual pad attached to the machine running it.
        fn path(text: &str) -> std::ffi::CString {
            std::ffi::CString::new(text).expect("no nul")
        }

        // The virtual pad reports as a game pad, because it is one, and it is
        // the only thing on the list that reports it without being a real
        // device. Selecting it would mean reading back whatever this app last
        // published — the wheel's position would be the wheel's last position,
        // which is a loop that settles wherever it started and drives the car
        // there. Worth more than a passing assertion: nothing about the shape
        // of that failure is loud enough to catch in play.
        let published = path(r"\\?\hid#vid_045e&pid_028e&ig_00#3&b8a95ad&0&0000");
        assert!(is_xinput(&published), "our own output must be excluded");

        // Upper case, because a driver may spell it either way and the check
        // must not depend on which one showed up.
        let shouted = path(r"\\?\HID#VID_046D&PID_C24F&MI_00#8&2D12BED&1&0000");
        assert!(!is_xinput(&shouted), "a real pad must be kept");

        // A wheel whose name merely contains those letters must not be caught:
        // the marker is the whole of `&IG_`, not the letters on their own.
        let unlucky = path(r"\\?\hid#vid_1234&pid_5678&fig_09");
        assert!(!is_xinput(&unlucky), "a name that contains ig_ is not one");
    }
}
