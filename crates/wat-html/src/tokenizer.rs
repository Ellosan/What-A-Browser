//! HTML tokenizer.
//!
//! A pragmatic subset of the HTML tokenization state machine: it handles tags,
//! attributes, comments, doctypes, character references and the raw-text
//! elements (`script`, `style`, `textarea`, `title`). Malformed input never
//! aborts tokenization — the tokenizer recovers the way browsers do.

use crate::entities;
use wat_dom::Attribute;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    Doctype(String),
    StartTag {
        name: String,
        attributes: Vec<Attribute>,
        self_closing: bool,
    },
    EndTag {
        name: String,
    },
    Text(String),
    Comment(String),
}

/// Elements whose content is not parsed as markup.
const RAWTEXT: &[&str] = &["script", "style", "xmp", "iframe", "noembed", "noframes"];
/// Elements whose content is text, but character references still apply.
const RCDATA: &[&str] = &["title", "textarea"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Data,
    /// Inside a raw-text element; the payload is the index into the element
    /// name table so we know which end tag closes it.
    Raw {
        decode_entities: bool,
    },
}

pub struct Tokenizer<'a> {
    input: &'a str,
    /// Byte offset into `input`.
    pos: usize,
    mode: Mode,
    /// Name of the raw-text element currently open, if any.
    raw_tag: String,
}

impl<'a> Tokenizer<'a> {
    pub fn new(input: &'a str) -> Self {
        Tokenizer {
            input,
            pos: 0,
            mode: Mode::Data,
            raw_tag: String::new(),
        }
    }

    /// Tokenizes the whole input.
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

    fn eof(&self) -> bool {
        self.pos >= self.input.len()
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn starts_with(&self, prefix: &str) -> bool {
        self.rest().starts_with(prefix)
    }

    fn starts_with_ignore_case(&self, prefix: &str) -> bool {
        // Compared byte by byte: slicing at `prefix.len()` would panic when the
        // input has a multi-byte character straddling that boundary, which is
        // exactly what an em dash right after a tag does.
        let rest = self.rest().as_bytes();
        rest.len() >= prefix.len() && rest[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
    }

    fn advance(&mut self, bytes: usize) {
        self.pos = (self.pos + bytes).min(self.input.len());
    }

    fn consume_char(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_ascii_whitespace() {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
    }

    pub fn next_token(&mut self) -> Option<Token> {
        if self.eof() {
            return None;
        }
        match self.mode {
            Mode::Data => self.next_data_token(),
            Mode::Raw { decode_entities } => Some(self.raw_text(decode_entities)),
        }
    }

    fn next_data_token(&mut self) -> Option<Token> {
        if self.starts_with("<!--") {
            return Some(self.comment());
        }
        if self.starts_with_ignore_case("<!doctype") {
            return Some(self.doctype());
        }
        if self.starts_with("<![") || self.starts_with("<?") {
            // Bogus comment: consume up to the next `>`.
            let end = self
                .rest()
                .find('>')
                .map(|i| i + 1)
                .unwrap_or(self.rest().len());
            let text = self.rest()[..end].to_string();
            self.advance(end);
            return Some(Token::Comment(text));
        }
        if self.starts_with("</") {
            return Some(self.end_tag());
        }
        if self.peek() == Some('<') && self.tag_name_follows() {
            return Some(self.start_tag());
        }
        Some(self.text())
    }

    /// True when the `<` at the cursor actually opens a tag. A stray `<` (as in
    /// `a < b`) is treated as text instead.
    fn tag_name_follows(&self) -> bool {
        self.rest()[1..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
    }

    fn text(&mut self) -> Token {
        let start = self.pos;
        // Always consume at least one character so we cannot loop forever on a
        // lone `<`.
        self.consume_char();
        while let Some(ch) = self.peek() {
            if ch == '<'
                && (self.starts_with("</")
                    || self.starts_with("<!")
                    || self.starts_with("<?")
                    || self.tag_name_follows())
            {
                break;
            }
            self.consume_char();
        }
        Token::Text(entities::decode_all(&self.input[start..self.pos]))
    }

    fn comment(&mut self) -> Token {
        self.advance(4); // `<!--`
        let rest = self.rest();
        match rest.find("-->") {
            Some(end) => {
                let text = rest[..end].to_string();
                self.advance(end + 3);
                Token::Comment(text)
            }
            None => {
                let text = rest.to_string();
                self.pos = self.input.len();
                Token::Comment(text)
            }
        }
    }

    fn doctype(&mut self) -> Token {
        self.advance("<!doctype".len());
        self.skip_whitespace();
        let rest = self.rest();
        let end = rest.find('>').unwrap_or(rest.len());
        let body = rest[..end].trim().to_string();
        self.advance(end + usize::from(end < rest.len()));
        let name = body
            .split_ascii_whitespace()
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        Token::Doctype(name)
    }

    fn end_tag(&mut self) -> Token {
        self.advance(2); // `</`
        let name = self.tag_name();
        // Skip any junk attributes on the end tag.
        let rest = self.rest();
        let end = rest.find('>').map(|i| i + 1).unwrap_or(rest.len());
        self.advance(end);
        Token::EndTag { name }
    }

    fn start_tag(&mut self) -> Token {
        self.advance(1); // `<`
        let name = self.tag_name();
        let mut attributes: Vec<Attribute> = Vec::new();
        let mut self_closing = false;

        loop {
            self.skip_whitespace();
            match self.peek() {
                None => break,
                Some('>') => {
                    self.advance(1);
                    break;
                }
                Some('/') => {
                    self.advance(1);
                    if self.peek() == Some('>') {
                        self.advance(1);
                        self_closing = true;
                        break;
                    }
                }
                Some(_) => {
                    if let Some(attr) = self.attribute() {
                        // First occurrence of a duplicate attribute wins.
                        if !attributes.iter().any(|a| a.name == attr.name) {
                            attributes.push(attr);
                        }
                    } else {
                        // Could not make progress; drop a character to stay live.
                        self.consume_char();
                    }
                }
            }
        }

        if RAWTEXT.contains(&name.as_str()) && !self_closing {
            self.mode = Mode::Raw {
                decode_entities: false,
            };
            self.raw_tag = name.clone();
        } else if RCDATA.contains(&name.as_str()) && !self_closing {
            self.mode = Mode::Raw {
                decode_entities: true,
            };
            self.raw_tag = name.clone();
        }

        Token::StartTag {
            name,
            attributes,
            self_closing,
        }
    }

    fn tag_name(&mut self) -> String {
        let start = self.pos;
        while let Some(ch) = self.peek() {
            if ch.is_ascii_whitespace() || ch == '>' || ch == '/' {
                break;
            }
            self.consume_char();
        }
        self.input[start..self.pos].to_ascii_lowercase()
    }

    fn attribute(&mut self) -> Option<Attribute> {
        let start = self.pos;
        while let Some(ch) = self.peek() {
            if ch.is_ascii_whitespace() || ch == '=' || ch == '>' || ch == '/' {
                break;
            }
            self.consume_char();
        }
        if self.pos == start {
            return None;
        }
        let name = self.input[start..self.pos].to_ascii_lowercase();

        self.skip_whitespace();
        if self.peek() != Some('=') {
            return Some(Attribute {
                name,
                value: String::new(),
            });
        }
        self.advance(1); // `=`
        self.skip_whitespace();

        let value = match self.peek() {
            Some(quote @ ('"' | '\'')) => {
                self.advance(1);
                let rest = self.rest();
                let end = rest.find(quote).unwrap_or(rest.len());
                let raw = rest[..end].to_string();
                self.advance(end + usize::from(end < rest.len()));
                raw
            }
            _ => {
                let start = self.pos;
                while let Some(ch) = self.peek() {
                    if ch.is_ascii_whitespace() || ch == '>' {
                        break;
                    }
                    self.consume_char();
                }
                self.input[start..self.pos].to_string()
            }
        };

        Some(Attribute {
            name,
            value: entities::decode_all(&value),
        })
    }

    /// Consumes raw text up to the matching end tag and returns it as a text
    /// token. The end tag itself is left for the next call.
    fn raw_text(&mut self, decode_entities: bool) -> Token {
        let closing = format!("</{}", self.raw_tag);
        let rest = self.rest();
        let mut search = 0;
        let end = loop {
            match rest[search..].find('<') {
                Some(offset) => {
                    let at = search + offset;
                    if rest[at..].len() >= closing.len()
                        && rest[at..at + closing.len()].eq_ignore_ascii_case(&closing)
                    {
                        break at;
                    }
                    search = at + 1;
                }
                None => break rest.len(),
            }
        };

        let text = rest[..end].to_string();
        self.advance(end);
        self.mode = Mode::Data;
        self.raw_tag.clear();
        Token::Text(if decode_entities {
            entities::decode_all(&text)
        } else {
            text
        })
    }
}

/// Convenience wrapper.
pub fn tokenize(input: &str) -> Vec<Token> {
    Tokenizer::new(input).tokenize()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start(name: &str) -> Token {
        Token::StartTag {
            name: name.into(),
            attributes: vec![],
            self_closing: false,
        }
    }

    #[test]
    fn simple_elements() {
        let tokens = tokenize("<p>hi</p>");
        assert_eq!(
            tokens,
            vec![
                start("p"),
                Token::Text("hi".into()),
                Token::EndTag { name: "p".into() }
            ]
        );
    }

    #[test]
    fn attributes_in_all_quoting_styles() {
        let tokens = tokenize(r#"<a href="/x" title='y' data-n=3 hidden>"#);
        let Token::StartTag { attributes, .. } = &tokens[0] else {
            panic!("expected start tag, got {:?}", tokens[0]);
        };
        assert_eq!(attributes.len(), 4);
        assert_eq!(attributes[0].value, "/x");
        assert_eq!(attributes[1].value, "y");
        assert_eq!(attributes[2].value, "3");
        assert_eq!(attributes[3].value, "");
    }

    #[test]
    fn tag_and_attribute_names_are_lowercased() {
        let tokens = tokenize(r#"<DIV CLASS="A">"#);
        let Token::StartTag {
            name, attributes, ..
        } = &tokens[0]
        else {
            panic!("expected start tag");
        };
        assert_eq!(name, "div");
        assert_eq!(attributes[0].name, "class");
        assert_eq!(attributes[0].value, "A", "values keep their case");
    }

    #[test]
    fn self_closing_is_reported() {
        let tokens = tokenize("<br/>");
        assert!(matches!(
            &tokens[0],
            Token::StartTag {
                self_closing: true,
                ..
            }
        ));
    }

    #[test]
    fn a_multi_byte_character_after_a_tag_does_not_panic() {
        // `</code> — text` used to be sliced at a byte offset inside the em
        // dash while looking ahead for `<!doctype`.
        let tokens = tokenize("<p><code>a</code> — done</p>");
        let text: String = tokens
            .iter()
            .filter_map(|token| match token {
                Token::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "a — done");
    }

    #[test]
    fn script_content_is_raw() {
        let tokens = tokenize("<script>if (a < b && c > d) {}</script>");
        assert_eq!(tokens[1], Token::Text("if (a < b && c > d) {}".into()));
        assert_eq!(
            tokens[2],
            Token::EndTag {
                name: "script".into()
            }
        );
    }

    #[test]
    fn style_content_is_raw() {
        let tokens = tokenize("<style>a > b { color: red }</style>");
        assert_eq!(tokens[1], Token::Text("a > b { color: red }".into()));
    }

    #[test]
    fn title_decodes_entities() {
        let tokens = tokenize("<title>A &amp; B</title>");
        assert_eq!(tokens[1], Token::Text("A & B".into()));
    }

    #[test]
    fn comments_and_doctype() {
        let tokens = tokenize("<!DOCTYPE html><!-- note -->x");
        assert_eq!(tokens[0], Token::Doctype("html".into()));
        assert_eq!(tokens[1], Token::Comment(" note ".into()));
        assert_eq!(tokens[2], Token::Text("x".into()));
    }

    #[test]
    fn stray_less_than_is_text() {
        let tokens = tokenize("a < b");
        assert_eq!(tokens, vec![Token::Text("a < b".into())]);
    }

    #[test]
    fn unterminated_tag_does_not_hang() {
        let tokens = tokenize("<div class=\"x");
        assert_eq!(tokens.len(), 1);
    }

    #[test]
    fn unterminated_comment_is_consumed() {
        let tokens = tokenize("<!-- oops");
        assert_eq!(tokens, vec![Token::Comment(" oops".into())]);
    }
}
