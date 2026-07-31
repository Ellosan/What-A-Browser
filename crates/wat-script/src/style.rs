//! Editing an inline `style` attribute as text.
//!
//! `el.style.color = 'red'` has to end up in the `style` attribute, because that
//! is where the cascade reads inline declarations from. Working on the text
//! rather than on parsed CSS values means a property the engine does not
//! understand still round-trips untouched.

/// Splits an inline style into `(property, value)` pairs.
///
/// Semicolons inside `url(…)` or a quoted string do not separate declarations,
/// so the split tracks nesting and quoting.
pub fn declarations(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for chunk in split_top_level(text, ';') {
        let Some((name, value)) = split_once_top_level(&chunk, ':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_string();
        if name.is_empty() || value.is_empty() {
            continue;
        }
        out.push((name, value));
    }
    out
}

/// Renders declarations back into an inline style.
pub fn serialise(declarations: &[(String, String)]) -> String {
    declarations
        .iter()
        .map(|(name, value)| format!("{name}: {value}"))
        .collect::<Vec<_>>()
        .join("; ")
}

/// The value of one property, if it is set.
pub fn property(text: &str, name: &str) -> Option<String> {
    declarations(text)
        .into_iter()
        .rev()
        .find(|(property, _)| property == name)
        .map(|(_, value)| value)
}

/// Sets a property, keeping the position of an existing declaration.
pub fn set_property(text: &str, name: &str, value: &str) -> String {
    let mut declarations = declarations(text);
    match declarations
        .iter_mut()
        .find(|(property, _)| property == name)
    {
        Some(existing) => existing.1 = value.to_string(),
        None => declarations.push((name.to_string(), value.to_string())),
    }
    serialise(&declarations)
}

pub fn remove_property(text: &str, name: &str) -> String {
    let mut declarations = declarations(text);
    declarations.retain(|(property, _)| property != name);
    serialise(&declarations)
}

/// `backgroundColor` to `background-color`.
///
/// A name that is already hyphenated, including a `--custom` property, is left
/// alone.
pub fn to_kebab_case(name: &str) -> String {
    if name.contains('-') {
        return name.to_ascii_lowercase();
    }
    let mut out = String::with_capacity(name.len() + 2);
    for ch in name.chars() {
        if ch.is_ascii_uppercase() {
            out.push('-');
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// `background-color` to `backgroundColor`.
pub fn to_camel_case(name: &str) -> String {
    // A custom property keeps its name exactly, because that is how it is read.
    if name.starts_with("--") {
        return name.to_string();
    }
    let mut out = String::with_capacity(name.len());
    let mut capitalise = false;
    for ch in name.chars() {
        if ch == '-' {
            capitalise = true;
            continue;
        }
        if capitalise {
            out.push(ch.to_ascii_uppercase());
            capitalise = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Splits on `separator`, ignoring ones inside brackets or quotes.
fn split_top_level(text: &str, separator: char) -> Vec<String> {
    let mut chunks = vec![String::new()];
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    for ch in text.chars() {
        if let Some(open) = quote {
            chunks.last_mut().unwrap().push(ch);
            if ch == open {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => {
                quote = Some(ch);
                chunks.last_mut().unwrap().push(ch);
            }
            '(' | '[' => {
                depth += 1;
                chunks.last_mut().unwrap().push(ch);
            }
            ')' | ']' => {
                depth = depth.saturating_sub(1);
                chunks.last_mut().unwrap().push(ch);
            }
            ch if ch == separator && depth == 0 => chunks.push(String::new()),
            ch => chunks.last_mut().unwrap().push(ch),
        }
    }
    chunks
}

/// Splits at the first top-level `separator`.
fn split_once_top_level(text: &str, separator: char) -> Option<(String, String)> {
    let parts = split_top_level(text, separator);
    if parts.len() < 2 {
        return None;
    }
    Some((parts[0].clone(), parts[1..].join(&separator.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declarations_are_split_and_normalised() {
        let parsed = declarations("Color: red; background : blue ;");
        assert_eq!(
            parsed,
            vec![
                ("color".to_string(), "red".to_string()),
                ("background".to_string(), "blue".to_string())
            ]
        );
    }

    #[test]
    fn empty_and_malformed_declarations_are_dropped() {
        assert!(declarations("").is_empty());
        assert!(declarations(";;").is_empty());
        assert!(declarations("color").is_empty(), "no colon, no declaration");
        assert!(
            declarations("color:").is_empty(),
            "no value, no declaration"
        );
        assert_eq!(declarations("color: red; oops").len(), 1);
    }

    #[test]
    fn a_semicolon_inside_a_url_or_a_string_does_not_split() {
        let parsed = declarations("background: url(a;b.png); color: red");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].1, "url(a;b.png)");
        let quoted = declarations("content: 'a;b'; color: red");
        assert_eq!(quoted.len(), 2);
        assert_eq!(quoted[0].1, "'a;b'");
    }

    #[test]
    fn a_value_may_contain_a_colon() {
        let parsed = declarations("background: url(http://x/y.png)");
        assert_eq!(parsed[0].1, "url(http://x/y.png)");
    }

    #[test]
    fn reading_a_property() {
        let text = "color: red; margin: 0";
        assert_eq!(property(text, "color").as_deref(), Some("red"));
        assert_eq!(property(text, "margin").as_deref(), Some("0"));
        assert_eq!(property(text, "padding"), None);
        // A duplicated property resolves to the last one, as the cascade does.
        assert_eq!(
            property("color: red; color: blue", "color").as_deref(),
            Some("blue")
        );
    }

    #[test]
    fn setting_a_property_keeps_its_position() {
        assert_eq!(set_property("", "color", "red"), "color: red");
        assert_eq!(
            set_property("color: red; margin: 0", "color", "blue"),
            "color: blue; margin: 0"
        );
        assert_eq!(
            set_property("color: red", "margin", "0"),
            "color: red; margin: 0"
        );
    }

    #[test]
    fn removing_a_property() {
        assert_eq!(
            remove_property("color: red; margin: 0", "color"),
            "margin: 0"
        );
        assert_eq!(remove_property("color: red", "margin"), "color: red");
        assert_eq!(remove_property("color: red", "color"), "");
    }

    #[test]
    fn a_property_the_engine_does_not_know_round_trips() {
        let text = "-wat-future-thing: 3px";
        assert_eq!(
            set_property(text, "color", "red"),
            "-wat-future-thing: 3px; color: red"
        );
    }

    #[test]
    fn case_conversion() {
        assert_eq!(to_kebab_case("backgroundColor"), "background-color");
        assert_eq!(to_kebab_case("color"), "color");
        assert_eq!(to_kebab_case("background-color"), "background-color");
        assert_eq!(to_kebab_case("--custom"), "--custom");
        assert_eq!(
            to_kebab_case("borderTopLeftRadius"),
            "border-top-left-radius"
        );

        assert_eq!(to_camel_case("background-color"), "backgroundColor");
        assert_eq!(to_camel_case("color"), "color");
        assert_eq!(to_camel_case("--custom"), "--custom");
        assert_eq!(
            to_camel_case("border-top-left-radius"),
            "borderTopLeftRadius"
        );
    }

    #[test]
    fn conversion_round_trips() {
        for name in [
            "color",
            "background-color",
            "border-top-left-radius",
            "--brand",
        ] {
            assert_eq!(to_kebab_case(&to_camel_case(name)), name);
        }
    }
}
