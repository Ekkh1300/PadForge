//! DualShock 4 HID report decoding.
//!
//! The DS4 speaks two closely related report layouts depending on how it is
//! connected, and the difference is entirely in the leading header:
//!
//! | transport | report id | length | analog base | sensor base |
//! |-----------|-----------|--------|-------------|-------------|
//! | USB       | `0x01`    | 64     | 1           | 23          |
//! | Bluetooth | `0x01`    | 78     | 4           | 26          |
//!
//! Everything past `analog_base` is identical once you account for the shift,
//! so [`parse`] picks the right offsets from the buffer length.

use serde::{Deserialize, Serialize};

/// Sony Interactive Entertainment USB vendor id.
pub const SONY_VENDOR_ID: u16 = 0x054C;

/// Every DualShock 4 USB product id we know how to talk to.
pub const DS4_PRODUCT_IDS: &[u16] = &[
    0x05C4, // DS4 (v1)
    0x09CC, // DS4 (v2)
    0x0BA0, // DS4 (v2, second revision)
];

/// How the pad is plugged in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Transport {
    Usb,
    Bluetooth,
}

impl Transport {
    pub fn label(self) -> &'static str {
        match self {
            Transport::Usb => "USB",
            Transport::Bluetooth => "Bluetooth",
        }
    }
}

/// Battery charge level, 0..=10, plus charging flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Battery {
    /// 0 = empty, 10 = full.
    pub level: u8,
    pub charging: bool,
}

impl Battery {
    /// Remaining charge as a 0..=1 fraction.
    pub fn fraction(&self) -> f32 {
        (self.level as f32 / 10.0).clamp(0.0, 1.0)
    }

    pub fn is_low(&self) -> bool {
        !self.charging && self.level <= 2
    }
}

/// Bit positions of every digital control, packed into a single `u16`.
///
/// Keeping them in one word means "is this button down" is a single AND, and
/// comparing against the previous word gives us edge detection for free.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Buttons(pub u16);

impl Buttons {
    pub const CROSS: u16 = 1 << 0;
    pub const CIRCLE: u16 = 1 << 1;
    pub const SQUARE: u16 = 1 << 2;
    pub const TRIANGLE: u16 = 1 << 3;
    pub const L1: u16 = 1 << 4;
    pub const R1: u16 = 1 << 5;
    pub const L2: u16 = 1 << 6;
    pub const R2: u16 = 1 << 7;
    pub const SHARE: u16 = 1 << 8;
    pub const OPTIONS: u16 = 1 << 9;
    pub const L3: u16 = 1 << 10;
    pub const R3: u16 = 1 << 11;
    pub const UP: u16 = 1 << 12;
    pub const DOWN: u16 = 1 << 13;
    pub const LEFT: u16 = 1 << 14;
    pub const RIGHT: u16 = 1 << 15;

    #[inline]
    pub const fn contains(self, mask: u16) -> bool {
        self.0 & mask == mask
    }

    #[inline]
    pub const fn any(self, mask: u16) -> bool {
        self.0 & mask != 0
    }

    #[inline]
    pub const fn raw(self) -> u16 {
        self.0
    }

    #[inline]
    pub const fn pressed(self, mask: u16) -> bool {
        self.contains(mask)
    }

    /// Bits that went down between `prev` and `self`.
    #[inline]
    pub const fn just_pressed(self, prev: Buttons) -> Buttons {
        Buttons(self.0 & !prev.0)
    }

    /// Bits that came up between `prev` and `self`.
    #[inline]
    pub const fn just_released(self, prev: Buttons) -> Buttons {
        Buttons(!self.0 & prev.0)
    }
}

/// Raw three-axis motion sensor reading.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Gyro {
    /// Pitch, degrees per second.
    pub pitch: f32,
    /// Yaw, degrees per second.
    pub yaw: f32,
    /// Roll, degrees per second.
    pub roll: f32,
}

impl Gyro {
    pub fn is_still(&self, epsilon: f32) -> bool {
        self.pitch.abs() < epsilon && self.yaw.abs() < epsilon && self.roll.abs() < epsilon
    }
}

/// Raw accelerometer reading in g.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Accel {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// Touchpad state as reported by the DS4.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct TouchState {
    /// The pad surface is being touched.
    pub pad_touched: bool,
    /// The clickable pad is being pressed.
    pub pad_clicked: bool,
    /// Finger 1 position, normalised to 0..=1 over the *active* pad area.
    pub x: f32,
    pub y: f32,
    /// Raw sensor coordinate, 0..=191.
    pub raw_x: u8,
    pub raw_y: u8,
}

/// One decoded DS4 input frame.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Ds4Report {
    pub buttons: Buttons,
    /// Left stick, normalised to -1..=1 (already un-centred and inverted in Y so
    /// that "up" is +1, matching the XInput convention).
    pub left_x: f32,
    pub left_y: f32,
    pub right_x: f32,
    pub right_y: f32,
    /// Raw stick bytes, kept for calibration UI.
    pub raw_left_x: u8,
    pub raw_left_y: u8,
    pub raw_right_x: u8,
    pub raw_right_y: u8,
    pub l2: f32,
    pub r2: f32,
    pub gyro: Gyro,
    pub accel: Accel,
    pub touch: TouchState,
    pub battery: Battery,
    /// True on the very first successful decode, used to prime edge detection.
    pub fresh: bool,
}

impl Ds4Report {
    /// Neutral values, useful as an initial state.
    pub fn neutral() -> Self {
        Self {
            left_x: 0.0,
            left_y: 0.0,
            right_x: 0.0,
            right_y: 0.0,
            raw_left_x: AXIS_MIDPOINT as u8,
            raw_left_y: AXIS_MIDPOINT as u8,
            raw_right_x: AXIS_MIDPOINT as u8,
            raw_right_y: AXIS_MIDPOINT as u8,
            fresh: true,
            ..Default::default()
        }
    }
}

/// Resting position of an un-deflected DS4 stick axis.
pub const AXIS_MIDPOINT: u16 = 128;
/// Number of counts from centre to the edge of the usable travel.
pub const AXIS_HALF_RANGE: u16 = 127;

/// Accelerometer LSB -> g.
pub const ACCEL_SCALE: f32 = 1.0 / 8192.0;
/// Gyroscope LSB -> degrees per second.
pub const GYRO_SCALE: f32 = 1.0 / 16.0;

/// The touchpad's usable coordinate range. Values outside are clamped away.
const TOUCHPAD_ACTIVE_MAX: u8 = 191;

/// Offsets into the report, selected per transport.
#[derive(Debug, Clone, Copy)]
struct Layout {
    analog: usize,
    sensor: usize,
}

impl Layout {
    fn for_len(len: usize) -> Option<Self> {
        // A DS4 report always carries at least the first 37 bytes; anything
        // shorter is not ours.
        if len < 37 {
            return None;
        }
        // Bluetooth reports are 78 bytes, and everything shifts by three.
        if len >= 78 {
            Some(Layout {
                analog: 4,
                sensor: 26,
            })
        } else {
            Some(Layout {
                analog: 1,
                sensor: 23,
            })
        }
    }
}

/// Decode a raw HID report. Returns `None` if the buffer is not a DS4 report.
///
/// Bluetooth reports are 78 bytes; USB reports are 64. Anything at or above 78
/// bytes is treated as Bluetooth. `usb_report_id` is not required because both
/// transports share report id `0x01`.
pub fn parse(buf: &[u8]) -> Option<Ds4Report> {
    if buf.len() < 14 || buf[0] != 0x01 {
        return None;
    }
    let layout = Layout::for_len(buf.len())?;
    let a = layout.analog;

    // Sticks: centre 128, up to 127 counts either side.
    let raw_lx = buf[a];
    let raw_ly = buf[a + 1];
    let raw_rx = buf[a + 2];
    let raw_ry = buf[a + 3];

    // Byte 4 of the analog block: D-pad in the high nibble, face buttons in the low.
    let face = buf[a + 4];
    // Byte 5 is the shoulder cluster; byte 6 is the digital L2/R2 pair.
    let shoulder = buf[a + 5];
    let triggers = buf[a + 6];
    // Byte 7 is the analog trigger the finger is not resting on; byte 8 is the
    // one being touched. Take the max so a resting finger on L1 still shows up.
    let l2 = buf[a + 7].max(triggers & 0x0F);
    let r2 = buf[a + 8].max((triggers >> 4) & 0x0F);

    let mut bits = 0u16;
    if face & 0x01 != 0 {
        bits |= Buttons::CROSS;
    }
    if face & 0x02 != 0 {
        bits |= Buttons::CIRCLE;
    }
    if face & 0x04 != 0 {
        bits |= Buttons::SQUARE;
    }
    if face & 0x08 != 0 {
        bits |= Buttons::TRIANGLE;
    }
    if face & 0x10 != 0 {
        bits |= Buttons::UP;
    }
    if face & 0x20 != 0 {
        bits |= Buttons::DOWN;
    }
    if face & 0x40 != 0 {
        bits |= Buttons::LEFT;
    }
    if face & 0x80 != 0 {
        bits |= Buttons::RIGHT;
    }
    if shoulder & 0x01 != 0 {
        bits |= Buttons::L1;
    }
    if shoulder & 0x02 != 0 {
        bits |= Buttons::R1;
    }
    if shoulder & 0x04 != 0 {
        bits |= Buttons::R3;
    }
    if shoulder & 0x08 != 0 {
        bits |= Buttons::L3;
    }
    if shoulder & 0x10 != 0 {
        bits |= Buttons::SHARE;
    }
    if shoulder & 0x20 != 0 {
        bits |= Buttons::OPTIONS;
    }
    if l2 > 0 {
        bits |= Buttons::L2;
    }
    if r2 > 0 {
        bits |= Buttons::R2;
    }

    // Touchpad, at a + 9 .. a + 11.
    let touch_flags = buf[a + 9];
    let raw_tx = buf[a + 10];
    let raw_ty = buf[a + 11];
    let touch = TouchState {
        pad_touched: touch_flags & 0x01 != 0,
        pad_clicked: touch_flags & 0x02 != 0,
        x: norm_touch(raw_tx),
        y: norm_touch(raw_ty),
        raw_x: raw_tx,
        raw_y: raw_ty,
    };

    // Motion sensors live right after a 10-byte multitouch block.
    let s = layout.sensor;
    let (gyro, accel) = if s + 12 <= buf.len() {
        (
            Gyro {
                pitch: i16le(&buf[s + 6..s + 8]) as f32 * GYRO_SCALE,
                yaw: i16le(&buf[s + 8..s + 10]) as f32 * GYRO_SCALE,
                roll: i16le(&buf[s + 10..s + 12]) as f32 * GYRO_SCALE,
            },
            Accel {
                x: i16le(&buf[s..s + 2]) as f32 * ACCEL_SCALE,
                y: i16le(&buf[s + 2..s + 4]) as f32 * ACCEL_SCALE,
                z: i16le(&buf[s + 4..s + 6]) as f32 * ACCEL_SCALE,
            },
        )
    } else {
        (Gyro::default(), Accel::default())
    };

    Some(Ds4Report {
        buttons: Buttons(bits),
        left_x: axis(raw_lx),
        // Y is inverted so that "up" is positive, matching XInput.
        left_y: -axis(raw_ly),
        right_x: axis(raw_rx),
        right_y: -axis(raw_ry),
        raw_left_x: raw_lx,
        raw_left_y: raw_ly,
        raw_right_x: raw_rx,
        raw_right_y: raw_ry,
        l2: l2 as f32 / 255.0,
        r2: r2 as f32 / 255.0,
        gyro,
        accel,
        touch,
        battery: Battery::default(),
        fresh: true,
    })
}

/// Merge a battery level read from a feature report into an existing frame.
pub fn apply_battery(report: &mut Ds4Report, raw: u8) {
    report.battery = Battery {
        level: raw & 0x0F,
        charging: raw & 0x10 != 0,
    };
}

/// Map a raw 0..=255 stick byte onto -1..=1.
///
/// The two halves are scaled separately: there are 128 counts below centre but
/// only 127 above it, so a single divisor would leave one direction slightly
/// short of full deflection.
fn axis(raw: u8) -> f32 {
    let v = raw as i32 - AXIS_MIDPOINT as i32;
    let out = if v > 0 {
        v as f32 / AXIS_HALF_RANGE as f32
    } else {
        v as f32 / AXIS_MIDPOINT as f32
    };
    out.clamp(-1.0, 1.0)
}

fn norm_touch(raw: u8) -> f32 {
    (raw as f32 / TOUCHPAD_ACTIVE_MAX as f32).clamp(0.0, 1.0)
}

fn i16le(b: &[u8]) -> i16 {
    if b.len() < 2 {
        return 0;
    }
    i16::from_le_bytes([b[0], b[1]])
}

/// Build the DS4 output report used for lightbar colour and rumble.
///
/// Offsets differ per transport, matching the input report layouts.
pub fn output_report(
    transport: Transport,
    red: u8,
    green: u8,
    blue: u8,
    rumble: Option<(u8, u8)>,
) -> Vec<u8> {
    // USB reports are 64 bytes, Bluetooth 78; the trailing bytes are unused.
    let len = match transport {
        Transport::Usb => 64,
        Transport::Bluetooth => 78,
    };
    let mut buf = vec![0u8; len];
    buf[0] = 0x05; // output report id
                   // RGB brightness runs from offset 1 on both transports.
    buf[1] = red;
    buf[2] = green;
    buf[3] = blue;
    // Bluetooth carries the same bytes at a different offset for the second
    // (unused) LED bank; keep it in sync so firmware never reads stale data.
    if transport == Transport::Bluetooth {
        buf[10] = red;
        buf[11] = green;
        buf[12] = blue;
    }
    if let Some((large, small)) = rumble {
        let (off, _) = match transport {
            Transport::Usb => (len - 1, ()),
            Transport::Bluetooth => (len - 1, ()),
        };
        // The two motors share one byte: low nibble = heavy (right), high nibble
        // = light (left).
        buf[off] = (large & 0x0F) | ((small & 0x0F) << 4);
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usb_report() -> Vec<u8> {
        let mut b = vec![0u8; 64];
        b[0] = 0x01;
        b[1] = 128; // LX centre
        b[2] = 128; // LY centre
        b[3] = 128;
        b[4] = 128;
        b[5] = 0x00; // no face buttons
        b[6] = 0x00;
        b[7] = 0x00;
        b
    }

    #[test]
    fn parses_neutral_usb_report() {
        let r = parse(&usb_report()).expect("should decode");
        assert!(r.buttons.raw() == 0);
        assert!((r.left_x).abs() < 0.01);
        assert!((r.left_y).abs() < 0.01);
    }

    #[test]
    fn face_buttons_map_to_bits() {
        let mut b = usb_report();
        b[5] = 0x01 | 0x40; // cross + dpad-left
        let r = parse(&b).unwrap();
        assert!(r.buttons.contains(Buttons::CROSS));
        assert!(r.buttons.contains(Buttons::LEFT));
        assert!(!r.buttons.any(Buttons::CIRCLE));
    }

    #[test]
    fn axes_scale_symmetrically() {
        let mut b = usb_report();
        b[1] = 255; // full right
        b[2] = 0; // full down -> +1 Y after inversion
        let r = parse(&b).unwrap();
        // 255 is one count beyond the 127-count half range, so the scale lands
        // slightly above 1.0 before clamping.
        assert!((r.left_x - 1.0).abs() < 0.02, "got {}", r.left_x);
        assert!((r.left_y - 1.0).abs() < 0.02, "got {}", r.left_y);
    }

    #[test]
    fn rejects_foreign_reports() {
        assert!(parse(&[0u8; 64]).is_none());
        assert!(parse(&[]).is_none());
        let mut b = usb_report();
        b[0] = 0x03;
        assert!(parse(&b).is_none());
    }

    #[test]
    fn edge_detection() {
        let before = Buttons(Buttons::CROSS);
        let after = Buttons(Buttons::CROSS | Buttons::TRIANGLE);

        // Triangle came down between the two samples.
        assert_eq!(after.just_pressed(before).raw(), Buttons::TRIANGLE);
        assert_eq!(before.just_pressed(after).raw(), 0);

        // Going the other way, Triangle came back up.
        assert_eq!(before.just_released(after).raw(), Buttons::TRIANGLE);
        assert_eq!(after.just_released(before).raw(), 0);
    }

    #[test]
    fn short_reports_are_rejected() {
        // Anything below the minimum length is not a DS4 report.
        assert!(parse(&[0x01; 36]).is_none());
        assert!(parse(&[0x01; 37]).is_some());
    }

    #[test]
    fn bluetooth_layout_uses_analog_offset_four() {
        let mut b = vec![0u8; 78];
        b[0] = 0x01;
        // Bluetooth sticks sit at offset 4, not 1.
        b[4] = 255;
        let r = parse(&b).expect("should decode");
        assert!((r.left_x - 1.0).abs() < 0.02, "got {}", r.left_x);
    }
}
