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
///
/// Selected by the leading report id rather than by length. Length is not a
/// usable discriminator: Windows pads every HID read to the device's declared
/// input report length, so a Bluetooth report arrives as 128 bytes whether the
/// payload is 78 or 97. That made a length check pick the Bluetooth layout for
/// USB reads on any machine where the two differ.
#[derive(Debug, Clone, Copy)]
struct Layout {
    analog: usize,
    sensor: usize,
}

/// Report id the pad sends over USB.
const REPORT_ID_USB: u8 = 0x01;
/// Report id the pad sends over Bluetooth.
///
/// Not the same as USB despite both being "input report 1". This is the value
/// Sony uses in Bluetooth mode and is what DS4Windows also insists on. A decoder
/// that only accepts 0x01 therefore sees a Bluetooth pad as silent, which looks
/// identical to a pad that is connected but not reporting.
const REPORT_ID_BT: u8 = 0x11;

impl Layout {
    /// Pick the layout from the report id at byte 0.
    fn for_id(id: u8) -> Option<Self> {
        match id {
            REPORT_ID_BT => Some(Layout {
                // DS4Windows skips the first two bytes of a Bluetooth report
                // (report id, then a CRC/flags byte) before applying the same
                // field layout it uses for USB.
                analog: 3,
                sensor: 15,
            }),
            REPORT_ID_USB => Some(Layout {
                analog: 1,
                sensor: 13,
            }),
            _ => None,
        }
    }
}

/// Decode a raw HID report. Returns `None` if the buffer is not a DS4 report.
///
/// Both transports are accepted, selected by the leading report id: `0x01` over
/// USB and `0x11` over Bluetooth. The two are not interchangeable, and a decoder
/// that only knows `0x01` silently discards every Bluetooth frame — a pad that
/// enumerates, connects, and is then treated as not reporting.
///
/// Length is deliberately not consulted. Windows pads a HID read out to the
/// device's declared input report length, so a Bluetooth payload arrives in a
/// 128-byte buffer regardless of its real size, and a length-based check picks
/// the wrong layout for whichever transport happens to have the longer
/// declaration.
pub fn parse(buf: &[u8]) -> Option<Ds4Report> {
    // 37 bytes is the smallest a report can be and still carry every field,
    // including the sensors.
    if buf.len() < 37 {
        return None;
    }
    let layout = Layout::for_id(buf[0])?;
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

    // Touchpad. The flags byte is the same one that carries PS and the frame
    // counter, so it is read from its own offset rather than from inside the
    // analog block, where it would alias the d-pad.
    //
    // Byte 9 of the analog block holds the touch flags, and the two 7-bit
    // coordinates follow it. Each coordinate is 12 bits, packed 8-4 across a
    // byte boundary rather than 8-8, which is why the low nibble of the middle
    // byte belongs to X and the high nibble to Y.
    let touch_flags = buf[a + 9];
    let (raw_tx, raw_ty) = if a + 12 <= buf.len() {
        // 12-bit value: high nibble then low byte.
        let x = ((buf[a + 11] & 0x0F) as u16) << 8 | buf[a + 10] as u16;
        let y = (buf[a + 12] as u16) << 4 | ((buf[a + 11] >> 4) as u16);
        (x.min(0xFFF) as u8, y.min(0xFFF) as u8)
    } else {
        (0, 0)
    };
    let touch = TouchState {
        pad_touched: touch_flags & 0x80 != 0,
        pad_clicked: touch_flags & 0x01 != 0,
        x: norm_touch(raw_tx),
        y: norm_touch(raw_ty),
        raw_x: raw_tx,
        raw_y: raw_ty,
    };

    // Motion sensors, gyro first and accelerometer six bytes after it.
    //
    // The order is not interchangeable, and reading it the other way round
    // produces no error at all: both fields decode cleanly from the wrong bytes
    // and simply report plausible-looking nonsense. On the captured frame it
    // gave an accelerometer reading 0.003 g while the pad was lying still, which
    // is not a state a pad can be in. Only the magnitude check catches it.
    let s = layout.sensor;
    let (gyro, accel) = if s + 12 <= buf.len() {
        (
            Gyro {
                yaw: i16le(&buf[s..s + 2]) as f32 * GYRO_SCALE,
                pitch: i16le(&buf[s + 2..s + 4]) as f32 * GYRO_SCALE,
                roll: i16le(&buf[s + 4..s + 6]) as f32 * GYRO_SCALE,
            },
            Accel {
                x: i16le(&buf[s + 6..s + 8]) as f32 * ACCEL_SCALE,
                y: i16le(&buf[s + 8..s + 10]) as f32 * ACCEL_SCALE,
                z: i16le(&buf[s + 10..s + 12]) as f32 * ACCEL_SCALE,
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
        // Two separate bytes, one per motor, each a full 0..=255. They are not
        // nibbles sharing a byte, and they are not at the end of the report.
        //
        // Both of those were wrong here, and both failed quietly: masking to four
        // bits capped every rumble at 6% strength, so a full-throttle rumble felt
        // like a tap, and writing to the last byte of the buffer put the value
        // where the pad does not read it, so it vibrated not at all. Neither
        // produced an error; the first just felt weak and the second felt absent.
        //
        // Offsets confirmed against DS4Windows, which writes these at [6] and [7]
        // over Bluetooth and [4] and [5] over USB.
        let (fast, slow) = match transport {
            Transport::Usb => (4usize, 5usize),
            Transport::Bluetooth => (6usize, 7usize),
        };
        buf[fast] = large;
        buf[slow] = small;
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

    /// The two motors are full bytes at fixed offsets, one per transport.
    ///
    /// Both facts were wrong at once. The value was masked to four bits, which
    /// capped every rumble at 6% strength, and it was written to the last byte of
    /// the buffer, which is not where the pad reads it. Neither raised an error:
    /// the first just felt weak, the second did nothing at all.
    #[test]
    fn rumble_uses_full_bytes_at_the_right_offsets() {
        for (transport, fast, slow) in [
            (Transport::Usb, 4usize, 5usize),
            (Transport::Bluetooth, 6, 7),
        ] {
            let buf = output_report(transport, 0, 0, 0, Some((200, 100)));
            assert_eq!(
                buf[fast],
                200,
                "{}: the fast motor byte must carry the full value, not a masked one",
                transport.label()
            );
            assert_eq!(
                buf[slow],
                100,
                "{}: the slow motor byte must carry the full value",
                transport.label()
            );
        }
    }

    /// Full strength has to survive, because masking to a nibble is precisely
    /// what made a maximum rumble feel like a tap.
    #[test]
    fn full_rumble_is_not_clipped() {
        let buf = output_report(Transport::Bluetooth, 0, 0, 0, Some((255, 255)));
        assert_eq!(buf[6], 255, "a full rumble must reach the pad as 255");
        assert_eq!(buf[7], 255, "both motors, or the light one was masked too");
    }

    /// The rumble bytes must not collide with the colour bytes, or setting one
    /// silently disturbs the other.
    #[test]
    fn rumble_and_colour_do_not_overlap() {
        let buf = output_report(Transport::Bluetooth, 11, 22, 33, Some((44, 55)));
        assert_eq!((buf[1], buf[2], buf[3]), (11, 22, 33), "colour bytes");
        assert_eq!((buf[6], buf[7]), (44, 55), "rumble bytes");
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

    /// Verbatim 83-byte frame from a DS4 v2 (PID 0x09CC) over Bluetooth, with the
    /// trailing padding Windows adds to a HID read removed.
    ///
    /// These bytes came off the pad, not from a description of it. That
    /// distinction is the whole reason the three bugs below were findable at all:
    /// the tests were previously built from an assumption about the layout, the
    /// assumption was wrong, and the tests passed by confirming it. Regenerate
    /// with `cargo run -p padcore --bin capture-fixture`.
    const CAPTURED_BT_REPORT: [u8; 83] = [
        0x11, 0xc0, 0x00, 0x80, 0x80, 0x80, 0x80, 0x08, 0x00, 0x00, 0x00, 0x00, //
        0x00, 0x4b, 0x10, 0x07, 0x00, 0xf0, 0xff, 0x16, 0x00, 0xc0, 0xfe, 0x1f, //
        0x1f, 0x79, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, //
        0x00, 0x80, 0x00, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x80, 0x00, //
        0x00, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00, 0x80, //
        0x00, 0x00, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00, //
        0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0xa2, 0x21, 0x9e, 0x34,
    ];

    /// The captured frame as a buffer, padded out to `len` the way Windows
    /// delivers it.
    fn real_bluetooth_report(len: usize) -> Vec<u8> {
        let mut b = vec![0u8; len.max(CAPTURED_BT_REPORT.len())];
        b[..CAPTURED_BT_REPORT.len()].copy_from_slice(&CAPTURED_BT_REPORT);
        b.truncate(len.max(CAPTURED_BT_REPORT.len()));
        b
    }

    #[test]
    fn bluetooth_report_id_is_accepted() {
        // The pad sends 0x11 in Bluetooth mode. A decoder that only accepts 0x01
        // discards every frame, which presents as a pad that enumerates,
        // connects, and is then treated as not reporting at all.
        let b = real_bluetooth_report(83);
        assert_eq!(b[0], 0x11, "the device sends 0x11 over Bluetooth");
        assert!(parse(&b).is_some(), "a real Bluetooth report must decode");
    }

    #[test]
    fn bluetooth_sticks_sit_at_offset_three() {
        // The captured frame has every stick at 128, which is centred. Reading
        // from offset 4 picks up byte 7, the d-pad nibble, as the right stick.
        let mut b = real_bluetooth_report(83);
        let r = parse(&b).expect("should decode");
        for (name, v) in [
            ("left_x", r.left_x),
            ("left_y", r.left_y),
            ("right_x", r.right_x),
            ("right_y", r.right_y),
        ] {
            assert!(v.abs() < 0.02, "{name} should be centred, got {v}");
        }

        // Full right deflection, written at the offset the device uses.
        b[3] = 255;
        let r = parse(&b).expect("should decode");
        assert!((r.left_x - 1.0).abs() < 0.02, "got {}", r.left_x);
    }

    #[test]
    fn touch_flags_are_the_high_bit_not_the_low_bits() {
        // Bit 7 of the flags byte is "finger present", bit 0 is the click. The
        // two used to be swapped, so a resting pad read as touched and a press
        // read as a light tap.
        const FLAGS: usize = 3 + 9;

        let mut b = real_bluetooth_report(83);
        b[FLAGS] |= 0x80;
        let r = parse(&b).expect("should decode");
        assert!(r.touch.pad_touched, "bit 7 set means a finger is down");
        assert!(
            !r.touch.pad_clicked,
            "bit 0 is clear so the pad is not clicked"
        );

        b[FLAGS] = (b[FLAGS] & !0x80) | 0x01;
        let r = parse(&b).expect("should decode");
        assert!(!r.touch.pad_touched, "bit 7 clear means no finger");
        assert!(r.touch.pad_clicked, "bit 0 set means the pad is clicked");
    }

    #[test]
    fn report_length_does_not_decide_the_layout() {
        // Windows pads a HID read out to the device's declared input report
        // length, so one Bluetooth payload arrives at 83, 97 and 128 bytes
        // depending on which interface enumerated it. A length-based layout
        // check picks the wrong one for any length that does not happen to match.
        for len in [83usize, 97, 128] {
            let b = real_bluetooth_report(len);
            let r = parse(&b).unwrap_or_else(|| panic!("a {len}-byte report must decode"));
            assert!(
                r.left_x.abs() < 0.02,
                "{len} bytes: wrong layout, left_x = {}",
                r.left_x
            );
        }

        // A USB report padded out to 128 bytes must still read as USB.
        let mut usb = usb_report();
        usb.resize(128, 0);
        let r = parse(&usb).expect("should decode");
        assert!(r.left_x.abs() < 0.02, "left_x = {}", r.left_x);
    }

    #[test]
    fn sensors_decode_from_the_real_frame() {
        // The pad was lying still on a desk, so the accelerometer must read
        // about 1 g. Reading from the old offset gives 4 g, which is not a state
        // a pad can be in; reading from an empty region gives 0 g, which is not
        // either. Either way nothing crashed, which is why it went unnoticed.
        let b = real_bluetooth_report(83);
        let r = parse(&b).expect("should decode");

        let magnitude = (r.accel.x.powi(2) + r.accel.y.powi(2) + r.accel.z.powi(2)).sqrt();
        assert!(
            (0.7..=1.3).contains(&magnitude),
            "a still pad reads about 1 g, got {magnitude:.3} g from ({:.3}, {:.3}, {:.3})",
            r.accel.x,
            r.accel.y,
            r.accel.z
        );
        assert!(
            r.gyro.is_still(60.0),
            "gyro should be near rest, got ({:.1}, {:.1}, {:.1})",
            r.gyro.yaw,
            r.gyro.pitch,
            r.gyro.roll
        );
    }
}
