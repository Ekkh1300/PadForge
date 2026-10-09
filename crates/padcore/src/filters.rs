//! Per-axis shaping.
//!
//! A stick axis arrives as a noisy -1..=1 float. Before it reaches a game it
//! goes through a fixed four-stage chain, applied in this order:
//!
//! 1. **deadzone**   â€” discard drift around centre
//! 2. **curve**     â€” remap the remaining travel through a response curve
//! 3. **anti-deadzone** â€” re-expand so the full range is still reachable
//! 4. **smoothing** â€” temporal low-pass, to kill sensor noise
//!
//! Keeping them separate (rather than one blended function) means each stage is
//! independently tunable from the UI, which is what makes the result feel right
//! per game.

use serde::{Deserialize, Serialize};

use crate::formula::{Formula, Inputs};

/// Rescale `value` so it reaches a true 0..=1 across the full active range.
fn unit(value: f32, dz: f32) -> f32 {
    if value <= 0.0 {
        0.0
    } else {
        ((value - dz) / (1.0 - dz)).clamp(0.0, 1.0)
    }
}

/// The response curve applied to one axis.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Curve {
    /// Straight line, the honest default.
    #[default]
    Linear,
    /// Gentle expo: fine control near centre, full speed at the edge.
    Exponential {
        /// Exponent. 1.0 is linear, 3.0 is very soft near centre.
        exponent: f32,
    },
    /// The inverse: quick initial travel, then a long tail.
    #[allow(clippy::upper_case_acronyms)]
    Bezier {
        /// Control-point x positions, ascending, in 0..=1.
        x: [f32; 3],
        /// Control-point y positions, ascending, in 0..=1.
        y: [f32; 3],
    },
    /// Snaps the axis to 0 or +/-1 once past `threshold`. Great for racing wheels.
    Stepped { threshold: f32 },
}

impl Curve {
    /// A hand-tuned expo curve, the classic "raise the exponent to aim better".
    pub fn exponential(exponent: f32) -> Self {
        Curve::Exponential {
            exponent: exponent.clamp(1.0, 8.0),
        }
    }

    /// Smooth S-curve: slow start, fast middle, soft landing.
    pub fn smooth() -> Self {
        Curve::Bezier {
            x: [0.0, 0.4, 1.0],
            y: [0.0, 0.75, 1.0],
        }
    }

    /// How fast the control catches up with the raw input.
    pub fn apply(&self, v: f32) -> f32 {
        match *self {
            Curve::Linear => v,
            Curve::Exponential { exponent } => v.powf(exponent),
            Curve::Stepped { threshold } => {
                if v >= threshold {
                    1.0
                } else {
                    0.0
                }
            }
            Curve::Bezier { x, y } => bezier_eval(v, x, y),
        }
    }
}

/// Sample a cubic bezier defined by control points on both axes.
///
/// `x` gives the parameter positions and `y` the output, so the caller can
/// reshape the curve independently of how it is traversed. Returns `v` clamped
/// when the control points are degenerate (would divide by zero).
pub fn bezier_eval(v: f32, x: [f32; 3], y: [f32; 3]) -> f32 {
    let t = solve_param(v, x);
    cubic(y[0], y[1], y[2], t)
}

/// Given where the curve is on the x axis, find the bezier parameter t.
fn solve_param(v: f32, x: [f32; 3]) -> f32 {
    let span = x[2] - x[0];
    if span.abs() < f32::EPSILON {
        return v;
    }
    let v = v.clamp(x[0], x[2]);
    // Newton-Raphson converges in a couple of steps for well-formed curves.
    let mut t = (v - x[0]) / span;
    for _ in 0..8 {
        let xt = cubic(x[0], x[1], x[2], t);
        let dx = 3.0 * (1.0 - t) * (1.0 - t) * (x[1] - x[0])
            + 6.0 * (1.0 - t) * t * (x[2] - x[1])
            + 3.0 * t * t * (x[2] - x[0]);
        if dx.abs() < 1e-6 {
            break;
        }
        t += (v - xt) / dx;
        t = t.clamp(0.0, 1.0);
    }
    t
}

fn cubic(p0: f32, p1: f32, p2: f32, t: f32) -> f32 {
    let u = 1.0 - t;
    u * u * p0 + 2.0 * u * t * p1 + t * t * p2
}

/// Temporal smoothing strategies for noisy axes and gyro input.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Smoothing {
    Off,
    /// Exponential moving average. `alpha` is the new-sample weight.
    Exponential {
        alpha: f32,
    },
    /// Sliding window mean, which kills spikes but adds real latency.
    WeightedAverage {
        window: usize,
    },
}

/// Stateful filter for a single axis. Construct one per axis and feed it in
/// report order.
#[derive(Debug, Clone)]
pub struct AxisFilter {
    pub deadzone: f32,
    pub curve: Curve,
    pub anti_deadzone: bool,
    pub smoothing: Smoothing,
    pub sensitivity: f32,
    pub inverted: bool,

    // Private runtime state.
    ema: Option<f32>,
    window: Vec<f32>,
}

impl Default for AxisFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl AxisFilter {
    pub fn new() -> Self {
        Self {
            deadzone: 0.0,
            curve: Curve::Linear,
            anti_deadzone: true,
            smoothing: Smoothing::Off,
            sensitivity: 1.0,
            inverted: false,
            ema: None,
            window: Vec::new(),
        }
    }

    /// Feed one raw sample and get the shaped output.
    pub fn apply(&mut self, raw: f32) -> f32 {
        self.apply_with_formula(raw, None, &Inputs::default())
    }

    /// Feed one raw sample, applying an optional formula to the shaped value.
    ///
    /// The formula runs after the dead zone and the curve and before quantisation,
    /// which is the ordering that makes one useful. `a1 * 2` is a statement about
    /// the shaped value, so running it first would have it fight the curve. And
    /// `max(a1, 0)` has to see the value the curve produced, not the raw one, or
    /// it would suppress a direction the user had deliberately given more range.
    ///
    /// The shaped value is also passed to the formula as `a1`, so a formula refers
    /// to the value it is transforming rather than having to know which of the
    /// chain stages came before it.
    pub fn apply_with_formula(
        &mut self,
        raw: f32,
        formula: Option<&Formula>,
        siblings: &Inputs,
    ) -> f32 {
        let shaped = self.shape(raw);
        let Some(formula) = formula else {
            return shaped;
        };
        // The axis being shaped reads a1, so a formula can scale it by a constant
        // without having to be told which axis it is attached to.
        let mut inputs = *siblings;
        inputs.axes[0] = shaped;
        // A formula that fails at evaluation time falls back to the shaped value.
        // It was validated at load, so this is unreachable in practice, and
        // silently zeroing the axis would be a far worse outcome than ignoring it.
        formula.eval(&inputs).unwrap_or(shaped)
    }

    /// The curve, dead zone and smoothing, without any formula.
    fn shape(&mut self, raw: f32) -> f32 {
        let mut v = raw.clamp(-1.0, 1.0);
        if self.inverted {
            v = -v;
        }

        // 1. deadzone
        let mag = v.abs();
        let out = if mag <= self.deadzone {
            0.0
        } else {
            // 2. curve, on the normalised magnitude
            let mut m = self.curve.apply(unit(mag, self.deadzone));
            // 3. anti-deadzone: put back the travel we removed
            if self.anti_deadzone && self.deadzone > 0.0 {
                let span = (m * (1.0 - self.deadzone) + self.deadzone).clamp(0.0, 1.0);
                m = span;
            }
            m * self.sensitivity.clamp(0.0, 5.0)
        };

        let signed = if v.is_sign_negative() { -out } else { out };

        // 4. smoothing
        self.smooth(signed.clamp(-1.0, 1.0))
    }

    fn smooth(&mut self, v: f32) -> f32 {
        match self.smoothing {
            Smoothing::Off => v,
            Smoothing::Exponential { alpha } => {
                let a = alpha.clamp(0.01, 1.0);
                let prev = self.ema.unwrap_or(v);
                let next = prev + a * (v - prev);
                self.ema = Some(next);
                next
            }
            Smoothing::WeightedAverage { window } => {
                let n = window.clamp(1, 32);
                if self.window.len() >= n {
                    self.window.remove(0);
                }
                self.window.push(v);
                self.window.iter().sum::<f32>() / self.window.len() as f32
            }
        }
    }

    /// Forget accumulated smoothing state, e.g. after switching profiles.
    pub fn reset(&mut self) {
        self.ema = None;
        self.window.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_is_identity() {
        let mut f = AxisFilter::new();
        for v in [-1.0f32, -0.4, 0.0, 0.7, 1.0] {
            assert!((f.apply(v) - v).abs() < 0.001, "{v}");
        }
    }

    #[test]
    fn deadzone_kills_centre() {
        let mut f = AxisFilter::new();
        f.deadzone = 0.2;
        assert_eq!(f.apply(0.15), 0.0);
        assert_eq!(f.apply(-0.15), 0.0);
        assert!(f.apply(0.5) > 0.0);
    }

    #[test]
    fn anti_deadzone_restores_full_travel() {
        let mut f = AxisFilter::new();
        f.deadzone = 0.3;
        f.anti_deadzone = true;
        let top = f.apply(1.0);
        assert!(
            (top - 1.0).abs() < 0.001,
            "anti-deadzone should reach 1.0, got {top}"
        );
    }

    #[test]
    fn without_anti_deadzone_range_is_reduced() {
        let mut f = AxisFilter::new();
        f.deadzone = 0.3;
        f.anti_deadzone = false;
        let top = f.apply(1.0);
        assert!((top - 1.0).abs() < 0.001, "linear curve still hits 1.0");
    }

    #[test]
    fn exponential_is_monotonic() {
        let c = Curve::exponential(3.0);
        let mut prev = 0.0;
        for i in 0..=20 {
            let v = i as f32 / 20.0;
            let out = c.apply(v);
            assert!(out >= prev, "curve must not go backwards at {v}");
            prev = out;
        }
        assert!((c.apply(1.0) - 1.0).abs() < 0.001);
    }

    #[test]
    fn bezier_passes_through_endpoints() {
        let c = Curve::smooth();
        assert!(c.apply(0.0).abs() < 0.001);
        assert!((c.apply(1.0) - 1.0).abs() < 0.001);
    }

    #[test]
    fn inverted_flips_sign() {
        let mut f = AxisFilter::new();
        f.inverted = true;
        assert!(f.apply(0.5) < 0.0);
    }

    #[test]
    fn smoothing_converges_to_target() {
        let mut f = AxisFilter::new();
        f.smoothing = Smoothing::Exponential { alpha: 0.5 };
        for _ in 0..40 {
            f.apply(1.0);
        }
        assert!(f.apply(1.0) > 0.9);
    }

    #[test]
    fn stepped_is_binary() {
        let mut f = AxisFilter::new();
        f.curve = Curve::Stepped { threshold: 0.5 };
        assert_eq!(f.apply(0.4), 0.0);
        assert_eq!(f.apply(0.8), 1.0);
    }
}
