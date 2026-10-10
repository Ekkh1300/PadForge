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
    // Two cards of one height, rather than two columns each stopping wherever
    // its own content happened to end: that ragged bottom edge is most of what
    // made the row look untidy.
    ui.columns(2, |cols| {
        live_input(&mut cols[0], &telemetry);
        // The preview makes the left card the taller of the two, so measure it
        // and ask the right card to reach the same height: one pass is enough
        // to have the row finish level.
        let height = cols[0].min_rect().height();
        controls(&mut cols[1], ctx, &telemetry, height);
    });
    // Navigation before the raw axes. The tiles are where a reader who wants
    // more goes; the axes are a readout, so the readout is the part that may
    // fall below the fold.
    ui.add_space(SPACE_MD);
    navigate(ui, ctx);

    ui.add_space(SPACE_MD);
    axes(ui, &telemetry);
}
/// The live pad, and whatever it is currently sending.
///
/// No fixed height: the right-hand card is asked to match whatever this one
/// comes out at, which is the only direction the two can be reconciled in.
fn live_input(ui: &mut egui::Ui, t: &padcore::engine::Telemetry) {
    card().show(ui, |ui| {
        heading(ui, "Live input", "what the pad is sending right now");
        ui.add_space(SPACE_SM);
        let mut preview = PadPreview {
            report: t.report.clone(),
            output_buttons: padcore::output::XButtons(t.output.buttons),
            lightbar: colour_of(t.lightbar),
            dimmed: !t.connected || t.paused,
        };
        preview.show(ui);
        ui.add_space(SPACE_SM);
        divider(ui);
        caption(ui, "HOLDING");
        ui.add_space(SPACE_XS);
        active_controls(ui, &t.active_controls);
    });
}

/// Everything on the dashboard that can be pressed, and the two readouts that
/// only mean something while the app is running: the motors and the profile.
///
/// `peer_height` is the outer height of the left card; the card's own padding
/// is subtracted so the two frames, not their contents, are what line up.
fn controls(
    ui: &mut egui::Ui,
    ctx: &mut super::Ctx,
    t: &padcore::engine::Telemetry,
    peer_height: f32,
) {
    card().show(ui, |ui| {
        ui.set_min_height((peer_height - 2.0 * SPACE_MD).max(0.0));
        heading(
            ui,
            "Control",
            "pause the output, test the motors, pick a profile",
        );
        ui.add_space(SPACE_SM);
        ui.horizontal(|ui| {
            if t.paused {
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
            // The vibration test lives here rather than only on the Output
            // page: it is the one control whose result is physical, and the
            // two bars underneath are what shows whether it went anywhere.
            if secondary_button(ui, "Test vibration").clicked() {
                ctx.engine.send(EngineCommand::TestRumble);
                ctx.state.toast("Buzzing for a moment...", ToastLevel::Info);
            }
        });

        ui.add_space(SPACE_MD);
        divider(ui);
        caption(ui, "ACTIVE PROFILE");
        ui.add_space(SPACE_XS);
        profile_chips(ctx, ui);

        ui.add_space(SPACE_MD);
        divider(ui);
        caption(ui, "VIBRATION");
        ui.add_space(SPACE_XS);
        widgets::trigger_bar(ui, "Low motor", t.rumble.0 as f32 / 255.0, t.rumble.0 > 0);
        widgets::trigger_bar(ui, "High motor", t.rumble.1 as f32 / 255.0, t.rumble.1 > 0);
        ui.label(
            RichText::new(if t.rumble_test {
                "Testing: both motors, for a moment."
            } else {
                "Driven by the game, or by the test button."
            })
            .color(TEXT_FAINT)
            .size(11.0),
        );
    });
}

/// Raw input, so a dead stick or a noisy one is obvious.
///
/// Three groups with their own headings rather than six bars in two columns:
/// sticks, sticks, triggers, which is what the bars are, instead of a list the
/// reader has to map back onto the pad.
fn axes(ui: &mut egui::Ui, t: &padcore::engine::Telemetry) {
    card().show(ui, |ui| {
        caption(ui, "AXES");
        ui.add_space(SPACE_XS);
        let r = &t.report;
        ui.columns(3, |cols| {
            group(&mut cols[0], "Left stick", |ui| {
                widgets::axis_bar(ui, "Left X", r.left_x, ACCENT);
                widgets::axis_bar(ui, "Left Y", r.left_y, ACCENT);
            });
            group(&mut cols[1], "Right stick", |ui| {
                widgets::axis_bar(ui, "Right X", r.right_x, ACCENT);
                widgets::axis_bar(ui, "Right Y", r.right_y, ACCENT);
            });
            group(&mut cols[2], "Triggers", |ui| {
                widgets::trigger_bar(ui, "L2", r.l2, r.buttons.any(padcore::report::Buttons::L2));
                widgets::trigger_bar(ui, "R2", r.r2, r.buttons.any(padcore::report::Buttons::R2));
            });
        });
    });
}

/// One card per page worth of depth, each saying what is inside it. The old
/// pair of plain buttons did not, which is why they read as decoration.
fn navigate(ui: &mut egui::Ui, ctx: &mut super::Ctx) {
    ui.columns(4, |cols| {
        // One statement per column: an array holding all four `&mut` at once
        // is a single borrow as far as the checker is concerned.
        tile(
            &mut cols[0],
            ctx,
            Page::Controller,
            "Controller",
            "Sticks, deadzones, curves",
        );
        tile(
            &mut cols[1],
            ctx,
            Page::Mapping,
            "Mapping",
            "Buttons, axes, formulas",
        );
        tile(
            &mut cols[2],
            ctx,
            Page::Output,
            "Output",
            "Virtual pad, lightbar, vibration",
        );
        tile(
            &mut cols[3],
            ctx,
            Page::Profiles,
            "Profiles",
            "Per-game and auto-match",
        );
    });
}

/// A card that acts as one big button.
fn tile(ui: &mut egui::Ui, ctx: &mut super::Ctx, page: Page, name: &str, sub: &str) {
    let inner = card().show(ui, |ui| {
        ui.set_min_height(28.0);
        ui.label(RichText::new(name).color(TEXT).size(14.0).strong());
        ui.label(RichText::new(sub).color(TEXT_FAINT).size(11.0));
    });
    let hit = ui.interact(
        inner.response.rect,
        inner.response.id.with("tile"),
        egui::Sense::click(),
    );
    if hit.hovered() {
        ui.painter().rect_filled(
            inner.response.rect,
            RADIUS_MD,
            egui::Color32::from_white_alpha(8),
        );
    }
    if hit.clicked() {
        ctx.state.page = page;
    }
}

/// A titled column of bars.
fn group(ui: &mut egui::Ui, title: &str, bars: impl FnOnce(&mut egui::Ui)) {
    ui.label(RichText::new(title).color(TEXT_MUTED).size(12.0).strong());
    ui.add_space(SPACE_XS);
    bars(ui);
}

/// Profile picker, as a row of chips.
///
/// Separate from [`controls`] because the click handler needs `&mut ctx`, which
/// cannot be borrowed while iterating the profile list out of it.
fn profile_chips(ctx: &mut super::Ctx, ui: &mut egui::Ui) {
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
}

/// Card title, with a line saying what the card is for.
fn heading(ui: &mut egui::Ui, title: &str, sub: &str) {
    ui.label(RichText::new(title).color(TEXT).size(15.0).strong());
    ui.label(RichText::new(sub).color(TEXT_FAINT).size(11.5));
}

/// The small caption that names a block inside a card.
fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).color(TEXT_FAINT).size(11.0).strong());
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
                widgets::battery_pill(ui, &t.battery);
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
        // The other silent failure: reports reach the pad's driver but the pad
        // turns them down, so the lightbar and the rumble quietly stop while
        // everything else looks like it is working.
        if let Some(reason) = &t.output_error {
            ui.add_space(SPACE_SM);
            banner(
                ui,
                WARNING,
                "The controller refused an output report",
                reason,
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
