//! Settings page: window behaviour, startup, hotkeys, logs, and diagnostics.
use egui::{RichText, Stroke};
use padcore::engine::EngineCommand;
use padcore::hotkey::{HotkeyAction, HotkeyBinding, Mods};
use crate::state::ToastLevel;
use crate::theme::*;

#[allow(clippy::too_many_lines)]
pub fn draw(ui: &mut egui::Ui, ctx: &mut super::Ctx) {
    ui.add_space(SPACE_SM);
    // Capture mode consumes raw key events, so grab them before anything else.
    let keys: Vec<egui::Event> = if ctx.state.capturing.is_some() {
        ctx.input_keys.clone()
    } else {
        Vec::new()
    };
    ui.columns(2, |cols| {
        general(&mut cols[0], ctx);
        hotkeys(&mut cols[1], ctx, &keys);
    });
    ui.columns(2, |cols| {
        startup(&mut cols[0], ctx);
        diagnostics(&mut cols[1], ctx);
    });
}
/// Window behaviour and general preferences.
fn general(ui: &mut egui::Ui, ctx: &mut super::Ctx) {
    card().show(ui, |ui| {
        ui.label(RichText::new("General").color(TEXT).size(15.0).strong());
        ui.add_space(SPACE_MD);
        let s = &mut ctx.state.settings;
        ui.checkbox(
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
        ui.checkbox(&mut s.start_minimized, "Start hidden in the tray");
        ui.checkbox(&mut s.auto_reapply_profile, "Re-apply the profile on reconnect");
        ui.add_space(SPACE_MD);
        divider(ui);
        ui.add_space(SPACE_MD);
        ui.label(RichText::new("AUTO PROFILES").color(TEXT_FAINT).size(11.0).strong());
        ui.add_space(SPACE_SM);
        ui.checkbox(&mut s.auto_profiles_enabled, "Switch profiles by foreground window");
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
            push(ctx);
        }
        ui.add_space(SPACE_MD);
        ui.horizontal(|ui| {
            if secondary_button(ui, "Save settings").clicked() {
                push(ctx);
                if let Err(e) = ctx.state.settings.save() {
                    ctx.state
                        .toast(format!("Could not save settings: {e}"), ToastLevel::Error);
                } else {
                    ctx.state
                        .toast("Settings saved", ToastLevel::Success);
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
/// Global hotkeys, with a capture mode.
fn hotkeys(ui: &mut egui::Ui, ctx: &mut super::Ctx, keys: &[egui::Event]) {
    card().show(ui, |ui| {
        ui.label(RichText::new("Global hotkeys").color(TEXT).size(15.0).strong());
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
                        if ui.add(btn).on_hover_text("Click, then press a combination").clicked()
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
        ctx.state
            .toast("Hotkey registered", ToastLevel::Success);
    }
}
/// Autostart, and the config location.
fn startup(ui: &mut egui::Ui, ctx: &mut super::Ctx) {
    card().show(ui, |ui| {
        ui.label(RichText::new("Startup").color(TEXT).size(15.0).strong());
        ui.add_space(SPACE_MD);
        let mut autostart = padcore::settings::autostart_enabled();
        if ui.checkbox(&mut autostart, "Start PadForge when I log in").changed() {
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
                Err(e) => ctx.state.toast(
                    format!("Could not update startup: {e}"),
                    ToastLevel::Error,
                ),
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
    });
}
/// Logging and a one-shot report.
fn diagnostics(ui: &mut egui::Ui, ctx: &mut super::Ctx) {
    let telemetry = ctx.engine.telemetry();
    card().show(ui, |ui| {
        ui.label(RichText::new("Diagnostics").color(TEXT).size(15.0).strong());
        ui.add_space(SPACE_MD);
        let mut logging = ctx.state.settings.logging;
        if ui.checkbox(&mut logging, "Write a log file").changed() {
            ctx.state.settings.logging = logging;
            push(ctx);
        }
        let mut debug = ctx.state.settings.debug_logging;
        if ui.checkbox(&mut debug, "Verbose logging").changed() {
            ctx.state.settings.debug_logging = debug;
            push(ctx);
            ctx.state
                .toast("Takes effect on restart", ToastLevel::Info);
        }
        ui.add_space(SPACE_MD);
        divider(ui);
        ui.add_space(SPACE_MD);
        ui.label(RichText::new("STATUS").color(TEXT_FAINT).size(11.0).strong());
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
                    (
                        "active profile",
                        telemetry.profile_name.clone(),
                    ),
                    (
                        "packets",
                        telemetry.packets.to_string(),
                    ),
                ];
                for (label, value) in rows {
                    ui.label(RichText::new(label).color(TEXT_MUTED).size(11.5));
                    ui.label(
                        RichText::new(value)
                            .color(TEXT)
                            .size(11.5)
                            .monospace(),
                    );
                    ui.end_row();
                }
            });
        ui.add_space(SPACE_MD);
        if secondary_button(ui, "Open the data folder").clicked() {
            let dir = padcore::paths::config_dir();
            if let Err(e) = std::process::Command::new("explorer")
                .arg(&dir)
                .spawn()
            {
                ctx.state.toast(
                    format!("Could not open {}: {e}", dir.display()),
                    ToastLevel::Error,
                );
            }
        }
    });
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
    ctx.engine
        .send(EngineCommand::ApplyHotkeys(bindings));
}
