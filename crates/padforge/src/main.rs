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

use std::io::Write;
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

/// Route diagnostics into `%APPDATA%\PadForge\logs\padforge.log`.
///
/// This used to write a single line and stop, because no tracing subscriber was
/// ever installed — so the setting was named "logging" and the app logged
/// nothing. That is not a cosmetic loss: the engine announces a real hardware
/// failure with `warn!` ("force feedback notifications unavailable"), and with
/// nowhere for it to go, a broken vibration looks identical to a working one.
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
        Err(e) => {
            eprintln!("padforge: could not open the log file: {e}");
            return;
        }
    };

    let file = Arc::new(Mutex::new(file));
    {
        let mut guard = file.lock().unwrap_or_else(|e| e.into_inner());
        let _ = writeln!(
            guard,
            "[{}] padforge {} starting (logging={}, debug={})",
            timestamp(),
            padcore::APP_VERSION,
            settings.logging,
            settings.debug_logging
        );
    }

    // `debug` only when asked for. The engine's normal chatter is a line per
    // poll-rate change and nothing more, but a verbose run still has to be
    // something a person chose rather than something they inherited.
    let level = if settings.debug_logging {
        "debug"
    } else {
        "info"
    };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(format!("padcore={level},padforge={level}"))
    });

    // `try_init`, not `init`: a second call would panic, and the cost of
    // silently keeping the first subscriber is a log file rather than a crash.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_target(false)
        .with_writer(SharedFile(file))
        .try_init();
}

/// The log file, shared between the banner line above and every later event.
///
/// `MakeWriter` is implemented by hand rather than borrowed from the closure
/// form: that form cannot return a borrow of its own capture, and a log file
/// that is written from the engine thread and the UI thread at once needs the
/// lock held for the length of each write or the lines interleave.
///
/// The guard is wrapped rather than used directly because `MutexGuard` is not
/// `io::Write`; the wrapper forwards to the file underneath it and holds the
/// lock for exactly as long as the borrow lives.
struct SharedFile(Arc<Mutex<std::fs::File>>);

struct SharedFileGuard<'a>(std::sync::MutexGuard<'a, std::fs::File>);

impl std::io::Write for SharedFileGuard<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for SharedFile {
    type Writer = SharedFileGuard<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        SharedFileGuard(self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }
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
