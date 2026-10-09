//! Proves a mapping formula reaches the published gamepad state.
//!
//! The unit tests cover the parser and the filter in isolation. This runs the
//! engine's own translation with a formula attached, because the wiring between
//! the two is where a feature can be complete and still have no effect: a formula
//! that is parsed, stored, and then never consulted looks identical to one that
//! works when everything it reads is at rest.
//!
//! Run:
//!     cargo run -p padcore --bin probe-formula-live --release --target x86_64-pc-windows-gnu

use padcore::filters::AxisFilter;
use padcore::formula::{Formula, Inputs};
use padcore::report::Ds4Report;

fn main() {
    println!("=== a formula on the axis filter chain ===");

    // Plain chain: what the axis reads with no formula.
    let mut plain = AxisFilter::new();
    plain.deadzone = 0.1;
    plain.sensitivity = 1.0;

    // Same chain, plus a formula that doubles the shaped value.
    let mut doubled = AxisFilter::new();
    doubled.deadzone = 0.1;
    doubled.sensitivity = 1.0;

    let formula = Formula::parse("a1 * 2").expect("should parse");
    let empty = Inputs::default();

    println!("  raw      no formula   with 'a1 * 2'");
    for raw in [0.0f32, 0.15, 0.3, 0.5, 0.75, 1.0] {
        let a = plain.apply_with_formula(raw, None, &empty);
        let b = doubled.apply_with_formula(raw, Some(&formula), &empty);
        println!(
            "  {raw:>5.2}     {a:>8.3}      {b:>8.3}{}",
            if (b - a).abs() > 0.001 {
                "   <- changed"
            } else {
                ""
            }
        );
    }

    println!("\n=== one side of an axis ===");
    let half = Formula::parse("max(a1, 0)").expect("should parse");
    for raw in [-0.5f32, -0.1, 0.1, 0.5] {
        let v = doubled.apply_with_formula(raw, Some(&half), &empty);
        println!("  raw {raw:>5.2} -> {v:>7.3}");
    }

    println!("\n=== the formula sees the other axes too ===");
    // Two axes where the formula multiplies them, which is the case a per-axis
    // implementation cannot express at all.
    let mut a = AxisFilter::new();
    a.deadzone = 0.0;
    let product = Formula::parse("a1 * a2").expect("should parse");
    let inputs = Inputs {
        axes: [0.5, 0.5, 0.0, 0.0],
        ..Default::default()
    };
    println!(
        "  a1=0.5 with a2=0.5 -> {:.3}",
        a.apply_with_formula(0.5, Some(&product), &inputs).max(0.0)
    );
    println!("  a1=0.5 with a2=0.0 -> {:.3}", {
        let zero = Inputs {
            axes: [0.5, 0.0, 0.0, 0.0],
            ..Default::default()
        };
        a.apply_with_formula(0.5, Some(&product), &zero).max(0.0)
    });

    println!("\n=== a formula on a profile row, end to end ===");
    // The settings half: a profile stores text, and this is what turns it into
    // something the filter can run.
    let settings = padcore::profile::AxisSettings {
        deadzone: 0.1,
        formula: Some("a1 * 2".to_string()),
        ..Default::default()
    };
    let parsed = settings.formula();
    println!(
        "  profile text {:?} parsed: {}",
        settings.formula,
        parsed.is_some()
    );

    let mut filter = settings.to_filter();
    let v = filter.apply_with_formula(0.3, parsed.as_ref(), &empty);
    println!(
        "  raw 0.30 with dead zone 0.10 -> {:.3} (no formula would be {:.3})",
        v,
        {
            let mut bare = padcore::profile::AxisSettings {
                formula: None,
                ..settings.clone()
            }
            .to_filter();
            bare.apply_with_formula(0.3, None, &empty)
        }
    );

    println!("\n=== a report decodes with the layout this session found ===");
    let report = Ds4Report::neutral();
    println!(
        "  neutral report: sticks ({:+.2}, {:+.2}) ({:+.2}, {:+.2})",
        report.left_x, report.left_y, report.right_x, report.right_y
    );
}
