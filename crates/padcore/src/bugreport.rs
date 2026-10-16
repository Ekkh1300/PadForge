//! Collecting a bug report into one file.
//!
//! Everything a maintainer needs to reproduce a report, in one archive, built
//! by one button. The alternative — telling someone to find `%APPDATA%`, work
//! out which log is relevant, right-click, compress and rename — reliably
//! produces two reports per bug: the one they send, and the one they abandon.
//!
//! # Privacy
//!
//! The one field that must not leave the machine is the device serial. A
//! DualShock serial number is a stable hardware identifier, so putting one in a
//! public issue links every bug a person has ever filed to the same device. It
//! is replaced with a marker rather than collected behind an opt-in: an opt-in
//! is one more decision between a frustrated user and a useful report, and the
//! serial is not needed to diagnose anything — what is needed is the *shape* of
//! the device, which the rest of the report keeps.
//!
//! Nothing else in the bundle is personal. Log lines are written by this
//! program; settings and profiles are lists of numbers and control names.

use std::path::PathBuf;

use crate::report_zip::{write_archive, Entry};

/// How much of each log to include.
///
/// A user who leaves PadForge running for a month accumulates a log nobody is
/// going to scroll. The *end* is kept, because that is where a failure is: the
/// thing that stopped working, or never started. 512 KB is a few thousand
/// lines, which is enough to see the run that led to the crash.
const LOG_TAIL_BYTES: usize = 512 * 1024;

/// Outcome of building a report.
pub struct Built {
    pub path: PathBuf,
    pub entries: usize,
    /// Anything that could not be collected, named. Shown to the user rather
    /// than swallowed: a report with no log in it needs to say so, or the
    /// maintainer spends an hour asking for a file that was never included.
    pub missing: Vec<String>,
}

/// Build the archive. `summary` is free-form text the caller adds about this
/// session: what the user was doing when it went wrong.
pub fn build(summary: &str) -> std::io::Result<Built> {
    let mut entries = Vec::new();
    let mut missing = Vec::new();

    entries.push(Entry::text("summary.txt", summary));
    entries.push(Entry::text(
        "what-is-in-here.txt",
        &format!(
            "PadForge {} bug report\n\
             \n\
             summary.txt      what you were doing, written by PadForge\n\
             version.txt      build, Windows version, and which virtual pad is published\n\
             settings.json    your settings, with the device serial removed\n\
             profiles.json    your profiles\n\
             logs/            the end of padforge.log, and any panic dumps\n\
             \n\
             The device serial has been replaced with <removed>. Nothing else\n\
             has been changed. If any of the files above says it could not be\n\
             read, it was not there to read.\n",
            crate::APP_VERSION
        ),
    ));

    match version_note() {
        Ok(text) => entries.push(Entry::text("version.txt", &text)),
        Err(e) => missing.push(format!("version: {e}")),
    }

    match redacted_settings() {
        Ok(text) => entries.push(Entry::text("settings.json", &text)),
        Err(e) => missing.push(format!("settings: {e}")),
    }

    match redacted_profiles() {
        Ok(text) => entries.push(Entry::text("profiles.json", &text)),
        Err(e) => missing.push(format!("profiles: {e}")),
    }

    for (name, text) in logs() {
        entries.push(Entry::text(&name, &text));
    }
    if entries.iter().all(|e| !e.name.starts_with("logs/")) {
        missing.push("logs: none found".to_string());
    }

    let path = crate::report_zip::report_file(crate::report_zip::SystemTimeish::now());
    write_archive(&path, &entries)?;

    Ok(Built {
        path,
        entries: entries.len(),
        missing,
    })
}

/// The settings as JSON, with the serial replaced.
fn redacted_settings() -> std::io::Result<String> {
    let path = crate::paths::settings_file();
    let raw = std::fs::read_to_string(&path)?;
    Ok(strip_serial(&raw))
}

/// The profiles as written, with the serial replaced.
fn redacted_profiles() -> std::io::Result<String> {
    let path = crate::profile::profiles_file();
    let raw = std::fs::read_to_string(&path)?;
    Ok(strip_serial(&raw))
}

/// The text that replaces a serial.
const REDACTED: &str = "<removed: a device serial is a stable hardware identifier, and it is not needed to diagnose anything>";

/// Replace the value of any `serial` field, wherever it appears.
///
/// The document is walked rather than scanned line by line, because the line
/// approach is only correct for one formatting: a compact one-line JSON object
/// — which is exactly what an exported profile looks like — slips straight
/// through it with the serial intact. Walking the structure finds the key at
/// any depth and in any layout.
///
/// The `id` of a profile is *not* a serial and is kept: it is what links an
/// auto-profile rule to a profile, and a report where those do not line up is
/// much harder to read than one with an opaque device name in it.
///
/// A file that does not parse falls back to the line scan, so a report is
/// still produced from a damaged file — and still has the serial taken out of
/// it, which is the one thing that must not be skipped.
fn strip_serial(raw: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(mut value) => {
            redact_value(&mut value);
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| raw.to_string())
        }
        Err(_) => strip_serial_by_line(raw),
    }
}

/// Replace every `serial` key at every depth.
fn redact_value(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                if key.eq_ignore_ascii_case("serial") {
                    *v = serde_json::Value::String(REDACTED.to_string());
                } else {
                    redact_value(v);
                }
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(redact_value),
        _ => {}
    }
}

/// The fallback for a file that will not parse.
fn strip_serial_by_line(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for line in raw.lines() {
        let t = line.trim_start();
        if t.starts_with("\"serial\"") && t.contains(':') {
            let indent = &line[..line.len() - t.len()];
            // Keep the key and replace the value, so a reader can see that the
            // field existed and that something was taken out of it.
            out.push_str(&format!("{indent}\"serial\": \"{REDACTED}\","));
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

fn version_note() -> std::io::Result<String> {
    let os = std::process::Command::new("cmd")
        .args(["/C", "ver"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".into());

    let arch = if cfg!(target_pointer_width = "64") {
        "64-bit"
    } else {
        "32-bit"
    };

    Ok(format!(
        "PadForge {}\nbuilt for {arch}\nOS: {os}\n\
         compiled: {}\nfeatures: {}\n",
        crate::APP_VERSION,
        option_env!("PADFORGE_BUILD_DATE").unwrap_or("not recorded"),
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
    ))
}

/// The logs, newest bytes last, plus any panic dumps.
fn logs() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let dir = crate::paths::logs_dir();

    let Ok(main) = std::fs::read(dir.join("padforge.log")) else {
        return out;
    };
    out.push((
        "logs/padforge.log".to_string(),
        tail(&main).unwrap_or_else(|| String::from_utf8_lossy(&main).into_owned()),
    ));

    // Panic dumps, newest last. A crash writes its own file, so these are the
    // ones a bug report actually needs and the log alone does not contain.
    let mut dumps: Vec<_> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("crash-"))
        .collect();
    // Sorted so the order in the archive matches the order in time. `read_dir`
    // gives no order at all, which makes the archive unreadable when there is
    // more than one.
    dumps.sort_by_key(|e| e.file_name());

    for d in dumps {
        let name = d.file_name().to_string_lossy().to_string();
        if let Ok(bytes) = std::fs::read(d.path()) {
            out.push((
                format!("logs/{name}"),
                tail(&bytes).unwrap_or_else(|| String::from_utf8_lossy(&bytes).into_owned()),
            ));
        }
    }
    out
}

/// The last [`LOG_TAIL_BYTES`], starting at a line boundary.
///
/// Cut mid-line it is still valid text, but it is also the kind of detail that
/// makes someone read the whole thing twice. Trimming to a newline costs
/// nothing.
fn tail(bytes: &[u8]) -> Option<String> {
    if bytes.len() <= LOG_TAIL_BYTES {
        return None;
    }
    let start = bytes.len() - LOG_TAIL_BYTES;
    let slice = &bytes[start..];
    let text = String::from_utf8_lossy(slice);
    match text.find('\n') {
        Some(i) => Some(text[i + 1..].to_string()),
        None => Some(text.into_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_serial_never_reaches_the_archive() {
        // The reason this module exists. If the serial survives redaction it
        // goes into a public issue, and that serial identifies the hardware
        // in every issue the same person ever files.
        let raw = r#"{
  "device": {
    "kind": "serial",
    "serial": "DS4-9f2a7c1e4b8"
  },
  "poll_rate_hz": 250
}"#;
        let out = strip_serial(raw);

        assert!(
            !out.contains("DS4-9f2a7c1e4b8"),
            "the serial leaked into: {out}"
        );
        assert!(
            out.contains("<removed"),
            "and the reader should see it was removed"
        );
        // Everything else untouched — a report with the wrong poll rate in it
        // is worse than no report.
        assert!(out.contains("\"poll_rate_hz\": 250"));
        assert!(
            out.contains("\"kind\": \"serial\""),
            "the shape is still readable"
        );
    }

    #[test]
    fn redaction_covers_the_profiles_too() {
        let raw = r#"{
  "profiles": [
    { "id": "a1b2", "name": "Driving", "serial": "9f2a7c1e", "deadzone": 0.08 }
  ]
}"#;
        let out = strip_serial(raw);
        assert!(
            !out.contains("9f2a7c1e"),
            "the profile serial leaked: {out}"
        );
        assert!(out.contains("a1b2"), "the profile id must survive");
        assert!(out.contains("Driving"));
    }

    #[test]
    fn a_line_that_only_mentions_a_serial_is_left_alone() {
        // Over-redaction costs too. A field like "serial_prefix" or a note that
        // says "no serial" is not the device identifier.
        let raw = "  \"serial_prefix\": \"DS4\",\n  \"note\": \"no serial selected\"\n";
        assert_eq!(strip_serial_by_line(raw), raw);
    }

    #[test]
    fn a_compact_one_line_file_still_loses_its_serial() {
        // The line scan would sail straight past this. It is not a contrived
        // format: an exported profile is compact JSON, and a user who has
        // edited theirs by hand may have compacted it.
        let raw = r#"{"kind":"serial","serial":"9f2a7c1e4b8d0","poll_rate_hz":250}"#;
        let out = strip_serial(raw);
        assert!(!out.contains("9f2a7c1e4b8d0"), "leaked: {out}");
        assert!(out.contains("<removed"));
        assert!(out.contains("250"), "the rest of the file must survive");
    }

    #[test]
    fn a_serial_nested_deep_inside_a_profile_is_found() {
        // Depth is the other thing a text scan gets wrong. Whatever nesting a
        // future version introduces, the walk still finds it.
        let raw = r#"{"a":{"b":{"c":[{"serial":"DEADBEEF","note":"keep me"}]}}}"#;
        let out = strip_serial(raw);
        assert!(!out.contains("DEADBEEF"), "leaked: {out}");
        assert!(out.contains("keep me"));
    }

    #[test]
    fn a_file_too_damaged_to_parse_still_has_the_serial_removed() {
        // This is the case where the structural walk cannot run at all, and it
        // is the one where giving up quietly would be the failure. A report is
        // built from a broken file precisely when the app is misbehaving, so
        // "could not parse, so here it is unredacted" is not an option.
        let raw = "{\n  \"device\": {\n    \"serial\": \"9f2a7c1e4b8d0\",\n    \"kind\": \"serial\"\n  },\n  \"poll\"";
        let out = strip_serial(raw);
        assert!(!out.contains("9f2a7c1e4b8d0"), "leaked: {out}");
        assert!(out.contains("<removed"));
        // The rest of the damage is preserved rather than tidied, so the
        // report shows the maintainer what is actually wrong with the file.
        assert!(out.contains("\"poll\""));
    }

    #[test]
    fn a_long_log_keeps_its_end() {
        // The failure is at the end. A truncated-from-the-front report is the
        // one thing a crash log must never be.
        let mut log = String::new();
        for i in 0..40_000 {
            log.push_str(&format!(
                "2026-10-16 14:32:{:02} INFO line {i} padding padding\n",
                i % 60
            ));
        }
        assert!(log.len() > LOG_TAIL_BYTES);

        let cut = tail(log.as_bytes()).expect("should be trimmed");
        assert!(cut.len() < log.len());
        assert!(cut.contains("line 39999"), "the last line must survive");
        assert!(!cut.contains("line 0 "), "the start must be gone");
        // And it starts on a whole line, not halfway through one.
        assert!(
            cut.starts_with("2026-"),
            "got: {}",
            &cut[..40.min(cut.len())]
        );
    }

    #[test]
    fn a_short_log_is_not_trimmed_at_all() {
        let log = "a short one\n";
        assert_eq!(tail(log.as_bytes()), None);
    }

    #[test]
    fn the_bundle_opens_and_contains_the_things_a_maintainer_asks_for() {
        let dir = std::env::temp_dir().join(format!("padforge-bundle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");

        // Assemble the same entries the builder does, without writing to the
        // user's real Downloads folder during a test.
        let entries = vec![
            Entry::text("summary.txt", "wheel went dead"),
            Entry::text("settings.json", &strip_serial(r#"{"serial":"S3CRET"}"#)),
        ];
        let path = dir.join("report.zip");
        write_archive(&path, &entries).expect("write");

        let out = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
                    path.display(),
                    dir.join("out").display()
                ),
            ])
            .output()
            .expect("powershell");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );

        let settings = std::fs::read_to_string(dir.join("out/settings.json")).expect("read");
        assert!(!settings.contains("S3CRET"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
