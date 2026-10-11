//! Racing: the shape of the steering axis, and the two pedals.
//!
//! The Controller page edits the same axes, but as six independent sets of
//! numbers. Racing is not six independent things: it is one steering axis whose
//! *shape* a driver feels through the wheel, and two pedals whose travel decides
//! whether a car settles or snaps. So this page leads with the shape itself —
//! drawn, not described — because a deadzone slider and a sensitivity slider
//! cannot between them say what their combination does, and the combination is
//! the entire subject.

use egui::RichText;
use padcore::filters::{Curve, Smoothing};
use padcore::profile::{AxisSettings, Profile};

use crate::state::ToastLevel;
use crate::theme::*;
use crate::widgets;

pub fn draw(ui: &mut egui::Ui, ctx: &mut super::Ctx) {
    let telemetry = ctx.engine.telemetry();
    ui.add_space(SPACE_SM);

    // Presets first. Someone opening this page usually knows the car feels
    // wrong and not yet how they want it to feel, and a starting point worth
    // tuning from beats a blank plot.
    presets(ui, ctx);
    ui.add_space(SPACE_MD);

    ui.columns(2, |cols| {
        steering(&mut cols[0], ctx, &telemetry);
        pedals(&mut cols[1], ctx, &telemetry);
    });
}

/// The steering axis: what it is doing now, what it will do, and the knobs.
fn steering(ui: &mut egui::Ui, ctx: &mut super::Ctx, t: &padcore::engine::Telemetry) {
    let mut s = ctx.state.editing_profile().left_x.clone();
    let live = t.report.left_x;

    card().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Steering").color(TEXT).size(14.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let turn = (live * 90.0) as i32;
                ui.label(
                    RichText::new(format!("{turn:>4} deg"))
                        .color(TEXT_MUTED)
                        .size(12.5)
                        .monospace(),
                );
            });
        });
        ui.label(
            RichText::new("Left stick across the road, through whatever shape is set here.")
                .color(TEXT_FAINT)
                .size(11.0),
        );
        ui.add_space(SPACE_SM);

        ui.columns(2, |cols| {
            widgets::steering_wheel(&mut cols[0], live, 116.0);
            widgets::response_curve(&mut cols[1], &s, live, 116.0);
        });

        ui.add_space(SPACE_SM);
        widgets::axis_bar(ui, "steering", live, ACCENT);

        ui.add_space(SPACE_MD);
        divider(ui);
        ui.add_space(SPACE_XS);

        if axis_knobs(ui, &mut s) {
            ctx.state.editing_profile_mut().left_x = s;
            crate::pages::dashboard::push_store(ctx);
        }
    });
}

/// The two pedals, each with its own travel and its own curve.
///
/// Split into throttle and brake rather than one "triggers" group because they
/// are not interchangeable: throttle is usually wanted linear from the top of
/// its travel, brake is usually wanted progressive so threshold braking is
/// possible, and a single setting for both forces a compromise on each.
fn pedals(ui: &mut egui::Ui, ctx: &mut super::Ctx, t: &padcore::engine::Telemetry) {
    let mut throttle = ctx.state.editing_profile().right_trigger.clone();
    let mut brake = ctx.state.editing_profile().left_trigger.clone();

    card().show(ui, |ui| {
        ui.label(RichText::new("Pedals").color(TEXT).size(14.0).strong());
        ui.label(
            RichText::new("R2 is throttle, L2 is brake, by the usual convention.")
                .color(TEXT_FAINT)
                .size(11.0),
        );
        ui.add_space(SPACE_SM);

        // What the shape does to each pedal, drawn the same way as steering so
        // the two read as one idea rather than two unrelated graphs.
        ui.columns(2, |cols| {
            caption(&mut cols[0], "THROTTLE");
            widgets::response_curve(&mut cols[0], &throttle, t.report.r2, 104.0);
            caption(&mut cols[1], "BRAKE");
            widgets::response_curve(&mut cols[1], &brake, t.report.l2, 104.0);
        });

        ui.add_space(SPACE_SM);
        widgets::trigger_bar(
            ui,
            "R2",
            t.report.r2,
            t.report.buttons.any(padcore::report::Buttons::R2),
        );
        widgets::trigger_bar(
            ui,
            "L2",
            t.report.l2,
            t.report.buttons.any(padcore::report::Buttons::L2),
        );

        ui.add_space(SPACE_MD);
        divider(ui);
        // Side by side, matching the two plots above them. Stacked, the two
        // sets of identical knobs ran to twice the height of everything else on
        // the page for the sake of repeating the same three controls.
        let mut changed = false;
        ui.columns(2, |cols| {
            caption(&mut cols[0], "THROTTLE");
            changed |= pedal_knobs(&mut cols[0], "racing-throttle", &mut throttle);
            caption(&mut cols[1], "BRAKE");
            changed |= pedal_knobs(&mut cols[1], "racing-brake", &mut brake);
        });

        if changed {
            let profile = ctx.state.editing_profile_mut();
            profile.right_trigger = throttle;
            profile.left_trigger = brake;
            crate::pages::dashboard::push_store(ctx);
        }
    });
}

/// The three knobs that decide how a steering axis feels, plus the two flags
/// that change what they mean.
fn axis_knobs(ui: &mut egui::Ui, s: &mut AxisSettings) -> bool {
    let mut changed = false;
    let before = s.clone();

    changed |= row(ui, "deadzone", |ui| {
        ui.add(egui::Slider::new(&mut s.deadzone, 0.0..=0.4).show_value(true))
            .changed()
    });
    ui.label(
        RichText::new("How far the wheel must move before the car does anything.")
            .color(TEXT_FAINT)
            .size(10.5),
    );
    ui.add_space(SPACE_SM);

    changed |= row(ui, "response", |ui| {
        let mut picked = false;
        egui::ComboBox::from_id_salt("racing-steer-curve")
            .selected_text(curve_label(&s.curve))
            .width(ui.available_width())
            .show_ui(ui, |ui| picked = curve_options(ui, &mut s.curve));
        picked
    });
    ui.label(
        RichText::new(curve_note(&s.curve))
            .color(TEXT_FAINT)
            .size(10.5),
    );
    ui.add_space(SPACE_SM);

    changed |= row(ui, "gain", |ui| {
        ui.add(
            egui::Slider::new(&mut s.sensitivity, 0.2..=3.0)
                .suffix("x")
                .show_value(true),
        )
        .changed()
    });

    ui.horizontal(|ui| {
        ui.add_sized([86.0, 22.0], egui::Label::new(RichText::new("")));
        changed |= ui.checkbox(&mut s.anti_deadzone, "anti-deadzone").changed();
        changed |= ui.checkbox(&mut s.inverted, "invert").changed();
    });

    // Comparing rather than trusting `changed`: a combo box reports a change
    // when the user re-picks what was already selected, and writing the profile
    // back for that is a save that did not need to happen.
    changed && *s != before
}

/// Deadzone, response and gain for one pedal. Fewer knobs than steering on
/// purpose: a pedal has one job and one direction, so the flags that only make
/// sense for a bidirectional axis are left out.
///
/// `id` separates the two pedals' combo boxes. Deriving it from the value being
/// edited would change the identity every time the slider moved, which is
/// exactly when egui must not think the widget is a different one.
fn pedal_knobs(ui: &mut egui::Ui, id: &'static str, s: &mut AxisSettings) -> bool {
    let mut changed = false;

    // Label above rather than beside. These sit in a half-width column, and the
    // fixed label column that works in the steering card leaves no room here:
    // slider and value box end up under the neighbouring column's label.
    changed |= stacked(ui, "deadzone", |ui| {
        ui.add(egui::Slider::new(&mut s.deadzone, 0.0..=0.5).show_value(true))
            .changed()
    });
    changed |= stacked(ui, "response", |ui| {
        let mut picked = false;
        egui::ComboBox::from_id_salt(id)
            .selected_text(curve_label(&s.curve))
            .width(ui.available_width())
            .show_ui(ui, |ui| picked = curve_options(ui, &mut s.curve));
        picked
    });
    changed |= stacked(ui, "gain", |ui| {
        ui.add(
            egui::Slider::new(&mut s.sensitivity, 0.2..=3.0)
                .suffix("x")
                .show_value(true),
        )
        .changed()
    });

    // Pedals are read in one direction, so a per-pedal invert is the only flag
    // that can mean anything here.
    changed |= ui.checkbox(&mut s.inverted, "invert").changed();

    changed
}

/// A caption over one control, for the columns too narrow for [`row`].
fn stacked(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui) -> bool) -> bool {
    ui.label(RichText::new(label).color(TEXT_MUTED).size(11.0));
    let changed = add(ui);
    ui.add_space(SPACE_SM);
    changed
}

/// One label column and one control, kept aligned with the rest of the app.
fn row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui) -> bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.add_sized(
            [86.0, 22.0],
            egui::Label::new(RichText::new(label).color(TEXT_MUTED)),
        );
        changed |= add(ui);
    });
    changed
}

/// A coherent set of values for a kind of racing, applied to steering and both
/// pedals at once.
///
/// The three are shapes, not vibes: each one is a specific answer to where a
/// driver wants the car to start responding and how quickly it should arrive.
fn presets(ui: &mut egui::Ui, ctx: &mut super::Ctx) {
    card().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                caption(ui, "PRESET");
                ui.label(
                    RichText::new("Applies to steering and both pedals. Tune from there.")
                        .color(TEXT_FAINT)
                        .size(11.0),
                );
            });
            // The buttons sit at the right of the row rather than under the
            // text. This also gives the card its width: a frame with only
            // short labels in it shrink-wraps to them, and a preset strip a
            // third of the window wide looks like an afterthought.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                for (name, preset) in PRESETS {
                    if widgets::preset_button(ui, name).clicked() {
                        apply_preset(ctx, preset);
                        ctx.state.toast(
                            format!("{name} applied to steering and pedals"),
                            ToastLevel::Success,
                        );
                    }
                }
            });
        });
    });
}

/// One coherent shape. `steer`, `throttle` and `brake` are the three axes.
struct Preset {
    steer: AxisSettings,
    throttle: AxisSettings,
    brake: AxisSettings,
}

/// Where each preset puts its dead zone and how it curves, spelled out so the
/// difference between them is a fact about the numbers rather than a label.
const PRESETS: &[(&str, Preset)] = &[
    (
        "Sim",
        Preset {
            // Long dead zone, hard expo: the car ignores small corrections at
            // speed and only answers once the driver means it.
            steer: axis(0.08, Curve::Exponential { exponent: 2.2 }, 1.0),
            throttle: axis(0.06, Curve::Exponential { exponent: 1.6 }, 1.0),
            brake: axis(0.04, Curve::Exponential { exponent: 1.8 }, 1.0),
        },
    ),
    (
        "Arcade",
        Preset {
            // Almost none: the stick is the car, and any delay between the two
            // is felt as lag.
            steer: axis(0.03, Curve::Linear, 1.15),
            throttle: axis(0.02, Curve::Linear, 1.0),
            brake: axis(0.02, Curve::Linear, 1.0),
        },
    ),
    (
        "Drift",
        Preset {
            // Bezier that leaves the centre fast, so opposite lock is available
            // before the car has finished rotating.
            steer: axis(
                0.02,
                Curve::Bezier {
                    x: [0.0, 0.55, 1.0],
                    y: [0.0, 0.82, 1.0],
                },
                1.1,
            ),
            throttle: axis(0.0, Curve::Linear, 1.0),
            brake: axis(0.05, Curve::Linear, 1.0),
        },
    ),
];

/// Build a steering-shaped [`AxisSettings`]: dead zone, response, gain, with
/// anti-deadzone on because a racing axis that leaves a gap after its dead zone
/// is unusable exactly where in the travel it matters.
///
/// `const` so the presets below can be built at compile time rather than on
/// every repaint.
const fn axis(deadzone: f32, curve: Curve, sensitivity: f32) -> AxisSettings {
    AxisSettings {
        deadzone,
        curve,
        anti_deadzone: true,
        smoothing: Smoothing::Exponential { alpha: 0.45 },
        sensitivity,
        inverted: false,
        formula: None,
    }
}

fn apply_preset(ctx: &mut super::Ctx, preset: &Preset) {
    let profile: &mut Profile = ctx.state.editing_profile_mut();
    profile.left_x = preset.steer.clone();
    profile.right_trigger = preset.throttle.clone();
    profile.left_trigger = preset.brake.clone();
    crate::pages::dashboard::push_store(ctx);
}

fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).color(TEXT_FAINT).size(11.0).strong());
    ui.add_space(SPACE_XS);
}

fn curve_options(ui: &mut egui::Ui, curve: &mut Curve) -> bool {
    // The change has to be gathered here: `show_ui` answers with an
    // `InnerResponse`, which carries no `changed`, so a combo that only edits
    // through the closure reports nothing back to its caller.
    let mut changed = false;
    changed |= ui
        .selectable_value(curve, Curve::Linear, "linear")
        .changed();
    changed |= ui
        .selectable_value(curve, Curve::Exponential { exponent: 1.6 }, "soft")
        .changed();
    changed |= ui
        .selectable_value(curve, Curve::Exponential { exponent: 2.5 }, "softer")
        .changed();
    changed |= ui
        .selectable_value(curve, Curve::smooth(), "S-curve")
        .changed();
    changed |= ui
        .selectable_value(
            curve,
            Curve::Bezier {
                x: [0.0, 0.55, 1.0],
                y: [0.0, 0.82, 1.0],
            },
            "quick off centre",
        )
        .changed();
    changed
}

fn curve_label(curve: &Curve) -> &'static str {
    match *curve {
        Curve::Linear => "linear",
        Curve::Exponential { exponent } if exponent < 2.0 => "soft",
        Curve::Exponential { .. } => "softer",
        Curve::Bezier {
            x: [0.0, 0.4, ..], ..
        } => "S-curve",
        Curve::Bezier { .. } => "quick off centre",
        Curve::Stepped { .. } => "stepped",
    }
}

/// What the selected response does, in one sentence, so the choice is not a
/// guess made from a word.
fn curve_note(curve: &Curve) -> &'static str {
    match *curve {
        Curve::Linear => "Straight through. What the stick does, unedited.",
        Curve::Exponential { .. } => "Quiet near centre, quick at the edges.",
        Curve::Bezier {
            x: [0.0, 0.4, ..], ..
        } => "Slow at both ends, fast through the middle.",
        Curve::Bezier { .. } => "Leaves centre fast, then eases into full lock.",
        Curve::Stepped { .. } => "Digital: on or off past the threshold.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shape a preset through its own filter, the way the engine would.
    fn steer(preset: &Preset, x: f32) -> f32 {
        let mut f = preset.steer.to_filter();
        f.apply(x)
    }

    /// A preset is only worth a button if it changes the shape, and the shape
    /// each one is named after is a claim. This checks the claim: sim is the
    /// quiet one, arcade is the direct one, drift is the one that leaves centre
    /// fast.
    ///
    /// The failure this guards against is real and quiet — retuning one preset
    /// until it matches another, which leaves three buttons doing one job.
    #[test]
    fn each_preset_is_the_shape_it_is_named_after() {
        let sim = &PRESETS[0].1;
        let arcade = &PRESETS[1].1;
        let drift = &PRESETS[2].1;

        assert!(
            matches!(sim.steer.curve, Curve::Exponential { .. }),
            "sim should be exponential"
        );
        assert_eq!(
            arcade.steer.curve,
            Curve::Linear,
            "arcade should be a straight stick"
        );
        assert!(
            matches!(drift.steer.curve, Curve::Bezier { .. }),
            "drift should leave centre fast"
        );

        // Sim asks the driver to mean it; the other two do not.
        assert!(
            sim.steer.deadzone > arcade.steer.deadzone,
            "sim must ignore small corrections the others would act on"
        );
    }

    /// Storing different numbers is not the same as driving differently. A
    /// driver spends most of a corner between centre and a quarter lock, so two
    /// curves that agree there are one preset under two names however different
    /// they look in the JSON.
    #[test]
    fn the_presets_actually_drive_differently() {
        let quarter = 0.25;
        let sim = steer(&PRESETS[0].1, quarter);
        let arcade = steer(&PRESETS[1].1, quarter);
        let drift = steer(&PRESETS[2].1, quarter);

        assert!(
            (sim - arcade).abs() > 0.05,
            "sim and arcade feel the same at a quarter lock: {sim} vs {arcade}"
        );
        assert!(
            (drift - arcade).abs() > 0.05,
            "drift and arcade feel the same at a quarter lock: {drift} vs {arcade}"
        );
    }

    /// A racing axis that leaves a gap right after its dead zone is unusable
    /// exactly where it is being driven, so every preset keeps the travel it
    /// took away, and neither pedal is ever left unable to reach full travel.
    #[test]
    fn no_preset_leaves_a_dead_axis() {
        for (name, preset) in PRESETS {
            assert!(
                preset.steer.anti_deadzone,
                "{name}: steering must not lose travel after its dead zone"
            );
            assert!(
                preset.throttle.sensitivity > 0.0,
                "{name}: throttle must still respond"
            );
            assert!(
                preset.brake.sensitivity > 0.0,
                "{name}: brake must still respond"
            );
            // Full lock has to stay reachable, or the gain is a cap in disguise.
            assert!(
                steer(preset, 1.0) >= 0.95,
                "{name}: steering must still reach full lock"
            );
        }
    }
}
