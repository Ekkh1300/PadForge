//! Profiles: the unit of per-game configuration.
//!
//! A profile bundles everything that makes a pad feel right for one game: axis
//! shaping, button remapping, gyro, touchpad, lightbar. Switching profiles
//! swaps the whole bundle in one atomic move, which is what lets auto-profiles
//! change behaviour mid-game without a visible glitch.

use std::collections::BTreeMap;
use std::f32::consts::TAU;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::device::Calibration;
use crate::filters::{AxisFilter, Curve, Smoothing};
use crate::formula::Formula;
use crate::gyro::GyroConfig;
use crate::mapping::{Ds4Control, Mapping, X360Control};
use crate::paths;
use crate::pointer::PointerConfig;
use crate::touchpad::TouchpadConfig;

/// Lightbar behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LightbarConfig {
    pub enabled: bool,
    /// Static colour.
    pub color: [u8; 3],
    /// Brightness 0..=1.
    pub brightness: f32,
    /// Animation, applied on top of `color`.
    pub mode: LightbarMode,
    /// Cycles per second, for flashing modes.
    pub rate: f32,
}

impl Default for LightbarConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            color: [0x00, 0x70, 0xC8],
            brightness: 0.6,
            mode: LightbarMode::Steady,
            rate: 1.5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LightbarMode {
    /// Solid colour.
    Steady,
    /// Pulse between `color` and black.
    Breathe,
    /// Hard blink.
    Flash,
    /// Rainbow sweep.
    Rainbow,
    /// Solid blue, the classic "PS4 is connected" cue.
    Blue,
    /// Fade out to off, for showing that output is paused.
    PulseFade,
    /// The light is off.
    ///
    /// Not the same as a black Steady: a real pad's lightbar draws no current at
    /// all when dark, and on a pad whose shell is light-coloured the difference
    /// between "off" and "black" is visible across a room. It is also the mode
    /// to reach for when the colour itself is the problem -- some players find
    /// any light bar distracting and want it dark without losing the rest of the
    /// profile.
    Off,
}

impl LightbarMode {
    /// Every mode, in the order the picker and the cycle hotkey walk them.
    ///
    /// `Off` is last deliberately. The hotkey advances to the next entry, so
    /// putting it at the end means adding it did not move anything a saved
    /// profile or a muscle memory already refers to.
    pub const ALL: &'static [LightbarMode] = &[
        LightbarMode::Steady,
        LightbarMode::Breathe,
        LightbarMode::Flash,
        LightbarMode::Rainbow,
        LightbarMode::Blue,
        LightbarMode::PulseFade,
        LightbarMode::Off,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LightbarMode::Steady => "Steady",
            LightbarMode::Breathe => "Breathe",
            LightbarMode::Flash => "Flash",
            LightbarMode::Rainbow => "Rainbow",
            LightbarMode::Blue => "Blue",
            LightbarMode::PulseFade => "Pulse fade",
            LightbarMode::Off => "Off",
        }
    }

    /// Does the animation speed change anything for this mode?
    ///
    /// The question the speed slider asks, and the one place it has to be
    /// asked: a mode that holds one colour forever does not care how fast time
    /// passes. This was a list of three modes in the UI, which is how Rainbow
    /// ended up with no speed control at all -- it was left out of a list that
    /// then also doubled as the test for whether to show the slider.
    pub fn animated(self) -> bool {
        !matches!(
            self,
            LightbarMode::Steady | LightbarMode::Blue | LightbarMode::Off
        )
    }

    /// Does this mode build its own colour, ignoring the chosen one?
    ///
    /// Separate from [`Self::animated`] on purpose. Rainbow is animated *and*
    /// builds its own colour, so a colour picker shown for it would suggest a
    /// setting that does nothing -- while a speed slider withheld from it would
    /// hide the one control that does. Conflating the two is what put Rainbow in
    /// the wrong bucket in both directions.
    pub fn ignores_colour(self) -> bool {
        matches!(
            self,
            LightbarMode::Rainbow | LightbarMode::Blue | LightbarMode::Off
        )
    }

    /// Is the light actually on in this mode?
    ///
    /// Not the same question as [`Self::ignores_colour`]. Blue ignores the
    /// chosen colour because it has one of its own; Off ignores it because there
    /// is no light to colour. The UI needs both: the picker is hidden by the
    /// second, and the brightness slider and the live swatch are pointless
    /// wherever this is false.
    pub fn lights_up(self) -> bool {
        self != LightbarMode::Off
    }

    /// Colour for a given point in the animation.
    pub fn colour_at(self, base: [u8; 3], t: f32) -> [u8; 3] {
        let phase = (t * TAU).rem_euclid(TAU);
        let lerp = |a: u8, b: u8, k: f32| (a as f32 + (b as f32 - a as f32) * k).round() as u8;
        match self {
            // Dark, and dark for a reason rather than by accident. Matching on
            // `lights_up` first would read better, but the exhaustive arms below
            // are what keep a mode added later from silently compiling into
            // this one.
            LightbarMode::Off => [0, 0, 0],
            LightbarMode::Steady | LightbarMode::Blue => {
                if self == LightbarMode::Blue {
                    [0x00, 0x70, 0xC8]
                } else {
                    base
                }
            }
            LightbarMode::Breathe => {
                let k = 0.5 + 0.5 * phase.sin();
                [
                    lerp(0, base[0], k),
                    lerp(0, base[1], k),
                    lerp(0, base[2], k),
                ]
            }
            LightbarMode::Flash => {
                if phase.sin() > 0.0 {
                    base
                } else {
                    [0, 0, 0]
                }
            }
            LightbarMode::Rainbow => {
                let h = phase / TAU;
                hsv_to_rgb(h, 1.0, 1.0)
            }
            LightbarMode::PulseFade => {
                let k = 0.5 + 0.5 * phase.sin();
                [
                    lerp(base[0], 0, 1.0 - k),
                    lerp(base[1], 0, 1.0 - k),
                    lerp(base[2], 0, 1.0 - k),
                ]
            }
        }
    }
}

/// HSV (all 0..=1) to RGB bytes.
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [u8; 3] {
    let h = h.rem_euclid(1.0) * 6.0;
    let i = h.floor();
    let f = h - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    let (r, g, b) = match i as u32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (b * 255.0).round() as u8,
    ]
}

/// Per-axis settings, serialisable form of [`AxisFilter`].
///
/// No longer `Copy`, because it holds the formula's source text. The clone only
/// happens where settings are read once per profile rather than per frame, so the
/// cost is not on the hot path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AxisSettings {
    pub deadzone: f32,
    pub curve: Curve,
    pub anti_deadzone: bool,
    pub smoothing: Smoothing,
    pub sensitivity: f32,
    pub inverted: bool,
    /// An expression applied after the curve, replacing `sensitivity`.
    ///
    /// Kept as text so a profile stays readable and hand-editable: `a1 * 2` in the
    /// JSON tells the reader what it does, where a token list would not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula: Option<String>,
}

impl Default for AxisSettings {
    fn default() -> Self {
        Self::from(&AxisFilter::new())
    }
}

impl From<&AxisFilter> for AxisSettings {
    fn from(f: &AxisFilter) -> Self {
        Self {
            deadzone: f.deadzone,
            curve: f.curve,
            anti_deadzone: f.anti_deadzone,
            smoothing: f.smoothing,
            sensitivity: f.sensitivity,
            inverted: f.inverted,
            // A plain filter has no formula; the text only exists in a profile a
            // person edited.
            formula: None,
        }
    }
}

impl AxisSettings {
    pub fn to_filter(&self) -> AxisFilter {
        let mut f = AxisFilter::new();
        f.deadzone = self.deadzone.clamp(0.0, 0.95);
        f.curve = self.curve;
        f.anti_deadzone = self.anti_deadzone;
        f.smoothing = self.smoothing;
        f.sensitivity = self.sensitivity.clamp(0.0, 5.0);
        f.inverted = self.inverted;
        f
    }

    /// The parsed formula, if this axis has a usable one.
    ///
    /// Parsed on demand and nowhere else: the text lives in the profile JSON so
    /// it stays readable and editable, which means something has to turn it into
    /// a form at some point. Doing it here means a bad formula is reported against
    /// the axis the user typed it into.
    ///
    /// The sensitivity field is deliberately left in place when a formula is set.
    /// Applying both would multiply them together, which is almost never what
    /// someone wants and makes the effective gain impossible to reason about, so
    /// the formula replaces the multiplier rather than stacking on it. That is the
    /// same rule x360ce documents.
    pub fn formula(&self) -> Option<Formula> {
        let source = self.formula.as_ref()?;
        match Formula::parse(source) {
            Ok(f) => Some(f),
            Err(e) => {
                warn!("axis formula {source:?} is not usable: {e}");
                None
            }
        }
    }
}

/// The classic PlayStation-style default: Cross = A, Circle = B, Square = X,
/// Triangle = Y, Share = Back, Options = Start.
///
/// The touchpad click is left unbound. See the note below the table.
fn default_mapping() -> BTreeMap<Ds4Control, Mapping> {
    use Ds4Control as D;
    use X360Control as X;
    let pairs: [(D, X); 16] = [
        (D::Cross, X::A),
        (D::Circle, X::B),
        (D::Square, X::X),
        (D::Triangle, X::Y),
        (D::L1, X::LeftBumper),
        (D::R1, X::RightBumper),
        (D::L2, X::LeftTriggerClick),
        (D::R2, X::RightTriggerClick),
        (D::Share, X::Back),
        (D::Options, X::Start),
        (D::L3, X::LeftThumb),
        (D::R3, X::RightThumb),
        (D::DpadUp, X::DpadUp),
        (D::DpadDown, X::DpadDown),
        (D::DpadLeft, X::DpadLeft),
        (D::DpadRight, X::DpadRight),
        // The touchpad click is deliberately NOT bound to Guide.
        //
        // It is the obvious mapping, and it was the default until real hardware
        // showed what it does: Windows reserves the Guide button as a system
        // chord, so pressing it pops the Xbox Game Bar open over whatever the
        // player is doing. The bar then takes focus, and the game loses it. From
        // the pad it looks like the application opened a menu at random.
        //
        // A mapping whose effect is owned by the operating system is not a
        // default worth shipping, even when it is the intuitive answer. Anyone
        // who wants it can bind it in Settings, where the consequence is their
        // choice rather than everyone's.
        //
        // The pad click is left unbound entirely rather than bound to something
        // else: there is no second control on the pad that wants this role, and
        // inventing a target would be a guess about what the user meant.
    ];
    pairs
        .into_iter()
        .map(|(d, x)| (d, Mapping::new(x)))
        .collect()
}

/// A complete configuration bundle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// Stable identifier, so renaming does not break auto-profile rules.
    pub id: String,
    pub name: String,

    pub left_x: AxisSettings,
    pub left_y: AxisSettings,
    pub right_x: AxisSettings,
    pub right_y: AxisSettings,
    pub left_trigger: AxisSettings,
    pub right_trigger: AxisSettings,

    /// Controls in insertion order; the UI reorders by group.
    pub mapping: BTreeMap<Ds4Control, Mapping>,

    pub gyro: GyroConfig,
    pub touchpad: TouchpadConfig,
    /// Gyro/touchpad-to-pointer settings, shared by both paths.
    #[serde(default)]
    pub pointer: PointerConfig,
    pub lightbar: LightbarConfig,

    /// When true, this profile's settings are ignored and the pad passes through.
    pub linked_to: Option<String>,
    /// Per-axis resting offsets captured by the calibration routine.
    #[serde(default)]
    pub calibration: Calibration,

    /// Schema version, for forward compatibility.
    #[serde(default = "schema_version")]
    pub version: u32,
}

fn schema_version() -> u32 {
    1
}

impl Default for Profile {
    fn default() -> Self {
        Self::new()
    }
}

impl Profile {
    pub fn new() -> Self {
        Self {
            id: new_id(),
            name: "Default".into(),
            left_x: AxisSettings::default(),
            left_y: AxisSettings::default(),
            right_x: AxisSettings::default(),
            right_y: AxisSettings::default(),
            left_trigger: AxisSettings {
                // Triggers default to a small deadzone so resting fingers do not
                // register as light pressure.
                deadzone: 0.04,
                ..Default::default()
            },
            right_trigger: AxisSettings {
                deadzone: 0.04,
                ..Default::default()
            },
            mapping: default_mapping(),
            gyro: GyroConfig::default(),
            touchpad: TouchpadConfig::default(),
            pointer: PointerConfig::default(),
            lightbar: LightbarConfig::default(),
            linked_to: None,
            calibration: Calibration::default(),
            version: schema_version(),
        }
    }

    /// A named copy, used by the UI's "duplicate" action.
    pub fn duplicate_with(&self, name: String) -> Self {
        Self {
            id: new_id(),
            name,
            ..self.clone()
        }
    }

    /// The mapping for a control, or a neutral one if it has none.
    ///
    /// Clones rather than copying: `Mapping` holds the formula text, so it is no
    /// longer a plain value. The allocation only happens when a row actually has a
    /// formula, and the alternative is putting the formula behind a reference,
    /// which would complicate the type for no gain at this size.
    pub fn mapping_for(&self, control: Ds4Control) -> Mapping {
        self.mapping.get(&control).cloned().unwrap_or_default()
    }

    pub fn set_mapping(&mut self, control: Ds4Control, mapping: Mapping) {
        self.mapping.insert(control, mapping);
    }

    /// True when this profile forwards to another instead of applying itself.
    pub fn is_link(&self) -> bool {
        self.linked_to.is_some()
    }

    /// Resolve a link chain to the profile that actually holds settings.
    ///
    /// Guards against cycles and unbounded depth; a profile that links to
    /// itself resolves to itself rather than recursing.
    pub fn resolve<'a>(&'a self, store: &'a ProfileStore) -> &'a Profile {
        let mut current = self;
        for _ in 0..8 {
            let Some(next_id) = current.linked_to.as_deref() else {
                return current;
            };
            match store.get(next_id) {
                Some(next) if next.id != current.id => current = next,
                _ => return current,
            }
        }
        current
    }
}

/// Generate a unique-enough profile id.
///
/// Time-based rather than random so no RNG dependency is needed, with a
/// process-local counter to break ties inside the same millisecond.
pub fn new_id() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    format!("{ms:010x}{n:04x}")
}

/// Everything loaded from disk: profiles plus which one is active.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
// See the note on `Settings`: a missing key falls back to the default rather
// than failing the whole parse, so one unrecognised field cannot reset the
// profile list. Individual profiles that still do not fit are dropped by
// `load_with_migration` rather than taking the whole store with them.
#[serde(default)]
pub struct ProfileStore {
    /// All known profiles, ordered as shown in the UI.
    pub profiles: Vec<Profile>,
    /// Index of the active profile.
    #[serde(default)]
    pub active: usize,
    /// Rules that switch profiles based on the foreground process.
    #[serde(default)]
    pub auto_profiles: Vec<AutoProfileRule>,
    /// Name of the profile to fall back to when nothing else matches.
    #[serde(default)]
    pub default_profile: Option<String>,
}

impl ProfileStore {
    /// Build a store containing a single default profile.
    pub fn bootstrap() -> Self {
        Self {
            profiles: vec![Profile::new()],
            active: 0,
            auto_profiles: Vec::new(),
            default_profile: None,
        }
    }

    pub fn active_profile(&self) -> &Profile {
        self.profiles
            .get(self.active)
            .or_else(|| self.profiles.first())
            .expect("store is never empty")
    }

    pub fn active_profile_mut(&mut self) -> &mut Profile {
        let idx = if self.active < self.profiles.len() {
            self.active
        } else {
            0
        };
        self.active = idx;
        &mut self.profiles[idx]
    }

    pub fn get(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.profiles.iter().position(|p| p.id == id)
    }

    /// Add a profile and select it.
    pub fn add(&mut self, profile: Profile) {
        self.profiles.push(profile);
        self.active = self.profiles.len() - 1;
    }

    /// Remove a profile by id. The first profile is never removable, so there is
    /// always something to fall back to.
    pub fn remove(&mut self, id: &str) -> bool {
        if self.profiles.len() <= 1 {
            return false;
        }
        let Some(idx) = self.index_of(id) else {
            return false;
        };
        self.profiles.remove(idx);
        // Drop rules that pointed at the deleted profile.
        self.auto_profiles.retain(|r| r.profile_id != id);
        self.default_profile = self.default_profile.take().filter(|d| d != id);
        if self.active >= self.profiles.len() {
            self.active = self.profiles.len() - 1;
        }
        true
    }

    pub fn rename(&mut self, id: &str, name: String) {
        if let Some(p) = self.profiles.iter_mut().find(|p| p.id == id) {
            p.name = name;
        }
    }

    /// Move to a profile by id, returning false if it does not exist.
    pub fn select(&mut self, id: &str) -> bool {
        match self.index_of(id) {
            Some(i) => {
                self.active = i;
                true
            }
            None => false,
        }
    }

    /// Select by the name a person would type, returning the id.
    ///
    /// For `--profile NAME`, so it has to be forgiving about the things a
    /// person gets wrong: surrounding whitespace, and case. Both are mistakes
    /// rather than intentions — nobody means to pass a profile called
    /// " driving" — and refusing to start over one is a poor trade.
    ///
    /// The error carries the names that do exist, because the overwhelmingly
    /// likely cause is a guess, and the list turns that into a fix.
    pub fn select_by_name(&mut self, name: &str) -> Result<String, Vec<String>> {
        let wanted = name.trim();
        let matches =
            |p: &Profile| p.name.eq_ignore_ascii_case(wanted) || p.id.eq_ignore_ascii_case(wanted);

        if let Some(p) = self.profiles.iter().find(|p| matches(p)) {
            let id = p.id.clone();
            self.select(&id);
            return Ok(id);
        }

        Err(self.profiles.iter().map(|p| p.name.clone()).collect())
    }

    /// Name of the active profile, for `--check` and the status line.
    pub fn active_profile_name(&self) -> String {
        self.profiles
            .get(self.active)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "none".into())
    }

    /// Cycle forward (or backward) through the profile list.
    pub fn cycle(&mut self, delta: isize) {
        if self.profiles.is_empty() {
            return;
        }
        let len = self.profiles.len() as isize;
        let next = (self.active as isize + delta).rem_euclid(len);
        self.active = next as usize;
    }

    /// First rule matching `process` wins.
    pub fn match_process(&self, process: &str) -> Option<&Profile> {
        let needle = process.to_ascii_lowercase();
        self.auto_profiles
            .iter()
            .filter(|r| r.enabled)
            .find(|r| needle.contains(&r.process.to_ascii_lowercase()))
            .and_then(|r| self.get(&r.profile_id))
    }

    pub fn save(&self) -> std::io::Result<()> {
        crate::schema::write(&profiles_file(), self, crate::schema::PROFILE_SCHEMA)
    }

    /// Load the store, migrating the file forward if it is from an older build.
    ///
    /// The second value means "it is safe to write this back"; see
    /// [`crate::schema`] for why a file from a newer build must not be.
    pub fn load_with_migration() -> (Self, bool) {
        let (loaded, safe) = crate::schema::load::<ProfileStore>(
            &profiles_file(),
            crate::schema::PROFILE_SCHEMA,
            "profiles",
        );
        let store = match loaded {
            Some(s) => s,
            None => Self::salvage(),
        };
        let mut store = store;
        // The repair writes, so it has to respect the same rule. On a first run
        // `bootstrap` has nothing to repair and does not save, so `safe` being
        // true there costs nothing.
        if safe {
            store.repair_system_reserved_bindings();
        }
        (store, safe)
    }

    /// Rebuild a store from a file whose *store* would not parse, keeping the
    /// profiles that do.
    ///
    /// `Profile` is deliberately stricter than `Settings`: it has an `id` and a
    /// `name`, and a profile missing either is not a profile — it is a
    /// collision waiting to happen, and two profiles claiming the same id is a
    /// worse bug than losing one.
    ///
    /// Which means one damaged entry in the array fails the whole array, and a
    /// user with nine good profiles loses all nine because of the tenth. So
    /// when that happens, each element is tried on its own and only the ones
    /// that genuinely do not fit are dropped — with their original bytes
    /// written beside the file, so a profile lost to a schema change is still
    /// recoverable by hand.
    fn salvage() -> Self {
        Self::recover_from(&profiles_file())
    }

    /// Build a store from `path`, keeping the profiles that parse.
    ///
    /// Takes the path rather than reading the global one so this is testable
    /// against a scratch directory: writing a salvage file to the developer's
    /// real `%APPDATA%\PadForge\profiles` during a test would be its own bug.
    pub fn recover_from(path: &std::path::Path) -> Self {
        let Ok(raw) = std::fs::read_to_string(path) else {
            return Self::bootstrap();
        };
        let body = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
        let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
            // Not JSON at all. `schema::load` has already copied it aside.
            return Self::bootstrap();
        };

        let items = match value.get("profiles").and_then(|v| v.as_array()) {
            Some(a) => a,
            None => return Self::bootstrap(),
        };

        let mut profiles = Vec::new();
        for item in items {
            match serde_json::from_value::<Profile>(item.clone()) {
                Ok(p) => profiles.push(p),
                Err(e) => {
                    tracing::warn!(
                        name = item.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
                        %e,
                        "this profile does not fit the current schema; the other \
                         profiles were kept and its raw text was saved beside the store"
                    );
                    let name = item
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unnamed");
                    let file = profile_salvage_file(path, name);
                    let _ = std::fs::write(
                        &file,
                        serde_json::to_string_pretty(item).unwrap_or_default(),
                    );
                }
            }
        }

        if profiles.is_empty() {
            return Self::bootstrap();
        }

        let dropped = items.len() - profiles.len();
        let first_id = profiles.first().map(|p| p.id.clone());
        tracing::warn!(
            kept = profiles.len(),
            dropped,
            "recovered a partial profile store"
        );

        Self {
            profiles,
            active: 0,
            auto_profiles: Vec::new(),
            default_profile: first_id,
        }
    }

    pub fn load() -> Self {
        Self::load_with_migration().0
    }

    /// Neutralise bindings that Windows reserves for itself, wherever they came from.
    ///
    /// Changing the default preset is not enough on its own: a profile written by
    /// an earlier version still carries the binding, and loading it verbatim
    /// reproduces exactly the behaviour the default was fixed for. So the repair
    /// runs on load, across every profile.
    ///
    /// The entry is kept and its target set to `None` rather than removed. The UI
    /// lists every control including the unmapped ones, so a control that
    /// vanished from the map would disappear from the mapping page entirely, which
    /// reads as the application having lost a setting.
    ///
    /// Scoped to Guide deliberately. Other XInput buttons have no system chord
    /// behind them, and silently unbinding a control someone chose would be worse
    /// than the problem it solves. Guide is different because the operating system
    /// acts on it whether or not any application is listening for it.
    pub fn repair_system_reserved_bindings(&mut self) {
        let mut changed = false;
        for profile in &mut self.profiles {
            for mapping in profile.mapping.values_mut() {
                if mapping.target == X360Control::Guide {
                    mapping.target = X360Control::None;
                    changed = true;
                }
            }
        }
        if changed {
            let _ = self.save();
        }
    }
}

/// Where the profile store lives on disk.
pub fn profiles_file() -> PathBuf {
    paths::profiles_dir().join("profiles.json")
}

/// Where a single exported profile is written.
pub fn export_file(name: &str) -> PathBuf {
    paths::profiles_dir().join(format!("{name}.json"))
}

/// Where a profile that would not parse is kept.
///
/// Beside the store rather than in a `logs` folder, because the person who
/// needs this file is recovering a profile by hand and will look where they
/// expect. The name is sanitised because `name` comes from the file itself and
/// may contain a path separator or a drive letter — writing it out verbatim
/// would turn a corrupt profile into a file written somewhere else entirely.
fn profile_salvage_file(store: &std::path::Path, name: &str) -> PathBuf {
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(48)
        .collect();
    let stem = if safe.is_empty() { "profile" } else { &safe };
    store
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(format!("{stem}.not-loaded.json"))
}

/// Rule that switches to a profile when a given process is in the foreground.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutoProfileRule {
    pub process: String,
    pub profile_id: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Optional window title substring, for rules that must be stricter.
    #[serde(default)]
    pub window_title: String,
}

fn yes() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_uses_playstation_layout() {
        let p = Profile::new();
        assert_eq!(p.mapping_for(Ds4Control::Cross).target, X360Control::A);
        assert_eq!(p.mapping_for(Ds4Control::Circle).target, X360Control::B);
        assert_eq!(
            p.mapping_for(Ds4Control::Options).target,
            X360Control::Start
        );
    }

    /// The Guide button must not be bound by default.
    ///
    /// Windows reserves it as a system chord, so a binding here makes the pad
    /// pop the Xbox Game Bar open mid-game and steal focus. That was the default
    /// mapping for the touchpad click until real hardware surfaced it, and this
    /// test is the reason it cannot come back unnoticed.
    #[test]
    fn default_profile_leaves_guide_unbound() {
        let p = Profile::new();
        for (control, mapping) in &p.mapping {
            assert_ne!(
                mapping.target,
                X360Control::Guide,
                "{control:?} is bound to Guide, which Windows owns as a system chord"
            );
        }
    }

    /// The same guarantee has to hold for profiles written by an earlier version,
    /// since fixing the default alone leaves every existing profile untouched.
    #[test]
    fn repair_unbinds_guide_from_an_existing_profile() {
        let mut store = ProfileStore::bootstrap();
        let id = store.profiles[0].id.clone();
        store
            .profiles
            .iter_mut()
            .find(|p| p.id == id)
            .expect("the bootstrap profile exists")
            .mapping
            .insert(Ds4Control::TouchpadClick, Mapping::new(X360Control::Guide));

        store.repair_system_reserved_bindings();

        let profile = &store.profiles[0];
        assert!(
            !profile
                .mapping
                .values()
                .any(|m| m.target == X360Control::Guide),
            "the stale Guide binding survived the repair"
        );
        // The entry stays, neutralised: the mapping page lists every control, so
        // a removed entry would vanish from the UI as well as from the profile.
        assert_eq!(
            profile
                .mapping
                .get(&Ds4Control::TouchpadClick)
                .map(|m| m.target),
            Some(X360Control::None),
            "the control should still be listed, just unbound"
        );
        // Everything else has to be left alone, or the repair is just destructive.
        assert_eq!(
            profile.mapping.get(&Ds4Control::Cross).map(|m| m.target),
            Some(X360Control::A),
            "the repair disturbed an unrelated binding"
        );
    }

    /// Regression guard: D-pad Right was silently left unbound, and the touchpad
    /// click was never given a target, so both read as "unmapped" in the UI.
    #[test]
    fn every_dpad_direction_is_bound_by_default() {
        let p = Profile::new();
        for (control, expected) in [
            (Ds4Control::DpadUp, X360Control::DpadUp),
            (Ds4Control::DpadDown, X360Control::DpadDown),
            (Ds4Control::DpadLeft, X360Control::DpadLeft),
            (Ds4Control::DpadRight, X360Control::DpadRight),
        ] {
            assert_eq!(
                p.mapping_for(control).target,
                expected,
                "{control:?} should default to {expected:?}"
            );
        }
    }

    /// Every control the UI shows in a default profile must have a real target,
    /// except the ones that are deliberately open.
    ///
    /// "Deliberately open" is a short list and each entry has a reason:
    ///
    ///   * stick directions and the pseudo-controls are opt-in extras rather than
    ///     part of the standard layout;
    ///   * the touchpad click is unbound because its obvious target, Guide, is
    ///     owned by the operating system as a system chord.
    #[test]
    fn default_profile_has_no_surprising_holes() {
        use Ds4Control as D;
        let p = Profile::new();
        for control in Ds4Control::ALL {
            let deliberately_open = control.is_axis_direction()
                || matches!(control, D::DpadAny | D::TouchpadGesture | D::TouchpadClick);
            let mapped = p.mapping_for(*control).target != X360Control::None;
            if deliberately_open {
                assert!(!mapped, "{control:?} should be unbound by default");
            } else {
                assert!(mapped, "{control:?} should be bound by default");
            }
        }
    }

    #[test]
    fn store_bootstrap_is_valid() {
        let s = ProfileStore::bootstrap();
        assert_eq!(s.profiles.len(), 1);
        assert_eq!(s.active, 0);
        assert!(!s.active_profile().name.is_empty());
    }

    #[test]
    fn removal_keeps_at_least_one_profile() {
        let mut s = ProfileStore::bootstrap();
        let id = s.profiles[0].id.clone();
        assert!(!s.remove(&id), "cannot remove the last profile");
        assert_eq!(s.profiles.len(), 1);
    }

    #[test]
    fn removal_drops_dangling_rules() {
        let mut s = ProfileStore::bootstrap();
        let extra = Profile::new();
        let extra_id = extra.id.clone();
        s.add(extra);
        s.auto_profiles.push(AutoProfileRule {
            process: "game.exe".into(),
            profile_id: extra_id.clone(),
            enabled: true,
            window_title: String::new(),
        });
        assert!(s.remove(&extra_id));
        assert!(s.auto_profiles.is_empty());
    }

    #[test]
    fn cycling_wraps_both_ways() {
        let mut s = ProfileStore::bootstrap();
        s.add(Profile::new());
        s.add(Profile::new());
        s.active = 0;
        s.cycle(1);
        assert_eq!(s.active, 1);
        s.cycle(-1);
        assert_eq!(s.active, 0);
        s.cycle(-1);
        assert_eq!(s.active, 2, "backwards cycle must wrap");
    }

    #[test]
    fn auto_profile_matching_is_substring_based() {
        let mut s = ProfileStore::bootstrap();
        let target = Profile::new();
        let id = target.id.clone();
        s.add(target);
        s.auto_profiles.push(AutoProfileRule {
            process: "cyberpunk".into(),
            profile_id: id.clone(),
            enabled: true,
            window_title: String::new(),
        });
        assert_eq!(
            s.match_process("Cyberpunk2077").map(|p| p.id.clone()),
            Some(id)
        );
        assert!(s.match_process("notepad").is_none());
    }

    #[test]
    fn disabled_rules_are_skipped() {
        let mut s = ProfileStore::bootstrap();
        let target = Profile::new();
        let id = target.id.clone();
        s.add(target);
        s.auto_profiles.push(AutoProfileRule {
            process: "game".into(),
            profile_id: id,
            enabled: false,
            window_title: String::new(),
        });
        assert!(s.match_process("game").is_none());
    }

    #[test]
    fn link_cycle_terminates() {
        let mut s = ProfileStore::bootstrap();
        let a = s.profiles[0].id.clone();
        let mut b = Profile::new();
        b.id = "b".into();
        let b_id = b.id.clone();
        s.add(b);
        // A -> B -> A is a cycle; resolve must still return.
        s.profiles[0].linked_to = Some(b_id.clone());
        let a_profile = s.profiles.iter_mut().find(|p| p.id == a).unwrap();
        a_profile.linked_to = Some(b_id.clone());
        // Resolving a cyclic link must terminate rather than recurse.
        let resolved = s.profiles[0].resolve(&s);
        assert!(!resolved.id.is_empty());
    }

    #[test]
    fn lightbar_rainbow_is_in_range() {
        for i in 0..32 {
            let t = i as f32 / 32.0;
            let c = LightbarMode::Rainbow.colour_at([255, 255, 255], t);
            // Every channel stays in the 0..=255 range a u8 can hold.
            assert!(c.iter().all(|v| (*v as u16) <= 255));
        }
    }

    #[test]
    fn animated_lightbar_modes_actually_move() {
        const BASE: [u8; 3] = [10, 180, 250];
        for mode in LightbarMode::ALL {
            // One whole cycle, sampled finely enough that any animation is
            // caught in the act.
            let samples: Vec<[u8; 3]> = (0..64)
                .map(|i| mode.colour_at(BASE, i as f32 / 64.0))
                .collect();
            let moves = samples.windows(2).any(|w| w[0] != w[1]);
            match mode {
                // These are dark or held by definition.
                LightbarMode::Steady | LightbarMode::Blue | LightbarMode::Off => {
                    assert!(!moves, "{} must not change colour", mode.label());
                }
                // An animation that never changes colour is indistinguishable
                // from Steady, which is exactly how a broken lightbar mode
                // shows up in the hand.
                _ => assert!(moves, "{} never changes colour", mode.label()),
            }
        }
    }

    #[test]
    fn off_is_dark_whatever_the_colour_and_the_speed() {
        // The whole point of the mode: a profile with a red lightbar should go
        // dark on the pad, not go dark-red. Every input is varied because a
        // mode that quietly ignored one of them would still look right in a
        // screenshot taken with the others at their defaults.
        for base in [[255, 0, 0], [0, 0, 255], [10, 180, 250], [0, 0, 0]] {
            for t in [0.0, 0.13, 0.5, 0.77, 0.99] {
                assert_eq!(
                    LightbarMode::Off.colour_at(base, t),
                    [0, 0, 0],
                    "Off lit up with {base:?} at t={t}"
                );
            }
        }

        // And it answers no to every question the UI asks about it, so no
        // control is offered for a light that is not on.
        assert!(!LightbarMode::Off.animated(), "speed would do nothing");
        assert!(
            LightbarMode::Off.ignores_colour(),
            "there is no colour to pick"
        );
        assert!(!LightbarMode::Off.lights_up());
    }

    #[test]
    fn lights_up_agrees_with_whether_the_colour_is_black() {
        // A mode that reports itself as lit while drawing black is a mode the UI
        // will offer a brightness slider for that does nothing, and the slider's
        // own value becomes a lie.
        for mode in LightbarMode::ALL {
            let drawn = mode.colour_at([255, 255, 255], 0.3);
            let black = drawn == [0, 0, 0];
            assert_eq!(
                mode.lights_up(),
                !black,
                "{}: lights_up says {}, but it draws {drawn:?}",
                mode.label(),
                mode.lights_up()
            );
        }
    }

    #[test]
    fn off_is_last_so_adding_it_moved_nothing() {
        // The cycle hotkey advances through ALL, so an insertion in the middle
        // silently re-points a saved profile and anyone's muscle memory. This
        // asserts the position rather than trusting a comment.
        assert_eq!(LightbarMode::ALL.last(), Some(&LightbarMode::Off));
        assert_eq!(LightbarMode::ALL.first(), Some(&LightbarMode::Steady));
        assert_eq!(
            LightbarMode::ALL.len(),
            7,
            "a new mode was added: check that its tests were written too"
        );
    }

    #[test]
    fn an_off_profile_survives_a_round_trip() {
        // Serde is how a mode reaches disk, and `off` is the string written
        // there. A typo in the rename would leave every saved profile refusing
        // to load rather than failing here.
        let p = LightbarConfig {
            mode: LightbarMode::Off,
            ..Default::default()
        };
        let text = serde_json::to_string(&p).expect("serialize");
        assert!(text.contains("\"off\""), "got {text}");
        let back: LightbarConfig = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(back.mode, LightbarMode::Off);
    }

    #[test]
    fn animated_agrees_with_what_colour_at_actually_does() {
        // `animated` is what decides whether the speed slider appears, and
        // `colour_at` is what the light does. If they disagree the slider is
        // either offered for a mode that ignores time or withheld from one
        // that obeys it -- and the second is invisible in the hand, because a
        // Rainbow with no speed control still looks like a Rainbow.
        //
        // This is the test that would have caught the real bug: Rainbow was
        // grouped with Blue as "fixed colour", which hid the one control that
        // matters most on the one mode whose entire character is its speed.
        for mode in LightbarMode::ALL {
            let moves = {
                let samples: Vec<[u8; 3]> = (0..64)
                    .map(|i| mode.colour_at([10, 180, 250], i as f32 / 64.0))
                    .collect();
                samples.windows(2).any(|w| w[0] != w[1])
            };
            assert_eq!(
                mode.animated(),
                moves,
                "{}: animated() says {}, but colour_at {}",
                mode.label(),
                mode.animated(),
                if moves { "moves" } else { "holds still" }
            );
        }
    }

    #[test]
    fn rainbow_ignores_the_chosen_colour_but_not_the_speed() {
        // The two questions have to stay separate. Rainbow sweeps the whole hue
        // circle, so the base colour cannot matter -- and it cannot be offered
        // as though it did. But `rate` scales time, so the sweep's speed is
        // entirely its own.
        let a = LightbarMode::Rainbow.colour_at([255, 0, 0], 0.25);
        let b = LightbarMode::Rainbow.colour_at([0, 0, 255], 0.25);
        assert_eq!(a, b, "Rainbow must ignore the chosen colour");

        assert!(LightbarMode::Rainbow.animated());
        assert!(LightbarMode::Rainbow.ignores_colour());

        // Steady is the opposite: it is the chosen colour, held still.
        assert!(!LightbarMode::Steady.animated());
        assert!(!LightbarMode::Steady.ignores_colour());
        assert_eq!(LightbarMode::Steady.colour_at([1, 2, 3], 0.9), [1, 2, 3]);
    }

    #[test]
    fn axis_settings_round_trip_through_filter() {
        let a = AxisSettings {
            deadzone: 0.15,
            curve: Curve::exponential(2.5),
            sensitivity: 1.4,
            inverted: true,
            ..Default::default()
        };
        let f = a.to_filter();
        assert!((f.deadzone - 0.15).abs() < 1e-6);
        assert!(f.inverted);
        assert!((f.sensitivity - 1.4).abs() < 1e-6);
    }

    #[test]
    fn ids_are_unique() {
        let a = new_id();
        let b = new_id();
        assert_ne!(a, b);
    }

    #[test]
    fn export_path_is_scoped_to_profiles_dir() {
        let p = export_file("My Profile");
        assert!(p.starts_with(paths::profiles_dir()));
        assert!(p.extension().is_some_and(|e| e == "json"));
    }
}
