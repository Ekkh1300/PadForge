//! Racing: the shape of the steering axis, and the two pedals.
//!
//! The Controller page edits the same axes, but as six independent sets of
//! numbers. Racing is not six independent things: it is one steering axis whose
//! *shape* a driver feels through the wheel, and two pedals whose travel decides
//! whether a car settles or snaps. So this page leads with the shape itself —
//! drawn, not described — because a deadzone slider and a sensitivity slider
//! cannot between them say what their combination does, and the combination is
//! the entire subject.

use std::time::{Duration, Instant};

use egui::RichText;
use padcore::engine::{EngineCommand, WheelControl};
use padcore::filters::{Curve, Smoothing};
use padcore::profile::{AxisSettings, Profile};
use padcore::wheel::{self, WheelBinding, WheelTarget};

use crate::state::ToastLevel;
use crate::theme::*;
use crate::widgets;

/// How long the learner waits for a control to be moved before it gives up.
///
/// Long enough to read the prompt and find the pedal, short enough that a
/// player who clicked by accident is not stuck waiting for it to expire.
const LEARN_TIMEOUT: Duration = Duration::from_secs(20);

/// How far a control must move from where it was when learning began, before
/// that counts as the player choosing it.
///
/// Set above the noise a resting sensor actually makes. A DualShock's motion
/// axes drift by more than a tenth on their own, and a learner that fired on
/// that would bind a control the player had not touched.
const LEARN_THRESHOLD: f32 = 0.25;

/// How stale the wheel list may be before it is enumerated again.
///
/// Enumerating is a round trip to the driver, and the list only changes when
/// something is plugged in or taken out — which is not sixty times a second.
const WHEEL_SCAN_INTERVAL: Duration = Duration::from_secs(5);

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

    ui.add_space(SPACE_MD);
    // Last, because it is the other half of the problem. Everything above edits
    // what a DualShock's stick does; this is for a wheel that is plugged in.
    wheel(ui, ctx, &telemetry);
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

/// A real steering wheel, as an input device rather than a shape of axis.
///
/// Everything above this edits what a DualShock's stick does. This is the other
/// half of the problem: a wheel that is plugged in, and what each of its
/// controls should be asked to do.
fn wheel(ui: &mut egui::Ui, ctx: &mut super::Ctx, t: &padcore::engine::Telemetry) {
    card().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Steering wheel")
                    .color(TEXT)
                    .size(14.0)
                    .strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut enabled = ctx.state.settings.wheel.enabled;
                if ui
                    .checkbox(
                        &mut enabled,
                        RichText::new("drive from the wheel").size(12.0),
                    )
                    .changed()
                {
                    ctx.state.settings.wheel.enabled = enabled;
                    push_settings(ctx);
                }
            });
        });
        ui.label(
            RichText::new(
                "Reads any HID joystick, so no wheel needs to be on a supported list. \
                 Bind a target, move that control, and it is learned.",
            )
            .color(TEXT_FAINT)
            .size(11.0),
        );
        ui.add_space(SPACE_SM);

        picker(ui, ctx);
        ui.add_space(SPACE_MD);
        divider(ui);
        ui.add_space(SPACE_SM);

        learn_step(ctx, t);

        // Said out loud rather than left to the button. A control that has to
        // be moved is not something a player can guess at from a label, and the
        // alternative to telling them is a row that quietly does nothing until
        // they work out why.
        if let Some(target) = ctx.state.learning {
            let prompt = RichText::new(format!("move the control for {}", target.label()))
                .color(ACCENT)
                .size(12.5)
                .strong();
            ui.horizontal(|ui| {
                ui.label(prompt);
                if ui.small_button("cancel").clicked() {
                    ctx.state.learning = None;
                }
            });
            ui.add_space(SPACE_SM);
        }

        binding_list(ui, ctx, t);
    });
}

/// Which wheel to read, out of the ones that report as one.
///
/// A list rather than a guess at which is the wheel: these usages are what a
/// wheel declares itself to be, and also what a flight stick, a guitar and an
/// arcade pad declare. Asking is the only answer that is right for all of them.
fn picker(ui: &mut egui::Ui, ctx: &mut super::Ctx) {
    refresh_wheels(ctx);

    ui.horizontal(|ui| {
        let wheels = &ctx.state.wheels;
        if wheels.is_empty() {
            // Said plainly rather than left as a dead end. The page still works
            // without a wheel — the left stick steers and L2/R2 are the pedals,
            // which is how a pad plays these games anyway — so the honest thing
            // is to say that, not to imply the page has nothing to offer.
            ui.label(
                RichText::new(
                    "no joystick found — plug a wheel in and it appears here. \
                     Without one, steering comes from the pad's left stick and \
                     the pedals from L2 and R2.",
                )
                .color(TEXT_FAINT)
                .size(11.5),
            );
        } else {
            let chosen = ctx.state.settings.wheel.device.clone();
            let selected = wheels
                .iter()
                .find(|d| Some(d.path.as_str()) == chosen.as_deref());
            let text = match selected {
                Some(d) => format!("{}  {}", d.label, d.ids()),
                None => "choose a wheel".to_string(),
            };
            let mut picked = false;
            egui::ComboBox::from_id_salt("racing-wheel-device")
                .selected_text(text)
                .width(ui.available_width().min(320.0))
                .show_ui(ui, |ui| {
                    for device in wheels {
                        let is_chosen = Some(device.path.as_str()) == chosen.as_deref();
                        let label = format!("{}  {}", device.label, device.ids());
                        if ui.selectable_label(is_chosen, label).clicked() {
                            ctx.state.settings.wheel.device = Some(device.path.clone());
                            picked = true;
                        }
                    }
                });
            if picked {
                push_settings(ctx);
            }
        }
        if ui.small_button("rescan").clicked() {
            ctx.state.wheels_scanned_at = None;
        }
    });

    // A wheel that was chosen and is now gone should say so where the choice is,
    // rather than the player discovering it by the steering doing nothing.
    let chosen = ctx.state.settings.wheel.device.clone();
    if let Some(chosen) = chosen.as_deref() {
        let attached = ctx.state.wheels.iter().any(|d| d.path == chosen);
        if !attached {
            ui.label(
                RichText::new("that wheel is not attached")
                    .color(TEXT_MUTED)
                    .size(11.0),
            );
        }
    }
}

/// Enumerate wheels when the cached list is old enough to be worth redoing.
fn refresh_wheels(ctx: &mut super::Ctx) {
    let stale = match ctx.state.wheels_scanned_at {
        Some(at) => at.elapsed() > WHEEL_SCAN_INTERVAL,
        None => true,
    };
    if !stale {
        return;
    }
    ctx.state.wheels = wheel::devices();
    ctx.state.wheels_scanned_at = Some(Instant::now());
}

/// One frame of the learn flow: watch for a control being moved, and bind it.
///
/// Runs every frame while a target is waiting, so the whole flow lives in the
/// reader rather than in a modal that would have to hold the wheel open.
fn learn_step(ctx: &mut super::Ctx, t: &padcore::engine::Telemetry) {
    let Some(target) = ctx.state.learning else {
        return;
    };

    // Asked before anything else, because a wheel that is unplugged halfway
    // through learning reports nothing, and a prompt that could not expire
    // would be one the player could only dismiss by restarting.
    if ctx
        .state
        .learning_started
        .is_some_and(|at| at.elapsed() > LEARN_TIMEOUT)
    {
        ctx.state.learning = None;
        ctx.state.toast(
            format!("learning {} timed out", target.label()),
            ToastLevel::Warning,
        );
        return;
    }

    let Some(w) = t.wheel.as_ref() else {
        // Nothing reporting yet. The prompt stays up — the player may still be
        // plugging the wheel in — and the timeout above is what ends it.
        return;
    };

    // The most a control has moved since learning began. Baseline first, so a
    // pedal resting a few percent off its end is not mistaken for a press.
    let rest = &ctx.state.learning_rest;
    let Some((id, moved)) = w
        .controls
        .iter()
        .filter_map(|c| {
            let base = rest.iter().find(|(i, _)| *i == c.id).map(|(_, v)| *v)?;
            Some((c.id, (c.value - base).abs()))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
    else {
        return;
    };

    if moved < LEARN_THRESHOLD {
        return;
    }

    let mut wheel = ctx.state.settings.wheel.clone();
    // Unbind first, so re-learning a target cannot leave two bindings claiming
    // one slot and the list showing something other than what the fold does.
    wheel.unbind(target);
    wheel.bindings.push(WheelBinding::new(target, id));
    wheel.enabled = true;
    ctx.state.settings.wheel = wheel;
    ctx.state.learning = None;
    push_settings(ctx);

    let label = w
        .controls
        .iter()
        .find(|c| c.id == id)
        .map(|c| c.label.clone())
        .unwrap_or_default();
    ctx.state.toast(
        format!("{} learned as {label}", target.label()),
        ToastLevel::Success,
    );
}

/// Every target, what is bound to it, and the knobs that shape it.
fn binding_list(ui: &mut egui::Ui, ctx: &mut super::Ctx, t: &padcore::engine::Telemetry) {
    let controls: Vec<WheelControl> = t
        .wheel
        .as_ref()
        .map(|w| w.controls.clone())
        .unwrap_or_default();
    let mut wheel = ctx.state.settings.wheel.clone();
    let mut changed = false;

    for target in WheelTarget::ALL {
        ui.horizontal(|ui| {
            ui.add_sized(
                [86.0, 22.0],
                egui::Label::new(RichText::new(target.label()).color(TEXT_MUTED)),
            );

            let position = wheel.bindings.iter().position(|b| b.target == *target);

            if controls.is_empty() {
                // Nothing to bind without a wheel reporting, and saying which
                // is more use than a control that silently does nothing.
                ui.label(
                    RichText::new("attach a wheel to bind this")
                        .color(TEXT_FAINT)
                        .size(11.0),
                );
                return;
            }

            let mut chosen = position;
            egui::ComboBox::from_id_salt(format!("racing-wheel-{}", target.label()))
                .selected_text(match position {
                    Some(i) => controls
                        .iter()
                        .find(|c| c.id == wheel.bindings[i].control)
                        .map(|c| c.label.clone())
                        .unwrap_or_else(|| "bound".into()),
                    None => "not bound".into(),
                })
                .width(180.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(position.is_none(), "not bound")
                        .clicked()
                    {
                        chosen = None;
                    }
                    for control in &controls {
                        let is_bound =
                            position.is_some_and(|i| wheel.bindings[i].control == control.id);
                        if ui
                            .selectable_label(is_bound, control.label.clone())
                            .clicked()
                        {
                            // Replacing rather than adding: one control drives
                            // one target, and a second binding for the same
                            // target is a state the list cannot show.
                            wheel.unbind(*target);
                            wheel.bindings.push(WheelBinding::new(*target, control.id));
                            chosen = wheel.bindings.len().checked_sub(1);
                        }
                    }
                });
            if chosen != position {
                if let Some(i) = position {
                    wheel.bindings.remove(i);
                }
                changed = true;
            }

            // Live: where the bound control is right now. Watching it move is
            // how a player confirms they picked the right one, which reading a
            // name never settles.
            if let Some(i) = position {
                let value = controls
                    .iter()
                    .find(|c| c.id == wheel.bindings[i].control)
                    .map(|c| c.value);
                if let Some(value) = value {
                    ui.add(
                        egui::ProgressBar::new(value)
                            .desired_width(ui.available_width().min(140.0))
                            .animate(true),
                    );
                }
            }

            if ui
                .small_button(if ctx.state.learning == Some(*target) {
                    "..."
                } else {
                    "learn"
                })
                .clicked()
            {
                // The baseline is captured here rather than taken as "zero",
                // because a control at rest is very often not at zero and the
                // learner has to know where each one started.
                ctx.state.learning_rest = controls.iter().map(|c| (c.id, c.value)).collect();
                ctx.state.learning = Some(*target);
                ctx.state.learning_started = Some(Instant::now());
            }
        });
    }

    if changed {
        ctx.state.settings.wheel = wheel;
        push_settings(ctx);
    }
}

/// Send the edited settings to the engine.
///
/// The engine holds its own copy and reopens the wheel from it, so a change
/// here is not live until it is sent.
fn push_settings(ctx: &mut super::Ctx) {
    let settings = ctx.state.settings.clone();
    ctx.state.settings_dirty = true;
    ctx.engine
        .send(EngineCommand::ApplySettings(Box::new(settings)));
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
