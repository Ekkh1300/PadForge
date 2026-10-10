//! Output page: what the virtual pad is doing, and where the input is going.
use crate::state::ToastLevel;
use crate::theme::*;
use crate::widgets;
use egui::{RichText, Stroke};
use padcore::engine::EngineCommand;
use padcore::settings::{OutputMode, SlotChoice};

#[allow(clippy::too_many_lines)]
pub fn draw(ui: &mut egui::Ui, ctx: &mut super::Ctx) {
    let telemetry = ctx.engine.telemetry();
    ui.add_space(SPACE_SM);
    // Explicit half-widths in a horizontal layout rather than `ui.columns`:
    // columns divide the space but do not shrink a child that asks for more, so
    // a wide card would spill past the window edge.
    let width = ((ui.available_width() - SPACE_MD) * 0.5).floor();
    let mut done = 0;
    ui.horizontal_top(|ui| {
        for side in 0..2 {
            if side == 1 {
                ui.add_space(SPACE_MD);
            }
            ui.vertical(|ui| {
                ui.set_width(width);
                if side == 0 {
                    backend_card(ui, ctx, &telemetry);
                } else {
                    monitor_card(ui, ctx, &telemetry);
                }
                done = 1;
            });
        }
    });
    debug_assert_eq!(done, 1, "both columns should have been drawn");
}
/// Driver status, output mode, and pointer injection.
fn backend_card(ui: &mut egui::Ui, ctx: &mut super::Ctx, t: &padcore::engine::Telemetry) {
    card().show(ui, |ui| {
        ui.label(
            RichText::new("Virtual gamepad")
                .color(TEXT)
                .size(15.0)
                .strong(),
        );
        ui.add_space(SPACE_SM);
        // Status line.
        ui.horizontal(|ui| {
            let (colour, label) = if t.output_connected {
                (SUCCESS, "Connected")
            } else {
                (DANGER, "Not available")
            };
            let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter().circle_filled(rect.center(), 5.0, colour);
            ui.label(RichText::new(label).color(colour).strong());
            ui.label(
                RichText::new(&t.output_backend)
                    .color(TEXT_MUTED)
                    .size(12.0),
            );
        });
        if !t.output_connected {
            ui.add_space(SPACE_MD);
            let reason = t
                .vigem_error
                .as_deref()
                .unwrap_or("the driver did not respond");
            ui.label(
                RichText::new("Games will not see your controller until this is fixed.")
                    .color(DANGER)
                    .size(12.5),
            );
            ui.label(
                RichText::new(
                    "PadForge publishes a virtual Xbox 360 pad through the ViGEmBus driver. \
                     Install that driver, then restart PadForge.",
                )
                .color(TEXT_MUTED)
                .size(12.0),
            );
            ui.label(
                RichText::new(format!("Driver reported: {reason}"))
                    .color(TEXT_FAINT)
                    .size(11.0)
                    .monospace(),
            );
        }
        ui.add_space(SPACE_MD);
        divider(ui);
        ui.add_space(SPACE_MD);
        ui.label(
            RichText::new("FORCE FEEDBACK")
                .color(TEXT_FAINT)
                .size(11.0)
                .strong(),
        );
        ui.add_space(SPACE_XS);
        // Vibration is the one thing the UI cannot show, and for a long time it
        // was also the one thing nothing in the app proved. Driving it by hand
        // uses the same route a game does, so a rumble that is wired but wrong
        // fails here rather than in a match nobody can pause.
        if secondary_button(ui, "Test vibration").clicked() {
            ctx.engine.send(EngineCommand::TestRumble);
        }
        ui.label(
            RichText::new(
                "Vibrates the controller for a moment through the path a game uses, \
                 so what you feel is what a game would feel.",
            )
            .color(TEXT_FAINT)
            .size(11.5),
        );

        ui.add_space(SPACE_MD);
        divider(ui);
        ui.add_space(SPACE_MD);
        // A virtual pad is anonymous without this: two pads on one machine look
        // identical in a game's controller list, and a split-screen game asks
        // for a player rather than a device. The slot is shown from what the
        // driver reported, not from what was asked for, because a request that
        // lost a race is answered by landing somewhere else.
        ui.label(
            RichText::new("VIRTUAL SLOT")
                .color(TEXT_FAINT)
                .size(11.0)
                .strong(),
        );
        ui.add_space(SPACE_XS);
        // The landed slot and the requested one are shown separately on
        // purpose. A request can be refused — a player already taken by another
        // controller leaves this pad where it was — and reporting only where it
        // ended up would read as though the choice had been accepted.
        let asked = ctx.state.settings.virtual_slot.player_no();
        let landed = t.output_slot.map(|s| s as usize + 1);
        let refused = matches!((asked, landed), (Some(a), Some(l)) if a != l);
        let (line, colour) = match (landed, refused) {
            (Some(l), false) => (format!("This pad is Player {l} right now."), TEXT),
            (Some(l), true) => (
                match asked {
                    Some(a) => format!("Player {a} is taken, so this pad became Player {l}."),
                    None => format!("This pad is Player {l} right now."),
                },
                DANGER,
            ),
            (None, _) => (
                "The driver has not reported which player this pad is.".to_string(),
                TEXT_FAINT,
            ),
        };
        ui.label(RichText::new(line).color(colour).size(12.0));
        ui.add_space(SPACE_XS);
        let mut slot = ctx.state.settings.virtual_slot;
        ui.horizontal(|ui| {
            for choice in SlotChoice::ALL {
                let selected = slot == choice;
                let btn = egui::Button::new(
                    RichText::new(choice.label())
                        .color(if selected { BACKDROP } else { TEXT })
                        .size(12.5),
                )
                .fill(if selected { ACCENT } else { SURFACE_RAISED })
                .stroke(Stroke::new(1.0, if selected { ACCENT } else { BORDER }))
                .corner_radius(RADIUS_SM);
                if ui.add(btn).clicked() {
                    slot = choice;
                }
            }
        });
        if slot != ctx.state.settings.virtual_slot {
            ctx.state.settings.virtual_slot = slot;
            let settings = ctx.state.settings.clone();
            ctx.state.settings_dirty = true;
            ctx.engine
                .send(EngineCommand::ApplySettings(Box::new(settings)));
            ctx.state.toast(
                match slot {
                    // Not the slot, which may well be refused; the request is
                    // what is being confirmed here.
                    SlotChoice::Auto => "Slot: automatic".into(),
                    SlotChoice::Player(i) => format!("Asking for Player {}", i + 1),
                },
                ToastLevel::Info,
            );
        }
        ui.label(
            RichText::new(
                "Games see the pad as this player. If the slot is already taken the \
                 pad appears as the next free one, and the line above says where it \
                 ended up.",
            )
            .color(TEXT_FAINT)
            .size(11.5),
        );

        ui.add_space(SPACE_MD);
        divider(ui);
        ui.add_space(SPACE_MD);
        ui.label(RichText::new("MODE").color(TEXT_FAINT).size(11.0).strong());
        ui.add_space(SPACE_XS);
        let current = ctx.state.settings.output_mode;
        // Stacked, not side by side: two cards plus their hint lines do not fit
        // half a window without the text colliding.
        for (mode, label, hint) in [
            (
                OutputMode::Xbox360,
                "Virtual pad",
                "Appears as an Xbox 360 controller in every game.",
            ),
            (
                OutputMode::MonitorOnly,
                "Monitor only",
                "Watches the input without publishing anything.",
            ),
        ] {
            let selected = current == mode;
            ui.horizontal(|ui| {
                let btn = egui::Button::new(
                    RichText::new(label)
                        .color(if selected { BACKDROP } else { TEXT })
                        .size(13.0),
                )
                .fill(if selected { ACCENT } else { SURFACE_RAISED })
                .stroke(Stroke::new(1.0, if selected { ACCENT } else { BORDER }))
                .corner_radius(RADIUS_SM)
                .min_size(egui::vec2(120.0, 30.0));
                if ui.add(btn).clicked() && !selected {
                    ctx.state.settings.output_mode = mode;
                    let settings = ctx.state.settings.clone();
                    ctx.state.settings_dirty = true;
                    ctx.engine
                        .send(EngineCommand::ApplySettings(Box::new(settings)));
                    ctx.state.toast(
                        if mode == OutputMode::Xbox360 {
                            "Publishing to a virtual pad"
                        } else {
                            "Monitor-only mode"
                        },
                        ToastLevel::Info,
                    );
                }
                ui.add_space(SPACE_XS);
                ui.label(RichText::new(hint).color(TEXT_FAINT).size(11.0));
            });
            ui.add_space(SPACE_XS);
        }
        ui.add_space(SPACE_MD);
        divider(ui);
        ui.add_space(SPACE_MD);
        ui.label(
            RichText::new("POLLING")
                .color(TEXT_FAINT)
                .size(11.0)
                .strong(),
        );
        ui.add_space(SPACE_XS);
        // A wrapping Label, not `ui.label`: the hint is longer than a
        // half-width column and would otherwise be clipped.
        ui.add(
            egui::Label::new(
                RichText::new(
                    "Higher rates lower latency and raise CPU use. 250 Hz is a good \
                     balance; 1000 Hz only helps for competitive shooters.",
                )
                .color(TEXT_MUTED)
                .size(12.0),
            )
            .wrap(),
        );
        ui.add_space(SPACE_SM);
        let mut rate = ctx.state.settings.poll_rate_hz as f32;
        if ui
            .add(
                egui::Slider::new(&mut rate, 125.0..=1000.0)
                    .text("poll rate")
                    .suffix(" Hz")
                    .step_by(125.0)
                    .show_value(true),
            )
            .changed()
        {
            ctx.state.settings.poll_rate_hz = rate.round() as u32;
            let settings = ctx.state.settings.clone();
            ctx.state.settings_dirty = true;
            ctx.engine
                .send(EngineCommand::ApplySettings(Box::new(settings)));
        }
        ui.add_space(SPACE_MD);
        divider(ui);
        ui.add_space(SPACE_MD);
        ui.label(
            RichText::new("DEVICE")
                .color(TEXT_FAINT)
                .size(11.0)
                .strong(),
        );
        ui.add_space(SPACE_SM);
        // Device list, refreshed at most once a second.
        refresh_devices(ctx);
        if ctx.state.devices.is_empty() {
            ui.label(
                RichText::new("No DualShock 4 detected. Connect one, or press Rescan.")
                    .color(TEXT_MUTED)
                    .size(12.0),
            );
        } else {
            for device in &ctx.state.devices {
                let selected = ctx.state.settings.device.serial() == device.serial.as_deref();
                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut ctx.state.settings.device,
                        padcore::settings::DeviceSelection::Serial(
                            device.serial.clone().unwrap_or_default(),
                        ),
                        device.label(),
                    );
                    if selected {
                        ui.label(RichText::new("in use").color(ACCENT).size(11.0));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(device.transport.label())
                                .color(TEXT_FAINT)
                                .size(11.0),
                        );
                    });
                });
            }
            ui.radio_value(
                &mut ctx.state.settings.device,
                padcore::settings::DeviceSelection::Auto,
                "Use whichever pad connects first",
            );
        }
        ui.add_space(SPACE_SM);
        ui.horizontal(|ui| {
            if secondary_button(ui, "Rescan").clicked() {
                ctx.state.devices.clear();
                ctx.state.devices_scanned_at = None;
            }
            if secondary_button(ui, "Apply device").clicked() {
                let selection = ctx.state.settings.device.clone();
                ctx.engine.send(EngineCommand::ApplyDevice(selection));
                ctx.state
                    .toast("Device selection applied", ToastLevel::Success);
            }
        });
    });
}
/// The XInput report being published, live.
fn monitor_card(ui: &mut egui::Ui, ctx: &mut super::Ctx, t: &padcore::engine::Telemetry) {
    card().show(ui, |ui| {
        ui.label(
            RichText::new("What games see")
                .color(TEXT)
                .size(15.0)
                .strong(),
        );
        ui.add_space(SPACE_SM);
        if !t.output_connected {
            ui.label(
                RichText::new("No output yet. Fix the driver, or switch to monitor-only.")
                    .color(TEXT_FAINT)
                    .size(12.0),
            );
            return;
        }
        let out = &t.output;
        // Buttons, as a grid of chips that light up.
        ui.label(
            RichText::new("BUTTONS")
                .color(TEXT_FAINT)
                .size(11.0)
                .strong(),
        );
        ui.add_space(SPACE_XS);
        let pressed = out.buttons;
        let button_row: [(u16, &str); 14] = [
            (padcore::output::XButtons::A, "A"),
            (padcore::output::XButtons::B, "B"),
            (padcore::output::XButtons::X, "X"),
            (padcore::output::XButtons::Y, "Y"),
            (padcore::output::XButtons::LB, "LB"),
            (padcore::output::XButtons::RB, "RB"),
            (padcore::output::XButtons::BACK, "Back"),
            (padcore::output::XButtons::START, "Start"),
            (padcore::output::XButtons::L3, "L3"),
            (padcore::output::XButtons::R3, "R3"),
            (padcore::output::XButtons::GUIDE, "Guide"),
            (padcore::output::XButtons::DPAD_UP, "^"),
            (padcore::output::XButtons::DPAD_DOWN, "v"),
            (padcore::output::XButtons::DPAD_LEFT, "<"),
        ];
        // Chips are positioned explicitly rather than laid out by a Grid or a
        // wrapped row: both size to their content, which pushed the row past
        // the card edge. Computing the positions from the card's own width keeps
        // the row inside it at any window size.
        const CHIP_H: f32 = 22.0;
        const GAP: f32 = 5.0;
        let row_w = ui.available_width();
        let per_row = (((row_w + GAP) / (CHIP_H + GAP)).floor() as usize).clamp(4, 7);
        let chip_w = ((row_w - GAP * (per_row as f32 - 1.0)) / per_row as f32).max(24.0);

        for (index, (bit, label)) in button_row.iter().enumerate() {
            let col = index % per_row;
            let row = index / per_row;
            let on = pressed & bit != 0;
            let fill = if on { ACCENT } else { SURFACE };
            let stroke = Stroke::new(1.0, if on { ACCENT } else { BORDER });
            // Position by hand inside the row's full width.
            let x = col as f32 * (chip_w + GAP);
            let y = row as f32 * (CHIP_H + GAP);
            ui.painter().rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(ui.cursor().min.x + x, ui.cursor().min.y + y),
                    egui::vec2(chip_w, CHIP_H),
                ),
                RADIUS_SM,
                fill,
            );
            ui.painter().rect_stroke(
                egui::Rect::from_min_size(
                    egui::pos2(ui.cursor().min.x + x, ui.cursor().min.y + y),
                    egui::vec2(chip_w, CHIP_H),
                ),
                RADIUS_SM,
                stroke,
                egui::StrokeKind::Inside,
            );
            ui.painter().text(
                egui::Rect::from_min_size(
                    egui::pos2(ui.cursor().min.x + x, ui.cursor().min.y + y),
                    egui::vec2(chip_w, CHIP_H),
                )
                .center(),
                egui::Align2::CENTER_CENTER,
                *label,
                egui::FontId::proportional(11.0),
                if on { BACKDROP } else { TEXT_MUTED },
            );
        }
        let rows = button_row.len().div_ceil(per_row).max(1);
        ui.allocate_space(egui::vec2(row_w, rows as f32 * (CHIP_H + GAP)));
        ui.add_space(SPACE_MD);
        // Axes, in XInput's own units, which is what a game actually reads.
        ui.label(RichText::new("AXES").color(TEXT_FAINT).size(11.0).strong());
        ui.add_space(SPACE_XS);
        // Two fixed-width columns: the bars size themselves to the available
        // width, which a Grid resolves against its own content and ends up
        // giving the two cells different widths.
        let col_w = ((ui.available_width() - SPACE_MD) * 0.5).max(80.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(col_w);
                axis_row(ui, "left X", out.thumb_lx);
                axis_row(ui, "left Y", out.thumb_ly);
                widgets::trigger_bar(
                    ui,
                    "LT",
                    out.left_trigger as f32 / 255.0,
                    pressed & padcore::output::XButtons::LB != 0,
                );
            });
            ui.vertical(|ui| {
                ui.set_width(col_w);
                axis_row(ui, "right X", out.thumb_rx);
                axis_row(ui, "right Y", out.thumb_ry);
                widgets::trigger_bar(
                    ui,
                    "RT",
                    out.right_trigger as f32 / 255.0,
                    pressed & padcore::output::XButtons::RB != 0,
                );
            });
        });
        ui.add_space(SPACE_MD);
        divider(ui);
        ui.add_space(SPACE_MD);
        ui.label(
            RichText::new("FRAME TIMING")
                .color(TEXT_FAINT)
                .size(11.0)
                .strong(),
        );
        ui.add_space(SPACE_XS);
        ui.label(
            RichText::new("Interval between HID reports, in milliseconds.")
                .color(TEXT_MUTED)
                .size(11.5),
        );
        widgets::sparkline(ui, &ctx.state.frame_history, 12.0, ACCENT);
        ui.add_space(SPACE_XS);
        egui::Grid::new("timing")
            .num_columns(2)
            .spacing([SPACE_MD, 2.0])
            .show(ui, |ui| {
                ui.label(RichText::new("packets").color(TEXT_MUTED).size(11.5));
                ui.label(
                    RichText::new(t.packets.to_string())
                        .color(TEXT)
                        .size(11.5)
                        .monospace(),
                );
                ui.end_row();
                ui.label(RichText::new("effective rate").color(TEXT_MUTED).size(11.5));
                ui.label(
                    RichText::new(if t.frame_ms > 0.0 {
                        format!("{:.0} Hz", 1000.0 / t.frame_ms)
                    } else {
                        "-".into()
                    })
                    .color(TEXT)
                    .size(11.5)
                    .monospace(),
                );
                ui.end_row();
            });
    });
}
/// A signed axis, shown both as a bar and as the raw count.
/// A signed axis in XInput's own units, which is what a game actually reads.
fn axis_row(ui: &mut egui::Ui, label: &str, value: i16) {
    widgets::axis_bar(ui, label, value as f32 / i16::MAX as f32, ACCENT);
}
fn refresh_devices(ctx: &mut super::Ctx) {
    let stale = match ctx.state.devices_scanned_at {
        Some(t) => t.elapsed().as_secs_f32() > 1.0,
        None => true,
    };
    if stale {
        ctx.state.devices = padcore::engine::list_devices();
        ctx.state.devices_scanned_at = Some(std::time::Instant::now());
    }
}
