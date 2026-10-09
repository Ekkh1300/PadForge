//! Turning controller motion into real mouse input.
//!
//! This is a port of DS4Windows' `MouseCursor`, which is the reference
//! implementation for this problem and the reason gyro aiming feels right on a
//! DualShock 4. Two things are worth understanding before changing anything here.
//!
//! **The DS4 reports rotation *rate*, not angle.** There is no orientation state
//! anywhere: each report's angular velocity is turned into a position delta for
//! that report and discarded. Integrating to an angle instead is the intuitive
//! design and it is wrong here — it drifts, and it has to be re-zeroed whenever
//! the pad moves relative to the player.
//!
//! **The coefficients are calibrated against the raw report units**, which are
//! degrees per second in sixteenths (`1/16 deg/s` per count). So 90 deg/s of yaw
//! arrives as 1440, not 90. DS4Windows' `0.012` coefficient only produces sane
//! pixel counts against that scale; reusing the number with degrees-per-second
//! would make aim roughly sixteen times too slow.
//!
//! Sub-pixel motion is carried between reports rather than discarded, otherwise
//! slow aiming would round to zero pixels and the cursor would sit still.

use crate::report::Gyro;

/// One count in the DS4 gyro report is 1/16 deg/s.
///
/// The mouse coefficients are tuned against this raw scale, so everything in this
/// module works in counts-per-second rather than degrees-per-second.
pub const GYRO_COUNTS_PER_DEG_SEC: f32 = 16.0;

/// DS4Windows' `gyroMouseSensSettings.mouseCoefficient` for a DS4.
const GYRO_MOUSE_COEFFICIENT: f32 = 0.012;

/// The per-report constant kick added along the dominant axis, so that very slow
/// rotation still moves the cursor. Small enough to be invisible at speed.
const GYRO_MOUSE_OFFSET: f32 = 0.2;

/// `MouseCursor.TOUCHPAD_MOUSE_OFFSET`.
const TOUCHPAD_MOUSE_OFFSET: f32 = 0.015;

/// Deadzone in raw gyro counts (`MouseCursor.GYRO_MOUSE_DEADZONE`).
const GYRO_MOUSE_DEADZONE: i32 = 10;

/// Below these magnitudes the jitter curve is applied, to stop the cursor
/// shivering. Gyro and touchpad use different thresholds.
const GYRO_JITTER_THRESHOLD: f32 = 0.26;
const TOUCHPAD_JITTER_THRESHOLD: f32 = 0.15;

/// Exponent of the jitter curve.
const JITTER_EXPONENT: f32 = 1.408;

/// Sensitivity is stored as a percentage, matching DS4Windows, so 100 is neutral.
pub const SENSITIVITY_NEUTRAL: f32 = 100.0;

/// Which gyro axis drives the horizontal pointer axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GyroPointerAxis {
    /// Yaw (the natural choice: tilting the pad left and right).
    Yaw,
    /// Roll (tilting the pad forward and back, like steering).
    Roll,
}

/// Which pointer axis to invert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerInvert {
    None,
    Horizontal,
    Vertical,
    Both,
}

impl PointerInvert {
    pub fn horizontal(self) -> bool {
        matches!(self, Self::Horizontal | Self::Both)
    }

    pub fn vertical(self) -> bool {
        matches!(self, Self::Vertical | Self::Both)
    }
}

/// How sub-threshold jitter is reshaped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JitterCompensation {
    /// Power curve that suppresses small movements. This is DS4Windows' default
    /// and it is what stops a resting pad from creeping.
    Power,
    Off,
}

/// Settings shared by both pointer paths.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PointerConfig {
    /// Gyro speed as a percentage; 100 is DS4Windows' default.
    pub gyro_sensitivity: f32,
    /// Which gyro axis is horizontal.
    pub gyro_axis: GyroPointerAxis,
    /// Extra vertical scale as a percentage; 100 leaves it alone.
    pub gyro_vertical_scale: f32,
    /// Touchpad speed as a percentage; 100 is DS4Windows' default and passes raw
    /// touchpad counts through as pixels one to one.
    pub touch_sensitivity: f32,
    /// Ignore pointer motion smaller than this magnitude. 1.0 means "never
    /// swallow motion", which is DS4Windows' default.
    pub min_threshold: f32,
    pub jitter: JitterCompensation,
    pub invert: PointerInvert,
    /// Rotate the touchpad by this many radians before use.
    pub touch_rotation: f32,
}

impl Default for PointerConfig {
    fn default() -> Self {
        Self {
            gyro_sensitivity: SENSITIVITY_NEUTRAL,
            gyro_axis: GyroPointerAxis::Yaw,
            gyro_vertical_scale: SENSITIVITY_NEUTRAL,
            touch_sensitivity: SENSITIVITY_NEUTRAL,
            min_threshold: 1.0,
            jitter: JitterCompensation::Power,
            invert: PointerInvert::None,
            touch_rotation: 0.0,
        }
    }
}

/// Holds the fractional pixel carried between reports.
///
/// Both pointer paths share this, and each keeps its own pair: sub-pixel motion
/// is real motion, and throwing it away every frame is what makes naive
/// implementations stutter at low speeds.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Remainder {
    pub x: f64,
    pub y: f64,
}

impl Remainder {
    /// Forget the carry when motion stops or reverses, matching DS4Windows:
    /// a leftover from the other direction would be motion the player never made.
    fn reset_on_reversal(&mut self, motion_x: f64, motion_y: f64) {
        if motion_x == 0.0 || (self.x > 0.0) != (motion_x > 0.0) {
            self.x = 0.0;
        }
        if motion_y == 0.0 || (self.y > 0.0) != (motion_y > 0.0) {
            self.y = 0.0;
        }
    }
}

/// DS4Windows' `Mapping.remainderCutoff` for the `(x * 100, 1.0)` call site.
///
/// Quantizes to whole hundredths by truncating *toward zero*, which is what C#'s
/// `(int)` cast does. Truncating toward zero rather than flooring matters: for a
/// negative value the two differ by one, which would bias left and up.
fn remainder_cutoff(v: f64) -> f64 {
    let scaled = v * 100.0;
    v - scaled.trunc() / 100.0
}

/// The direction-cosine weighting DS4Windows applies.
///
/// A deadzone removed along the axis of travel should shrink as the motion turns
/// towards the other axis, and the constant offset kick should only ever be
/// applied along the dominant axis, otherwise a purely horizontal movement
/// would also drift vertically.
fn direction_weights(dx: f64, dy: f64) -> (f64, f64) {
    let angle = (-dy).atan2(dx);
    (angle.cos().abs(), angle.sin().abs())
}

/// Apply the jitter curve, or return `motion` unchanged when it is off.
fn apply_jitter(motion: f64, sign: f64, weight: f64, threshold: f32) -> f64 {
    let magnitude = motion.abs();
    let limit = weight * threshold as f64;
    if magnitude <= limit && limit > 0.0 {
        sign * (magnitude / threshold as f64).powf(JITTER_EXPONENT as f64) * limit
    } else {
        motion
    }
}

/// Turn a floating-point pixel motion into an integer delta, carrying the
/// fraction forward.
///
/// `min_threshold` is a magnitude gate: below it nothing is emitted at all and
/// the whole motion is carried, which is how DS4Windows suppresses micro-drift.
fn quantise(
    motion_x: f64,
    motion_y: f64,
    min_threshold: f64,
    remainder: &mut Remainder,
) -> (i32, i32) {
    let motion_x = motion_x - remainder_cutoff(motion_x);
    let motion_y = motion_y - remainder_cutoff(motion_y);

    let magnitude_sq = motion_x * motion_x + motion_y * motion_y;
    let below_threshold = magnitude_sq < min_threshold * min_threshold;

    if below_threshold {
        remainder.x = motion_x;
        remainder.y = motion_y;
        (0, 0)
    } else {
        let x = motion_x.trunc();
        let y = motion_y.trunc();
        remainder.x = motion_x - x;
        remainder.y = motion_y - y;
        (x as i32, y as i32)
    }
}

/// Apply the configured inversion to a pixel delta.
fn apply_inversion(delta: (i32, i32), invert: PointerInvert) -> (i32, i32) {
    let (x, y) = delta;
    match invert {
        PointerInvert::None => (x, y),
        PointerInvert::Horizontal => (-x, y),
        PointerInvert::Vertical => (x, -y),
        PointerInvert::Both => (-x, -y),
    }
}

/// Gyro-to-pointer conversion, stateful across reports.
#[derive(Debug, Clone)]
pub struct GyroPointer {
    config: PointerConfig,
    remainder: Remainder,
}

impl GyroPointer {
    pub fn new(config: PointerConfig) -> Self {
        Self {
            config,
            remainder: Remainder::default(),
        }
    }

    pub fn set_config(&mut self, config: PointerConfig) {
        self.config = config;
    }

    pub fn reset(&mut self) {
        self.remainder = Remainder::default();
    }

    /// Convert one report's angular rates into a pixel delta.
    ///
    /// `dt` is the time since the previous report. The gyro path is the only one
    /// that needs it: because there is no integration, the rate has to be scaled
    /// by elapsed time to become a per-report displacement, and skipping that
    /// makes the pointer move faster on a fast-polling connection.
    pub fn process(&mut self, gyro: &Gyro, dt: f32) -> (i32, i32) {
        // Work in raw counts so the coefficients below match DS4Windows.
        let yaw = gyro.yaw * GYRO_COUNTS_PER_DEG_SEC;
        let pitch = gyro.pitch * GYRO_COUNTS_PER_DEG_SEC;
        let roll = gyro.roll * GYRO_COUNTS_PER_DEG_SEC;

        let mut dx = match self.config.gyro_axis {
            GyroPointerAxis::Yaw => yaw,
            GyroPointerAxis::Roll => roll,
        } as f64;
        // DS4Windows negates pitch here; `report` already flips Y so that
        // "up" is positive, so this restores the sign the formula expects.
        let mut dy = -pitch as f64;

        let elapsed = (dt as f64).clamp(0.0, 0.05);
        let time_scale = elapsed * 200.0;

        let coefficient =
            (self.config.gyro_sensitivity as f64 * 0.01) * GYRO_MOUSE_COEFFICIENT as f64;
        let vertical_scale = self.config.gyro_vertical_scale as f64 * 0.01;

        self.remainder.reset_on_reversal(dx, dy);

        let (weight_x, weight_y) = direction_weights(dx, dy);
        let sign_x = if dx > 0.0 {
            1.0
        } else if dx < 0.0 {
            -1.0
        } else {
            0.0
        };
        let sign_y = if dy > 0.0 {
            1.0
        } else if dy < 0.0 {
            -1.0
        } else {
            0.0
        };

        // Directional deadzone, in raw counts.
        let deadzone_x = (weight_x * GYRO_MOUSE_DEADZONE as f64) as i32;
        let deadzone_y = (weight_y * GYRO_MOUSE_DEADZONE as f64) as i32;
        dx = apply_deadzone(dx, sign_x as i32, deadzone_x);
        dy = apply_deadzone(dy, sign_y as i32, deadzone_y);

        let mut x_motion = if dx != 0.0 {
            coefficient * (dx * time_scale) + weight_x * (GYRO_MOUSE_OFFSET as f64 * sign_x)
        } else {
            0.0
        };
        let mut y_motion = if dy != 0.0 {
            (coefficient * vertical_scale) * (dy * time_scale)
                + weight_y * (GYRO_MOUSE_OFFSET as f64 * sign_y)
        } else {
            0.0
        };

        if self.config.jitter == JitterCompensation::Power {
            x_motion = apply_jitter(x_motion, sign_x, weight_x, GYRO_JITTER_THRESHOLD);
            y_motion = apply_jitter(y_motion, sign_y, weight_y, GYRO_JITTER_THRESHOLD);
        }

        // A null motion is not a reason to keep the carry: there is nothing to
        // carry it into.
        if x_motion == 0.0 {
            self.remainder.x = 0.0;
        }
        if y_motion == 0.0 {
            self.remainder.y = 0.0;
        }
        let x_motion = x_motion + self.remainder.x;
        let y_motion = y_motion + self.remainder.y;

        let delta = quantise(
            x_motion,
            y_motion,
            self.config.min_threshold as f64,
            &mut self.remainder,
        );
        apply_inversion(delta, self.config.invert)
    }
}

/// Subtract a deadzone, or zero the axis when it is inside it.
fn apply_deadzone(value: f64, sign: i32, deadzone: i32) -> f64 {
    if value.abs() > deadzone as f64 {
        value - sign as f64 * deadzone as f64
    } else {
        0.0
    }
}

/// Touchpad-to-pointer conversion, stateful across reports.
#[derive(Debug, Clone)]
pub struct TouchpadPointer {
    config: PointerConfig,
    remainder: Remainder,
}

impl TouchpadPointer {
    pub fn new(config: PointerConfig) -> Self {
        Self {
            config,
            remainder: Remainder::default(),
        }
    }

    pub fn set_config(&mut self, config: PointerConfig) {
        self.config = config;
    }

    pub fn reset(&mut self) {
        self.remainder = Remainder::default();
    }

    /// Convert one report's touchpad displacement into a pixel delta.
    ///
    /// Unlike the gyro path this takes a displacement, not a rate, so there is no
    /// elapsed-time term: a finger that crosses the same distance takes the same
    /// time at any polling rate, and the motion is already what the player did.
    pub fn process(&mut self, dx: f64, dy: f64) -> (i32, i32) {
        let (mut dx, mut dy) = if self.config.touch_rotation != 0.0 {
            // Rotation runs at f64 to match the displacement arithmetic.
            let angle = self.config.touch_rotation as f64;
            let (sin, cos) = angle.sin_cos();
            (dx * cos - dy * sin, dx * sin + dy * cos)
        } else {
            (dx, dy)
        };
        dx = dx.clamp(-1920.0, 1920.0);
        dy = dy.clamp(-942.0, 942.0);

        self.remainder.reset_on_reversal(dx, dy);

        let (weight_x, weight_y) = direction_weights(dx, dy);
        let sign_x = if dx > 0.0 {
            1.0
        } else if dx < 0.0 {
            -1.0
        } else {
            0.0
        };
        let sign_y = if dy > 0.0 {
            1.0
        } else if dy < 0.0 {
            -1.0
        } else {
            0.0
        };

        let coefficient = self.config.touch_sensitivity as f64 * 0.01;

        let mut x_motion = if dx != 0.0 {
            coefficient * dx + weight_x * (TOUCHPAD_MOUSE_OFFSET as f64 * sign_x)
        } else {
            0.0
        };
        let mut y_motion = if dy != 0.0 {
            coefficient * dy + weight_y * (TOUCHPAD_MOUSE_OFFSET as f64 * sign_y)
        } else {
            0.0
        };

        if self.config.jitter == JitterCompensation::Power {
            x_motion = apply_jitter(x_motion, sign_x, weight_x, TOUCHPAD_JITTER_THRESHOLD);
            y_motion = apply_jitter(y_motion, sign_y, weight_y, TOUCHPAD_JITTER_THRESHOLD);
        }

        // Carry in the same direction only, so a leftover from the other way
        // round cannot be spent on this movement.
        let same_direction = |motion: f64, carry: f64| {
            carry != 0.0 && motion != 0.0 && carry.is_sign_positive() == motion.is_sign_positive()
        };
        if same_direction(x_motion, self.remainder.x) {
            x_motion += self.remainder.x;
        }
        if same_direction(y_motion, self.remainder.y) {
            y_motion += self.remainder.y;
        }

        let motion = quantise(
            x_motion,
            y_motion,
            self.config.min_threshold as f64,
            &mut self.remainder,
        );

        apply_inversion(motion, self.config.invert)
    }
}

/// Inject synthetic mouse input through the real Windows input stack.
///
/// This goes through `SendInput`, not `mouse_event`, so the motion is
/// indistinguishable from real hardware to the receiving application: it is
/// subject to the same UIPI integrity checks and shows up in raw-input games the
/// same way a physical mouse would.
#[cfg(target_os = "windows")]
pub fn inject_relative_motion(dx: i32, dy: i32) -> std::io::Result<()> {
    if dx == 0 && dy == 0 {
        return Ok(());
    }
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_MOVE, MOUSEINPUT,
    };

    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE,
                // Zero lets the system stamp the event, so it is ordered against
                // other real input rather than arriving with a bogus time.
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };

    let sent = unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
    if sent == 1 {
        Ok(())
    } else {
        // `SendInput` returning 0 with no error is the documented way of saying
        // UIPI blocked it, which happens when this process is less trusted than
        // the foreground window.
        Err(std::io::Error::last_os_error())
    }
}

/// Press or release a mouse button.
#[cfg(target_os = "windows")]
pub fn inject_button(button: MouseButton, down: bool) -> std::io::Result<()> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
        MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
        MOUSEINPUT,
    };

    let flag = match (button, down) {
        (MouseButton::Left, true) => MOUSEEVENTF_LEFTDOWN,
        (MouseButton::Left, false) => MOUSEEVENTF_LEFTUP,
        (MouseButton::Right, true) => MOUSEEVENTF_RIGHTDOWN,
        (MouseButton::Right, false) => MOUSEEVENTF_RIGHTUP,
        (MouseButton::Middle, true) => MOUSEEVENTF_MIDDLEDOWN,
        (MouseButton::Middle, false) => MOUSEEVENTF_MIDDLEUP,
    };

    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: flag,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let sent = unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
    if sent == 1 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// The mouse buttons a controller can emulate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[cfg(not(target_os = "windows"))]
pub fn inject_relative_motion(_dx: i32, _dy: i32) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "pointer injection requires Windows",
    ))
}

#[cfg(not(target_os = "windows"))]
pub fn inject_button(_button: MouseButton, _down: bool) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "pointer injection requires Windows",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gyro(yaw: f32, pitch: f32, roll: f32) -> Gyro {
        Gyro { yaw, pitch, roll }
    }

    #[test]
    fn counts_per_degree_is_sixteen() {
        // If this ever changes, every gyro-to-mouse coefficient is wrong.
        assert_eq!(GYRO_COUNTS_PER_DEG_SEC, 16.0);
    }

    #[test]
    fn still_pad_produces_no_motion() {
        let mut p = GyroPointer::new(PointerConfig::default());
        let dt = 0.004;
        for _ in 0..200 {
            assert_eq!(p.process(&gyro(0.0, 0.0, 0.0), dt), (0, 0));
        }
    }

    #[test]
    fn right_yaw_moves_right() {
        let mut p = GyroPointer::new(PointerConfig::default());
        let (x, y) = p.process(&gyro(90.0, 0.0, 0.0), 0.004);
        assert!(x > 0, "expected rightward motion, got {x}");
        assert_eq!(y, 0, "yaw must not move the vertical axis");
    }

    /// DS4Windows computes `deltaY = -pitch`, so positive pitch moves the
    /// pointer *down*. This is reproduced deliberately rather than "corrected":
    /// the sign is part of what makes gyro aiming match DS4Windows, and the
    /// user-facing `invert` setting is how anyone changes it.
    #[test]
    fn pitch_sign_matches_ds4windows() {
        let mut p = GyroPointer::new(PointerConfig::default());
        let (x, y) = p.process(&gyro(0.0, 90.0, 0.0), 0.004);
        assert!(y < 0, "positive pitch should move down, got {y}");
        assert_eq!(x, 0);
        // And negative pitch moves up.
        assert!(p.process(&gyro(0.0, -90.0, 0.0), 0.004).1 > 0);
    }

    #[test]
    fn roll_can_replace_yaw() {
        let config = PointerConfig {
            gyro_axis: GyroPointerAxis::Roll,
            ..Default::default()
        };
        let mut p = GyroPointer::new(config);

        // Yaw alone must now do nothing on the horizontal axis.
        assert_eq!(p.process(&gyro(90.0, 0.0, 0.0), 0.004).0, 0);
        // Roll drives it instead.
        assert!(p.process(&gyro(0.0, 0.0, 90.0), 0.004).0 > 0);
    }

    #[test]
    fn sensitivity_scales_motion() {
        let measure = |sensitivity: f32| {
            let config = PointerConfig {
                gyro_sensitivity: sensitivity,
                jitter: JitterCompensation::Off,
                ..Default::default()
            };
            let mut p = GyroPointer::new(config);
            p.process(&gyro(90.0, 0.0, 0.0), 0.004).0
        };
        let normal = measure(100.0);
        let double = measure(200.0);
        assert!(normal > 0);
        assert!(
            double as f64 > normal as f64 * 1.8,
            "doubling sensitivity should roughly double motion: {normal} -> {double}"
        );
    }

    #[test]
    fn slow_rotation_still_accumulates_into_motion() {
        // The whole point of the remainder carry: below one pixel per report, a
        // naive implementation would emit nothing forever.
        let mut p = GyroPointer::new(PointerConfig::default());
        let mut total = 0;
        // 1 deg/s for a quarter second at 250 Hz.
        for _ in 0..62 {
            total += p.process(&gyro(1.0, 0.0, 0.0), 0.004).0;
        }
        assert!(
            total > 0,
            "sub-pixel motion must be carried, not discarded (got {total})"
        );
    }

    #[test]
    fn rotation_reverses_direction() {
        let mut p = GyroPointer::new(PointerConfig::default());
        assert!(p.process(&gyro(90.0, 0.0, 0.0), 0.004).0 > 0);
        assert!(p.process(&gyro(-90.0, 0.0, 0.0), 0.004).0 < 0);
    }

    #[test]
    fn direction_change_carries_no_stale_motion() {
        // A leftover fraction from a previous direction must not be spent on a
        // different movement.
        let mut p = GyroPointer::new(PointerConfig::default());
        for _ in 0..30 {
            p.process(&gyro(3.0, 0.0, 0.0), 0.004);
        }
        let (x, y) = p.process(&gyro(0.0, 90.0, 0.0), 0.004);
        assert_eq!(x, 0, "a vertical movement must not emit horizontal motion");
        assert!(y < 0, "expected vertical motion, got {y}");
    }

    #[test]
    fn inversion_flips_the_right_axis() {
        let config = PointerConfig {
            invert: PointerInvert::Horizontal,
            ..Default::default()
        };
        let mut p = GyroPointer::new(config);
        let (x, _) = p.process(&gyro(90.0, 0.0, 0.0), 0.004);
        assert!(x < 0, "horizontal inversion should move left, got {x}");
    }

    #[test]
    fn min_threshold_swallow_suppresses_micro_motion() {
        let config = PointerConfig {
            min_threshold: 1000.0,
            ..Default::default()
        };
        let mut p = GyroPointer::new(config);
        for _ in 0..20 {
            assert_eq!(p.process(&gyro(1.0, 0.0, 0.0), 0.004), (0, 0));
        }
    }

    /// Below the threshold the curve must shrink motion without ever flipping
    /// its sign, or a resting pad would drift instead of sitting still.
    #[test]
    fn jitter_curve_shrinks_but_preserves_sign() {
        let threshold = GYRO_JITTER_THRESHOLD as f64;
        for magnitude in [0.01, 0.05, 0.1, 0.2, threshold * 0.99] {
            for (motion, sign) in [(magnitude, 1.0), (magnitude, -1.0)] {
                let out = apply_jitter(motion, sign, 1.0, GYRO_JITTER_THRESHOLD);
                assert!(
                    out.abs() < magnitude,
                    "{magnitude} should shrink, got {out}"
                );
                assert_eq!(out.is_sign_positive(), sign > 0.0, "sign flipped");
            }
        }
        // At or above the threshold the curve must not touch the motion.
        let above = threshold * 1.5;
        assert!((apply_jitter(above, 1.0, 1.0, GYRO_JITTER_THRESHOLD) - above).abs() < 1e-9);
    }

    #[test]
    fn jitter_weight_scales_the_limit() {
        // A motion along the diagonal gets a proportionally smaller limit, so the
        // deadzone shape follows the direction of travel.
        let motion = 0.05;
        let along_x = apply_jitter(motion, 1.0, 1.0, TOUCHPAD_JITTER_THRESHOLD);
        let along_y = apply_jitter(motion, 1.0, 0.25, TOUCHPAD_JITTER_THRESHOLD);
        assert!(
            along_y > along_x,
            "a smaller weight should leave more motion: {along_x} vs {along_y}"
        );
    }

    /// The gyro reports a *rate*, so the per-report displacement has to scale
    /// with elapsed time. Halving `dt` must halve the motion of one report.
    #[test]
    fn motion_per_report_scales_linearly_with_dt() {
        let measure = |dt: f32| {
            let config = PointerConfig {
                min_threshold: 0.0,
                jitter: JitterCompensation::Off,
                ..Default::default()
            };
            let mut p = GyroPointer::new(config);
            p.process(&gyro(120.0, 0.0, 0.0), dt).0 as f64
        };
        let fast = measure(0.002);
        let slow = measure(0.004);
        assert!(fast > 0.0 && slow > 0.0, "both rates must produce motion");
        // Halving `dt` should halve the motion. It is slightly more than half,
        // because the constant offset does not scale with time.
        let ratio = fast / slow;
        assert!(
            (0.45..=0.62).contains(&ratio),
            "halving dt should roughly halve the motion, got {ratio:.2}x"
        );
    }

    /// Total motion over a fixed duration should be broadly comparable across
    /// report rates. It is not exact, and deliberately so: the constant offset is
    /// added once per report, so a higher report rate adds more of it. DS4Windows
    /// behaves the same way.
    #[test]
    fn total_motion_is_broadly_rate_independent() {
        let total = |dt: f32| {
            let mut p = GyroPointer::new(PointerConfig::default());
            let reports = (0.2 / dt) as i32;
            let mut sum = 0;
            for _ in 0..reports {
                sum += p.process(&gyro(60.0, 0.0, 0.0), dt).0;
            }
            sum
        };
        let fast = total(0.002);
        let slow = total(0.008);
        assert!(fast > 0 && slow > 0);
        let ratio = slow as f64 / fast as f64;
        assert!(
            (0.5..=1.5).contains(&ratio),
            "total motion over the same duration should be comparable, got {ratio:.2}x"
        );
    }

    #[test]
    fn touchpad_counts_pass_through_at_neutral_sensitivity() {
        let mut p = TouchpadPointer::new(PointerConfig::default());
        // At 100% the coefficient is exactly 1.0, so a raw displacement is the
        // same number of pixels.
        let (x, y) = p.process(10.0, -4.0);
        assert!((x - 10).abs() <= 1, "got {x}");
        assert!((y + 4).abs() <= 1, "got {y}");
    }

    #[test]
    fn touchpad_sensitivity_scales() {
        let config = PointerConfig {
            touch_sensitivity: 200.0,
            jitter: JitterCompensation::Off,
            ..Default::default()
        };
        let mut p = TouchpadPointer::new(config);
        let (x, _) = p.process(10.0, 0.0);
        assert!(x >= 19, "doubling should roughly double, got {x}");
    }

    #[test]
    fn touchpad_is_not_time_scaled() {
        // A displacement is already what the player did; a pointer fed the same
        // displacement twice must move twice as far, unlike the gyro path.
        let mut p = TouchpadPointer::new(PointerConfig::default());
        let once = p.process(8.0, 0.0).0;
        let twice = p.process(8.0, 0.0).0 + p.process(8.0, 0.0).0;
        assert!(once > 0);
        assert!(
            (twice - (once * 2)).abs() <= 2,
            "expected ~{expected}, got {twice}",
            expected = once * 2
        );
    }

    #[test]
    fn touchpad_rotation_swaps_axes() {
        let config = PointerConfig {
            touch_rotation: std::f32::consts::FRAC_PI_2,
            ..Default::default()
        };
        let mut p = TouchpadPointer::new(config);
        let (x, y) = p.process(10.0, 0.0);
        assert!(
            x.abs() <= 1,
            "a quarter turn should leave no horizontal motion, got {x}"
        );
        assert!(
            (y - 10).abs() <= 1,
            "a quarter turn should move fully vertical, got {y}"
        );
    }

    #[test]
    fn remainder_cutoff_truncates_towards_zero() {
        // C#'s `(int)` cast truncates toward zero; flooring here would bias
        // negative motion by a whole hundredth.
        // 1.239 quantises down to 1.23, leaving 0.009.
        assert!((remainder_cutoff(1.239) - 0.009).abs() < 1e-9);
        assert!((remainder_cutoff(-1.239) - (-0.009)).abs() < 1e-9);
        assert!(remainder_cutoff(2.0).abs() < 1e-9);
        assert!(remainder_cutoff(-2.0).abs() < 1e-9);
    }

    #[test]
    fn deadzone_is_directional() {
        // Purely horizontal motion must not be suppressed by the vertical part of
        // the deadzone, and vice versa.
        let mut p = GyroPointer::new(PointerConfig::default());
        let (x, y) = p.process(&gyro(90.0, 0.0, 0.0), 0.004);
        assert!(x > 0 && y == 0, "got ({x}, {y})");
    }
}
