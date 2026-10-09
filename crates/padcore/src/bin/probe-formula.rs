//! Prints the token stream for a formula, and what each parses to.
//!
//! When a formula evaluates to the wrong number the two candidates are a
//! misread token stream and a correct parser with wrong precedence. Printing the
//! tokens settles which, without having to reason about the grammar from the
//! outside.
//!
//! Run:
//!     cargo run -p padcore --bin probe-formula --release --target x86_64-pc-windows-gnu -- "1 + 2 * 3"

use padcore::formula::{Formula, Inputs};

fn main() {
    let sources: Vec<String> = std::env::args().skip(1).collect();
    let sources = if sources.is_empty() {
        vec![
            "1 + 2 * 3".to_string(),
            "(1 + 2) * 3".to_string(),
            "a1 * 2".to_string(),
            "max(a1, 0)".to_string(),
            "abs(a1) * 2 - 0.25".to_string(),
            "t".to_string(),
            "1 +".to_string(),
        ]
    } else {
        sources
    };

    for source in &sources {
        print!("{source:<22} ");
        match Formula::parse(source) {
            Ok(f) => {
                println!("{:?}", f.tokens());
                let sources_used = f.sources();
                // One fixed set of inputs, so every formula in the list sees the same
                // numbers and two lines can be compared directly.
                let inputs = Inputs {
                    axes: [0.5, -0.5, 0.25, 0.0],
                    buttons: [1.0, 0.0, 0.0, 0.0],
                    sliders: [0.75, 0.0, 0.0, 0.0],
                    now_ms: 2500.0,
                };
                match f.eval(&inputs) {
                    Ok(v) => println!(
                        "ok   value={v:>8.3}  reads={sources_used:?}  (a1=0.5 a2=-0.5 b1=1 s1=0.75 now=2500ms)"
                    ),
                    Err(e) => println!("parsed but eval failed: {e}"),
                }
            }
            Err(e) => println!("rejected: {e}"),
        }
    }
}
