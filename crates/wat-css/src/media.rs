//! Media query parsing and evaluation.

use crate::tokenizer::{tokenize, Token};
use crate::values::{LengthContext, Unit, Value};

/// The environment a media query is evaluated against.
#[derive(Clone, Copy, Debug)]
pub struct MediaContext {
    pub width: f32,
    pub height: f32,
    /// Device pixel ratio, for `resolution` queries.
    pub device_pixel_ratio: f32,
    pub prefers_dark: bool,
    pub prefers_reduced_motion: bool,
    /// True on touch-first devices, driving `pointer: coarse`.
    pub coarse_pointer: bool,
    /// `screen`, `print`, …
    pub media_type: MediaType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaType {
    Screen,
    Print,
    All,
}

impl MediaType {
    fn parse(name: &str) -> Option<MediaType> {
        match name.to_ascii_lowercase().as_str() {
            "screen" => Some(MediaType::Screen),
            "print" => Some(MediaType::Print),
            "all" => Some(MediaType::All),
            // `speech`, `tty`, … are recognised but never match `screen`.
            _ => None,
        }
    }

    fn accepts(self, actual: MediaType) -> bool {
        self == MediaType::All || actual == MediaType::All || self == actual
    }
}

impl Default for MediaContext {
    fn default() -> Self {
        MediaContext {
            width: 1280.0,
            height: 800.0,
            device_pixel_ratio: 1.0,
            prefers_dark: false,
            prefers_reduced_motion: false,
            coarse_pointer: false,
            media_type: MediaType::Screen,
        }
    }
}

impl MediaContext {
    pub fn screen(width: f32, height: f32) -> Self {
        MediaContext {
            width,
            height,
            ..Default::default()
        }
    }

    pub fn with_dark(mut self, dark: bool) -> Self {
        self.prefers_dark = dark;
        self
    }

    pub fn with_coarse_pointer(mut self, coarse: bool) -> Self {
        self.coarse_pointer = coarse;
        self
    }

    fn length_context(&self) -> LengthContext {
        LengthContext::new(16.0, 16.0, (self.width, self.height))
    }
}

/// A single `(feature: value)` test, or a bare `(feature)`.
#[derive(Clone, Debug, PartialEq)]
pub struct MediaFeature {
    pub name: String,
    pub value: Option<Value>,
}

impl MediaFeature {
    fn evaluate(&self, ctx: &MediaContext) -> bool {
        let lengths = ctx.length_context();
        let as_px = |basis: f32| {
            self.value
                .as_ref()
                .and_then(|v| v.to_px(&lengths, Some(basis)))
        };
        let as_number = || self.value.as_ref().and_then(Value::as_number);
        let as_keyword = || {
            self.value
                .as_ref()
                .and_then(Value::as_keyword)
                .map(str::to_ascii_lowercase)
        };

        match self.name.as_str() {
            "width" => as_px(ctx.width).is_some_and(|v| (ctx.width - v).abs() < 0.5),
            "min-width" => as_px(ctx.width).is_some_and(|v| ctx.width >= v),
            "max-width" => as_px(ctx.width).is_some_and(|v| ctx.width <= v),
            "height" => as_px(ctx.height).is_some_and(|v| (ctx.height - v).abs() < 0.5),
            "min-height" => as_px(ctx.height).is_some_and(|v| ctx.height >= v),
            "max-height" => as_px(ctx.height).is_some_and(|v| ctx.height <= v),
            "aspect-ratio" | "min-aspect-ratio" | "max-aspect-ratio" => {
                // Ratios are written `16 / 9`, which parses as a list.
                let items = self.value.as_ref().map(Value::items).unwrap_or_default();
                let numbers: Vec<f32> = items.iter().filter_map(Value::as_number).collect();
                let Some(target) = (match numbers.len() {
                    1 => Some(numbers[0]),
                    2.. if numbers[1] != 0.0 => Some(numbers[0] / numbers[1]),
                    _ => None,
                }) else {
                    return false;
                };
                let actual = ctx.width / ctx.height.max(1.0);
                match self.name.as_str() {
                    "min-aspect-ratio" => actual >= target,
                    "max-aspect-ratio" => actual <= target,
                    _ => (actual - target).abs() < 0.01,
                }
            }
            "orientation" => match as_keyword().as_deref() {
                Some("portrait") => ctx.height >= ctx.width,
                Some("landscape") => ctx.width > ctx.height,
                _ => false,
            },
            "prefers-color-scheme" => match as_keyword().as_deref() {
                Some("dark") => ctx.prefers_dark,
                Some("light") => !ctx.prefers_dark,
                _ => false,
            },
            "prefers-reduced-motion" => match as_keyword().as_deref() {
                Some("reduce") => ctx.prefers_reduced_motion,
                Some("no-preference") | None => !ctx.prefers_reduced_motion,
                _ => false,
            },
            "pointer" | "any-pointer" => match as_keyword().as_deref() {
                Some("coarse") => ctx.coarse_pointer,
                Some("fine") => !ctx.coarse_pointer,
                Some("none") => false,
                _ => false,
            },
            "hover" | "any-hover" => match as_keyword().as_deref() {
                Some("hover") => !ctx.coarse_pointer,
                Some("none") => ctx.coarse_pointer,
                _ => false,
            },
            "resolution" | "min-resolution" | "max-resolution" => {
                let dppx = match self.value.as_ref() {
                    Some(Value::Dimension(v, Unit::Unknown)) => *v,
                    Some(Value::Number(v)) => *v,
                    _ => return false,
                };
                match self.name.as_str() {
                    "min-resolution" => ctx.device_pixel_ratio >= dppx,
                    "max-resolution" => ctx.device_pixel_ratio <= dppx,
                    _ => (ctx.device_pixel_ratio - dppx).abs() < 0.01,
                }
            }
            // A bare feature name asks "is it non-zero?".
            "color" | "monochrome" => match as_number() {
                Some(bits) => (bits > 0.0) == (self.name == "color"),
                None => self.name == "color",
            },
            _ => false,
        }
    }
}

/// One query in a comma-separated media query list.
#[derive(Clone, Debug, PartialEq)]
pub struct MediaQuery {
    pub negated: bool,
    pub media_type: Option<MediaType>,
    pub features: Vec<MediaFeature>,
    /// Set when the query used syntax we do not understand; such a query never
    /// matches, which is the spec's error behaviour.
    pub invalid: bool,
}

impl MediaQuery {
    fn evaluate(&self, ctx: &MediaContext) -> bool {
        if self.invalid {
            return false;
        }
        let type_ok = self
            .media_type
            .is_none_or(|wanted| wanted.accepts(ctx.media_type));
        let features_ok = self.features.iter().all(|f| f.evaluate(ctx));
        let result = type_ok && features_ok;
        result != self.negated
    }
}

/// A comma-separated list; the list matches if any query matches.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediaQueryList {
    pub queries: Vec<MediaQuery>,
}

impl MediaQueryList {
    /// An empty list matches everything, as `@media {}` and a missing `media`
    /// attribute both do.
    pub fn matches(&self, ctx: &MediaContext) -> bool {
        if self.queries.is_empty() {
            return true;
        }
        self.queries.iter().any(|q| q.evaluate(ctx))
    }

    pub fn parse(input: &str) -> MediaQueryList {
        let tokens = tokenize(input);
        let mut queries = Vec::new();
        for chunk in tokens.split(|t| matches!(t, Token::Comma)) {
            if chunk.iter().all(Token::is_whitespace) {
                continue;
            }
            queries.push(parse_query(chunk));
        }
        MediaQueryList { queries }
    }
}

fn parse_query(tokens: &[Token]) -> MediaQuery {
    let mut query = MediaQuery {
        negated: false,
        media_type: None,
        features: Vec::new(),
        invalid: false,
    };
    let mut index = 0;
    let significant: Vec<&Token> = tokens.iter().filter(|t| !t.is_whitespace()).collect();

    while index < significant.len() {
        match significant[index] {
            Token::Ident(word) => {
                let lower = word.to_ascii_lowercase();
                match lower.as_str() {
                    "not" if query.media_type.is_none() && query.features.is_empty() => {
                        query.negated = true;
                    }
                    // `only` exists to hide the query from ancient parsers.
                    "only" => {}
                    "and" => {}
                    other => match MediaType::parse(other) {
                        Some(media_type) if query.media_type.is_none() => {
                            query.media_type = Some(media_type)
                        }
                        // An unknown media type can never match.
                        _ => query.invalid = true,
                    },
                }
                index += 1;
            }
            Token::ParenOpen => {
                let mut depth = 1;
                let mut inner: Vec<Token> = Vec::new();
                index += 1;
                while index < significant.len() {
                    match significant[index] {
                        Token::ParenOpen | Token::Function(_) => depth += 1,
                        Token::ParenClose => {
                            depth -= 1;
                            if depth == 0 {
                                index += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    inner.push(significant[index].clone());
                    index += 1;
                }
                match parse_feature(&inner) {
                    Some(feature) => query.features.push(feature),
                    None => query.invalid = true,
                }
            }
            _ => {
                query.invalid = true;
                index += 1;
            }
        }
    }

    query
}

fn parse_feature(tokens: &[Token]) -> Option<MediaFeature> {
    let mut iter = tokens.iter();
    let name = match iter.next()? {
        Token::Ident(name) => name.to_ascii_lowercase(),
        _ => return None,
    };
    match iter.next() {
        None => Some(MediaFeature { name, value: None }),
        Some(Token::Colon) => {
            let rest: Vec<Token> = iter.cloned().collect();
            let value = crate::values::parse_value(&rest);
            Some(MediaFeature {
                name,
                value: Some(value),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(query: &str, ctx: &MediaContext) -> bool {
        MediaQueryList::parse(query).matches(ctx)
    }

    #[test]
    fn width_features() {
        let ctx = MediaContext::screen(500.0, 900.0);
        assert!(matches("(max-width: 600px)", &ctx));
        assert!(!matches("(min-width: 600px)", &ctx));
        assert!(matches("(min-width: 500px)", &ctx));
        assert!(matches("(width: 500px)", &ctx));
    }

    #[test]
    fn media_types() {
        let ctx = MediaContext::screen(1000.0, 800.0);
        assert!(matches("screen", &ctx));
        assert!(!matches("print", &ctx));
        assert!(matches("all", &ctx));
        assert!(matches("screen and (min-width: 900px)", &ctx));
        assert!(!matches("print and (min-width: 900px)", &ctx));
        assert!(!matches("tty", &ctx), "unknown types never match");
    }

    #[test]
    fn negation() {
        let ctx = MediaContext::screen(500.0, 900.0);
        assert!(matches("not (min-width: 600px)", &ctx));
        assert!(!matches("not (max-width: 600px)", &ctx));
        assert!(matches("only screen and (max-width: 600px)", &ctx));
    }

    #[test]
    fn comma_list_is_a_union() {
        let ctx = MediaContext::screen(500.0, 900.0);
        assert!(matches("print, (max-width: 600px)", &ctx));
        assert!(!matches("print, (min-width: 600px)", &ctx));
    }

    #[test]
    fn orientation_and_preferences() {
        let portrait = MediaContext::screen(400.0, 800.0);
        assert!(matches("(orientation: portrait)", &portrait));
        assert!(!matches("(orientation: landscape)", &portrait));

        let dark = MediaContext::screen(400.0, 800.0).with_dark(true);
        assert!(matches("(prefers-color-scheme: dark)", &dark));
        assert!(!matches("(prefers-color-scheme: light)", &dark));
        assert!(matches("(prefers-color-scheme: light)", &portrait));
    }

    #[test]
    fn pointer_type() {
        let touch = MediaContext::screen(390.0, 844.0).with_coarse_pointer(true);
        assert!(matches("(pointer: coarse)", &touch));
        assert!(!matches("(pointer: fine)", &touch));
        assert!(matches("(hover: none)", &touch));
    }

    #[test]
    fn relative_units_use_the_viewport() {
        let ctx = MediaContext::screen(1000.0, 500.0);
        assert!(matches("(min-width: 50em)", &ctx), "50em == 800px");
        assert!(!matches("(min-width: 100em)", &ctx));
    }

    #[test]
    fn empty_and_garbage_queries() {
        let ctx = MediaContext::default();
        assert!(MediaQueryList::default().matches(&ctx));
        assert!(matches("", &ctx));
        assert!(!matches("(bogus-feature: 3)", &ctx));
        assert!(!matches("((", &ctx));
    }

    #[test]
    fn aspect_ratio() {
        let ctx = MediaContext::screen(1600.0, 900.0);
        assert!(matches("(min-aspect-ratio: 16/10)", &ctx));
        assert!(!matches("(max-aspect-ratio: 4/3)", &ctx));
    }
}
