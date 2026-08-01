//! The JavaScript tokenizer.
//!
//! Produces tokens with enough context for the parser to do automatic semicolon
//! insertion: every token records whether a line terminator preceded it.
//!
//! Template literals are lexed whole — the quasis and the raw source of each
//! `${…}` substitution — and the parser re-parses the substitutions. That keeps
//! the lexer free of the mode switching a streaming approach would need.

use std::fmt;

/// The keywords the parser recognises. Anything else is an identifier.
pub const KEYWORDS: &[&str] = &[
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "in",
    "instanceof",
    "let",
    "new",
    "null",
    "of",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "undefined",
    "var",
    "void",
    "while",
    "yield",
];

/// Multi-character punctuators, longest first so greedy matching is correct.
const PUNCTUATORS: &[&str] = &[
    ">>>=", "...", "===", "!==", "**=", "<<=", ">>=", ">>>", "&&=", "||=", "??=", "=>", "==", "!=",
    "<=", ">=", "&&", "||", "??", "?.", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=",
    "**", "<<", ">>", "{", "}", "(", ")", "[", "]", ";", ",", "<", ">", "+", "-", "*", "/", "%",
    "&", "|", "^", "!", "~", "?", ":", "=", ".",
];

/// One piece of a template literal.
#[derive(Clone, Debug, PartialEq)]
pub enum TemplatePiece {
    /// Literal text.
    Text(String),
    /// The raw source of a `${…}` substitution, parsed later.
    Expr(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Number(f64),
    Str(String),
    Template(Vec<TemplatePiece>),
    Ident(String),
    /// One of [`KEYWORDS`].
    Keyword(&'static str),
    /// One of [`PUNCTUATORS`].
    Punct(&'static str),
    Eof,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    /// 1-based line, for error messages.
    pub line: u32,
    /// Was there a line break between this token and the one before it?
    ///
    /// This is what makes automatic semicolon insertion possible.
    pub newline_before: bool,
}

impl Token {
    pub fn is_punct(&self, text: &str) -> bool {
        matches!(&self.kind, TokenKind::Punct(p) if *p == text)
    }

    pub fn is_keyword(&self, text: &str) -> bool {
        matches!(&self.kind, TokenKind::Keyword(k) if *k == text)
    }

    pub fn is_eof(&self) -> bool {
        self.kind == TokenKind::Eof
    }

    /// The identifier-like text of this token, treating keywords as names.
    ///
    /// Property positions accept reserved words: `obj.new`, `{ class: 1 }`.
    pub fn name(&self) -> Option<&str> {
        match &self.kind {
            TokenKind::Ident(name) => Some(name),
            TokenKind::Keyword(name) => Some(name),
            _ => None,
        }
    }
}

/// A lexing or parsing failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxError {
    pub message: String,
    pub line: u32,
}

impl fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (line {})", self.message, self.line)
    }
}

impl std::error::Error for SyntaxError {}

impl SyntaxError {
    pub fn new(message: impl Into<String>, line: u32) -> Self {
        SyntaxError {
            message: message.into(),
            line,
        }
    }
}

pub struct Lexer<'a> {
    source: &'a str,
    pos: usize,
    line: u32,
    newline_pending: bool,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Lexer {
            source,
            pos: 0,
            line: 1,
            newline_pending: false,
        }
    }

    /// Tokenizes the whole input.
    pub fn tokenize(mut self) -> Result<Vec<Token>, SyntaxError> {
        let mut tokens = Vec::new();
        loop {
            let token = self.next_token()?;
            let done = token.is_eof();
            tokens.push(token);
            if done {
                return Ok(tokens);
            }
        }
    }

    fn rest(&self) -> &'a str {
        &self.source[self.pos..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.rest().chars().nth(offset)
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += ch.len_utf8();
        if ch == '\n' {
            self.line += 1;
        }
        Some(ch)
    }

    /// Skips whitespace and comments, remembering whether a line ended.
    fn skip_trivia(&mut self) -> Result<(), SyntaxError> {
        loop {
            match self.peek() {
                Some('\n') | Some('\r') | Some('\u{2028}') | Some('\u{2029}') => {
                    self.newline_pending = true;
                    self.bump();
                }
                Some(ch) if ch.is_whitespace() => {
                    self.bump();
                }
                Some('/') if self.peek_at(1) == Some('/') => {
                    while let Some(ch) = self.peek() {
                        if ch == '\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                Some('/') if self.peek_at(1) == Some('*') => {
                    let start_line = self.line;
                    self.bump();
                    self.bump();
                    loop {
                        match self.peek() {
                            None => {
                                return Err(SyntaxError::new("unterminated comment", start_line))
                            }
                            Some('*') if self.peek_at(1) == Some('/') => {
                                self.bump();
                                self.bump();
                                break;
                            }
                            _ => {
                                self.bump();
                            }
                        }
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn make(&mut self, kind: TokenKind) -> Token {
        Token {
            kind,
            line: self.line,
            newline_before: std::mem::take(&mut self.newline_pending),
        }
    }

    pub fn next_token(&mut self) -> Result<Token, SyntaxError> {
        self.skip_trivia()?;
        let Some(ch) = self.peek() else {
            return Ok(self.make(TokenKind::Eof));
        };

        if ch.is_ascii_digit() || (ch == '.' && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()))
        {
            return self.number();
        }
        if ch == '"' || ch == '\'' {
            return self.string();
        }
        if ch == '`' {
            return self.template();
        }
        // A private class field, `#count`, is lexed as a single identifier so
        // the rest of the engine can treat it as an ordinary — if unusually
        // named — property. Enumeration skips these names, which is what makes
        // them private in practice.
        if is_ident_start(ch) || (ch == '#' && self.peek_at(1).is_some_and(is_ident_start)) {
            return Ok(self.identifier());
        }
        for punctuator in PUNCTUATORS {
            if self.rest().starts_with(punctuator) {
                // `?.` before a digit is a conditional, not optional chaining:
                // `a ? .5 : 1`.
                if *punctuator == "?." && self.peek_at(2).is_some_and(|c| c.is_ascii_digit()) {
                    continue;
                }
                self.pos += punctuator.len();
                return Ok(self.make(TokenKind::Punct(punctuator)));
            }
        }

        let line = self.line;
        Err(SyntaxError::new(
            format!("unexpected character `{ch}`"),
            line,
        ))
    }

    fn identifier(&mut self) -> Token {
        let start = self.pos;
        if self.peek() == Some('#') {
            self.bump();
        }
        while self.peek().is_some_and(is_ident_part) {
            self.bump();
        }
        let text = &self.source[start..self.pos];
        match KEYWORDS.iter().find(|keyword| **keyword == text) {
            Some(keyword) => self.make(TokenKind::Keyword(keyword)),
            None => {
                let owned = text.to_string();
                self.make(TokenKind::Ident(owned))
            }
        }
    }

    fn number(&mut self) -> Result<Token, SyntaxError> {
        let start = self.pos;
        let line = self.line;

        // Radix prefixes.
        if self.peek() == Some('0') {
            if let Some(marker) = self.peek_at(1) {
                let radix = match marker.to_ascii_lowercase() {
                    'x' => Some(16),
                    'b' => Some(2),
                    'o' => Some(8),
                    _ => None,
                };
                if let Some(radix) = radix {
                    self.bump();
                    self.bump();
                    let digits_start = self.pos;
                    while self.peek().is_some_and(|c| c.is_digit(radix) || c == '_') {
                        self.bump();
                    }
                    let digits = self.source[digits_start..self.pos].replace('_', "");
                    let value = i64::from_str_radix(&digits, radix).map_err(|_| {
                        SyntaxError::new(format!("invalid number `{digits}`"), line)
                    })?;
                    return Ok(self.make(TokenKind::Number(value as f64)));
                }
            }
        }

        while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '_') {
            self.bump();
        }
        if self.peek() == Some('.') {
            self.bump();
            while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '_') {
                self.bump();
            }
        }
        if matches!(self.peek(), Some('e') | Some('E')) {
            let mut lookahead = 1;
            if matches!(self.peek_at(1), Some('+') | Some('-')) {
                lookahead = 2;
            }
            if self.peek_at(lookahead).is_some_and(|c| c.is_ascii_digit()) {
                for _ in 0..lookahead {
                    self.bump();
                }
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.bump();
                }
            }
        }

        let text = self.source[start..self.pos].replace('_', "");
        let value: f64 = text
            .parse()
            .map_err(|_| SyntaxError::new(format!("invalid number `{text}`"), line))?;
        Ok(self.make(TokenKind::Number(value)))
    }

    fn string(&mut self) -> Result<Token, SyntaxError> {
        let line = self.line;
        let quote = self.bump().expect("a quote");
        let mut out = String::new();
        loop {
            match self.bump() {
                None => return Err(SyntaxError::new("unterminated string", line)),
                Some(ch) if ch == quote => break,
                Some('\n') => return Err(SyntaxError::new("unterminated string", line)),
                Some('\\') => self.escape(&mut out)?,
                Some(ch) => out.push(ch),
            }
        }
        Ok(self.make(TokenKind::Str(out)))
    }

    /// Handles the character after a backslash.
    fn escape(&mut self, out: &mut String) -> Result<(), SyntaxError> {
        let line = self.line;
        match self.bump() {
            None => return Err(SyntaxError::new("unterminated escape", line)),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('b') => out.push('\u{8}'),
            Some('f') => out.push('\u{c}'),
            Some('v') => out.push('\u{b}'),
            Some('0') => out.push('\0'),
            // A line continuation contributes nothing.
            Some('\n') => {}
            Some('x') => {
                let mut digits = String::new();
                for _ in 0..2 {
                    digits.extend(self.bump());
                }
                let code = u32::from_str_radix(&digits, 16)
                    .map_err(|_| SyntaxError::new("invalid \\x escape", line))?;
                out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
            }
            Some('u') => {
                let mut digits = String::new();
                if self.peek() == Some('{') {
                    self.bump();
                    while let Some(ch) = self.peek() {
                        if ch == '}' {
                            self.bump();
                            break;
                        }
                        digits.push(ch);
                        self.bump();
                    }
                } else {
                    for _ in 0..4 {
                        digits.extend(self.bump());
                    }
                }
                let code = u32::from_str_radix(&digits, 16)
                    .map_err(|_| SyntaxError::new("invalid \\u escape", line))?;
                out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
            }
            Some(other) => out.push(other),
        }
        Ok(())
    }

    fn template(&mut self) -> Result<Token, SyntaxError> {
        let line = self.line;
        self.bump(); // the opening backtick
        let mut pieces = Vec::new();
        let mut text = String::new();

        loop {
            match self.peek() {
                None => return Err(SyntaxError::new("unterminated template literal", line)),
                Some('`') => {
                    self.bump();
                    break;
                }
                Some('\\') => {
                    self.bump();
                    self.escape(&mut text)?;
                }
                Some('$') if self.peek_at(1) == Some('{') => {
                    if !text.is_empty() {
                        pieces.push(TemplatePiece::Text(std::mem::take(&mut text)));
                    }
                    self.bump();
                    self.bump();
                    // Capture the substitution's source, tracking nesting so a
                    // nested object literal or template does not end it early.
                    let start = self.pos;
                    let mut depth = 1usize;
                    while let Some(ch) = self.peek() {
                        match ch {
                            '{' => depth += 1,
                            '}' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            '\'' | '"' => {
                                // Skip over strings so braces inside them do not count.
                                let quote = ch;
                                self.bump();
                                while let Some(inner) = self.peek() {
                                    self.bump();
                                    if inner == '\\' {
                                        self.bump();
                                    } else if inner == quote {
                                        break;
                                    }
                                }
                                continue;
                            }
                            _ => {}
                        }
                        self.bump();
                    }
                    if self.peek() != Some('}') {
                        return Err(SyntaxError::new("unterminated template substitution", line));
                    }
                    let expr = self.source[start..self.pos].to_string();
                    self.bump(); // the closing brace
                    pieces.push(TemplatePiece::Expr(expr));
                }
                Some(_) => {
                    let ch = self.bump().expect("a character");
                    text.push(ch);
                }
            }
        }
        if !text.is_empty() {
            pieces.push(TemplatePiece::Text(text));
        }
        Ok(self.make(TokenKind::Template(pieces)))
    }
}

fn is_ident_start(ch: char) -> bool {
    ch.is_alphabetic() || ch == '_' || ch == '$' || !ch.is_ascii()
}

fn is_ident_part(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '$' || !ch.is_ascii()
}

/// Tokenizes `source`.
pub fn tokenize(source: &str) -> Result<Vec<Token>, SyntaxError> {
    Lexer::new(source).tokenize()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<TokenKind> {
        tokenize(source)
            .expect("lexes")
            .into_iter()
            .map(|token| token.kind)
            .filter(|kind| *kind != TokenKind::Eof)
            .collect()
    }

    #[test]
    fn numbers_in_every_form() {
        assert_eq!(
            kinds("1 2.5 .5 1e3 1e-2 0xff 0b101 0o17 1_000"),
            vec![
                TokenKind::Number(1.0),
                TokenKind::Number(2.5),
                TokenKind::Number(0.5),
                TokenKind::Number(1000.0),
                TokenKind::Number(0.01),
                TokenKind::Number(255.0),
                TokenKind::Number(5.0),
                TokenKind::Number(15.0),
                TokenKind::Number(1000.0),
            ]
        );
    }

    #[test]
    fn strings_and_escapes() {
        assert_eq!(
            kinds(r#" 'a' "b" 'it\'s' "tab\there" "A" "\x42" "#),
            vec![
                TokenKind::Str("a".into()),
                TokenKind::Str("b".into()),
                TokenKind::Str("it's".into()),
                TokenKind::Str("tab\there".into()),
                TokenKind::Str("A".into()),
                TokenKind::Str("B".into()),
            ]
        );
    }

    #[test]
    fn unterminated_strings_are_errors() {
        assert!(tokenize("'oops").is_err());
        assert!(tokenize("'oops\nmore'").is_err());
    }

    #[test]
    fn identifiers_and_keywords() {
        assert_eq!(
            kinds("let x = className"),
            vec![
                TokenKind::Keyword("let"),
                TokenKind::Ident("x".into()),
                TokenKind::Punct("="),
                TokenKind::Ident("className".into()),
            ]
        );
        // `$` and `_` are identifier characters.
        assert_eq!(kinds("$_a1"), vec![TokenKind::Ident("$_a1".into())]);
    }

    #[test]
    fn punctuators_are_matched_greedily() {
        assert_eq!(
            kinds("a >>>= b === c ?? d ?. e => f"),
            vec![
                TokenKind::Ident("a".into()),
                TokenKind::Punct(">>>="),
                TokenKind::Ident("b".into()),
                TokenKind::Punct("==="),
                TokenKind::Ident("c".into()),
                TokenKind::Punct("??"),
                TokenKind::Ident("d".into()),
                TokenKind::Punct("?."),
                TokenKind::Ident("e".into()),
                TokenKind::Punct("=>"),
                TokenKind::Ident("f".into()),
            ]
        );
    }

    #[test]
    fn a_conditional_before_a_decimal_is_not_optional_chaining() {
        assert_eq!(
            kinds("a?.5:1"),
            vec![
                TokenKind::Ident("a".into()),
                TokenKind::Punct("?"),
                TokenKind::Number(0.5),
                TokenKind::Punct(":"),
                TokenKind::Number(1.0),
            ]
        );
    }

    #[test]
    fn comments_are_skipped() {
        assert_eq!(
            kinds("a // trailing\n/* block\nspanning */ b"),
            vec![TokenKind::Ident("a".into()), TokenKind::Ident("b".into())]
        );
        assert!(tokenize("/* never closed").is_err());
    }

    #[test]
    fn newlines_are_recorded_for_semicolon_insertion() {
        let tokens = tokenize("a\nb c").expect("lexes");
        assert!(!tokens[0].newline_before);
        assert!(tokens[1].newline_before, "b follows a line break");
        assert!(!tokens[2].newline_before, "c is on the same line as b");
    }

    #[test]
    fn line_numbers_are_tracked() {
        let tokens = tokenize("a\n\nb").expect("lexes");
        assert_eq!(tokens[0].line, 1);
        assert_eq!(tokens[1].line, 3);
    }

    #[test]
    fn template_literals_split_into_pieces() {
        let kinds = kinds("`hi ${name}!`");
        assert_eq!(
            kinds,
            vec![TokenKind::Template(vec![
                TemplatePiece::Text("hi ".into()),
                TemplatePiece::Expr("name".into()),
                TemplatePiece::Text("!".into()),
            ])]
        );
    }

    #[test]
    fn template_substitutions_may_nest_braces() {
        let kinds = kinds("`${ {a: 1}.a }`");
        assert_eq!(
            kinds,
            vec![TokenKind::Template(vec![TemplatePiece::Expr(
                " {a: 1}.a ".into()
            )])]
        );
    }

    #[test]
    fn template_substitutions_ignore_braces_in_strings() {
        let kinds = kinds("`${ f('}') }`");
        assert_eq!(
            kinds,
            vec![TokenKind::Template(vec![TemplatePiece::Expr(
                " f('}') ".into()
            )])]
        );
    }

    #[test]
    fn an_unterminated_template_is_an_error() {
        assert!(tokenize("`unclosed").is_err());
        assert!(tokenize("`${expr").is_err());
    }

    #[test]
    fn an_empty_program_is_just_eof() {
        let tokens = tokenize("   \n  ").expect("lexes");
        assert_eq!(tokens.len(), 1);
        assert!(tokens[0].is_eof());
    }

    #[test]
    fn unknown_characters_are_reported_with_a_line() {
        let error = tokenize("a\n@").expect_err("should fail");
        assert_eq!(error.line, 2);
        assert!(error.message.contains('@'));
    }
}
