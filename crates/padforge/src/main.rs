//! PadForge: give a PlayStation DualShock 4 a native voice on Windows.
//!
//! The DS4 speaks HID; almost every Windows game speaks XInput. PadForge reads
//! the real HID report, reshapes every axis, remaps every button, and publishes
//! the result as a virtual Xbox 360 gamepad.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod icons;
mod pages;
mod state;
mod theme;
mod tray;
mod widgets;

use std::sync::{Arc, Mutex};

use padcore::engine::{self, EngineHandle};
use padcore::profile::ProfileStore;
use padcore::settings::Settings;

use egui::{IconData, ViewportBuilder};

use app::App;
use state::AppState;
use tray::{TrayCommand, TrayInbox};

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let flags = CommandLine::parse(&args);

    // Load persisted state before spawning anything, so the engine's first
    // frame already reflects the user's configuration.
    let settings = Settings::load();
    let store = ProfileStore::load();

    init_logging(&settings);

    let engine = engine::spawn(settings.clone(), store.clone());
    let inbox: TrayInbox = Arc::new(Mutex::new(Vec::new()));
    let state = AppState::new(settings, store);

    let viewport = ViewportBuilder::default()
        .with_title("PadForge")
        .with_app_id("padforge.app")
        .with_inner_size([1180.0, 760.0])
        .with_min_inner_size([980.0, 640.0])
        .with_icon(IconData {
            rgba: icons::app_icon_rgba(),
            width: 64,
            height: 64,
        })
        .with_visible(!flags.start_hidden);

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        padcore::APP_NAME,
        options,
        Box::new(
            move |cc| Ok(Box::new(App::new(cc, engine, state, inbox)) as Box<dyn eframe::App>),
        ),
    )
}

/// Parsed command line.
struct CommandLine {
    /// `--tray`, or no window at all.
    start_hidden: bool,
}

impl CommandLine {
    fn parse(args: &[String]) -> Self {
        let hidden = args
            .iter()
            .skip(1)
            .any(|a| a == "--tray" || a == "--hidden" || a == "--background");
        Self {
            start_hidden: hidden,
        }
    }
}

/// A minimal file logger, so diagnostics land somewhere without pulling in a
/// tracing subscriber.
fn init_logging(settings: &Settings) {
    if !settings.logging {
        return;
    }
    if let Err(e) = padcore::paths::ensure_dir(&padcore::paths::logs_dir()) {
        eprintln!("padforge: could not create the log directory: {e}");
        return;
    }

    let path = padcore::paths::logs_dir().join("padforge.log");
    let file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(f) => f,
        Err(_) => return,
    };

    use std::io::Write;
    let mut file = file;
    let _ = writeln!(
        file,
        "[{}] padforge {} starting (logging={}, debug={})",
        timestamp(),
        padcore::APP_VERSION,
        settings.logging,
        settings.debug_logging
    );
}

/// Seconds since the Unix epoch, for log lines.
fn timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Drain tray commands and route them to the engine or the UI.
pub fn service_tray(inbox: &TrayInbox, engine: &EngineHandle, state: &mut AppState) {
    let commands: Vec<TrayCommand> = match inbox.lock() {
        Ok(mut v) => std::mem::take(&mut *v),
        Err(_) => return,
    };
    for cmd in commands {
        match cmd {
            TrayCommand::Show => state.restore_requested = true,
            TrayCommand::Quit => state.quit = true,
            other => {
                tray::apply(other, engine);
            }
        }
    }
}
