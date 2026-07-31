//! Character reference decoding.
//!
//! Covers the numeric forms plus the named references that appear in practice.
//! Unknown references are passed through verbatim, which is what browsers do.

/// The named references we recognise, sorted so lookup can binary-search.
const NAMED: &[(&str, &str)] = &[
    ("AMP", "&"),
    ("Aacute", "Á"),
    ("Agrave", "À"),
    ("Auml", "Ä"),
    ("COPY", "©"),
    ("Ccedil", "Ç"),
    ("Dagger", "‡"),
    ("Eacute", "É"),
    ("Egrave", "È"),
    ("GT", ">"),
    ("LT", "<"),
    ("Ouml", "Ö"),
    ("QUOT", "\""),
    ("REG", "®"),
    ("Uuml", "Ü"),
    ("aacute", "á"),
    ("acirc", "â"),
    ("acute", "´"),
    ("aelig", "æ"),
    ("agrave", "à"),
    ("amp", "&"),
    ("apos", "'"),
    ("aring", "å"),
    ("asymp", "≈"),
    ("atilde", "ã"),
    ("auml", "ä"),
    ("bdquo", "„"),
    ("bull", "•"),
    ("ccedil", "ç"),
    ("cent", "¢"),
    ("check", "✓"),
    ("circ", "ˆ"),
    ("clubs", "♣"),
    ("copy", "©"),
    ("curren", "¤"),
    ("dagger", "†"),
    ("darr", "↓"),
    ("deg", "°"),
    ("diams", "♦"),
    ("divide", "÷"),
    ("eacute", "é"),
    ("ecirc", "ê"),
    ("egrave", "è"),
    ("emsp", "\u{2003}"),
    ("ensp", "\u{2002}"),
    ("euml", "ë"),
    ("euro", "€"),
    ("frac12", "½"),
    ("frac14", "¼"),
    ("frac34", "¾"),
    ("gt", ">"),
    ("harr", "↔"),
    ("hearts", "♥"),
    ("hellip", "…"),
    ("iacute", "í"),
    ("icirc", "î"),
    ("iexcl", "¡"),
    ("igrave", "ì"),
    ("infin", "∞"),
    ("iquest", "¿"),
    ("iuml", "ï"),
    ("laquo", "«"),
    ("larr", "←"),
    ("ldquo", "“"),
    ("le", "≤"),
    ("lsaquo", "‹"),
    ("lsquo", "‘"),
    ("lt", "<"),
    ("macr", "¯"),
    ("mdash", "—"),
    ("micro", "µ"),
    ("middot", "·"),
    ("minus", "−"),
    ("nbsp", "\u{00a0}"),
    ("ndash", "–"),
    ("ne", "≠"),
    ("not", "¬"),
    ("ntilde", "ñ"),
    ("oacute", "ó"),
    ("ocirc", "ô"),
    ("oelig", "œ"),
    ("ograve", "ò"),
    ("ordf", "ª"),
    ("ordm", "º"),
    ("oslash", "ø"),
    ("otilde", "õ"),
    ("ouml", "ö"),
    ("para", "¶"),
    ("permil", "‰"),
    ("plusmn", "±"),
    ("pound", "£"),
    ("prime", "′"),
    ("quot", "\""),
    ("raquo", "»"),
    ("rarr", "→"),
    ("rdquo", "”"),
    ("reg", "®"),
    ("rsaquo", "›"),
    ("rsquo", "’"),
    ("sbquo", "‚"),
    ("sect", "§"),
    ("shy", "\u{00ad}"),
    ("spades", "♠"),
    ("sup1", "¹"),
    ("sup2", "²"),
    ("sup3", "³"),
    ("szlig", "ß"),
    ("thinsp", "\u{2009}"),
    ("thorn", "þ"),
    ("tilde", "˜"),
    ("times", "×"),
    ("trade", "™"),
    ("uacute", "ú"),
    ("uarr", "↑"),
    ("ucirc", "û"),
    ("ugrave", "ù"),
    ("uml", "¨"),
    ("uuml", "ü"),
    ("yacute", "ý"),
    ("yen", "¥"),
    ("yuml", "ÿ"),
];

/// Windows-1252 replacements the HTML spec mandates for C1 code points.
fn numeric_replacement(code: u32) -> Option<char> {
    let replacement = match code {
        0x00 => '\u{fffd}',
        0x80 => '\u{20ac}',
        0x82 => '\u{201a}',
        0x83 => '\u{0192}',
        0x84 => '\u{201e}',
        0x85 => '\u{2026}',
        0x86 => '\u{2020}',
        0x87 => '\u{2021}',
        0x88 => '\u{02c6}',
        0x89 => '\u{2030}',
        0x8a => '\u{0160}',
        0x8b => '\u{2039}',
        0x8c => '\u{0152}',
        0x8e => '\u{017d}',
        0x91 => '\u{2018}',
        0x92 => '\u{2019}',
        0x93 => '\u{201c}',
        0x94 => '\u{201d}',
        0x95 => '\u{2022}',
        0x96 => '\u{2013}',
        0x97 => '\u{2014}',
        0x98 => '\u{02dc}',
        0x99 => '\u{2122}',
        0x9a => '\u{0161}',
        0x9b => '\u{203a}',
        0x9c => '\u{0153}',
        0x9e => '\u{017e}',
        0x9f => '\u{0178}',
        _ => return None,
    };
    Some(replacement)
}

fn lookup_named(name: &str) -> Option<&'static str> {
    NAMED
        .binary_search_by(|(candidate, _)| (*candidate).cmp(name))
        .ok()
        .map(|index| NAMED[index].1)
}

/// Decodes a character reference starting *after* the `&`.
///
/// Returns the replacement text and how many bytes of `rest` were consumed, or
/// `None` when `rest` does not start a recognised reference.
pub fn decode(rest: &str) -> Option<(String, usize)> {
    let bytes = rest.as_bytes();
    if bytes.first() == Some(&b'#') {
        let (digits, radix, prefix) = match bytes.get(1) {
            Some(b'x') | Some(b'X') => (&rest[2..], 16, 2),
            _ => (&rest[1..], 10, 1),
        };
        let end = digits
            .find(|c: char| !c.is_digit(radix))
            .unwrap_or(digits.len());
        if end == 0 {
            return None;
        }
        let code = u32::from_str_radix(&digits[..end], radix).unwrap_or(0xfffd);
        let ch = numeric_replacement(code)
            .or_else(|| char::from_u32(code))
            .unwrap_or('\u{fffd}');
        // A trailing semicolon is part of the reference when present.
        let semi = usize::from(digits.as_bytes().get(end) == Some(&b';'));
        return Some((ch.to_string(), prefix + end + semi));
    }

    let end = rest
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    // Longest match wins: `&notit;` is `¬` + "it;" in the spec, and shrinking the
    // candidate reproduces that behaviour for the references we know.
    for len in (1..=end).rev() {
        if let Some(text) = lookup_named(&rest[..len]) {
            let semi = usize::from(rest.as_bytes().get(len) == Some(&b';'));
            return Some((text.to_string(), len + semi));
        }
    }
    None
}

/// Replaces every character reference in `input`.
pub fn decode_all(input: &str) -> String {
    if !input.contains('&') {
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(index) = rest.find('&') {
        out.push_str(&rest[..index]);
        let after = &rest[index + 1..];
        match decode(after) {
            Some((text, consumed)) => {
                out.push_str(&text);
                rest = &after[consumed..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_references() {
        assert_eq!(decode_all("a &amp; b"), "a & b");
        assert_eq!(decode_all("&lt;div&gt;"), "<div>");
        assert_eq!(decode_all("caf&eacute;"), "café");
        assert_eq!(decode_all("\u{00a0}"), "\u{00a0}");
        assert_eq!(decode_all("x&nbsp;y"), "x\u{00a0}y");
    }

    #[test]
    fn numeric_references() {
        assert_eq!(decode_all("&#65;&#x42;"), "AB");
        assert_eq!(decode_all("&#x1F600;").chars().count(), 1);
        // C1 range is remapped per spec.
        assert_eq!(decode_all("&#151;"), "—");
    }

    #[test]
    fn unknown_references_are_literal() {
        assert_eq!(decode_all("&bogus;"), "&bogus;");
        assert_eq!(decode_all("100% & more"), "100% & more");
        assert_eq!(decode_all("&"), "&");
    }

    #[test]
    fn named_table_is_sorted() {
        assert!(NAMED.windows(2).all(|w| w[0].0 < w[1].0));
    }
}
