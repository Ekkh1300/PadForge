//! UI-side state: what the app is editing, independent of the engine.

use std::time::Instant;

use padcore::device::DeviceInfo;
use padcore::gyro::GyroConfig;
use padcore::hotkey::{Hotkey, HotkeyAction, HotkeyBinding, Mods};
use padcore::profile::{Profile, ProfileStore};
use padcore::settings::Settings;

/// Which top-level page is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Dashboard,
    Controller,
    Mapping,
    Gyro,
    Profiles,
    Output,
    Settings,
}

impl Page {
    pub const ALL: &'static [Page] = &[
        Page::Dashboard,
        Page::Controller,
        Page::Mapping,
        Page::Gyro,
        Page::Profiles,
        Page::Output,
        Page::Settings,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Page::Dashboard => "Dashboard",
            Page::Controller => "Controller",
            Page::Mapping => "Mapping",
            Page::Gyro => "Motion",
            Page::Profiles => "Profiles",
            Page::Output => "Output",
            Page::Settings => "Settings",
        }
    }

    /// Short ASCII tag shown before the page name in the nav rail. egui's
    /// built-in fonts cover Latin-1 only, so this stays deliberately plain.
    pub fn tag(self) -> &'static str {
        match self {
            Page::Dashboard => "01",
            Page::Controller => "02",
            Page::Mapping => "03",
            Page::Gyro => "04",
            Page::Profiles => "05",
            Page::Output => "06",
            Page::Settings => "07",
        }
    }
}

/// A toast shown for a few seconds.
#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    pub level: ToastLevel,
    pub born: Instant,
    pub ttl_secs: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastLevel {
    Info,
    Success,
    Warning,
    Error,
}

/// Everything the UI owns.
pub struct AppState {
    pub page: Page,

    /// Settings being edited. Saved when `settings_dirty` is set.
    pub settings: Settings,
    pub settings_dirty: bool,

    /// Profile store being edited.
    pub store: ProfileStore,
    /// Index of the profile currently open in the editors.
    pub editing: usize,

    /// Hotkey rows.
    pub hotkeys: Vec<HotkeyBinding>,
    /// Set while capturing a new combination for a hotkey row.
    pub capturing: Option<usize>,

    /// Gyro config being edited, mirrored from the active profile.
    pub gyro: GyroConfig,

    /// Recent gyro deltas, for the sparkline.
    pub gyro_history: Vec<f32>,

    /// Recent frame intervals, for the latency sparkline.
    pub frame_history: Vec<f32>,

    pub toasts: Vec<Toast>,

    /// Set when the window should close.
    pub quit: bool,
    /// Set when the window should hide to the tray.
    pub minimise_requested: bool,
    /// Set when the tray asks to show the window again.
    pub restore_requested: bool,
    /// True while the window is hidden.
    pub hidden: bool,
    /// Set once a tray icon has been created, or once we know we cannot.
    pub tray_pending: bool,

    /// Profiles changed since the last save.
    pub profiles_dirty: bool,

    /// Cached device list, refreshed on demand.
    pub devices: Vec<DeviceInfo>,
    pub devices_scanned_at: Option<Instant>,

    /// True while dragging the main window, so we skip expensive redraws.
    pub dragging: bool,

    /// Expand/collapse state for collapsible sections.
    pub show_advanced_mapping: bool,
    pub show_gyro_advanced: bool,
    pub show_lightbar: bool,
}

impl AppState {
    pub fn new(settings: Settings, store: ProfileStore) -> Self {
        let gyro = store.active_profile().gyro;
        let hotkeys = default_hotkeys();
        Self {
            page: Page::Dashboard,
            settings,
            settings_dirty: false,
            editing: store.active.min(store.profiles.len().saturating_sub(1)),
            store,
            hotkeys,
            capturing: None,
            gyro,
            gyro_history: Vec::with_capacity(120),
            frame_history: Vec::with_capacity(120),
            toasts: Vec::new(),
            quit: false,
            minimise_requested: false,
            restore_requested: false,
            hidden: false,
            tray_pending: true,
            profiles_dirty: true,
            devices: Vec::new(),
            devices_scanned_at: None,
            dragging: false,
            show_advanced_mapping: false,
            show_gyro_advanced: false,
            show_lightbar: true,
        }
    }

    /// The profile currently being edited.
    pub fn editing_profile(&self) -> &Profile {
        self.store
            .profiles
            .get(self.editing)
            .or_else(|| self.store.profiles.first())
            .expect("store always has at least one profile")
    }

    pub fn editing_profile_mut(&mut self) -> &mut Profile {
        let idx = self
            .editing
            .min(self.store.profiles.len().saturating_sub(1));
        self.editing = idx;
        &mut self.store.profiles[idx]
    }

    pub fn toast(&mut self, text: impl Into<String>, level: ToastLevel) {
        let text = text.into();
        // Replace any existing toast with the same text rather than stacking
        // duplicates, which happens easily with slider drags.
        self.toasts.retain(|t| t.text != text);
        self.toasts.push(Toast {
            text,
            level,
            born: Instant::now(),
            ttl_secs: match level {
                ToastLevel::Error => 8.0,
                ToastLevel::Warning => 6.0,
                _ => 3.5,
            },
        });
    }

    /// Drop expired toasts.
    pub fn expire_toasts(&mut self) {
        self.toasts
            .retain(|t| t.born.elapsed().as_secs_f32() < t.ttl_secs);
    }

    /// Push a value onto a bounded history buffer.
    pub fn push_history(buf: &mut Vec<f32>, value: f32, capacity: usize) {
        if buf.len() >= capacity {
            buf.remove(0);
        }
        buf.push(value);
    }
}

/// A sensible starting hotkey set: cycle profiles, pause, recalibrate.
pub fn default_hotkeys() -> Vec<HotkeyBinding> {
    vec![
        HotkeyBinding::new(
            HotkeyAction::NextProfile,
            Some(Hotkey::new(
                Mods {
                    alt: false,
                    ctrl: true,
                    shift: true,
                    win: false,
                },
                0x7A, // F11
            )),
        ),
        HotkeyBinding::new(
            HotkeyAction::PrevProfile,
            Some(Hotkey::new(
                Mods {
                    alt: false,
                    ctrl: true,
                    shift: true,
                    win: false,
                },
                0x79, // F10
            )),
        ),
        HotkeyBinding::new(
            HotkeyAction::TogglePause,
            Some(Hotkey::new(
                Mods {
                    alt: false,
                    ctrl: true,
                    shift: true,
                    win: false,
                },
                0x7B, // F12
            )),
        ),
        HotkeyBinding::new(HotkeyAction::Recalibrate, None),
    ]
}
