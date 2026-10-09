//! Profiles page: create, edit, and auto-assign per-game configurations.
use crate::state::ToastLevel;
use crate::theme::*;
use egui::{RichText, Stroke};
use padcore::engine::EngineCommand;
use padcore::profile::{AutoProfileRule, Profile};

#[allow(clippy::too_many_lines)]
pub fn draw(ui: &mut egui::Ui, ctx: &mut super::Ctx) {
    ui.add_space(SPACE_SM);
    ui.columns(2, |cols| {
        list(&mut cols[0], ctx);
        auto_rules(&mut cols[1], ctx);
    });
}
/// The profile list, with add / duplicate / rename / delete.
fn list(ui: &mut egui::Ui, ctx: &mut super::Ctx) {
    card().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Profiles").color(TEXT).size(15.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if primary_button(ui, "+  New").clicked() {
                    let mut p = Profile::new();
                    p.name = unique_name(&ctx.state.store.profiles, "New profile");
                    ctx.state.store.add(p);
                    ctx.state.editing = ctx.state.store.active;
                    push_store(ctx);
                    ctx.state.toast("Profile created", ToastLevel::Success);
                }
            });
        });
        ui.add_space(SPACE_SM);
        let profiles: Vec<(usize, String, bool)> = ctx
            .state
            .store
            .profiles
            .iter()
            .enumerate()
            .map(|(i, p)| (i, p.name.clone(), i == ctx.state.store.active))
            .collect();
        for (index, name, is_active) in profiles {
            ui.horizontal(|ui| {
                // Selecting a row is the primary action; the buttons act on the
                // row that is being edited.
                let selected = ctx.state.editing == index;
                let text = RichText::new(&name)
                    .color(if is_active { BACKDROP } else { TEXT })
                    .size(13.5);
                let button = egui::Button::new(text)
                    .fill(if is_active {
                        ACCENT
                    } else if selected {
                        SURFACE_HOVER
                    } else {
                        SURFACE
                    })
                    .stroke(Stroke::new(1.0, if is_active { ACCENT } else { BORDER }))
                    .corner_radius(RADIUS_SM)
                    .min_size(egui::vec2(0.0, 30.0));
                let response = ui.add(button);
                // A single click just selects for editing; a double click also
                // makes it the live profile.
                if response.double_clicked() {
                    ctx.state.editing = index;
                    let id = ctx.state.store.profiles[index].id.clone();
                    ctx.engine.send(EngineCommand::SelectProfile(id.clone()));
                    ctx.state.store.select(&id);
                    push_store(ctx);
                } else if response.clicked() {
                    ctx.state.editing = index;
                }
            });
        }
        ui.add_space(SPACE_MD);
        divider(ui);
        ui.add_space(SPACE_MD);
        let editing_id = ctx.state.editing_profile().id.clone();
        let can_delete = ctx.state.store.profiles.len() > 1;
        ui.horizontal(|ui| {
            if secondary_button(ui, "Duplicate").clicked() {
                let source = ctx.state.editing_profile().clone();
                let name = unique_name(&ctx.state.store.profiles, &format!("{} copy", source.name));
                let copy = source.duplicate_with(name);
                ctx.state.store.add(copy);
                ctx.state.editing = ctx.state.store.active;
                push_store(ctx);
                ctx.state.toast("Profile duplicated", ToastLevel::Success);
            }
            let _ = can_delete;
            if secondary_button(ui, "Delete").clicked() {
                if ctx.state.store.remove(&editing_id) {
                    ctx.state.editing = 0;
                    push_store(ctx);
                    ctx.state.toast("Profile deleted", ToastLevel::Warning);
                } else {
                    ctx.state
                        .toast("The last profile cannot be deleted", ToastLevel::Warning);
                }
            }
        });
        ui.add_space(SPACE_MD);
        section(ui, "Rename");
        let mut name = ctx.state.editing_profile().name.clone();
        if ui
            .add(egui::TextEdit::singleline(&mut name).desired_width(ui.available_width()))
            .changed()
        {
            ctx.state.store.rename(&editing_id, name);
            push_store(ctx);
        }
    });
}
/// Auto-profile rules: switch profile when a process comes to the front.
fn auto_rules(ui: &mut egui::Ui, ctx: &mut super::Ctx) {
    card().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Auto profiles")
                    .color(TEXT)
                    .size(15.0)
                    .strong(),
            );
        });
        ui.add_space(SPACE_SM);
        ui.label(
            RichText::new(
                "When a matching program comes to the front, its profile is applied automatically. \
                 Matching is on the process name, case-insensitively.",
            )
            .color(TEXT_MUTED)
            .size(12.0),
        );
        ui.add_space(SPACE_MD);
        let mut enabled = ctx.state.settings.auto_profiles_enabled;
        if ui
            .checkbox(&mut enabled, "Watch the foreground window")
            .changed()
        {
            ctx.state.settings.auto_profiles_enabled = enabled;
            push_settings(ctx);
        }
        ui.add_space(SPACE_SM);
        // Rule rows.
        let rules: Vec<AutoProfileRule> = ctx.state.store.auto_profiles.clone();
        let mut changed = false;
        let mut to_remove: Option<usize> = None;
        for (i, rule) in rules.iter().enumerate() {
            ui.push_id(i, |ui| {
                ui.horizontal(|ui| {
                    let mut proc = rule.process.clone();
                    ui.add(
                        egui::TextEdit::singleline(&mut proc)
                            .hint_text("process name, e.g. cyberpunk2077")
                            .desired_width(150.0),
                    );
                    if proc != rule.process {
                        changed = true;
                        // Mutate through the index we captured.
                        if let Some(slot) = ctx.state.store.auto_profiles.get_mut(i) {
                            slot.process = proc;
                        }
                    }
                    let mut profile_id = rule.profile_id.clone();
                    let profiles: Vec<(String, String)> = ctx
                        .state
                        .store
                        .profiles
                        .iter()
                        .map(|p| (p.id.clone(), p.name.clone()))
                        .collect();
                    egui::ComboBox::from_id_salt("rule_profile")
                        .selected_text(
                            profiles
                                .iter()
                                .find(|(id, _)| *id == profile_id)
                                .map(|(_, n)| n.clone())
                                .unwrap_or_else(|| "missing".to_string()),
                        )
                        .width(120.0)
                        .show_ui(ui, |ui| {
                            for (id, name) in &profiles {
                                ui.selectable_value(&mut profile_id, id.clone(), name.clone());
                            }
                        });
                    if profile_id != rule.profile_id {
                        changed = true;
                        if let Some(slot) = ctx.state.store.auto_profiles.get_mut(i) {
                            slot.profile_id = profile_id;
                        }
                    }
                    let mut on = rule.enabled;
                    if ui.checkbox(&mut on, "").changed() {
                        changed = true;
                        if let Some(slot) = ctx.state.store.auto_profiles.get_mut(i) {
                            slot.enabled = on;
                        }
                    }
                    if ui
                        .small_button("X")
                        .on_hover_text("Remove this rule")
                        .clicked()
                    {
                        to_remove = Some(i);
                    }
                });
            });
        }
        if let Some(i) = to_remove {
            ctx.state.store.auto_profiles.remove(i);
            changed = true;
        }
        if changed {
            push_store(ctx);
        }
        ui.add_space(SPACE_MD);
        ui.horizontal(|ui| {
            if secondary_button(ui, "+  Add rule").clicked() {
                // Seed with the first profile so the row is immediately valid.
                let profile_id = ctx
                    .state
                    .store
                    .profiles
                    .first()
                    .map(|p| p.id.clone())
                    .unwrap_or_default();
                ctx.state.store.auto_profiles.push(AutoProfileRule {
                    process: String::new(),
                    profile_id,
                    enabled: true,
                    window_title: String::new(),
                });
                push_store(ctx);
            }
        });
        if ctx.state.store.auto_profiles.is_empty() {
            ui.add_space(SPACE_SM);
            ui.label(
                RichText::new("No rules yet. Add one and type a process name.")
                    .color(TEXT_FAINT)
                    .size(12.0),
            );
        }
        ui.add_space(SPACE_MD);
        divider(ui);
        // Import and export.
        ui.add_space(SPACE_MD);
        section(ui, "Transfer");
        ui.horizontal(|ui| {
            if secondary_button(ui, "Import profile...").clicked() {
                import_profile(ctx);
            }
            if secondary_button(ui, "Export current...").clicked() {
                export_profile(ctx);
            }
        });
        ui.add_space(SPACE_XS);
        ui.label(
            RichText::new(format!(
                "Profiles live in {}",
                padcore::paths::profiles_dir().display()
            ))
            .color(TEXT_FAINT)
            .size(11.0),
        );
    });
}
/// Write the active profile to a file the user picks.
fn export_profile(ctx: &mut super::Ctx) {
    let profile = ctx.state.editing_profile().clone();
    let suggested = format!(
        "{}.json",
        profile.name.replace(|c: char| !c.is_alphanumeric(), "_")
    );
    let path = rfd::FileDialog::new()
        .set_title("Export profile")
        .set_file_name(&suggested)
        .add_filter("JSON", &["json"])
        .save_file();
    let Some(path) = path else { return };
    match serde_json::to_string_pretty(&profile) {
        Ok(body) => match std::fs::write(&path, body) {
            Ok(()) => ctx.state.toast(
                format!("Exported to {}", path.display()),
                ToastLevel::Success,
            ),
            Err(e) => ctx
                .state
                .toast(format!("Could not write the file: {e}"), ToastLevel::Error),
        },
        Err(e) => ctx.state.toast(
            format!("Could not serialise the profile: {e}"),
            ToastLevel::Error,
        ),
    }
}
/// Read a profile from disk and add it to the store.
fn import_profile(ctx: &mut super::Ctx) {
    let path = rfd::FileDialog::new()
        .set_title("Import profile")
        .add_filter("JSON", &["json"])
        .pick_file();
    let Some(path) = path else { return };
    match std::fs::read_to_string(&path) {
        Ok(body) => match serde_json::from_str::<Profile>(&body) {
            Ok(mut p) => {
                // A fresh id, so importing the same file twice does not collide.
                p.id = padcore::profile::new_id();
                let name = unique_name(&ctx.state.store.profiles, &p.name);
                p.name = name;
                ctx.state.store.add(p);
                ctx.state.editing = ctx.state.store.active;
                push_store(ctx);
                ctx.state.toast("Profile imported", ToastLevel::Success);
            }
            Err(e) => ctx.state.toast(
                format!("That is not a PadForge profile: {e}"),
                ToastLevel::Error,
            ),
        },
        Err(e) => ctx
            .state
            .toast(format!("Could not read the file: {e}"), ToastLevel::Error),
    }
}
/// Pick a name that does not collide with an existing profile.
fn unique_name(existing: &[Profile], base: &str) -> String {
    if !existing.iter().any(|p| p.name == base) {
        return base.to_string();
    }
    for n in 2..1000 {
        let candidate = format!("{base} {n}");
        if !existing.iter().any(|p| p.name == candidate) {
            return candidate;
        }
    }
    format!("{base} {}", new_id_short())
}
fn new_id_short() -> String {
    padcore::profile::new_id()[..6].to_string()
}
fn push_store(ctx: &mut super::Ctx) {
    let store = ctx.state.store.clone();
    ctx.engine
        .send(EngineCommand::ApplyProfiles(Box::new(store)));
}
fn push_settings(ctx: &mut super::Ctx) {
    let settings = ctx.state.settings.clone();
    ctx.state.settings_dirty = true;
    ctx.engine
        .send(EngineCommand::ApplySettings(Box::new(settings)));
}
