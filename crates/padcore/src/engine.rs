//! The engine: one thread that owns the pad, the virtual bus, and the pipeline.
//!
//! ```text
//!   HID thread          engine thread                     UI thread
//!  +-----------+       +------------------------+       +------------+
//!  | decode    +------>| filter -> map -> output +------>| telemetry  |
//!  | report    | frame | gyro, touchpad, LED    | event | commands   |
//!  +-----------+       +------------------------+<------+            |
//!                                                                +------------+
//! ```
//!
//! The engine never blocks on the UI: telemetry goes out over a bounded channel
//! with `try_send`, so a slow frame drops a visual update rather than stalling
//! input.

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tracing::{debug, info, warn};

use crate::device::{self, Calibration, DeviceReader, DeviceSnapshot};
use crate::filters::{AxisFilter, Curve};
use crate::formula::{Formula, Inputs};
use crate::gyro::{GyroConfig, GyroOutputMode, GyroProcessor, GyroSmoothing};
use crate::hotkey::{HotkeyAction, HotkeyBinding, HotkeyManager};
use crate::mapping::{Ds4Control, X360Control};
use crate::output::{GamepadState, NullBackend, OutputBackend, XButtons};
use crate::pointer::{self, GyroPointer, MouseButton, TouchpadPointer};
use crate::process;
use crate::profile::{LightbarConfig, LightbarMode, Profile, ProfileStore};
use crate::report::{self, Ds4Report, Transport};
use crate::settings::{DeviceSelection, OutputMode, Settings};
use crate::touchpad::{TouchpadMode, TouchpadProcessor};

/// Telemetry publish rate. Faster than this and the UI cannot keep up anyway.
const PUBLISH_INTERVAL: Duration = Duration::from_millis(16);

/// What the UI observes. Cheap to clone, sent roughly every frame.
#[derive(Debug, Clone)]
pub struct Telemetry {
    pub connected: bool,
    pub device_label: String,
    pub transport: Option<Transport>,
    pub battery: report::Battery,
    /// Live pad state, after calibration but before the filter chain.
    pub report: Ds4Report,
    /// What actually reaches the OS.
    pub output: GamepadState,
    /// DS4 controls currently held that are mapped somewhere.
    pub active_controls: Vec<&'static str>,
    pub output_backend: String,
    pub output_connected: bool,
    pub profile_name: String,
    pub profile_id: String,
    pub packets: u64,
    pub frame_ms: f32,
    pub paused: bool,
    pub lightbar: [u8; 3],
    pub auto_profile_matched: Option<String>,
    pub gyro_delta: (f32, f32),
    /// The last pointer delta injected, in pixels. Zero means nothing moved.
    pub pointer_delta: (i32, i32),
    /// Why pointer injection failed, if it did.
    pub pointer_error: Option<String>,
    /// True when gyro is enabled in the active profile.
    pub gyro_enabled: bool,
    /// Flattened gyro smoothing parameters, so the UI does not have to re-derive
    /// them from the config enum on every frame.
    pub gyro_smoothing: GyroSmoothingParams,
    pub calibration: Calibration,
    pub vigem_error: Option<String>,
}

/// The smoothing knobs, unpacked from [`GyroSmoothing`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GyroSmoothingParams {
    pub kind: GyroSmoothingKind,
    pub min_cutoff: f32,
    pub beta: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GyroSmoothingKind {
    None,
    LowPass,
    OneEuro,
}

impl Default for Telemetry {
    fn default() -> Self {
        Self {
            connected: false,
            device_label: String::new(),
            transport: None,
            battery: report::Battery::default(),
            report: Ds4Report::neutral(),
            output: GamepadState::neutral(),
            active_controls: Vec::new(),
            output_backend: "starting".into(),
            output_connected: false,
            profile_name: String::new(),
            profile_id: String::new(),
            packets: 0,
            frame_ms: 0.0,
            paused: false,
            lightbar: [0, 0, 0],
            auto_profile_matched: None,
            gyro_delta: (0.0, 0.0),
            pointer_delta: (0, 0),
            pointer_error: None,
            gyro_enabled: false,
            gyro_smoothing: GyroSmoothingParams {
                kind: GyroSmoothingKind::None,
                min_cutoff: 1.0,
                beta: 0.0,
            },
            calibration: Calibration::default(),
            vigem_error: None,
        }
    }
}

impl Telemetry {
    /// Everything the pipeline needs is alive and producing.
    pub fn is_usable(&self) -> bool {
        self.connected && self.output_connected && !self.paused
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventLevel {
    Info,
    Warning,
    Error,
}

/// Something happened worth surfacing as a toast in the UI.
#[derive(Debug, Clone)]
pub struct EngineEvent {
    pub level: EventLevel,
    pub text: String,
}

/// Commands the UI sends into the engine.
#[derive(Debug, Clone)]
pub enum EngineCommand {
    /// Persist new settings and reconfigure whatever they affect.
    ApplySettings(Box<Settings>),
    /// Replace the whole profile store and re-apply the active profile.
    ApplyProfiles(Box<ProfileStore>),
    /// Switch to a profile by id.
    SelectProfile(String),
    /// Cycle profiles by this many steps.
    CycleProfile(i32),
    /// Toggle the global pause.
    TogglePause,
    /// Re-capture resting stick positions.
    Recalibrate,
    /// Re-register hotkeys.
    ApplyHotkeys(Vec<HotkeyBinding>),
    /// Rebind the device watcher.
    ApplyDevice(DeviceSelection),
    /// Quit cleanly, unplugging the virtual pad.
    Shutdown,
}

/// Handle the UI uses to talk to the engine.
pub struct EngineHandle {
    tx: std::sync::mpsc::Sender<EngineCommand>,
    telemetry: Arc<Mutex<Telemetry>>,
    events: Arc<Mutex<Vec<EngineEvent>>>,
}

impl EngineHandle {
    /// Queue a command. The engine drains these once per loop and the last
    /// write wins, which is what a slider drag needs.
    pub fn send(&self, cmd: EngineCommand) {
        let _ = self.tx.send(cmd);
    }

    /// Latest telemetry snapshot.
    pub fn telemetry(&self) -> Telemetry {
        self.telemetry.lock().clone()
    }

    /// Take and clear pending notifications.
    pub fn drain_events(&self) -> Vec<EngineEvent> {
        std::mem::take(&mut *self.events.lock())
    }
}

/// Start the engine and its two pump threads, returning the UI-side handle.
/// How many samples of gyro output the UI keeps for its trace.
pub const HISTORY_LEN: usize = 140;

pub fn spawn(initial: Settings, store: ProfileStore) -> EngineHandle {
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<EngineCommand>();
    // Bounded so a stalled UI drops telemetry rather than growing unbounded.
    let (evt_tx, evt_rx) = std::sync::mpsc::sync_channel::<EngineEvent>(16);
    let (tel_tx, tel_rx) = std::sync::mpsc::sync_channel::<Telemetry>(4);

    let telemetry = Arc::new(Mutex::new(Telemetry::default()));
    let events = Arc::new(Mutex::new(Vec::new()));

    // The engine thread owns the command receiver and both senders.
    std::thread::Builder::new()
        .name("padforge-engine".into())
        .spawn(move || {
            let mut engine = Engine::new(initial, store, evt_tx);
            engine.run(cmd_rx, tel_tx);
            debug!("engine thread finished");
        })
        .expect("spawn engine thread");

    // A separate pump keeps the shared slot fresh without ever applying
    // back-pressure to the input loop.
    {
        let telemetry = Arc::clone(&telemetry);
        std::thread::Builder::new()
            .name("padforge-telemetry".into())
            .spawn(move || {
                while let Ok(t) = tel_rx.recv() {
                    *telemetry.lock() = t;
                }
                debug!("telemetry pump finished");
            })
            .expect("spawn telemetry thread");
    }

    {
        let events = Arc::clone(&events);
        std::thread::Builder::new()
            .name("padforge-events".into())
            .spawn(move || {
                while let Ok(ev) = evt_rx.recv() {
                    events.lock().push(ev);
                }
                debug!("event pump finished");
            })
            .expect("spawn event thread");
    }

    EngineHandle {
        tx: cmd_tx,
        telemetry,
        events,
    }
}

/// Everything the engine owns, living on the engine thread.
struct Engine {
    settings: Settings,
    store: ProfileStore,

    reader: Option<DeviceReader>,
    backend: Box<dyn OutputBackend>,
    vigem_error: Option<String>,

    // Per-axis filter chains, rebuilt whenever the profile changes.
    left_x: AxisFilter,
    left_y: AxisFilter,
    right_x: AxisFilter,
    right_y: AxisFilter,
    left_trigger: AxisFilter,
    right_trigger: AxisFilter,

    gyro: GyroProcessor,
    touchpad: TouchpadProcessor,
    /// Converts gyro rates and touchpad displacement into pointer pixels.
    gyro_pointer: GyroPointer,
    touchpad_pointer: TouchpadPointer,
    /// Last pointer delta emitted, so the UI can show it is working.
    last_pointer: (i32, i32),
    /// A pointer injection that Windows refused, surfaced once in the UI.
    pointer_error: Option<String>,
    /// Current synthetic mouse button state, so each edge is sent once.
    mouse_left_down: bool,
    mouse_right_down: bool,
    mouse_middle_down: bool,

    hotkeys: Option<HotkeyManager>,
    hotkey_bindings: Vec<HotkeyBinding>,

    /// Parsed axis formulas, in the order the six shaped axes are processed.
    ///
    /// A fixed array rather than a map keyed by control: the set of axes is
    /// closed, so indexing costs nothing where a hash lookup would show up in the
    /// per-frame profile the performance suite measures. Filled in
    /// `apply_profile`, which is the point a profile change arrives at.
    formulas: [Option<Formula>; 6],
    /// When the engine started, for the `t` and `now` formula sources.
    started: Instant,

    last_frame: Instant,
    last_publish: Instant,
    last_foreground_check: Instant,
    last_process: String,
    auto_match: Option<String>,

    lightbar_time: f32,
    last_lightbar: [u8; 3],
    last_gyro_delta: (f32, f32),

    /// Copied out of the active profile so the lightbar pass does not have to
    /// re-resolve the link chain on every frame.
    active_lightbar: LightbarConfig,
    /// Pending rumble command, consumed by the next output report.
    rumble: Option<(u8, u8)>,

    events: std::sync::mpsc::SyncSender<EngineEvent>,
}

impl Engine {
    fn new(
        settings: Settings,
        store: ProfileStore,
        events: std::sync::mpsc::SyncSender<EngineEvent>,
    ) -> Self {
        let profile = store.active_profile().resolve(&store).clone();
        let now = Instant::now();
        Self {
            settings,
            store,
            reader: None,
            backend: Box::new(NullBackend::default()),
            vigem_error: None,
            left_x: AxisFilter::new(),
            left_y: AxisFilter::new(),
            right_x: AxisFilter::new(),
            right_y: AxisFilter::new(),
            left_trigger: AxisFilter::new(),
            right_trigger: AxisFilter::new(),
            gyro: GyroProcessor::new(profile.gyro),
            touchpad: TouchpadProcessor::new(profile.touchpad),
            gyro_pointer: GyroPointer::new(profile.pointer),
            touchpad_pointer: TouchpadPointer::new(profile.pointer),
            last_pointer: (0, 0),
            pointer_error: None,
            mouse_left_down: false,
            mouse_right_down: false,
            mouse_middle_down: false,
            hotkeys: None,
            hotkey_bindings: Vec::new(),
            formulas: std::array::from_fn(|_| None),
            started: now,
            last_frame: now,
            last_publish: now,
            last_foreground_check: now,
            last_process: String::new(),
            auto_match: None,
            lightbar_time: 0.0,
            last_lightbar: [0, 0, 0],
            last_gyro_delta: (0.0, 0.0),
            active_lightbar: profile.lightbar,
            rumble: None,
            events,
        }
    }

    /// (Re)connect the output backend, the reader, and the hotkeys.
    fn bring_up(&mut self) {
        self.build_backend();
        self.start_reader();
        self.start_hotkeys();
    }

    fn build_backend(&mut self) {
        self.backend.shutdown();
        self.vigem_error = None;

        #[cfg(target_os = "windows")]
        if self.settings.output_mode == OutputMode::Xbox360 {
            let vigem = crate::output::VigemBackend::connect();
            self.vigem_error = vigem.last_error().map(str::to_owned);
            if vigem.is_connected() {
                info!("virtual pad ready");
                self.backend = Box::new(vigem);
            } else {
                warn!("no virtual pad; input will not reach games");
                self.backend = Box::new(NullBackend::default());
            }
        }

        if self.settings.output_mode == OutputMode::MonitorOnly {
            self.backend = Box::new(NullBackend::default());
        }
    }

    fn start_reader(&mut self) {
        // Dropping the reader joins its thread, so this must happen first.
        self.reader = None;
        let wanted = self.settings.device.serial().map(str::to_owned);
        let rate = self.settings.poll_rate_hz;
        match DeviceReader::start(wanted.clone(), rate) {
            Ok(reader) => {
                debug!(serial = ?wanted, rate, "watching for a pad");
                self.reader = Some(reader);
            }
            Err(e) => self.emit(
                EventLevel::Error,
                format!("Could not reach the HID bus: {e}"),
            ),
        }
    }

    fn start_hotkeys(&mut self) {
        match HotkeyManager::new() {
            Ok(mut mgr) => {
                match mgr.register_all(&self.hotkey_bindings) {
                    Ok(failures) => {
                        for (action, why) in failures {
                            self.emit(
                                EventLevel::Warning,
                                format!("{} is unavailable: {why}", describe_action(action)),
                            );
                        }
                    }
                    Err(e) => self.emit(EventLevel::Warning, format!("Hotkeys unavailable: {e}")),
                }
                self.hotkeys = Some(mgr);
            }
            Err(e) => {
                debug!("hotkey manager unavailable: {e}");
                self.emit(
                    EventLevel::Warning,
                    "Global hotkeys are unavailable on this system.".into(),
                );
                self.hotkeys = None;
            }
        }
    }

    /// Pull the active profile's settings into the live filter chain.
    fn apply_profile(&mut self) {
        let profile = {
            let store = &self.store;
            store.active_profile().resolve(store).clone()
        };
        self.left_x = profile.left_x.to_filter();
        self.left_y = profile.left_y.to_filter();
        self.right_x = profile.right_x.to_filter();
        self.right_y = profile.right_y.to_filter();
        self.left_trigger = profile.left_trigger.to_filter();
        self.right_trigger = profile.right_trigger.to_filter();

        // Formulas are parsed here rather than per frame. The text only changes
        // when a profile is edited, and this is the point where that arrives, so
        // parsing costs nothing on the hot path and a bad formula is reported
        // against the axis it was typed into rather than silently ignored.
        self.formulas = [
            profile.left_x.formula(),
            profile.left_y.formula(),
            profile.right_x.formula(),
            profile.right_y.formula(),
            profile.left_trigger.formula(),
            profile.right_trigger.formula(),
        ];
        self.gyro.set_config(profile.gyro);
        self.touchpad.set_config(profile.touchpad);
        self.gyro_pointer.set_config(profile.pointer);
        self.touchpad_pointer.set_config(profile.pointer);
        // A stale sub-pixel carry would be spent on the new profile's motion.
        self.gyro_pointer.reset();
        self.touchpad_pointer.reset();
        self.active_lightbar = profile.lightbar;
        self.lightbar_time = 0.0;
        self.last_gyro_delta = (0.0, 0.0);
        debug!("applied profile '{}'", profile.name);
    }

    fn run(
        &mut self,
        rx: std::sync::mpsc::Receiver<EngineCommand>,
        ttx: std::sync::mpsc::SyncSender<Telemetry>,
    ) {
        self.bring_up();
        let interval = poll_interval(self.settings.poll_rate_hz);

        loop {
            // Drain every pending command: last write wins.
            while let Ok(cmd) = rx.try_recv() {
                if !self.handle(cmd) {
                    return;
                }
            }

            let now = Instant::now();
            let dt = now.duration_since(self.last_frame).as_secs_f32();
            self.last_frame = now;
            self.tick(dt, &ttx);

            std::thread::sleep(interval);
        }
    }

    /// Returns false when the engine should exit.
    fn handle(&mut self, cmd: EngineCommand) -> bool {
        match cmd {
            EngineCommand::ApplySettings(s) => {
                let next = s.sanitised();
                let needs_reader = next.poll_rate_hz != self.settings.poll_rate_hz
                    || next.device != self.settings.device;
                let needs_backend = next.output_mode != self.settings.output_mode;
                self.settings = next;
                if needs_backend {
                    self.build_backend();
                }
                if needs_reader {
                    self.start_reader();
                }
                true
            }
            EngineCommand::ApplyProfiles(store) => {
                self.store = *store;
                self.apply_profile();
                true
            }
            EngineCommand::SelectProfile(id) => {
                if self.store.select(&id) {
                    self.apply_profile();
                    let name = self.store.active_profile().name.clone();
                    self.emit(EventLevel::Info, format!("Profile: {name}"));
                }
                true
            }
            EngineCommand::CycleProfile(delta) => {
                self.store.cycle(delta as isize);
                self.apply_profile();
                let name = self.store.active_profile().name.clone();
                self.emit(EventLevel::Info, format!("Profile: {name}"));
                true
            }
            EngineCommand::TogglePause => {
                self.settings.paused = !self.settings.paused;
                if self.settings.paused {
                    // Release everything so the game immediately sees "let go".
                    self.backend.submit(GamepadState::neutral());
                }
                true
            }
            EngineCommand::Recalibrate => {
                if let Some(r) = self.reader.as_mut() {
                    r.reset_calibration();
                    self.emit(
                        EventLevel::Info,
                        "Calibrating: hold the sticks still for a moment.".into(),
                    );
                }
                true
            }
            EngineCommand::ApplyHotkeys(bindings) => {
                self.hotkey_bindings = bindings;
                if let Some(mgr) = self.hotkeys.as_mut() {
                    match mgr.register_all(&self.hotkey_bindings) {
                        Ok(failures) => {
                            for (action, why) in failures {
                                self.emit(
                                    EventLevel::Warning,
                                    format!("{} is unavailable: {why}", describe_action(action)),
                                );
                            }
                        }
                        Err(e) => {
                            self.emit(EventLevel::Warning, format!("Hotkeys unavailable: {e}"))
                        }
                    }
                }
                true
            }
            EngineCommand::ApplyDevice(sel) => {
                self.settings.device = sel;
                self.start_reader();
                true
            }
            EngineCommand::Shutdown => {
                // Release, then unplug, so nothing is left held down in a game.
                self.backend.submit(GamepadState::neutral());
                self.backend.shutdown();
                // Also release synthetic mouse buttons: a stuck left button would
                // outlive the process in whatever window the player returns to.
                for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
                    self.set_mouse_button(button, false);
                }
                false
            }
        }
    }

    fn tick(&mut self, dt: f32, ttx: &std::sync::mpsc::SyncSender<Telemetry>) {
        self.service_hotkeys();
        self.service_auto_profiles();
        self.lightbar_time += dt;

        // Copy out what the reader knows before taking `&mut self`, so the
        // translation and lightbar passes can both mutate engine state.
        let (snap, calibration) = match self.reader.as_ref() {
            Some(reader) => (reader.snapshot(), reader.calibration()),
            // No reader at all: stay quiet rather than spamming empty reports.
            None => return,
        };
        let profile = {
            let store = &self.store;
            store.active_profile().resolve(store).clone()
        };

        self.drive_lightbar(&snap);

        let active = if snap.connected && !self.settings.paused {
            let state = self.translate(&snap.report, &profile, dt);
            self.backend.submit(state);
            active_control_names(&snap.report, &profile)
        } else {
            // Always keep the virtual pad in a known-released state.
            self.backend.submit(GamepadState::neutral());
            Vec::new()
        };

        if self.last_publish.elapsed() >= PUBLISH_INTERVAL {
            self.last_publish = Instant::now();
            let telemetry = Telemetry {
                connected: snap.connected,
                device_label: snap.info.label(),
                transport: snap.connected.then_some(snap.info.transport),
                battery: snap.report.battery,
                report: snap.report.clone(),
                output: self.backend.last_state(),
                active_controls: active,
                output_backend: self.backend.name(),
                output_connected: self.backend.is_connected(),
                profile_name: profile.name.clone(),
                profile_id: profile.id.clone(),
                packets: snap.packets,
                frame_ms: snap.frame_ms,
                paused: self.settings.paused,
                lightbar: self.last_lightbar,
                auto_profile_matched: self.auto_match.clone(),
                gyro_delta: self.last_gyro_delta,
                pointer_delta: self.last_pointer,
                pointer_error: self.pointer_error.clone(),
                gyro_enabled: profile.gyro.enabled,
                gyro_smoothing: smoothing_params(&profile.gyro),
                calibration,
                vigem_error: self.vigem_error.clone(),
            };
            // try_send means a busy UI drops a visual frame instead of stalling
            // the input pipeline.
            if ttx.try_send(telemetry).is_err() {
                debug!("telemetry queue full, dropping a frame");
            }
        }
    }

    /// Apply the formula attached to one of the six shaped axes, if it has one.
    ///
    /// `slot` indexes [`Self::formulas`], which is filled in `apply_profile` when
    /// a profile is activated. Parsing happens there rather than here so the hot
    /// path is a token walk and nothing else.
    ///
    /// The axis being shaped is offered to its own formula as `a1`, so a formula can
    /// say "scale me" without knowing which axis it is attached to, and the other
    /// three are visible as `a2` to `a4`.
    fn with_formula(&self, slot: usize, value: f32, axes: [f32; 4], now_ms: f64) -> f32 {
        let Some(formula) = self.formulas.get(slot).and_then(|f| f.as_ref()) else {
            return value;
        };
        let mut inputs = Inputs {
            axes,
            buttons: [0.0; 4],
            sliders: [0.0; 4],
            now_ms,
        };
        inputs.axes[0] = value;
        // A formula was validated when the profile was activated, so reaching here
        // with an error would mean something bypassed that. Falling back to the
        // shaped value keeps the axis alive rather than zeroing it, which would be
        // far more disruptive than ignoring a formula that should not exist.
        formula.eval(&inputs).unwrap_or(value)
    }

    /// Turn one DS4 report into an `XINPUT_GAMEPAD` state.
    fn translate(&mut self, report: &Ds4Report, profile: &Profile, dt: f32) -> GamepadState {
        let mut state = GamepadState::neutral();
        // The origin for the time a formula can read. Taken once per frame so every
        // axis in this report sees the same value.
        // Elapsed time for formulas that read it. Taken once per frame so every
        // axis in this report sees the same value rather than six slightly
        // different ones.
        let now_ms = self.started.elapsed().as_secs_f64() * 1000.0;

        // Analogue axes through their filter chains.
        let lx = self.left_x.apply(report.left_x);
        let ly = self.left_y.apply(report.left_y);
        let rx = self.right_x.apply(report.right_x);
        let ry = self.right_y.apply(report.right_y);

        // Formulas run on the shaped values, and each axis sees all four so that
        // something like `a1 * a2` is possible. The value being shaped is offered
        // to its own formula as `a1`, which is what lets a formula say "scale me"
        // without knowing which axis it is attached to.
        let shaped = [lx, ly, rx, ry];
        let lx = self.with_formula(0, shaped[0], shaped, now_ms);
        let ly = self.with_formula(1, shaped[1], shaped, now_ms);
        let rx = self.with_formula(2, shaped[2], shaped, now_ms);
        let ry = self.with_formula(3, shaped[3], shaped, now_ms);

        state.thumb_lx = GamepadState::quantise(lx);
        state.thumb_ly = GamepadState::quantise(ly);
        state.thumb_rx = GamepadState::quantise(rx);
        state.thumb_ry = GamepadState::quantise(ry);

        // Triggers carry both an analog value and a digital press threshold.
        let l2 = self.left_trigger.apply(report.l2);
        let r2 = self.right_trigger.apply(report.r2);
        let triggers = [l2, r2, 0.0, 0.0];
        let l2 = self.with_formula(4, l2, triggers, now_ms);
        let r2 = self.with_formula(5, r2, triggers, now_ms);
        state.left_trigger = GamepadState::quantise_trigger(l2);
        state.right_trigger = GamepadState::quantise_trigger(r2);

        let held = report.buttons.raw();
        let touch_held = report.touch.pad_touched || report.touch.pad_clicked;
        let mut buttons = XButtons::default();

        // --- digital controls -------------------------------------------
        for control in Ds4Control::ALL {
            if control.is_axis_direction() {
                continue; // handled in the stick-emulation pass below
            }
            let mapping = profile.mapping_for(*control);
            if !mapping.is_active() || !mapping.modifiers_met(held, touch_held) {
                continue;
            }
            if let Some(mod_target) = mapping.mod_target {
                if !target_gate_met(mod_target, held, report) {
                    continue;
                }
            }

            let pressed = match control {
                Ds4Control::TouchpadClick => report.touch.pad_clicked,
                Ds4Control::TouchpadGesture => touch_held,
                other => held & other.button_mask() == other.button_mask(),
            };
            if !pressed {
                continue;
            }

            // A curve or sensitivity on a digital control acts as a gate: below
            // the threshold the button simply never fires.
            if gate_from_curve(mapping.curve, mapping.sensitivity) {
                buttons.insert(mapping.target.bit());
            }
        }

        // --- touchpad as a d-pad ----------------------------------------
        if profile.touchpad.mode == TouchpadMode::Dpad {
            let t = self.touchpad.process(&report.touch);
            if t.dpad.0 {
                buttons.insert(XButtons::DPAD_UP);
            }
            if t.dpad.1 {
                buttons.insert(XButtons::DPAD_DOWN);
            }
            if t.dpad.2 {
                buttons.insert(XButtons::DPAD_LEFT);
            }
            if t.dpad.3 {
                buttons.insert(XButtons::DPAD_RIGHT);
            }
        }

        // --- stick direction emulation -----------------------------------
        for control in Ds4Control::STICKS {
            let mapping = profile.mapping_for(*control);
            if !mapping.is_active() || !mapping.target.is_stick_emulation() {
                continue;
            }
            if !mapping.modifiers_met(held, touch_held) {
                continue;
            }
            let (axis, positive) = direction_of(*control);
            let value = match axis {
                StickAxis::LeftX => state.thumb_lx,
                StickAxis::LeftY => state.thumb_ly,
                StickAxis::RightX => state.thumb_rx,
                StickAxis::RightY => state.thumb_ry,
            };
            let engaged = if positive {
                value >= STICK_EMULATION_THRESHOLD
            } else {
                value <= -STICK_EMULATION_THRESHOLD
            };
            if engaged {
                buttons.insert(mapping.target.bit());
            }
        }

        // --- gyro ---------------------------------------------------------
        self.last_gyro_delta = (0.0, 0.0);
        if profile.gyro.enabled {
            let d = self.gyro.process(&report.gyro, dt);
            self.last_gyro_delta = (d.x, d.y);
            match profile.gyro.output_mode {
                GyroOutputMode::Mouse => {
                    // The gyro processor still runs above so its filters and
                    // readouts stay live; only the injection is separate, because
                    // it must work in raw report units rather than shaped ones.
                    let delta = self.gyro_pointer.process(&report.gyro, dt);
                    self.emit_pointer(delta);
                }
                GyroOutputMode::Stick => {
                    state.thumb_lx = GamepadState::quantise(lx + d.x);
                    state.thumb_ly = GamepadState::quantise(ly - d.y);
                }
                GyroOutputMode::RightStick => {
                    state.thumb_rx = GamepadState::quantise(rx + d.x);
                    state.thumb_ry = GamepadState::quantise(ry - d.y);
                }
                GyroOutputMode::Triggers => {
                    state.left_trigger =
                        GamepadState::quantise_trigger((d.x * 0.5 + 0.5).clamp(0.0, 1.0));
                    state.right_trigger =
                        GamepadState::quantise_trigger((d.y * 0.5 + 0.5).clamp(0.0, 1.0));
                }
            }
        }

        // The touchpad always runs so its internal state stays coherent, even
        // when its output is not currently routed anywhere.
        if profile.touchpad.mode != TouchpadMode::Off {
            let out = self.touchpad.process(&report.touch);
            if profile.touchpad.mode == TouchpadMode::Mouse {
                // `mouse_delta` is already a displacement in pad fractions; the
                // pointer path wants pad counts, so scale by the pad resolution.
                let delta = self.touchpad_pointer.process(
                    out.mouse_delta.0 as f64 * TOUCHPAD_RES_X,
                    out.mouse_delta.1 as f64 * TOUCHPAD_RES_Y,
                );
                self.emit_pointer(delta);
                self.set_mouse_button(MouseButton::Left, out.left_down);
                self.set_mouse_button(MouseButton::Right, out.right_down);
            } else {
                // Release anything the pointer path was holding, so switching
                // modes cannot leave a button stuck down.
                self.set_mouse_button(MouseButton::Left, false);
                self.set_mouse_button(MouseButton::Right, false);
            }
        }

        state.buttons = buttons.0;
        state
    }

    /// Inject one pointer delta, remembering it for the UI.
    ///
    /// A failure is recorded rather than logged and forgotten: `SendInput`
    /// returns 0 with no error when UIPI blocks the call, which happens when
    /// this process is less trusted than the foreground window. That is the most
    /// common reason gyro aiming silently does nothing, so the user needs to see
    /// it.
    fn emit_pointer(&mut self, delta: (i32, i32)) {
        self.last_pointer = delta;
        if delta == (0, 0) {
            return;
        }
        if let Err(e) = pointer::inject_relative_motion(delta.0, delta.1) {
            let message = format!("Pointer injection was blocked: {e}");
            if self.pointer_error.as_deref() != Some(message.as_str()) {
                self.pointer_error = Some(message.clone());
                self.emit(EventLevel::Warning, message);
            }
        }
    }

    /// Drive a mouse button, remembering its state so it is only sent on change.
    fn set_mouse_button(&mut self, button: MouseButton, down: bool) {
        let slot = match button {
            MouseButton::Left => &mut self.mouse_left_down,
            MouseButton::Right => &mut self.mouse_right_down,
            MouseButton::Middle => &mut self.mouse_middle_down,
        };
        if *slot == down {
            return;
        }
        *slot = down;
        if let Err(e) = pointer::inject_button(button, down) {
            tracing::trace!("mouse button injection failed: {e}");
        }
    }

    /// Queue a lightbar report, but only when the colour actually changed.
    fn drive_lightbar(&mut self, snap: &DeviceSnapshot) {
        let (Some(reader), true) = (self.reader.as_ref(), snap.connected) else {
            return;
        };
        let cfg = &self.active_lightbar;
        let colour = if cfg.enabled {
            let c = cfg
                .mode
                .colour_at(cfg.color, self.lightbar_time * cfg.rate.max(0.05));
            let scale = cfg.brightness.clamp(0.0, 1.0);
            [
                (c[0] as f32 * scale).round() as u8,
                (c[1] as f32 * scale).round() as u8,
                (c[2] as f32 * scale).round() as u8,
            ]
        } else {
            [0, 0, 0]
        };

        if colour == self.last_lightbar {
            return;
        }
        let buf = report::output_report(
            snap.info.transport,
            colour[0],
            colour[1],
            colour[2],
            self.rumble.take(),
        );
        if reader.send_output(&buf) {
            self.last_lightbar = colour;
        }
    }

    fn service_hotkeys(&mut self) {
        let actions: Vec<HotkeyAction> = match self.hotkeys.as_mut() {
            Some(m) => m.poll(),
            None => return,
        };
        for action in actions {
            match action {
                HotkeyAction::SelectProfile(i) => {
                    if let Some(id) = self.store.profiles.get(i).map(|p| p.id.clone()) {
                        self.store.select(&id);
                        self.apply_profile();
                    }
                }
                HotkeyAction::NextProfile => {
                    self.store.cycle(1);
                    self.apply_profile();
                }
                HotkeyAction::PrevProfile => {
                    self.store.cycle(-1);
                    self.apply_profile();
                }
                HotkeyAction::TogglePause => {
                    self.settings.paused = !self.settings.paused;
                    if self.settings.paused {
                        self.backend.submit(GamepadState::neutral());
                    }
                }
                HotkeyAction::CycleLightbar => {
                    let next = {
                        let store = &mut self.store;
                        let p = store.active_profile_mut();
                        let i = LightbarMode::ALL
                            .iter()
                            .position(|m| *m == p.lightbar.mode)
                            .unwrap_or(0);
                        p.lightbar.mode = LightbarMode::ALL[(i + 1) % LightbarMode::ALL.len()];
                        p.lightbar.mode
                    };
                    self.emit(EventLevel::Info, format!("Lightbar: {}", next.label()));
                }
                HotkeyAction::Recalibrate => {
                    if let Some(r) = self.reader.as_mut() {
                        r.reset_calibration();
                        self.emit(EventLevel::Info, "Recalibrating sticks.".into());
                    }
                }
            }
        }
    }

    fn service_auto_profiles(&mut self) {
        if !self.settings.auto_profiles_enabled {
            return;
        }
        if self.last_foreground_check.elapsed()
            < Duration::from_millis(self.settings.auto_profile_interval_ms)
        {
            return;
        }
        self.last_foreground_check = Instant::now();

        let Some(proc) = process::foreground_process() else {
            return;
        };
        if proc.name == self.last_process {
            return;
        }
        self.last_process = proc.name.clone();

        let matched = self.store.match_process(&proc.name).map(|p| p.id.clone());
        self.auto_match = matched.clone();
        if let Some(id) = matched {
            if self.store.index_of(&id) != Some(self.store.active) {
                self.store.select(&id);
                self.apply_profile();
                let name = self.store.active_profile().name.clone();
                debug!(process = %proc.name, profile = %name, "auto-profile switched");
            }
        }
    }

    fn emit(&self, level: EventLevel, text: String) {
        let _ = self.events.try_send(EngineEvent { level, text });
    }
}

/// The touchpad's raw resolution, used to convert the processor's fractional
/// displacement back into pad counts for the pointer path.
const TOUCHPAD_RES_X: f64 = 1920.0;
const TOUCHPAD_RES_Y: f64 = 942.0;

/// Stick deflection, in XInput counts, at which a direction mapping fires.
const STICK_EMULATION_THRESHOLD: i16 = 12_000;

/// Whether a digital mapping fires at full deflection.
///
/// A curve on a digital control is unusual, so the semantics are chosen to be
/// predictable: `sensitivity` scales the output, and a curve whose output at
/// full press falls below half scale mutes the button. `Linear` and `Exponential`
/// always reach 1.0 at full input, so only a badly shaped bezier or a low
/// sensitivity can suppress one.
fn gate_from_curve(curve: Curve, sensitivity: f32) -> bool {
    let shaped = match curve {
        Curve::Linear => 1.0,
        // An exponential is x^n, which is 1.0 at x = 1 regardless of n, so the
        // exponent cannot gate on its own.
        Curve::Exponential { .. } => 1.0,
        Curve::Bezier { x, y } => crate::filters::bezier_eval(1.0, x, y),
        // A stepped curve is live for any threshold at or below full travel.
        Curve::Stepped { threshold } => {
            if threshold <= 1.0 {
                1.0
            } else {
                0.0
            }
        }
    };
    shaped * sensitivity >= 0.5
}

/// Unpack the smoothing enum into the flat fields telemetry carries.
fn smoothing_params(cfg: &GyroConfig) -> GyroSmoothingParams {
    let (kind, min_cutoff, beta) = match cfg.smoothing {
        GyroSmoothing::None => (GyroSmoothingKind::None, 0.0, 0.0),
        GyroSmoothing::LowPass { cutoff } => (GyroSmoothingKind::LowPass, cutoff, 0.0),
        GyroSmoothing::OneEuro { min_cutoff, beta } => {
            (GyroSmoothingKind::OneEuro, min_cutoff, beta)
        }
    };
    GyroSmoothingParams {
        kind,
        min_cutoff,
        beta,
    }
}

/// Is the DS4 control that normally drives `target` currently held?
fn target_gate_met(target: X360Control, held: u16, report: &Ds4Report) -> bool {
    let Some(control) = default_inverse().get(&target) else {
        return false;
    };
    match control {
        Ds4Control::TouchpadClick => report.touch.pad_clicked,
        Ds4Control::TouchpadGesture => report.touch.pad_touched,
        other => held & other.button_mask() == other.button_mask(),
    }
}

/// Inverse of the default mapping: which DS4 control normally drives a target.
fn default_inverse() -> &'static std::collections::HashMap<X360Control, Ds4Control> {
    static CELL: OnceLock<std::collections::HashMap<X360Control, Ds4Control>> = OnceLock::new();
    CELL.get_or_init(|| {
        let p = Profile::new();
        let mut map = std::collections::HashMap::new();
        for c in Ds4Control::ALL {
            if c.is_axis_direction() {
                continue;
            }
            let m = p.mapping_for(*c);
            if m.target != X360Control::None {
                map.entry(m.target).or_insert(*c);
            }
        }
        map
    })
}

#[derive(Clone, Copy)]
enum StickAxis {
    LeftX,
    LeftY,
    RightX,
    RightY,
}

fn direction_of(c: Ds4Control) -> (StickAxis, bool) {
    match c {
        Ds4Control::LeftXNegative => (StickAxis::LeftX, false),
        Ds4Control::LeftXPositive => (StickAxis::LeftX, true),
        Ds4Control::LeftYNegative => (StickAxis::LeftY, false),
        Ds4Control::LeftYPositive => (StickAxis::LeftY, true),
        Ds4Control::RightXNegative => (StickAxis::RightX, false),
        Ds4Control::RightXPositive => (StickAxis::RightX, true),
        Ds4Control::RightYNegative => (StickAxis::RightY, false),
        Ds4Control::RightYPositive => (StickAxis::RightY, true),
        // Unreachable: the caller filters axis directions out first.
        _ => (StickAxis::LeftX, true),
    }
}

/// Names of the mapped DS4 controls currently held, for the activity monitor.
fn active_control_names(report: &Ds4Report, profile: &Profile) -> Vec<&'static str> {
    let mut out = Vec::new();
    for control in Ds4Control::ALL {
        if control.is_axis_direction() {
            continue;
        }
        let pressed = match control {
            Ds4Control::TouchpadClick => report.touch.pad_clicked,
            Ds4Control::TouchpadGesture => report.touch.pad_touched,
            other => report.buttons.raw() & other.button_mask() == other.button_mask(),
        };
        if pressed && profile.mapping_for(*control).is_active() {
            out.push(control.label());
        }
    }
    out
}

fn describe_action(action: HotkeyAction) -> String {
    match action {
        HotkeyAction::SelectProfile(i) => format!("Profile {i}"),
        HotkeyAction::NextProfile => "Next profile".into(),
        HotkeyAction::PrevProfile => "Previous profile".into(),
        HotkeyAction::TogglePause => "Pause toggle".into(),
        HotkeyAction::CycleLightbar => "Lightbar cycle".into(),
        HotkeyAction::Recalibrate => "Recalibrate".into(),
    }
}

/// Poll interval matching a target rate.
pub fn poll_interval(hz: u32) -> Duration {
    Duration::from_micros(1_000_000 / hz.clamp(1, 1000) as u64)
}

/// Stick deflection, as a fraction of full travel, at which a direction mapping
/// fires. Mirrors `STICK_EMULATION_THRESHOLD` so the UI can highlight the same
/// inputs the engine is reacting to.
pub fn poll_threshold_fraction() -> f32 {
    STICK_EMULATION_THRESHOLD as f32 / i16::MAX as f32
}

/// Enumerate connected pads, without opening them.
pub fn list_devices() -> Vec<crate::device::DeviceInfo> {
    device::enumerate()
}

/// Gyro config re-export, used by the UI's settings page.
pub use crate::gyro::GyroConfig as GyroSettings;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_interval_is_reasonable() {
        assert_eq!(poll_interval(1000), Duration::from_micros(1000));
        assert!(poll_interval(125) > poll_interval(1000));
    }

    #[test]
    fn telemetry_usability_requires_all_three() {
        let mut t = Telemetry::default();
        assert!(!t.is_usable());
        t.connected = true;
        assert!(!t.is_usable(), "still needs an output backend");
        t.output_connected = true;
        assert!(t.is_usable());
        t.paused = true;
        assert!(!t.is_usable());
    }

    #[test]
    fn linear_curve_always_fires() {
        assert!(gate_from_curve(Curve::Linear, 1.0));
        // Even a heavily damped linear mapping still fires at full deflection.
        assert!(gate_from_curve(Curve::Linear, 0.5));
    }

    #[test]
    fn exponential_shape_alone_never_mutes() {
        // x^n is 1.0 at x = 1 for any n, so the exponent cannot gate a button.
        assert!(gate_from_curve(Curve::Exponential { exponent: 7.0 }, 1.0));
        assert!(gate_from_curve(Curve::Exponential { exponent: 1.5 }, 1.0));
    }

    #[test]
    fn badly_shaped_bezier_can_mute() {
        // A curve whose last control point sits low loses output at full press.
        let flat_end = Curve::Bezier {
            x: [0.0, 0.9, 1.0],
            y: [0.0, 0.2, 0.3],
        };
        assert!(!gate_from_curve(flat_end, 1.0));
        // The standard smooth curve keeps full output and stays live.
        assert!(gate_from_curve(Curve::smooth(), 1.0));
    }

    #[test]
    fn sensitivity_below_half_mutes() {
        assert!(!gate_from_curve(Curve::Linear, 0.25));
        assert!(gate_from_curve(Curve::Linear, 0.75));
    }

    #[test]
    fn inverse_map_covers_default_bindings() {
        let inv = default_inverse();
        assert_eq!(inv.get(&X360Control::A), Some(&Ds4Control::Cross));
        assert_eq!(inv.get(&X360Control::Start), Some(&Ds4Control::Options));
    }

    #[test]
    fn direction_lookup_is_total() {
        for c in Ds4Control::ALL.iter().filter(|c| c.is_axis_direction()) {
            let _ = direction_of(*c);
        }
    }

    #[test]
    fn active_names_ignore_unmapped_controls() {
        let profile = Profile::new();
        let mut report = Ds4Report::neutral();
        report.buttons = crate::report::Buttons(crate::report::Buttons::CROSS);
        let names = active_control_names(&report, &profile);
        assert!(names.contains(&"Cross"));

        // Remove the mapping: the control is no longer reported.
        let mut bare = Profile::new();
        bare.mapping.clear();
        assert!(active_control_names(&report, &bare).is_empty());
    }
}
