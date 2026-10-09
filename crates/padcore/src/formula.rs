//! A small expression language for mapping rows.
//!
//! The feature this exists for is a formula instead of a fixed sensitivity, so a
//! row can say `a1 * 2` to reach full travel at half a press, or
//! `max(a1, 0)` to use only one half of a pedal. x360ce has this and it earns
//! its place.
//!
//! It is a parser and evaluator rather than a general expression evaluator for a
//! reason: a user-supplied formula is untrusted input. There is no assignment, no
//! indexing, no function calls beyond `abs`, `min` and `max`, no recursion, and no
//! way to name anything outside the values a row is given. The worst a formula
//! can do is return the wrong number for one axis of one control.
//!
//! Grammar, in full:
//!
//! ```text
//! expr    := term (('+' | '-') term)*
//! term    := factor (('*' | '/' | '%') factor)*
//! factor  := ('-' | '+')? primary
//! primary := number | 'abs' '(' expr ')' | '(' expr ')'
//!          | 'a' N | 'b' N | 's' N | 'now' | 't'
//! ```
//!
//! `a` reads an axis: -1..=1 for sticks, 0..=1 for triggers. `b` reads a button as
//! 1 or 0. `s` reads a slider as 0..=1.
//! `now` is milliseconds since the engine
//! started and `t` is seconds, so a control can be driven by elapsed time.
//!
//! Division by zero yields zero. An infinity reaching a quantiser wraps around
//! and pins the control at full travel, which is the most confusing failure
//! available; zero is wrong but visibly wrong rather than silently wrong.

use std::fmt;

/// One parsed token.
///
/// Public so the diagnostic probe can print a stream and settle whether a wrong
/// answer came from misreading the text or from the precedence rules.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Number(f32),
    /// An axis reference. The index is stored zero-based.
    Axis(usize),
    /// A button reference.
    Button(usize),
    /// A slider reference.
    Slider(usize),
    /// Milliseconds since the engine started.
    Millis,
    /// Seconds since the engine started.
    Seconds,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Open,
    Close,
    Comma,
    Abs,
    Min,
    Max,
}

/// Why a formula could not be used.
///
/// Every variant says what to change rather than naming a problem, because the
/// whole point of these is to be shown to whoever typed them.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// The text ends in the middle of something.
    UnexpectedEnd,
    /// A character with no meaning here.
    BadCharacter(char),
    /// A token in a position where it makes no sense.
    Unexpected(&'static str),
    /// A reference with no number after it, such as a bare `a`.
    MissingIndex,
    /// An index that cannot exist.
    IndexOutOfRange(&'static str, usize),
    /// More nesting or more tokens than is allowed.
    TooDeep,
    /// Too long to evaluate per frame.
    TooLong,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEnd => write!(f, "the formula ends too early"),
            Self::BadCharacter(c) => write!(f, "'{c}' is not something a formula can use"),
            Self::Unexpected(what) => write!(f, "{what} cannot go there"),
            Self::MissingIndex => {
                write!(f, "a, b and s need a number after them, as in a1 or b2")
            }
            Self::IndexOutOfRange(kind, n) => {
                write!(f, "{n} is out of range for {kind}; it runs from 1 to 4")
            }
            Self::TooDeep => write!(f, "the formula nests too deeply"),
            Self::TooLong => write!(f, "the formula is too long"),
        }
    }
}

impl std::error::Error for Error {}

/// Hard cap on nesting depth.
///
/// Evaluation runs once per axis per frame, and a person typing a formula is not
/// going to nest more than a couple of levels, so a small limit costs nothing and
/// bounds the recursion. Sixteen brackets is far beyond any real formula.
const MAX_DEPTH: usize = 16;
/// Hard cap on token count, for the same reason.
const MAX_TOKENS: usize = 64;
/// Highest axis, button or slider index a formula may name.
pub const MAX_SOURCES: usize = 4;

/// The values a formula is evaluated against.
#[derive(Debug, Clone, Copy)]
pub struct Inputs {
    /// Axis values, indexed from zero. Sticks read -1..=1, triggers 0..=1.
    pub axes: [f32; MAX_SOURCES],
    /// Button states, indexed from zero.
    pub buttons: [f32; MAX_SOURCES],
    /// Slider positions, indexed from zero.
    pub sliders: [f32; MAX_SOURCES],
    /// Milliseconds since the engine started.
    pub now_ms: f64,
}

impl Default for Inputs {
    fn default() -> Self {
        Self {
            axes: [0.0; MAX_SOURCES],
            buttons: [0.0; MAX_SOURCES],
            sliders: [0.0; MAX_SOURCES],
            now_ms: 0.0,
        }
    }
}

/// Turn `a3` into `Axis(2)`, checking the index.
///
/// Takes the character slice and a starting position and returns the token along
/// with how far it consumed, rather than borrowing a lexer. A lexer type for one
/// call site is an indirection with nothing behind it: the parse loop already holds
/// the characters and the position, and this only has to read a digit run.
fn indexed(name: &str, chars: &[char], start: usize) -> Result<(Token, usize), Error> {
    let kind: &'static str = match name {
        "a" => "axis",
        "b" => "button",
        _ => "slider",
    };
    let mut pos = start;
    while pos < chars.len() && chars[pos].is_ascii_digit() {
        pos += 1;
    }
    let digits: String = chars[start..pos].iter().collect();
    if digits.is_empty() {
        return Err(Error::MissingIndex);
    }
    let index: usize = digits.parse().unwrap_or(usize::MAX);
    if index == 0 || index > MAX_SOURCES {
        return Err(Error::IndexOutOfRange(kind, index));
    }
    // Stored zero-based: the source text is one-based because that is how the UI
    // numbers its controls, and index 0 is easy to write by accident.
    let token = match name {
        "a" => Token::Axis(index - 1),
        "b" => Token::Button(index - 1),
        _ => Token::Slider(index - 1),
    };
    Ok((token, pos))
}

/// Re-lex with one more character consumed as a name, then a digit run.
///
/// The lexer reads a whole alphabetic run in one go, so `abs` and `a` and `now`
/// all arrive together and have to be split here. Returning the name length keeps
/// that decision in one place instead of at each call site.
fn split_name(chars: &[char], start: usize) -> (String, usize) {
    let mut end = start;
    while end < chars.len() && chars[end].is_ascii_alphabetic() {
        end += 1;
    }
    (chars[start..end].iter().collect(), end)
}

/// A parsed formula, ready to evaluate per frame.
///
/// Stored as tokens rather than a tree: evaluation happens once per axis per
/// frame and the perf suite asserts the hot path does not allocate, so a flat
/// walk is both simpler and cheaper than building nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct Formula {
    tokens: Vec<Token>,
}

impl Formula {
    /// The token stream, for diagnostics.
    pub fn tokens(&self) -> &[Token] {
        &self.tokens
    }
}

impl Formula {
    /// Parse a formula, rejecting anything the evaluator would not accept.
    ///
    /// Fully validated before being stored, which is what lets a bad formula be
    /// reported while it is being typed rather than at the moment a button is
    /// first pressed.
    pub fn parse(source: &str) -> Result<Self, Error> {
        let mut tokens = Vec::new();
        let chars: Vec<char> = source.chars().collect();
        let mut pos = 0usize;
        let mut count = 0usize;

        while pos < chars.len() {
            while pos < chars.len() && chars[pos].is_whitespace() {
                pos += 1;
            }
            if pos >= chars.len() {
                break;
            }
            count += 1;
            if count > MAX_TOKENS {
                return Err(Error::TooLong);
            }

            let c = chars[pos];
            let single = match c {
                '+' => Some(Token::Plus),
                '-' => Some(Token::Minus),
                '*' => Some(Token::Star),
                '/' => Some(Token::Slash),
                '%' => Some(Token::Percent),
                '(' => Some(Token::Open),
                ')' => Some(Token::Close),
                ',' => Some(Token::Comma),
                _ => None,
            };
            if let Some(t) = single {
                tokens.push(t);
                pos += 1;
                continue;
            }

            if c.is_ascii_digit() || c == '.' {
                let start = pos;
                while pos < chars.len() && (chars[pos].is_ascii_digit() || chars[pos] == '.') {
                    pos += 1;
                }
                let text: String = chars[start..pos].iter().collect();
                match text.parse::<f32>() {
                    Ok(v) => tokens.push(Token::Number(v)),
                    Err(_) => return Err(Error::BadCharacter(c)),
                }
                continue;
            }

            if c.is_ascii_alphabetic() {
                let (name, end) = split_name(&chars, pos);
                match name.as_str() {
                    "abs" => {
                        tokens.push(Token::Abs);
                        pos = end;
                    }
                    "min" => {
                        tokens.push(Token::Min);
                        pos = end;
                    }
                    "max" => {
                        tokens.push(Token::Max);
                        pos = end;
                    }
                    "now" => {
                        tokens.push(Token::Millis);
                        pos = end;
                    }
                    "t" => {
                        tokens.push(Token::Seconds);
                        pos = end;
                    }
                    "a" | "b" | "s" => {
                        let (token, consumed) = indexed(&name, &chars, end)?;
                        tokens.push(token);
                        pos = consumed;
                    }
                    _ => return Err(Error::BadCharacter(c)),
                }
                continue;
            }

            return Err(Error::BadCharacter(c));
        }

        if tokens.is_empty() {
            return Err(Error::UnexpectedEnd);
        }

        // A full structural pass now, so evaluation cannot be surprised later.
        let mut parser = Parser {
            tokens: &tokens,
            pos: 0,
            depth: 0,
            inputs: None,
        };
        parser.parse_expr()?;
        if parser.pos != tokens.len() {
            return Err(Error::Unexpected("that"));
        }

        Ok(Self { tokens })
    }

    /// Whether the formula reads a given source, as the UI names them.
    ///
    /// Lets a row reject `a3` when it only has two axes, rather than silently
    /// reading zero and looking like a formula that does not work.
    pub fn needs(&self, kind: &str, index: usize) -> bool {
        self.tokens.iter().any(|t| match (kind, t) {
            ("axis", Token::Axis(i)) => *i == index,
            ("button", Token::Button(i)) => *i == index,
            ("slider", Token::Slider(i)) => *i == index,
            _ => false,
        })
    }

    /// Every source the formula reads, as `(kind, zero-based index)`.
    pub fn sources(&self) -> Vec<(&'static str, usize)> {
        let mut out = Vec::new();
        for t in &self.tokens {
            match t {
                Token::Axis(i) => out.push(("axis", *i)),
                Token::Button(i) => out.push(("button", *i)),
                Token::Slider(i) => out.push(("slider", *i)),
                _ => {}
            }
        }
        out
    }

    /// Evaluate against the given inputs.
    ///
    /// The result is clamped to the range an XInput control can hold, and a NaN
    /// becomes zero. Both matter: the quantiser wraps rather than saturating, so
    /// an unbounded or undefined value would pin a control at full travel instead
    /// of failing visibly.
    pub fn eval(&self, inputs: &Inputs) -> Result<f32, Error> {
        let mut parser = Parser {
            tokens: &self.tokens,
            pos: 0,
            depth: 0,
            inputs: Some(inputs),
        };
        let v = parser.parse_expr()?;
        Ok(sanitise(v))
    }
}

/// Bring a result into the range a control can hold.
/// Bring a finished result into the range a control can hold.
///
/// Applied once, to the value the formula produces, and never to an
/// intermediate. Clamping inside the arithmetic looks harmless and is not:
/// `1 + 2 * 3` has an intermediate of six, which clamped to one, so the sum came
/// out as two rather than seven. The limit belongs at the boundary where the
/// value reaches a control, not in the middle of a calculation that is free to use
/// any number it likes.
fn sanitise(v: f32) -> f32 {
    if v.is_nan() {
        return 0.0;
    }
    v.clamp(-1.0, 1.0)
}

/// Recursive-descent evaluator over a token slice.
///
/// `inputs` is absent during the structural pass that runs at parse time. Every
/// reference then reads zero, which is what the structural pass wants anyway: it
/// is checking shape, never a value.
struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    depth: usize,
    inputs: Option<&'a Inputs>,
}

/// The value a reference reads when there is nothing to read from.
static ZERO: Inputs = Inputs {
    axes: [0.0; MAX_SOURCES],
    buttons: [0.0; MAX_SOURCES],
    sliders: [0.0; MAX_SOURCES],
    now_ms: 0.0,
};

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn bump(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn parse_expr(&mut self) -> Result<f32, Error> {
        let mut left = self.parse_term()?;
        loop {
            match self.peek() {
                Some(Token::Plus) => {
                    self.bump();
                    left += self.parse_term()?;
                }
                Some(Token::Minus) => {
                    self.bump();
                    left -= self.parse_term()?;
                }
                _ => return Ok(left),
            }
        }
    }

    fn parse_term(&mut self) -> Result<f32, Error> {
        let mut left = self.parse_factor()?;
        loop {
            match self.peek() {
                Some(Token::Star) => {
                    self.bump();
                    left *= self.parse_factor()?;
                }
                Some(Token::Slash) => {
                    self.bump();
                    let d = self.parse_factor()?;
                    left = if d.abs() < 1e-6 { 0.0 } else { left / d };
                }
                Some(Token::Percent) => {
                    self.bump();
                    let d = self.parse_factor()?;
                    left = if d.abs() < 1e-6 { 0.0 } else { left % d };
                }
                _ => return Ok(left),
            }
        }
    }

    fn parse_factor(&mut self) -> Result<f32, Error> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(Error::TooDeep);
        }
        let out = self.parse_primary();
        self.depth -= 1;
        out
    }

    fn parse_primary(&mut self) -> Result<f32, Error> {
        let Some(token) = self.bump() else {
            return Err(Error::UnexpectedEnd);
        };
        Ok(match token {
            Token::Number(v) => v,
            Token::Plus => self.parse_factor()?,
            Token::Minus => -self.parse_factor()?,
            Token::Axis(i) => self.inputs().axes[i],
            Token::Button(i) => self.inputs().buttons[i],
            Token::Slider(i) => self.inputs().sliders[i],
            Token::Millis => self.inputs().now_ms as f32,
            Token::Seconds => (self.inputs().now_ms / 1000.0) as f32,
            Token::Open => {
                let v = self.parse_expr()?;
                match self.bump() {
                    Some(Token::Close) => v,
                    _ => return Err(Error::Unexpected("a closing bracket")),
                }
            }
            Token::Abs => {
                // Unary and a prefix, so it takes a whole factor rather than a
                // bare primary: `abs(a1) * 2` has to be `abs(a1)` and not `abs(a1`
                // with a stray bracket left over.
                let v = self.parse_factor()?;
                v.abs()
            }
            Token::Min | Token::Max => {
                // Two arguments in brackets, so this is parsed as a prefix rather
                // than as an operator. `max(a1, 0)` reads naturally this way and
                // is what the x360ce documentation shows.
                if !matches!(self.bump(), Some(Token::Open)) {
                    return Err(Error::Unexpected("an opening bracket"));
                }
                let a = self.parse_expr()?;
                if !matches!(self.bump(), Some(Token::Comma)) {
                    return Err(Error::Unexpected("a comma"));
                }
                let b = self.parse_expr()?;
                if !matches!(self.bump(), Some(Token::Close)) {
                    return Err(Error::Unexpected("a closing bracket"));
                }
                if matches!(token, Token::Min) {
                    a.min(b)
                } else {
                    a.max(b)
                }
            }
            Token::Close => return Err(Error::Unexpected("a closing bracket")),
            Token::Comma => return Err(Error::Unexpected("a comma")),
            Token::Star => return Err(Error::Unexpected("a multiplication sign")),
            Token::Slash => return Err(Error::Unexpected("a division sign")),
            Token::Percent => return Err(Error::Unexpected("a modulo sign")),
        })
    }

    /// The inputs this parse reads.
    ///
    /// A field rather than a parameter so the many call sites inside
    /// `parse_primary` do not each have to thread it through, and `Option` so the
    /// structural pass that happens at parse time can share this code with an
    /// absent value rather than inventing one.
    fn inputs(&self) -> &Inputs {
        self.inputs.unwrap_or(&ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Evaluate without the final clamp.
    ///
    /// Precedence and associativity have to be checked against the arithmetic
    /// itself, and the clamp hides it: `1 + 2 * 3` is seven internally and one on
    /// the way out, so a test against the public result cannot tell a correct
    /// evaluator from one that clamps at every step.
    fn raw(source: &str, f: impl FnOnce(&mut Inputs)) -> f32 {
        let mut inputs = Inputs::default();
        f(&mut inputs);
        let formula =
            Formula::parse(source).unwrap_or_else(|e| panic!("{source:?} should parse: {e}"));
        let mut parser = Parser {
            tokens: formula.tokens(),
            pos: 0,
            depth: 0,
            inputs: Some(&inputs),
        };
        parser.parse_expr().expect("should evaluate")
    }

    /// Evaluate through the public path, which clamps to the control's range.
    fn ev(source: &str, f: impl FnOnce(&mut Inputs)) -> f32 {
        let mut inputs = Inputs::default();
        f(&mut inputs);
        Formula::parse(source)
            .unwrap_or_else(|e| panic!("{source:?} should parse: {e}"))
            .eval(&inputs)
            .unwrap()
    }

    /// Precedence and associativity, checked before the final clamp.
    ///
    /// The clamp is applied once at the boundary and never inside the arithmetic, so
    /// `2 * 3` really is six on the way through. Testing against the public result
    /// would only ever see one and could not tell this from clamping at every step.
    #[test]
    fn plain_arithmetic() {
        assert_eq!(
            raw("1 + 2 * 3", |_| {}),
            7.0,
            "multiplication binds tighter"
        );
        assert_eq!(raw("(1 + 2) * 3", |_| {}), 9.0, "brackets override");
        assert_eq!(raw("10 - 4 - 3", |_| {}), 3.0, "left to right, not 9");
        assert_eq!(raw("7 / 2", |_| {}), 3.5);
        assert_eq!(raw("-3 + 5", |_| {}), 2.0);
        assert_eq!(raw("2 * -3", |_| {}), -6.0);
        assert_eq!(raw("1 + 2 - 3 + 4", |_| {}), 4.0);
        assert_eq!(raw("2 % 3", |_| {}), 2.0);
    }

    /// The clamp belongs at the boundary, once.
    ///
    /// Values above one are a normal result of arithmetic rather than a fault:
    /// `a1 * 2` at full deflection is two. Clamping early would make it one and
    /// quietly halve every formula that scales.
    #[test]
    fn the_clamp_is_applied_once_at_the_end() {
        // Half scale doubled is exactly one, which is indistinguishable from a
        // clamped one, so the test uses a value that lands between.
        assert_eq!(
            ev("a1 * 2", |i| i.axes[0] = 0.3),
            0.6,
            "no clamp before the end"
        );
        assert_eq!(raw("a1 * 2", |i| i.axes[0] = 0.3), 0.6);
        assert_eq!(raw("a1 * 4", |i| i.axes[0] = 0.5), 2.0);
        assert_eq!(
            ev("a1 * 4", |i| i.axes[0] = 0.5),
            1.0,
            "clamped on the way out"
        );
    }

    #[test]
    fn axes_are_read_and_scaled() {
        assert_eq!(ev("a1 * 2", |i| i.axes[0] = 0.5), 1.0);
        assert_eq!(ev("a1 * 0.5", |i| i.axes[0] = 1.0), 0.5);
        assert_eq!(ev("a1 * abs(a1)", |i| i.axes[0] = 0.5), 0.25);
        assert_eq!(ev("a2", |i| i.axes[1] = -0.75), -0.75);
    }

    #[test]
    fn clamp_happens_after_evaluation() {
        // A formula that overshoots is limited, not wrapped. Wrapping would pin
        // the control at full travel, which is how a *4 becomes a broken stick.
        assert_eq!(ev("a1 * 4", |i| i.axes[0] = 0.5), 1.0);
        assert_eq!(ev("a1 * -4", |i| i.axes[0] = 0.5), -1.0);
        assert_eq!(ev("a1 * 10", |i| i.axes[0] = 0.5), 1.0);
    }

    #[test]
    fn divide_by_zero_is_zero_not_infinity() {
        assert_eq!(ev("a1 / (a2 - a2)", |i| i.axes[0] = 0.8), 0.0);
        assert_eq!(ev("a1 / 0", |i| i.axes[0] = 0.8), 0.0);
    }

    #[test]
    fn half_an_axis_uses_one_side() {
        assert_eq!(ev("max(a1, 0)", |i| i.axes[0] = 0.5), 0.5);
        assert_eq!(ev("max(a1, 0)", |i| i.axes[0] = -0.5), 0.0);
        assert_eq!(ev("min(a1, 0)", |i| i.axes[0] = -0.5), -0.5);
    }

    #[test]
    fn buttons_and_sliders_read() {
        assert_eq!(ev("b1 * 0.5", |i| i.buttons[0] = 1.0), 0.5);
        assert_eq!(ev("b2", |i| i.buttons[1] = 1.0), 1.0);
        assert_eq!(ev("s1 * 2", |i| i.sliders[0] = 0.3), 0.6);
    }

    #[test]
    fn time_reads() {
        // Checked raw, because seconds since start is routinely past one and the
        // clamp would reduce every reading to a full-scale constant.
        let v = raw("t", |i| i.now_ms = 2500.0);
        assert!((v - 2.5).abs() < 0.01, "seconds, got {v}");
        let v = raw("now / 1000", |i| i.now_ms = 4000.0);
        assert!((v - 4.0).abs() < 0.01, "milliseconds, got {v}");
        // Through the public path it is limited, which is what keeps a long
        // session from pinning whatever the formula drives.
        let v = ev("t", |i| i.now_ms = 2500.0);
        assert!((v - 1.0).abs() < 0.01, "clamped to full scale, got {v}");
    }

    #[test]
    fn bad_syntax_is_rejected_at_parse_time() {
        for bad in [
            "", "   ", "1 +", "* 2", "(1 + 2", "1 + 2)", "a", "a 1", "a0", "a5", "b", "abs",
            "abs(", "1 2", "$", "a1 @ a2",
        ] {
            assert!(Formula::parse(bad).is_err(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn depth_and_length_are_bounded() {
        let deep = "(".repeat(MAX_DEPTH + 2) + "1" + &")".repeat(MAX_DEPTH + 2);
        assert_eq!(Formula::parse(&deep), Err(Error::TooDeep));

        let long = (0..MAX_TOKENS + 5).map(|_| "1+").collect::<String>() + "1";
        assert_eq!(Formula::parse(&long), Err(Error::TooLong));
    }

    /// A formula is typed by a person into a text box, so every error has to say
    /// something actionable rather than naming a parse state.
    #[test]
    fn errors_explain_themselves() {
        assert!(Error::MissingIndex.to_string().contains("a1"));
        assert!(Error::IndexOutOfRange("axis", 5)
            .to_string()
            .contains("1 to 4"));
        assert!(Error::UnexpectedEnd.to_string().contains("ends"));
    }

    #[test]
    fn sources_are_reported_for_validation() {
        let f = Formula::parse("a1 * 2 + b2").unwrap();
        let mut s = f.sources();
        s.sort();
        assert_eq!(s, vec![("axis", 0), ("button", 1)]);
        assert!(f.needs("axis", 0));
        assert!(!f.needs("axis", 1));
        assert!(f.needs("button", 1));
    }

    /// A parsed formula is reusable, which is what makes it safe to keep one per
    /// mapping row and evaluate it per frame.
    #[test]
    fn the_parsed_formula_is_reusable() {
        let f = Formula::parse("abs(a1) * 2 - 0.25").unwrap();
        let mut inputs = Inputs::default();
        for (axis, raw_expected) in [
            (-1.0f32, 1.75f32),
            (-0.5, 0.75),
            (0.0, -0.25),
            (0.5, 0.75),
            (1.0, 1.75),
        ] {
            inputs.axes[0] = axis;
            // -0.25 is out of range for a control, so it clamps; the rest pass
            // through. Comparing against the clamped value rather than a hand
            // written list is what keeps this honest when the curve changes.
            let expected = raw_expected.clamp(-1.0, 1.0);
            let got = f.eval(&inputs).unwrap();
            assert!(
                (got - expected).abs() < 1e-6,
                "at {axis}: expected {expected}, got {got}"
            );
        }
    }
}
