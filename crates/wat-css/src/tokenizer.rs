//! CSS tokenizer.
//!
//! Produces the component tokens the parser needs. Comments are stripped, and
//! unterminated constructs are closed at end of input rather than erroring.

#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    Ident(String),
    /// `name(` — the opening paren is consumed.
    Function(String),
    /// `@media`, `@import`, …
    AtKeyword(String),
    /// `#fff`, `#main`
    Hash(String),
    Str(String),
    Url(String),
    Number(f32),
    /// A number with a unit, e.g. `12px`.
    Dimension(f32, String),
    Percentage(f32),
    /// Any single character that is not otherwise significant.
    Delim(char),
    Whitespace,
    Colon,
    Semicolon,
    Comma,
    ParenOpen,
    ParenClose,
    BracketOpen,
    BracketClose,
    BraceOpen,
    BraceClose,
}

impl Token {
    pub fn is_whitespace(&self) -> bool {
        matches!(self, Token::Whitespace)
    }
}

fn is_ident_start(ch: char) -> bool {
    ch.is_alphabetic() || ch == '_' || ch == '-' || !ch.is_ascii()
}

fn is_ident_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '-' || !ch.is_ascii()
}

pub struct Tokenizer<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Tokenizer<'a> {
    pub fn new(input: &'a str) -> Self {
        Tokenizer { input, pos: 0 }
    }

    pub fn tokenize(mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        while let Some(token) = self.next_token() {
            tokens.push(token);
        }
        tokens
    }

    fn rest(&self) -> &'a str {
        &self.input[self.pos..]
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
        Some(ch)
    }

    fn next_token(&mut self) -> Option<Token> {
        self.skip_comments();
        let ch = self.peek()?;

        if ch.is_whitespace() {
            while self.peek().is_some_and(char::is_whitespace) {
                self.bump();
            }
            // Whitespace immediately followed by a comment collapses into one run.
            self.skip_comments();
            while self.peek().is_some_and(char::is_whitespace) {
                self.bump();
            }
            return Some(Token::Whitespace);
        }

        let token = match ch {
            ':' => {
                self.bump();
                Token::Colon
            }
            ';' => {
                self.bump();
                Token::Semicolon
            }
            ',' => {
                self.bump();
                Token::Comma
            }
            '(' => {
                self.bump();
                Token::ParenOpen
            }
            ')' => {
                self.bump();
                Token::ParenClose
            }
            '[' => {
                self.bump();
                Token::BracketOpen
            }
            ']' => {
                self.bump();
                Token::BracketClose
            }
            '{' => {
                self.bump();
                Token::BraceOpen
            }
            '}' => {
                self.bump();
                Token::BraceClose
            }
            '"' | '\'' => self.string(),
            '#' => {
                self.bump();
                let name = self.consume_name();
                if name.is_empty() {
                    Token::Delim('#')
                } else {
                    Token::Hash(name)
                }
            }
            '@' => {
                self.bump();
                let name = self.consume_name();
                if name.is_empty() {
                    Token::Delim('@')
                } else {
                    Token::AtKeyword(name.to_ascii_lowercase())
                }
            }
            '+' | '.' | '-' if self.numeric_ahead() => self.numeric(),
            '0'..='9' => self.numeric(),
            _ if is_ident_start(ch) => {
                let name = self.consume_name();
                if self.peek() == Some('(') {
                    self.bump();
                    if name.eq_ignore_ascii_case("url") {
                        return Some(self.url_body());
                    }
                    Token::Function(name)
                } else {
                    Token::Ident(name)
                }
            }
            _ => {
                self.bump();
                Token::Delim(ch)
            }
        };
        Some(token)
    }

    fn skip_comments(&mut self) {
        while self.rest().starts_with("/*") {
            match self.rest()[2..].find("*/") {
                Some(end) => self.pos += 2 + end + 2,
                None => self.pos = self.input.len(),
            }
        }
    }

    /// Does a number start at the cursor (allowing a leading sign or dot)?
    fn numeric_ahead(&self) -> bool {
        let first = self.peek();
        match first {
            Some('+') | Some('-') => match self.peek_at(1) {
                Some(c) if c.is_ascii_digit() => true,
                Some('.') => self.peek_at(2).is_some_and(|c| c.is_ascii_digit()),
                _ => false,
            },
            Some('.') => self.peek_at(1).is_some_and(|c| c.is_ascii_digit()),
            Some(c) => c.is_ascii_digit(),
            None => false,
        }
    }

    fn consume_name(&mut self) -> String {
        let start = self.pos;
        while let Some(ch) = self.peek() {
            if is_ident_char(ch) {
                self.bump();
            } else if ch == '\\' {
                // Escaped character: keep the escaped glyph verbatim.
                self.bump();
                self.bump();
            } else {
                break;
            }
        }
        self.input[start..self.pos].replace('\\', "")
    }

    fn string(&mut self) -> Token {
        let quote = self.bump().unwrap();
        let mut out = String::new();
        while let Some(ch) = self.bump() {
            match ch {
                c if c == quote => break,
                '\\' => {
                    if let Some(next) = self.bump() {
                        if next != '\n' {
                            out.push(next);
                        }
                    }
                }
                '\n' => break, // unterminated string ends at newline
                c => out.push(c),
            }
        }
        Token::Str(out)
    }

    fn url_body(&mut self) -> Token {
        while self.peek().is_some_and(char::is_whitespace) {
            self.bump();
        }
        let value = match self.peek() {
            Some('"') | Some('\'') => {
                let Token::Str(s) = self.string() else {
                    unreachable!("string() always returns Token::Str")
                };
                s
            }
            _ => {
                let start = self.pos;
                while let Some(ch) = self.peek() {
                    if ch == ')' {
                        break;
                    }
                    self.bump();
                }
                self.input[start..self.pos].trim().to_string()
            }
        };
        while let Some(ch) = self.bump() {
            if ch == ')' {
                break;
            }
        }
        Token::Url(value)
    }

    fn numeric(&mut self) -> Token {
        let start = self.pos;
        if matches!(self.peek(), Some('+') | Some('-')) {
            self.bump();
        }
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.bump();
        }
        if self.peek() == Some('.') && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
            self.bump();
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.bump();
            }
        }
        // Scientific notation, e.g. `1e3`.
        if matches!(self.peek(), Some('e') | Some('E')) {
            let mut lookahead = 1;
            if matches!(self.peek_at(1), Some('+') | Some('-')) {
                lookahead = 2;
            }
            if self.peek_at(lookahead).is_some_and(|c| c.is_ascii_digit()) {
                for _ in 0..=lookahead {
                    self.bump();
                }
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.bump();
                }
            }
        }

        let number: f32 = self.input[start..self.pos].parse().unwrap_or(0.0);

        if self.peek() == Some('%') {
            self.bump();
            return Token::Percentage(number);
        }
        if self.peek().is_some_and(is_ident_start) {
            let unit = self.consume_name().to_ascii_lowercase();
            return Token::Dimension(number, unit);
        }
        Token::Number(number)
    }
}

/// Tokenizes a stylesheet or declaration list.
pub fn tokenize(input: &str) -> Vec<Token> {
    Tokenizer::new(input).tokenize()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_ws(input: &str) -> Vec<Token> {
        tokenize(input)
            .into_iter()
            .filter(|t| !t.is_whitespace())
            .collect()
    }

    #[test]
    fn simple_rule() {
        assert_eq!(
            no_ws("a { color: red; }"),
            vec![
                Token::Ident("a".into()),
                Token::BraceOpen,
                Token::Ident("color".into()),
                Token::Colon,
                Token::Ident("red".into()),
                Token::Semicolon,
                Token::BraceClose,
            ]
        );
    }

    #[test]
    fn dimensions_and_percentages() {
        assert_eq!(
            no_ws("12px -3.5em 50% 0 1e2"),
            vec![
                Token::Dimension(12.0, "px".into()),
                Token::Dimension(-3.5, "em".into()),
                Token::Percentage(50.0),
                Token::Number(0.0),
                Token::Number(100.0),
            ]
        );
    }

    #[test]
    fn comments_are_stripped() {
        assert_eq!(
            no_ws("a/* hi */b"),
            vec![Token::Ident("a".into()), Token::Ident("b".into())]
        );
        assert_eq!(no_ws("/* unterminated"), Vec::<Token>::new());
    }

    #[test]
    fn functions_and_urls() {
        assert_eq!(
            no_ws("rgb(1,2,3) url(a.png) url('b.png')"),
            vec![
                Token::Function("rgb".into()),
                Token::Number(1.0),
                Token::Comma,
                Token::Number(2.0),
                Token::Comma,
                Token::Number(3.0),
                Token::ParenClose,
                Token::Url("a.png".into()),
                Token::Url("b.png".into()),
            ]
        );
    }

    #[test]
    fn strings_with_escapes() {
        assert_eq!(no_ws(r#""a\"b""#), vec![Token::Str("a\"b".into())]);
        assert_eq!(
            no_ws("'unterminated"),
            vec![Token::Str("unterminated".into())]
        );
    }

    #[test]
    fn hashes_and_at_keywords() {
        assert_eq!(
            no_ws("#fff @MEDIA"),
            vec![Token::Hash("fff".into()), Token::AtKeyword("media".into())]
        );
    }

    #[test]
    fn whitespace_runs_collapse() {
        let tokens = tokenize("a  \n\t b");
        assert_eq!(
            tokens,
            vec![
                Token::Ident("a".into()),
                Token::Whitespace,
                Token::Ident("b".into())
            ]
        );
    }
}
