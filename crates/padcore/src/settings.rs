//! Global application settings.
//!
//! These are the things that are *not* per-game: which pad to watch, how fast to
//! poll, window behaviour, startup, and so on.

use serde::{Deserialize, Serialize};

use crate::paths;

/// How the lightbar should react when no specific profile colour is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OutputMode {
    /// Publish a virtual Xbox 360 pad.
    #[default]
    Xbox360,
    /// Watch and display only; never touch the virtual bus.
    MonitorOnly,
}

/// What the virtual pad should look like to Windows.
///
/// The choice is not cosmetic. A DualShock 4 is a HID device, so programs that
/// talk to a pad directly — emulators, Steam's own stack, anything that wants
/// the share and options buttons by name — see the shape they expect. An Xbox
/// 360 pad is what almost every game already knows how to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OutputKind {
    /// An Xbox 360 controller, on the XInput bus.
    #[default]
    Xbox360,
    /// A Sony DualShock 4, on the HID bus.
    DualShock4,
}

impl OutputKind {
    pub const ALL: &'static [OutputKind] = &[OutputKind::Xbox360, OutputKind::DualShock4];

    pub fn label(self) -> &'static str {
        match self {
            OutputKind::Xbox360 => "Xbox 360",
            OutputKind::DualShock4 => "DualShock 4",
        }
    }
}

/// Which pad to bind to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeviceSelection {
    /// Whatever connects first.
    #[default]
    Auto,
    /// One specific pad, by serial number.
    Serial(String),
}

impl DeviceSelection {
    pub fn serial(&self) -> Option<&str> {
        match self {
            DeviceSelection::Auto => None,
            DeviceSelection::Serial(s) => Some(s.as_str()),
        }
    }
}

/// Polling rate presets, in Hz. Higher means lower latency and more CPU.
pub const POLL_RATES: &[u32] = &[125, 250, 500, 1000];

/// The four slots Windows gives to XInput.
///
/// These are not a range to be argued with: XInput addresses a controller by a
/// number from 0 to 3 and refuses to talk about any other, so a virtual pad
/// placed outside them would be invisible to every game. The count is named
/// because the choice control and the reporting both need to agree on it.
pub const SLOT_COUNT: usize = 4;

/// Which XInput controller the virtual pad is published as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SlotChoice {
    /// Take whichever free slot the driver has, which is the usual case and the
    /// only choice when there is nothing else to disagree with.
    #[default]
    Auto,
    /// Ask for a specific player, so two machines or two profiles can be told
    /// apart without looking at which slot happened to be free.
    ///
    /// A request is honoured when the slot is free and ignored when it is not.
    /// Claiming it would mean unplugging a controller somebody else is already
    /// using, and a controller that vanishes mid-game is a worse outcome than
    /// a preference that quietly did not apply.
    Player(usize),
}

impl SlotChoice {
    /// The requested slot, or `None` for "whatever is free".
    pub fn index(self) -> Option<usize> {
        match self {
            SlotChoice::Auto => None,
            SlotChoice::Player(i) => Some(i),
        }
    }

    /// One-based player number, for display. `None` for [`SlotChoice::Auto`],
    /// which has no player to number.
    pub fn player_no(self) -> Option<usize> {
        self.index().map(|i| i + 1)
    }

    /// Every choice the UI can offer, in display order.
    pub const ALL: [SlotChoice; SLOT_COUNT + 1] = [
        SlotChoice::Auto,
        SlotChoice::Player(0),
        SlotChoice::Player(1),
        SlotChoice::Player(2),
        SlotChoice::Player(3),
    ];

    /// The label shown on the choice control.
    pub fn label(self) -> String {
        match self {
            SlotChoice::Auto => "Automatic".into(),
            SlotChoice::Player(i) => format!("Player {}", i + 1),
        }
    }
}

/// How a steering wheel is set up, when one is being used.
///
/// This is hardware, not a way of playing, so it lives in the settings and not
/// in a profile: the same pedals are bound to the same targets in every game,
/// and re-learning them per game would be the very work this exists to avoid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct WheelSettings {
    /// Drive the virtual pad from the wheel rather than from the pad.
    ///
    /// Separate from choosing a wheel, because a wheel that is picked and
    /// switched off must stay picked. Losing a binding because the wheel was
    /// unplugged for an evening is the kind of thing that makes an app feel
    /// like it is not listening.
    pub enabled: bool,

    /// The wheel, by the OS path it was when it was chosen.
    ///
    /// Not an index, because an index is an enumeration detail and the list
    /// changes the moment anything is plugged in. The path is what the OS
    /// calls this device, and it stays put while the device does.
    pub device: Option<String>,

    /// What each control on that wheel is asked to do.
    #[serde(default)]
    pub bindings: Vec<crate::wheel::WheelBinding>,
}

impl WheelSettings {
    /// Whether a steering control has been learned yet.
    ///
    /// Worth asking separately from "are there any bindings", because a wheel
    /// with pedals bound but no steering is a setup that will turn badly, and
    /// the interface should say so rather than drive on.
    pub fn has_steering(&self) -> bool {
        self.bindings
            .iter()
            .any(|b| b.target == crate::wheel::WheelTarget::Steer)
    }

    /// Remove whatever is already bound to `target`, so adding a new binding
    /// for it cannot leave two claiming one slot.
    ///
    /// The fold already resolves that by taking the first, so this is not
    /// needed for correctness — it is needed so what the list shows and what
    /// the fold does are the same thing.
    pub fn unbind(&mut self, target: crate::wheel::WheelTarget) {
        self.bindings.retain(|b| b.target != target);
    }

    /// Clamp anything that could have been hand-edited into a value the axis
    /// cannot honour.
    ///
    /// A dead zone of 1.0 is the one worth checking: it silences a control at
    /// every position including full travel, which is not a dead zone but a
    /// disconnection, and it is exactly the sort of thing a hand-edited file
    /// produces.
    fn sanitise(&mut self) {
        for binding in &mut self.bindings {
            binding.deadzone = binding.deadzone.clamp(0.0, 0.95);
            // A zero gain is a control that reports movement and publishes
            // none, which is the same trap as the dead zone above.
            binding.sensitivity = binding.sensitivity.max(0.0);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub device: DeviceSelection,
    pub poll_rate_hz: u32,
    pub output_mode: OutputMode,
    /// What the published pad looks like to Windows.
    ///
    /// Defaulted for settings written before the choice existed, which are the
    /// only settings that can lack it.
    #[serde(default)]
    pub output_kind: OutputKind,
    /// Which XInput controller the virtual pad is published as.
    pub virtual_slot: SlotChoice,

    /// Steering-wheel input, for players who drive rather than play.
    ///
    /// Kept here and not in a profile because a wheel is a piece of hardware,
    /// not a way of playing: the same pedals are bound to the same targets in
    /// every game, and re-learning them per game would be the work this exists
    /// to avoid.
    #[serde(default)]
    pub wheel: WheelSettings,

    /// Stop publishing reports without unloading anything.
    pub paused: bool,

    /// Start hidden in the notification area.
    pub start_minimized: bool,
    /// Keep running after the window is closed.
    pub minimize_on_close: bool,
    /// Add a Run-at-login entry.
    pub autostart: bool,

    /// Poll the foreground process to drive auto-profiles.
    pub auto_profiles_enabled: bool,
    /// How often to check the foreground process, in milliseconds.
    pub auto_profile_interval_ms: u64,

    /// Write a rolling log file.
    pub logging: bool,
    /// Verbose logging.
    pub debug_logging: bool,

    /// Interval for the battery refresh, in seconds.
    pub battery_interval_secs: u64,

    /// Re-apply the active profile when the pad reconnects.
    pub auto_reapply_profile: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            device: DeviceSelection::Auto,
            poll_rate_hz: 250,
            output_mode: OutputMode::Xbox360,
            output_kind: OutputKind::Xbox360,
            virtual_slot: SlotChoice::Auto,
            wheel: WheelSettings::default(),
            paused: false,
            start_minimized: false,
            minimize_on_close: true,
            autostart: false,
            auto_profiles_enabled: true,
            auto_profile_interval_ms: 750,
            logging: true,
            debug_logging: false,
            battery_interval_secs: 30,
            auto_reapply_profile: true,
        }
    }
}

impl Settings {
    /// Clamp anything that could have been hand-edited into an invalid value.
    pub fn sanitised(mut self) -> Self {
        if !POLL_RATES.contains(&self.poll_rate_hz) {
            self.poll_rate_hz = 250;
        }
        self.auto_profile_interval_ms = self.auto_profile_interval_ms.clamp(100, 10_000);
        self.battery_interval_secs = self.battery_interval_secs.clamp(5, 3600);
        // XInput only has four slots. A settings file naming a fifth would ask
        // for something no driver can hand out, so it falls back to taking
        // whatever is free rather than failing to attach at all.
        if matches!(self.virtual_slot, SlotChoice::Player(i) if i >= SLOT_COUNT) {
            self.virtual_slot = SlotChoice::Auto;
        }
        self.wheel.sanitise();
        self
    }

    pub fn load() -> Self {
        paths::read_json(&paths::settings_file())
            .map(Settings::sanitised)
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        paths::write_json(&paths::settings_file(), self)
    }

    /// Human-readable summary of the device binding, for the status bar.
    pub fn device_label(&self) -> String {
        match &self.device {
            DeviceSelection::Auto => "First pad to connect".into(),
            DeviceSelection::Serial(s) => format!("Serial {s}"),
        }
    }
}

#[cfg(target_os = "windows")]
mod autostart {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_FILE_EXISTS, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
        KEY_READ, KEY_WRITE, REG_SZ,
    };

    /// `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
    const SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    const VALUE: &str = "PadForge";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn set(command: &str) -> std::io::Result<()> {
        unsafe {
            let mut key: HKEY = std::ptr::null_mut();
            let subkey = wide(SUBKEY);
            let status = RegOpenKeyExW(
                std::ptr::null_mut(),
                subkey.as_ptr(),
                0,
                KEY_WRITE,
                &mut key,
            );
            if status != ERROR_SUCCESS {
                return Err(std::io::Error::from_raw_os_error(status as i32));
            }
            let name = wide(VALUE);
            let data = wide(command);
            let bytes = (data.len() * std::mem::size_of::<u16>()) as u32;
            let rc = RegSetValueExW(key, name.as_ptr(), 0, REG_SZ, data.as_ptr().cast(), bytes);
            let _ = RegCloseKey(key);
            if rc != ERROR_SUCCESS {
                return Err(std::io::Error::from_raw_os_error(rc as i32));
            }
            Ok(())
        }
    }

    pub fn clear() -> std::io::Result<()> {
        unsafe {
            let mut key: HKEY = std::ptr::null_mut();
            let subkey = wide(SUBKEY);
            let status = RegOpenKeyExW(
                std::ptr::null_mut(),
                subkey.as_ptr(),
                0,
                KEY_WRITE,
                &mut key,
            );
            if status != ERROR_SUCCESS {
                return Err(std::io::Error::from_raw_os_error(status as i32));
            }
            let name = wide(VALUE);
            let rc = RegDeleteValueW(key, name.as_ptr());
            let _ = RegCloseKey(key);
            match rc {
                ERROR_SUCCESS => Ok(()),
                ERROR_FILE_EXISTS => Ok(()),
                other => Err(std::io::Error::from_raw_os_error(other as i32)),
            }
        }
    }

    pub fn is_enabled() -> bool {
        unsafe {
            let mut key: HKEY = std::ptr::null_mut();
            let subkey = wide(SUBKEY);
            let status =
                RegOpenKeyExW(std::ptr::null_mut(), subkey.as_ptr(), 0, KEY_READ, &mut key);
            if status != ERROR_SUCCESS {
                return false;
            }
            let name = wide(VALUE);
            let mut size: u32 = 0;
            let rc = RegQueryValueExW(
                key,
                name.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            );
            let _error = GetLastError();
            let _ = RegCloseKey(key);
            rc == ERROR_SUCCESS && size > 0
        }
    }
}

/// Path of the running executable, quoted for a Run-key command.
#[cfg(target_os = "windows")]
pub fn autostart_command() -> String {
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "padforge.exe".into());
    format!("\"{exe}\" --tray")
}

#[cfg(target_os = "windows")]
pub fn set_autostart(enabled: bool) -> std::io::Result<()> {
    if enabled {
        autostart::set(&autostart_command())
    } else {
        // Clearing an absent entry is not an error worth surfacing.
        match autostart::clear() {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}

#[cfg(target_os = "windows")]
pub fn autostart_enabled() -> bool {
    autostart::is_enabled()
}

#[cfg(not(target_os = "windows"))]
pub fn set_autostart(enabled: bool) -> std::io::Result<()> {
    let _ = enabled;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn autostart_enabled() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let s = Settings::default();
        assert!(POLL_RATES.contains(&s.poll_rate_hz));
        assert!(s.minimize_on_close);
        assert!(s.auto_profiles_enabled);
    }

    #[test]
    fn sanitise_repairs_bad_values() {
        let s = Settings {
            poll_rate_hz: 999,
            auto_profile_interval_ms: 5,
            battery_interval_secs: 0,
            ..Default::default()
        }
        .sanitised();
        assert_eq!(s.poll_rate_hz, 250);
        assert_eq!(s.auto_profile_interval_ms, 100);
        assert_eq!(s.battery_interval_secs, 5);
    }

    #[test]
    fn device_selection_serial_accessor() {
        assert!(DeviceSelection::Auto.serial().is_none());
        let d = DeviceSelection::Serial("abc".into());
        assert_eq!(d.serial(), Some("abc"));
    }

    #[test]
    fn json_round_trip() {
        let s = Settings {
            poll_rate_hz: 500,
            paused: true,
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn a_settings_file_written_before_the_choice_existed_still_loads() {
        // The output kind was added after settings files were already being
        // written, so one of those files has no such key. A missing key is not
        // a corrupt file, and refusing to open it would lock a player out of
        // their own profiles over a preference they never had the chance to
        // set.
        let mut json: serde_json::Value = serde_json::to_value(Settings::default()).unwrap();
        json.as_object_mut().unwrap().remove("output_kind");
        assert!(json.get("output_kind").is_none());

        let back: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(back.output_kind, OutputKind::Xbox360);
    }

    #[test]
    fn the_chosen_output_kind_survives_a_round_trip() {
        let s = Settings {
            output_kind: OutputKind::DualShock4,
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        // Sniffed rather than just re-read: a round trip that passes while the
        // file still says "xbox360" would mean the choice never leaves memory.
        assert!(json.contains("dual_shock4"), "got {json}");
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.output_kind, OutputKind::DualShock4);
    }

    #[test]
    fn every_output_kind_has_a_label() {
        for kind in OutputKind::ALL {
            assert!(!kind.label().is_empty());
        }
        assert_ne!(OutputKind::Xbox360.label(), OutputKind::DualShock4.label());
    }

    #[test]
    fn sanitise_rejects_a_slot_outside_xinput() {
        for i in SLOT_COUNT..12 {
            let s = Settings {
                virtual_slot: SlotChoice::Player(i),
                ..Default::default()
            }
            .sanitised();
            assert_eq!(
                s.virtual_slot,
                SlotChoice::Auto,
                "player {i} does not exist"
            );
        }

        // The four that do exist are left alone.
        for i in 0..SLOT_COUNT {
            let s = Settings {
                virtual_slot: SlotChoice::Player(i),
                ..Default::default()
            }
            .sanitised();
            assert_eq!(s.virtual_slot, SlotChoice::Player(i));
        }
    }

    #[test]
    fn slot_numbers_are_one_based_everywhere_they_are_shown() {
        let s = Settings {
            virtual_slot: SlotChoice::Player(0),
            ..Default::default()
        }
        .sanitised();
        assert_eq!(s.virtual_slot.player_no(), Some(1));
        assert_eq!(s.virtual_slot.label(), "Player 1");

        assert_eq!(SlotChoice::Player(3).label(), "Player 4");
        assert_eq!(SlotChoice::Player(3).player_no(), Some(4));
        // Automatic has no player to number, and must not claim one.
        assert_eq!(SlotChoice::Auto.player_no(), None);
        assert_eq!(SlotChoice::Auto.index(), None);
        assert_eq!(SlotChoice::Auto.label(), "Automatic");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn autostart_command_is_quoted() {
        let cmd = autostart_command();
        assert!(cmd.starts_with('"'), "command must be quoted: {cmd}");
    }

    // -- the wheel -----------------------------------------------------------

    use crate::wheel::{ControlId, WheelBinding, WheelTarget};

    const PEDAL: ControlId = ControlId {
        page: 0x01,
        usage: 0x35,
    };
    const WHEEL_AXIS: ControlId = ControlId {
        page: 0x01,
        usage: 0x38,
    };

    #[test]
    fn wheel_settings_load_in_a_settings_file_that_never_had_them() {
        // Every settings file written before the wheel existed is still a valid
        // settings file, or upgrading costs the player every setting they had.
        //
        // Built from the default rather than typed out, so the check does not
        // quietly stop meaning "a file from before the wheel" the next time a
        // field is added to `Settings`.
        let mut value = serde_json::to_value(Settings::default()).expect("default serialises");
        value
            .as_object_mut()
            .expect("settings are an object")
            .remove("wheel");

        let loaded: Settings = serde_json::from_value(value).expect("older settings still load");
        assert!(!loaded.wheel.enabled);
        assert!(loaded.wheel.device.is_none());
        assert!(loaded.wheel.bindings.is_empty());
    }

    #[test]
    fn a_wheel_survives_a_round_trip_through_the_settings_file() {
        let mut s = Settings::default();
        s.wheel.enabled = true;
        s.wheel.device = Some(r"\\?\hid#vid_046d&pid_c24f".into());
        s.wheel
            .bindings
            .push(WheelBinding::new(WheelTarget::Throttle, PEDAL));

        let json = serde_json::to_string(&s).expect("serialize");
        let back: Settings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.wheel, s.wheel);
    }

    #[test]
    fn a_hand_edited_dead_zone_cannot_silence_a_control() {
        // A dead zone of 1.0 silences the control at every position including
        // full travel. That is a disconnection, not a dead zone, and it is
        // exactly what a hand-edited file produces by accident.
        let mut s = Settings::default();
        let mut binding = WheelBinding::new(WheelTarget::Steer, WHEEL_AXIS);
        binding.deadzone = 1.0;
        s.wheel.bindings.push(binding);
        s.wheel.enabled = true;

        let s = s.sanitised();
        assert!(s.wheel.bindings[0].deadzone <= 0.95);
        // And it must still be usable at full travel, which is the whole point.
        assert!(
            s.wheel.bindings[0].shape(1.0).abs() > 0.9,
            "must still reach full lock"
        );
    }

    #[test]
    fn a_negative_gain_becomes_a_gain_of_zero() {
        // A negative gain would flip the axis, which is what `invert` is for
        // and is never what a file with a minus sign in it meant to say.
        let mut s = Settings::default();
        let mut binding = WheelBinding::new(WheelTarget::Throttle, PEDAL);
        binding.sensitivity = -2.0;
        s.wheel.bindings.push(binding);

        let s = s.sanitised();
        assert_eq!(s.wheel.bindings[0].sensitivity, 0.0);
    }

    #[test]
    fn unbinding_a_target_leaves_the_others_alone() {
        // The learner replaces a target by unbinding it first, so a bug here
        // would quietly discard a pedal the player had already set up.
        let mut w = WheelSettings::default();
        w.bindings
            .push(WheelBinding::new(WheelTarget::Throttle, PEDAL));
        w.bindings
            .push(WheelBinding::new(WheelTarget::Brake, PEDAL));
        w.bindings
            .push(WheelBinding::new(WheelTarget::Steer, WHEEL_AXIS));

        w.unbind(WheelTarget::Brake);
        assert_eq!(w.bindings.len(), 2);
        assert!(w.bindings.iter().all(|b| b.target != WheelTarget::Brake));
        // The other two are untouched, not swept up with it.
        assert!(w.bindings.iter().any(|b| b.target == WheelTarget::Throttle));
        assert!(w.has_steering());
    }

    #[test]
    fn a_setup_without_steering_says_so_rather_than_driving_on() {
        // A wheel with pedals bound and no steering will turn badly, and the
        // only sign would otherwise be a car that will not change direction.
        let mut w = WheelSettings::default();
        assert!(!w.has_steering());
        w.bindings
            .push(WheelBinding::new(WheelTarget::Throttle, PEDAL));
        assert!(!w.has_steering(), "pedals are not steering");
        w.bindings
            .push(WheelBinding::new(WheelTarget::Steer, WHEEL_AXIS));
        assert!(w.has_steering());
    }
}
