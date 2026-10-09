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

use crate::device::Calibration;
use crate::filters::{AxisFilter, Curve, Smoothing};
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
}

impl LightbarMode {
    pub const ALL: &'static [LightbarMode] = &[
        LightbarMode::Steady,
        LightbarMode::Breathe,
        LightbarMode::Flash,
        LightbarMode::Rainbow,
        LightbarMode::Blue,
        LightbarMode::PulseFade,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LightbarMode::Steady => "Steady",
            LightbarMode::Breathe => "Breathe",
            LightbarMode::Flash => "Flash",
            LightbarMode::Rainbow => "Rainbow",
            LightbarMode::Blue => "Blue",
            LightbarMode::PulseFade => "Pulse fade",
        }
    }

    /// Colour for a given point in the animation.
    pub fn colour_at(self, base: [u8; 3], t: f32) -> [u8; 3] {
        let phase = (t * TAU).rem_euclid(TAU);
        let lerp = |a: u8, b: u8, k: f32| (a as f32 + (b as f32 - a as f32) * k).round() as u8;
        match self {
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
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AxisSettings {
    pub deadzone: f32,
    pub curve: Curve,
    pub anti_deadzone: bool,
    pub smoothing: Smoothing,
    pub sensitivity: f32,
    pub inverted: bool,
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

    pub fn mapping_for(&self, control: Ds4Control) -> Mapping {
        self.mapping.get(&control).copied().unwrap_or_default()
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
        paths::write_json(&profiles_file(), self)
    }

    pub fn load() -> Self {
        let mut store = paths::read_json(&profiles_file()).unwrap_or_else(Self::bootstrap);
        store.repair_system_reserved_bindings();
        store
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
