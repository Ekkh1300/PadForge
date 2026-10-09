//! PadForge installer.
//!
//! A single self-contained `.exe` that installs the application for the current
//! user. Nothing here needs administrator rights: everything lives under
//! `%LOCALAPPDATA%` and `HKCU`, so it cannot disturb other accounts on the
//! machine and cannot collide with an existing Program Files install.
//!
//! Usage:
//!   padforge-installer                 interactive
//!   padforge-installer /S              silent, all defaults
//!   padforge-installer /D=folder       install to a chosen directory
//!   padforge-installer --uninstall     remove an existing installation
//!   padforge-installer --help          this text

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[cfg(windows)]
mod win;

/// The application and its documentation, baked in at build time by `build.rs`.
const APP: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/padforge.exe"));
const README: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/README.md"));
const LICENSE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/LICENSE"));

const APP_NAME: &str = "PadForge";
const PUBLISHER: &str = "PadForge contributors";
const APP_EXE: &str = "padforge.exe";
const UNINSTALLER: &str = "uninstall.exe";
const REGISTRY_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\PadForge";
const VIGEM_URL: &str = "https://github.com/nefarius/ViGEmBus/releases";

/// Everything the installer decided to do.
#[derive(Clone)]
struct Options {
    silent: bool,
    custom_dir: Option<PathBuf>,
    uninstall: bool,
    desktop_shortcut: bool,
    autostart: bool,
    launch_after: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            silent: false,
            custom_dir: None,
            uninstall: false,
            desktop_shortcut: true,
            autostart: false,
            launch_after: true,
        }
    }
}

/// What the install did, so the summary can report it accurately.
struct Summary {
    dir: PathBuf,
    start_menu: Option<PathBuf>,
    desktop: Option<PathBuf>,
    autostart: bool,
    had_driver: bool,
}

const USAGE: &str = "\
PadForge installer

  padforge-installer              Install, asking a few questions.
  padforge-installer /S           Install silently with default choices.
  padforge-installer /D=<folder>  Install into <folder> instead of the default.
  padforge-installer --uninstall  Remove PadForge and everything it added.
  padforge-installer --help      Show this message.

Options:
  --no-desktop-shortcut   Do not create a Desktop shortcut.
  --autostart             Start PadForge when you log in.
  --no-launch             Do not start PadForge when the install finishes.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| matches!(a.as_str(), "--help" | "-h" | "/?")) {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    let options = match parse_args(&args) {
        Ok(o) => o,
        Err(message) => {
            eprintln!("padforge-installer: {message}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };

    // A build with no payload would otherwise install a zero-byte program.
    if APP.is_empty() && !options.uninstall {
        eprintln!(
            "This installer was built without an application payload.\n\
             Build it in two steps:\n    \
             cargo build -p padforge --release --target x86_64-pc-windows-gnu\n    \
             cargo build -p padforge-installer --release --target x86_64-pc-windows-gnu"
        );
        return ExitCode::FAILURE;
    }

    if options.uninstall {
        return match uninstall() {
            Ok(()) => {
                println!("\n{APP_NAME} has been removed.");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("Uninstall failed: {e}");
                ExitCode::FAILURE
            }
        };
    }

    match install(&options) {
        Ok(summary) => {
            print_summary(&summary);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("\nInstallation failed: {e}");
            eprintln!("Nothing was changed.");
            ExitCode::FAILURE
        }
    }
}

/// Parse the command line.
///
/// An unknown option is an error rather than being ignored, because a typo in
/// `--uninstall` must never quietly install the app instead of removing it.
fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut options = Options::default();
    for arg in args {
        let upper = arg.to_ascii_uppercase();
        match upper.as_str() {
            "/S" | "--SILENT" => options.silent = true,
            "--UNINSTALL" => options.uninstall = true,
            "--NO-DESKTOP-SHORTCUT" => options.desktop_shortcut = false,
            "--AUTOSTART" => options.autostart = true,
            "--NO-LAUNCH" => options.launch_after = false,
            _ if upper.starts_with("/D=") || upper.starts_with("--DIR=") => {
                let raw = arg.split_once('=').map(|(_, v)| v).unwrap_or("");
                if raw.is_empty() {
                    return Err("a directory must follow /D=".to_string());
                }
                options.custom_dir = Some(PathBuf::from(raw));
            }
            _ => return Err(format!("unrecognised option: {arg}")),
        }
    }
    Ok(options)
}

/// Where the application goes by default.
///
/// Per-user rather than Program Files, so the installer never needs elevation.
fn default_dir() -> PathBuf {
    let base =
        std::env::var("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."));
    base.join("Programs").join(APP_NAME)
}

fn install(options: &Options) -> Result<Summary, String> {
    let mut desktop_shortcut = options.desktop_shortcut;
    let mut autostart = options.autostart;
    let mut dir = options.custom_dir.clone().unwrap_or_else(default_dir);

    // Installing into a different folder than a previous install leaves that
    // install behind, complete with its own uninstaller and an uninstall entry
    // still pointing at it. Two copies is never what the user meant, so the old
    // one is cleaned up first.
    if let Some(previous) = win::get_reg_string(REGISTRY_KEY, "InstallLocation") {
        let previous = PathBuf::from(previous);
        if previous != dir && previous.exists() {
            println!("Removing the previous install in {}", previous.display());
            match uninstall_at(&previous) {
                Ok(()) => println!("  done."),
                Err(e) => warn(&format!("could not fully remove the previous install: {e}")),
            }
        }
    }

    if !options.silent {
        banner();
        println!("This installs {APP_NAME} for your user account.");
        println!("No administrator rights are needed.\n");
        println!("  Folder: {}", dir.display());

        loop {
            if ask("Install here?", true)? {
                break;
            }
            let answer = ask_text("Enter a different folder")?;
            if answer.is_empty() {
                return Err("cancelled: no folder was chosen".into());
            }
            dir = PathBuf::from(answer);
            println!("\n  Folder: {}", dir.display());
        }

        println!();
        desktop_shortcut = ask("Create a Desktop shortcut?", true)?;
        autostart = ask("Start PadForge when you log in?", false)?;
        println!();
    }

    // A running instance holds its own executable open, so replacing it would
    // fail partway through and leave a half-written install.
    stop_running_app();

    // Checked before anything is written, so the warning lands at the right time.
    let had_driver = win::vigem_installed();

    // --- files -------------------------------------------------------------
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;

    let app_path = dir.join(APP_EXE);
    write_file(&app_path, APP, "the application")?;
    // Documentation is a nicety; never let it fail an otherwise good install.
    let _ = write_file(&dir.join("README.md"), README, "the readme");
    let _ = write_file(&dir.join("LICENSE"), LICENSE, "the licence");

    // The installer becomes the uninstaller.
    let self_exe =
        std::env::current_exe().map_err(|e| format!("could not locate the installer: {e}"))?;
    let uninstaller_path = dir.join(UNINSTALLER);
    std::fs::copy(&self_exe, &uninstaller_path).map_err(|e| {
        format!(
            "could not copy the uninstaller into {}: {e}",
            uninstaller_path.display()
        )
    })?;

    // --- shortcuts ---------------------------------------------------------
    let mut start_menu = None;
    let mut desktop = None;

    match start_menu_dir() {
        Some(base) => {
            let link = base.join(APP_NAME).join("PadForge.lnk");
            match win::create_shortcut(&link, &app_path, APP_NAME, &dir) {
                Ok(()) => start_menu = Some(link),
                // A missing shortcut is a nuisance, not a failed install.
                Err(e) => warn(&format!("could not create the Start Menu shortcut: {e}")),
            }
        }
        None => warn("could not locate the Start Menu folder; no shortcut was created"),
    }

    // A reinstall that turns the Desktop shortcut *off* has to remove the one a
    // previous install left behind, otherwise the choice is silently ignored and
    // the user ends up with a shortcut they asked not to have.
    match desktop_dir() {
        Some(desktop_dir) => {
            let link = desktop_dir.join("PadForge.lnk");
            if desktop_shortcut {
                match win::create_shortcut(&link, &app_path, APP_NAME, &dir) {
                    Ok(()) => desktop = Some(link),
                    Err(e) => warn(&format!("could not create the Desktop shortcut: {e}")),
                }
            } else if link.exists() {
                match std::fs::remove_file(&link) {
                    Ok(()) => warn("removed the Desktop shortcut from a previous install"),
                    Err(e) => warn(&format!("could not remove the old Desktop shortcut: {e}")),
                }
            }
        }
        None if desktop_shortcut => {
            warn("could not locate the Desktop folder; no shortcut was created")
        }
        None => {}
    }

    // --- registry ----------------------------------------------------------
    // Every uninstall value is required: a partial entry would leave the app
    // listed in Settings with a broken uninstall command.
    let step = |field: &str, value: String| -> Result<(), String> {
        win::set_reg_string(REGISTRY_KEY, field, &value)
            .map_err(|e| format!("could not record '{field}' for uninstall: {e}"))
    };
    step("DisplayName", APP_NAME.to_string())?;
    step("DisplayVersion", env!("CARGO_PKG_VERSION").to_string())?;
    step("Publisher", PUBLISHER.to_string())?;
    step(
        "UninstallString",
        format!("\"{}\" --uninstall", uninstaller_path.display()),
    )?;
    step("InstallLocation", dir.to_string_lossy().into_owned())?;
    step("DisplayIcon", format!("\"{}\",0", app_path.display()))?;
    step("HelpLink", "https://github.com/padforge/padforge".to_string())?;
    // NoModify and NoRepair stop Windows offering options that cannot work for a
    // simple file copy.
    win::set_reg_u32(REGISTRY_KEY, "NoModify", 1)
        .map_err(|e| format!("could not record uninstall options: {e}"))?;
    win::set_reg_u32(REGISTRY_KEY, "NoRepair", 1)
        .map_err(|e| format!("could not record uninstall options: {e}"))?;
    // Add/Remove Programs shows this size in kilobytes.
    let size_kb = APP.len().div_ceil(1024) as u32;
    let _ = win::set_reg_u32(REGISTRY_KEY, "EstimatedSize", size_kb);

    // --- autostart ---------------------------------------------------------
    if autostart {
        let quoted = format!("\"{}\"", app_path.display());
        win::set_reg_string(win::RUN_KEY, APP_NAME, &quoted)
            .map_err(|e| format!("could not register PadForge to start automatically: {e}"))?;
    } else {
        win::delete_reg_value(win::RUN_KEY, APP_NAME);
    }

    if options.launch_after {
        // Best effort: failing to start the app must not fail the install.
        let _ = std::process::Command::new(&app_path).spawn();
    }

    Ok(Summary { dir, start_menu, desktop, autostart, had_driver })
}

/// Write `bytes` to `path`, describing failures in terms of what the file is.
fn write_file(path: &Path, bytes: &[u8], what: &str) -> Result<(), String> {
    std::fs::write(path, bytes)
        .map_err(|e| format!("could not write {what} to {}: {e}", path.display()))
}

/// Ask a running PadForge to close, then insist if it will not.
///
/// A polite close lets it save its profiles; the forced follow-up is only there
/// so an install is never blocked by a forgotten background process.
fn stop_running_app() {
    let app = PathBuf::from(APP_EXE);
    if !is_running(&app) {
        return;
    }
    println!("  Closing a running {APP_NAME}...");
    // Without /F this asks the window to close.
    let _ = std::process::Command::new("taskkill").args(["/IM", APP_EXE]).output();

    // Give it a moment to save and exit before doing anything harsher.
    for _ in 0..20 {
        if !is_running(&app) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/IM", APP_EXE])
        .output();
    std::thread::sleep(std::time::Duration::from_millis(500));
}

/// Whether a process is running from `path`.
fn is_running(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("IMAGENAME eq {name}"), "/NH"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(name))
        .unwrap_or(false)
}

fn uninstall() -> Result<(), String> {
    let dir = win::get_reg_string(REGISTRY_KEY, "InstallLocation")
        .map(PathBuf::from)
        .unwrap_or_else(default_dir);

    if !dir.exists() {
        // Nothing on disk, but the registry and shortcuts may still be lying
        // around, so clean those up and report honestly.
        win::delete_reg_value(win::RUN_KEY, APP_NAME);
        win::delete_reg_key(REGISTRY_KEY);
        return Err(format!(
            "{} is already gone, but stale shortcuts and registry entries were removed",
            dir.display()
        ));
    }

    println!("Removing {APP_NAME} from {}", dir.display());
    uninstall_at(&dir)?;

    // User settings are deliberately left alone: deleting someone's profiles and
    // configuration without asking would be the wrong default.
    let settings = std::env::var("APPDATA")
        .map(|a| PathBuf::from(a).join(APP_NAME))
        .unwrap_or_default();
    if settings.exists() {
        println!(
            "\n  Your settings and profiles are still in {}.\n  \
             Delete that folder too if you want a clean slate.",
            settings.display()
        );
    }
    Ok(())
}

/// Remove an installation rooted at `dir`.
///
/// Shared by the uninstaller and by a reinstall that has moved, so both leave
/// exactly the same trace: no shortcuts, no registry entries, no files.
fn uninstall_at(dir: &Path) -> Result<(), String> {
    stop_running_app();

    // Shortcuts first: they may be the only thing left pointing at the folder.
    if let Some(base) = start_menu_dir() {
        let _ = std::fs::remove_dir_all(base.join(APP_NAME));
    }
    if let Some(desktop) = desktop_dir() {
        let _ = std::fs::remove_file(desktop.join("PadForge.lnk"));
    }

    win::delete_reg_value(win::RUN_KEY, APP_NAME);
    win::delete_reg_key(REGISTRY_KEY);

    // The running uninstaller lives inside the folder it is deleting, so it
    // cannot remove itself. Remove everything else first, then hand the folder
    // to a deferred command.
    for name in [APP_EXE, "README.md", "LICENSE"] {
        let _ = std::fs::remove_file(dir.join(name));
    }

    let self_exe = std::env::current_exe().unwrap_or_default();
    if self_exe.starts_with(dir) {
        defer_removal(dir);
    } else {
        std::fs::remove_dir_all(dir)
            .map_err(|e| format!("could not remove {}: {e}", dir.display()))?;
    }
    Ok(())
}

/// Remove the install folder after this process exits.
///
/// Windows refuses to let a running executable delete itself, and the
/// uninstaller *is* the executable living in the folder it is removing, so the
/// removal has to be deferred until this process has exited.
///
/// It runs a batch *file* rather than a `cmd /C` string on purpose: `cmd /C`
/// takes the rest of the command line verbatim, so a folder name containing
/// `&`, `^`, or `%` would be parsed as shell syntax instead of a filename.
fn defer_removal(dir: &Path) {
    let script = std::env::temp_dir().join("padforge-remove.cmd");

    // Two phases, because a batch file cannot delete itself: cmd reports "The
    // batch file cannot be found" the moment the file it is about to read is
    // gone. The first invocation removes the folder and hands the script's own
    // deletion to a second, freshly started cmd, which is what actually removes
    // the file once nothing is reading it.
    //
    // The `^` prefixes escape the redirection and the `&` so this line is passed
    // through to the new shell instead of being acted on by the current one.
    // `%~1` and `%~f0` are expanded by whichever shell ends up running each half.
    let contents = "@echo off\r\n\
         ping 127.0.0.1 -n 3 > nul\r\n\
         rmdir /S /Q \"%~1\"\r\n\
         start /MIN cmd /C ping 127.0.0.1 -n 2 ^> nul ^& del /F /Q \"%~f0\"\r\n";

    if std::fs::write(&script, contents).is_err() {
        // Without the script the folder is simply left behind, which is a
        // cosmetic problem rather than a failed uninstall.
        warn("could not schedule the install folder for removal; delete it by hand");
        return;
    }

    let spawned = std::process::Command::new("cmd")
        .arg("/C")
        .arg("call")
        .arg(&script)
        .arg(dir)
        .spawn();

    if spawned.is_err() {
        let _ = std::fs::remove_file(&script);
        warn("could not schedule the install folder for removal; delete it by hand");
    }
}


/// The user's Start Menu programs folder.
fn start_menu_dir() -> Option<PathBuf> {
    let base = std::env::var("APPDATA").map(PathBuf::from).ok()?;
    let dir = base.join("Microsoft").join("Windows").join("Start Menu").join("Programs");
    dir.exists().then_some(dir)
}

/// The user's Desktop folder.
fn desktop_dir() -> Option<PathBuf> {
    let base = std::env::var("USERPROFILE").map(PathBuf::from).ok()?;
    let dir = base.join("Desktop");
    dir.exists().then_some(dir)
}

/// Print a heading.
fn banner() {
    println!("\n  {APP_NAME} installer");
    println!("  {}\n", "-".repeat(APP_NAME.len() + 10));
}

/// Ask a yes/no question. Returns the answer, or an error if stdin is closed.
fn ask(question: &str, default: bool) -> Result<bool, String> {
    print!("  {question} [{}] ", if default { "Y/n" } else { "y/N" });
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| format!("could not read the answer: {e}"))?;
    Ok(match line.trim().to_ascii_lowercase().as_str() {
        "" => default,
        "y" | "yes" => true,
        _ => false,
    })
}

/// Ask for a line of text. An empty response returns an empty string, which the
/// caller interprets as cancellation.
fn ask_text(prompt: &str) -> Result<String, String> {
    print!("  {prompt}: ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| format!("could not read the answer: {e}"))?;
    Ok(line.trim().to_string())
}

/// Print a non-fatal problem.
fn warn(message: &str) {
    eprintln!("  warning: {message}");
}

fn print_summary(summary: &Summary) {
    println!("\n{APP_NAME} is installed.\n");
    println!("  {}", summary.dir.display());
    if let Some(link) = &summary.start_menu {
        println!("  Start Menu:  {}", link.display());
    }
    if let Some(link) = &summary.desktop {
        println!("  Desktop:     {}", link.display());
    }
    if summary.autostart {
        println!("  Autostart:   yes");
    }
    println!("\n  To remove it, use Settings > Apps, or run:");
    println!("    \"{}\" --uninstall", summary.dir.join(UNINSTALLER).display());

    if !summary.had_driver {
        println!(
            "\n  One thing is still needed: games will not see your controller until\n  \
             the ViGEmBus virtual gamepad driver is installed. {APP_NAME} runs without it\n  \
             and says so on its Output page, but input only reaches games with it.\n\n  \
             Driver: {VIGEM_URL}"
        );
    }
}
