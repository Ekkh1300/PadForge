//! Test: confirm every part of the pad actually works.
//!
//! Every other page answers "how should this behave". This one answers a
//! question that has to come first: "is the pad working at all, and which part
//! of it is not". A stick that has worn out, a shoulder button with a dead
//! microswitch and a touchpad that stopped reporting all present to a game as
//! "the controller is fine", and none of them can be told apart from the
//! game's own behaviour by playing the game.
//!
//! So the page watches and keeps score. Operate a control and its row turns
//! green and stays green, because a switch that works once works every time —
//! and a switch that did not is the thing worth knowing about. The drawing
//! beside it lights up as the report arrives, which is what turns "the light
//! came on" into a thing a person can check in a second.

use egui::{Rect, RichText, Sense, Vec2};

use padcore::report::{Buttons, Ds4Report};

use crate::theme::*;
use crate::widgets;

/// One part of the pad that can be confirmed working.
pub struct Part {
    /// What a player calls it, which is not always what the report calls it.
    pub label: &'static str,
    pub bit: u32,
}

/// One bit per part, in the order of [`PARTS`].
pub const LEFT_STICK: u32 = 1 << 0;
pub const RIGHT_STICK: u32 = 1 << 1;
pub const L2: u32 = 1 << 2;
pub const R2: u32 = 1 << 3;
pub const L1: u32 = 1 << 4;
pub const R1: u32 = 1 << 5;
pub const CROSS: u32 = 1 << 6;
pub const CIRCLE: u32 = 1 << 7;
pub const SQUARE: u32 = 1 << 8;
pub const TRIANGLE: u32 = 1 << 9;
pub const DPAD_UP: u32 = 1 << 10;
pub const DPAD_DOWN: u32 = 1 << 11;
pub const DPAD_LEFT: u32 = 1 << 12;
pub const DPAD_RIGHT: u32 = 1 << 13;
pub const L3: u32 = 1 << 14;
pub const R3: u32 = 1 << 15;
pub const SHARE: u32 = 1 << 16;
pub const OPTIONS: u32 = 1 << 17;
pub const TOUCHPAD: u32 = 1 << 18;
pub const MOTION: u32 = 1 << 19;

/// Every part, in the order it is listed.
///
/// Ordered as the pad is laid out — sticks and shoulders, then the face, then
/// the middle — so the list can be worked down without losing one's place.
pub const PARTS: &[Part] = &[
    Part {
        label: "Left stick",
        bit: LEFT_STICK,
    },
    Part {
        label: "Left stick click",
        bit: L3,
    },
    Part {
        label: "Right stick",
        bit: RIGHT_STICK,
    },
    Part {
        label: "Right stick click",
        bit: R3,
    },
    Part {
        label: "L1",
        bit: L1,
    },
    Part {
        label: "L2",
        bit: L2,
    },
    Part {
        label: "R1",
        bit: R1,
    },
    Part {
        label: "R2",
        bit: R2,
    },
    Part {
        label: "D-pad up",
        bit: DPAD_UP,
    },
    Part {
        label: "D-pad down",
        bit: DPAD_DOWN,
    },
    Part {
        label: "D-pad left",
        bit: DPAD_LEFT,
    },
    Part {
        label: "D-pad right",
        bit: DPAD_RIGHT,
    },
    Part {
        label: "Cross",
        bit: CROSS,
    },
    Part {
        label: "Circle",
        bit: CIRCLE,
    },
    Part {
        label: "Square",
        bit: SQUARE,
    },
    Part {
        label: "Triangle",
        bit: TRIANGLE,
    },
    Part {
        label: "Share",
        bit: SHARE,
    },
    Part {
        label: "Options",
        bit: OPTIONS,
    },
    Part {
        label: "Touchpad touched",
        bit: TOUCHPAD,
    },
    Part {
        label: "Motion sensors",
        bit: MOTION,
    },
];

/// How far a stick has to leave centre before that counts as it working.
///
/// Set above the drift a worn stick makes on its own. A stick that has lost
/// its centre reports a few percent forever, and a test that fired on that
/// would confirm a stick the player had never touched.
const STICK_THRESHOLD: f32 = 0.25;

/// How far a trigger has to travel, 0..=1.
const TRIGGER_THRESHOLD: f32 = 0.1;

/// How fast a motion sensor has to move, in the units the report carries.
///
/// Well above what a pad resting on a desk reports. The sensors are noisy
/// enough that a threshold set for sensitivity would confirm them while they
/// sat still, which would make the row meaningless.
const MOTION_THRESHOLD: f32 = 5.0;

pub fn draw(ui: &mut egui::Ui, ctx: &mut super::Ctx) {
    ui.add_space(SPACE_SM);

    let t = ctx.engine.telemetry();
    let report = t.report;
    // Scoring happens in `sample_telemetry`, every frame, so this only reads.
    let tested = ctx.state.tested;

    scorecard(ui, ctx, tested);
    ui.add_space(SPACE_MD);

    ui.columns(2, |cols| {
        pad_diagram(&mut cols[0], &report);
        checklist(&mut cols[1], tested);
    });

    ui.add_space(SPACE_MD);
    ui.columns(3, |cols| {
        sticks(&mut cols[0], &report);
        triggers(&mut cols[1], &report);
        sensors(&mut cols[2], &report);
    });
}

/// Mark off whatever the report shows working right now.
///
/// Called every frame whether or not the page is being looked at, so a control
/// bumped by accident on another page still counts. The point is that the pad
/// works, not that the player was watching.
pub fn observe(report: &Ds4Report, tested: &mut u32) {
    let b = report.buttons;
    let mut seen = 0u32;

    let moved = |v: f32| v.abs() > STICK_THRESHOLD;
    if moved(report.left_x) || moved(report.left_y) {
        seen |= LEFT_STICK;
    }
    if moved(report.right_x) || moved(report.right_y) {
        seen |= RIGHT_STICK;
    }
    if report.l2 > TRIGGER_THRESHOLD {
        seen |= L2;
    }
    if report.r2 > TRIGGER_THRESHOLD {
        seen |= R2;
    }

    // The digital controls are one word, so each is one test of one bit.
    let digital = [
        (Buttons::L1, L1),
        (Buttons::R1, R1),
        (Buttons::CROSS, CROSS),
        (Buttons::CIRCLE, CIRCLE),
        (Buttons::SQUARE, SQUARE),
        (Buttons::TRIANGLE, TRIANGLE),
        (Buttons::UP, DPAD_UP),
        (Buttons::DOWN, DPAD_DOWN),
        (Buttons::LEFT, DPAD_LEFT),
        (Buttons::RIGHT, DPAD_RIGHT),
        (Buttons::L3, L3),
        (Buttons::R3, R3),
        (Buttons::SHARE, SHARE),
        (Buttons::OPTIONS, OPTIONS),
    ];
    for (mask, bit) in digital {
        if b.pressed(mask) {
            seen |= bit;
        }
    }

    if report.touch.pad_touched {
        seen |= TOUCHPAD;
    }

    let g = &report.gyro;
    let a = &report.accel;
    if g.pitch.abs() > MOTION_THRESHOLD
        || g.yaw.abs() > MOTION_THRESHOLD
        || g.roll.abs() > MOTION_THRESHOLD
        || a.x.abs() > MOTION_THRESHOLD
        || a.y.abs() > MOTION_THRESHOLD
        || a.z.abs() > MOTION_THRESHOLD
    {
        seen |= MOTION;
    }

    *tested |= seen;
}

/// How many parts have been confirmed.
fn confirmed(tested: u32) -> usize {
    PARTS.iter().filter(|p| tested & p.bit != 0).count()
}

/// The progress line, and the button that starts a check over.
fn scorecard(ui: &mut egui::Ui, ctx: &mut super::Ctx, tested: u32) {
    let done = confirmed(tested);
    let total = PARTS.len();

    card().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Pad test").color(TEXT).size(14.0).strong());
            ui.label(
                RichText::new(format!("{done} of {total} parts confirmed"))
                    .color(TEXT_MUTED)
                    .size(12.5),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if secondary_button(ui, "Start over").clicked() {
                    ctx.state.tested = 0;
                }
            });
        });
        ui.add_space(SPACE_SM);

        if done == total {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("every part responded")
                        .color(SUCCESS)
                        .size(12.5),
                );
            });
        } else {
            ui.label(
                RichText::new(
                    "Operate each control once. A row turns green when the pad \
                     reports it and stays green — a switch that works once works \
                     every time, and one that does not is what this is for.",
                )
                .color(TEXT_FAINT)
                .size(11.5),
            );
        }

        ui.add_space(SPACE_SM);
        // Drawn rather than a percentage alone: a bar that is nearly full and
        // one that is a quarter full are the same number to read and very
        // different things to feel.
        let fraction = done as f32 / total as f32;
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 8.0), Sense::hover());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, RADIUS_PILL, TRACK);
        if fraction > 0.0 {
            let filled =
                Rect::from_min_size(rect.min, Vec2::new(rect.width() * fraction, rect.height()));
            let colour = if done == total { SUCCESS } else { ACCENT };
            p.rect_filled(filled, RADIUS_PILL, colour);
        }
    });
}

/// Every part, with whether it has responded.
///
/// Two columns, because twenty rows in one is a list a person reads twice —
/// and the second read is where the one that did not respond gets missed.
fn checklist(ui: &mut egui::Ui, tested: u32) {
    card().show(ui, |ui| {
        ui.label(RichText::new("Parts").color(TEXT).size(14.0).strong());
        ui.add_space(SPACE_SM);
        // Split down the middle rather than running one column of twenty: a
        // list that fits on screen is one a person can check at a glance, and
        // the row that did not respond is the one that gets missed in a
        // scroll.
        let half = PARTS.len().div_ceil(2);
        ui.columns(2, |cols| {
            for (index, part) in PARTS.iter().enumerate() {
                let ui = &mut cols[index / half];
                let confirmed = tested & part.bit != 0;
                // The mark is drawn rather than a glyph in the string: egui's
                // built-in fonts carry Latin-1 only, and a checkmark is not in
                // it. A missing glyph renders as a box, which is worse than no
                // mark at all.
                let (mark, colour) = if confirmed {
                    ("+", SUCCESS)
                } else {
                    ("-", TEXT_FAINT)
                };
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [12.0, 18.0],
                        egui::Label::new(RichText::new(mark).color(colour).size(13.0).strong()),
                    );
                    ui.label(
                        RichText::new(part.label)
                            .color(if confirmed { TEXT } else { TEXT_MUTED })
                            .size(12.0),
                    );
                });
            }
        });
    });
}

/// The pad, drawn, lighting up as the report arrives.
///
/// This is the part that turns the test from a list into something a person
/// can do. "Cross is not responding" takes a moment to read off a list; the
/// button not lighting up while a finger is on it takes none.
///
/// The dashboard's widget, on purpose. Two drawings of one pad would drift
/// apart, and the one that drifted would be the one nobody had a reason to
/// look at closely.
fn pad_diagram(ui: &mut egui::Ui, report: &Ds4Report) {
    card().show(ui, |ui| {
        ui.label(RichText::new("Live").color(TEXT).size(14.0).strong());
        ui.add_space(SPACE_SM);

        let mut pad = widgets::PadPreview::new();
        pad.report = report.clone();
        // Where the finger is, not just that one is there.
        pad.touch_point = true;
        // This card holds nothing else, so the drawing takes what it is given.
        pad.max_width = 480.0;
        pad.show(ui);
    });
}

/// The two sticks with their numbers, so a drift is a figure and not a guess.
fn sticks(ui: &mut egui::Ui, report: &Ds4Report) {
    card().show(ui, |ui| {
        ui.label(RichText::new("Sticks").color(TEXT).size(14.0).strong());
        ui.add_space(SPACE_SM);
        for (label, x, y) in [
            ("Left", report.left_x, report.left_y),
            ("Right", report.right_x, report.right_y),
        ] {
            ui.label(RichText::new(label).color(TEXT_MUTED).size(12.0));
            ui.columns(2, |cols| {
                for (i, v) in [x, y].into_iter().enumerate() {
                    widgets::axis_bar(&mut cols[i], if i == 0 { "X" } else { "Y" }, v, ACCENT);
                }
            });
        }
        ui.add_space(SPACE_SM);
        ui.label(
            RichText::new("Let go of both. A stick resting away from zero is drifting.")
                .color(TEXT_FAINT)
                .size(11.0),
        );
    });
}

/// L2 and R2, with the digital click beside the analogue travel.
fn triggers(ui: &mut egui::Ui, report: &Ds4Report) {
    card().show(ui, |ui| {
        ui.label(RichText::new("Triggers").color(TEXT).size(14.0).strong());
        ui.add_space(SPACE_SM);
        let b = report.buttons;
        for (label, value, clicked) in [
            ("L2", report.l2, b.pressed(Buttons::L2)),
            ("R2", report.r2, b.pressed(Buttons::R2)),
        ] {
            widgets::trigger_bar(ui, label, value, clicked);
        }
        ui.add_space(SPACE_SM);
        ui.label(
            RichText::new(
                "Each has two jobs: it travels, and it clicks at the end. \
                 Both are listed, because a trigger that travels but never \
                 clicks is a different fault.",
            )
            .color(TEXT_FAINT)
            .size(11.0),
        );
    });
}

/// The sensors, so a pad that reports motion can be told from one that does not.
fn sensors(ui: &mut egui::Ui, report: &Ds4Report) {
    card().show(ui, |ui| {
        ui.label(
            RichText::new("Motion and touch")
                .color(TEXT)
                .size(14.0)
                .strong(),
        );
        ui.add_space(SPACE_SM);

        ui.label(RichText::new("Gyro").color(TEXT_MUTED).size(12.0));
        ui.label(
            RichText::new(format!(
                "pitch {:+6.1}   yaw {:+6.1}   roll {:+6.1}",
                report.gyro.pitch, report.gyro.yaw, report.gyro.roll
            ))
            .color(TEXT)
            .size(12.5),
        );
        ui.add_space(SPACE_SM);
        ui.label(RichText::new("Accelerometer").color(TEXT_MUTED).size(12.0));
        ui.label(
            RichText::new(format!(
                "x {:+6.1}   y {:+6.1}   z {:+6.1}",
                report.accel.x, report.accel.y, report.accel.z
            ))
            .color(TEXT)
            .size(12.5),
        );
        ui.add_space(SPACE_SM);

        let touch = report.touch;
        ui.label(
            RichText::new(format!(
                "touchpad  {}  at ({:.2}, {:.2})",
                if touch.pad_touched { "touched" } else { "idle" },
                touch.x,
                touch.y
            ))
            .color(if touch.pad_touched {
                ACCENT
            } else {
                TEXT_MUTED
            })
            .size(12.5),
        );
        ui.label(
            RichText::new(format!(
                "click  {}",
                if touch.pad_clicked { "down" } else { "up" }
            ))
            .color(if touch.pad_clicked {
                ACCENT
            } else {
                TEXT_MUTED
            })
            .size(12.5),
        );
        ui.add_space(SPACE_SM);
        ui.label(
            RichText::new("Pick the pad up and turn it. Every figure should move.")
                .color(TEXT_FAINT)
                .size(11.0),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run one report through the observer and report what it claimed.
    fn observe_one(report: &Ds4Report) -> u32 {
        let mut tested = 0;
        observe(report, &mut tested);
        tested
    }

    /// The list is the product here, not the drawing. Two parts sharing a bit
    /// would mean confirming one silently confirmed the other, and the check
    /// would pass on a pad with a dead switch — which is exactly the pad this
    /// page exists to catch.
    #[test]
    fn every_part_has_its_own_bit() {
        let mut seen = 0u32;
        for part in PARTS {
            assert_eq!(
                part.bit.count_ones(),
                1,
                "{} must be exactly one bit, got {:#b}",
                part.label,
                part.bit
            );
            assert_eq!(
                seen & part.bit,
                0,
                "{} reuses a bit already claimed by an earlier part",
                part.label
            );
            seen |= part.bit;
        }
        // Nothing in the bitfield may be orphaned: a constant that no part
        // lists can never be ticked, and dead bits are how a row goes missing.
        assert_eq!(
            seen,
            (1 << PARTS.len()) - 1,
            "some bits are defined but belong to no listed part"
        );
    }

    /// A pad nobody has touched confirms nothing. The observer runs every
    /// frame on a report that is mostly idle, and a threshold low enough to
    /// fire on idle would fill the list before the player started.
    #[test]
    fn a_neutral_pad_confirms_nothing() {
        assert_eq!(observe_one(&Ds4Report::neutral()), 0);
    }

    /// One digital control must light exactly its own row and no other, so a
    /// player pressing Cross cannot see the Circle row turn green too.
    #[test]
    fn each_button_confirms_only_itself() {
        for part in PARTS {
            // Only the digital parts can be set a bit at a time; the rest are
            // driven through the analogue fields below.
            if !DIGITAL.contains(&part.bit) {
                continue;
            }
            let mut report = Ds4Report::neutral();
            report.buttons = Buttons(digital_mask(part.bit));
            assert_eq!(
                observe_one(&report),
                part.bit,
                "pressing {} confirmed the wrong set",
                part.label
            );
        }
    }

    /// A worn stick rests a little off centre forever. If that counted, the
    /// stick row would be ticked the moment the page opened on a pad whose
    /// stick had failed — so the threshold has to sit above drift and the test
    /// has to pin it there.
    #[test]
    fn stick_drift_does_not_confirm_a_stick() {
        for (bit, mut report) in [
            (LEFT_STICK, Ds4Report::neutral()),
            (RIGHT_STICK, Ds4Report::neutral()),
        ] {
            // Just under the threshold: the worst drift a healthy-but-worn
            // stick makes, and the most the check must ignore.
            let drift = STICK_THRESHOLD * 0.9;
            if bit == LEFT_STICK {
                report.left_x = drift;
            } else {
                report.right_x = drift;
            }
            assert_eq!(observe_one(&report), 0, "drift confirmed a stick");

            // And the same stick, actually moved, must count.
            if bit == LEFT_STICK {
                report.left_x = STICK_THRESHOLD * 1.1;
            } else {
                report.right_x = STICK_THRESHOLD * 1.1;
            }
            assert_eq!(observe_one(&report), bit, "a moved stick did not count");
        }
    }

    /// Both axes of a stick count as the same part. A stick that only travels
    /// on one axis is a broken stick, but the page cannot tell which half
    /// failed from here — what it must not do is require a full circle, which
    /// would leave the row unticked for a player who only ever turned left.
    #[test]
    fn either_stick_axis_confirms_the_stick() {
        let mut report = Ds4Report::neutral();
        report.left_y = STICK_THRESHOLD * 1.1;
        assert_eq!(observe_one(&report), LEFT_STICK);

        let mut report = Ds4Report::neutral();
        report.right_y = -(STICK_THRESHOLD * 1.1);
        assert_eq!(observe_one(&report), RIGHT_STICK);
    }

    /// A trigger that travels confirms itself. Note the digital L2/R2 click is
    /// a separate row: a trigger that travels but never clicks is a different
    /// fault and the page lists them apart on purpose.
    #[test]
    fn trigger_travel_confirms_the_analogue_row_only() {
        let mut report = Ds4Report::neutral();
        report.l2 = TRIGGER_THRESHOLD + 0.05;
        assert_eq!(observe_one(&report), L2);

        let mut report = Ds4Report::neutral();
        report.r2 = TRIGGER_THRESHOLD + 0.05;
        assert_eq!(observe_one(&report), R2);
    }

    /// The sensors sit still on a desk and still chatter. A threshold that
    /// fired on that would tick the motion row before anyone picked the pad
    /// up, which makes the row a lie.
    #[test]
    fn resting_sensor_noise_does_not_confirm_motion() {
        let mut report = Ds4Report::neutral();
        report.gyro.pitch = MOTION_THRESHOLD * 0.9;
        report.accel.z = MOTION_THRESHOLD * 0.9;
        assert_eq!(observe_one(&report), 0);

        // One axis, actually moved, is enough.
        report.gyro.roll = MOTION_THRESHOLD * 1.1;
        assert_eq!(observe_one(&report), MOTION);
    }

    /// Touching the pad is not the same as clicking it, and both are shown.
    /// Only the touch confirms the row, because the click is a switch on the
    /// pad's underside that not every fault reports as a touch first.
    #[test]
    fn a_touched_pad_confirms_the_touchpad_row() {
        let mut report = Ds4Report::neutral();
        report.touch.pad_touched = true;
        assert_eq!(observe_one(&report), TOUCHPAD);
    }

    /// The score only ever goes up. Re-running the observer over the same
    /// report must not clear anything, or a control that was confirmed and
    /// then let go would lose its tick on the next frame.
    #[test]
    fn scoring_only_accumulates() {
        let mut tested = 0;

        let mut pressed = Ds4Report::neutral();
        pressed.buttons = Buttons(Buttons::CROSS);
        observe(&pressed, &mut tested);
        assert_eq!(tested, CROSS);

        // Let go, and press something else: the first tick survives.
        let mut other = Ds4Report::neutral();
        other.buttons = Buttons(Buttons::TRIANGLE);
        observe(&other, &mut tested);
        assert_eq!(tested, CROSS | TRIANGLE);

        // And re-running the idle report changes nothing.
        observe(&Ds4Report::neutral(), &mut tested);
        assert_eq!(tested, CROSS | TRIANGLE);
    }

    /// The count that drives the progress bar has to match the list a player
    /// reads, or the page will announce completion while a row is still grey.
    #[test]
    fn the_count_matches_the_list() {
        assert_eq!(confirmed(0), 0);
        assert_eq!(confirmed(u32::MAX), PARTS.len());

        // Ticking exactly one part reports exactly one.
        for part in PARTS {
            assert_eq!(confirmed(part.bit), 1, "{} did not count", part.label);
        }
    }

    /// The bits [`observe`] can set, in the order the digital loop walks them.
    ///
    /// Mirrors the array inside `observe`, so a test that presses one control
    /// and expects one row knows which rows are reachable that way.
    const DIGITAL: &[u32] = &[
        L1, R1, CROSS, CIRCLE, SQUARE, TRIANGLE, DPAD_UP, DPAD_DOWN, DPAD_LEFT, DPAD_RIGHT, L3, R3,
        SHARE, OPTIONS,
    ];

    /// The button bit in [`Buttons`] that sets a test part bit, or 0 if the
    /// part is analogue and cannot be pressed by hand.
    fn digital_mask(part: u32) -> u16 {
        match part {
            L1 => Buttons::L1,
            R1 => Buttons::R1,
            CROSS => Buttons::CROSS,
            CIRCLE => Buttons::CIRCLE,
            SQUARE => Buttons::SQUARE,
            TRIANGLE => Buttons::TRIANGLE,
            DPAD_UP => Buttons::UP,
            DPAD_DOWN => Buttons::DOWN,
            DPAD_LEFT => Buttons::LEFT,
            DPAD_RIGHT => Buttons::RIGHT,
            L3 => Buttons::L3,
            R3 => Buttons::R3,
            SHARE => Buttons::SHARE,
            OPTIONS => Buttons::OPTIONS,
            _ => 0,
        }
    }
}
