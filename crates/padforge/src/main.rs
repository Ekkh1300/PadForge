//! PadForge: give a PlayStation DualShock 4 a native voice on Windows.
//!
//! The DS4 speaks HID; almost every Windows game speaks XInput. PadForge reads
//! the real HID report, reshapes every axis, remaps every button, and publishes
//! the result as a virtual Xbox 360 gamepad.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod cli;
mod icons;
mod pages;
mod state;
mod theme;
mod tray;
mod widgets;

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Set once the panic hook has written a dump, so the UI can offer it.
static PANIC_DUMPED: AtomicBool = AtomicBool::new(false);

/// Whether a crash has already been written out.
pub fn panic_was_dumped() -> bool {
    PANIC_DUMPED.load(Ordering::Relaxed)
}

use padcore::engine::{self, EngineHandle};
use padcore::profile::ProfileStore;
use padcore::settings::Settings;

use egui::{IconData, ViewportBuilder};

use app::App;
use state::AppState;
use tray::{TrayCommand, TrayInbox};

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let flags = cli::CommandLine::parse(&args);

    match flags.mode {
        cli::Mode::Help => {
            // Written to stdout rather than through `println!`, because this is
            // a GUI-subsystem binary: a console it opens on its own is the thing
            // the installer was changed to stop doing. A closed stdout — the
            // normal case when launched from Explorer — is not an error here.
            let _ = std::io::stdout().write_all(flags.usage().as_bytes());
            Ok(())
        }
        cli::Mode::Check => {
            // Deliberately before `init_logging`: this runs on someone else's
            // machine to answer a question, and the first thing it must not do
            // is change anything. No log file, no panic hook, no engine.
            let settings = Settings::load();
            let store = ProfileStore::load();
            let mut report = cli::check(&settings, &store);
            for u in &flags.unknown {
                report.lines.push(cli::Line {
                    state: "fail",
                    label: "command line",
                    detail: format!("unrecognised: {u}"),
                });
            }
            if flags.json {
                print!("{}", report.to_json());
            } else {
                print!("{}", report.render());
            }
            let _ = std::io::stdout().flush();
            std::process::exit(report.exit_code());
        }
        cli::Mode::Run { start_hidden } => {
            for u in &flags.unknown {
                eprintln!("padforge: ignoring unrecognised argument: {u}");
            }
            run(start_hidden, flags.profile)
        }
    }
}

/// The normal path: a window, and the engine behind it.
fn run(start_hidden: bool, profile: Option<String>) -> eframe::Result<()> {
    // Load persisted state before spawning anything, so the engine's first
    // frame already reflects the user's configuration.
    let settings = Settings::load();
    let mut store = ProfileStore::load();

    init_logging(&settings);
    install_panic_hook();

    // `--profile NAME`. Applied here rather than in the engine so the choice is
    // visible in the UI immediately, and a name that does not exist says so
    // instead of silently starting on something else.
    if let Some(name) = profile {
        match store.select_by_name(&name) {
            Ok(id) => {
                tracing::info!(profile = %name, %id, "starting on the profile named on the command line");
                let _ = store.save();
            }
            Err(available) => {
                let message = if available.is_empty() {
                    format!("no profile named '{name}'; there are none yet")
                } else {
                    format!(
                        "no profile named '{name}'. Available: {}",
                        available.join(", ")
                    )
                };
                tracing::warn!(%message);
                eprintln!("padforge: {message}");
                // Not fatal. Starting on the last-used profile is a reasonable
                // answer to a typo, and refusing to start over one would be
                // worse than the typo.
            }
        }
    }

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
        .with_visible(!start_hidden);

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

/// Write a crash to its own file before the process disappears.
///
/// Without this a panic takes everything with it. The log file exists but the
/// last thing in it is whatever happened to be logged a moment earlier, which
/// is rarely the panic — the default hook's message goes to the debugger's
/// output, which for a GUI-subsystem binary nobody has attached to. So the
/// evidence is written to a file that survives, in a place the bug-report
/// button will find without being asked.
///
/// Installed *after* the logging subscriber, and deliberately: the hook chains
/// to whatever was there before, so the previous behaviour is kept rather than
/// replaced.
fn install_panic_hook() {
    let previous = std::panic::take_hook();

    std::panic::set_hook(Box::new(move |info| {
        // Best effort, and each step independent. A panic handler that panics
        // is a second panic inside the first, and on some builds that aborts
        // before anything is written.
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "the panic payload was not a string".to_string());

        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".to_string());

        tracing::error!(%message, %location, "panic");

        if let Err(e) = std::fs::create_dir_all(padcore::paths::logs_dir()) {
            eprintln!("padforge: could not create the log directory: {e}");
        } else {
            let name = format!(
                "crash-{}.txt",
                padcore::report_zip::SystemTimeish::now().stamp()
            );
            let path = padcore::paths::logs_dir().join(&name);
            let body = format!(
                "PadForge {}\n{location}\n\n{message}\n",
                padcore::APP_VERSION
            );
            match std::fs::write(&path, body) {
                Ok(()) => PANIC_DUMPED.store(true, Ordering::Relaxed),
                Err(e) => eprintln!("padforge: could not write the crash file: {e}"),
            }
        }

        // A GUI-subsystem binary has no console, so this line goes nowhere on a
        // normal run. It is here for the case someone is launching from a
        // terminal to get the output.
        eprintln!("padforge: {message}  (at {location})");
        previous(info);
    }));
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
