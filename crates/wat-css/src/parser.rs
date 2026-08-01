//! Stylesheet parsing: qualified rules, declarations and at-rules.

use crate::media::{MediaContext, MediaQueryList};
use crate::selector::{parse_selector_list, Selector};
use crate::tokenizer::{tokenize, Token};
use crate::values::{parse_value, Value};

/// Where a declaration came from, which decides cascade order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    UserAgent,
    User,
    Author,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Declaration {
    /// Lower-cased property name.
    pub name: String,
    pub value: Value,
    pub important: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StyleRule {
    pub selectors: Vec<Selector>,
    pub declarations: Vec<Declaration>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MediaRule {
    pub condition: MediaQueryList,
    pub rules: Vec<Rule>,
}

/// `@font-face` and `@keyframes` are retained so the engine can report what a
/// page asked for, even though neither is applied yet.
#[derive(Clone, Debug, PartialEq)]
pub enum Rule {
    Style(StyleRule),
    Media(MediaRule),
    Import {
        url: String,
        condition: MediaQueryList,
    },
    FontFace(Vec<Declaration>),
    Keyframes {
        name: String,
    },
    /// Any at-rule we do not model; kept for diagnostics.
    Unknown {
        name: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stylesheet {
    pub rules: Vec<Rule>,
    pub origin: Origin,
    /// URL the sheet was loaded from, used to resolve relative `url()` values.
    pub href: Option<String>,
}

impl Stylesheet {
    pub fn empty(origin: Origin) -> Self {
        Stylesheet {
            rules: Vec::new(),
            origin,
            href: None,
        }
    }

    pub fn parse(input: &str, origin: Origin) -> Self {
        Stylesheet {
            rules: parse_rules(&tokenize(input)),
            origin,
            href: None,
        }
    }

    pub fn parse_with_href(input: &str, origin: Origin, href: impl Into<String>) -> Self {
        let mut sheet = Stylesheet::parse(input, origin);
        sheet.href = Some(href.into());
        sheet
    }

    /// Style rules that apply in `ctx`, flattening matching `@media` blocks.
    pub fn style_rules<'a>(&'a self, ctx: &MediaContext) -> Vec<&'a StyleRule> {
        let mut out = Vec::new();
        collect_style_rules(&self.rules, ctx, &mut out);
        out
    }

    /// `@import` URLs whose media condition matches.
    pub fn imports(&self, ctx: &MediaContext) -> Vec<&str> {
        self.rules
            .iter()
            .filter_map(|rule| match rule {
                Rule::Import { url, condition } if condition.matches(ctx) => Some(url.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Total number of style rules, including those inside `@media`.
    pub fn rule_count(&self) -> usize {
        fn count(rules: &[Rule]) -> usize {
            rules
                .iter()
                .map(|rule| match rule {
                    Rule::Style(_) => 1,
                    Rule::Media(media) => count(&media.rules),
                    _ => 0,
                })
                .sum()
        }
        count(&self.rules)
    }
}

fn collect_style_rules<'a>(rules: &'a [Rule], ctx: &MediaContext, out: &mut Vec<&'a StyleRule>) {
    for rule in rules {
        match rule {
            Rule::Style(style) => out.push(style),
            Rule::Media(media) if media.condition.matches(ctx) => {
                collect_style_rules(&media.rules, ctx, out)
            }
            _ => {}
        }
    }
}

fn parse_rules(tokens: &[Token]) -> Vec<Rule> {
    let mut rules = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        match &tokens[index] {
            Token::Whitespace | Token::Semicolon => index += 1,
            // A stray `}` closes a block we are not in; skip it.
            Token::BraceClose => index += 1,
            Token::AtKeyword(name) => {
                let (rule, next) = parse_at_rule(name.clone(), tokens, index + 1);
                if let Some(rule) = rule {
                    rules.push(rule);
                }
                index = next;
            }
            _ => {
                let (rule, next) = parse_qualified_rule(tokens, index);
                if let Some(rule) = rule {
                    rules.push(Rule::Style(rule));
                }
                index = next;
            }
        }
    }
    rules
}

/// Parses `selectors { declarations }` starting at `start`.
fn parse_qualified_rule(tokens: &[Token], start: usize) -> (Option<StyleRule>, usize) {
    let mut index = start;
    let mut prelude: Vec<Token> = Vec::new();
    while index < tokens.len() {
        match &tokens[index] {
            Token::BraceOpen => break,
            Token::Semicolon => {
                // Declaration-like garbage at the top level; discard it.
                return (None, index + 1);
            }
            other => {
                prelude.push(other.clone());
                index += 1;
            }
        }
    }
    if index >= tokens.len() {
        return (None, tokens.len());
    }
    let (block, next) = take_block(tokens, index);
    let selectors = parse_selector_list(&tokens_to_source(&prelude));
    if selectors.is_empty() {
        return (None, next);
    }
    let declarations = parse_declarations(&block);
    (
        Some(StyleRule {
            selectors,
            declarations,
        }),
        next,
    )
}

fn parse_at_rule(name: String, tokens: &[Token], start: usize) -> (Option<Rule>, usize) {
    let mut index = start;
    let mut prelude: Vec<Token> = Vec::new();
    // The prelude runs up to `{` (block at-rule) or `;` (statement at-rule).
    while index < tokens.len() {
        match &tokens[index] {
            Token::BraceOpen | Token::Semicolon => break,
            other => {
                prelude.push(other.clone());
                index += 1;
            }
        }
    }

    let statement = matches!(tokens.get(index), Some(Token::Semicolon)) || index >= tokens.len();
    if statement {
        let next = index + 1;
        let rule = match name.as_str() {
            "import" => {
                let mut url = None;
                let mut condition_tokens: Vec<Token> = Vec::new();
                for token in &prelude {
                    match token {
                        Token::Url(u) if url.is_none() => url = Some(u.clone()),
                        Token::Str(s) if url.is_none() => url = Some(s.clone()),
                        Token::Whitespace if url.is_none() => {}
                        other if url.is_some() => condition_tokens.push(other.clone()),
                        _ => {}
                    }
                }
                url.map(|url| Rule::Import {
                    url,
                    condition: MediaQueryList::parse(&tokens_to_source(&condition_tokens)),
                })
            }
            "charset" | "namespace" => None,
            other => Some(Rule::Unknown {
                name: other.to_string(),
            }),
        };
        return (rule, next);
    }

    let (block, next) = take_block(tokens, index);
    let rule = match name.as_str() {
        "media" => Some(Rule::Media(MediaRule {
            condition: MediaQueryList::parse(&tokens_to_source(&prelude)),
            rules: parse_rules(&block),
        })),
        // `@supports` conditions are not evaluated; the body is applied, which
        // matches how a browser that supports everything in it would behave.
        "supports" => Some(Rule::Media(MediaRule {
            condition: MediaQueryList::default(),
            rules: parse_rules(&block),
        })),
        "font-face" => Some(Rule::FontFace(parse_declarations(&block))),
        "keyframes" | "-webkit-keyframes" => Some(Rule::Keyframes {
            name: tokens_to_source(&prelude).trim().to_string(),
        }),
        other => Some(Rule::Unknown {
            name: other.to_string(),
        }),
    };
    (rule, next)
}

/// Given `tokens[open]` is `{`, returns the inner tokens and the index past `}`.
fn take_block(tokens: &[Token], open: usize) -> (Vec<Token>, usize) {
    debug_assert!(matches!(tokens.get(open), Some(Token::BraceOpen)));
    let mut depth = 0usize;
    let mut inner = Vec::new();
    let mut index = open;
    while index < tokens.len() {
        match &tokens[index] {
            Token::BraceOpen => {
                depth += 1;
                if depth > 1 {
                    inner.push(Token::BraceOpen);
                }
            }
            Token::BraceClose => {
                depth -= 1;
                if depth == 0 {
                    return (inner, index + 1);
                }
                inner.push(Token::BraceClose);
            }
            other => inner.push(other.clone()),
        }
        index += 1;
    }
    // Unterminated block: everything to end of input is the body.
    (inner, tokens.len())
}

/// Parses a declaration list (the inside of `{ … }` or a `style` attribute).
pub fn parse_declarations(tokens: &[Token]) -> Vec<Declaration> {
    let mut declarations = Vec::new();
    for chunk in split_declarations(tokens) {
        if let Some(declaration) = parse_declaration(&chunk) {
            declarations.push(declaration);
        }
    }
    declarations
}

/// Parses a `style="…"` attribute.
pub fn parse_inline_style(input: &str) -> Vec<Declaration> {
    parse_declarations(&tokenize(input))
}

/// Splits on top-level semicolons.
fn split_declarations(tokens: &[Token]) -> Vec<Vec<Token>> {
    let mut chunks = vec![Vec::new()];
    let mut depth = 0usize;
    for token in tokens {
        match token {
            Token::ParenOpen | Token::BracketOpen | Token::BraceOpen | Token::Function(_) => {
                depth += 1;
                chunks.last_mut().unwrap().push(token.clone());
            }
            Token::ParenClose | Token::BracketClose | Token::BraceClose => {
                depth = depth.saturating_sub(1);
                chunks.last_mut().unwrap().push(token.clone());
            }
            Token::Semicolon if depth == 0 => chunks.push(Vec::new()),
            other => chunks.last_mut().unwrap().push(other.clone()),
        }
    }
    chunks
}

fn parse_declaration(tokens: &[Token]) -> Option<Declaration> {
    let mut index = 0;
    while matches!(tokens.get(index), Some(Token::Whitespace)) {
        index += 1;
    }
    let name = match tokens.get(index)? {
        Token::Ident(name) => name.to_ascii_lowercase(),
        _ => return None,
    };
    index += 1;
    while matches!(tokens.get(index), Some(Token::Whitespace)) {
        index += 1;
    }
    if !matches!(tokens.get(index), Some(Token::Colon)) {
        return None;
    }
    index += 1;

    let mut value_tokens: Vec<Token> = tokens[index..].to_vec();

    // Detect and strip a trailing `!important`.
    let mut important = false;
    let significant: Vec<usize> = value_tokens
        .iter()
        .enumerate()
        .filter(|(_, t)| !t.is_whitespace())
        .map(|(i, _)| i)
        .collect();
    if significant.len() >= 2 {
        let last = significant[significant.len() - 1];
        let before = significant[significant.len() - 2];
        if matches!(&value_tokens[before], Token::Delim('!'))
            && matches!(&value_tokens[last], Token::Ident(word) if word.eq_ignore_ascii_case("important"))
        {
            important = true;
            value_tokens.truncate(before);
        }
    }

    let value = parse_value(&value_tokens);
    if name.is_empty() || value == Value::Keyword(String::new()) {
        return None;
    }
    Some(Declaration {
        name,
        value,
        important,
    })
}

/// Reconstructs source text from tokens, so sub-parsers can reuse the
/// string-based entry points.
fn tokens_to_source(tokens: &[Token]) -> String {
    let mut out = String::new();
    for token in tokens {
        match token {
            Token::Ident(name) => out.push_str(name),
            Token::Function(name) => {
                out.push_str(name);
                out.push('(');
            }
            Token::AtKeyword(name) => {
                out.push('@');
                out.push_str(name);
            }
            Token::Hash(name) => {
                out.push('#');
                out.push_str(name);
            }
            Token::Str(text) => {
                out.push('"');
                out.push_str(&text.replace('"', "\\\""));
                out.push('"');
            }
            Token::Url(url) => {
                out.push_str("url(\"");
                out.push_str(url);
                out.push_str("\")");
            }
            Token::Number(n) => out.push_str(&Value::Number(*n).to_css()),
            Token::Dimension(n, unit) => {
                out.push_str(&Value::Number(*n).to_css());
                out.push_str(unit);
            }
            Token::Percentage(p) => {
                out.push_str(&Value::Number(*p).to_css());
                out.push('%');
            }
            Token::Delim(ch) => out.push(*ch),
            Token::Whitespace => out.push(' '),
            Token::Colon => out.push(':'),
            Token::Semicolon => out.push(';'),
            Token::Comma => out.push(','),
            Token::ParenOpen => out.push('('),
            Token::ParenClose => out.push(')'),
            Token::BracketOpen => out.push('['),
            Token::BracketClose => out.push(']'),
            Token::BraceOpen => out.push('{'),
            Token::BraceClose => out.push('}'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::values::Unit;

    fn sheet(input: &str) -> Stylesheet {
        Stylesheet::parse(input, Origin::Author)
    }

    fn only_style(input: &str) -> StyleRule {
        let sheet = sheet(input);
        match sheet.rules.into_iter().next() {
            Some(Rule::Style(rule)) => rule,
            other => panic!("expected a style rule, got {other:?}"),
        }
    }

    #[test]
    fn parses_a_basic_rule() {
        let rule = only_style("p { color: red; margin: 0 auto }");
        assert_eq!(rule.selectors.len(), 1);
        assert_eq!(rule.declarations.len(), 2);
        assert_eq!(rule.declarations[0].name, "color");
        assert_eq!(
            rule.declarations[0].value.to_color(),
            Some(Color::rgb(255, 0, 0))
        );
        assert_eq!(rule.declarations[1].value.items().len(), 2);
    }

    #[test]
    fn multiple_selectors_and_rules() {
        let sheet = sheet("h1, h2 { font-weight: 700 } p { color: #333 }");
        assert_eq!(sheet.rules.len(), 2);
        assert_eq!(sheet.rule_count(), 2);
        let Rule::Style(first) = &sheet.rules[0] else {
            panic!("expected style rule");
        };
        assert_eq!(first.selectors.len(), 2);
    }

    #[test]
    fn important_is_detected_and_stripped() {
        let rule = only_style("p { color: red !important; width: 2px }");
        assert!(rule.declarations[0].important);
        assert_eq!(
            rule.declarations[0].value,
            Value::Keyword("red".into()),
            "the !important flag must not stay in the value"
        );
        assert!(!rule.declarations[1].important);
    }

    #[test]
    fn media_rules_nest_and_filter() {
        let sheet = sheet(
            "body { color: black } \
             @media (max-width: 600px) { body { color: blue } .m { display: none } }",
        );
        assert_eq!(sheet.rules.len(), 2);
        assert_eq!(sheet.rule_count(), 3);

        let wide = MediaContext::screen(1000.0, 800.0);
        assert_eq!(sheet.style_rules(&wide).len(), 1);
        let narrow = MediaContext::screen(400.0, 800.0);
        assert_eq!(sheet.style_rules(&narrow).len(), 3);
    }

    #[test]
    fn imports_are_collected() {
        let sheet = sheet("@import url(base.css); @import \"dark.css\" (prefers-color-scheme: dark); p{color:red}");
        let light = MediaContext::default();
        assert_eq!(sheet.imports(&light), vec!["base.css"]);
        let dark = MediaContext::default().with_dark(true);
        assert_eq!(sheet.imports(&dark), vec!["base.css", "dark.css"]);
    }

    #[test]
    fn supports_blocks_are_applied() {
        let sheet = sheet("@supports (display: grid) { .g { display: grid } }");
        assert_eq!(sheet.style_rules(&MediaContext::default()).len(), 1);
    }

    #[test]
    fn font_face_and_keyframes_are_kept() {
        let sheet =
            sheet("@font-face { font-family: X; src: url(x.woff) } @keyframes spin { from { } }");
        assert!(matches!(sheet.rules[0], Rule::FontFace(_)));
        assert!(matches!(sheet.rules[1], Rule::Keyframes { .. }));
    }

    #[test]
    fn malformed_input_recovers() {
        // Invalid selector, stray brace, and a rule truncated mid-declaration.
        let sheet = sheet("!!! { color: red } } p { color: blue } div { color:");
        let rules = sheet.style_rules(&MediaContext::default());
        assert_eq!(rules.len(), 2, "the invalid selector's rule is dropped");
        assert_eq!(rules[0].declarations[0].name, "color");
        assert_eq!(
            rules[0].declarations[0].value.to_color(),
            Some(Color::rgb(0, 0, 255))
        );
        assert!(
            rules[1].declarations.is_empty(),
            "the truncated declaration is dropped but its selector survives"
        );
    }

    #[test]
    fn declarations_without_colon_are_dropped() {
        let rule = only_style("p { color red; width: 3px }");
        assert_eq!(rule.declarations.len(), 1);
        assert_eq!(rule.declarations[0].name, "width");
    }

    #[test]
    fn semicolons_inside_functions_do_not_split() {
        let rule = only_style("p { background: url(a;b.png); color: red }");
        assert_eq!(rule.declarations.len(), 2);
        assert_eq!(rule.declarations[0].value.as_url(), Some("a;b.png"));
    }

    #[test]
    fn inline_style_parsing() {
        let declarations = parse_inline_style("color:#fff;padding:4px 8px");
        assert_eq!(declarations.len(), 2);
        assert_eq!(declarations[0].value, Value::Color(Color::WHITE));
        assert_eq!(
            declarations[1].value.items()[1],
            Value::Dimension(8.0, Unit::Px)
        );
    }

    #[test]
    fn comments_between_rules() {
        let sheet = sheet("/* a */ p { /* b */ color: red /* c */ }");
        assert_eq!(sheet.rule_count(), 1);
    }

    #[test]
    fn empty_stylesheet() {
        let sheet = sheet("");
        assert!(sheet.rules.is_empty());
        assert!(sheet.style_rules(&MediaContext::default()).is_empty());
    }

    #[test]
    fn nested_media_queries() {
        let sheet = sheet("@media screen { @media (min-width: 500px) { p { color: red } } }");
        assert_eq!(sheet.rule_count(), 1);
        assert_eq!(
            sheet.style_rules(&MediaContext::screen(600.0, 800.0)).len(),
            1
        );
        assert_eq!(
            sheet.style_rules(&MediaContext::screen(400.0, 800.0)).len(),
            0
        );
    }
}
