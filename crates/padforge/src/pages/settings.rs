//! Settings page: general, auto profiles, hotkeys, and diagnostics, each in its
//! own card.
use crate::state::ToastLevel;
use crate::theme::*;
use crate::widgets::toggle_switch;
use egui::{RichText, Stroke};
use padcore::engine::EngineCommand;
use padcore::hotkey::{HotkeyAction, HotkeyBinding, Mods};

/// Labels get their own size and weight, so a switch's neighbouring text
/// cannot drift off the checkbox sizing it replaced.
///
/// Returns true when the switch moved, so the caller can push the setting.
fn toggle_row(ui: &mut egui::Ui, on: &mut bool, label: &str) -> bool {
    ui.horizontal(|ui| {
        let moved = toggle_switch(ui, on);
        ui.label(RichText::new(label).color(TEXT).size(13.0));
        moved.changed()
    })
    .inner
}

#[allow(clippy::too_many_lines)]
pub fn draw(ui: &mut egui::Ui, ctx: &mut super::Ctx) {
    ui.add_space(SPACE_SM);
    // Capture mode consumes raw key events, so grab them before anything else.
    let keys: Vec<egui::Event> = if ctx.state.capturing.is_some() {
        ctx.input_keys.clone()
    } else {
        Vec::new()
    };
    ui.set_max_width(ui.available_width());
    settings_grid(ui, ctx, &keys);
}

/// The four cards, as two rows of two, made of two plain `ui.columns` calls.
///
/// The middle of the page's history did not believe two back-to-back calls
/// could sit tight — it reserved the viewport's spare height for the first
/// row to force the alignment instead, which traded one wrong number for
/// another: the row gap came to track the window height, not the spacing. On
/// egui 0.36 the original assumption is simply false, as egui's `columns`
/// advance the cursor past the tallest card actually drawn, so the plain pair
/// is what is here, and the render test beside the page proves the claim
/// rather than restating it.
///
/// Four cards in one row was tried instead and is the wrong shape at this
/// width: the cards come out narrower than the text inside them, and the
/// fourth one ends up under the scrollbar.
fn settings_grid(ui: &mut egui::Ui, ctx: &mut super::Ctx, keys: &[egui::Event]) {
    // One spacing pair governs everything: card to card inside a row, and row
    // to row below it.
    let gap = SPACE_MD;
    let spacing = ui.spacing().item_spacing;
    ui.spacing_mut().item_spacing.x = gap;
    ui.spacing_mut().item_spacing.y = gap;

    // egui 0.36's `columns` shrink-wraps: after each call the cursor sits at
    // the tallest card of that row, so two plain calls stack the rows edge to
    // edge. What this replaced reserved the viewport's *spare* height for the
    // first row, which made the gap between the rows track the window height
    // instead of the spacing: 17px spare on a 700px-tall window was invisible,
    // and the same layout on a taller one spaced the rows by the leftover
    // room. The render test beside the page pins the gap to the horizontal
    // one so a regression reads as numbers, not vibes.
    ui.columns(2, |cols| {
        general(&mut cols[0], ctx);
        auto_profiles(&mut cols[1], ctx);
    });
    ui.columns(2, |cols| {
        hotkeys(&mut cols[0], ctx, keys);
        diagnostics(&mut cols[1], ctx);
    });

    ui.spacing_mut().item_spacing = spacing;
}
/// Window behaviour and general preferences.
fn general(ui: &mut egui::Ui, ctx: &mut super::Ctx) {
    card().show(ui, |ui| {
        ui.label(RichText::new("General").color(TEXT).size(15.0).strong());
        ui.add_space(SPACE_MD);
        // Scoped so the settings borrow ends before the autostart row, which
        // reads the registry rather than the settings struct, needs `ctx` again.
        {
            let mut changed = false;
            {
                let s = &mut ctx.state.settings;
                changed |= toggle_row(
                    ui,
                    &mut s.minimize_on_close,
                    "Keep running when the window is closed",
                );
                ui.label(
                    RichText::new(
                        "PadForge stays in the notification area so a game keeps receiving input.",
                    )
                    .color(TEXT_FAINT)
                    .size(11.5),
                );
                ui.add_space(SPACE_SM);
                changed |= toggle_row(ui, &mut s.start_minimized, "Start hidden in the tray");
                changed |= toggle_row(
                    ui,
                    &mut s.auto_reapply_profile,
                    "Re-apply the profile on reconnect",
                );
            }
            // Applied as toggled, not on "Save settings": `auto_reapply_profile`
            // is the engine's behaviour on a reconnected pad, and a change that
            // waits for the button is a change that is not really in the app
            // yet. Autosave persists them in the same pass; the button remains
            // as the explicit "put it on disk now, again" gesture.
            if changed {
                push(ctx);
            }
        }
        ui.add_space(SPACE_MD);
        divider(ui);
        ui.add_space(SPACE_MD);
        ui.label(
            RichText::new("START WITH WINDOWS")
                .color(TEXT_FAINT)
                .size(11.0)
                .strong(),
        );
        ui.add_space(SPACE_SM);
        let mut autostart = padcore::settings::autostart_enabled();
        if toggle_row(ui, &mut autostart, "Start PadForge when I log in") {
            match padcore::settings::set_autostart(autostart) {
                Ok(()) => {
                    ctx.state.toast(
                        if autostart {
                            "Added to the Run registry key"
                        } else {
                            "Removed from the Run registry key"
                        },
                        ToastLevel::Success,
                    );
                }
                Err(e) => ctx
                    .state
                    .toast(format!("Could not update startup: {e}"), ToastLevel::Error),
            }
        }
        ui.label(
            RichText::new(
                "Implemented with the current-user Run key, so no administrator rights are needed.",
            )
            .color(TEXT_FAINT)
            .size(11.5),
        );
        ui.add_space(SPACE_MD);
        ui.horizontal(|ui| {
            if secondary_button(ui, "Save settings").clicked() {
                push(ctx);
                if let Err(e) = ctx.state.settings.save() {
                    ctx.state
                        .toast(format!("Could not save settings: {e}"), ToastLevel::Error);
                } else {
                    ctx.state.toast("Settings saved", ToastLevel::Success);
                }
            }
            if secondary_button(ui, "Revert").clicked() {
                let fresh = padcore::settings::Settings::load();
                ctx.state.settings = fresh;
                push(ctx);
            }
        });
    });
}
/// Profile switching by foreground window.
fn auto_profiles(ui: &mut egui::Ui, ctx: &mut super::Ctx) {
    let mut dirty = false;
    card().show(ui, |ui| {
        ui.label(
            RichText::new("Auto profiles")
                .color(TEXT)
                .size(15.0)
                .strong(),
        );
        ui.add_space(SPACE_MD);
        let s = &mut ctx.state.settings;
        if toggle_row(
            ui,
            &mut s.auto_profiles_enabled,
            "Switch profiles by foreground window",
        ) {
            dirty = true;
        }
        ui.label(
            RichText::new(
                "Give each profile a window title on the Profiles page; the one in \
                 front decides which applies.",
            )
            .color(TEXT_FAINT)
            .size(11.5),
        );
        ui.add_space(SPACE_SM);
        let mut interval = s.auto_profile_interval_ms as f32;
        if ui
            .add(
                egui::Slider::new(&mut interval, 100.0..=3000.0)
                    .text("check every")
                    .suffix(" ms")
                    .step_by(50.0)
                    .show_value(true),
            )
            .changed()
        {
            s.auto_profile_interval_ms = interval.round() as u64;
            dirty = true;
        }
    });
    if dirty {
        push(ctx);
    }
}
/// Global hotkeys, with a capture mode.
fn hotkeys(ui: &mut egui::Ui, ctx: &mut super::Ctx, keys: &[egui::Event]) {
    card().show(ui, |ui| {
        ui.label(
            RichText::new("Global hotkeys")
                .color(TEXT)
                .size(15.0)
                .strong(),
        );
        ui.add_space(SPACE_SM);
        ui.label(
            RichText::new(
                "These work while a game has focus. A combination must include at least one \
                 modifier, so it cannot swallow a bare key.",
            )
            .color(TEXT_MUTED)
            .size(12.0),
        );
        ui.add_space(SPACE_MD);
        if let Some(row) = ctx.state.capturing {
            capture_banner(ui, row, ctx, keys);
        }
        let bindings: Vec<HotkeyBinding> = ctx.state.hotkeys.clone();
        for (i, binding) in bindings.iter().enumerate() {
            ui.push_id(i, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(action_label(&binding.action))
                            .color(TEXT)
                            .size(13.0),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("X").on_hover_text("Remove").clicked() {
                            ctx.state.hotkeys.remove(i);
                            push_hotkeys(ctx);
                        }
                        let text = binding
                            .hotkey
                            .map(|h| h.describe())
                            .unwrap_or_else(|| "not set".to_string());
                        let btn = egui::Button::new(
                            RichText::new(text)
                                .color(if binding.enabled { ACCENT } else { TEXT_FAINT })
                                .size(12.0)
                                .monospace(),
                        )
                        .fill(SURFACE)
                        .stroke(Stroke::new(1.0, BORDER))
                        .corner_radius(RADIUS_SM);
                        if ui
                            .add(btn)
                            .on_hover_text("Click, then press a combination")
                            .clicked()
                        {
                            ctx.state.capturing = Some(i);
                        }
                    });
                });
            });
        }
        ui.add_space(SPACE_MD);
        ui.horizontal(|ui| {
            if secondary_button(ui, "+  Add hotkey").clicked() {
                ctx.state
                    .hotkeys
                    .push(HotkeyBinding::new(HotkeyAction::NextProfile, None));
                push_hotkeys(ctx);
            }
            if secondary_button(ui, "Reset defaults").clicked() {
                ctx.state.hotkeys = crate::state::default_hotkeys();
                push_hotkeys(ctx);
                ctx.state.toast("Hotkeys reset", ToastLevel::Info);
            }
        });
    });
}
/// The "press a key" prompt, which captures into a binding row.
fn capture_banner(ui: &mut egui::Ui, row: usize, ctx: &mut super::Ctx, keys: &[egui::Event]) {
    crate::pages::dashboard::banner(
        ui,
        ACCENT,
        "Press a combination",
        "Ctrl / Alt / Shift / Win plus a key. Escape cancels, Backspace clears.",
    );
    // Read raw key events while capturing.
    let mut captured: Option<(Mods, u32)> = None;
    let mut cancel = false;
    let mut clear = false;
    for event in keys {
        if let egui::Event::Key {
            key,
            modifiers,
            pressed: true,
            repeat: false,
            ..
        } = event
        {
            match key {
                egui::Key::Escape => {
                    cancel = true;
                    break;
                }
                egui::Key::Backspace => {
                    clear = true;
                    break;
                }
                _ => {}
            }
            let Some(code) = virtual_key_code(*key) else {
                continue;
            };
            // A bare modifier keypress is not a combination; the modifiers arrive
            // through the event's own modifier state.
            if modifiers.is_none() {
                continue;
            }
            captured = Some((
                Mods {
                    alt: modifiers.alt,
                    ctrl: modifiers.ctrl,
                    shift: modifiers.shift,
                    win: false,
                },
                code,
            ));
            break;
        }
    }
    if cancel {
        ctx.state.capturing = None;
    } else if clear {
        if let Some(b) = ctx.state.hotkeys.get_mut(row) {
            b.hotkey = None;
            b.enabled = false;
        }
        ctx.state.capturing = None;
        push_hotkeys(ctx);
    } else if let Some((mods, code)) = captured {
        if let Some(b) = ctx.state.hotkeys.get_mut(row) {
            b.hotkey = Some(padcore::hotkey::Hotkey::new(mods, code));
            b.enabled = true;
        }
        ctx.state.capturing = None;
        push_hotkeys(ctx);
        ctx.state.toast("Hotkey registered", ToastLevel::Success);
    }
}
/// Logging, live status, and where the files live.
fn diagnostics(ui: &mut egui::Ui, ctx: &mut super::Ctx) {
    let telemetry = ctx.engine.telemetry();
    card().show(ui, |ui| {
        ui.label(RichText::new("Diagnostics").color(TEXT).size(15.0).strong());
        ui.add_space(SPACE_MD);
        let mut logging = ctx.state.settings.logging;
        if toggle_row(ui, &mut logging, "Write a log file") {
            ctx.state.settings.logging = logging;
            push(ctx);
        }
        let mut debug = ctx.state.settings.debug_logging;
        if toggle_row(ui, &mut debug, "Verbose logging") {
            ctx.state.settings.debug_logging = debug;
            push(ctx);
            ctx.state.toast("Takes effect on restart", ToastLevel::Info);
        }
        ui.add_space(SPACE_MD);
        divider(ui);
        ui.add_space(SPACE_MD);
        ui.label(
            RichText::new("STATUS")
                .color(TEXT_FAINT)
                .size(11.0)
                .strong(),
        );
        ui.add_space(SPACE_SM);
        egui::Grid::new("diag")
            .num_columns(2)
            .spacing([SPACE_MD, 2.0])
            .show(ui, |ui| {
                let rows: [(&str, String); 7] = [
                    ("version", padcore::APP_VERSION.to_string()),
                    (
                        "pad",
                        if telemetry.connected {
                            telemetry.device_label.clone()
                        } else {
                            "not connected".into()
                        },
                    ),
                    (
                        "transport",
                        telemetry
                            .transport
                            .map(|t| t.label().to_string())
                            .unwrap_or_else(|| "-".into()),
                    ),
                    ("output", telemetry.output_backend.clone()),
                    (
                        "calibration",
                        if telemetry.calibration.is_captured() {
                            "captured".into()
                        } else {
                            "not captured".into()
                        },
                    ),
                    ("active profile", telemetry.profile_name.clone()),
                    ("packets", telemetry.packets.to_string()),
                ];
                for (label, value) in rows {
                    ui.label(RichText::new(label).color(TEXT_MUTED).size(11.5));
                    ui.label(RichText::new(value).color(TEXT).size(11.5).monospace());
                    ui.end_row();
                }
            });
        ui.add_space(SPACE_MD);
        divider(ui);
        ui.add_space(SPACE_MD);
        ui.label(RichText::new("FILES").color(TEXT_FAINT).size(11.0).strong());
        ui.add_space(SPACE_SM);
        for (label, path) in [
            ("Settings", padcore::paths::settings_file()),
            ("Profiles", padcore::profile::profiles_file()),
            ("Logs", padcore::paths::logs_dir()),
        ] {
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).color(TEXT_MUTED).size(12.0));
                ui.label(
                    RichText::new(path.display().to_string())
                        .color(TEXT_FAINT)
                        .size(11.0)
                        .monospace(),
                );
            });
        }
        ui.add_space(SPACE_MD);
        ui.horizontal(|ui| {
            if secondary_button(ui, "Open the data folder").clicked() {
                let dir = padcore::paths::config_dir();
                if let Err(e) = std::process::Command::new("explorer").arg(&dir).spawn() {
                    ctx.state.toast(
                        format!("Could not open {}: {e}", dir.display()),
                        ToastLevel::Error,
                    );
                }
            }
            if primary_button(ui, "Report a bug").clicked() {
                build_report(ctx);
            }
        });
        ui.label(
            RichText::new(
                "Collects the log, your settings and your profiles into one file in your \
                 Downloads folder, ready to attach to an issue. Your device serial is \
                 removed; nothing else is.",
            )
            .color(TEXT_FAINT)
            .size(11.5),
        );
    });
}

/// Build a bug report and say plainly where it went.
fn build_report(ctx: &mut super::Ctx) {
    let telemetry = ctx.engine.telemetry();

    // The things a maintainer will otherwise ask for in the first reply. All of
    // them are already known here, so asking means one extra round trip per
    // report.
    let summary = format!(
        "PadForge {}\n\
         pad: {}\n\
         transport: {}\n\
         output: {}\n\
         profile: {}\n\
         reports seen: {}\n\
         output slot: {}\n\
         lightbar: rgb({}, {}, {})\n\
         gyroscope enabled: {}\n\
         pointer problem: {}\n\
         output problem: {}\n",
        padcore::APP_VERSION,
        if telemetry.connected {
            telemetry.device_label.clone()
        } else {
            "not connected".into()
        },
        telemetry
            .transport
            .map(|t| t.label().to_string())
            .unwrap_or_else(|| "-".into()),
        telemetry.output_backend,
        telemetry.profile_name,
        telemetry.packets,
        telemetry
            .output_slot
            .map(|s| s.to_string())
            .unwrap_or_else(|| "not reported".into()),
        telemetry.lightbar[0],
        telemetry.lightbar[1],
        telemetry.lightbar[2],
        telemetry.gyro_enabled,
        telemetry.pointer_error.as_deref().unwrap_or("none"),
        telemetry.output_error.as_deref().unwrap_or("none"),
    );

    match padcore::bugreport::build(&summary) {
        Ok(built) => {
            tracing::info!(path = %built.path.display(), entries = built.entries, "bug report built");
            if !built.missing.is_empty() {
                ctx.state.toast(
                    format!(
                        "Report saved, but {} could not be collected.",
                        built.missing.join(", ")
                    ),
                    ToastLevel::Warning,
                );
            } else {
                ctx.state.toast(
                    format!("Report saved to {}", built.path.display()),
                    ToastLevel::Success,
                );
            }
            // Only offer to open the folder when the file exists. Offering it
            // otherwise is the kind of small lie that makes a person distrust
            // the next message.
            let path = built.path.clone();
            let _ = std::process::Command::new("explorer")
                .arg("/select,")
                .arg(&path)
                .spawn();
        }
        Err(e) => {
            tracing::error!(%e, "could not build the bug report");
            ctx.state.toast(
                format!("Could not build the report: {e}"),
                ToastLevel::Error,
            );
        }
    }
}
fn action_label(action: &HotkeyAction) -> &'static str {
    match action {
        HotkeyAction::SelectProfile(_) => "Switch to profile",
        HotkeyAction::NextProfile => "Next profile",
        HotkeyAction::PrevProfile => "Previous profile",
        HotkeyAction::TogglePause => "Pause / resume output",
        HotkeyAction::CycleLightbar => "Cycle lightbar mode",
        HotkeyAction::Recalibrate => "Recalibrate sticks",
    }
}
/// egui key to Win32 virtual-key code.
fn virtual_key_code(key: egui::Key) -> Option<u32> {
    use egui::Key;
    let code = match key {
        Key::ArrowLeft => 0x25,
        Key::ArrowUp => 0x26,
        Key::ArrowRight => 0x27,
        Key::ArrowDown => 0x28,
        Key::Backspace => 0x08,
        Key::Tab => 0x09,
        Key::Enter => 0x0D,
        Key::Escape => 0x1B,
        Key::Space => 0x20,
        Key::Insert => 0x2D,
        Key::Delete => 0x2E,
        Key::Home => 0x24,
        Key::End => 0x23,
        Key::PageUp => 0x21,
        Key::PageDown => 0x22,
        Key::F1 => 0x70,
        Key::F2 => 0x71,
        Key::F3 => 0x72,
        Key::F4 => 0x73,
        Key::F5 => 0x74,
        Key::F6 => 0x75,
        Key::F7 => 0x76,
        Key::F8 => 0x77,
        Key::F9 => 0x78,
        Key::F10 => 0x79,
        Key::F11 => 0x7A,
        Key::F12 => 0x7B,
        Key::Num0 => 0x60,
        Key::Num1 => 0x61,
        Key::Num2 => 0x62,
        Key::Num3 => 0x63,
        Key::Num4 => 0x64,
        Key::Num5 => 0x65,
        Key::Num6 => 0x66,
        Key::Num7 => 0x67,
        Key::Num8 => 0x68,
        Key::Num9 => 0x69,
        // Letters and digits: egui keys are already character-based for these.
        Key::A => 0x41,
        Key::B => 0x42,
        Key::C => 0x43,
        Key::D => 0x44,
        Key::E => 0x45,
        Key::F => 0x46,
        Key::G => 0x47,
        Key::H => 0x48,
        Key::I => 0x49,
        Key::J => 0x4A,
        Key::K => 0x4B,
        Key::L => 0x4C,
        Key::M => 0x4D,
        Key::N => 0x4E,
        Key::O => 0x4F,
        Key::P => 0x50,
        Key::Q => 0x51,
        Key::R => 0x52,
        Key::S => 0x53,
        Key::T => 0x54,
        Key::U => 0x55,
        Key::V => 0x56,
        Key::W => 0x57,
        Key::X => 0x58,
        Key::Y => 0x59,
        Key::Z => 0x5A,
        _ => return None,
    };
    Some(code)
}
fn push(ctx: &mut super::Ctx) {
    let settings = ctx.state.settings.clone();
    ctx.state.settings_dirty = true;
    ctx.engine
        .send(EngineCommand::ApplySettings(Box::new(settings)));
}
fn push_hotkeys(ctx: &mut super::Ctx) {
    let bindings = ctx.state.hotkeys.clone();
    ctx.engine.send(EngineCommand::ApplyHotkeys(bindings));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pages::testing as harness;
    use egui::vec2;

    /// Two rows forming a tidy pair: the first two cards share a top edge, the
    /// last two share one, and the vertical gap between the rows is the gap
    /// the cards get from each other horizontally — at any window height.
    ///
    /// The layout this replaced reserved the viewport's spare height for the
    /// first row, so the second row sat behind a band of empty space that
    /// grew with the window: invisible at 700px tall was the luck of the
    /// measured size, and taller windows spaced the rows by whatever room
    /// was left over. Two heights here mean the regression needs the slope
    /// right, not one lucky sample. A render is the only way to see that
    /// answer complete.
    #[test]
    fn the_four_cards_form_two_rows_with_no_hollow_between_them() {
        let ctx = egui::Context::default();
        for height in [700.0, 900.0] {
            let rects = harness::page_cards(&ctx, vec2(1100.0, height), draw);
            assert_eq!(
                rects.len(),
                4,
                "at {height:.0}px tall: expected the four settings cards, got {rects:#?}"
            );

            let mut by_x: Vec<egui::Rect> = rects.clone();
            by_x.sort_by_key(|r| (r.top() as i64, r.left() as i64));

            let (r0, r1, r2, r3) = (by_x[0], by_x[1], by_x[2], by_x[3]);
            // Row 0: the top two cards share a top edge.
            assert!(
                (r0.top() - r1.top()).abs() < 1.0,
                "first row cards are not level: {r0:?} vs {r1:?}"
            );
            // Row 1: same, one row down.
            assert!(
                (r2.top() - r3.top()).abs() < 1.0,
                "second row cards are not level: {r2:?} vs {r3:?}"
            );
            assert!(
                r2.top() > r0.bottom() + SPACE_SM - 1.0,
                "the second row overlaps the first"
            );

            // The hollow: what separates the rows must be the spacing, not the
            // stretch of nothing that used to sit here. Painted-rect gaps also
            // absorb a pixel or two of corner anti-aliasing rounding, hence
            // the tolerance.
            let gap_rows = r2.top() - (r0.bottom().max(r1.bottom()));
            let gap_cols = r1.left() - r0.right();
            assert!(
                gap_rows <= gap_cols + 2.0,
                "at a {height:.0}px-tall window the second row floats \
                 {gap_rows:.0}px below the first while the cards sit \
                 {gap_cols:.0}px apart horizontally"
            );
        }
    }

    /// The page must fit *under* one vertical scroll on a normal window rather
    /// than starting with the second row below the fold.
    #[test]
    fn the_two_rows_are_not_one_viewport_tall() {
        let ctx = egui::Context::default();
        let rects = harness::page_cards(&ctx, vec2(1100.0, 700.0), draw);
        let lowest = rects.iter().map(|r| r.bottom()).fold(0.0, f32::max);
        assert!(
            lowest < 1500.0,
            "the page's cards are {lowest:.0}px deep; the second row has slipped below the fold"
        );
    }
}
