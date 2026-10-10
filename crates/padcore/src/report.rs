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

/// Battery charge level, as the pad reports it, plus its charging flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Battery {
    /// The raw nibble: 0 = empty, 8 = full while on battery, 11 = full while
    /// charging. It is not a percentage and the two ceilings differ, which is
    /// what [`Battery::fraction`] accounts for.
    pub level: u8,
    pub charging: bool,
}

impl Battery {
    /// Decode the status byte of an input report: level in the low nibble,
    /// charging in bit four.
    ///
    /// This is where DS4Windows reads it (offset 30 over USB). It used to come
    /// from a feature report instead, which never carried an answer, so the
    /// dashboard showed a flat pad whether or not one was plugged in.
    pub fn from_status(raw: u8) -> Self {
        Self {
            level: raw & 0x0F,
            charging: raw & 0x10 != 0,
        }
    }

    /// Remaining charge as a 0..=1 fraction.
    ///
    /// The pad counts to 8 on battery and to 11 on charge, so one divisor
    /// would under-read a plugged-in pad and over-read one that is not.
    pub fn fraction(&self) -> f32 {
        let ceiling = if self.charging { 11.0 } else { 8.0 };
        (self.level as f32 / ceiling).clamp(0.0, 1.0)
    }

    /// True when the pad is nearly empty and nothing is topping it up.
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
    /// Raw sensor coordinate as the pad sends it, 12 bits: X up to 1919, Y up
    /// to 941. Kept wide because the packed field is wide; truncating it to a
    /// byte loses the top bits and wraps, so a finger crossing the middle of the
    /// pad reports itself jumping back to the left edge.
    pub raw_x: u16,
    pub raw_y: u16,
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

/// The touchpad's usable coordinate range, one divisor per axis.
///
/// The surface is about twice as wide as it is tall, and a single divisor for
/// both squashes whichever axis it does not belong to: normalising Y by the X
/// range would put the entire vertical travel in the top half of the screen.
/// Both values are the ones the pointer code scales back up by (1920x942), so a
/// finger crossing the pad moves the cursor by the same distance it travelled.
const TOUCHPAD_ACTIVE_X: f32 = 1919.0;
const TOUCHPAD_ACTIVE_Y: f32 = 941.0;

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

    // Byte 4 of the analog block: a four-bit hat switch in the low nibble and the
    // four face buttons in the high one.
    let face = buf[a + 4];
    // Byte 5 is the shoulder cluster: L1, R1, the digital L2/R2 pair, Share,
    // Options, L3 and R3, one bit each from bit 0 upward.
    let shoulder = buf[a + 5];
    // Byte 6 is the button flags byte -- PS, touchpad click, and the frame
    // counter in its upper six bits. Bytes 7 and 8 are the analog triggers.
    //
    // Byte 6 used to be read as a second, coarser reading of the triggers and
    // OR-ed into them. It is not one, and the difference is not subtle: its
    // middle bits *are* the frame counter, so an untouched L2 counted along with
    // the poll rate between two and three percent, and because any non-zero
    // value also set the L2 button, a game saw the trigger half-held while the
    // pad sat on the desk with no finger near it. DS4Windows reads the trigger
    // from bytes 8 and 9 and nothing else, which is what this now does.
    let flags = buf[a + 6];
    let l2 = buf[a + 7];
    let r2 = buf[a + 8];

    let mut bits = 0u16;

    // The face buttons occupy the high nibble, in descending bit order from
    // Triangle down to Square. They used to be read from the low nibble, which
    // meant every face button was reported as the wrong one and the d-pad — the
    // four bits they were actually sitting on — was reported as four face
    // buttons.
    if face & 0x10 != 0 {
        bits |= Buttons::SQUARE;
    }
    if face & 0x20 != 0 {
        bits |= Buttons::CROSS;
    }
    if face & 0x40 != 0 {
        bits |= Buttons::CIRCLE;
    }
    if face & 0x80 != 0 {
        bits |= Buttons::TRIANGLE;
    }

    // The d-pad is one hat switch with nine positions, not four buttons. It runs
    // like a clock face — 0 is up, then clockwise in eighths of a turn — and 8
    // is the resting centre.
    //
    // Reading it as four independent bits was wrong in both directions: an
    // untouched pad reports 8, which has bit 3 set, so "right" was lit while the
    // pad sat on the desk, and the four diagonal positions could not be
    // expressed at all because they are single values rather than pairs of bits.
    match face & 0x0F {
        0 => bits |= Buttons::UP,
        1 => bits |= Buttons::UP | Buttons::RIGHT,
        2 => bits |= Buttons::RIGHT,
        3 => bits |= Buttons::DOWN | Buttons::RIGHT,
        4 => bits |= Buttons::DOWN,
        5 => bits |= Buttons::DOWN | Buttons::LEFT,
        6 => bits |= Buttons::LEFT,
        7 => bits |= Buttons::UP | Buttons::LEFT,
        // 8 is centred, and anything above it is the pad being held between
        // positions. Neither presses anything.
        _ => {}
    }
    if shoulder & 0x01 != 0 {
        bits |= Buttons::L1;
    }
    if shoulder & 0x02 != 0 {
        bits |= Buttons::R1;
    }
    if shoulder & 0x04 != 0 {
        bits |= Buttons::L2;
    }
    if shoulder & 0x08 != 0 {
        bits |= Buttons::R2;
    }
    if shoulder & 0x10 != 0 {
        bits |= Buttons::SHARE;
    }
    if shoulder & 0x20 != 0 {
        bits |= Buttons::OPTIONS;
    }
    if shoulder & 0x40 != 0 {
        bits |= Buttons::L3;
    }
    if shoulder & 0x80 != 0 {
        bits |= Buttons::R3;
    }

    // L2 and R2 as buttons, from either reading of them. The analog value is
    // the sensitive one and fires as soon as the trigger moves; the digital bit
    // is the hardware's own click, which arrives later but is there even if the
    // analog byte is stuck.
    //
    // Bits 2 and 3 used to be decoded as R3 and L3 instead, with the real L3/R3
    // -- bits 6 and 7, the last two in the byte -- never read at all. So pressing
    // a trigger reported a thumbstick click to the game, and clicking either
    // thumbstick did nothing.
    if l2 > 0 || shoulder & 0x04 != 0 {
        bits |= Buttons::L2;
    }
    if r2 > 0 || shoulder & 0x08 != 0 {
        bits |= Buttons::R2;
    }

    // Touchpad. The packet starts at byte 34 of the analog block -- byte 35
    // counting the report id -- and not at byte 9, where it used to be read.
    //
    // Bytes 9 and 10 are a 16-bit timestamp the pad stamps on every frame, so
    // the flags byte was following the clock: bit 7 is a timestamp bit, and bit 0
    // is the lowest bit of the counter, which toggles every other frame. The pad
    // therefore reported itself tapped a few times a second with nobody touching
    // it, and the coordinates came out of the gyro. The captured frame has 0x80
    // here -- idle -- and the touch block itself starts at 37 on Bluetooth,
    // which is what DS4Windows reads.
    //
    // Four bytes make up one touch: the flags byte, whose bit 7 set means *no*
    // finger is down, then X and Y as 12 bits each packed 8-4 across the byte
    // boundaries rather than 8-8, which is why the low nibble of the middle byte
    // belongs to X and the high nibble to Y.
    let touch_flags = if a + 34 < buf.len() {
        buf[a + 34]
    } else {
        0x80 // short buffer: report no finger rather than a phantom one
    };
    let (raw_tx, raw_ty) = if a + 37 < buf.len() {
        let mid = buf[a + 36];
        let x = ((mid & 0x0F) as u16) << 8 | buf[a + 35] as u16;
        let y = (buf[a + 37] as u16) << 4 | ((mid >> 4) as u16);
        (x.min(0xFFF), y.min(0xFFF))
    } else {
        (0, 0)
    };
    let touch = TouchState {
        pad_touched: touch_flags & 0x80 == 0,
        pad_clicked: flags & 0x02 != 0,
        x: norm_touch(raw_tx, TOUCHPAD_ACTIVE_X),
        y: norm_touch(raw_ty, TOUCHPAD_ACTIVE_Y),
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
        // DS4Windows reads the battery out of the input report (offset 30 over
        // USB); the status byte carries no frame of its own, so a device with
        // no battery data would still report something rather than nothing.
        battery: Battery::from_status(buf.get(a + 29).copied().unwrap_or(0)),
        fresh: true,
    })
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

fn norm_touch(raw: u16, axis_max: f32) -> f32 {
    (raw as f32 / axis_max).clamp(0.0, 1.0)
}

fn i16le(b: &[u8]) -> i16 {
    if b.len() < 2 {
        return 0;
    }
    i16::from_le_bytes([b[0], b[1]])
}

/// Length of the Bluetooth output report.
///
/// Every field the pad acts on lives in the first 78 bytes, and the last four
/// of those are the CRC. Nothing after them is read.
///
/// The number of bytes actually handed to the driver is not this, and is not
/// the application's to choose: hidapi pads the buffer to the output report
/// length the interface declares — 547 on this pad — whatever is passed in. So
/// a shorter or longer buffer here changes only how much zero padding travels
/// with an identical report.
pub const BT_PAYLOAD_LEN: usize = 78;

/// First byte of the CRC's input.
///
/// The pad drops any Bluetooth output report whose last four bytes are not a
/// CRC-32 over `[0xA2, report[..len - 4]]`. A write without it is accepted by
/// the driver and then ignored by the pad, which is indistinguishable in the UI
/// from a write that never happened.
const BT_CRC_SEED: u8 = 0xA2;

/// Where the Bluetooth report's CRC is stored, and how much it covers.
const BT_CRC_AT: usize = BT_PAYLOAD_LEN - 4;

/// Standard CRC-32: reflected, polynomial 0xEDB88320, initial and final XOR
/// 0xFFFFFFFF. The same CRC as zip, gzip and Ethernet, which is what the pad
/// asks for.
fn crc32(data: &[u8], seed: u32) -> u32 {
    let mut crc = seed;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    crc
}

/// Build the DS4 output report used for lightbar colour and rumble.
///
/// Offsets differ per transport, matching the input report layouts. The buffer
/// returned is the report and nothing else; the write path pads it.
pub fn output_report(
    transport: Transport,
    red: u8,
    green: u8,
    blue: u8,
    rumble: Option<(u8, u8)>,
) -> Vec<u8> {
    output_report_len(transport, 0, red, green, blue, rumble)
}

/// As [`output_report`], padded to at least `min_len` bytes.
///
/// The extra bytes are all zero and the pad reads none of them, so `min_len`
/// never changes what the pad does — only how much padding rides along. Kept
/// separate because the probes that write to the pad directly need to control
/// the exact size, and because testing that question honestly means being able
/// to vary it.
pub fn output_report_len(
    transport: Transport,
    write_len: usize,
    red: u8,
    green: u8,
    blue: u8,
    rumble: Option<(u8, u8)>,
) -> Vec<u8> {
    let payload = match transport {
        Transport::Usb => 64,
        Transport::Bluetooth => BT_PAYLOAD_LEN,
    };
    // Never truncate: a caller asking for fewer bytes than the payload has
    // misread the interface, and silently chopping the report would turn a
    // device that answers wrongly into one that answers never.
    let mut buf = vec![0u8; write_len.max(payload)];

    // The two transports do not share a single offset, so each field is placed
    // by name rather than by a tuple that has to be counted out.
    //
    // Layouts confirmed against DS4Windows, PrepareOutputReportInner. Both were
    // previously wrong in the same way: the colour sat where the feature mask
    // belongs and the mask was never written at all, so the USB pad was told
    // nothing about what to do with the bytes it was sent — which is why the
    // lightbar only ever worked over Bluetooth.
    struct Out {
        report_id: u8,
        /// The Bluetooth poll rate byte, which has no USB counterpart.
        poll: Option<u8>,
        /// Bit 0 enables the rumble, bit 1 the lightbar, bit 2 the flash. The
        /// pad reads this before it reads anything else, so leaving it zero
        /// turns every report that follows into an inert block of bytes.
        features: usize,
        reserved: usize,
        fast: usize,
        slow: usize,
        r: usize,
        g: usize,
        b: usize,
        flash_on: usize,
        flash_off: usize,
    }

    let out = match transport {
        Transport::Usb => Out {
            report_id: 0x05,
            poll: None,
            features: 1,
            reserved: 2,
            fast: 4,
            slow: 5,
            r: 6,
            g: 7,
            b: 8,
            flash_on: 9,
            flash_off: 10,
        },
        Transport::Bluetooth => Out {
            report_id: 0x11,
            // A rate code rather than a frequency: 0xC0 is 125 Hz, the pad's own
            // mode. Getting this wrong makes the pad ignore the rest of the
            // report, so it is set explicitly rather than left at zero.
            poll: Some(0xC0),
            features: 3,
            reserved: 4,
            fast: 6,
            slow: 7,
            r: 8,
            g: 9,
            b: 10,
            flash_on: 11,
            flash_off: 12,
        },
    };

    buf[0] = out.report_id;
    if let Some(rate) = out.poll {
        buf[1] = rate;
    }
    buf[out.features] = 0x07;
    buf[out.reserved] = 0x04;
    buf[out.r] = red;
    buf[out.g] = green;
    buf[out.b] = blue;
    // No flashing: the flash durations are a separate pair of bytes, and
    // leaving them at the pad's default is what keeps a steady colour steady.
    buf[out.flash_on] = 0;
    buf[out.flash_off] = 0;

    if let Some((heavy, fast_motor)) = rumble {
        // Full bytes, one per motor. They are not nibbles sharing a byte, and a
        // 0x0F mask capped every rumble at about 6% strength, so a full one felt
        // like a tap rather than a rumble.
        buf[out.fast] = fast_motor;
        buf[out.slow] = heavy;
    }

    if transport == Transport::Bluetooth {
        // The seal on the report. The driver forwards whatever it is given; the
        // pad is the one that checks the last four bytes, and a report without
        // them is dropped without a word. Written little-endian, which is the
        // order the pad reads it in.
        let crc = crc32(&[BT_CRC_SEED], 0xFFFF_FFFF);
        let crc = !crc32(&buf[..BT_CRC_AT], crc);
        buf[BT_CRC_AT] = crc as u8;
        buf[BT_CRC_AT + 1] = (crc >> 8) as u8;
        buf[BT_CRC_AT + 2] = (crc >> 16) as u8;
        buf[BT_CRC_AT + 3] = (crc >> 24) as u8;
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
        // 0x08 is the hat switch at rest. An idle pad sends this and nothing
        // else in the byte, so a fixture that zeroes it is describing a pad
        // someone is holding a direction on.
        b[5] = 0x08;
        b[6] = 0x00;
        b[7] = 0x00;
        b
    }

    /// The two transports use entirely different output layouts, and the
    /// Bluetooth one is the reason rumble appeared to work while doing nothing.
    ///
    /// Sending report 0x05 to a Bluetooth pad is rejected by the driver with
    /// ERROR_INVALID_PARAMETER, so nothing ever reached the hardware. The old code
    /// also put the colour bytes at the USB offsets and the motors at the end of
    /// the buffer, neither of which the pad reads in Bluetooth mode.
    #[test]
    fn bluetooth_output_uses_its_own_layout() {
        let buf = output_report(Transport::Bluetooth, 11, 22, 33, Some((44, 55)));
        assert_eq!(buf[0], 0x11, "Bluetooth output is report 0x11, not 0x05");
        assert_eq!(buf[3], 0x07, "rumble and lightbar must be enabled");
        assert_eq!((buf[6], buf[7]), (55, 44), "motors at 6 and 7");
        assert_eq!(
            (buf[8], buf[9], buf[10]),
            (11, 22, 33),
            "colour at 8, 9, 10"
        );
    }

    /// The feature mask has to be written on both transports.
    ///
    /// It used to be written on neither, and on USB it was then overwritten by
    /// the red channel, which had been placed at the mask's own offset. The pad
    /// was therefore told to enable nothing, and a lightbar command sent over
    /// USB arrived as a valid report the pad was instructed to ignore.
    #[test]
    fn both_transports_enable_rumble_and_lightbar() {
        for transport in [Transport::Usb, Transport::Bluetooth] {
            let buf = output_report(transport, 255, 0, 0, Some((255, 255)));
            let at = match transport {
                Transport::Usb => 1usize,
                Transport::Bluetooth => 3,
            };
            assert_eq!(
                buf[at],
                0x07,
                "{}: rumble, lightbar and flash must all be enabled",
                transport.label()
            );
        }
    }

    /// Each field of the outgoing report checked against where DS4Windows,
    /// PrepareOutputReportInner, puts it.
    ///
    /// The mask alone does not settle anything. A report can enable the motors
    /// and still address them somewhere the pad is not looking, which is exactly
    /// what happened when every value sat eight bytes early over Bluetooth: the
    /// mask was honoured, the lightbar moved, and the motors read zeros from a
    /// pair of bytes nothing had written to.
    #[test]
    fn outgoing_fields_land_where_the_pad_reads_them() {
        // Every value distinct, so a field read from a neighbour's offset cannot
        // pass by borrowing a value that happens to be right elsewhere.
        let bt = output_report(Transport::Bluetooth, 0x11, 0x22, 0x33, Some((0x44, 0x55)));

        assert_eq!(bt[0], 0x11, "Bluetooth output is report 0x11");
        assert_eq!(bt[1], 0xC0, "the rate code, not a raw frequency");
        assert_eq!(bt[4], 0x04, "the reserved byte the pad expects");
        assert_eq!((bt[6], bt[7]), (0x55, 0x44), "fast, then heavy");
        assert_eq!((bt[8], bt[9], bt[10]), (0x11, 0x22, 0x33), "colour follows");

        // Same report over USB, where nothing sits where it did over Bluetooth.
        let usb = output_report(Transport::Usb, 0x11, 0x22, 0x33, Some((0x44, 0x55)));
        assert_eq!(usb[0], 0x05, "USB output is report 0x05");
        assert_eq!(usb[2], 0x04, "the reserved byte moved with everything else");
        assert_eq!((usb[4], usb[5]), (0x55, 0x44), "motors stay at 4 and 5");
        assert_eq!(
            (usb[6], usb[7], usb[8]),
            (0x11, 0x22, 0x33),
            "colour at 6, 7, 8"
        );

        // The padding carries none of it, so asking for a longer write cannot
        // move a field: what the pad reads is the same either way.
        let long = output_report_len(
            Transport::Bluetooth,
            547,
            0x11,
            0x22,
            0x33,
            Some((0x44, 0x55)),
        );
        assert_eq!(
            long.len(),
            547,
            "padded to the length the interface declares"
        );
        assert_eq!(&long[..bt.len()], &bt[..], "padding changes no field");
    }

    /// Full strength has to survive, because masking to a nibble is what made a
    /// maximum rumble feel like a tap.
    #[test]
    fn full_rumble_is_not_clipped() {
        for (transport, fast, slow) in [
            (Transport::Usb, 4usize, 5usize),
            (Transport::Bluetooth, 6, 7),
        ] {
            let buf = output_report(transport, 0, 0, 0, Some((255, 255)));
            assert_eq!(
                buf[fast],
                255,
                "{}: fast motor must reach 255",
                transport.label()
            );
            assert_eq!(
                buf[slow],
                255,
                "{}: slow motor must reach 255",
                transport.label()
            );
        }
    }

    /// The rumble bytes must not collide with the colour bytes, or setting one
    /// silently disturbs the other.
    #[test]
    fn rumble_and_colour_do_not_overlap() {
        for (transport, fast, slow, r, g, b) in [
            (Transport::Usb, 4usize, 5usize, 6usize, 7usize, 8usize),
            (Transport::Bluetooth, 6, 7, 8, 9, 10),
        ] {
            let buf = output_report(transport, 11, 22, 33, Some((44, 55)));
            assert_eq!(
                (buf[r], buf[g], buf[b]),
                (11, 22, 33),
                "{}: colour",
                transport.label()
            );
            assert_eq!(
                (buf[fast], buf[slow]),
                (55, 44),
                "{}: rumble",
                transport.label()
            );
        }
    }

    /// The report must be exactly the length its protocol declares, because
    /// that is the part the pad parses. Anything beyond it is padding the write
    /// path supplies and the pad never reads.
    #[test]
    fn output_lengths_match_the_transport() {
        assert_eq!(output_report(Transport::Usb, 0, 0, 0, None).len(), 64);
        assert_eq!(output_report(Transport::Bluetooth, 0, 0, 0, None).len(), 78);
    }

    /// The Bluetooth report is only worth sending with a valid CRC, because the
    /// pad discards a report whose last four bytes do not match — and a
    /// discarded report and a never-sent report are indistinguishable from
    /// inside the application. This is what separates a lightbar that changes
    /// from one that silently ignores every command.
    ///
    /// The value is checked against the algorithm rather than a golden number
    /// copied from a successful run, so the test still says something if the
    /// inputs change.
    #[test]
    fn bluetooth_report_carries_a_valid_crc() {
        let buf = output_report(Transport::Bluetooth, 11, 22, 33, Some((44, 55)));

        // The pad seeds the CRC with a single leading byte, so hashing that
        // byte plus everything before the checksum must reproduce the checksum.
        let mut covered = vec![0xA2u8];
        covered.extend_from_slice(&buf[..BT_CRC_AT]);
        let computed = !crc32(&covered, 0xFFFF_FFFF);

        let stored = u32::from(buf[BT_CRC_AT])
            | (u32::from(buf[BT_CRC_AT + 1]) << 8)
            | (u32::from(buf[BT_CRC_AT + 2]) << 16)
            | (u32::from(buf[BT_CRC_AT + 3]) << 24);

        assert_eq!(
            stored, computed,
            "the CRC the pad checks must match the report it covers"
        );
        // The checksum lives in the last four bytes of the payload, not over it.
        assert_eq!(BT_CRC_AT + 4, BT_PAYLOAD_LEN);
    }

    /// The checksum has to cover the fields it protects: if the colour or the
    /// motors change, the stored CRC must change with them.
    #[test]
    fn bluetooth_crc_follows_the_report_contents() {
        let a = output_report(Transport::Bluetooth, 255, 0, 0, Some((255, 0)));
        let b = output_report(Transport::Bluetooth, 0, 255, 0, Some((0, 255)));
        assert_ne!(
            a[BT_CRC_AT..BT_CRC_AT + 4],
            b[BT_CRC_AT..BT_CRC_AT + 4],
            "two different reports must not carry the same checksum"
        );
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
        b[5] = 0x20 | 0x06; // cross (bit 5) and hat position 6, which is left
        let r = parse(&b).unwrap();
        assert!(r.buttons.contains(Buttons::CROSS));
        assert!(r.buttons.contains(Buttons::LEFT));
        assert!(!r.buttons.any(Buttons::CIRCLE));
    }

    /// The four face buttons live in the high nibble, one bit each, and every
    /// one has to land on its own button. They used to be read from the low
    /// nibble, which is where the d-pad lives, so Triangle lit up on a pad
    /// nobody was holding and Cross lit up when the d-pad was pushed left.
    #[test]
    fn every_face_button_lands_on_its_own_bit() {
        let face = Buttons::SQUARE | Buttons::CROSS | Buttons::CIRCLE | Buttons::TRIANGLE;
        for (mask, want) in [
            (0x10u8, Buttons::SQUARE),
            (0x20, Buttons::CROSS),
            (0x40, Buttons::CIRCLE),
            (0x80, Buttons::TRIANGLE),
        ] {
            let mut b = usb_report();
            b[5] = mask;
            let r = parse(&b).expect("should decode");
            assert!(
                r.buttons.contains(want) && (r.buttons.raw() & face) == want,
                "byte {mask:#04x} must be that button and no other"
            );
        }
    }

    /// The d-pad is a single hat switch with nine positions, running clockwise
    /// from up, with 8 as the resting centre.
    ///
    /// The four diagonals matter most here: they are single values on the wire,
    /// not two directions at once, so any decoder treating the d-pad as four
    /// independent buttons cannot express them however carefully it is written.
    #[test]
    fn the_hat_switch_reports_all_nine_positions() {
        let dpad = Buttons::UP | Buttons::DOWN | Buttons::LEFT | Buttons::RIGHT;
        for (hat, want) in [
            (0x00u8, Buttons::UP),
            (0x01, Buttons::UP | Buttons::RIGHT),
            (0x02, Buttons::RIGHT),
            (0x03, Buttons::DOWN | Buttons::RIGHT),
            (0x04, Buttons::DOWN),
            (0x05, Buttons::DOWN | Buttons::LEFT),
            (0x06, Buttons::LEFT),
            (0x07, Buttons::UP | Buttons::LEFT),
            (0x08, 0u16),
        ] {
            let mut b = usb_report();
            b[5] = hat;
            let r = parse(&b).expect("should decode");
            assert_eq!(
                r.buttons.raw() & dpad,
                want,
                "hat position {hat} must decode as {want}"
            );
        }
    }

    /// The frame the pad sends when nobody is touching it has to decode as
    /// untouched.
    ///
    /// This is the regression test for the whole button bug: the captured frame
    /// carries 0x08 in the button byte, and 0x08 has bit 3 set, so a decoder
    /// reading the hat as four buttons reported "right" on a pad sitting
    /// untouched — and, from the same byte, a face button that nobody pressed.
    #[test]
    fn an_idle_pad_holds_nothing_down() {
        let r = parse(&real_bluetooth_report(83)).expect("should decode");
        assert_eq!(r.buttons.raw(), 0, "an untouched pad presses nothing");
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
    fn an_idle_pad_reports_no_touch_and_no_click() {
        // The captured frame is a pad nobody is touching. It must decode as
        // untouched: bit 7 of the flags byte is set precisely when no finger is
        // down, and the click lives in the button byte, not in the flags.
        const FLAGS: usize = 3 + 34; // analog block is 3 on Bluetooth
        const BUTTONS: usize = 3 + 6; // PS / touchpad click / frame counter
        const TOUCH_X: usize = 3 + 35;
        const TOUCH_Y_HI: usize = 3 + 37;

        let b = real_bluetooth_report(83);
        assert_eq!(b[FLAGS], 0x80, "idle frame, no finger");
        let r = parse(&b).expect("should decode");
        assert!(!r.touch.pad_touched, "bit 7 set means no finger is down");
        assert!(!r.touch.pad_clicked, "the click button is not pressed");
        assert_eq!((r.touch.raw_x, r.touch.raw_y), (0, 0), "nothing on it");

        // A finger landing: the flag clears, the coordinates come with it.
        let mut b = b;
        b[FLAGS] = 0x00;
        b[TOUCH_X] = 0x21; // low byte of X
        b[FLAGS + 2] = 0x05; // X high nibble 5, Y low nibble 0
        b[TOUCH_Y_HI] = 0x03; // Y high byte
        let r = parse(&b).expect("should decode");
        assert!(r.touch.pad_touched, "bit 7 clear means a finger is down");
        assert!(!r.touch.pad_clicked, "the click button is separate");
        assert_eq!(r.touch.raw_x, (5 << 8) | 0x21, "X is 12 bits, packed 8-4");
        assert_eq!(
            r.touch.raw_y,
            3 << 4,
            "Y takes the high nibble of the middle byte"
        );
        assert!(r.touch.x > 0.0 && r.touch.x < 1.0, "x = {}", r.touch.x);
        assert!(r.touch.y > 0.0 && r.touch.y < 1.0, "y = {}", r.touch.y);

        // The click comes from byte 6, bit 1 -- the same byte as the PS button
        // and the frame counter, and the counter is what used to make the pad
        // tap by itself a few times a second.
        b[BUTTONS] |= 0x02;
        let r = parse(&b).expect("should decode");
        assert!(r.touch.pad_clicked, "button byte bit 1 is the click");
    }

    #[test]
    fn resting_triggers_are_zero() {
        // Byte 6 is the frame counter, not a coarse trigger. Reading it as one
        // left an untouched L2 sitting at two or three percent, moving with the
        // poll rate, while the same value counted as a half-held trigger to any
        // game. DS4Windows reads bytes 8 and 9 only.
        const L2: usize = 1 + 7;
        const R2: usize = 1 + 8;
        const BUTTONS: usize = 1 + 6;
        const SHOULDER: usize = 1 + 5;

        let mut b = [0u8; 64];
        b[0] = 0x01;
        b[1] = 128;
        b[2] = 128;
        b[3] = 128;
        b[4] = 128;
        b[5] = 0x08; // neutral d-pad
        b[L2] = 0;
        b[R2] = 0;
        for counter in [0u8, 1, 2, 3, 47, 48, 49, 255] {
            b[BUTTONS] = counter << 2; // how the pad packs the frame counter
            let r = parse(&b).expect("should decode");
            assert_eq!(r.l2, 0.0, "L2 idle, frame counter {counter}");
            assert_eq!(r.r2, 0.0, "R2 idle, frame counter {counter}");
            assert!(
                !r.buttons.any(Buttons::L2 | Buttons::R2),
                "no trigger held, frame counter {counter}"
            );
        }

        // A real press still reads, and the digital bit counts too.
        b[L2] = 255;
        let r = parse(&b).expect("should decode");
        assert_eq!(r.l2, 1.0, "L2 fully pressed");
        assert!(r.buttons.any(Buttons::L2), "analog sets the button");

        b[L2] = 0;
        b[SHOULDER] |= 0x04; // digital L2 click
        let r = parse(&b).expect("should decode");
        assert!(
            r.buttons.any(Buttons::L2),
            "the hardware's own click counts even if the analog byte is stuck"
        );
    }

    #[test]
    fn l3_and_r3_live_in_the_last_two_bits_of_the_shoulder_byte() {
        // Bits 2 and 3 are the digital L2/R2 pair. They used to be decoded as
        // R3 and L3, so pressing a trigger reported a thumbstick click while the
        // real L3/R3 -- bits 6 and 7 -- were never read at all.
        const SHOULDER: usize = 1 + 5;

        let mut b = [0u8; 64];
        b[0] = 0x01;
        b[1] = 128;
        b[2] = 128;
        b[3] = 128;
        b[4] = 128;
        b[5] = 0x08;

        b[SHOULDER] = 0x40;
        let r = parse(&b).expect("should decode");
        assert!(r.buttons.any(Buttons::L3), "bit 6 is L3");
        assert!(!r.buttons.any(Buttons::R3 | Buttons::R2), "not bit 7 or 3");

        b[SHOULDER] = 0x80;
        let r = parse(&b).expect("should decode");
        assert!(r.buttons.any(Buttons::R3), "bit 7 is R3");

        b[SHOULDER] = 0x0C; // digital L2 and R2
        let r = parse(&b).expect("should decode");
        assert!(
            !r.buttons.any(Buttons::L3 | Buttons::R3),
            "bits 2 and 3 are the triggers, not the stick clicks"
        );
        assert!(r.buttons.any(Buttons::L2 | Buttons::R2));
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
