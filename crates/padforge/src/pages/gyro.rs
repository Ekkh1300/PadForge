//! Motion page: gyro aiming, touchpad routing, and their live tuning.
use egui::RichText;
use padcore::engine::EngineCommand;
use padcore::gyro::{GyroAxisMode, GyroInvert, GyroOutputMode, GyroSmoothing};
use padcore::pointer::{GyroPointerAxis, JitterCompensation};
use padcore::touchpad::TouchpadMode;

use crate::state::ToastLevel;
use crate::theme::*;
use crate::widgets;

pub fn draw(ui: &mut egui::Ui, ctx: &mut super::Ctx) {
    let telemetry = ctx.engine.telemetry();
    ui.add_space(SPACE_SM);

    ui.columns(2, |cols| {
        gyro_card(&mut cols[0], ctx, &telemetry);
        touchpad_card(&mut cols[1], ctx);
    });
}

/// Gyro aiming, with a live trace of the output.
fn gyro_card(ui: &mut egui::Ui, ctx: &mut super::Ctx, t: &padcore::engine::Telemetry) {
    card().show(ui, |ui| {
        let mut enabled = ctx.state.gyro.enabled;

        ui.horizontal(|ui| {
            ui.checkbox(&mut enabled, "");
            ui.label(RichText::new("Gyro aiming").color(TEXT).size(15.0).strong());
        });

        if enabled != ctx.state.gyro.enabled {
            ctx.state.gyro.enabled = enabled;
            push_profile(ctx);
        }

        ui.add_space(SPACE_SM);
        ui.label(
            RichText::new(
                "The DS4 reports rotation speed, not angle, so this integrates over time. \
                 Smoothing keeps it usable; too much makes it feel like syrup.",
            )
            .color(TEXT_MUTED)
            .size(12.0),
        );

        if !ctx.state.gyro.enabled {
            return;
        }

        ui.add_space(SPACE_MD);

        // --- output routing ---------------------------------------------
        ui.label(RichText::new("OUTPUT").color(TEXT_FAINT).size(11.0).strong());
        ui.horizontal_wrapped(|ui| {
            let mode = ctx.state.gyro.output_mode;
            for (value, label) in [
                (GyroOutputMode::Mouse, "Mouse"),
                (GyroOutputMode::Stick, "Left stick"),
                (GyroOutputMode::RightStick, "Right stick"),
                (GyroOutputMode::Triggers, "Triggers"),
            ] {
                let selected = mode == value;
                let btn = egui::Button::new(
                    RichText::new(label)
                        .color(if selected { BACKDROP } else { TEXT })
                        .size(12.5),
                )
                .fill(if selected { ACCENT } else { SURFACE_RAISED })
                .stroke(egui::Stroke::new(
                    1.0,
                    if selected { ACCENT } else { BORDER },
                ))
                .corner_radius(RADIUS_PILL);
                if ui.add(btn).clicked() {
                    ctx.state.gyro.output_mode = value;
                    push_profile(ctx);
                }
            }
        });

        if ctx.state.gyro.output_mode == GyroOutputMode::Mouse {
            // The conversion is live, so show what it is doing rather than a
            // warning about a missing feature.
            if let Some(reason) = &t.pointer_error {
                crate::pages::dashboard::banner(
                    ui,
                    WARNING,
                    "Windows is blocking pointer injection",
                    reason,
                );
            } else {
                ui.label(
                    RichText::new(
                        "Motion is injected through SendInput, so it is indistinguishable\n                         \
                         from a real mouse. Keep the pad still and level while aiming.",
                    )
                    .color(TEXT_MUTED)
                    .size(11.5),
                );
                ui.add_space(SPACE_XS);
                widgets::axis_bar(ui, "pointer X", t.pointer_delta.0 as f32, ACCENT);
                widgets::axis_bar(ui, "pointer Y", t.pointer_delta.1 as f32, ACCENT);
            }
        }

        ui.add_space(SPACE_MD);

        // --- axes and inversion -----------------------------------------
        ui.horizontal(|ui| {
            ui.label(RichText::new("axes").color(TEXT_MUTED));
            let axis = ctx.state.gyro.axis_mode;
            egui::ComboBox::from_id_salt("gyro_axes")
                .selected_text(match axis {
                    GyroAxisMode::YawPitch => "Yaw to X, Pitch to Y",
                    GyroAxisMode::YawRoll => "Yaw to X, Roll to Y",
                    GyroAxisMode::YawOnly => "Yaw only",
                    GyroAxisMode::PitchOnly => "Pitch only",
                })
                .width(190.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut ctx.state.gyro.axis_mode,
                        GyroAxisMode::YawPitch,
                        "Yaw to X, Pitch to Y",
                    );
                    ui.selectable_value(
                        &mut ctx.state.gyro.axis_mode,
                        GyroAxisMode::YawRoll,
                        "Yaw to X, Roll to Y",
                    );
                    ui.selectable_value(
                        &mut ctx.state.gyro.axis_mode,
                        GyroAxisMode::YawOnly,
                        "Yaw only",
                    );
                    ui.selectable_value(
                        &mut ctx.state.gyro.axis_mode,
                        GyroAxisMode::PitchOnly,
                        "Pitch only",
                    );
                });
        });

        ui.horizontal(|ui| {
            ui.label(RichText::new("invert").color(TEXT_MUTED));
            let inv = ctx.state.gyro.invert;
            egui::ComboBox::from_id_salt("gyro_invert")
                .selected_text(match inv {
                    GyroInvert::None => "none",
                    GyroInvert::Horizontal => "horizontal",
                    GyroInvert::Vertical => "vertical",
                    GyroInvert::Both => "both",
                })
                .width(150.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut ctx.state.gyro.invert, GyroInvert::None, "none");
                    ui.selectable_value(
                        &mut ctx.state.gyro.invert,
                        GyroInvert::Horizontal,
                        "horizontal",
                    );
                    ui.selectable_value(
                        &mut ctx.state.gyro.invert,
                        GyroInvert::Vertical,
                        "vertical",
                    );
                    ui.selectable_value(&mut ctx.state.gyro.invert, GyroInvert::Both, "both");
                });
        });

        ui.add_space(SPACE_MD);

        // --- tuning -----------------------------------------------------
        section(ui, "Tuning");
        {
            let g = &mut ctx.state.gyro;
            let sensitivity = &mut g.sensitivity;
            slider(ui, "sensitivity", sensitivity, 0.1..=4.0, "x");
            let max_output = &mut g.max_output;
            slider(ui, "max output", max_output, 1.0..=80.0, "");
            let deadzone = &mut g.deadzone;
            slider(ui, "deadzone", deadzone, 0.0..=12.0, "deg/s");
            let min_threshold = &mut g.min_threshold;
            slider(ui, "min threshold", min_threshold, 0.0..=2.0, "");
            let max_rate = &mut g.max_rate;
            slider(ui, "max rate", max_rate, 60.0..=1200.0, "deg/s");
        }

        ui.add_space(SPACE_MD);

        // --- smoothing --------------------------------------------------
        ui.label(RichText::new("smoothing").color(TEXT_MUTED).size(12.0));
        ui.horizontal_wrapped(|ui| {
            let current = ctx.state.gyro.smoothing;
            let options: [(GyroSmoothing, &str, &str); 3] = [
                (GyroSmoothing::None, "none", "Raw angular rates. Noisy."),
                (
                    GyroSmoothing::LowPass { cutoff: 25.0 },
                    "low-pass",
                    "Smooth at any speed, but adds latency.",
                ),
                (
                    GyroSmoothing::OneEuro {
                        min_cutoff: 1.0,
                        beta: 0.05,
                    },
                    "1-euro",
                    "Smooth when still, sharp when moving fast. The best default.",
                ),
            ];
            for (value, label, hint) in options {
                let selected = matches!(
                    (current, &value),
                    (GyroSmoothing::None, GyroSmoothing::None)
                        | (GyroSmoothing::LowPass { .. }, GyroSmoothing::LowPass { .. })
                        | (GyroSmoothing::OneEuro { .. }, GyroSmoothing::OneEuro { .. })
                );
                let btn = egui::Button::new(
                    RichText::new(label)
                        .color(if selected { BACKDROP } else { TEXT })
                        .size(12.0),
                )
                .fill(if selected { ACCENT } else { SURFACE_RAISED })
                .stroke(egui::Stroke::new(
                    1.0,
                    if selected { ACCENT } else { BORDER },
                ))
                .corner_radius(RADIUS_SM);
                if ui.add(btn).on_hover_text(hint).clicked() {
                    ctx.state.gyro.smoothing = value;
                    push_profile(ctx);
                }
            }
        });

        // Parameters belong to the active filter, so they come after it.
        match ctx.state.gyro.smoothing {
            GyroSmoothing::None => {}
            GyroSmoothing::LowPass { cutoff } => {
                let mut v = cutoff;
                if ui
                    .add(
                        egui::Slider::new(&mut v, 1.0..=120.0)
                            .text("cutoff")
                            .suffix(" Hz")
                            .show_value(true),
                    )
                    .changed()
                {
                    ctx.state.gyro.smoothing = GyroSmoothing::LowPass { cutoff: v };
                    push_profile(ctx);
                }
            }
            GyroSmoothing::OneEuro { min_cutoff, beta } => {
                let mut mc = min_cutoff;
                let changed_cutoff = ui
                    .add(
                        egui::Slider::new(&mut mc, 0.5..=30.0)
                            .text("min cutoff")
                            .suffix(" Hz")
                            .show_value(true),
                    )
                    .changed();
                let mut b = beta;
                let changed_beta = ui
                    .add(
                        egui::Slider::new(&mut b, 0.0..=20.0)
                            .text("beta")
                            .show_value(true),
                    )
                    .changed();
                if changed_cutoff || changed_beta {
                    ctx.state.gyro.smoothing = GyroSmoothing::OneEuro {
                        min_cutoff: mc,
                        beta: b,
                    };
                    push_profile(ctx);
                }
            }
        }

        ui.add_space(SPACE_MD);
        section(ui, "Pointer");
        let pointer = ctx.state.editing_profile().pointer;
        let mut sensitivity = pointer.gyro_sensitivity;
        if ui
            .add(
                egui::Slider::new(&mut sensitivity, 10.0..=400.0)
                    .suffix("%")
                    .show_value(true),
            )
            .changed()
        {
            ctx.state.editing_profile_mut().pointer.gyro_sensitivity = sensitivity;
            push_profile(ctx);
        }
        let mut vertical = pointer.gyro_vertical_scale;
        if ui
            .add(
                egui::Slider::new(&mut vertical, 10.0..=400.0)
                    .suffix("%")
                    .show_value(true),
            )
            .changed()
        {
            ctx.state.editing_profile_mut().pointer.gyro_vertical_scale = vertical;
            push_profile(ctx);
        }
        let mut threshold = pointer.min_threshold;
        if ui
            .add(
                egui::Slider::new(&mut threshold, 0.0..=8.0)
                    .suffix(" px")
                    .show_value(true),
            )
            .changed()
        {
            ctx.state.editing_profile_mut().pointer.min_threshold = threshold;
            push_profile(ctx);
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("horizontal axis").color(TEXT_MUTED));
            let axis = pointer.gyro_axis;
            egui::ComboBox::from_id_salt("gyro_pointer_axis")
                .selected_text(match axis {
                    GyroPointerAxis::Yaw => "Yaw",
                    GyroPointerAxis::Roll => "Roll",
                })
                .width(120.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut ctx.state.editing_profile_mut().pointer.gyro_axis,
                        GyroPointerAxis::Yaw,
                        "Yaw",
                    );
                    ui.selectable_value(
                        &mut ctx.state.editing_profile_mut().pointer.gyro_axis,
                        GyroPointerAxis::Roll,
                        "Roll",
                    );
                });
        });
        ui.horizontal(|ui| {
            ui.add_space(SPACE_SM);
            // `checkbox` needs a `&mut bool`, so mirror the config enum into a
            // local and write it back on change.
            let mut jitter = ctx.state.editing_profile().pointer.jitter == JitterCompensation::Power;
            if ui.checkbox(&mut jitter, "jitter compensation").changed() {
                ctx.state.editing_profile_mut().pointer.jitter = if jitter {
                    JitterCompensation::Power
                } else {
                    JitterCompensation::Off
                };
                push_profile(ctx);
            }
        });

        ui.add_space(SPACE_MD);
        divider(ui);
        ui.add_space(SPACE_SM);

        widgets::axis_bar(ui, "gyro X", t.gyro_delta.0, ACCENT);
        widgets::axis_bar(ui, "gyro Y", t.gyro_delta.1, ACCENT);
        ui.add_space(SPACE_XS);
        ui.label(RichText::new("recent output").color(TEXT_FAINT).size(11.0));
        widgets::sparkline(ui, &ctx.state.gyro_history, 40.0, ACCENT);

        ui.add_space(SPACE_XS);
        let raw = &t.report.gyro;
        egui::Grid::new("gyro_raw")
            .num_columns(2)
            .spacing([SPACE_MD, 2.0])
            .show(ui, |ui| {
                for (name, v) in [("yaw", raw.yaw), ("pitch", raw.pitch), ("roll", raw.roll)] {
                    ui.label(RichText::new(name).color(TEXT_MUTED).size(11.5));
                    ui.label(
                        RichText::new(format!("{:+7.1} deg/s", v))
                            .color(TEXT)
                            .size(11.5)
                            .monospace(),
                    );
                    ui.end_row();
                }
            });
    });
}

/// Touchpad routing and gestures.
fn touchpad_card(ui: &mut egui::Ui, ctx: &mut super::Ctx) {
    // Needed for the live pointer readout in mouse mode.
    let t = ctx.engine.telemetry();
    card().show(ui, |ui| {
        ui.label(RichText::new("Touchpad").color(TEXT).size(15.0).strong());
        ui.add_space(SPACE_SM);

        let mode = ctx.state.editing_profile().touchpad.mode;
        egui::ComboBox::from_id_salt("tp_mode")
            .selected_text(match mode {
                TouchpadMode::Off => "Off",
                TouchpadMode::Mouse => "Pointer",
                TouchpadMode::Dpad => "D-pad",
                TouchpadMode::Gestures => "Gestures",
            })
            .width(160.0)
            .show_ui(ui, |ui| {
                for (value, label) in [
                    (TouchpadMode::Off, "Off"),
                    (TouchpadMode::Mouse, "Pointer"),
                    (TouchpadMode::Dpad, "D-pad"),
                    (TouchpadMode::Gestures, "Gestures"),
                ] {
                    ui.selectable_value(
                        &mut ctx.state.editing_profile_mut().touchpad.mode,
                        value,
                        label,
                    );
                }
            });
        push_profile(ctx);

        match mode {
            TouchpadMode::Off => {
                ui.add_space(SPACE_SM);
                ui.label(
                    RichText::new(
                        "The touchpad surface is ignored. Its click can still be mapped on the \
                         Mapping page.",
                    )
                    .color(TEXT_FAINT)
                    .size(12.0),
                );
                return;
            }
            TouchpadMode::Mouse => {
                if let Some(reason) = &t.pointer_error {
                    crate::pages::dashboard::banner(
                        ui,
                        WARNING,
                        "Windows is blocking pointer injection",
                        reason,
                    );
                } else {
                    ui.add_space(SPACE_SM);
                    ui.label(
                        RichText::new(
                            "A finger crossing the pad moves the pointer one to one at 100\n                             \
                             sensitivity. The pad click can also act as a mouse button.",
                        )
                        .color(TEXT_MUTED)
                        .size(12.0),
                    );
                    ui.add_space(SPACE_XS);
                    widgets::axis_bar(ui, "pointer X", t.pointer_delta.0 as f32, ACCENT);
                    widgets::axis_bar(ui, "pointer Y", t.pointer_delta.1 as f32, ACCENT);
                }
            }
            TouchpadMode::Gestures => {
                ui.add_space(SPACE_SM);
                ui.label(
                    RichText::new(
                        "Swipe across the pad to fire a direction. Each swipe needs a lift in \
                         between, so holding a finger will not repeat.",
                    )
                    .color(TEXT_MUTED)
                    .size(12.0),
                );
            }
            TouchpadMode::Dpad => {
                ui.add_space(SPACE_SM);
                ui.label(
                    RichText::new(
                        "Touch the pad and drag; the direction lights up as XInput D-pad buttons.",
                    )
                    .color(TEXT_MUTED)
                    .size(12.0),
                );
            }
        }

        ui.add_space(SPACE_MD);
        section(ui, "Gesture detection");

        let mut threshold = ctx.state.editing_profile().touchpad.swipe_threshold;
        if ui
            .add(
                egui::Slider::new(&mut threshold, 0.1..=0.9)
                    .text("swipe threshold")
                    .suffix("%")
                    .show_value(true),
            )
            .changed()
        {
            ctx.state
                .editing_profile_mut()
                .touchpad
                .swipe_threshold = threshold;
            push_profile(ctx);
        }

        let mut min_duration = ctx.state.editing_profile().touchpad.min_duration_ms;
        if ui
            .add(
                egui::Slider::new(&mut min_duration, 0..=600)
                    .text("minimum hold")
                    .suffix(" ms")
                    .show_value(true),
            )
            .changed()
        {
            ctx.state
                .editing_profile_mut()
                .touchpad
                .min_duration_ms = min_duration;
            push_profile(ctx);
        }

        ui.add_space(SPACE_SM);
        let mut right_click = ctx.state.editing_profile().touchpad.right_click;
        if ui.checkbox(&mut right_click, "click emits a right-click").changed() {
            ctx.state
                .editing_profile_mut()
                .touchpad
                .right_click = right_click;
            push_profile(ctx);
        }

        ui.add_space(SPACE_SM);
        if secondary_button(ui, "Revert touchpad settings").clicked() {
            let fresh = padcore::touchpad::TouchpadConfig::default();
            ctx.state.editing_profile_mut().touchpad = fresh;
            push_profile(ctx);
            ctx.state
                .toast("Touchpad settings reset", ToastLevel::Info);
        }
    });
}

/// A labelled slider with no side effects, so the caller can decide when to push.
fn slider(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    suffix: &str,
) {
    ui.add(
        egui::Slider::new(value, range)
            .text(label)
            .suffix(suffix)
            .show_value(true),
    );
}

/// Push the active profile into the engine after an edit.
fn push_profile(ctx: &mut super::Ctx) {
    let profile = ctx.state.editing_profile();
    ctx.state.gyro = profile.gyro;
    let store = ctx.state.store.clone();
    ctx.engine
        .send(EngineCommand::ApplyProfiles(Box::new(store)));
}
