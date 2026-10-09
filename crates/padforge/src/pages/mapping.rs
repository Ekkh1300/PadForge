//! Mapping page: reassign every DS4 control to any XInput target.
use crate::theme::*;
use egui::{RichText, Stroke};
use padcore::engine::EngineCommand;
use padcore::mapping::{Ds4Control, X360Control};

#[allow(clippy::too_many_lines)]
pub fn draw(ui: &mut egui::Ui, ctx: &mut super::Ctx) {
    let telemetry = ctx.engine.telemetry();
    ui.add_space(SPACE_SM);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(
                "Left click an input to assign it. The default layout follows PlayStation conventions: \
                 Cross is A, Circle is B, and the touchpad is Guide.",
            )
            .color(TEXT_MUTED)
            .size(12.5),
        );
    });
    ui.add_space(SPACE_MD);
    let groups: [(&str, &[Ds4Control]); 5] = [
        ("Face buttons", Ds4Control::FACE),
        ("Shoulders & triggers", Ds4Control::SHOULDERS),
        ("Sticks", Ds4Control::STICKS),
        ("D-pad", Ds4Control::DPAD),
        ("System", Ds4Control::SYSTEM),
    ];
    ui.columns(2, |cols| {
        for (i, (title, controls)) in groups.iter().enumerate() {
            let col = if i < 3 { &mut cols[0] } else { &mut cols[1] };
            group_card(col, title, controls, ctx, &telemetry);
            col.add_space(SPACE_MD);
        }
    });
    // The touchpad as a whole is handled separately from its click button.
    ui.columns(2, |cols| {
        touchpad_card(&mut cols[0], ctx);
        presets_card(&mut cols[1], ctx);
    });
}
fn group_card(
    ui: &mut egui::Ui,
    title: &str,
    controls: &[Ds4Control],
    ctx: &mut super::Ctx,
    t: &padcore::engine::Telemetry,
) {
    card().show(ui, |ui| {
        ui.label(RichText::new(title).color(TEXT).size(14.0).strong());
        ui.add_space(SPACE_SM);
        for control in controls {
            mapping_row(ui, ctx, *control, t);
            if Some(*control) == controls.last().copied() {
                break;
            }
            ui.add_space(SPACE_XS);
        }
    });
}
/// One input on the left, one target on the right.
fn mapping_row(
    ui: &mut egui::Ui,
    ctx: &mut super::Ctx,
    control: Ds4Control,
    t: &padcore::engine::Telemetry,
) {
    let mapping = ctx.state.editing_profile().mapping_for(control);
    let held = control_is_held(control, t);
    ui.horizontal(|ui| {
        // Source, highlighted while held.
        let source = egui::Button::new(
            RichText::new(control.label())
                .color(if held { BACKDROP } else { TEXT })
                .size(12.5),
        )
        .fill(if held { ACCENT } else { SURFACE })
        .stroke(Stroke::new(1.0, if held { ACCENT } else { BORDER }))
        .corner_radius(RADIUS_SM)
        .min_size(egui::vec2(140.0, 26.0));
        if ui
            .add(source)
            .on_hover_text("Click to pick a new target")
            .clicked()
        {
            ctx.state.toast(
                format!("{} -> {}", control.label(), mapping.target.label()),
                crate::state::ToastLevel::Info,
            );
        }
        ui.label(RichText::new("->").color(TEXT_FAINT).size(12.0));
        // Target selector.
        let mut selected = mapping.target;
        egui::ComboBox::from_id_salt(("target", control))
            .selected_text(
                RichText::new(selected.label()).color(if mapping.is_active() {
                    ACCENT
                } else {
                    TEXT_FAINT
                }),
            )
            .width(160.0)
            .show_ui(ui, |ui| {
                for target in target_options() {
                    // "Unmapped" is dimmed in the closed state, which the label
                    // alone already conveys once selected.
                    ui.selectable_value(&mut selected, target, target.label());
                }
            });
        // Push the change straight into the profile and the engine.
        if selected != mapping.target {
            ctx.state
                .editing_profile_mut()
                .set_mapping(control, mapping_with_target(mapping, selected));
            let store = ctx.state.store.clone();
            ctx.engine
                .send(EngineCommand::ApplyProfiles(Box::new(store)));
        }
        // Modifiers, shown only when the mapping is live.
        if mapping.is_active() {
            let has_mod = mapping.mod_ds4.is_some() || mapping.mod_target.is_some();
            if has_mod {
                pill(ui, "mod", WARNING);
            }
        }
    });
    // Modifier row, indented under the mapping it belongs to.
    if mapping.is_active() && (mapping.mod_ds4.is_some() || mapping.mod_target.is_some()) {
        ui.horizontal(|ui| {
            ui.add_space(140.0);
            ui.label(RichText::new("hold").color(TEXT_FAINT).size(11.0));
            let mut mod_ds4 = mapping.mod_ds4;
            egui::ComboBox::from_id_salt(("mod_ds4", control))
                .selected_text(
                    mapping
                        .mod_ds4
                        .map(|c| c.label())
                        .unwrap_or("nothing")
                        .to_string(),
                )
                .width(150.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut mod_ds4, None, "nothing");
                    for c in Ds4Control::ALL {
                        if c.is_axis_direction() {
                            continue;
                        }
                        ui.selectable_value(&mut mod_ds4, Some(*c), c.label());
                    }
                });
            if mod_ds4 != mapping.mod_ds4 {
                let mut next = mapping;
                next.mod_ds4 = mod_ds4;
                ctx.state.editing_profile_mut().set_mapping(control, next);
                let store = ctx.state.store.clone();
                ctx.engine
                    .send(EngineCommand::ApplyProfiles(Box::new(store)));
            }
            if secondary_button(ui, "clear").clicked() {
                let mut next = mapping;
                next.mod_ds4 = None;
                next.mod_target = None;
                ctx.state.editing_profile_mut().set_mapping(control, next);
                let store = ctx.state.store.clone();
                ctx.engine
                    .send(EngineCommand::ApplyProfiles(Box::new(store)));
            }
        });
    }
}
/// All assignable targets, in a deliberate order.
fn target_options() -> Vec<X360Control> {
    vec![
        X360Control::None,
        X360Control::A,
        X360Control::B,
        X360Control::X,
        X360Control::Y,
        X360Control::LeftBumper,
        X360Control::RightBumper,
        X360Control::LeftTriggerClick,
        X360Control::RightTriggerClick,
        X360Control::Back,
        X360Control::Start,
        X360Control::Guide,
        X360Control::LeftThumb,
        X360Control::RightThumb,
        X360Control::DpadUp,
        X360Control::DpadDown,
        X360Control::DpadLeft,
        X360Control::DpadRight,
        X360Control::LeftThumbUp,
        X360Control::LeftThumbDown,
        X360Control::LeftThumbLeft,
        X360Control::LeftThumbRight,
        X360Control::RightThumbUp,
        X360Control::RightThumbDown,
        X360Control::RightThumbLeft,
        X360Control::RightThumbRight,
    ]
}
fn mapping_with_target(
    mut mapping: padcore::mapping::Mapping,
    target: X360Control,
) -> padcore::mapping::Mapping {
    mapping.target = target;
    if target == X360Control::None {
        mapping.mod_ds4 = None;
        mapping.mod_target = None;
    }
    mapping
}
/// Is this control currently held, per live telemetry?
fn control_is_held(control: Ds4Control, t: &padcore::engine::Telemetry) -> bool {
    use padcore::report::Buttons as B;
    let r = &t.report;
    match control {
        Ds4Control::TouchpadClick => r.touch.pad_clicked,
        Ds4Control::TouchpadGesture => r.touch.pad_touched,
        Ds4Control::DpadAny => r.buttons.any(B::UP | B::DOWN | B::LEFT | B::RIGHT),
        other if other.is_axis_direction() => {
            let (value, positive) = match other {
                Ds4Control::LeftXNegative => (r.left_x, false),
                Ds4Control::LeftXPositive => (r.left_x, true),
                Ds4Control::LeftYNegative => (r.left_y, false),
                Ds4Control::LeftYPositive => (r.left_y, true),
                Ds4Control::RightXNegative => (r.right_x, false),
                Ds4Control::RightXPositive => (r.right_x, true),
                Ds4Control::RightYNegative => (r.right_y, false),
                _ => (r.right_y, true),
            };
            // Match the engine's threshold, expressed as a fraction.
            let threshold = padcore::engine::poll_threshold_fraction();
            if positive {
                value >= threshold
            } else {
                value <= -threshold
            }
        }
        other => r.buttons.contains(other.button_mask()),
    }
}
/// The touchpad mode, which routes touch rather than a button press.
fn touchpad_card(ui: &mut egui::Ui, ctx: &mut super::Ctx) {
    use padcore::touchpad::TouchpadMode;
    card().show(ui, |ui| {
        ui.label(
            RichText::new("Touchpad behaviour")
                .color(TEXT)
                .size(14.0)
                .strong(),
        );
        ui.add_space(SPACE_SM);
        let mode = ctx.state.editing_profile().touchpad.mode;
        egui::ComboBox::from_id_salt("touch_mode")
            .selected_text(match mode {
                TouchpadMode::Off => "Off",
                TouchpadMode::Mouse => "Pointer (mouse)",
                TouchpadMode::Dpad => "D-pad",
                TouchpadMode::Gestures => "Swipe gestures",
            })
            .width(200.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut ctx.state.editing_profile_mut().touchpad.mode,
                    TouchpadMode::Off,
                    "Off",
                );
                ui.selectable_value(
                    &mut ctx.state.editing_profile_mut().touchpad.mode,
                    TouchpadMode::Mouse,
                    "Pointer (mouse)",
                );
                ui.selectable_value(
                    &mut ctx.state.editing_profile_mut().touchpad.mode,
                    TouchpadMode::Dpad,
                    "D-pad",
                );
                ui.selectable_value(
                    &mut ctx.state.editing_profile_mut().touchpad.mode,
                    TouchpadMode::Gestures,
                    "Swipe gestures",
                );
            });
        if mode != TouchpadMode::Off {
            let mut sensitivity = ctx.state.editing_profile().touchpad.sensitivity;
            if ui
                .add(
                    egui::Slider::new(&mut sensitivity, 0.2..=4.0)
                        .text("sensitivity")
                        .suffix("x")
                        .show_value(true),
                )
                .changed()
            {
                ctx.state.editing_profile_mut().touchpad.sensitivity = sensitivity;
                push_store(ctx);
            }
        }
    });
}
/// Quick ways to get back to a known-good layout.
fn presets_card(ui: &mut egui::Ui, ctx: &mut super::Ctx) {
    card().show(ui, |ui| {
        ui.label(RichText::new("Presets").color(TEXT).size(14.0).strong());
        ui.add_space(SPACE_SM);
        ui.label(
            RichText::new("Overwrite every button in this profile with a known layout.")
                .color(TEXT_MUTED)
                .size(12.0),
        );
        ui.add_space(SPACE_SM);
        ui.horizontal(|ui| {
            if secondary_button(ui, "PlayStation layout").clicked() {
                apply_preset(ctx, preset_playstation());
                ctx.state.toast(
                    "Applied the PlayStation layout",
                    crate::state::ToastLevel::Success,
                );
            }
            if secondary_button(ui, "Xbox layout").clicked() {
                apply_preset(ctx, preset_xbox());
                ctx.state
                    .toast("Applied the Xbox layout", crate::state::ToastLevel::Success);
            }
        });
    });
}
type Preset = Vec<(Ds4Control, X360Control)>;
fn preset_playstation() -> Preset {
    use Ds4Control as D;
    use X360Control as X;
    vec![
        (D::Cross, X::A),
        (D::Circle, X::B),
        (D::Square, X::X),
        (D::Triangle, X::Y),
        (D::L1, X::LeftBumper),
        (D::R1, X::RightBumper),
        (D::L2, X::LeftTriggerClick),
        (D::R2, X::RightTriggerClick),
        (D::Share, X::Back),
        (D::Options, X::Start),
        (D::L3, X::LeftThumb),
        (D::R3, X::RightThumb),
        (D::TouchpadClick, X::Guide),
    ]
}
fn preset_xbox() -> Preset {
    use Ds4Control as D;
    use X360Control as X;
    vec![
        // Physical-position matching: the bottom button is A on an Xbox pad.
        (D::Cross, X::Y),
        (D::Circle, X::B),
        (D::Square, X::A),
        (D::Triangle, X::X),
        (D::L1, X::LeftBumper),
        (D::R1, X::RightBumper),
        (D::L2, X::LeftTriggerClick),
        (D::R2, X::RightTriggerClick),
        (D::Share, X::Back),
        (D::Options, X::Start),
        (D::L3, X::LeftThumb),
        (D::R3, X::RightThumb),
        (D::TouchpadClick, X::Guide),
    ]
}
/// Every preset must bind the same set of controls as the default profile,
/// otherwise applying one silently unbinds whatever it forgot.
fn assert_preset_is_complete(preset: &Preset) {
    let covered: std::collections::HashSet<Ds4Control> = preset.iter().map(|(c, _)| *c).collect();
    for control in Ds4Control::ALL {
        let expected = matches!(control, Ds4Control::DpadAny | Ds4Control::TouchpadGesture);
        assert_eq!(
            covered.contains(control),
            !expected,
            "preset is missing {control:?}"
        );
    }
    // And no control may be bound twice.
    assert_eq!(
        preset.len(),
        covered.len(),
        "preset maps the same control more than once"
    );
}

fn apply_preset(ctx: &mut super::Ctx, preset: Preset) {
    assert_preset_is_complete(&preset);
    {
        let profile = ctx.state.editing_profile_mut();
        for (control, target) in preset {
            profile.set_mapping(control, padcore::mapping::Mapping::new(target));
        }
    }
    push_store(ctx);
}
fn push_store(ctx: &mut super::Ctx) {
    let store = ctx.state.store.clone();
    ctx.engine
        .send(EngineCommand::ApplyProfiles(Box::new(store)));
}
