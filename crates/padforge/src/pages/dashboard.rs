//! The dashboard: at-a-glance health, the live pad, and the primary controls.
use crate::state::{Page, ToastLevel};
use crate::theme::*;
use crate::widgets::{self, PadPreview};
use egui::epaint::MarginF32;
use egui::{Align2, FontId, RichText, Stroke};
use padcore::engine::EngineCommand;

#[allow(clippy::too_many_lines)]
pub fn draw(ui: &mut egui::Ui, ctx: &mut super::Ctx) {
    let telemetry = ctx.engine.telemetry();
    ui.add_space(SPACE_SM);
    // --- status strip ---------------------------------------------------
    status_strip(ui, &telemetry);
    ui.add_space(SPACE_MD);
    // Two columns: the pad preview on the left, the controls on the right.
    ui.columns(2, |cols| {
        let left = &mut cols[0];
        card().show(left, |ui| {
            ui.label(RichText::new("Live input").color(TEXT).size(15.0).strong());
            ui.add_space(SPACE_SM);
            let mut preview = PadPreview {
                report: telemetry.report.clone(),
                output_buttons: padcore::output::XButtons(telemetry.output.buttons),
                lightbar: colour_of(telemetry.lightbar),
                dimmed: !telemetry.connected || telemetry.paused,
            };
            preview.show(ui);
            ui.add_space(SPACE_SM);
            active_controls(ui, &telemetry.active_controls);
        });
        let right = &mut cols[1];
        card().show(right, |ui| {
            ui.label(
                RichText::new("Quick controls")
                    .color(TEXT)
                    .size(15.0)
                    .strong(),
            );
            ui.add_space(SPACE_SM);
            ui.horizontal(|ui| {
                if telemetry.paused {
                    if primary_button(ui, ">  Resume").clicked() {
                        ctx.engine.send(EngineCommand::TogglePause);
                        ctx.state.toast("Output resumed", ToastLevel::Success);
                    }
                } else if primary_button(ui, "||||  Pause").clicked() {
                    ctx.engine.send(EngineCommand::TogglePause);
                    ctx.state.toast("Output paused", ToastLevel::Warning);
                }
                if secondary_button(ui, "~  Recalibrate").clicked() {
                    ctx.engine.send(EngineCommand::Recalibrate);
                    ctx.state
                        .toast("Hold the sticks still...", ToastLevel::Info);
                }
            });
            ui.add_space(SPACE_MD);
            ui.label(
                RichText::new("ACTIVE PROFILE")
                    .color(TEXT_FAINT)
                    .size(11.0)
                    .strong(),
            );
            ui.add_space(SPACE_XS);
            // Profile picker, as a row of chips.
            //
            // The chips are collected up front: the click handler needs
            // `&mut ctx`, which cannot be borrowed while iterating the profile
            // list out of it.
            let chips: Vec<(usize, String, bool)> = {
                let store = &ctx.state.store;
                store
                    .profiles
                    .iter()
                    .enumerate()
                    .map(|(i, p)| (i, p.name.clone(), i == store.active))
                    .collect()
            };
            ui.horizontal_wrapped(|ui| {
                for (index, name, selected) in &chips {
                    let text = RichText::new(name)
                        .color(if *selected { BACKDROP } else { TEXT })
                        .size(13.0);
                    let button = egui::Button::new(text)
                        .fill(if *selected { ACCENT } else { SURFACE_RAISED })
                        .stroke(egui::Stroke::new(
                            1.0,
                            if *selected { ACCENT } else { BORDER },
                        ))
                        .corner_radius(RADIUS_PILL);
                    if ui.add(button).clicked() {
                        let id = ctx.state.store.profiles[*index].id.clone();
                        ctx.engine.send(EngineCommand::SelectProfile(id.clone()));
                        ctx.state.store.select(&id);
                        ctx.state.editing = ctx.state.store.active;
                        push_store(ctx);
                    }
                }
            });
            ui.add_space(SPACE_MD);
            ui.label(RichText::new("AXES").color(TEXT_FAINT).size(11.0).strong());
            ui.add_space(SPACE_XS);
            // Raw input axes, so a dead stick or a noisy one is obvious.
            // Two fixed-width columns rather than a Grid: the bars size themselves to the
            // available width, which a Grid resolves against its own content and
            // ends up giving the two cells different widths.
            let r = &telemetry.report;
            let buttons = r.buttons;
            let col_w = ((ui.available_width() - SPACE_MD) * 0.5).max(80.0);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(col_w);
                    widgets::axis_bar(ui, "Left X", r.left_x, ACCENT);
                    widgets::axis_bar(ui, "Left Y", r.left_y, ACCENT);
                    widgets::trigger_bar(ui, "L2", r.l2, buttons.any(padcore::report::Buttons::L2));
                });
                ui.vertical(|ui| {
                    ui.set_width(col_w);
                    widgets::axis_bar(ui, "Right X", r.right_x, ACCENT);
                    widgets::axis_bar(ui, "Right Y", r.right_y, ACCENT);
                    widgets::trigger_bar(ui, "R2", r.r2, buttons.any(padcore::report::Buttons::R2));
                });
            });
            ui.add_space(SPACE_SM);
            divider(ui);
            // Link through to the deeper pages.
            ui.horizontal(|ui| {
                if secondary_button(ui, "Controller settings").clicked() {
                    ctx.state.page = Page::Controller;
                }
                if secondary_button(ui, "Mapping").clicked() {
                    ctx.state.page = Page::Mapping;
                }
            });
        });
    });
}
/// Connection state, profile match, and the numbers that matter.
fn status_strip(ui: &mut egui::Ui, t: &padcore::engine::Telemetry) {
    card().show(ui, |ui| {
        ui.horizontal(|ui| {
            // Connection dot plus label.
            let (dot_colour, label) = if !t.connected {
                (DANGER, "No pad")
            } else if t.paused {
                (WARNING, "Paused")
            } else if t.output_connected {
                (SUCCESS, "Live")
            } else {
                (WARNING, "No output")
            };
            let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter()
                .circle_filled(rect.center(), 5.0, dot_colour);
            ui.label(RichText::new(label).color(dot_colour).size(14.0).strong());
            if t.connected {
                ui.label(
                    RichText::new(&t.device_label)
                        .color(TEXT_MUTED)
                        .size(13.0),
                );
                if let Some(transport) = t.transport {
                    ui.label(
                        RichText::new(transport.label())
                            .color(TEXT_FAINT)
                            .size(11.5),
                    );
                }
                widgets::battery_pill(ui, t.battery.level, t.battery.charging);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Latency readout, averaged over the frame history.
                let avg = if t.frame_ms > 0.0 {
                    format!("{:.1} ms", t.frame_ms)
                } else {
                    "-".to_string()
                };
                ui.label(
                    RichText::new(avg)
                        .color(TEXT_MUTED)
                        .size(12.5)
                        .monospace(),
                );
                if let Some(matched) = &t.auto_profile_matched {
                    pill(ui, &format!("auto: {matched}"), ACCENT);
                }
            });
        });
        // Surface a missing virtual driver prominently: it is the one problem
        // that silently makes the whole app look like it is not working.
        if !t.output_connected {
            ui.add_space(SPACE_SM);
            let reason = t
                .vigem_error
                .as_deref()
                .unwrap_or("the virtual gamepad driver is not responding");
            banner(
                ui,
                DANGER,
                "Games will not see your controller",
                &format!("{reason}. Install the ViGEmBus driver, or switch to Monitor-only mode on the Output page."),
            );
        }
    });
}
/// The controls currently held, as a row of small chips.
fn active_controls(ui: &mut egui::Ui, active: &[&str]) {
    if active.is_empty() {
        ui.label(
            RichText::new("Nothing pressed")
                .color(TEXT_FAINT)
                .size(12.0),
        );
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for name in active {
            pill(ui, name, ACCENT);
        }
    });
}
/// A coloured callout for a persistent problem.
pub fn banner(ui: &mut egui::Ui, colour: egui::Color32, title: &str, body: &str) {
    egui::Frame::new()
        .fill(colour.gamma_multiply(0.12))
        .stroke(Stroke::new(1.0, colour.gamma_multiply(0.4)))
        .corner_radius(RADIUS_SM)
        .inner_margin(MarginF32::same(SPACE_MD))
        .show(ui, |ui| {
            ui.label(RichText::new(title).color(colour).strong());
            ui.label(RichText::new(body).color(TEXT_MUTED).size(12.5));
        });
}
/// Bytes to a colour, used for the lightbar preview.
fn colour_of(rgb: [u8; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}
/// Push the profile store into the engine after a UI-side change.
pub fn push_store(ctx: &mut super::Ctx) {
    ctx.state.profiles_dirty = true;
    let store = ctx.state.store.clone();
    ctx.engine
        .send(EngineCommand::ApplyProfiles(Box::new(store)));
}
/// A centred logo lockup, shown on the dashboard header.
pub fn logo(ui: &mut egui::Ui, accent: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    // Two interlocking rings: a pad and a bridge.
    painter.circle_stroke(rect.center(), 11.0, Stroke::new(2.0, accent));
    painter.circle_stroke(
        rect.center(),
        5.0,
        Stroke::new(2.0, accent.gamma_multiply(0.6)),
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        "",
        FontId::proportional(1.0),
        accent,
    );
}
