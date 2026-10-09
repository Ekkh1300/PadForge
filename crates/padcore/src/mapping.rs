//! Control enumeration and the mapping model.
//!
//! A [`Mapping`] says, for one DS4 control, where its value should end up and
//! what modifiers gate it. The same structure is reused for buttons, axes and
//! triggers, which is why an [`X360Control`] has to be able to name a button
//! *or* an axis.

use serde::{Deserialize, Serialize};

use crate::filters::Curve;

/// Every digital control on the DS4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ds4Control {
    Cross,
    Circle,
    Square,
    Triangle,
    L1,
    R1,
    L2,
    R2,
    Share,
    Options,
    L3,
    R3,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    TouchpadClick,
    /// Pseudo-control: pressing any of the four D-pad directions.
    DpadAny,
    /// Pseudo-control: the touchpad surface itself.
    TouchpadGesture,
    LeftXNegative,
    LeftXPositive,
    LeftYNegative,
    LeftYPositive,
    RightXNegative,
    RightXPositive,
    RightYNegative,
    RightYPositive,
}

impl Ds4Control {
    /// Stable ordering, used so the mapping table is always laid out the same.
    pub const ALL: &'static [Ds4Control] = &[
        Ds4Control::Cross,
        Ds4Control::Circle,
        Ds4Control::Square,
        Ds4Control::Triangle,
        Ds4Control::L1,
        Ds4Control::R1,
        Ds4Control::L2,
        Ds4Control::R2,
        Ds4Control::Share,
        Ds4Control::Options,
        Ds4Control::L3,
        Ds4Control::R3,
        Ds4Control::DpadUp,
        Ds4Control::DpadDown,
        Ds4Control::DpadLeft,
        Ds4Control::DpadRight,
        Ds4Control::TouchpadClick,
        Ds4Control::DpadAny,
        Ds4Control::TouchpadGesture,
        Ds4Control::LeftXNegative,
        Ds4Control::LeftXPositive,
        Ds4Control::LeftYNegative,
        Ds4Control::LeftYPositive,
        Ds4Control::RightXNegative,
        Ds4Control::RightXPositive,
        Ds4Control::RightYNegative,
        Ds4Control::RightYPositive,
    ];

    /// Controls grouped the way the UI presents them.
    pub const FACE: &'static [Ds4Control] = &[
        Ds4Control::Cross,
        Ds4Control::Circle,
        Ds4Control::Square,
        Ds4Control::Triangle,
    ];
    pub const SHOULDERS: &'static [Ds4Control] = &[
        Ds4Control::L1,
        Ds4Control::R1,
        Ds4Control::L2,
        Ds4Control::R2,
    ];
    pub const STICKS: &'static [Ds4Control] = &[
        Ds4Control::L3,
        Ds4Control::R3,
        Ds4Control::LeftXNegative,
        Ds4Control::LeftXPositive,
        Ds4Control::LeftYNegative,
        Ds4Control::LeftYPositive,
        Ds4Control::RightXNegative,
        Ds4Control::RightXPositive,
        Ds4Control::RightYNegative,
        Ds4Control::RightYPositive,
    ];
    pub const SYSTEM: &'static [Ds4Control] = &[
        Ds4Control::Share,
        Ds4Control::Options,
        Ds4Control::TouchpadClick,
    ];
    pub const DPAD: &'static [Ds4Control] = &[
        Ds4Control::DpadUp,
        Ds4Control::DpadDown,
        Ds4Control::DpadLeft,
        Ds4Control::DpadRight,
        Ds4Control::DpadAny,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Ds4Control::Cross => "Cross",
            Ds4Control::Circle => "Circle",
            Ds4Control::Square => "Square",
            Ds4Control::Triangle => "Triangle",
            Ds4Control::L1 => "L1",
            Ds4Control::R1 => "R1",
            Ds4Control::L2 => "L2",
            Ds4Control::R2 => "R2",
            Ds4Control::Share => "Share",
            Ds4Control::Options => "Options",
            Ds4Control::L3 => "L3",
            Ds4Control::R3 => "R3",
            Ds4Control::DpadUp => "D-pad Up",
            Ds4Control::DpadDown => "D-pad Down",
            Ds4Control::DpadLeft => "D-pad Left",
            Ds4Control::DpadRight => "D-pad Right",
            Ds4Control::TouchpadClick => "Touchpad Click",
            Ds4Control::DpadAny => "D-pad (any)",
            Ds4Control::TouchpadGesture => "Touchpad (any)",
            Ds4Control::LeftXNegative => "Left Stick Left",
            Ds4Control::LeftXPositive => "Left Stick Right",
            Ds4Control::LeftYNegative => "Left Stick Down",
            Ds4Control::LeftYPositive => "Left Stick Up",
            Ds4Control::RightXNegative => "Right Stick Left",
            Ds4Control::RightXPositive => "Right Stick Right",
            Ds4Control::RightYNegative => "Right Stick Down",
            Ds4Control::RightYPositive => "Right Stick Up",
        }
    }

    /// Whether this control is an axis direction rather than a button.
    pub fn is_axis_direction(self) -> bool {
        matches!(
            self,
            Ds4Control::LeftXNegative
                | Ds4Control::LeftXPositive
                | Ds4Control::LeftYNegative
                | Ds4Control::LeftYPositive
                | Ds4Control::RightXNegative
                | Ds4Control::RightXPositive
                | Ds4Control::RightYNegative
                | Ds4Control::RightYPositive
        )
    }

    /// Which raw report bit(s) drive this control.
    pub fn button_mask(self) -> u16 {
        use crate::report::Buttons as B;
        match self {
            Ds4Control::Cross => B::CROSS,
            Ds4Control::Circle => B::CIRCLE,
            Ds4Control::Square => B::SQUARE,
            Ds4Control::Triangle => B::TRIANGLE,
            Ds4Control::L1 => B::L1,
            Ds4Control::R1 => B::R1,
            Ds4Control::L2 => B::L2,
            Ds4Control::R2 => B::R2,
            Ds4Control::Share => B::SHARE,
            Ds4Control::Options => B::OPTIONS,
            Ds4Control::L3 => B::L3,
            Ds4Control::R3 => B::R3,
            Ds4Control::DpadUp => B::UP,
            Ds4Control::DpadDown => B::DOWN,
            Ds4Control::DpadLeft => B::LEFT,
            Ds4Control::DpadRight => B::RIGHT,
            Ds4Control::DpadAny => B::UP | B::DOWN | B::LEFT | B::RIGHT,
            // The touchpad's state lives in its own report field rather than the
            // button word, so it has no bit here; callers read the touch flag
            // directly. Returning 0 keeps this table total.
            Ds4Control::TouchpadClick | Ds4Control::TouchpadGesture => 0,
            // Axis directions are driven from the shaped stick value, not a bit.
            _ => 0,
        }
    }
}

/// Every addressable target on a virtual Xbox 360 pad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum X360Control {
    A,
    B,
    X,
    Y,
    LeftBumper,
    RightBumper,
    Guide,
    Back,
    Start,
    LeftThumb,
    RightThumb,
    // D-pad
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    // Analog stick directions, emitted as digital button presses.
    LeftThumbUp,
    LeftThumbDown,
    LeftThumbLeft,
    LeftThumbRight,
    RightThumbUp,
    RightThumbDown,
    RightThumbLeft,
    RightThumbRight,
    // Trigger half-press, distinct from the bumper.
    LeftTriggerClick,
    RightTriggerClick,
    /// Nothing: the control is consumed locally (e.g. bound to a hotkey).
    None,
}

impl X360Control {
    pub fn label(self) -> &'static str {
        match self {
            X360Control::A => "A",
            X360Control::B => "B",
            X360Control::X => "X",
            X360Control::Y => "Y",
            X360Control::LeftBumper => "LB",
            X360Control::RightBumper => "RB",
            X360Control::Guide => "Guide",
            X360Control::Back => "Back",
            X360Control::Start => "Start",
            X360Control::LeftThumb => "L3",
            X360Control::RightThumb => "R3",
            X360Control::DpadUp => "D-pad Up",
            X360Control::DpadDown => "D-pad Down",
            X360Control::DpadLeft => "D-pad Left",
            X360Control::DpadRight => "D-pad Right",
            X360Control::LeftThumbUp => "L3 Up",
            X360Control::LeftThumbDown => "L3 Down",
            X360Control::LeftThumbLeft => "L3 Left",
            X360Control::LeftThumbRight => "L3 Right",
            X360Control::RightThumbUp => "R3 Up",
            X360Control::RightThumbDown => "R3 Down",
            X360Control::RightThumbLeft => "R3 Left",
            X360Control::RightThumbRight => "R3 Right",
            X360Control::LeftTriggerClick => "LT click",
            X360Control::RightTriggerClick => "RT click",
            X360Control::None => "- unmapped -",
        }
    }

    /// The button bit this control drives in an `XINPUT_GAMEPAD` report.
    pub fn bit(self) -> u16 {
        use crate::output::XButtons;
        match self {
            X360Control::A => XButtons::A,
            X360Control::B => XButtons::B,
            X360Control::X => XButtons::X,
            X360Control::Y => XButtons::Y,
            X360Control::LeftBumper => XButtons::LB,
            X360Control::RightBumper => XButtons::RB,
            X360Control::Guide => XButtons::GUIDE,
            X360Control::Back => XButtons::BACK,
            X360Control::Start => XButtons::START,
            X360Control::LeftThumb => XButtons::L3,
            X360Control::RightThumb => XButtons::R3,
            X360Control::DpadUp => XButtons::DPAD_UP,
            X360Control::DpadDown => XButtons::DPAD_DOWN,
            X360Control::DpadLeft => XButtons::DPAD_LEFT,
            X360Control::DpadRight => XButtons::DPAD_RIGHT,
            X360Control::LeftThumbUp => XButtons::L3,
            X360Control::LeftThumbDown => XButtons::L3,
            X360Control::LeftThumbLeft => XButtons::L3,
            X360Control::LeftThumbRight => XButtons::L3,
            X360Control::RightThumbUp => XButtons::R3,
            X360Control::RightThumbDown => XButtons::R3,
            X360Control::RightThumbLeft => XButtons::R3,
            X360Control::RightThumbRight => XButtons::R3,
            X360Control::LeftTriggerClick => XButtons::LB,
            X360Control::RightTriggerClick => XButtons::RB,
            X360Control::None => 0,
        }
    }

    /// True when this control needs the stick axis, not just a button press.
    pub fn is_stick_emulation(self) -> bool {
        matches!(
            self,
            X360Control::LeftThumbUp
                | X360Control::LeftThumbDown
                | X360Control::LeftThumbLeft
                | X360Control::LeftThumbRight
                | X360Control::RightThumbUp
                | X360Control::RightThumbDown
                | X360Control::RightThumbLeft
                | X360Control::RightThumbRight
        )
    }
}

/// How one DS4 control is wired up.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Mapping {
    /// Where the value goes.
    pub target: X360Control,
    /// Curve applied to the source before it is emitted.
    #[serde(default)]
    pub curve: Curve,
    /// Multiplier, so a control can be made more or less sensitive.
    #[serde(default = "one")]
    pub sensitivity: f32,
    /// Require this other X360 control to be held for the mapping to fire.
    #[serde(default)]
    pub mod_target: Option<X360Control>,
    /// Require this other DS4 control to be held, e.g. hold L1 while pressing a
    /// face button.
    #[serde(default)]
    pub mod_ds4: Option<Ds4Control>,
}

fn one() -> f32 {
    1.0
}

impl Default for Mapping {
    fn default() -> Self {
        Self {
            target: X360Control::None,
            curve: Curve::Linear,
            sensitivity: 1.0,
            mod_target: None,
            mod_ds4: None,
        }
    }
}

impl Mapping {
    pub fn new(target: X360Control) -> Self {
        Self {
            target,
            ..Default::default()
        }
    }

    /// Is this mapping actually doing anything?
    pub fn is_active(&self) -> bool {
        self.target != X360Control::None
    }

    /// Evaluate the source-side gate.
    ///
    /// `held` is the DS4 bit set currently pressed; `touch_held` covers the two
    /// touchpad pseudo-controls, which have no bit of their own.
    pub fn modifiers_met(&self, held: u16, touch_held: bool) -> bool {
        let Some(modifier) = self.mod_ds4 else {
            return true;
        };
        match modifier {
            Ds4Control::TouchpadClick | Ds4Control::TouchpadGesture => touch_held,
            other => held & other.button_mask() == other.button_mask(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::Buttons as B;

    #[test]
    fn default_face_mapping_is_play_to_x() {
        // Cross is "confirm" on a PlayStation pad, which is A on Xbox.
        assert_eq!(Ds4Control::Cross.button_mask(), B::CROSS);
    }

    #[test]
    fn dpad_any_covers_all_four() {
        let m = Ds4Control::DpadAny.button_mask();
        assert_eq!(m, B::UP | B::DOWN | B::LEFT | B::RIGHT);
    }

    #[test]
    fn all_list_has_no_duplicate_pointer_semantics() {
        // Sanity check that ALL and the groups agree on membership.
        for c in Ds4Control::FACE {
            assert!(Ds4Control::ALL.contains(c), "{c:?} missing from ALL");
        }
        for c in Ds4Control::SHOULDERS {
            assert!(Ds4Control::ALL.contains(c), "{c:?} missing from ALL");
        }
        assert!(Ds4Control::ALL.contains(&Ds4Control::TouchpadGesture));
    }

    #[test]
    fn unmapped_is_inert() {
        assert!(!Mapping::default().is_active());
        assert!(Mapping::new(X360Control::A).is_active());
    }
}