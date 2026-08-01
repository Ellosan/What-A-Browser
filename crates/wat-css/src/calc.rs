//! `calc()`, `min()`, `max()` and `clamp()`.
//!
//! A maths expression that mixes lengths with percentages cannot be reduced to a
//! single number until the percentage basis is known, so it reduces instead to a
//! [`CalcLength`]: so many pixels plus so much of whatever the basis turns out to
//! be. That is exactly the shape `calc(100% - 2rem)` needs, and it is enough for
//! every combination CSS allows, because multiplying two percentages is invalid
//! anyway.

use crate::values::{LengthContext, Unit, Value};

/// A `min()` or `max()` bound that cannot be applied until the basis is known,
/// which is what `min(100%, 420px)` amounts to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bound {
    pub px: f32,
    /// `max()` rather than `min()`.
    pub largest: bool,
}

/// A length of the form `percent% of the basis + px`, optionally bounded.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CalcLength {
    pub px: f32,
    /// Percentage of the basis, as a percentage rather than a fraction.
    pub percent: f32,
    /// A limit to apply once the basis is known.
    pub bound: Option<Bound>,
}

impl CalcLength {
    pub fn new(px: f32, percent: f32) -> CalcLength {
        CalcLength {
            px,
            percent,
            bound: None,
        }
    }

    pub fn px(px: f32) -> CalcLength {
        CalcLength::new(px, 0.0)
    }

    pub fn percent(percent: f32) -> CalcLength {
        CalcLength::new(0.0, percent)
    }

    /// Whether this needs a percentage basis to become a number.
    pub fn needs_basis(&self) -> bool {
        self.percent != 0.0 || self.bound.is_some()
    }

    /// Resolves against a percentage basis.
    pub fn resolve(&self, basis: f32) -> f32 {
        let value = self.px + basis * self.percent / 100.0;
        match self.bound {
            Some(Bound { px, largest: true }) => value.max(px),
            Some(Bound { px, largest: false }) => value.min(px),
            None => value,
        }
    }

    /// Resolves only if no basis is needed.
    pub fn resolve_definite(&self) -> Option<f32> {
        (!self.needs_basis()).then_some(self.px)
    }
}

/// One operand while evaluating.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Term {
    /// A unitless number, which is what a multiplier is.
    Number(f32),
    Length(CalcLength),
}

impl Term {
    /// A number and a length can only be added when the number is zero, which is
    /// the one case CSS is relaxed about in practice.
    fn as_length(self) -> Option<CalcLength> {
        match self {
            Term::Length(length) => Some(length),
            Term::Number(0.0) => Some(CalcLength::px(0.0)),
            Term::Number(_) => None,
        }
    }
}

/// Is this value a maths function this module can evaluate?
pub fn is_math_function(value: &Value) -> bool {
    matches!(value, Value::Function { name, .. }
        if matches!(name.as_str(), "calc" | "(" | "min" | "max" | "clamp"))
}

/// Evaluates a maths function to a length.
///
/// Returns `None` for an expression that is not valid — a unit that cannot be
/// converted, a `var()` that was never substituted, or a `min()` whose arguments
/// cannot be told apart.
pub fn evaluate(value: &Value, ctx: &LengthContext) -> Option<CalcLength> {
    match term(value, ctx)? {
        Term::Length(length) => Some(length),
        // `calc(2 * 3)` is a number, not a length; only zero is both.
        Term::Number(0.0) => Some(CalcLength::px(0.0)),
        Term::Number(_) => None,
    }
}

/// Evaluates a maths function that is expected to produce a plain number, as
/// `line-height: calc(1.2 * 2)` does.
pub fn evaluate_number(value: &Value, ctx: &LengthContext) -> Option<f32> {
    match term(value, ctx)? {
        Term::Number(value) => Some(value),
        Term::Length(_) => None,
    }
}

fn term(value: &Value, ctx: &LengthContext) -> Option<Term> {
    let Value::Function { name, args } = value else {
        return primary(value, ctx);
    };
    match name.as_str() {
        // A parenthesised group inside a maths expression is a nested sum.
        "calc" | "(" => sum(args, &mut 0, ctx),
        "min" | "max" | "clamp" => extremum(name, args, ctx),
        _ => None,
    }
}

/// `min()`, `max()` and `clamp()`.
///
/// The parser drops the commas between arguments, so an argument that is itself
/// an expression cannot be told from the next argument. Rather than guess, a
/// list containing an operator is reported as unsupported.
fn extremum(name: &str, args: &[Value], ctx: &LengthContext) -> Option<Term> {
    if args.iter().any(is_operator) {
        return None;
    }
    let mut lengths = Vec::with_capacity(args.len());
    for arg in args {
        lengths.push(term(arg, ctx)?.as_length()?);
    }
    if lengths.is_empty() {
        return None;
    }
    if lengths.iter().any(|length| length.bound.is_some()) {
        return None;
    }

    // A comparison between a percentage and a length cannot be made until the
    // basis is known, so `min(100%, 420px)` keeps the limit and applies it in
    // layout. That is the shape of the common max-width idiom, so it is worth
    // carrying rather than dropping.
    let definite: Vec<CalcLength> = lengths
        .iter()
        .copied()
        .filter(|length| !length.needs_basis())
        .collect();
    let relative: Vec<CalcLength> = lengths
        .iter()
        .copied()
        .filter(|length| length.needs_basis())
        .collect();
    if !definite.is_empty() && !relative.is_empty() {
        // Only a two-argument min or max can be expressed as one bound.
        if name == "clamp" || relative.len() != 1 || definite.len() != 1 {
            return None;
        }
        let mut bounded = relative[0];
        bounded.bound = Some(Bound {
            px: definite[0].px,
            largest: name == "max",
        });
        return Some(Term::Length(bounded));
    }

    let key = |length: CalcLength| length.px + length.percent;

    let chosen = match name {
        "min" => *lengths
            .iter()
            .min_by(|a, b| key(**a).total_cmp(&key(**b)))
            .unwrap(),
        "max" => *lengths
            .iter()
            .max_by(|a, b| key(**a).total_cmp(&key(**b)))
            .unwrap(),
        // `clamp(low, value, high)` is `max(low, min(value, high))`.
        _ => {
            if lengths.len() != 3 {
                return None;
            }
            let (low, value, high) = (lengths[0], lengths[1], lengths[2]);
            if key(value) < key(low) {
                low
            } else if key(value) > key(high) {
                high
            } else {
                value
            }
        }
    };
    Some(Term::Length(chosen))
}

fn is_operator(value: &Value) -> bool {
    matches!(value, Value::Keyword(text) if matches!(text.as_str(), "+" | "-" | "*" | "/"))
}

fn operator_at(args: &[Value], index: usize) -> Option<&str> {
    match args.get(index) {
        Some(Value::Keyword(text)) if is_operator(&Value::Keyword(text.clone())) => Some(text),
        _ => None,
    }
}

/// `a + b - c`
fn sum(args: &[Value], index: &mut usize, ctx: &LengthContext) -> Option<Term> {
    let mut left = product(args, index, ctx)?;
    while let Some(op) = operator_at(args, *index) {
        if op != "+" && op != "-" {
            break;
        }
        *index += 1;
        let right = product(args, index, ctx)?;
        let sign = if op == "+" { 1.0 } else { -1.0 };
        left = match (left, right) {
            (Term::Number(a), Term::Number(b)) => Term::Number(a + sign * b),
            (a, b) => {
                let a = a.as_length()?;
                let b = b.as_length()?;
                // A bounded operand cannot take part in arithmetic: the bound
                // applies after the basis is known, and this does not.
                if a.bound.is_some() || b.bound.is_some() {
                    return None;
                }
                Term::Length(CalcLength::new(
                    a.px + sign * b.px,
                    a.percent + sign * b.percent,
                ))
            }
        };
    }
    Some(left)
}

/// `a * b / c`
fn product(args: &[Value], index: &mut usize, ctx: &LengthContext) -> Option<Term> {
    let mut left = unary(args, index, ctx)?;
    while let Some(op) = operator_at(args, *index) {
        if op != "*" && op != "/" {
            break;
        }
        *index += 1;
        let right = unary(args, index, ctx)?;
        left = match (op, left, right) {
            ("*", Term::Number(a), Term::Number(b)) => Term::Number(a * b),
            ("*", Term::Length(length), Term::Number(factor))
            | ("*", Term::Number(factor), Term::Length(length))
                if length.bound.is_none() =>
            {
                Term::Length(CalcLength::new(length.px * factor, length.percent * factor))
            }
            ("/", Term::Number(a), Term::Number(b)) if b != 0.0 => Term::Number(a / b),
            ("/", Term::Length(length), Term::Number(divisor))
                if divisor != 0.0 && length.bound.is_none() =>
            {
                Term::Length(CalcLength::new(
                    length.px / divisor,
                    length.percent / divisor,
                ))
            }
            // Multiplying two lengths, or dividing by one, is not a length.
            _ => return None,
        };
    }
    Some(left)
}

/// A leading `-` or `+`, which the tokenizer only separates when it is spaced.
fn unary(args: &[Value], index: &mut usize, ctx: &LengthContext) -> Option<Term> {
    if let Some(op) = operator_at(args, *index) {
        if op == "-" || op == "+" {
            *index += 1;
            let inner = unary(args, index, ctx)?;
            if op == "+" {
                return Some(inner);
            }
            return Some(match inner {
                Term::Number(value) => Term::Number(-value),
                Term::Length(length) if length.bound.is_none() => {
                    Term::Length(CalcLength::new(-length.px, -length.percent))
                }
                Term::Length(_) => return None,
            });
        }
    }
    let value = args.get(*index)?;
    *index += 1;
    term(value, ctx)
}

fn primary(value: &Value, ctx: &LengthContext) -> Option<Term> {
    match value {
        Value::Number(number) => Some(Term::Number(*number)),
        Value::Percentage(percent) => Some(Term::Length(CalcLength::percent(*percent))),
        Value::Dimension(number, unit) if unit.is_length() => {
            Some(Term::Length(CalcLength::px(ctx.to_px(*number, *unit))))
        }
        // An angle or a time in a length expression is not a length; neither is
        // a keyword, which is what an unsubstituted `var()` leaves behind.
        Value::Dimension(_, Unit::Unknown) | Value::Keyword(_) => None,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::values::parse_value_str;

    fn ctx() -> LengthContext {
        LengthContext::new(16.0, 20.0, (1000.0, 500.0))
    }

    fn calc(source: &str) -> Option<CalcLength> {
        evaluate(&parse_value_str(source), &ctx())
    }

    fn px(source: &str) -> Option<f32> {
        calc(source).and_then(|length| length.resolve_definite())
    }

    #[test]
    fn addition_and_subtraction() {
        assert_eq!(px("calc(10px + 5px)"), Some(15.0));
        assert_eq!(px("calc(10px - 5px)"), Some(5.0));
        assert_eq!(px("calc(10px + 5px - 3px)"), Some(12.0));
        assert_eq!(
            px("calc(1px - 5px)"),
            Some(-4.0),
            "a negative result is fine"
        );
    }

    #[test]
    fn multiplication_and_division() {
        assert_eq!(px("calc(10px * 3)"), Some(30.0));
        assert_eq!(px("calc(3 * 10px)"), Some(30.0));
        assert_eq!(px("calc(30px / 3)"), Some(10.0));
        assert_eq!(px("calc(2 * 3 * 4px)"), Some(24.0));
    }

    #[test]
    fn precedence_puts_products_first() {
        assert_eq!(px("calc(10px + 2 * 5px)"), Some(20.0));
        assert_eq!(px("calc(2 * 5px + 10px)"), Some(20.0));
        assert_eq!(px("calc(20px - 10px / 2)"), Some(15.0));
    }

    #[test]
    fn parentheses_group() {
        assert_eq!(px("calc((10px + 10px) * 2)"), Some(40.0));
        assert_eq!(px("calc(2 * (5px + 5px))"), Some(20.0));
        assert_eq!(px("calc((100px - 20px) / 4)"), Some(20.0));
    }

    #[test]
    fn units_are_converted_before_the_arithmetic() {
        assert_eq!(px("calc(1em + 4px)"), Some(20.0), "1em is 16px here");
        assert_eq!(px("calc(1rem + 0px)"), Some(20.0), "1rem is the root size");
        assert_eq!(px("calc(10vw)"), Some(100.0));
        assert_eq!(px("calc(10vh)"), Some(50.0));
        assert_eq!(px("calc(1in - 1px)"), Some(95.0));
    }

    #[test]
    fn a_percentage_keeps_its_basis() {
        let mixed = calc("calc(100% - 20px)").unwrap();
        assert!(mixed.needs_basis());
        assert_eq!(mixed.percent, 100.0);
        assert_eq!(mixed.px, -20.0);
        assert_eq!(mixed.resolve(200.0), 180.0);
        assert_eq!(mixed.resolve_definite(), None);
    }

    #[test]
    fn percentages_combine_with_each_other() {
        let value = calc("calc(50% + 25%)").unwrap();
        assert_eq!(value.percent, 75.0);
        assert_eq!(calc("calc(50% * 2)").unwrap().percent, 100.0);
        assert_eq!(calc("calc(100% / 4)").unwrap().percent, 25.0);
    }

    #[test]
    fn a_bare_number_is_not_a_length() {
        assert_eq!(calc("calc(2 * 3)"), None);
        assert_eq!(
            evaluate_number(&parse_value_str("calc(2 * 3)"), &ctx()),
            Some(6.0)
        );
        assert_eq!(
            evaluate_number(&parse_value_str("calc(1.2 + 0.3)"), &ctx()),
            Some(1.5)
        );
        assert_eq!(
            evaluate_number(&parse_value_str("calc(10px + 1px)"), &ctx()),
            None
        );
    }

    #[test]
    fn zero_works_as_either() {
        assert_eq!(px("calc(0 + 10px)"), Some(10.0));
        assert_eq!(px("calc(10px + 0)"), Some(10.0));
    }

    #[test]
    fn invalid_expressions_are_rejected() {
        assert_eq!(calc("calc(10px * 10px)"), None, "a length times a length");
        assert_eq!(calc("calc(10px / 0)"), None, "division by zero");
        assert_eq!(calc("calc(10px + red)"), None, "a keyword is not a length");
        assert_eq!(calc("calc(10px +)"), None, "a missing operand");
        assert_eq!(calc("calc()"), None, "nothing to evaluate");
        assert_eq!(calc("calc(10deg + 1px)"), None, "an angle is not a length");
        assert_eq!(
            calc("calc(var(--gone) + 1px)"),
            None,
            "an unsubstituted var"
        );
    }

    #[test]
    fn negative_operands() {
        assert_eq!(px("calc(-10px + 20px)"), Some(10.0));
        assert_eq!(px("calc(20px + -10px)"), Some(10.0));
        assert_eq!(px("calc(-1 * 10px)"), Some(-10.0));
    }

    #[test]
    fn min_and_max_pick_an_argument() {
        assert_eq!(px("min(10px, 20px)"), Some(10.0));
        assert_eq!(px("max(10px, 20px)"), Some(20.0));
        assert_eq!(px("min(1em, 8px)"), Some(8.0), "after unit conversion");
        assert_eq!(px("max(1px, 2px, 3px)"), Some(3.0));
        assert_eq!(calc("min(50%, 80%)").unwrap().percent, 50.0);
    }

    #[test]
    fn clamp_bounds_its_middle_argument() {
        assert_eq!(px("clamp(10px, 5px, 20px)"), Some(10.0), "below the floor");
        assert_eq!(px("clamp(10px, 15px, 20px)"), Some(15.0), "inside");
        assert_eq!(
            px("clamp(10px, 50px, 20px)"),
            Some(20.0),
            "above the ceiling"
        );
        assert_eq!(calc("clamp(1px, 2px)"), None, "clamp needs three arguments");
    }

    #[test]
    fn a_comparison_that_needs_the_basis_keeps_the_bound() {
        // Whether 50% is smaller than 100px depends on the basis, so the limit
        // travels with the value instead of being decided here. This is the
        // shape of `width: min(100%, 420px)`.
        let bounded = calc("min(50%, 100px)").unwrap();
        assert_eq!(bounded.percent, 50.0);
        assert_eq!(
            bounded.bound,
            Some(Bound {
                px: 100.0,
                largest: false
            })
        );
        assert!(bounded.needs_basis());
        assert_eq!(
            bounded.resolve(100.0),
            50.0,
            "50% of 100 is under the limit"
        );
        assert_eq!(bounded.resolve(400.0), 100.0, "and over it here");

        let at_least = calc("max(50%, 100px)").unwrap();
        assert_eq!(at_least.resolve(100.0), 100.0);
        assert_eq!(at_least.resolve(400.0), 200.0);
    }

    #[test]
    fn what_a_bound_cannot_do() {
        assert_eq!(
            calc("min(100% - 10px, 20px)"),
            None,
            "an operator is ambiguous"
        );
        assert_eq!(
            calc("clamp(10px, 50%, 20px)"),
            None,
            "a three-way clamp needs two bounds"
        );
        assert_eq!(
            calc("min(25%, 50%, 100px)"),
            None,
            "and so does more than one relative argument"
        );
        assert_eq!(
            calc("calc(min(50%, 100px) + 5px)"),
            None,
            "a bound cannot take part in arithmetic"
        );
    }

    #[test]
    fn nested_functions() {
        assert_eq!(px("calc(min(10px, 20px) + 5px)"), Some(15.0));
        assert_eq!(px("min(calc(4px + 4px), 20px)"), Some(8.0));
        assert_eq!(px("calc(calc(2px + 2px) * 3)"), Some(12.0));
    }

    #[test]
    fn recognising_maths_functions() {
        for source in [
            "calc(1px)",
            "min(1px, 2px)",
            "max(1px)",
            "clamp(1px, 2px, 3px)",
        ] {
            assert!(is_math_function(&parse_value_str(source)), "{source}");
        }
        assert!(!is_math_function(&parse_value_str("10px")));
        assert!(!is_math_function(&parse_value_str("url(a.png)")));
    }
}
