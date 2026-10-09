//! The egui application: window chrome, navigation, page routing, and toasts.

use std::time::Duration;

use egui::epaint::MarginF32;
use egui::{Align, Color32, CornerRadius, RichText, Stroke};

use padcore::engine::{EngineCommand, EngineHandle};

use crate::pages::{self, Ctx};
use crate::state::{AppState, Page, ToastLevel};
use crate::theme::*;
use crate::tray::{self, TrayIcon, TrayInbox};

/// Accent colour, shared with the tray icon.
pub const ACCENT: Color32 = Color32::from_rgb(0x35, 0xD0, 0xE8);

/// Root eframe application.
pub struct App {
    engine: EngineHandle,
    state: AppState,
    tray: Option<TrayIcon>,
    /// Commands raised by the tray icon's background thread.
    inbox: TrayInbox,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext,
        engine: EngineHandle,
        state: AppState,
        inbox: TrayInbox,
    ) -> Self {
        crate::theme::install(&cc.egui_ctx);

        let mut app = Self {
            engine,
            state,
            tray: None,
            inbox,
        };
        app.push_initial_commands();
        app
    }

    /// Send the initial settings, hotkeys and profiles to the engine, so its
    /// first frame already reflects the user's configuration.
    fn push_initial_commands(&mut self) {
        self.engine.send(EngineCommand::ApplySettings(Box::new(
            self.state.settings.clone(),
        )));
        let store = self.state.store.clone();
        self.engine
            .send(EngineCommand::ApplyProfiles(Box::new(store)));
        let hotkeys = self.state.hotkeys.clone();
        self.engine.send(EngineCommand::ApplyHotkeys(hotkeys));
    }

    /// Create the tray icon once, on the first frame.
    fn ensure_tray(&mut self) {
        if self.tray.is_some() {
            return;
        }
        match TrayIcon::new(std::sync::Arc::clone(&self.inbox)) {
            Ok(tray) => self.tray = Some(tray),
            Err(e) => {
                // Do not retry every frame; the window remains fully usable.
                self.state.tray_pending = false;
                self.state
                    .toast(format!("Tray icon unavailable: {e}"), ToastLevel::Warning);
            }
        }
    }

    /// Sample telemetry into the history buffers.
    fn sample_telemetry(&mut self) {
        let t = self.engine.telemetry();
        AppState::push_history(
            &mut self.state.gyro_history,
            t.gyro_delta.0,
            padcore::engine::HISTORY_LEN,
        );
        AppState::push_history(
            &mut self.state.frame_history,
            t.frame_ms,
            padcore::engine::HISTORY_LEN,
        );
    }

    /// Persist anything marked dirty.
    fn autosave(&mut self) {
        if self.state.settings_dirty {
            self.state.settings_dirty = false;
            if let Err(e) = self.state.settings.save() {
                self.state
                    .toast(format!("Could not save settings: {e}"), ToastLevel::Error);
            }
        }
        if self.state.profiles_dirty {
            self.state.profiles_dirty = false;
            if let Err(e) = self.state.store.save() {
                self.state
                    .toast(format!("Could not save profiles: {e}"), ToastLevel::Error);
            }
        }
    }

    /// Release the pad and persist state, then close.
    fn shutdown(&mut self, ctx: &egui::Context) {
        self.state.quit = false;
        self.state.settings.save().ok();
        self.state.store.save().ok();
        self.engine.send(EngineCommand::Shutdown);
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    /// Handle window visibility and the close button.
    fn handle_window_commands(&mut self, ctx: &egui::Context) {
        if self.state.restore_requested {
            self.state.restore_requested = false;
            self.state.hidden = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }

        if self.state.minimise_requested {
            self.state.minimise_requested = false;
            self.state.hidden = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            return;
        }

        if ctx.input(|i| i.viewport().close_requested()) {
            if self.state.settings.minimize_on_close {
                // Cancel the close and hide instead, so a running game keeps
                // receiving input.
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.state.minimise_requested = true;
            } else {
                self.state.quit = true;
            }
        }
    }

    /// Repaint faster when a pad is live; there is no point drawing at 60 Hz
    /// with nothing attached.
    fn schedule_repaint(&self, ctx: &egui::Context) {
        let interval = if self.engine.telemetry().connected {
            Duration::from_millis(16)
        } else {
            Duration::from_millis(250)
        };
        ctx.request_repaint_after(interval);
    }

    /// Act on anything the tray raised.
    fn service_tray(&mut self) {
        let commands: Vec<tray::TrayCommand> = match self.inbox.lock() {
            Ok(mut v) => std::mem::take(&mut *v),
            Err(_) => return,
        };
        for cmd in commands {
            match cmd {
                tray::TrayCommand::Show => self.state.restore_requested = true,
                tray::TrayCommand::Quit => self.state.quit = true,
                other => {
                    tray::apply(other, &self.engine);
                }
            }
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Called even while hidden, which is exactly when we still need to
        // process tray commands and engine events.
        self.service_tray();
        ctx.request_repaint();
    }

    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.ensure_tray();

        for event in self.engine.drain_events() {
            let level = match event.level {
                padcore::engine::EventLevel::Info => ToastLevel::Info,
                padcore::engine::EventLevel::Warning => ToastLevel::Warning,
                padcore::engine::EventLevel::Error => ToastLevel::Error,
            };
            self.state.toast(event.text, level);
        }

        self.state.expire_toasts();
        self.sample_telemetry();
        self.schedule_repaint(root.ctx());

        self.handle_window_commands(root.ctx());

        let ctx = root.ctx().clone();
        self.draw(root, &ctx);
        self.autosave();

        if self.state.quit {
            self.shutdown(&ctx);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Make sure nothing is left held down in a game.
        self.state.settings.save().ok();
        self.state.store.save().ok();
        self.engine.send(EngineCommand::Shutdown);
        self.tray = None;
    }
}

impl App {
    /// The whole UI: header, navigation rail, page body, toasts.
    fn draw(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        egui::Panel::top("header")
            .resizable(false)
            .exact_size(52.0)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(SURFACE)
                    .stroke(Stroke::new(1.0, BORDER))
                    .inner_margin(MarginF32::symmetric(SPACE_MD, SPACE_SM)),
            )
            .show(root, |ui| self.header(ui));

        egui::Panel::left("nav")
            .resizable(false)
            .exact_size(196.0)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(BACKDROP)
                    .stroke(Stroke::new(1.0, BORDER))
                    .inner_margin(MarginF32::same(SPACE_SM)),
            )
            .show(root, |ui| self.nav(ui));

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(BACKDROP)
                    .inner_margin(MarginF32::same(SPACE_LG)),
            )
            .show(root, |ui| {
                let page = self.state.page;
                // Snapshot the key events before handing out a &mut to state.
                let keys: Vec<egui::Event> = ctx.input(|i| i.events.clone());
                let mut page_ctx = Ctx {
                    state: &mut self.state,
                    engine: &self.engine,
                    input_keys: keys,
                };
                // The viewport width has to be captured here: inside the
                // ScrollArea below, `available_width()` is unbounded, so any
                // widget that sizes itself to it would grow past the window.
                let content_w = ui.available_width();
                ui.set_max_width(content_w);
                egui::ScrollArea::vertical()
                    .id_salt("page_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_max_width(content_w);
                        match page {
                            Page::Dashboard => pages::dashboard::draw(ui, &mut page_ctx),
                            Page::Controller => pages::controller::draw(ui, &mut page_ctx),
                            Page::Mapping => pages::mapping::draw(ui, &mut page_ctx),
                            Page::Gyro => pages::gyro::draw(ui, &mut page_ctx),
                            Page::Profiles => pages::profiles::draw(ui, &mut page_ctx),
                            Page::Output => pages::output::draw(ui, &mut page_ctx),
                            Page::Settings => pages::settings::draw(ui, &mut page_ctx),
                        }
                    });
            });

        self.draw_toasts(ctx);
    }

    /// Window header: wordmark, connection state, pause toggle.
    fn header(&mut self, ui: &mut egui::Ui) {
        let t = self.engine.telemetry();
        ui.horizontal(|ui| {
            pages::dashboard::logo(ui, ACCENT);
            ui.add_space(SPACE_SM);
            ui.vertical(|ui| {
                ui.label(RichText::new("PadForge").color(TEXT).size(16.0).strong());
                ui.label(
                    RichText::new(self.state.page.title())
                        .color(TEXT_FAINT)
                        .size(10.5),
                );
            });

            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                let paused = self.state.settings.paused;
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(if paused { ">" } else { "||||" })
                                .color(if paused { BACKDROP } else { TEXT })
                                .size(13.0),
                        )
                        .fill(if paused { WARNING } else { SURFACE_RAISED })
                        .stroke(Stroke::new(1.0, BORDER))
                        .corner_radius(RADIUS_SM)
                        .min_size(egui::vec2(34.0, 28.0)),
                    )
                    .on_hover_text("Pause or resume output")
                    .clicked()
                {
                    self.engine.send(EngineCommand::TogglePause);
                    // Keep our copy in step so the button colour is right.
                    self.state.settings.paused = !paused;
                }

                let profile_label = if t.profile_name.is_empty() {
                    "no profile".to_string()
                } else {
                    t.profile_name.clone()
                };
                pill(ui, &profile_label, ACCENT);

                let (colour, label) = if !t.connected {
                    (DANGER, "Disconnected")
                } else if t.paused {
                    (WARNING, "Paused")
                } else {
                    (SUCCESS, "Connected")
                };
                pill(ui, label, colour);
            });
        });
    }

    /// The left navigation rail.
    fn nav(&mut self, ui: &mut egui::Ui) {
        ui.add_space(SPACE_XS);
        ui.label(
            RichText::new("PADFORGE")
                .color(TEXT)
                .size(13.0)
                .strong()
                .extra_letter_spacing(2.0),
        );
        ui.label(
            RichText::new("DualShock 4 -> Windows")
                .color(TEXT_FAINT)
                .size(10.5),
        );
        ui.add_space(SPACE_LG);

        for page in Page::ALL {
            let selected = self.state.page == *page;
            let text = RichText::new(format!("{}  {}", page.tag(), page.title()))
                .color(if selected { BACKDROP } else { TEXT_MUTED })
                .size(13.0);
            let button = egui::Button::new(text)
                .fill(if selected {
                    ACCENT
                } else {
                    Color32::TRANSPARENT
                })
                .stroke(Stroke::NONE)
                .corner_radius(RADIUS_SM)
                .min_size(egui::vec2(ui.available_width(), 32.0));
            if ui.add(button).clicked() {
                self.state.page = *page;
            }
            ui.add_space(2.0);
        }

        // Pin the health summary to the bottom of the rail.
        ui.with_layout(egui::Layout::bottom_up(Align::Min), |ui| {
            let t = self.engine.telemetry();
            ui.label(
                RichText::new(format!("v{}", padcore::APP_VERSION))
                    .color(TEXT_FAINT)
                    .size(10.5),
            );
            if !t.output_connected {
                ui.label(RichText::new("No virtual pad").color(DANGER).size(11.0));
                if secondary_button(ui, "Fix").clicked() {
                    self.state.page = Page::Output;
                }
            }
        });
    }

    /// Transient messages, stacked bottom-right.
    fn draw_toasts(&mut self, ctx: &egui::Context) {
        if self.state.toasts.is_empty() {
            return;
        }
        let toasts = self.state.toasts.clone();
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -16.0))
            .order(egui::Order::Tooltip)
            .show(ctx, |ui| {
                // Reverse so the newest sits nearest the corner.
                for toast in toasts.iter().rev() {
                    let colour = match toast.level {
                        ToastLevel::Info => ACCENT,
                        ToastLevel::Success => SUCCESS,
                        ToastLevel::Warning => WARNING,
                        ToastLevel::Error => DANGER,
                    };
                    // Fade out over the last third of the toast's life.
                    let age = toast.born.elapsed().as_secs_f32();
                    let progress = (age / toast.ttl_secs).clamp(0.0, 1.0);
                    let fade = if progress < 0.7 {
                        1.0
                    } else {
                        1.0 - (progress - 0.7) / 0.3
                    };

                    egui::Frame::new()
                        .fill(SURFACE_RAISED.gamma_multiply(fade))
                        .stroke(Stroke::new(1.0, colour.gamma_multiply(0.55 * fade)))
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(MarginF32::symmetric(SPACE_MD, SPACE_SM))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("*")
                                        .color(colour.gamma_multiply(fade))
                                        .size(10.0),
                                );
                                ui.label(
                                    RichText::new(&toast.text)
                                        .color(TEXT.gamma_multiply(fade))
                                        .size(12.5),
                                );
                            });
                        });
                    ui.add_space(SPACE_SM);
                }
            });
    }
}
