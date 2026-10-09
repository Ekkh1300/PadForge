//! Controller page: per-axis shaping, lightbar, and stick calibration.

use egui::epaint::MarginF32;
use egui::{Color32, RichText, Stroke};
use padcore::filters::{Curve, Smoothing};
use padcore::profile::AxisSettings;

use crate::state::ToastLevel;
use crate::theme::*;
use crate::widgets::{self, ColourPicker};

/// Every editable axis on the pad, in the order the UI presents them.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Axis {
    LeftX,
    LeftY,
    RightX,
    RightY,
    LeftTrigger,
    RightTrigger,
}

impl Axis {
    /// The two axes of one hardware group, as `(x, y)`.
    const PAIRS: [(Axis, Axis); 3] = [
        (Axis::LeftX, Axis::LeftY),
        (Axis::RightX, Axis::RightY),
        (Axis::LeftTrigger, Axis::RightTrigger),
    ];

    fn label(self) -> &'static str {
        match self {
            Axis::LeftX => "X",
            Axis::LeftY => "Y",
            Axis::RightX => "X",
            Axis::RightY => "Y",
            Axis::LeftTrigger => "L2",
            Axis::RightTrigger => "R2",
        }
    }

    fn full_label(self) -> &'static str {
        match self {
            Axis::LeftX => "Left X",
            Axis::LeftY => "Left Y",
            Axis::RightX => "Right X",
            Axis::RightY => "Right Y",
            Axis::LeftTrigger => "Left trigger",
            Axis::RightTrigger => "Right trigger",
        }
    }

    fn read(self, p: &padcore::profile::Profile) -> AxisSettings {
        match self {
            Axis::LeftX => p.left_x,
            Axis::LeftY => p.left_y,
            Axis::RightX => p.right_x,
            Axis::RightY => p.right_y,
            Axis::LeftTrigger => p.left_trigger,
            Axis::RightTrigger => p.right_trigger,
        }
    }

    fn write(self, p: &mut padcore::profile::Profile, s: AxisSettings) {
        match self {
            Axis::LeftX => p.left_x = s,
            Axis::LeftY => p.left_y = s,
            Axis::RightX => p.right_x = s,
            Axis::RightY => p.right_y = s,
            Axis::LeftTrigger => p.left_trigger = s,
            Axis::RightTrigger => p.right_trigger = s,
        }
    }
}

pub fn draw(ui: &mut egui::Ui, ctx: &mut super::Ctx) {
    let telemetry = ctx.engine.telemetry();
    ui.add_space(SPACE_SM);

    ui.columns(2, |cols| {
        for (title, pair) in [
            ("Left stick", Axis::PAIRS[0]),
            ("Right stick", Axis::PAIRS[1]),
            ("Triggers", Axis::PAIRS[2]),
        ] {
            axis_card(&mut cols[0], ctx, title, pair, &telemetry);
            cols[0].add_space(SPACE_MD);
        }

        lightbar_card(&mut cols[1], ctx, &telemetry);
        cols[1].add_space(SPACE_MD);
        calibration_card(&mut cols[1], ctx, &telemetry);
    });
}

/// One hardware group: live bars plus the settings for each of its two axes.
fn axis_card(
    ui: &mut egui::Ui,
    ctx: &mut super::Ctx,
    title: &str,
    pair: (Axis, Axis),
    t: &padcore::engine::Telemetry,
) {
    let (ax, ay) = pair;
    let live = |axis: Axis| -> f32 {
        let r = &t.report;
        match axis {
            Axis::LeftX => r.left_x,
            Axis::LeftY => r.left_y,
            Axis::RightX => r.right_x,
            Axis::RightY => r.right_y,
            Axis::LeftTrigger => r.l2,
            Axis::RightTrigger => r.r2,
        }
    };
    let is_trigger = matches!(ax, Axis::LeftTrigger | Axis::RightTrigger);

    card().show(ui, |ui| {
        ui.label(RichText::new(title).color(TEXT).size(14.0).strong());
        ui.add_space(SPACE_SM);

        // Live output for both axes.
        for axis in [ax, ay] {
            let value = live(axis);
            if is_trigger {
                let pressed = match axis {
                    Axis::LeftTrigger => t.report.buttons.any(padcore::report::Buttons::L2),
                    _ => t.report.buttons.any(padcore::report::Buttons::R2),
                };
                widgets::trigger_bar(ui, axis.label(), value, pressed);
            } else {
                widgets::axis_bar(ui, axis.label(), value, ACCENT);
            }
        }
        ui.add_space(SPACE_SM);

        for (index, axis) in [ax, ay].iter().enumerate() {
            if index == 1 {
                divider(ui);
            }
            edit_one_axis(ui, ctx, *axis);
        }
    });
}

/// The editing controls for a single axis.
fn edit_one_axis(ui: &mut egui::Ui, ctx: &mut super::Ctx, axis: Axis) {
    // Read the live settings so a slider drag starts from current state.
    let mut s = axis.read(ctx.state.editing_profile());

    ui.label(
        RichText::new(axis.full_label())
            .color(TEXT)
            .size(13.0)
            .strong(),
    );
    ui.add_space(SPACE_XS);

    let mut changed = false;

    // A fixed label column keeps every row aligned regardless of control width.
    const LABEL_W: f32 = 86.0;

    ui.horizontal(|ui| {
        ui.add_sized(
            [LABEL_W, 22.0],
            egui::Label::new(RichText::new("deadzone").color(TEXT_MUTED)),
        );
        changed |= ui
            .add(
                egui::Slider::new(&mut s.deadzone, 0.0..=0.5)
                    .suffix("%")
                    .show_value(true),
            )
            .changed();
    });

    ui.horizontal(|ui| {
        ui.add_sized(
            [LABEL_W, 22.0],
            egui::Label::new(RichText::new("curve").color(TEXT_MUTED)),
        );
        egui::ComboBox::from_id_salt(("curve", axis))
            .selected_text(curve_label(&s.curve))
            .width(ui.available_width())
            .show_ui(ui, |ui| curve_options(ui, &mut s.curve));
    });

    ui.horizontal(|ui| {
        ui.add_sized(
            [LABEL_W, 22.0],
            egui::Label::new(RichText::new("sensitivity").color(TEXT_MUTED)),
        );
        changed |= ui
            .add(
                egui::Slider::new(&mut s.sensitivity, 0.2..=3.0)
                    .suffix("x")
                    .show_value(true),
            )
            .changed();
    });

    ui.horizontal(|ui| {
        ui.add_sized([LABEL_W, 22.0], egui::Label::new(RichText::new("")));
        ui.checkbox(&mut s.anti_deadzone, "anti-deadzone");
        ui.checkbox(&mut s.inverted, "invert");
    });

    ui.horizontal(|ui| {
        ui.add_sized(
            [LABEL_W, 22.0],
            egui::Label::new(RichText::new("smoothing").color(TEXT_MUTED)),
        );
        egui::ComboBox::from_id_salt(("smooth", axis))
            .selected_text(smoothing_label(&s.smoothing))
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut s.smoothing, Smoothing::Off, "off");
                ui.selectable_value(&mut s.smoothing, Smoothing::Exponential { alpha: 0.3 }, "light");
                ui.selectable_value(
                    &mut s.smoothing,
                    Smoothing::Exponential { alpha: 0.15 },
                    "medium",
                );
                ui.selectable_value(
                    &mut s.smoothing,
                    Smoothing::Exponential { alpha: 0.05 },
                    "heavy",
                );
                ui.selectable_value(
                    &mut s.smoothing,
                    Smoothing::WeightedAverage { window: 6 },
                    "windowed average",
                );
            });
    });

    if changed {
        axis.write(ctx.state.editing_profile_mut(), s);
        push_store(ctx);
    }
}

/// Lightbar colour, animation, and brightness.
fn lightbar_card(ui: &mut egui::Ui, ctx: &mut super::Ctx, t: &padcore::engine::Telemetry) {
    card().show(ui, |ui| {
        let mut enabled = ctx.state.editing_profile().lightbar.enabled;
        let mut expanded = ctx.state.show_lightbar;

        ui.horizontal(|ui| {
            ui.checkbox(&mut enabled, "");
            ui.label(RichText::new("Lightbar").color(TEXT).size(14.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(if expanded { "v" } else { ">" })
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::NONE),
                    )
                    .clicked()
                {
                    expanded = !expanded;
                }
            });
        });

        if enabled != ctx.state.editing_profile().lightbar.enabled {
            ctx.state.editing_profile_mut().lightbar.enabled = enabled;
            push_store(ctx);
        }
        ctx.state.show_lightbar = expanded;

        ui.add_space(SPACE_SM);

        if !enabled {
            ui.label(
                RichText::new("The lightbar is off for this profile.")
                    .color(TEXT_FAINT)
                    .size(12.0),
            );
            return;
        }
        if !expanded {
            return;
        }

        // Mode.
        ui.horizontal(|ui| {
            ui.label(RichText::new("mode").color(TEXT_MUTED));
            let current = ctx.state.editing_profile().lightbar.mode;
            egui::ComboBox::from_id_salt("lightbar_mode")
                .selected_text(current.label())
                .width(140.0)
                .show_ui(ui, |ui| {
                    for mode in padcore::profile::LightbarMode::ALL {
                        let selected = current == *mode;
                        ui.selectable_value(
                            &mut ctx.state.editing_profile_mut().lightbar.mode,
                            *mode,
                            mode.label(),
                        );
                        let _ = selected;
                    }
                });
        });

        // Rainbow and Blue have fixed colours, so a picker would mislead.
        let mode = ctx.state.editing_profile().lightbar.mode;
        let fixed_colour = matches!(
            mode,
            padcore::profile::LightbarMode::Rainbow | padcore::profile::LightbarMode::Blue
        );
        if fixed_colour {
            ui.add_space(SPACE_SM);
            ui.label(
                RichText::new("This mode uses its own colour.")
                    .color(TEXT_FAINT)
                    .size(11.5),
            );
        } else {
            ui.add_space(SPACE_SM);
            let mut picker = ColourPicker::new(ctx.state.editing_profile().lightbar.color);
            picker.show(ui);
            let rgb = picker.rgb();
            if rgb != ctx.state.editing_profile().lightbar.color {
                ctx.state.editing_profile_mut().lightbar.color = rgb;
                push_store(ctx);
            }
        }

        ui.add_space(SPACE_MD);
        let mut brightness = ctx.state.editing_profile().lightbar.brightness;
        if ui
            .add(
                egui::Slider::new(&mut brightness, 0.05..=1.0)
                    .text("brightness")
                    .suffix("%")
                    .show_value(true),
            )
            .changed()
        {
            ctx.state.editing_profile_mut().lightbar.brightness = brightness;
            push_store(ctx);
        }

        if matches!(
            mode,
            padcore::profile::LightbarMode::Flash
                | padcore::profile::LightbarMode::Breathe
                | padcore::profile::LightbarMode::PulseFade
        ) {
            let mut rate = ctx.state.editing_profile().lightbar.rate;
            if ui
                .add(
                    egui::Slider::new(&mut rate, 0.1..=6.0)
                        .text("animation speed")
                        .suffix(" Hz")
                        .show_value(true),
                )
                .changed()
            {
                ctx.state.editing_profile_mut().lightbar.rate = rate;
                push_store(ctx);
            }
        }

        ui.add_space(SPACE_SM);
        // What the pad is actually showing right now.
        let live = egui::Color32::from_rgb(t.lightbar[0], t.lightbar[1], t.lightbar[2]);
        ui.horizontal(|ui| {
            ui.label(RichText::new("live").color(TEXT_MUTED).size(11.0));
            egui::Frame::new()
                .fill(live)
                .corner_radius(RADIUS_SM)
                .inner_margin(MarginF32::same(4.0))
                .show(ui, |ui| {
                    ui.label(RichText::new(" ").size(10.0));
                });
        });
    });
}

/// The captured resting offsets, and a recalibrate button.
fn calibration_card(ui: &mut egui::Ui, ctx: &mut super::Ctx, t: &padcore::engine::Telemetry) {
    card().show(ui, |ui| {
        ui.label(
            RichText::new("Stick calibration")
                .color(TEXT)
                .size(14.0)
                .strong(),
        );
        ui.add_space(SPACE_SM);

        let cal = t.calibration;
        ui.label(
            RichText::new(if cal.is_captured() {
                "Offsets are being tracked from the pad's resting position."
            } else {
                "No calibration yet. Leave the sticks at rest for a moment."
            })
            .color(if cal.is_captured() { SUCCESS } else { TEXT_MUTED })
            .size(12.5),
        );

        ui.add_space(SPACE_SM);
        egui::Grid::new("cal_grid")
            .num_columns(4)
            .spacing([SPACE_MD, SPACE_XS])
            .show(ui, |ui| {
                let (lx, ly, rx, ry) = cal.offsets();
                for (name, value) in [
                    ("Left X", lx),
                    ("Left Y", ly),
                    ("Right X", rx),
                    ("Right Y", ry),
                ] {
                    ui.label(RichText::new(name).color(TEXT_MUTED).size(12.0));
                    ui.label(
                        RichText::new(format!("{:+}", value))
                            .color(TEXT)
                            .size(12.0)
                            .monospace(),
                    );
                    ui.end_row();
                }
            });

        ui.add_space(SPACE_SM);
        if secondary_button(ui, "Recalibrate now").clicked() {
            ctx.engine
                .send(padcore::engine::EngineCommand::Recalibrate);
            ctx.state
                .toast("Hold the sticks still...", ToastLevel::Info);
        }
    });
}

fn curve_label(curve: &Curve) -> String {
    match curve {
        Curve::Linear => "Linear".into(),
        Curve::Exponential { exponent } => format!("Expo {exponent:.1}"),
        Curve::Bezier { .. } => "Custom bezier".into(),
        Curve::Stepped { .. } => "Stepped".into(),
    }
}

fn curve_options(ui: &mut egui::Ui, curve: &mut Curve) {
    ui.selectable_value(curve, Curve::Linear, "Linear");
    for e in [1.5f32, 2.0, 3.0, 4.5] {
        ui.selectable_value(
            curve,
            Curve::Exponential { exponent: e },
            format!("Expo {e:.1}"),
        );
    }
    ui.selectable_value(curve, Curve::smooth(), "Smooth S-curve");
    ui.selectable_value(
        curve,
        Curve::Bezier {
            x: [0.0, 0.2, 1.0],
            y: [0.0, 0.95, 1.0],
        },
        "Fast early",
    );
    ui.selectable_value(
        curve,
        Curve::Bezier {
            x: [0.0, 0.8, 1.0],
            y: [0.0, 0.35, 1.0],
        },
        "Slow early",
    );
    ui.selectable_value(curve, Curve::Stepped { threshold: 0.5 }, "Stepped 50%");
}

fn smoothing_label(s: &Smoothing) -> String {
    match s {
        Smoothing::Off => "off".into(),
        Smoothing::Exponential { alpha } if *alpha >= 0.25 => "light".into(),
        Smoothing::Exponential { alpha } if *alpha >= 0.1 => "medium".into(),
        Smoothing::Exponential { .. } => "heavy".into(),
        Smoothing::WeightedAverage { .. } => "windowed average".into(),
    }
}

/// Push the profile store into the engine and mark it for saving.
fn push_store(ctx: &mut super::Ctx) {
    crate::pages::dashboard::push_store(ctx);
}