//! DualShock 4 discovery and streaming.
//!
//! Discovery goes through hidapi rather than raw Win32 so the same code builds
//! anywhere hidapi does; the two report layouts (USB and Bluetooth) are handled
//! inside [`crate::report`].
//!
//! The reader owns the open HID handle and runs on its own thread. It publishes
//! the newest frame through a mutex-protected slot and accepts output reports
//! (lightbar colour, rumble) over a channel, so neither side ever blocks.

use std::ffi::CString;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tracing::{debug, info, warn};

use crate::report::{self, Ds4Report, Transport};

/// Description of a pad we can see, without holding it open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub path: String,
    pub serial: Option<String>,
    pub product: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub transport: Transport,
}

impl DeviceInfo {
    /// Short label for the UI, disambiguating multiple pads.
    ///
    /// A pad on USB reports no serial of its own, and the placeholder it gives
    /// instead is a run of zeroes; printing either would come out as
    /// `Wireless Controller - ` with nothing after the dash, so both are
    /// treated as "no serial".
    pub fn label(&self) -> String {
        let base = if self.product.trim().is_empty() {
            "DualShock 4".to_string()
        } else {
            self.product.trim().to_string()
        };
        let serial = self
            .serial
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.chars().all(|c| c == '0' || c == ':'));
        match serial {
            Some(s) if s.chars().count() >= 4 => {
                let tail: String = s.chars().skip(s.chars().count() - 4).collect();
                format!("{base} - {tail}")
            }
            Some(s) => format!("{base} - {s}"),
            None => base,
        }
    }
}

/// Everything the HID layer can see, with no filtering at all.
///
/// This exists to answer one question when a pad will not connect: is the
/// filter wrong, or is the device not being enumerated? Those look identical
/// from the outside and have nothing in common as fixes, so the diagnostic needs
/// to say which one it is.
pub fn probe_all_devices() -> Vec<RawDeviceInfo> {
    let api = match hidapi::HidApi::new() {
        Ok(a) => a,
        Err(e) => {
            warn!("could not initialise the HID API: {e}");
            return Vec::new();
        }
    };

    api.device_list()
        .map(|d| RawDeviceInfo {
            path: d.path().to_string_lossy().into_owned(),
            vendor_id: d.vendor_id(),
            product_id: d.product_id(),
            usage_page: d.usage_page(),
            usage: d.usage(),
            interface_number: d.interface_number(),
            bus_type: format!("{:?}", d.bus_type()),
            serial: d.serial_number().unwrap_or_default().to_string(),
            manufacturer: d.manufacturer_string().unwrap_or_default().to_string(),
            product: d.product_string().unwrap_or_default().to_string(),
        })
        .collect()
}

/// One entry from [`probe_all_devices`].
///
/// hidapi exposes no report-length accessors on this version, so the sizes are
/// absent. They are not needed: which interface is the gamepad is decided by the
/// usage page and usage, and the report size is fixed per transport.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawDeviceInfo {
    pub path: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub usage_page: u16,
    pub usage: u16,
    pub interface_number: i32,
    /// `hidapi::BusType`, rendered. Kept as text because the probe only reports
    /// it; nothing branches on it.
    pub bus_type: String,
    pub serial: String,
    pub manufacturer: String,
    pub product: String,
}

/// The pads PadForge would actually connect to, applying the same filter the
/// reader thread uses.
///
/// Sharing the filter with the reader matters: a probe that duplicates the
/// predicate can agree with itself while disagreeing with the engine.
pub fn probe_matching_devices() -> Vec<RawDeviceInfo> {
    probe_all_devices()
        .into_iter()
        .filter(|d| {
            d.vendor_id == report::SONY_VENDOR_ID && report::DS4_PRODUCT_IDS.contains(&d.product_id)
        })
        // The Bluetooth pad enumerates once per HID interface. Only the gamepad
        // interface carries reports; the others are the speaker, microphone and
        // the control-transport endpoint, and opening one of those yields a device
        // that never sends a report.
        .filter(|d| d.usage_page == 0x01 && d.usage == 0x05)
        .collect()
}

/// Live state shared between the reader thread and the engine.
#[derive(Debug, Clone)]
pub struct DeviceSnapshot {
    pub report: Ds4Report,
    pub info: DeviceInfo,
    /// Cumulative reports decoded, shown as a health signal.
    pub packets: u64,
    /// Time since the previous report, used to spot a stalled link.
    pub frame_ms: f32,
    pub connected: bool,
}

impl DeviceSnapshot {
    fn empty() -> Self {
        Self {
            report: Ds4Report::neutral(),
            info: DeviceInfo {
                path: String::new(),
                serial: None,
                product: String::new(),
                vendor_id: 0,
                product_id: 0,
                transport: Transport::Usb,
            },
            packets: 0,
            frame_ms: 0.0,
            connected: false,
        }
    }
}

/// Resting-position offsets captured per axis.
///
/// Stored as running sums rather than a mean: the axes only span 0..=255, so the
/// public fields are the accumulated offsets in counts and are what gets applied
/// to incoming reports.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Calibration {
    /// Accumulated samples seen, used to compute the mean on read.
    #[serde(default)]
    pub count: u32,
    /// Running sum of `raw - midpoint` for each axis.
    #[serde(default)]
    pub sum_x: i32,
    #[serde(default)]
    pub sum_y: i32,
    #[serde(default)]
    pub sum_rx: i32,
    #[serde(default)]
    pub sum_ry: i32,
    /// True once at least one resting sample has been folded in. Distinguishes
    /// "calibrated as centred" from "never calibrated", which both read as zero.
    #[serde(default)]
    pub captured: bool,
}

impl Calibration {
    /// Effective offset for the left stick's X axis, in counts.
    pub fn left_x(&self) -> i16 {
        self.mean(self.sum_x)
    }

    pub fn left_y(&self) -> i16 {
        self.mean(self.sum_y)
    }

    pub fn right_x(&self) -> i16 {
        self.mean(self.sum_rx)
    }

    pub fn right_y(&self) -> i16 {
        self.mean(self.sum_ry)
    }

    fn mean(&self, sum: i32) -> i16 {
        if self.count == 0 {
            return 0;
        }
        (sum / self.count as i32).clamp(i16::MIN as i32, i16::MAX as i32) as i16
    }

    /// All four offsets, for callers that want them together.
    pub fn offsets(&self) -> (i16, i16, i16, i16) {
        (self.left_x(), self.left_y(), self.right_x(), self.right_y())
    }

    pub fn is_captured(&self) -> bool {
        self.captured && self.count > 0
    }

    /// Fold one resting sample into the running mean.
    ///
    /// An incremental EMA does not work here: the stored value is a small
    /// integer, so both truncating and rounding-on-store stall at zero for any
    /// offset below the smoothing factor (`0 + 10/64` rounds back to 0, every
    /// time). Accumulating into a wider sum and dividing on read converges
    /// correctly regardless of magnitude.
    pub fn accumulate(&mut self, sample: &Ds4Report) {
        self.count = self.count.saturating_add(1);
        self.sum_x += sample.raw_left_x as i32 - report::AXIS_MIDPOINT as i32;
        self.sum_y += sample.raw_left_y as i32 - report::AXIS_MIDPOINT as i32;
        self.sum_rx += sample.raw_right_x as i32 - report::AXIS_MIDPOINT as i32;
        self.sum_ry += sample.raw_right_y as i32 - report::AXIS_MIDPOINT as i32;
        self.captured = true;
    }
}

/// How writes to the pad are faring.
///
/// Output reports are sent on the reader thread, and for a long time the result
/// of sending one was dropped on the floor. A pad that refuses a report then
/// looks exactly like a pad that accepted it, which is how a wrong layout stays
/// invisible: the write "succeeded" from the caller's side either way.
#[derive(Default)]
struct OutputHealth {
    failures: AtomicU64,
    last_error: Mutex<Option<String>>,
}

impl OutputHealth {
    fn failed(&self, error: String) {
        self.failures.fetch_add(1, Ordering::Relaxed);
        *self.last_error.lock() = Some(error);
    }

    /// A write went through, so any earlier complaint no longer applies.
    fn sent(&self) {
        *self.last_error.lock() = None;
    }

    fn failures(&self) -> u64 {
        self.failures.load(Ordering::Relaxed)
    }

    fn last_error(&self) -> Option<String> {
        self.last_error.lock().clone()
    }
}

/// Handle to the running reader thread. Dropping it stops the thread.
pub struct DeviceReader {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    snapshot: Arc<Mutex<DeviceSnapshot>>,
    out_tx: Arc<Mutex<Option<Sender<Vec<u8>>>>>,
    calibration: Arc<Mutex<Calibration>>,
    output: Arc<OutputHealth>,
}

impl DeviceReader {
    /// Start watching for a pad.
    ///
    /// `want_serial` pins to one specific pad; `None` grabs whichever connects
    /// first. `poll_rate_hz` is a hint used only when no reports are pending.
    pub fn start(want_serial: Option<String>, poll_rate_hz: u32) -> Result<Self, hidapi::HidError> {
        let api = Arc::new(hidapi::HidApi::new()?);
        let snapshot = Arc::new(Mutex::new(DeviceSnapshot::empty()));
        let calibration = Arc::new(Mutex::new(Calibration::default()));
        let output = Arc::new(OutputHealth::default());
        let out_tx: Arc<Mutex<Option<Sender<Vec<u8>>>>> = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));

        let thread = std::thread::Builder::new()
            .name("padforge-hid".into())
            .spawn({
                let stop = Arc::clone(&stop);
                let snapshot = Arc::clone(&snapshot);
                let calibration = Arc::clone(&calibration);
                let output = Arc::clone(&output);
                let out_tx = Arc::clone(&out_tx);
                move || {
                    run(
                        api,
                        want_serial,
                        poll_rate_hz,
                        stop,
                        snapshot,
                        calibration,
                        out_tx,
                        output,
                    )
                }
            })?;

        Ok(Self {
            stop,
            thread: Some(thread),
            snapshot,
            out_tx,
            calibration,
            output,
        })
    }

    /// Latest decoded frame.
    pub fn snapshot(&self) -> DeviceSnapshot {
        self.snapshot.lock().clone()
    }

    /// Queue one output report (lightbar colour and/or rumble).
    ///
    /// Returns false when no pad is open, which is not an error worth surfacing
    /// to the user: it just means the lightbar is momentarily unavailable.
    pub fn send_output(&self, buf: &[u8]) -> bool {
        match self.out_tx.lock().as_ref() {
            Some(tx) => tx.send(buf.to_vec()).is_ok(),
            None => false,
        }
    }

    /// Current per-axis resting offsets.
    pub fn calibration(&self) -> Calibration {
        *self.calibration.lock()
    }

    /// Discard captured offsets so the next resting sample rebuilds them.
    pub fn reset_calibration(&self) {
        *self.calibration.lock() = Calibration::default();
    }

    /// Reports the pad refused, if the last write failed. Cleared by a write
    /// that goes through, so a transient rejection does not linger as a
    /// permanent accusation.
    pub fn output_error(&self) -> Option<String> {
        self.output.last_error()
    }

    /// How many reports the pad has refused since the reader started.
    pub fn output_failures(&self) -> u64 {
        self.output.failures()
    }
}

impl Drop for DeviceReader {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// How often to re-scan the bus while no pad is present.
const RESCAN_INTERVAL: Duration = Duration::from_millis(250);

/// How long a candidate interface gets to produce a decodable report before it
/// is treated as silent.
///
/// Short, because this runs inside the connect path and a pad that is working
/// reports at up to 1000 Hz, so a single frame arrives in a millisecond. Three
/// seconds is already generous for a machine under load.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// How long a single read waits before returning empty, in milliseconds.
///
/// Short enough that the loop comes back around often to notice a shutdown
/// request or a stalled link, long enough that a 1000 Hz pad never misses its
/// turn and the loop spins.
const READ_TIMEOUT_MS: i32 = 50;

/// Attempts made at writing one output report before the refusal is kept.
///
/// More than one because a rejection here is usually the pad being momentarily
/// unavailable rather than the report being wrong, and fewer than the retry
/// would matter would mean reporting every one of those.
const OUTPUT_ATTEMPTS: u32 = 3;

/// Pause between write attempts. Long enough for the pad to finish what it was
/// doing, short enough that a genuine failure is still visible within a frame.
const OUTPUT_RETRY_BACKOFF: Duration = Duration::from_millis(4);

/// How long the pad may go without delivering a frame before it is treated as
/// disconnected.
///
/// Far longer than the gap between reports at any supported rate, because a
/// pad that has gone quiet for this long is not coming back on its own.
const STALL_TIMEOUT: Duration = Duration::from_secs(10);

/// Choose the interface that actually delivers reports.
///
/// A Bluetooth DS4 exposes several HID collections at once. Measured on a real
/// DS4 v2: interface 3 opens successfully and then never returns from `read`,
/// while interface -1 delivers every frame. Nothing in the device list
/// distinguishes them — same vendor id, same product id, same usage page and
/// usage — so the only reliable test is to read from each and see which one
/// speaks.
///
/// The first candidate is probed with a short deadline, and any that stays
/// silent is remembered so the remaining candidates are tried within the same
/// pass. When every candidate is silent the first is returned anyway, so a pad
/// that is merely slow to start still gets its reader thread rather than
/// silently disappearing.
fn pick_live_interface<'a>(
    api: &hidapi::HidApi,
    candidates: &[&'a hidapi::DeviceInfo],
    stop: &Arc<AtomicBool>,
) -> Option<&'a hidapi::DeviceInfo> {
    // Order the candidates so the most likely one is probed first, which is
    // what keeps the common case at one probe rather than several.
    let ordered = order_candidates(candidates);

    let mut dead: std::collections::HashSet<String> = std::collections::HashSet::new();
    for candidate in &ordered {
        if stop.load(Ordering::Relaxed) {
            return None;
        }
        let key = candidate.path().to_string_lossy().into_owned();
        if dead.contains(&key) {
            continue;
        }
        if interface_delivers_reports(api, candidate, stop) {
            if !dead.is_empty() {
                // Worth saying out loud: without this the reader looks like it
                // lost the pad, when in fact it walked past a silent interface.
                info!("skipped {} silent interface(s) before this one", dead.len());
            }
            return Some(candidate);
        }
        dead.insert(key);
    }

    // Nothing answered. Fall back to the first candidate so the reader still
    // runs: a pad that has just been woken may start reporting on the next pass,
    // and a reader thread with nothing to read is at least retrying.
    ordered.first().copied()
}

/// Order candidates by how likely each is to be the reporting interface.
fn order_candidates<'a>(candidates: &[&'a hidapi::DeviceInfo]) -> Vec<&'a hidapi::DeviceInfo> {
    let mut ordered: Vec<_> = candidates.to_vec();
    ordered.sort_by_key(|d| interface_rank(d.interface_number()));
    ordered
}

/// Whether one interface produces a decodable DS4 report within the timeout.
fn interface_delivers_reports(
    api: &hidapi::HidApi,
    candidate: &hidapi::DeviceInfo,
    stop: &Arc<AtomicBool>,
) -> bool {
    let Ok(path) = CString::new(candidate.path().to_string_lossy().as_ref()) else {
        return false;
    };
    // `read_timeout` takes `&self`, so no mutable binding is needed here.
    let Ok(device) = api.open_path(&path) else {
        return false;
    };

    let deadline = Instant::now() + PROBE_TIMEOUT;
    let mut buffer = [0u8; 256];
    while Instant::now() < deadline {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        // A short read timeout rather than a blocking one, so the deadline is
        // actually reachable. `read` on a silent collection otherwise never
        // returns, and this probe would hang for the life of the process.
        match device.read_timeout(&mut buffer, 50) {
            Ok(n) if n > 0 => {
                if crate::report::parse(&buffer[..n]).is_some() {
                    return true;
                }
            }
            Ok(_) => {}
            Err(_) => return false,
        }
    }
    false
}

#[allow(clippy::too_many_arguments)]
fn run(
    api: Arc<hidapi::HidApi>,
    want_serial: Option<String>,
    poll_rate_hz: u32,
    stop: Arc<AtomicBool>,
    snapshot: Arc<Mutex<DeviceSnapshot>>,
    calibration: Arc<Mutex<Calibration>>,
    out_tx: Arc<Mutex<Option<Sender<Vec<u8>>>>>,
    output: Arc<OutputHealth>,
) {
    let idle_backoff = interval_for_rate(poll_rate_hz);

    while !stop.load(Ordering::Relaxed) {
        // Collect every candidate rather than taking the first. A DS4 over
        // Bluetooth enumerates as more than one HID collection, and only one of
        // them carries gamepad reports. Opening a collection that carries none
        // succeeds and then blocks forever on read, which stalls the reader
        // thread and presents as a pad that enumerates and is then treated as
        // silent. Taking the first match therefore looks like a dead pad even
        // though the pad is reporting perfectly well on another interface.
        let candidates: Vec<_> = api
            .device_list()
            .filter(|d| {
                d.vendor_id() == report::SONY_VENDOR_ID
                    && report::DS4_PRODUCT_IDS.contains(&d.product_id())
                    && match (want_serial.as_deref(), d.serial_number()) {
                        (Some(want), Some(have)) => want == have,
                        (Some(_), None) => false,
                        (None, _) => true,
                    }
            })
            .collect();

        let Some(raw_info) = pick_live_interface(&api, &candidates, &stop) else {
            {
                let mut s = snapshot.lock();
                if s.connected {
                    info!("pad disconnected");
                    *s = DeviceSnapshot::empty();
                }
            }
            *out_tx.lock() = None;
            stop_wait(&stop, RESCAN_INTERVAL);
            continue;
        };

        let info = describe(raw_info);
        let path = match CString::new(info.path.as_str()) {
            Ok(p) => p,
            Err(_) => {
                warn!("pad path contains a NUL byte, skipping");
                stop_wait(&stop, RESCAN_INTERVAL);
                continue;
            }
        };

        let device = match api.open_path(&path) {
            Ok(d) => d,
            Err(e) => {
                warn!("could not open pad: {e}");
                stop_wait(&stop, RESCAN_INTERVAL);
                continue;
            }
        };

        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        *out_tx.lock() = Some(tx);

        info!(
            "connected: {} via {} ({})",
            info.label(),
            info.transport.label(),
            info.serial.as_deref().unwrap_or("no serial")
        );

        *snapshot.lock() = DeviceSnapshot {
            report: Ds4Report::neutral(),
            info: info.clone(),
            packets: 0,
            frame_ms: 0.0,
            connected: true,
        };

        // Discard any output queued while the handle was being opened.
        drain(&rx);

        let mut buf = [0u8; 128];
        let mut last_frame = Instant::now();
        let mut dead = false;
        // When the pad last actually delivered a frame. A read timeout returning
        // nothing is not itself a disconnect: at 1000 Hz a gap of a few hundred
        // milliseconds is normal.
        let mut last_report = Instant::now();

        while !stop.load(Ordering::Relaxed) && !dead {
            service_output(&device, &rx, &output);

            // A timeout, not a blocking read. Two reasons, both learned the hard
            // way: a silent interface blocks forever, and a pad that goes to
            // sleep or walks out of range stops reporting without the handle
            // reporting anything. Either way the loop needs to come back around
            // to notice the shutdown flag or the stall.
            match device.read_timeout(&mut buf, READ_TIMEOUT_MS) {
                Ok(0) => {
                    if last_report.elapsed() > STALL_TIMEOUT {
                        warn!(
                            "pad stopped reporting for {:?}, treating it as gone",
                            STALL_TIMEOUT
                        );
                        dead = true;
                    }
                    continue;
                }
                Ok(n) => {
                    let Some(mut rep) = report::parse(&buf[..n]) else {
                        continue;
                    };
                    rep.fresh = false;
                    last_report = Instant::now();

                    let cal = calibration.lock();
                    let (lx, ly, rx, ry) = cal.offsets();
                    drop(cal);
                    rep.left_x = recentre(rep.raw_left_x, lx);
                    rep.left_y = recentre(rep.raw_left_y, ly);
                    rep.right_x = recentre(rep.raw_right_x, rx);
                    rep.right_y = recentre(rep.raw_right_y, ry);

                    let now = Instant::now();
                    let frame_ms = now.duration_since(last_frame).as_secs_f32() * 1000.0;
                    last_frame = now;

                    let still = rep.buttons.raw() == 0;
                    {
                        let mut s = snapshot.lock();
                        s.report = rep;
                        s.packets += 1;
                        s.frame_ms = if s.packets == 1 { 0.0 } else { frame_ms };

                        // While nothing is pressed and the sticks are centred,
                        // refine the resting centre. Taking the snapshot under
                        // the same lock keeps the two consistent.
                        if still && is_centred(&s.report) {
                            calibration.lock().accumulate(&s.report);
                        }
                    }
                }
                Err(hidapi::HidError::IoError { .. })
                | Err(hidapi::HidError::HidApiError { .. }) => {
                    // The handle went away: unplugged, suspended, or reset.
                    dead = true;
                }
                Err(_) => {
                    // Nothing buffered yet. Back off so a dead pad does not spin.
                    if idle_backoff < Duration::from_millis(4) {
                        std::thread::sleep(Duration::from_millis(4));
                    }
                }
            }
        }

        *out_tx.lock() = None;
        if snapshot.lock().connected {
            debug!("pad link lost");
            *snapshot.lock() = DeviceSnapshot::empty();
        }
    }
}

/// Enumerate every connected DS4 without opening them.
pub fn enumerate() -> Vec<DeviceInfo> {
    let Ok(api) = hidapi::HidApi::new() else {
        return Vec::new();
    };
    api.device_list()
        .filter(|d| {
            d.vendor_id() == report::SONY_VENDOR_ID
                && report::DS4_PRODUCT_IDS.contains(&d.product_id())
        })
        .map(describe)
        .collect()
}

/// Write queued reports to the pad, and report the ones it refuses.
///
/// A rejected write used to be dropped here, which is the worst possible place
/// for it: the caller has already been told the report was queued, so a pad
/// that refuses everything looks exactly like a pad that is accepting
/// everything. Now the result decides whether the complaint clears or is kept.
///
/// Each write is attempted a few times before it counts. The pad turns down
/// output while it is waking over Bluetooth, and a single rejection is not
/// worth putting a warning on screen for; the one that keeps failing is, and
/// that is what `health` holds on to.
fn service_output(device: &hidapi::HidDevice, rx: &Receiver<Vec<u8>>, health: &OutputHealth) {
    loop {
        let buf = match rx.try_recv() {
            Ok(buf) => buf,
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => return,
        };

        let mut failure: Option<String> = None;
        for attempt in 1..=OUTPUT_ATTEMPTS {
            match device.send_output_report(&buf) {
                Ok(()) => {
                    health.sent();
                    failure = None;
                    break;
                }
                Err(e) => {
                    failure = Some(e.to_string());
                    if attempt < OUTPUT_ATTEMPTS {
                        std::thread::sleep(OUTPUT_RETRY_BACKOFF);
                    }
                }
            }
        }

        if let Some(message) = failure {
            warn!("the pad refused an output report: {message}");
            health.failed(message);
        }
    }
}

fn drain(rx: &Receiver<Vec<u8>>) {
    while rx.try_recv().is_ok() {}
}

fn stop_wait(stop: &AtomicBool, total: Duration) {
    // Wake early so shutdown is responsive even with a long rescan interval.
    let step = Duration::from_millis(25);
    let mut left = total;
    while left > Duration::ZERO && !stop.load(Ordering::Relaxed) {
        let s = step.min(left);
        std::thread::sleep(s);
        left -= s;
    }
}

fn describe(d: &hidapi::DeviceInfo) -> DeviceInfo {
    let transport = match d.bus_type() {
        hidapi::BusType::Bluetooth => Transport::Bluetooth,
        _ => Transport::Usb,
    };
    DeviceInfo {
        path: d.path().to_string_lossy().into_owned(),
        serial: d.serial_number().map(str::to_owned),
        product: d
            .product_string()
            .unwrap_or("DualShock 4")
            .trim()
            .to_owned(),
        vendor_id: d.vendor_id(),
        product_id: d.product_id(),
        transport,
    }
}

/// True when every stick axis is close enough to its resting centre that the
/// sample is a valid calibration reading.
fn is_centred(report: &Ds4Report) -> bool {
    let centre = report::AXIS_MIDPOINT as i32;
    let within = |raw: u8| (raw as i32 - centre).abs() <= 12;
    within(report.raw_left_x)
        && within(report.raw_left_y)
        && within(report.raw_right_x)
        && within(report.raw_right_y)
}

/// Re-centre a raw stick byte using a captured offset.
fn recentre(raw: u8, offset: i16) -> f32 {
    let centred = raw as i32 - report::AXIS_MIDPOINT as i32 - offset as i32;
    let v = if centred > 0 {
        centred as f32 / report::AXIS_HALF_RANGE as f32
    } else {
        centred as f32 / report::AXIS_MIDPOINT as f32
    };
    v.clamp(-1.0, 1.0)
}

/// Sleep interval matching a target polling rate.
pub fn interval_for_rate(hz: u32) -> Duration {
    Duration::from_micros(1_000_000 / hz.clamp(1, 1000) as u64)
}

/// The sort key deciding which interface is probed first.
///
/// Exposed as a free function so the ordering can be tested without a device
/// handle, which `hidapi::DeviceInfo` cannot be built without.
fn interface_rank(interface_number: i32) -> usize {
    // Bluetooth composite devices report real interface numbers, and the gamepad
    // collection is not always the lowest one. A negative number means no
    // interface, which on this pad is the top-level collection that does report,
    // so it has to sort ahead of every numbered interface.
    match interface_number {
        n if n < 0 => 0,
        n => 1 + n as usize,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Output reports are written on the reader thread and their result used to
    /// be dropped, which meant a pad refusing everything looked exactly like a
    /// pad accepting everything. These are the two halves of that: the refusal
    /// is kept, and a write that later goes through clears it.
    #[test]
    fn a_refused_report_is_kept_and_a_good_one_clears_it() {
        let health = OutputHealth::default();
        assert_eq!(health.failures(), 0);
        assert_eq!(health.last_error(), None);

        health.failed("Access denied".into());
        assert_eq!(health.failures(), 1);
        assert_eq!(health.last_error().as_deref(), Some("Access denied"));

        // A second refusal replaces the message rather than appending, so what
        // the UI shows is what is wrong now, not a history of what went wrong.
        health.failed("Device removed".into());
        assert_eq!(health.failures(), 2);
        assert_eq!(health.last_error().as_deref(), Some("Device removed"));

        // One good write is enough to stop complaining: the count stays as a
        // diagnostic, the message does not stay as an accusation.
        health.sent();
        assert_eq!(health.last_error(), None);
        assert_eq!(health.failures(), 2);
    }

    /// Measured on a real DS4 v2: interface 3 opens successfully and then never
    /// returns from read, while interface -1 delivers every frame. Taking the
    /// first match therefore blocks the reader on the silent one and the pad
    /// looks disconnected even though it is reporting perfectly well.
    #[test]
    fn top_level_interfaces_are_tried_before_composite_ones() {
        assert_eq!(interface_rank(-1), 0, "the top-level collection leads");
        assert!(
            interface_rank(-1) < interface_rank(3),
            "interface -1 must be probed before interface 3"
        );

        // Among numbered interfaces, the lowest number goes first, and the order
        // is total so the choice is deterministic.
        assert!(interface_rank(0) < interface_rank(1));
        assert!(interface_rank(2) < interface_rank(5));

        // The exact pair from the real pad, in the order the probe found them.
        let mut numbers = [3i32, -1];
        numbers.sort_by_key(|n| interface_rank(*n));
        assert_eq!(
            numbers[0], -1,
            "the reporting interface has to be probed first"
        );
    }

    /// The stall timeout must comfortably exceed the gap between reports at any rate
    /// the pad can be configured for, or a healthy pad gets declared gone while
    /// sitting still.
    ///
    /// The rate matters rather than being decorative: at 1000 Hz a frame arrives
    /// every millisecond, so a timeout of even a few hundred milliseconds is
    /// hundreds of missed frames, while at the low end the gap is orders of
    /// magnitude larger. Both ends are checked because the timeout has to clear
    /// them.
    #[test]
    fn stall_timeout_clearly_exceeds_the_report_interval() {
        for hz in [1u32, 10, 125, 250, 500, 1000] {
            let gap = interval_for_rate(hz);
            assert!(
                STALL_TIMEOUT > gap * 4,
                "at {hz} Hz the gap is {gap:?}, which the {STALL_TIMEOUT:?} timeout does not cover"
            );
        }
    }

    /// The read timeout is the floor on how long the reader takes to notice a
    /// shutdown request, so it has to stay small relative to the stall timeout:
    /// a timeout longer than the stall check would make the stall detection
    /// unreachable in practice.
    #[test]
    fn read_timeout_is_short_relative_to_the_stall_timeout() {
        let read = Duration::from_millis(READ_TIMEOUT_MS as u64);
        assert!(
            read * 20 < STALL_TIMEOUT,
            "a {read:?} read timeout means the {STALL_TIMEOUT:?} stall check is only \
             sampled every {read:?}, which is too coarse"
        );
    }

    #[test]
    fn enumerate_does_not_panic_without_hardware() {
        for d in enumerate() {
            assert_eq!(d.vendor_id, report::SONY_VENDOR_ID);
            assert!(report::DS4_PRODUCT_IDS.contains(&d.product_id));
        }
    }

    #[test]
    fn recentre_uses_offset() {
        let v = recentre((report::AXIS_MIDPOINT + 5) as u8, 5);
        assert!(v.abs() < 0.01, "got {v}");
        // Without the offset the same byte reads high.
        assert!(recentre((report::AXIS_MIDPOINT + 5) as u8, 0) > 0.03);
    }

    #[test]
    fn poll_interval_scales_with_rate() {
        assert_eq!(interval_for_rate(1000), Duration::from_micros(1000));
        assert_eq!(interval_for_rate(125), Duration::from_micros(8000));
    }

    #[test]
    fn label_falls_back_when_no_serial() {
        let d = DeviceInfo {
            path: String::new(),
            serial: None,
            product: "Wireless Controller".into(),
            vendor_id: 0x054C,
            product_id: 0x05C4,
            transport: Transport::Usb,
        };
        assert_eq!(d.label(), "Wireless Controller");
    }

    #[test]
    fn calibration_converges_towards_offset() {
        let mut cal = Calibration::default();
        let rep = Ds4Report {
            raw_left_x: (report::AXIS_MIDPOINT + 10) as u8,
            ..Ds4Report::neutral()
        };
        for _ in 0..500 {
            cal.accumulate(&rep);
        }
        assert_eq!(cal.count, 500);
        assert!((cal.left_x() - 10).abs() <= 1, "got {}", cal.left_x());
        // The untouched axes average out to zero.
        assert_eq!(cal.left_y(), 0);
        assert!(cal.is_captured());
    }

    #[test]
    fn stop_wait_returns_early_on_shutdown() {
        // The point is responsiveness, so the durations are deliberately tiny:
        // the sleep granularity is 25ms, and a test that genuinely waits its
        // timeout proves nothing that a one-step timeout does not, while costing
        // the whole suite its runtime.
        let step = Duration::from_millis(25);

        // Set part-way through: the wait must abandon the remainder. The flag is
        // shared through an Arc because an atomic cannot be handed to the thread
        // that sets it by value.
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let setter = Arc::clone(&stop);
        std::thread::spawn(move || {
            std::thread::sleep(step);
            setter.store(true, Ordering::Relaxed);
        });

        let start = std::time::Instant::now();
        // Four steps of headroom, so a loaded machine still gets to shut down.
        stop_wait(&stop, step * 4);
        let elapsed = start.elapsed();

        assert!(stop.load(Ordering::Relaxed), "the flag was never observed");
        assert!(
            elapsed < step * 4,
            "waited {elapsed:?}, which is the full timeout rather than an early exit"
        );
    }

    #[test]
    fn stop_wait_actually_waits_when_nothing_asks_it_to_stop() {
        // The companion to the test above. Without this, a `stop_wait` that
        // returned immediately would still pass the shutdown test, and shutdown
        // would silently stop waiting out a rescan at all.
        let step = Duration::from_millis(25);
        let stop = AtomicBool::new(false);

        let start = std::time::Instant::now();
        stop_wait(&stop, step);
        let elapsed = start.elapsed();

        assert!(!stop.load(Ordering::Relaxed));
        assert!(
            elapsed >= step,
            "returned after {elapsed:?} without being asked to stop"
        );
    }
}
