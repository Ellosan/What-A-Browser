//! Parsed CSS component values and length resolution.

use crate::color::Color;
use crate::tokenizer::Token;

/// A CSS unit. `Number` covers unitless values such as `line-height: 1.5`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Px,
    Em,
    Rem,
    Ex,
    Ch,
    Vw,
    Vh,
    Vmin,
    Vmax,
    Pt,
    Pc,
    In,
    Cm,
    Mm,
    Q,
    Deg,
    Rad,
    Grad,
    Turn,
    Sec,
    Ms,
    Fr,
    Unknown,
}

impl Unit {
    pub fn parse(text: &str) -> Unit {
        match text {
            "px" => Unit::Px,
            "em" => Unit::Em,
            "rem" => Unit::Rem,
            "ex" => Unit::Ex,
            "ch" => Unit::Ch,
            "vw" => Unit::Vw,
            "vh" => Unit::Vh,
            "vmin" => Unit::Vmin,
            "vmax" => Unit::Vmax,
            "pt" => Unit::Pt,
            "pc" => Unit::Pc,
            "in" => Unit::In,
            "cm" => Unit::Cm,
            "mm" => Unit::Mm,
            "q" => Unit::Q,
            "deg" => Unit::Deg,
            "rad" => Unit::Rad,
            "grad" => Unit::Grad,
            "turn" => Unit::Turn,
            "s" => Unit::Sec,
            "ms" => Unit::Ms,
            "fr" => Unit::Fr,
            _ => Unit::Unknown,
        }
    }

    /// Is this a length unit (as opposed to an angle or time)?
    pub fn is_length(self) -> bool {
        matches!(
            self,
            Unit::Px
                | Unit::Em
                | Unit::Rem
                | Unit::Ex
                | Unit::Ch
                | Unit::Vw
                | Unit::Vh
                | Unit::Vmin
                | Unit::Vmax
                | Unit::Pt
                | Unit::Pc
                | Unit::In
                | Unit::Cm
                | Unit::Mm
                | Unit::Q
        )
    }

    pub fn is_angle(self) -> bool {
        matches!(self, Unit::Deg | Unit::Rad | Unit::Grad | Unit::Turn)
    }
}

/// Everything a relative length needs in order to become pixels.
#[derive(Clone, Copy, Debug)]
pub struct LengthContext {
    /// Font size of the element the length belongs to.
    pub font_size: f32,
    /// Font size of the root element.
    pub root_font_size: f32,
    pub viewport_width: f32,
    pub viewport_height: f32,
}

impl Default for LengthContext {
    fn default() -> Self {
        LengthContext {
            font_size: 16.0,
            root_font_size: 16.0,
            viewport_width: 1280.0,
            viewport_height: 800.0,
        }
    }
}

impl LengthContext {
    pub fn new(font_size: f32, root_font_size: f32, viewport: (f32, f32)) -> Self {
        LengthContext {
            font_size,
            root_font_size,
            viewport_width: viewport.0,
            viewport_height: viewport.1,
        }
    }

    /// Converts `value` in `unit` to CSS pixels.
    pub fn to_px(&self, value: f32, unit: Unit) -> f32 {
        match unit {
            Unit::Px => value,
            Unit::Em => value * self.font_size,
            Unit::Rem => value * self.root_font_size,
            // Approximations: real values would come from font metrics.
            Unit::Ex => value * self.font_size * 0.5,
            Unit::Ch => value * self.font_size * 0.5,
            Unit::Vw => value * self.viewport_width / 100.0,
            Unit::Vh => value * self.viewport_height / 100.0,
            Unit::Vmin => value * self.viewport_width.min(self.viewport_height) / 100.0,
            Unit::Vmax => value * self.viewport_width.max(self.viewport_height) / 100.0,
            Unit::Pt => value * 96.0 / 72.0,
            Unit::Pc => value * 16.0,
            Unit::In => value * 96.0,
            Unit::Cm => value * 96.0 / 2.54,
            Unit::Mm => value * 96.0 / 25.4,
            Unit::Q => value * 96.0 / 101.6,
            _ => value,
        }
    }
}

/// A parsed component value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Keyword(String),
    Number(f32),
    Dimension(f32, Unit),
    Percentage(f32),
    Color(Color),
    Str(String),
    Url(String),
    Function {
        name: String,
        args: Vec<Value>,
    },
    /// Space-separated values, e.g. `1px solid red`.
    List(Vec<Value>),
    /// Comma-separated values, e.g. a font stack or gradient stop list.
    Commas(Vec<Value>),
}

impl Value {
    pub fn keyword(name: &str) -> Value {
        Value::Keyword(name.to_string())
    }

    pub fn px(value: f32) -> Value {
        Value::Dimension(value, Unit::Px)
    }

    pub fn as_keyword(&self) -> Option<&str> {
        match self {
            Value::Keyword(k) => Some(k),
            _ => None,
        }
    }

    pub fn is_keyword(&self, name: &str) -> bool {
        self.as_keyword()
            .is_some_and(|k| k.eq_ignore_ascii_case(name))
    }

    pub fn as_number(&self) -> Option<f32> {
        match self {
            Value::Number(n) => Some(*n),
            // A unitless zero is valid wherever a number is.
            Value::Dimension(n, _) => Some(*n),
            _ => None,
        }
    }

    pub fn as_str_value(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            Value::Keyword(k) => Some(k),
            _ => None,
        }
    }

    pub fn as_url(&self) -> Option<&str> {
        match self {
            Value::Url(u) => Some(u),
            _ => None,
        }
    }

    /// Resolves this value to a colour, including named keywords.
    pub fn to_color(&self) -> Option<Color> {
        match self {
            Value::Color(c) => Some(*c),
            Value::Keyword(name) => crate::color::parse_named(&name.to_ascii_lowercase()),
            _ => None,
        }
    }

    /// Resolves a length, treating percentages against `basis` when given.
    pub fn to_px(&self, ctx: &LengthContext, basis: Option<f32>) -> Option<f32> {
        match self {
            Value::Dimension(value, unit) if unit.is_length() => Some(ctx.to_px(*value, *unit)),
            Value::Number(value) if *value == 0.0 => Some(0.0),
            Value::Percentage(pct) => basis.map(|b| b * pct / 100.0),
            _ => None,
        }
    }

    /// Angle in degrees.
    pub fn to_degrees(&self) -> Option<f32> {
        match self {
            Value::Dimension(value, Unit::Deg) => Some(*value),
            Value::Dimension(value, Unit::Grad) => Some(value * 0.9),
            Value::Dimension(value, Unit::Turn) => Some(value * 360.0),
            Value::Dimension(value, Unit::Rad) => Some(value * 180.0 / std::f32::consts::PI),
            Value::Number(value) => Some(*value),
            _ => None,
        }
    }

    /// Time in seconds.
    pub fn to_seconds(&self) -> Option<f32> {
        match self {
            Value::Dimension(value, Unit::Sec) => Some(*value),
            Value::Dimension(value, Unit::Ms) => Some(value / 1000.0),
            Value::Number(value) if *value == 0.0 => Some(0.0),
            _ => None,
        }
    }

    /// A single value behaves as a one-element list.
    pub fn items(&self) -> &[Value] {
        match self {
            Value::List(items) | Value::Commas(items) => items,
            other => std::slice::from_ref(other),
        }
    }

    /// Comma-separated segments; a non-comma value yields itself.
    pub fn comma_items(&self) -> Vec<&Value> {
        match self {
            Value::Commas(items) => items.iter().collect(),
            other => vec![other],
        }
    }

    /// Serialises back to CSS-ish text. Used for debugging and `style` output.
    pub fn to_css(&self) -> String {
        match self {
            Value::Keyword(k) => k.clone(),
            Value::Number(n) => format_number(*n),
            Value::Dimension(n, unit) => format!("{}{}", format_number(*n), unit_suffix(*unit)),
            Value::Percentage(p) => format!("{}%", format_number(*p)),
            Value::Color(c) => c.to_hex(),
            Value::Str(s) => format!("{s:?}"),
            Value::Url(u) => format!("url({u})"),
            Value::Function { name, args } => {
                let inner: Vec<String> = args.iter().map(Value::to_css).collect();
                format!("{}({})", name, inner.join(", "))
            }
            Value::List(items) => items
                .iter()
                .map(Value::to_css)
                .collect::<Vec<_>>()
                .join(" "),
            Value::Commas(items) => items
                .iter()
                .map(Value::to_css)
                .collect::<Vec<_>>()
                .join(", "),
        }
    }
}

fn format_number(n: f32) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        let text = format!("{n:.4}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn unit_suffix(unit: Unit) -> &'static str {
    match unit {
        Unit::Px => "px",
        Unit::Em => "em",
        Unit::Rem => "rem",
        Unit::Ex => "ex",
        Unit::Ch => "ch",
        Unit::Vw => "vw",
        Unit::Vh => "vh",
        Unit::Vmin => "vmin",
        Unit::Vmax => "vmax",
        Unit::Pt => "pt",
        Unit::Pc => "pc",
        Unit::In => "in",
        Unit::Cm => "cm",
        Unit::Mm => "mm",
        Unit::Q => "q",
        Unit::Deg => "deg",
        Unit::Rad => "rad",
        Unit::Grad => "grad",
        Unit::Turn => "turn",
        Unit::Sec => "s",
        Unit::Ms => "ms",
        Unit::Fr => "fr",
        Unit::Unknown => "",
    }
}

/// Parses a run of tokens (the right-hand side of a declaration) into a value.
pub fn parse_value(tokens: &[Token]) -> Value {
    let mut index = 0;
    let value = parse_comma_list(tokens, &mut index);
    value.unwrap_or_else(|| Value::Keyword(String::new()))
}

/// Parses a declaration value from source text.
pub fn parse_value_str(input: &str) -> Value {
    parse_value(&crate::tokenizer::tokenize(input))
}

fn parse_comma_list(tokens: &[Token], index: &mut usize) -> Option<Value> {
    let mut segments = Vec::new();
    loop {
        let segment = parse_space_list(tokens, index);
        if let Some(segment) = segment {
            segments.push(segment);
        }
        match tokens.get(*index) {
            Some(Token::Comma) => {
                *index += 1;
                continue;
            }
            _ => break,
        }
    }
    match segments.len() {
        0 => None,
        1 => segments.pop(),
        _ => Some(Value::Commas(segments)),
    }
}

fn parse_space_list(tokens: &[Token], index: &mut usize) -> Option<Value> {
    let mut items = Vec::new();
    loop {
        while matches!(tokens.get(*index), Some(Token::Whitespace)) {
            *index += 1;
        }
        match tokens.get(*index) {
            None
            | Some(Token::Comma)
            | Some(Token::Semicolon)
            | Some(Token::BraceClose)
            | Some(Token::ParenClose) => break,
            _ => {}
        }
        match parse_single(tokens, index) {
            Some(value) => items.push(value),
            None => break,
        }
    }
    match items.len() {
        0 => None,
        1 => items.pop(),
        _ => Some(Value::List(items)),
    }
}

fn parse_single(tokens: &[Token], index: &mut usize) -> Option<Value> {
    let token = tokens.get(*index)?;
    *index += 1;
    let value = match token {
        Token::Ident(name) => Value::Keyword(name.clone()),
        Token::Number(n) => Value::Number(*n),
        Token::Percentage(p) => Value::Percentage(*p),
        Token::Dimension(n, unit) => Value::Dimension(*n, Unit::parse(unit)),
        Token::Str(s) => Value::Str(s.clone()),
        Token::Url(u) => Value::Url(u.clone()),
        Token::Hash(hex) => match crate::color::parse_hex(hex) {
            Some(color) => Value::Color(color),
            None => Value::Keyword(format!("#{hex}")),
        },
        Token::Function(name) => {
            let mut args = Vec::new();
            loop {
                match tokens.get(*index) {
                    None => break,
                    Some(Token::ParenClose) => {
                        *index += 1;
                        break;
                    }
                    Some(Token::Comma) | Some(Token::Whitespace) => {
                        *index += 1;
                    }
                    _ => match parse_single(tokens, index) {
                        Some(arg) => args.push(arg),
                        None => break,
                    },
                }
            }
            let lower = name.to_ascii_lowercase();
            // Colour functions collapse to a concrete colour immediately.
            if matches!(lower.as_str(), "rgb" | "rgba" | "hsl" | "hsla") {
                let text: Vec<String> = args.iter().map(Value::to_css).collect();
                match crate::color::parse_function(&lower, &text.join(" ")) {
                    Some(color) => Value::Color(color),
                    None => Value::Function { name: lower, args },
                }
            } else {
                Value::Function { name: lower, args }
            }
        }
        // A slash separates values in shorthands like `font: 12px/1.5 serif`.
        Token::Delim(ch) => Value::Keyword(ch.to_string()),
        Token::Colon => Value::Keyword(":".into()),
        Token::BracketOpen => Value::Keyword("[".into()),
        Token::BracketClose => Value::Keyword("]".into()),
        Token::ParenOpen => Value::Keyword("(".into()),
        Token::AtKeyword(name) => Value::Keyword(format!("@{name}")),
        Token::Whitespace
        | Token::Comma
        | Token::Semicolon
        | Token::BraceOpen
        | Token::BraceClose
        | Token::ParenClose => return None,
    };
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_values() {
        assert_eq!(parse_value_str("red"), Value::Keyword("red".into()));
        assert_eq!(parse_value_str("12px"), Value::Dimension(12.0, Unit::Px));
        assert_eq!(parse_value_str("50%"), Value::Percentage(50.0));
        assert_eq!(parse_value_str("1.5"), Value::Number(1.5));
    }

    #[test]
    fn space_separated_list() {
        let value = parse_value_str("1px solid red");
        assert_eq!(
            value,
            Value::List(vec![
                Value::Dimension(1.0, Unit::Px),
                Value::Keyword("solid".into()),
                Value::Keyword("red".into()),
            ])
        );
        assert_eq!(value.items().len(), 3);
    }

    #[test]
    fn comma_separated_list() {
        let value = parse_value_str("Helvetica, \"Segoe UI\", sans-serif");
        let items = value.comma_items();
        assert_eq!(items.len(), 3);
        assert_eq!(items[1].as_str_value(), Some("Segoe UI"));
    }

    #[test]
    fn colors_collapse_to_color_values() {
        assert_eq!(
            parse_value_str("#ff0000"),
            Value::Color(Color::rgb(255, 0, 0))
        );
        assert_eq!(
            parse_value_str("rgba(0,0,0,.5)"),
            Value::Color(Color::rgba(0, 0, 0, 128))
        );
        assert_eq!(
            parse_value_str("hsl(120 100% 50%)"),
            Value::Color(Color::rgb(0, 255, 0))
        );
    }

    #[test]
    fn keyword_colors_resolve_on_demand() {
        assert_eq!(
            parse_value_str("red").to_color(),
            Some(Color::rgb(255, 0, 0))
        );
        assert_eq!(parse_value_str("inherit").to_color(), None);
    }

    #[test]
    fn unknown_functions_are_preserved() {
        let value = parse_value_str("calc(100% - 20px)");
        let Value::Function { name, args } = &value else {
            panic!("expected function, got {value:?}");
        };
        assert_eq!(name, "calc");
        assert_eq!(args.len(), 3);
    }

    #[test]
    fn nested_functions() {
        let value = parse_value_str("linear-gradient(180deg, rgba(0,0,0,0.2), transparent)");
        let Value::Function { name, args } = &value else {
            panic!("expected function");
        };
        assert_eq!(name, "linear-gradient");
        assert_eq!(args.len(), 3);
        assert_eq!(args[0].to_degrees(), Some(180.0));
        assert_eq!(args[1], Value::Color(Color::rgba(0, 0, 0, 51)));
    }

    #[test]
    fn length_resolution() {
        let ctx = LengthContext::new(20.0, 16.0, (1000.0, 500.0));
        assert_eq!(parse_value_str("2em").to_px(&ctx, None), Some(40.0));
        assert_eq!(parse_value_str("2rem").to_px(&ctx, None), Some(32.0));
        assert_eq!(parse_value_str("10vw").to_px(&ctx, None), Some(100.0));
        assert_eq!(parse_value_str("10vh").to_px(&ctx, None), Some(50.0));
        assert_eq!(parse_value_str("50%").to_px(&ctx, Some(200.0)), Some(100.0));
        assert_eq!(parse_value_str("50%").to_px(&ctx, None), None);
        assert_eq!(parse_value_str("0").to_px(&ctx, None), Some(0.0));
        assert_eq!(parse_value_str("72pt").to_px(&ctx, None), Some(96.0));
    }

    #[test]
    fn css_round_trip() {
        for input in ["1px solid red", "12px", "50%", "1.5"] {
            let value = parse_value_str(input);
            assert_eq!(
                parse_value_str(&value.to_css()),
                value,
                "round trip failed for {input}"
            );
        }
    }

    #[test]
    fn empty_value_does_not_panic() {
        assert_eq!(parse_value_str(""), Value::Keyword(String::new()));
        assert_eq!(parse_value_str("   "), Value::Keyword(String::new()));
    }
}
