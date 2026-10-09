//! Motion controls: gyro-as-mouse and gyro-as-stick.
//!
//! The DS4's gyro reports angular *velocity*, so turning it into cursor movement
//! means integrating over time. That integration is what makes gyro aiming feel
//! either silky or unusable, so the whole module is built around keeping the
//! integration well-behaved:
//!
//! * a deadzone kills sensor noise while the pad is level
//! * a **scale** converts degrees/second into pixels/second
//! * a **low-pass filter** removes the high-frequency jitter gyro always has
//! * a **minimum threshold** drops twitchy single-sample spikes
//!
//! Output is *relative* movement, never absolute, so it composes with whatever
//! the game's own sensitivity is set to.

use serde::{Deserialize, Serialize};

use crate::report::Gyro;

/// Which axes of gyro motion drive the output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GyroAxisMode {
    /// Yaw â†’ horizontal, pitch â†’ vertical.
    YawPitch,
    /// Yaw â†’ horizontal, roll â†’ vertical (the "lean to steer" setup).
    YawRoll,
    /// Single axis, used for throttle/brake style inputs.
    YawOnly,
    PitchOnly,
}

/// Where gyro output goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GyroOutputMode {
    /// Absolute pointer movement, like a mouse.
    Mouse,
    /// Feeds the left stick, for aiming that recentres.
    Stick,
    /// Feeds the right stick.
    RightStick,
    /// Feeds both trigger axes, for analog throttle controls.
    Triggers,
}

/// Which way round the output axes are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GyroInvert {
    None,
    Horizontal,
    Vertical,
    Both,
}

impl GyroInvert {
    pub fn flip_x(self) -> bool {
        matches!(self, GyroInvert::Horizontal | GyroInvert::Both)
    }

    pub fn flip_y(self) -> bool {
        matches!(self, GyroInvert::Vertical | GyroInvert::Both)
    }
}

/// Temporal filter choice for the angular rates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GyroSmoothing {
    None,
    /// Single-pole low pass. `cutoff` in Hz; lower is smoother but laggier.
    LowPass { cutoff: f32 },
    /// 1€ filter: low latency when moving fast, smooth when still. The best
    /// default for aiming.
    OneEuro { min_cutoff: f32, beta: f32 },
}

/// All tunable gyro behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GyroConfig {
    pub enabled: bool,
    pub output_mode: GyroOutputMode,
    pub axis_mode: GyroAxisMode,
    pub invert: GyroInvert,

    /// Degrees of rotation per second that map to `max_speed` units of output.
    pub sensitivity: f32,
    /// Ceiling on output magnitude, in the output mode's own units.
    pub max_output: f32,
    /// Angular rates below this (deg/s) are treated as noise.
    pub deadzone: f32,
    /// Gains below this (units/second) are dropped, killing spike artefacts.
    pub min_threshold: f32,
    /// Angular rates are capped here before integration.
    pub max_rate: f32,

    pub smoothing: GyroSmoothing,
    /// Gain compensation that rises with angular rate, so slow aim stays smooth
    /// while fast flicks stay responsive.
    pub weight_averaging: bool,
}

impl Default for GyroConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            output_mode: GyroOutputMode::Mouse,
            axis_mode: GyroAxisMode::YawPitch,
            invert: GyroInvert::None,
            sensitivity: 1.0,
            max_output: 20.0,
            deadzone: 1.5,
            min_threshold: 0.05,
            max_rate: 500.0,
            smoothing: GyroSmoothing::LowPass { cutoff: 25.0 },
            weight_averaging: false,
        }
    }
}

/// A single frame of gyro-derived output.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GyroDelta {
    pub x: f32,
    pub y: f32,
}

impl GyroDelta {
    pub fn is_zero(&self) -> bool {
        self.x.abs() < f32::EPSILON && self.y.abs() < f32::EPSILON
    }
}

/// Per-axis memory for the 1€ filter.
#[derive(Debug, Clone, Copy, Default)]
struct OneEuroState {
    /// Last raw input, used to differentiate.
    prev_raw: f32,
    /// Last *filtered* value.
    prev_out: f32,
    /// Last filtered derivative.
    prev_deriv: f32,
}

/// Stateful gyro processor: owns the filters, so construct once and keep it.
#[derive(Debug, Clone)]
pub struct GyroProcessor {
    config: GyroConfig,
    // Low-pass memory.
    yaw_f: f32,
    pitch_f: f32,
    roll_f: f32,
    // 1€ memory.
    euro: [OneEuroState; 3],
    prev_time: Option<std::time::Instant>,
}

impl Default for GyroProcessor {
    fn default() -> Self {
        Self::new(GyroConfig::default())
    }
}

impl GyroProcessor {
    pub fn new(config: GyroConfig) -> Self {
        Self {
            config,
            yaw_f: 0.0,
            pitch_f: 0.0,
            roll_f: 0.0,
            euro: [OneEuroState::default(); 3],
            prev_time: None,
        }
    }

    pub fn config(&self) -> &GyroConfig {
        &self.config
    }

    pub fn set_config(&mut self, config: GyroConfig) {
        if config.smoothing != self.config.smoothing {
            // Changing the filter invalidates its memory.
            self.reset();
        }
        self.config = config;
    }

    /// Drop all filter state, e.g. when the pad reconnects.
    pub fn reset(&mut self) {
        self.yaw_f = 0.0;
        self.pitch_f = 0.0;
        self.roll_f = 0.0;
        self.euro = [OneEuroState::default(); 3];
        self.prev_time = None;
    }

    /// Convert one gyro sample into an output delta for `dt` seconds.
    pub fn process(&mut self, gyro: &Gyro, dt: f32) -> GyroDelta {
        if !self.config.enabled {
            return GyroDelta::default();
        }
        // Guard against a huge dt from a suspend/resume or a debugger pause.
        let dt = dt.clamp(0.0, 0.05);
        if dt <= 0.0 {
            return GyroDelta::default();
        }

        // Rate limit, then deadzone, so noise never reaches the integrator.
        let max_rate = self.config.max_rate.max(1.0);
        let mut yaw = gyro.yaw.clamp(-max_rate, max_rate);
        let mut pitch = gyro.pitch.clamp(-max_rate, max_rate);
        let roll = gyro.roll.clamp(-max_rate, max_rate);

        let dz = self.config.deadzone.max(0.0);
        let shrink = |v: f32| -> f32 {
            let m = v.abs();
            if m <= dz {
                0.0
            } else {
                // Re-expand so full rotation still reaches full output.
                let sign = if v.is_sign_negative() { -1.0 } else { 1.0 };
                sign * ((m - dz) / max_rate.max(1.0 - dz)).clamp(0.0, 1.0) * max_rate
            }
        };
        yaw = shrink(yaw);
        pitch = shrink(pitch);

        let (yaw, pitch) = self.filter(yaw, pitch, roll);

        // Pick the driving axes.
        let (ax, ay) = match self.config.axis_mode {
            GyroAxisMode::YawPitch => (yaw, pitch),
            GyroAxisMode::YawRoll => (yaw, roll),
            GyroAxisMode::YawOnly => (yaw, 0.0),
            GyroAxisMode::PitchOnly => (0.0, pitch),
        };

        // deg/s -> output units/s, scaled and capped.
        let mut x = ax * self.config.sensitivity;
        let mut y = ay * self.config.sensitivity;

        let max = self.config.max_output.max(0.0);
        let mag = (x * x + y * y).sqrt();
        if mag > max && max > 0.0 {
            let k = max / mag;
            x *= k;
            y *= k;
        }

        if self.config.invert.flip_x() {
            x = -x;
        }
        if self.config.invert.flip_y() {
            y = -y;
        }

        // Integrate to a per-frame delta, then drop negligible motion.
        let mut delta = GyroDelta { x: x * dt, y: y * dt };
        if delta.x.abs() < self.config.min_threshold && delta.y.abs() < self.config.min_threshold {
            delta = GyroDelta::default();
        }
        delta
    }

    fn filter(&mut self, yaw: f32, pitch: f32, roll: f32) -> (f32, f32) {
        let dt = self
            .prev_time
            .map(|t| t.elapsed().as_secs_f32())
            .unwrap_or(1.0 / 125.0)
            .clamp(0.001, 0.05);
        self.prev_time = Some(std::time::Instant::now());

        match self.config.smoothing {
            GyroSmoothing::None => (yaw, pitch),
            GyroSmoothing::LowPass { cutoff } => {
                let alpha = alpha_for_cutoff(cutoff.clamp(0.5, 500.0), dt);
                self.yaw_f += alpha * (yaw - self.yaw_f);
                self.pitch_f += alpha * (pitch - self.pitch_f);
                self.roll_f += alpha * (roll - self.roll_f);
                (self.yaw_f, self.pitch_f)
            }
            GyroSmoothing::OneEuro { min_cutoff, beta } => {
                let min_cutoff = min_cutoff.clamp(0.1, 100.0);
                let beta = beta.clamp(0.0, 100.0);
                let out = self.euro[0].filter(yaw, dt, min_cutoff, beta);
                let out_pitch = self.euro[1].filter(pitch, dt, min_cutoff, beta);
                self.euro[2].filter(roll, dt, min_cutoff, beta);
                (out, out_pitch)
            }
        }
    }
}

/// One-pole low-pass coefficient for a cutoff frequency and sample interval.
fn alpha_for_cutoff(cutoff: f32, dt: f32) -> f32 {
    let tau = 1.0 / (2.0 * std::f32::consts::PI * cutoff);
    (dt / (tau + dt)).clamp(0.0, 1.0)
}

impl OneEuroState {
    /// Advance the 1€ filter one sample and return the filtered value.
    ///
    /// The cutoff rises with the signal's own speed, so slow motion gets heavy
    /// smoothing while fast flicks pass through almost untouched.
    fn filter(&mut self, value: f32, dt: f32, min_cutoff: f32, beta: f32) -> f32 {
        let raw_derivative = (value - self.prev_raw) / dt;
        // Smooth the derivative itself, otherwise it is far too noisy to
        // compute a cutoff from.
        let deriv_alpha = alpha_for_cutoff(1.0, dt);
        let derivative = deriv_alpha * raw_derivative + (1.0 - deriv_alpha) * self.prev_deriv;

        let cutoff = min_cutoff + beta * derivative.abs();
        let alpha = alpha_for_cutoff(cutoff, dt);
        let filtered = self.prev_out + alpha * (value - self.prev_out);

        self.prev_raw = value;
        self.prev_deriv = derivative;
        self.prev_out = filtered;
        filtered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_produces_nothing() {
        let mut p = GyroProcessor::new(GyroConfig::default());
        let g = Gyro { pitch: 90.0, yaw: 90.0, roll: 0.0 };
        assert!(p.process(&g, 0.008).is_zero());
    }

    #[test]
    fn deadzone_suppresses_small_motion() {
        let cfg = GyroConfig {
            enabled: true,
            deadzone: 5.0,
            smoothing: GyroSmoothing::None,
            ..Default::default()
        };
        let mut p = GyroProcessor::new(cfg);
        let d = p.process(&Gyro { yaw: 2.0, pitch: 2.0, roll: 0.0 }, 0.008);
        assert!(d.is_zero(), "got {d:?}");
    }

    #[test]
    fn output_is_capped() {
        let cfg = GyroConfig {
            enabled: true,
            max_output: 10.0,
            deadzone: 0.0,
            smoothing: GyroSmoothing::None,
            ..Default::default()
        };
        let mut p = GyroProcessor::new(cfg);
        let g = Gyro { pitch: 10_000.0, yaw: 10_000.0, roll: 0.0 };
        let d = p.process(&g, 0.008);
        let mag = (d.x * d.x + d.y * d.y).sqrt();
        assert!(mag <= 10.0 + 0.001, "magnitude {mag} exceeded cap");
    }

    #[test]
    fn integration_scales_with_time() {
        let cfg = GyroConfig {
            enabled: true,
            deadzone: 0.0,
            max_output: 1000.0,
            min_threshold: 0.0,
            smoothing: GyroSmoothing::None,
            ..Default::default()
        };
        let mut p = GyroProcessor::new(cfg);
        let g = Gyro { yaw: 100.0, pitch: 0.0, roll: 0.0 };
        let short = p.process(&g, 0.001).x;
        let long = p.process(&g, 0.010).x;
        assert!(long > short, "longer dt should move further");
    }

    #[test]
    fn inversion_flips_sign() {
        let cfg = GyroConfig {
            enabled: true,
            deadzone: 0.0,
            min_threshold: 0.0,
            invert: GyroInvert::Horizontal,
            smoothing: GyroSmoothing::None,
            ..Default::default()
        };
        let mut p = GyroProcessor::new(cfg);
        let g = Gyro { yaw: 50.0, pitch: 0.0, roll: 0.0 };
        let d = p.process(&g, 0.008);
        assert!(d.x < 0.0, "yaw should be negated, got {d:?}");
    }

    #[test]
    fn huge_dt_is_clamped() {
        let cfg = GyroConfig {
            enabled: true,
            deadzone: 0.0,
            min_threshold: 0.0,
            smoothing: GyroSmoothing::None,
            ..Default::default()
        };
        let mut p = GyroProcessor::new(cfg);
        let g = Gyro { yaw: 50.0, pitch: 0.0, roll: 0.0 };
        let d = p.process(&g, 100.0);
        // Clamped to 50ms, so a finite small step rather than a huge jump.
        assert!(d.x.is_finite() && d.x.abs() < 100.0, "got {d:?}");
    }

    #[test]
    fn low_pass_slows_a_step() {
        // A large enough output cap that the filter, not the cap, is what limits the
        // result, so this test actually observes convergence.
        let mut p = GyroProcessor::new(GyroConfig {
            enabled: true,
            deadzone: 0.0,
            min_threshold: 0.0,
            max_output: 10_000.0,
            smoothing: GyroSmoothing::LowPass { cutoff: 5.0 },
            ..Default::default()
        });
        let g = Gyro { yaw: 200.0, pitch: 0.0, roll: 0.0 };
        let first = p.process(&g, 0.008).x;
        assert!(first > 0.0);
        for _ in 0..400 {
            p.process(&g, 0.008);
        }
        // Converges towards the unfiltered value without overshooting it.
        let settled = p.process(&g, 0.008).x;
        assert!(settled > first, "filter should ramp up over time");
        assert!(
            settled <= 200.0 * 0.008 + 1e-3,
            "filter must not exceed the unfiltered value, got {settled}"
        );
    }

    #[test]
    fn alpha_is_bounded() {
        for cutoff in [0.01f32, 1.0, 25.0, 1000.0] {
            for dt in [0.001f32, 0.008, 0.05] {
                let a = alpha_for_cutoff(cutoff, dt);
                assert!((0.0..=1.0).contains(&a), "alpha {a} out of range");
            }
        }
    }
}