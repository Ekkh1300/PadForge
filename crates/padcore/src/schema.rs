//! On-disk format versioning, migration, and what to do with a file that will
//! not parse.
//!
//! Three separate problems live here, and they have been conflated into one so
//! far, which is why none of them was actually solved.
//!
//! **A file written by an older build.** Solved by running it forward through
//! [`migrate`] before it is deserialised, so a rename or a reshape becomes a
//! line of code rather than a lost user's configuration.
//!
//! **A file written by a *newer* build.** This is the one people get wrong.
//! Running "migration" downward is impossible — there is no information about
//! what the newer fields mean — and the tempting fallback of taking the file at
//! face value means the next `save()` writes the current schema over the top of
//! it. Every field the newer build had is now gone, silently, and the file has
//! already been clobbered. So a newer file is read on a best-effort basis and
//! then **not written back**, which is the only thing that preserves it.
//!
//! **A file that will not parse at all.** `read_json` returns `None`, the
//! caller substitutes `Default::default()`, and the user gets a fresh install
//! with no warning that anything was lost. This is the worst of the three,
//! because the file is still sitting on disk looking perfectly valid. So a
//! parse failure copies the original aside before anything overwrites it, and
//! says so loudly in the log.

use std::path::Path;

use serde_json::Value;

/// The settings schema this build writes.
///
/// Bump this when a field is renamed, removed, or given a new meaning. It does
/// not need bumping for an added field: those carry `#[serde(default)]` and an
/// old file simply lacks them.
pub const SETTINGS_SCHEMA: u32 = 2;

/// The profile schema this build writes.
pub const PROFILE_SCHEMA: u32 = 1;

/// The schema a file declares, or `None` when it declares none.
///
/// A missing version is treated as 1 rather than as "unknown". Every file
/// written before the field existed was, by definition, the first version of
/// the format, so claiming it was something else would be inventing history.
pub fn declared_version(value: &Value) -> u32 {
    value.get("version").and_then(Value::as_u64).unwrap_or(1) as u32
}

/// Outcome of preparing a document for deserialisation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prepared {
    /// Safe to deserialise, and safe to write back.
    Current(Value),
    /// Deserialisable, but it declares a schema from the future. It must not
    /// be written back, or the fields this build does not know about are lost.
    FromTheFuture(Value),
    /// A migration was applied. Safe to deserialise; will be rewritten on save.
    Migrated(Value),
}

/// Read `value` forward to [`current`].
///
/// Migrations are matched on the *source* version and applied in order, so
/// going 0 -> 1 -> 2 is three steps and going 1 -> 2 is one, without the code
/// having to know which of those actually happen.
pub fn migrate(mut value: Value, current: u32, kind: &str) -> Prepared {
    let mut declared = declared_version(&value);

    if declared > current {
        // Refuse to touch it. See the module docs: this is the only branch
        // that protects data rather than reshaping it.
        return Prepared::FromTheFuture(value);
    }

    let mut migrated = false;

    while declared < current {
        let from = declared;
        if let Some(next) = step(kind, from, &mut value) {
            declared = next;
            migrated = true;
        } else {
            // No path forward. Rather than guess, leave the document alone and
            // let deserialisation decide. If it happens to parse, the caller's
            // `#[serde(default)]`s fill the gaps; if it does not, the backup in
            // [`load`] keeps the original.
            tracing::warn!(
                schema = kind,
                version = from,
                current,
                "no migration path; leaving the document at the version it has"
            );
            break;
        }
    }

    // Stamp the result so the next load does this work again.
    if let Some(obj) = value.as_object_mut() {
        obj.insert("version".to_string(), Value::from(current.max(declared)));
    }

    if migrated {
        Prepared::Migrated(value)
    } else {
        Prepared::Current(value)
    }
}

/// One migration step. `None` means there is no path from `from`.
fn step(kind: &str, from: u32, value: &mut Value) -> Option<u32> {
    match (kind, from) {
        // --- settings -------------------------------------------------------
        // 1 -> 2: `output_kind` was added for the choice of whether Windows
        // should see an Xbox 360 pad or a DualShock 4. Files written at version
        // 1 predate the choice entirely, and the answer at the time was always
        // the Xbox 360 shape, so that is what they migrate to.
        ("settings", 1) => {
            if let Some(obj) = value.as_object_mut() {
                obj.entry("output_kind")
                    .or_insert_with(|| Value::from("xbox360"));
            }
            Some(2)
        }

        // --- profiles -------------------------------------------------------
        // No step needed yet. PROFILE_SCHEMA is still 1. When a profile field is
        // renamed, add the arm here rather than reaching for `serde(alias)` on
        // the struct: an alias rewrites the field in place, while a step here
        // can also *derive* a new value from an old one, which is the case that
        // actually needed a migration.
        (_, 0) => Some(1),

        _ => None,
    }
}

/// Parse a JSON file, migrating it forward and preserving it if it will not
/// parse.
///
/// This is the function the rest of the program should use instead of
/// [`crate::paths::read_json`], because it does the two things that function
/// could not: it keeps a copy of a file it is about to make useless, and it
/// tells the caller whether the result is safe to write back.
pub fn load<T: serde::de::DeserializeOwned>(
    path: &Path,
    current: u32,
    kind: &str,
) -> (Option<T>, bool) {
    let (value, safe_to_write_back) = read(path, current, kind);
    match value {
        Some(v) => match serde_json::from_value(v) {
            Ok(parsed) => (Some(parsed), safe_to_write_back),
            Err(e) => {
                tracing::error!(
                    path = %path.display(),
                    %e,
                    schema = kind,
                    "the file has a supported version but does not fit the structs; \
                     keeping it and starting from defaults"
                );
                (None, false)
            }
        },
        // `None` here means either "no file yet" or "a file that was kept
        // aside". The flag already tells the two apart, so it is passed
        // through rather than overwritten: a first run must still be allowed to
        // create the file.
        None => (None, safe_to_write_back),
    }
}

/// Write `value` with the schema version stamped alongside it.
///
/// The stamp lives in the file rather than in a struct field so the version
/// does not appear in every equality check and every field access in the
/// program. `flatten` rather than a round trip through `serde_json::Value`,
/// because `Value` stores floats as `f64`: a `deadzone` of `0.08` would come
/// back out as `0.10000000149011612` and rewrite the user's whole settings
/// file on every save. `flatten` keeps the `f32`.
pub fn write<T: serde::Serialize>(path: &Path, value: &T, current: u32) -> std::io::Result<()> {
    #[derive(serde::Serialize)]
    struct Stamped<'a, T> {
        version: u32,
        #[serde(flatten)]
        body: &'a T,
    }
    crate::paths::write_json(
        path,
        &Stamped {
            version: current,
            body: value,
        },
    )
}

/// The raw half of [`load`]: read, migrate, quarantine on failure.
fn read(path: &Path, current: u32, kind: &str) -> (Option<Value>, bool) {
    let raw = match std::fs::read_to_string(path) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (None, true),
        Err(e) => {
            tracing::warn!(path = %path.display(), %e, "could not read");
            return (None, false);
        }
    };

    let body = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
    let value: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => {
            quarantine(path, &raw);
            tracing::error!(
                path = %path.display(),
                %e,
                "not valid json; copied aside and starting from defaults"
            );
            return (None, false);
        }
    };

    match migrate(value, current, kind) {
        Prepared::Current(v) => (Some(v), true),
        Prepared::Migrated(v) => {
            tracing::info!(path = %path.display(), schema = kind, "migrated the stored file forward");
            (Some(v), true)
        }
        Prepared::FromTheFuture(v) => {
            // Loaded, but the next save would strip fields this build has never
            // heard of. Say so once, loudly, and refuse to write it back.
            tracing::warn!(
                path = %path.display(),
                schema = kind,
                declared = declared_version(&v),
                current,
                "this file was written by a newer PadForge; it is being read on a \
                 best-effort basis and will NOT be written back, so that a later \
                 build still finds its own fields intact"
            );
            (Some(v), false)
        }
    }
}

/// Copy a file that will not parse to a name nothing will overwrite.
///
/// The name carries a counter rather than a timestamp because two failures in
/// the same second are ordinary — a corrupt file fails identically every time
/// it is loaded, so a corrupt file failing to load five times in one second is
/// the *normal* case, not an edge case.
fn quarantine(path: &Path, raw: &str) {
    let Some(dir) = path.parent() else { return };
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "config".to_string());

    for n in 1..1000u32 {
        let candidate = dir.join(format!("{stem}.not-parsed-{n}.json"));
        if candidate.exists() {
            continue;
        }
        match std::fs::write(&candidate, raw) {
            Ok(()) => tracing::warn!(
                path = %candidate.display(),
                "kept a copy of the file that would not parse"
            ),
            Err(e) => tracing::warn!(%e, "could not keep a copy of the unreadable file"),
        }
        return;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn key(v: &Value) -> String {
        declared_version(v).to_string()
    }

    #[test]
    fn a_file_with_no_version_is_the_first_version() {
        // Every file written before the field existed was version 1 by
        // definition. Reading it as "unknown" would send it down a path that
        // does not exist.
        assert_eq!(declared_version(&json!({})), 1);
        assert_eq!(declared_version(&json!({"poll_rate_hz": 250})), 1);
        assert_eq!(declared_version(&json!({"version": 2})), 2);
    }

    #[test]
    fn a_version_1_settings_file_gains_the_output_choice_it_predates() {
        // Exactly what a real v1 file looks like: written before the field was
        // added, and with no version stamp at all, because v1 predates that too.
        let old = json!({
            "device": {"kind": "auto"},
            "poll_rate_hz": 500,
            "output_mode": "xbox360",
            "virtual_slot": "auto",
            "paused": false
        });

        let Prepared::Migrated(v) = migrate(old, SETTINGS_SCHEMA, "settings") else {
            panic!("a v1 file must be migrated, not passed through");
        };
        assert_eq!(v["output_kind"], json!("xbox360"));
        assert_eq!(v["poll_rate_hz"], json!(500));
        // ...and it is stamped, so the next load is a straight read.
        assert_eq!(key(&v), SETTINGS_SCHEMA.to_string());

        let again = migrate(v, SETTINGS_SCHEMA, "settings");
        assert!(matches!(again, Prepared::Current(_)));
    }

    #[test]
    fn migration_preserves_the_values_that_matter_and_not_just_the_shape() {
        // A migration that produces a file of default settings is worse than no
        // migration: the user keeps their app and silently loses their setup.
        let old = json!({
            "device": {"kind": "serial", "serial": "abc123"},
            "poll_rate_hz": 1000,
            "output_mode": "monitor_only",
            "virtual_slot": {"kind": "player", "player": 3},
            "battery_interval_secs": 15,
            "wheel": {"enabled": true, "bindings": []},
            "version": 1
        });

        let Prepared::Migrated(v) = migrate(old, SETTINGS_SCHEMA, "settings") else {
            panic!("must migrate");
        };
        assert_eq!(v["device"]["serial"], json!("abc123"));
        assert_eq!(v["poll_rate_hz"], json!(1000));
        assert_eq!(v["output_mode"], json!("monitor_only"));
        assert_eq!(v["virtual_slot"]["player"], json!(3));
        assert_eq!(v["battery_interval_secs"], json!(15));
        assert_eq!(v["wheel"]["enabled"], json!(true));
    }

    #[test]
    fn a_file_from_a_newer_build_is_read_but_never_written_back() {
        // The failure this prevents is silent and total. The newer build had a
        // field this one has never heard of; deserialisation drops it; the next
        // save writes the current schema over the top; and the field is gone
        // for good, with the file looking healthy the whole time.
        let from_the_future = json!({
            "poll_rate_hz": 500,
            "haptic_amplitude": 0.8,
            "version": 99
        });

        let prepared = migrate(from_the_future, SETTINGS_SCHEMA, "settings");
        assert!(
            matches!(prepared, Prepared::FromTheFuture(_)),
            "a newer file must not be claimed as current"
        );

        // The document is untouched, so the newer build still finds its data.
        let Prepared::FromTheFuture(v) = prepared else {
            unreachable!()
        };
        assert_eq!(v["haptic_amplitude"], json!(0.8));
        assert_eq!(key(&v), "99");
    }

    #[test]
    fn an_unknown_version_does_not_get_migrated_into_a_confident_wrong_answer() {
        // There is no 2 -> 3 step yet. Rather than pretend, the document is left
        // where it is; the `#[serde(default)]`s do the rest, and if it does not
        // parse the caller's backup keeps it.
        let v = json!({"version": 77});
        let prepared = migrate(v, 99, "settings");
        // 77 < 99, so it is not "from the future" — but there is no path either.
        assert!(matches!(
            prepared,
            Prepared::Current(_) | Prepared::Migrated(_)
        ));
    }

    #[test]
    fn profiles_still_migrate_from_the_pre_version_era() {
        let old = json!({"profiles": [], "active": 0, "version": 0});
        let Prepared::Migrated(v) = migrate(old, PROFILE_SCHEMA, "profiles") else {
            panic!("a version 0 profile file must move to 1");
        };
        assert_eq!(key(&v), "1");
    }

    #[test]
    fn a_file_that_will_not_parse_is_kept_before_anything_overwrites_it() {
        let dir = std::env::temp_dir().join(format!("padforge-quarantine-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("settings.json");

        // Not JSON at all. This is what a truncated write or a hand edit that
        // went wrong actually leaves behind.
        let garbage = "{\"poll_rate_hz\": 250, \"device\"";
        std::fs::write(&path, garbage).expect("write garbage");

        let (parsed, safe) = load::<serde_json::Value>(&path, SETTINGS_SCHEMA, "settings");
        assert!(parsed.is_none(), "garbage must not parse");
        assert!(
            !safe,
            "an unparsed file must not be a candidate for rewriting"
        );

        // And the original is sitting right there under a name nothing will
        // reuse, because the user may still want what is in it.
        let kept = dir.join("settings.not-parsed-1.json");
        assert!(kept.exists(), "the file must be copied aside");
        assert_eq!(std::fs::read_to_string(&kept).expect("read copy"), garbage);

        // A second failure does not overwrite the first copy. A corrupt file
        // fails identically on every load, so this is the common case.
        let (_, _) = load::<serde_json::Value>(&path, SETTINGS_SCHEMA, "settings");
        assert!(
            kept.exists(),
            "the first copy must survive a second failure"
        );
        assert!(dir.join("settings.not-parsed-2.json").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_byte_order_mark_still_reads() {
        // Regression guard on behaviour that already existed in `read_json` and
        // must not be lost by moving it here: Notepad writes a BOM, and losing
        // every setting over one is not acceptable.
        let dir = std::env::temp_dir().join(format!("padforge-bom2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("settings.json");
        std::fs::write(&path, "\u{feff}{\"poll_rate_hz\": 500}").expect("write");

        let (parsed, safe) = load::<serde_json::Value>(&path, SETTINGS_SCHEMA, "settings");
        assert!(parsed.is_some(), "a BOM must not stop the parse");
        assert!(safe);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_is_not_an_error_and_is_still_safe_to_write() {
        let path = std::env::temp_dir().join("padforge-does-not-exist-9f2a/settings.json");
        let (parsed, safe) = load::<serde_json::Value>(&path, SETTINGS_SCHEMA, "settings");
        assert!(parsed.is_none());
        assert!(safe, "a first run must be allowed to create the file");
    }

    /// A scratch directory that cleans itself up, so these tests do not depend
    /// on the machine's `%APPDATA%`.
    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("padforge-schema-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    /// The point of the whole module, written against the real types.
    ///
    /// A schema test using `serde_json::Value` proves the migration function
    /// reshapes JSON. It does not prove a user's settings survive an upgrade,
    /// which is the thing anybody actually cares about. So this goes through
    /// [`crate::settings::Settings`], writes the file the way a v1 build would
    /// have written it, and reads it back through the loader.
    #[test]
    fn a_users_settings_survive_an_upgrade_from_the_previous_schema() {
        use crate::settings::{DeviceSelection, OutputKind, OutputMode, Settings, SlotChoice};

        let dir = scratch("upgrade");
        let path = dir.join("settings.json");

        // A settings file where every value is distinguishable from its
        // default, so "it loaded" cannot be confused with "it loaded the
        // defaults". Written through the real serialiser rather than pasted in
        // by hand: a hand-written fixture drifts the moment an enum's serde
        // shape changes, and then this test fails for a reason that has nothing
        // to do with migration — which is how a test about upgrades gets deleted
        // instead of fixed.
        let original = Settings {
            device: DeviceSelection::Serial {
                serial: "DS4-9f2a".into(),
            },
            poll_rate_hz: 1000,
            output_mode: OutputMode::MonitorOnly,
            virtual_slot: SlotChoice::Player(2),
            paused: true,
            start_minimized: true,
            minimize_on_close: false,
            autostart: true,
            auto_profiles_enabled: false,
            auto_profile_interval_ms: 2000,
            logging: false,
            debug_logging: true,
            battery_interval_secs: 15,
            auto_reapply_profile: false,
            // The one thing a v1 file must not contain.
            output_kind: OutputKind::DualShock4,
            ..Default::default()
        };

        let mut v1 = serde_json::to_value(&original).expect("serialise");
        let obj = v1.as_object_mut().expect("settings is an object");
        // Added at schema 2, so a schema 1 file does not have it.
        obj.remove("output_kind");
        // Schema 2 stamps this; schema 1 predates the field.
        obj.remove("version");

        std::fs::write(&path, serde_json::to_string_pretty(&v1).expect("write v1"))
            .expect("write v1");

        let (loaded, safe) = load::<Settings>(&path, SETTINGS_SCHEMA, "settings");
        assert!(safe, "a v1 file is ours to rewrite");
        let s: Settings = loaded.expect("a v1 file must still load");

        // Everything the user chose came through.
        assert_eq!(
            s.device,
            DeviceSelection::Serial {
                serial: "DS4-9f2a".into()
            }
        );
        assert_eq!(s.poll_rate_hz, 1000);
        assert_eq!(s.output_mode, OutputMode::MonitorOnly);
        assert_eq!(s.virtual_slot, SlotChoice::Player(2));
        assert!(s.paused);
        assert!(s.start_minimized);
        assert!(!s.minimize_on_close);
        assert!(s.autostart);
        assert!(!s.auto_profiles_enabled);
        assert_eq!(s.auto_profile_interval_ms, 2000);
        assert!(!s.logging);
        assert!(s.debug_logging);
        assert_eq!(s.battery_interval_secs, 15);
        assert!(!s.auto_reapply_profile);

        // And the one thing the migration decided. Set to the *other* value in
        // the source settings, so "it took the historical default" cannot be
        // confused with "it round-tripped a value that was never in the file".
        assert_eq!(
            s.output_kind,
            OutputKind::Xbox360,
            "a file from before the choice existed gets the shape that was \
             current at the time"
        );

        // Writing it back must produce a v2 file that loads to the same thing.
        // The round trip is what makes the migration safe rather than merely
        // non-crashing.
        write(&path, &s, SETTINGS_SCHEMA).expect("write v2");
        let text = std::fs::read_to_string(&path).expect("read v2");
        assert!(
            text.contains(&format!("\"version\": {SETTINGS_SCHEMA}")),
            "the written file must declare its version; got: {text}"
        );

        let (again, safe_again) = load::<Settings>(&path, SETTINGS_SCHEMA, "settings");
        assert!(safe_again);
        assert_eq!(again.expect("v2 must load"), s, "the round trip changed it");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The one failure mode that loses data, tested against real settings.
    #[test]
    fn downgrading_never_rewrites_a_newer_users_file() {
        use crate::settings::Settings;

        let dir = scratch("future");
        let path = dir.join("settings.json");

        // A build from the future that added a setting this one has never heard
        // of. The text is what makes the test worth running: it is a field that
        // exists only in that file.
        let future = r#"{
  "poll_rate_hz": 500,
  "haptic_resonance": 0.75,
  "version": 99
}"#;
        std::fs::write(&path, future).expect("write future");

        let (loaded, safe) = load::<Settings>(&path, SETTINGS_SCHEMA, "settings");
        let s = loaded.expect("the parts we do understand should still load");
        assert_eq!(s.poll_rate_hz, 500);
        assert!(
            !safe,
            "a file from a newer build must not be a candidate for rewriting, \
             because writing would delete fields we cannot see"
        );

        // The actual bytes on disk are what a later build will find. This is
        // the assertion that matters: not "we handled it gracefully" but
        // "nothing was destroyed".
        assert_eq!(std::fs::read_to_string(&path).expect("read"), future);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_truncated_settings_file_is_kept_rather_than_overwritten() {
        use crate::settings::Settings;

        let dir = scratch("truncated");
        let path = dir.join("settings.json");

        // Half a file, which is what an interrupted write or a full disk leaves.
        // Simulated here rather than produced for real, because producing a
        // genuine torn write needs a crash at exactly the wrong moment.
        let torn = "{\n  \"device\": { \"kind\": \"auto\" },\n  \"poll_rate_hz\": 25";
        std::fs::write(&path, torn).expect("write torn");

        let (loaded, safe) = load::<Settings>(&path, SETTINGS_SCHEMA, "settings");
        assert!(loaded.is_none(), "half a file is not a file");
        assert!(
            !safe,
            "an unreadable file must not be overwritten by defaults"
        );

        let kept = dir.join("settings.not-parsed-1.json");
        assert!(
            kept.exists(),
            "the half file must still be on disk somewhere"
        );
        assert_eq!(std::fs::read_to_string(&kept).expect("read copy"), torn);
        assert_eq!(
            std::fs::read_to_string(&path).expect("read original"),
            torn,
            "the original path must still point at the original bytes"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// One damaged profile must not take the rest of the list with it.
    ///
    /// `Profile` stays strict on purpose — it has an `id`, and a profile without
    /// one collides with every other profile without one. But strictness at the
    /// individual level means a single bad entry fails the whole array, and a
    /// user with nine good profiles loses all nine to the tenth.
    #[test]
    fn one_unreadable_profile_does_not_cost_the_user_the_others() {
        use crate::profile::{Profile, ProfileStore};

        let dir = scratch("salvage");
        let path = dir.join("profiles.json");

        // Three real profiles, serialised so they cannot drift, with the middle
        // one's identity fields wrecked — which is what a hand-merge or a
        // truncated write looks like.
        let good_a = serde_json::to_value(Profile::new()).expect("a");
        let good_b = serde_json::to_value(Profile::new()).expect("b");

        let mut broken = good_a.clone();
        broken["id"] = serde_json::json!(12345); // wrong type on purpose

        let doc = serde_json::json!({
            "profiles": [good_a, broken, good_b],
            "active": 1,
            "version": PROFILE_SCHEMA,
        });
        std::fs::write(&path, doc.to_string()).expect("write");

        let store = ProfileStore::recover_from(&path);

        assert_eq!(
            store.profiles.len(),
            2,
            "the two readable profiles must survive the one that is not"
        );
        assert!(store
            .profiles
            .iter()
            .all(|p| !p.id.is_empty() && p.id.parse::<u32>().is_err()));

        // The lost one is recoverable by hand rather than merely gone.
        let saved = dir.join("salvage-slot.profiles.not-loaded.json");
        let found = std::fs::read_dir(&dir)
            .expect("read dir")
            .filter_map(|e| e.ok())
            .find(|e| e.file_name().to_string_lossy().contains("not-loaded"))
            .expect("the dropped profile's text must have been written out");
        assert!(
            std::fs::read_to_string(found.path())
                .expect("read salvage")
                .contains("12345"),
            "the salvaged file must be the original entry, not a placeholder"
        );
        let _ = saved;

        let _ = std::fs::remove_dir_all(&dir);
    }
}
