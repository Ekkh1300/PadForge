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
    pub fn label(&self) -> String {
        let base = if self.product.trim().is_empty() {
            "DualShock 4".to_string()
        } else {
            self.product.trim().to_string()
        };
        match self.serial.as_deref() {
            Some(s) if s.len() >= 4 => format!("{base} Â· {}", &s[s.len() - 4..]),
            Some(s) => format!("{base} Â· {s}"),
            None => base,
        }
    }
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
        (
            self.left_x(),
            self.left_y(),
            self.right_x(),
            self.right_y(),
        )
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

/// Handle to the running reader thread. Dropping it stops the thread.
pub struct DeviceReader {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    snapshot: Arc<Mutex<DeviceSnapshot>>,
    out_tx: Arc<Mutex<Option<Sender<Vec<u8>>>>>,
    calibration: Arc<Mutex<Calibration>>,
    battery_reads: Arc<AtomicU64>,
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
        let battery_reads = Arc::new(AtomicU64::new(0));
        let out_tx: Arc<Mutex<Option<Sender<Vec<u8>>>>> = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));

        let thread = std::thread::Builder::new()
            .name("padforge-hid".into())
            .spawn({
                let stop = Arc::clone(&stop);
                let snapshot = Arc::clone(&snapshot);
                let calibration = Arc::clone(&calibration);
                let battery_reads = Arc::clone(&battery_reads);
                let out_tx = Arc::clone(&out_tx);
                move || {
                    run(
                        api,
                        want_serial,
                        poll_rate_hz,
                        stop,
                        snapshot,
                        calibration,
                        battery_reads,
                        out_tx,
                    )
                }
            })?;

        Ok(Self {
            stop,
            thread: Some(thread),
            snapshot,
            out_tx,
            calibration,
            battery_reads,
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

    /// Number of battery feature reads performed, useful for diagnostics.
    pub fn battery_reads(&self) -> u64 {
        self.battery_reads.load(Ordering::Relaxed)
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

/// How often to poll for battery while a pad is idle.
const BATTERY_INTERVAL: Duration = Duration::from_secs(30);
/// How often to re-scan the bus while no pad is present.
const RESCAN_INTERVAL: Duration = Duration::from_millis(250);

#[allow(clippy::too_many_arguments)]
fn run(
    api: Arc<hidapi::HidApi>,
    want_serial: Option<String>,
    poll_rate_hz: u32,
    stop: Arc<AtomicBool>,
    snapshot: Arc<Mutex<DeviceSnapshot>>,
    calibration: Arc<Mutex<Calibration>>,
    battery_reads: Arc<AtomicU64>,
    out_tx: Arc<Mutex<Option<Sender<Vec<u8>>>>>,
) {
    let idle_backoff = interval_for_rate(poll_rate_hz);

    while !stop.load(Ordering::Relaxed) {
        let found = api.device_list().find(|d| {
            d.vendor_id() == report::SONY_VENDOR_ID
                && report::DS4_PRODUCT_IDS.contains(&d.product_id())
                && match (want_serial.as_deref(), d.serial_number()) {
                    (Some(want), Some(have)) => want == have,
                    (Some(_), None) => false,
                    (None, _) => true,
                }
        });

        let Some(raw_info) = found else {
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
        let mut last_battery = Instant::now();
        let mut dead = false;

        while !stop.load(Ordering::Relaxed) && !dead {
            service_output(&device, &rx);

            match device.read(&mut buf) {
                Ok(n) => {
                    let Some(mut rep) = report::parse(&buf[..n]) else {
                        continue;
                    };
                    rep.fresh = false;

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

                    if last_battery.elapsed() > BATTERY_INTERVAL {
                        last_battery = now;
                        battery_reads.fetch_add(1, Ordering::Relaxed);
                        if let Some(level) = read_battery(&device) {
                            let mut s = snapshot.lock();
                            report::apply_battery(&mut s.report, level);
                        }
                    }
                }
                Err(hidapi::HidError::IoError { .. }) | Err(hidapi::HidError::HidApiError { .. }) => {
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
            d.vendor_id() == report::SONY_VENDOR_ID && report::DS4_PRODUCT_IDS.contains(&d.product_id())
        })
        .map(describe)
        .collect()
}

/// Ask the pad for its battery level via feature report `0x81`.
fn read_battery(device: &hidapi::HidDevice) -> Option<u8> {
    let mut buf = [0u8; 64];
    buf[0] = 0x81;
    device.get_feature_report(&mut buf).ok()?;
    // Byte 0 is the report id, byte 1 padding, byte 2 the status nibble.
    buf.get(2).copied()
}

fn service_output(device: &hidapi::HidDevice, rx: &Receiver<Vec<u8>>) {
    loop {
        match rx.try_recv() {
            Ok(buf) => {
                let _ = device.send_output_report(&buf);
            }
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => return,
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

#[cfg(test)]
mod tests {
    use super::*;

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