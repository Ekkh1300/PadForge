//! The command line, and the headless mode built on it.
//!
//! Two arguments that are not about starting the app differently:
//!
//! `--profile NAME` starts on that profile instead of the last one used. Useful
//! on its own, and the thing that makes the next one possible.
//!
//! `--check` validates the configuration and exits, without opening a window and
//! without touching the pad. It answers one question — *would this
//! installation actually work?* — which until now could only be answered by
//! launching the app and reading what the dashboard says. That is a poor way to
//! answer it in a script, and an impossible way to answer it on a machine
//! where the answer is "it needs the driver and this account cannot install
//! it".
//!
//! The output is a checklist with a verdict per line and a non-zero exit code,
//! so it is usable from a setup script, a CI job, or a support conversation.
//! It never writes anything, which is the property that makes it safe to run
//! on someone else's machine.

use std::fmt::Write as _;

/// What the command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// Start normally.
    Run { start_hidden: bool },
    /// `--check`: validate and exit.
    Check,
    /// `--help`: print usage and exit.
    Help,
}

pub struct CommandLine {
    pub mode: Mode,
    /// Profile named by `--profile`.
    pub profile: Option<String>,
    /// `--json`, with `--check`: machine-readable output.
    pub json: bool,
    /// Anything we did not recognise, kept so `--check` can say so rather than
    /// silently ignoring a typo'd flag.
    pub unknown: Vec<String>,
}

const USAGE: &str = "\
PadForge - use a DualShock 4 on Windows

USAGE
    padforge [options]

OPTIONS
    --tray, --hidden, --background
            Start with no window, in the notification area.

    --profile <name>
            Start on this profile instead of the one used last.

    --check
            Check the installation and exit. Reports whether the pad, the
            driver and the configuration are all in order. Writes nothing.

    --json
            With --check: print the result as JSON.

    --help, -h
            This text.
";

impl CommandLine {
    pub fn parse(args: &[String]) -> Self {
        let mut mode = Mode::Run {
            start_hidden: false,
        };
        let mut profile = None;
        let mut json = false;
        let mut unknown = Vec::new();

        let mut i = 1;
        while i < args.len() {
            let arg = args[i].as_str();
            match arg {
                "--tray" | "--hidden" | "--background" => mode = Mode::Run { start_hidden: true },
                "--check" => mode = Mode::Check,
                "--json" => json = true,
                "--help" | "-h" | "/?" => mode = Mode::Help,
                "--profile" => match args.get(i + 1) {
                    Some(name) if !name.starts_with('-') => {
                        profile = Some(name.clone());
                        i += 1;
                    }
                    _ => unknown.push("--profile needs a profile name after it".into()),
                },
                other if other.starts_with("--profile=") => {
                    let name = &other["--profile=".len()..];
                    if name.is_empty() {
                        unknown.push("--profile needs a profile name after it".into());
                    } else {
                        profile = Some(name.to_string());
                    }
                }
                other => unknown.push(other.to_string()),
            }
            i += 1;
        }

        Self {
            mode,
            profile,
            json,
            unknown,
        }
    }

    pub fn usage(&self) -> &'static str {
        USAGE
    }
}

/// One line of the verdict.
pub struct Line {
    /// `ok`, `warn`, `fail`, or `skip` for something not applicable.
    pub state: &'static str,
    pub label: &'static str,
    pub detail: String,
}

/// The outcome of `--check`.
pub struct Report {
    pub lines: Vec<Line>,
}

impl Report {
    /// Exit code: 0 when nothing failed, 1 otherwise.
    ///
    /// A warning is not a failure. A missing driver with monitor-only output
    /// chosen is a working installation that does one thing less, and a script
    /// that exits non-zero for it would train everyone to ignore the exit code.
    pub fn exit_code(&self) -> i32 {
        i32::from(self.lines.iter().any(|l| l.state == "fail"))
    }

    fn failed(&self) -> bool {
        self.exit_code() != 0
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        for l in &self.lines {
            let _ = writeln!(out, "[{:<4}] {:<26} {}", l.state, l.label, l.detail);
        }
        out.push('\n');
        out.push_str(if self.failed() {
            "PadForge will not work as configured. The lines marked fail say why."
        } else {
            "PadForge is set up and ready."
        });
        out
    }

    pub fn to_json(&self) -> String {
        let mut out = String::from("{\n  \"checks\": [\n");
        for (i, l) in self.lines.iter().enumerate() {
            let comma = if i + 1 == self.lines.len() { "" } else { "," };
            let _ = writeln!(
                out,
                "    {{ \"state\": \"{}\", \"check\": \"{}\", \"detail\": {} }}{}",
                l.state,
                l.label,
                json_string(&l.detail),
                comma
            );
        }
        let _ = writeln!(out, "  ],\n  \"ok\": {}\n}}", !self.failed());
        out
    }
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Run every check that can be answered without starting the engine.
///
/// Each line is reported independently and none of them stops the rest. A
/// checklist that stops at the first problem is worse than useless here: the
/// person running it is trying to find out *everything* that needs fixing, and
/// making them re-run it once per problem is the opposite of helpful.
pub fn check(
    settings: &padcore::settings::Settings,
    store: &padcore::profile::ProfileStore,
) -> Report {
    let mut lines = Vec::new();
    let mut line = |state: &'static str, label: &'static str, detail: String| {
        lines.push(Line {
            state,
            label,
            detail,
        });
    };

    line(
        "ok",
        "version",
        format!("PadForge {}", padcore::APP_VERSION),
    );

    // --- the pad -----------------------------------------------------------
    let pads = padcore::device::enumerate();
    if pads.is_empty() {
        line(
            "warn",
            "controller",
            "no DualShock 4 is connected. Plug one in, or attach this later.".into(),
        );
    } else {
        line(
            "ok",
            "controller",
            format!(
                "{} pad(s): {}",
                pads.len(),
                pads.iter()
                    .map(|p| p.label())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
    }

    // --- the driver --------------------------------------------------------
    let vigem = padcore::output::VigemBackend::connect_with_slot(None, settings.output_kind);
    match vigem.last_error() {
        None => line(
            "ok",
            "virtual gamepad driver",
            "ViGEmBus is installed and answering".into(),
        ),
        Some(e) => {
            let detail = format!(
                "the virtual gamepad driver is not available ({e}). Install ViGEmBus \
                 from https://github.com/nefarius/ViGEmBus/releases"
            );
            if matches!(
                settings.output_mode,
                padcore::settings::OutputMode::MonitorOnly
            ) {
                line(
                    "warn",
                    "virtual gamepad driver",
                    format!(
                        "{detail}. Monitor-only mode is selected, so the app \
                             still works -- it will read the pad and show it, but no \
                             game will see it."
                    ),
                );
            } else {
                line("fail", "virtual gamepad driver", detail);
            }
        }
    }

    // --- the configuration -------------------------------------------------
    if matches!(
        settings.output_mode,
        padcore::settings::OutputMode::MonitorOnly
    ) {
        line(
            "warn",
            "output mode",
            "monitor-only: the pad is read and displayed but not published, so no \
             game will see it"
                .into(),
        );
    } else {
        line(
            "ok",
            "output mode",
            format!("publishing a virtual {} pad", settings.output_kind.label()),
        );
    }

    if !padcore::settings::POLL_RATES.contains(&settings.poll_rate_hz) {
        line(
            "fail",
            "poll rate",
            format!(
                "{} Hz is not one of the offered rates",
                settings.poll_rate_hz
            ),
        );
    } else {
        line("ok", "poll rate", format!("{} Hz", settings.poll_rate_hz));
    }

    // --- the profiles ------------------------------------------------------
    if store.profiles.is_empty() {
        line(
            "warn",
            "profiles",
            "none exist yet; one will be created on first run".into(),
        );
    } else {
        line(
            "ok",
            "profiles",
            format!(
                "{} profile(s), active: {}",
                store.profiles.len(),
                store.active_profile_name()
            ),
        );
        if store.active >= store.profiles.len() {
            line(
                "fail",
                "active profile",
                format!(
                    "the store points at profile {} but only {} exist",
                    store.active,
                    store.profiles.len()
                ),
            );
        }
        // A rule pointing at a profile that is gone switches to nothing, and
        // the symptom is that the profile never changes -- which looks like the
        // auto-profile feature being broken rather than a stale reference.
        let dangling: Vec<&str> = store
            .auto_profiles
            .iter()
            .filter(|r| store.get(&r.profile_id).is_none())
            .map(|r| r.process.as_str())
            .collect();
        if !dangling.is_empty() {
            line(
                "warn",
                "auto profiles",
                format!("no profile to switch to for: {}", dangling.join(", ")),
            );
        }
    }

    // --- the files ---------------------------------------------------------
    for (label, path) in [
        ("settings file", padcore::paths::settings_file()),
        ("profiles file", padcore::profile::profiles_file()),
    ] {
        match std::fs::metadata(&path) {
            Ok(m) => line(
                "ok",
                label,
                format!("{} ({} bytes)", path.display(), m.len()),
            ),
            Err(e) => line("fail", label, format!("{}: {e}", path.display())),
        }
    }

    if !settings.logging {
        line(
            "warn",
            "log file",
            "logging is off, so a crash would leave nothing to read. Turn it on in \
             Settings, or use Settings > Report a bug."
                .into(),
        );
    }

    Report { lines }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        std::iter::once("padforge".to_string())
            .chain(list.iter().map(|s| (*s).to_string()))
            .collect()
    }

    #[test]
    fn no_arguments_just_runs() {
        let c = CommandLine::parse(&args(&[]));
        assert_eq!(
            c.mode,
            Mode::Run {
                start_hidden: false
            }
        );
        assert!(c.profile.is_none());
        assert!(c.unknown.is_empty(), "{:?}", c.unknown);
    }

    #[test]
    fn hidden_and_check_and_profile_all_parse() {
        assert_eq!(
            CommandLine::parse(&args(&["--tray"])).mode,
            Mode::Run { start_hidden: true }
        );
        assert_eq!(
            CommandLine::parse(&args(&["--background"])).mode,
            Mode::Run { start_hidden: true }
        );
        assert_eq!(CommandLine::parse(&args(&["--check"])).mode, Mode::Check);

        assert_eq!(
            CommandLine::parse(&args(&["--profile", "Driving"]))
                .profile
                .as_deref(),
            Some("Driving")
        );
        // Both spellings, because a script author and a person at a prompt will
        // not agree on which is right.
        assert_eq!(
            CommandLine::parse(&args(&["--profile=Driving"]))
                .profile
                .as_deref(),
            Some("Driving")
        );
    }

    #[test]
    fn a_profile_name_that_looks_like_a_flag_is_refused_rather_than_eaten() {
        // Otherwise `--profile --check` silently becomes "start hidden with no
        // profile", which is the opposite of what was asked for and does not
        // say so.
        let c = CommandLine::parse(&args(&["--profile", "--check"]));
        assert!(c.profile.is_none());
        assert_eq!(c.unknown.len(), 1, "{:?}", c.unknown);
        assert!(c.unknown[0].contains("--profile"));
    }

    #[test]
    fn an_unknown_flag_is_reported_rather_than_ignored() {
        // A typo'd flag that is silently ignored means the user believes
        // something is in effect that is not.
        let c = CommandLine::parse(&args(&["--profiel"]));
        assert_eq!(c.unknown, vec!["--profiel".to_string()]);
        assert!(c.profile.is_none());
    }

    #[test]
    fn a_bare_value_after_an_unknown_flag_is_also_reported() {
        // Both tokens, rather than guessing that `Driving` was meant for
        // `--profiel`. Guessing would hide the real mistake behind a plausible
        // one, and this way the message names exactly what was not understood.
        let c = CommandLine::parse(&args(&["--profiel", "Driving"]));
        assert_eq!(
            c.unknown,
            vec!["--profiel".to_string(), "Driving".to_string()]
        );
        assert!(c.profile.is_none(), "a typo must not select a profile");
    }

    #[test]
    fn check_wins_over_hidden_because_it_does_not_start_the_app() {
        let c = CommandLine::parse(&args(&["--tray", "--check", "--json"]));
        assert_eq!(c.mode, Mode::Check);
        assert!(c.json);
    }

    #[test]
    fn help_is_reachable_from_both_spellings() {
        assert_eq!(CommandLine::parse(&args(&["--help"])).mode, Mode::Help);
        assert_eq!(CommandLine::parse(&args(&["-h"])).mode, Mode::Help);
    }

    #[test]
    fn the_exit_code_follows_the_worst_line() {
        let mk = |states: &[&'static str]| Report {
            lines: states
                .iter()
                .enumerate()
                .map(|(i, s)| Line {
                    state: s,
                    label: "x",
                    detail: format!("{i}"),
                })
                .collect(),
        };

        // A warning is not a failure. Exiting non-zero for one would make every
        // script ignore the exit code entirely, which is the thing an exit code
        // is for.
        assert_eq!(mk(&["ok", "warn", "ok"]).exit_code(), 0);
        assert_eq!(mk(&["ok", "fail"]).exit_code(), 1);
    }

    #[test]
    fn the_json_output_is_parseable() {
        let r = Report {
            lines: vec![
                Line {
                    state: "ok",
                    label: "version",
                    detail: "1.0.1".into(),
                },
                Line {
                    state: "fail",
                    label: "controller",
                    // Quotes and a newline, which is what a real error from
                    // Windows tends to contain.
                    detail: "could not open: \"device\\hid\" (code 5)\nnested".into(),
                },
            ],
        };
        let text = r.to_json();
        let parsed: serde_json::Value =
            serde_json::from_str(&text).expect("the JSON output must parse");
        assert_eq!(parsed["ok"], serde_json::json!(false));
        assert_eq!(parsed["checks"].as_array().expect("array").len(), 2);
        assert_eq!(parsed["checks"][1]["state"], serde_json::json!("fail"));
        assert_eq!(
            parsed["checks"][1]["detail"]
                .as_str()
                .expect("str")
                .lines()
                .count(),
            2
        );
    }

    #[test]
    fn the_plain_output_names_every_failure() {
        let r = Report {
            lines: vec![
                Line {
                    state: "ok",
                    label: "version",
                    detail: "1.0.1".into(),
                },
                Line {
                    state: "fail",
                    label: "driver",
                    detail: "not installed".into(),
                },
            ],
        };
        let text = r.render();
        assert!(text.contains("will not work"));
        assert!(text.contains("driver"));
        assert!(text.contains("not installed"));
    }
}
