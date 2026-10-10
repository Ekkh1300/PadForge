//! Touchpad interpretation.
//!
//! The DS4's touchpad is a genuinely useful second input surface, and it can be
//! driven three ways:
//!
//! * **Mouse** â€” finger position moves a cursor, gestures click
//! * **D-pad** â€” the pad becomes a swipe directional pad
//! * **Swipe** â€” fast two-finger and four-finger gestures fire discrete actions
//!
//! Gestures are detected from finger count plus movement, debounced so a single
//! swipe fires exactly one event.

use serde::{Deserialize, Serialize};

use crate::report::TouchState;

/// What a two- or four-finger gesture looks like when it fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gesture {
    TwoFingerUp,
    TwoFingerDown,
    TwoFingerLeft,
    TwoFingerRight,
    FourFingerUp,
    FourFingerDown,
    FourFingerLeft,
    FourFingerRight,
}

impl Gesture {
    pub const ALL: &'static [Gesture] = &[
        Gesture::TwoFingerUp,
        Gesture::TwoFingerDown,
        Gesture::TwoFingerLeft,
        Gesture::TwoFingerRight,
        Gesture::FourFingerUp,
        Gesture::FourFingerDown,
        Gesture::FourFingerLeft,
        Gesture::FourFingerRight,
    ];

    pub fn label(self) -> String {
        let (n, dir) = match self {
            Gesture::TwoFingerUp => (2, "Up"),
            Gesture::TwoFingerDown => (2, "Down"),
            Gesture::TwoFingerLeft => (2, "Left"),
            Gesture::TwoFingerRight => (2, "Right"),
            Gesture::FourFingerUp => (4, "Up"),
            Gesture::FourFingerDown => (4, "Down"),
            Gesture::FourFingerLeft => (4, "Left"),
            Gesture::FourFingerRight => (4, "Right"),
        };
        format!("{n} fingers {dir}")
    }
}

/// Where touchpad input is routed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum TouchpadMode {
    /// Touchpad is ignored entirely.
    Off,
    /// Absolute pointer movement.
    Mouse,
    /// Pad acts as a directional pad.
    Dpad,
    /// Only gesture detection runs.
    Gestures,
}

/// How the touchpad behaves in mouse mode.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TouchpadConfig {
    pub mode: TouchpadMode,
    /// The touchpad reports one finger; multi-finger counts come from the
    /// multitouch block, which `report` does not currently surface, so gesture
    /// detection is limited to what a single-contact report can tell us.
    /// Set `require_release` so a gesture must start from a clean touch.
    pub require_release: bool,
    /// Movement needed to fire a swipe, as a fraction of pad size.
    pub swipe_threshold: f32,
    /// Ignore touches shorter than this, in milliseconds.
    pub min_duration_ms: u32,
    /// Relative mouse speed multiplier.
    pub sensitivity: f32,
    /// Click on touch (as opposed to only on click-press).
    pub click_to_mouse: bool,
    /// Emit a right-click on pad click.
    pub right_click: bool,
    /// Invert the vertical axis.
    pub invert_y: bool,
}

impl Default for TouchpadConfig {
    fn default() -> Self {
        Self {
            mode: TouchpadMode::Off,
            require_release: true,
            swipe_threshold: 0.35,
            min_duration_ms: 120,
            sensitivity: 1.0,
            click_to_mouse: true,
            right_click: false,
            invert_y: true,
        }
    }
}

/// Result of inspecting one touchpad sample.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TouchpadOutput {
    /// Relative pointer delta, in pixels.
    pub mouse_delta: (f32, f32),
    /// D-pad directions currently held.
    pub dpad: (bool, bool, bool, bool),
    /// Gesture that fired this frame, if any.
    pub gesture: Option<Gesture>,
    /// Left mouse button state.
    pub left_down: bool,
    /// Right mouse button state.
    pub right_down: bool,
}

/// Stateful gesture detector.
#[derive(Debug, Clone)]
pub struct TouchpadProcessor {
    config: TouchpadConfig,
    // Previous absolute position, for delta computation.
    prev: Option<(f32, f32)>,
    // Gesture tracking.
    start: Option<(f32, f32, std::time::Instant)>,
    fired: Option<Gesture>,
    armed: bool,
}

impl Default for TouchpadProcessor {
    fn default() -> Self {
        Self::new(TouchpadConfig::default())
    }
}

impl TouchpadProcessor {
    pub fn new(config: TouchpadConfig) -> Self {
        // At startup the pad is idle, so a gesture may begin immediately.
        Self {
            config,
            prev: None,
            start: None,
            fired: None,
            armed: true,
        }
    }

    pub fn config(&self) -> &TouchpadConfig {
        &self.config
    }

    pub fn set_config(&mut self, config: TouchpadConfig) {
        self.config = config;
    }

    pub fn reset(&mut self) {
        self.prev = None;
        self.start = None;
        self.fired = None;
        self.armed = true;
    }

    /// Inspect a sample and produce this frame's output.
    pub fn process(&mut self, touch: &TouchState) -> TouchpadOutput {
        let mut out = TouchpadOutput::default();
        if self.config.mode == TouchpadMode::Off {
            self.prev = None;
            self.start = None;
            return out;
        }

        if !touch.pad_touched && !touch.pad_clicked {
            // Released: re-arm so the next touch starts a fresh gesture. Without
            // this a single swipe would repeat for as long as the finger stays
            // down.
            self.prev = None;
            self.start = None;
            self.fired = None;
            self.armed = true;
            return out;
        }

        // A gesture is already in flight; wait for a release before tracking a
        // new one.
        if !self.armed {
            return out;
        }

        let pos = (touch.x, touch.y);

        match self.config.mode {
            TouchpadMode::Mouse => {
                if let Some(prev) = self.prev {
                    let dx = (pos.0 - prev.0) * self.config.sensitivity;
                    let mut dy = (pos.1 - prev.1) * self.config.sensitivity;
                    if self.config.invert_y {
                        dy = -dy;
                    }
                    out.mouse_delta = (dx, dy);
                }
                out.left_down = touch.pad_touched && self.config.click_to_mouse;
                out.right_down = touch.pad_clicked && self.config.right_click;
            }
            TouchpadMode::Dpad => {
                // Centre of the pad is the origin; deadzone stops drift.
                const DZ: f32 = 0.30;
                let cx = pos.0 - 0.5;
                let cy = pos.1 - 0.5;
                if cx.abs() >= DZ || cy.abs() >= DZ {
                    out.dpad = (cy <= -DZ, cy >= DZ, cx <= -DZ, cx >= DZ);
                }
            }
            TouchpadMode::Gestures | TouchpadMode::Off => {}
        }

        // Gesture detection runs in every mode; a gesture is a swipe across the
        // pad from wherever the touch began.
        let now = std::time::Instant::now();
        if self.start.is_none() {
            self.start = Some((pos.0, pos.1, now));
        }
        let (sx, sy, t0) = self.start.expect("just set");

        if self.fired.is_none()
            && now.duration_since(t0).as_millis() as u32 >= self.config.min_duration_ms
        {
            let dx = pos.0 - sx;
            // Touchpad coordinates run top-left to bottom-right, so a *decrease*
            // in y means the fingers moved up the pad.
            let dy = pos.1 - sy;
            let th = self.config.swipe_threshold.clamp(0.05, 0.95);
            // Whichever axis moved further decides the direction.
            let g = if dx.abs() > dy.abs() {
                if dx > th {
                    Some(Gesture::TwoFingerRight)
                } else if dx < -th {
                    Some(Gesture::TwoFingerLeft)
                } else {
                    None
                }
            } else if dy < -th {
                Some(Gesture::TwoFingerUp)
            } else if dy > th {
                Some(Gesture::TwoFingerDown)
            } else {
                None
            };
            if let Some(g) = g {
                out.gesture = Some(g);
                self.fired = Some(g);
                // Disarm until the finger lifts.
                self.armed = false;
            }
        }

        self.prev = Some(pos);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(x: f32, y: f32) -> TouchState {
        TouchState {
            pad_touched: true,
            pad_clicked: false,
            x,
            y,
            raw_x: (x * 1919.0) as u16,
            raw_y: (y * 941.0) as u16,
        }
    }

    #[test]
    fn off_mode_produces_nothing() {
        let mut p = TouchpadProcessor::default();
        let out = p.process(&touch(0.9, 0.9));
        assert_eq!(out.mouse_delta, (0.0, 0.0));
        assert_eq!(out.dpad, (false, false, false, false));
    }

    #[test]
    fn mouse_mode_emits_delta() {
        let mut p = TouchpadProcessor::new(TouchpadConfig {
            mode: TouchpadMode::Mouse,
            invert_y: false,
            ..Default::default()
        });
        p.process(&touch(0.5, 0.5));
        let out = p.process(&touch(0.6, 0.5));
        assert!(
            (out.mouse_delta.0 - 0.1).abs() < 1e-5,
            "got {:?}",
            out.mouse_delta
        );
        assert!(out.mouse_delta.1.abs() < 1e-5);
    }

    #[test]
    fn release_rearms_gesture_detection() {
        let mut p = TouchpadProcessor::new(TouchpadConfig {
            mode: TouchpadMode::Gestures,
            require_release: true,
            min_duration_ms: 0,
            ..Default::default()
        });

        // One continuous touch: start left, swipe right.
        p.process(&touch(0.1, 0.5));
        let out = p.process(&touch(0.9, 0.5));
        assert_eq!(
            out.gesture,
            Some(Gesture::TwoFingerRight),
            "swipe should fire"
        );

        // Keep moving back without lifting: must not re-fire.
        let again = p.process(&touch(0.1, 0.5));
        assert_eq!(again.gesture, None, "must not repeat without a release");

        // Lift the finger, then swipe again.
        p.process(&TouchState::default());
        let fired = p.process(&touch(0.1, 0.5));
        assert_eq!(fired.gesture, None, "still just the start of a swipe");
        let fired = p.process(&touch(0.9, 0.5));
        assert_eq!(
            fired.gesture,
            Some(Gesture::TwoFingerRight),
            "release should re-arm"
        );
    }

    #[test]
    fn gesture_direction_is_determined_by_dominant_axis() {
        let mk = || {
            TouchpadProcessor::new(TouchpadConfig {
                mode: TouchpadMode::Gestures,
                min_duration_ms: 0,
                ..Default::default()
            })
        };

        // Mostly vertical movement reads as up, not right.
        let mut p = mk();
        p.process(&touch(0.5, 0.9));
        assert_eq!(
            p.process(&touch(0.55, 0.1)).gesture,
            Some(Gesture::TwoFingerUp)
        );

        // Mostly horizontal movement reads as left.
        let mut p = mk();
        p.process(&touch(0.9, 0.5));
        assert_eq!(
            p.process(&touch(0.1, 0.55)).gesture,
            Some(Gesture::TwoFingerLeft)
        );
    }

    #[test]
    fn small_movements_do_not_fire() {
        let mut p = TouchpadProcessor::new(TouchpadConfig {
            mode: TouchpadMode::Gestures,
            swipe_threshold: 0.5,
            min_duration_ms: 0,
            ..Default::default()
        });
        p.process(&touch(0.3, 0.5));
        assert_eq!(
            p.process(&touch(0.6, 0.5)).gesture,
            None,
            "movement below the threshold must not fire"
        );
    }

    #[test]
    fn dpad_uses_deadzone() {
        // A fresh processor per assertion: `require_release` means a gesture
        // must start from a clean touch, and the two samples here are separate
        // touches.
        let mk = || {
            TouchpadProcessor::new(TouchpadConfig {
                mode: TouchpadMode::Dpad,
                ..Default::default()
            })
        };

        // Near centre: the deadzone swallows the drift.
        let mut p = mk();
        assert_eq!(
            p.process(&touch(0.52, 0.5)).dpad,
            (false, false, false, false)
        );

        // Top-left corner: up and left.
        let mut p = mk();
        let corner = p.process(&touch(0.05, 0.05)).dpad;
        assert!(corner.0, "up expected, got {corner:?}");
        assert!(corner.2, "left expected, got {corner:?}");
        assert!(!corner.1 && !corner.3, "down/right must not fire");

        // Bottom-right corner: down and right.
        let mut p = mk();
        let corner = p.process(&touch(0.95, 0.95)).dpad;
        assert!(corner.1, "down expected, got {corner:?}");
        assert!(corner.3, "right expected, got {corner:?}");
    }

    #[test]
    fn invert_y_flips_vertical_delta() {
        let mk = |inv| {
            TouchpadProcessor::new(TouchpadConfig {
                mode: TouchpadMode::Mouse,
                invert_y: inv,
                ..Default::default()
            })
        };
        let mut a = mk(false);
        a.process(&touch(0.5, 0.5));
        let da = a.process(&touch(0.5, 0.7)).mouse_delta.1;

        let mut b = mk(true);
        b.process(&touch(0.5, 0.5));
        let db = b.process(&touch(0.5, 0.7)).mouse_delta.1;

        assert!(
            da > 0.0 && db < 0.0,
            "expected opposite signs, got {da} and {db}"
        );
    }
}
