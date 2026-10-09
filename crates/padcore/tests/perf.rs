//! Throughput of the per-report hot path.
//!
//! The engine runs once per HID report, which is up to 1000 Hz on a DS4 over
//! USB. Every stage below sits in that loop, so each has a millisecond a second
//! to spend. These are ordinary tests so they run with `cargo test`, but they are
//! ignored by default: they are measurements, not correctness checks.
//!
//! Each figure subtracts a trivial loop's cost first, because at these speeds the
//! harness overhead is the same order as the work being measured.
//!
//! Run with:
//!     cargo test -p padcore --release --test perf -- --ignored --nocapture

use std::time::Instant;

use padcore::filters::{AxisFilter, Curve, Smoothing};
use padcore::gyro::GyroProcessor;
use padcore::pointer::{GyroPointer, PointerConfig};
use padcore::report::{Gyro, TouchState};
use padcore::touchpad::TouchpadProcessor;

/// Samples per measurement.
const SAMPLES: usize = 200_000;

/// Nanoseconds per call for `f`.
///
/// No baseline is subtracted. An earlier version did, and it produced negative
/// figures for the cheapest stages, which is a sign the subtraction was larger
/// than the measurement: a `sin` in the reference loop costs more than an entire
/// filter stage. Timing the loop alone is both simpler and more honest, since
/// every measurement here runs the identical loop shape and so pays the same
/// per-iteration cost.
///
/// `FnMut` because every stage measured here is stateful: they each hold filter
/// memory, which is exactly the work being timed.
fn ns_per_sample(mut f: impl FnMut(usize)) -> f64 {
    let start = Instant::now();
    for i in 0..SAMPLES {
        f(i);
    }
    let total = start.elapsed().as_secs_f64();
    std::hint::black_box(total);
    (total / SAMPLES as f64) * 1e9
}

/// The floor: what an empty call costs, so the reader can judge the figures.
fn loop_floor_ns() -> f64 {
    ns_per_sample(|_| {})
}

fn report(name: &str, ns: f64, budget_ns: f64) -> bool {
    let verdict = if ns <= budget_ns { "ok  " } else { "SLOW" };
    println!("  {verdict} {name:<36} {ns:>8.1} ns  (budget {budget_ns:.0})");
    ns <= budget_ns
}

/// A touchpad processor that is actually switched on.
///
/// Same reasoning as [`running_gyro`]: the default mode is `Off`, and `process`
/// returns immediately in that case, so the default would measure nothing.
fn running_touchpad() -> TouchpadProcessor {
    TouchpadProcessor::new(padcore::touchpad::TouchpadConfig {
        mode: padcore::touchpad::TouchpadMode::Gestures,
        ..Default::default()
    })
}

/// A gyro processor that is actually switched on.
///
/// The default config has `enabled: false`, and `process` returns immediately in
/// that case, so benchmarking the default would measure an early return rather
/// than the work a user with motion aiming switched on actually pays for.
fn running_gyro() -> GyroProcessor {
    GyroProcessor::new(padcore::gyro::GyroConfig {
        enabled: true,
        ..Default::default()
    })
}

/// A filter shaped the way a real stick axis is: a small deadzone, a bezier
/// response, and temporal smoothing so sensor noise does not reach the stick.
fn configured_filter(smoothing: Smoothing) -> AxisFilter {
    // Built through `new()` then configured, because the struct also holds
    // private runtime state that a literal would leave unset.
    let mut filter = AxisFilter::new();
    filter.deadzone = 0.08;
    filter.curve = Curve::Bezier {
        x: [0.0, 0.4, 1.0],
        y: [0.0, 0.06, 1.0],
    };
    filter.anti_deadzone = true;
    filter.smoothing = smoothing;
    filter.sensitivity = 1.6;
    filter
}

#[test]
#[ignore = "performance measurement; run with --ignored --nocapture"]
fn hot_path_stages_are_within_budget() {
    println!("\nper-sample cost of the 1 kHz loop ({SAMPLES} samples each):");
    println!("  floor for an empty call: {:.1} ns\n", loop_floor_ns());

    // The axis chain. Four of these run per report, so this is the stage that
    // matters most.
    let mut ema = configured_filter(Smoothing::Exponential { alpha: 0.25 });
    let ema_ns = ns_per_sample(|i| {
        let raw = ((i % 200) as f32 / 100.0) - 1.0;
        let _ = std::hint::black_box(ema.apply(raw));
    });

    // The most expensive smoothing: a sliding window that shifts on every
    // sample, which is the case that would turn an O(1) stage into an O(n) one.
    let mut windowed = configured_filter(Smoothing::WeightedAverage { window: 16 });
    let window_ns = ns_per_sample(|i| {
        let raw = ((i % 200) as f32 / 100.0) - 1.0;
        let _ = std::hint::black_box(windowed.apply(raw));
    });

    let mut gyro = running_gyro();
    let motion = Gyro {
        yaw: 0.4,
        pitch: -0.2,
        roll: 0.1,
    };
    let gyro_ns = ns_per_sample(|_| {
        let _ = std::hint::black_box(gyro.process(&motion, 0.001));
    });

    let mut pointer = GyroPointer::new(PointerConfig::default());
    let pointer_ns = ns_per_sample(|_| {
        let _ = std::hint::black_box(pointer.process(&motion, 0.001));
    });

    let mut touch = running_touchpad();
    let fingers = TouchState {
        pad_touched: true,
        ..Default::default()
    };
    let touch_ns = ns_per_sample(|_| {
        let _ = std::hint::black_box(touch.process(&fingers));
    });

    // A generous ceiling: these all measure in tens of nanoseconds, so a budget
    // of a microsecond still catches a change that turns a subtraction into an
    // allocation without flaking on a loaded machine.
    const BUDGET_NS: f64 = 1_000.0;

    let mut ok = true;
    ok &= report("axis filter (exponential smoothing)", ema_ns, BUDGET_NS);
    ok &= report(
        "axis filter (16-wide weighted window)",
        window_ns,
        BUDGET_NS,
    );
    ok &= report("gyro integrate + filter", gyro_ns, BUDGET_NS);
    ok &= report("gyro to pointer", pointer_ns, BUDGET_NS);
    ok &= report("touchpad routing", touch_ns, BUDGET_NS);
    println!();

    assert!(ok, "a hot-path stage exceeded its per-sample budget");
}

/// The whole chain as the engine drives it, against the real frame budget.
///
/// A DS4 over USB reports at 1000 Hz, so a frame is 1 ms. Four axes, the gyro,
/// and the pointer all have to fit inside that, several times over, because the
/// HID thread is not the only thing on the machine.
#[test]
#[ignore = "performance measurement; run with --ignored --nocapture"]
fn full_loop_clears_one_millisecond_per_frame() {
    const FRAME_BUDGET_NS: f64 = 1_000_000.0;
    const FRAMES: usize = 50_000;

    let mut filters = [
        configured_filter(Smoothing::Exponential { alpha: 0.25 }),
        configured_filter(Smoothing::Exponential { alpha: 0.25 }),
        configured_filter(Smoothing::Off),
        configured_filter(Smoothing::Off),
    ];
    let mut gyro = running_gyro();
    let mut pointer = GyroPointer::new(PointerConfig::default());
    let motion = Gyro {
        yaw: 0.3,
        pitch: -0.15,
        roll: 0.05,
    };

    let start = Instant::now();
    for i in 0..FRAMES {
        let t = i as f32 / 100.0;
        // Two sticks and two triggers, moving the way a hand moves them.
        let raw = [
            t.sin() * 0.9,
            t.cos() * 0.9,
            ((i % 90) as f32 / 100.0) - 0.45,
            ((i % 180) as f32 / 200.0) - 0.45,
        ];
        for (filter, value) in filters.iter_mut().zip(raw) {
            let _ = std::hint::black_box(filter.apply(value));
        }
        let _ = std::hint::black_box(gyro.process(&motion, 0.001));
        let _ = std::hint::black_box(pointer.process(&motion, 0.001));
    }
    let elapsed = start.elapsed().as_secs_f64();

    let per_frame = elapsed / FRAMES as f64 * 1e9;
    println!("\nfull loop over {FRAMES} frames:");
    println!("  measured {per_frame:.0} ns/frame");
    println!("  budget   {FRAME_BUDGET_NS:.0} ns/frame (1 kHz report rate)");
    println!("  headroom {:.0}x", FRAME_BUDGET_NS / per_frame);
    println!(
        "  of one core: {:.4} %\n",
        elapsed / FRAMES as f64 * 100_000.0
    );

    assert!(
        per_frame < FRAME_BUDGET_NS,
        "a frame took {per_frame:.0} ns, over the {FRAME_BUDGET_NS:.0} ns budget"
    );
}

/// Allocation behaviour on the hot path.
///
/// A per-report allocation would be invisible in a throughput number that still
/// fits the budget, but it would show up as jitter in the HID thread, which is
/// exactly what causes dropped or late reports. So this counts allocations
/// directly, with a counting global allocator installed for the duration.
#[test]
fn hot_path_does_not_allocate() {
    // A global allocator can only be installed once per program, so the counter
    // is always active and read selectively. It counts only allocations that
    // happen while the flag is set, which keeps other tests unaffected.
    static COUNTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    static ALLOCS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    /// Wraps the real allocator and tallies the calls made while armed.
    struct Counting;

    unsafe impl std::alloc::GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
            if COUNTING.load(std::sync::atomic::Ordering::Relaxed) {
                ALLOCS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            std::alloc::System.alloc(layout)
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
            std::alloc::System.dealloc(ptr, layout)
        }
        unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
            if COUNTING.load(std::sync::atomic::Ordering::Relaxed) {
                ALLOCS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            std::alloc::System.alloc_zeroed(layout)
        }
        unsafe fn realloc(
            &self,
            ptr: *mut u8,
            layout: std::alloc::Layout,
            new_size: usize,
        ) -> *mut u8 {
            if COUNTING.load(std::sync::atomic::Ordering::Relaxed) {
                ALLOCS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            std::alloc::System.realloc(ptr, layout, new_size)
        }
    }

    #[global_allocator]
    static ALLOCATOR: Counting = Counting;

    use std::sync::atomic::Ordering;

    // Warm every stage up first: the first call to each may legitimately
    // allocate (filling the sliding window, for instance). What matters is that
    // a steady-state loop does not.
    let mut ema = configured_filter(Smoothing::Exponential { alpha: 0.25 });
    let mut windowed = configured_filter(Smoothing::WeightedAverage { window: 16 });
    let mut gyro = running_gyro();
    let mut pointer = GyroPointer::new(PointerConfig::default());
    let mut touch = running_touchpad();
    let motion = Gyro {
        yaw: 0.4,
        pitch: -0.2,
        roll: 0.1,
    };
    let fingers = TouchState {
        pad_touched: true,
        ..Default::default()
    };

    for i in 0..64 {
        let raw = (i as f32 / 32.0) - 1.0;
        let _ = std::hint::black_box(ema.apply(raw));
        let _ = std::hint::black_box(windowed.apply(raw));
        let _ = std::hint::black_box(gyro.process(&motion, 0.001));
        let _ = std::hint::black_box(pointer.process(&motion, 0.001));
        let _ = std::hint::black_box(touch.process(&fingers));
    }

    COUNTING.store(true, Ordering::Relaxed);
    let before = ALLOCS.load(Ordering::Relaxed);
    for i in 0..10_000 {
        let raw = ((i % 200) as f32 / 100.0) - 1.0;
        let _ = std::hint::black_box(ema.apply(raw));
        let _ = std::hint::black_box(windowed.apply(raw));
        let _ = std::hint::black_box(gyro.process(&motion, 0.001));
        let _ = std::hint::black_box(pointer.process(&motion, 0.001));
        let _ = std::hint::black_box(touch.process(&fingers));
    }
    let after = ALLOCS.load(Ordering::Relaxed);
    COUNTING.store(false, Ordering::Relaxed);

    let allocated = after - before;
    assert_eq!(
        allocated, 0,
        "the hot path allocated {allocated} time(s) over 10,000 reports; \
         a steady-state HID loop must not allocate"
    );
}
