//! Selector parsing, specificity and matching.
//!
//! Matching runs right-to-left, which is what makes descendant selectors cheap:
//! the rightmost compound is tested first and only then do we walk ancestors.

use crate::tokenizer::{tokenize, Token};
use wat_dom::{Document, NodeId};

/// Attribute selector operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttrOp {
    /// `[attr]`
    Exists,
    /// `[attr=value]`
    Equals,
    /// `[attr~=value]` — whitespace-separated word match.
    Includes,
    /// `[attr|=value]` — exact match or `value-` prefix.
    DashMatch,
    /// `[attr^=value]`
    Prefix,
    /// `[attr$=value]`
    Suffix,
    /// `[attr*=value]`
    Substring,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttrSelector {
    pub name: String,
    pub op: AttrOp,
    pub value: String,
    /// `[attr=value i]`
    pub case_insensitive: bool,
}

impl AttrSelector {
    fn matches(&self, actual: Option<&str>) -> bool {
        let Some(actual) = actual else {
            return false;
        };
        let (haystack, needle) = if self.case_insensitive {
            (actual.to_ascii_lowercase(), self.value.to_ascii_lowercase())
        } else {
            (actual.to_string(), self.value.clone())
        };
        match self.op {
            AttrOp::Exists => true,
            AttrOp::Equals => haystack == needle,
            AttrOp::Includes => {
                !needle.is_empty() && haystack.split_ascii_whitespace().any(|w| w == needle)
            }
            AttrOp::DashMatch => haystack == needle || haystack.starts_with(&format!("{needle}-")),
            AttrOp::Prefix => !needle.is_empty() && haystack.starts_with(&needle),
            AttrOp::Suffix => !needle.is_empty() && haystack.ends_with(&needle),
            AttrOp::Substring => !needle.is_empty() && haystack.contains(&needle),
        }
    }
}

/// An `an+b` pattern for `:nth-*()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Nth {
    pub a: i32,
    pub b: i32,
}

impl Nth {
    /// `index` is 1-based, per CSS.
    pub fn matches(&self, index: i32) -> bool {
        if self.a == 0 {
            return index == self.b;
        }
        let offset = index - self.b;
        offset % self.a == 0 && offset / self.a >= 0
    }

    fn parse(text: &str) -> Option<Nth> {
        let text: String = text
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_ascii_lowercase();
        match text.as_str() {
            "odd" => return Some(Nth { a: 2, b: 1 }),
            "even" => return Some(Nth { a: 2, b: 0 }),
            _ => {}
        }
        if let Some(pos) = text.find('n') {
            let coefficient = &text[..pos];
            let a = match coefficient {
                "" | "+" => 1,
                "-" => -1,
                other => other.parse().ok()?,
            };
            let rest = &text[pos + 1..];
            let b = if rest.is_empty() {
                0
            } else {
                rest.parse().ok()?
            };
            return Some(Nth { a, b });
        }
        Some(Nth {
            a: 0,
            b: text.parse().ok()?,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PseudoClass {
    Hover,
    Active,
    Focus,
    FocusVisible,
    FocusWithin,
    FirstChild,
    LastChild,
    OnlyChild,
    FirstOfType,
    LastOfType,
    OnlyOfType,
    Root,
    Empty,
    Checked,
    Disabled,
    Enabled,
    Required,
    Link,
    AnyLink,
    Visited,
    Target,
    NthChild(Nth),
    NthLastChild(Nth),
    NthOfType(Nth),
    NthLastOfType(Nth),
    Not(Vec<Compound>),
    Is(Vec<Compound>),
    /// Recognised syntactically but never matched, so unknown pseudo-classes
    /// cannot make a rule apply by accident.
    Unsupported(String),
}

/// Pseudo-elements are parsed so selectors are not silently mismatched, but the
/// engine only generates boxes for `::before` and `::after`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PseudoElement {
    Before,
    After,
    Marker,
    Placeholder,
    Selection,
    Other,
}

/// A sequence of simple selectors with no combinator between them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Compound {
    /// `None` matches any tag name.
    pub tag: Option<String>,
    /// Set by an explicit `*`. Adds nothing to matching, but distinguishes `*`
    /// from an empty compound so `* { … }` parses.
    pub universal: bool,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub attrs: Vec<AttrSelector>,
    pub pseudos: Vec<PseudoClass>,
}

impl Compound {
    fn is_empty(&self) -> bool {
        !self.universal
            && self.tag.is_none()
            && self.id.is_none()
            && self.classes.is_empty()
            && self.attrs.is_empty()
            && self.pseudos.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Combinator {
    Descendant,
    Child,
    NextSibling,
    SubsequentSibling,
}

/// Specificity triple, ordered as CSS requires.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Specificity {
    pub ids: u16,
    pub classes: u16,
    pub types: u16,
}

/// One selector in a selector list, e.g. `nav > a.active`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selector {
    /// Compounds in source order.
    pub compounds: Vec<Compound>,
    /// `combinators[i]` joins `compounds[i]` to `compounds[i + 1]`.
    pub combinators: Vec<Combinator>,
    pub pseudo_element: Option<PseudoElement>,
    pub specificity: Specificity,
}

impl Selector {
    /// Does this selector match `node`?
    pub fn matches(&self, doc: &Document, node: NodeId, ctx: &MatchContext) -> bool {
        if self.compounds.is_empty() {
            return false;
        }
        self.match_from(doc, node, self.compounds.len() - 1, ctx)
    }

    fn match_from(&self, doc: &Document, node: NodeId, index: usize, ctx: &MatchContext) -> bool {
        if !matches_compound(&self.compounds[index], doc, node, ctx) {
            return false;
        }
        if index == 0 {
            return true;
        }
        let combinator = self.combinators[index - 1];
        match combinator {
            Combinator::Child => match doc.node(node).parent {
                Some(parent) if doc.node(parent).is_element() => {
                    self.match_from(doc, parent, index - 1, ctx)
                }
                _ => false,
            },
            Combinator::Descendant => {
                let mut current = doc.node(node).parent;
                while let Some(ancestor) = current {
                    if doc.node(ancestor).is_element()
                        && self.match_from(doc, ancestor, index - 1, ctx)
                    {
                        return true;
                    }
                    current = doc.node(ancestor).parent;
                }
                false
            }
            Combinator::NextSibling => match previous_element(doc, node) {
                Some(sibling) => self.match_from(doc, sibling, index - 1, ctx),
                None => false,
            },
            Combinator::SubsequentSibling => {
                let mut current = previous_element(doc, node);
                while let Some(sibling) = current {
                    if self.match_from(doc, sibling, index - 1, ctx) {
                        return true;
                    }
                    current = previous_element(doc, sibling);
                }
                false
            }
        }
    }
}

fn previous_element(doc: &Document, node: NodeId) -> Option<NodeId> {
    let mut current = doc.node(node).prev_sibling;
    while let Some(id) = current {
        if doc.node(id).is_element() {
            return Some(id);
        }
        current = doc.node(id).prev_sibling;
    }
    None
}

/// Interactive state that selectors can observe.
#[derive(Clone, Copy, Debug, Default)]
pub struct MatchContext {
    pub hover: Option<NodeId>,
    pub active: Option<NodeId>,
    pub focus: Option<NodeId>,
    /// Node matched by the document fragment, for `:target`.
    pub target: Option<NodeId>,
}

impl MatchContext {
    pub fn with_hover(node: Option<NodeId>) -> Self {
        MatchContext {
            hover: node,
            ..Default::default()
        }
    }

    /// True when `node` is the hovered node or an ancestor of it.
    fn hovered(&self, doc: &Document, node: NodeId) -> bool {
        match self.hover {
            Some(hover) => hover == node || doc.ancestors(hover).any(|a| a == node),
            None => false,
        }
    }
}

fn matches_compound(compound: &Compound, doc: &Document, node: NodeId, ctx: &MatchContext) -> bool {
    let Some(element) = doc.element(node) else {
        return false;
    };
    if let Some(tag) = &compound.tag {
        if element.name != *tag {
            return false;
        }
    }
    if let Some(id) = &compound.id {
        if element.id() != Some(id.as_str()) {
            return false;
        }
    }
    for class in &compound.classes {
        if !element.classes().any(|c| c == class) {
            return false;
        }
    }
    for attr in &compound.attrs {
        if !attr.matches(element.attr(&attr.name)) {
            return false;
        }
    }
    compound
        .pseudos
        .iter()
        .all(|pseudo| matches_pseudo(pseudo, doc, node, ctx))
}

fn matches_pseudo(pseudo: &PseudoClass, doc: &Document, node: NodeId, ctx: &MatchContext) -> bool {
    let element = match doc.element(node) {
        Some(el) => el,
        None => return false,
    };
    let (index, count) = doc.element_index(node);
    let one_based = index as i32 + 1;

    match pseudo {
        PseudoClass::Hover => ctx.hovered(doc, node),
        PseudoClass::Active => ctx.active == Some(node),
        PseudoClass::Focus | PseudoClass::FocusVisible => ctx.focus == Some(node),
        PseudoClass::FocusWithin => match ctx.focus {
            Some(focus) => focus == node || doc.ancestors(focus).any(|a| a == node),
            None => false,
        },
        PseudoClass::FirstChild => index == 0,
        PseudoClass::LastChild => index + 1 == count,
        PseudoClass::OnlyChild => count == 1,
        PseudoClass::Root => doc
            .node(node)
            .parent
            .is_some_and(|p| !doc.node(p).is_element()),
        PseudoClass::Empty => doc.children(node).all(|child| {
            doc.node(child)
                .as_text()
                .is_some_and(|t| t.chars().all(char::is_whitespace))
        }),
        PseudoClass::Checked => element.has_attr("checked") || element.has_attr("selected"),
        PseudoClass::Disabled => element.has_attr("disabled"),
        PseudoClass::Enabled => !element.has_attr("disabled"),
        PseudoClass::Required => element.has_attr("required"),
        PseudoClass::Link | PseudoClass::AnyLink => element.name == "a" && element.has_attr("href"),
        // Without history integration every link is unvisited.
        PseudoClass::Visited => false,
        PseudoClass::Target => ctx.target == Some(node),
        PseudoClass::NthChild(nth) => nth.matches(one_based),
        PseudoClass::NthLastChild(nth) => nth.matches(count as i32 - index as i32),
        PseudoClass::NthOfType(nth) => {
            let (type_index, _) = type_index(doc, node, &element.name);
            nth.matches(type_index as i32 + 1)
        }
        PseudoClass::NthLastOfType(nth) => {
            let (type_index, type_count) = type_index(doc, node, &element.name);
            nth.matches(type_count as i32 - type_index as i32)
        }
        PseudoClass::FirstOfType => type_index(doc, node, &element.name).0 == 0,
        PseudoClass::LastOfType => {
            let (i, n) = type_index(doc, node, &element.name);
            i + 1 == n
        }
        PseudoClass::OnlyOfType => type_index(doc, node, &element.name).1 == 1,
        PseudoClass::Not(compounds) => !compounds
            .iter()
            .any(|compound| matches_compound(compound, doc, node, ctx)),
        PseudoClass::Is(compounds) => compounds
            .iter()
            .any(|compound| matches_compound(compound, doc, node, ctx)),
        PseudoClass::Unsupported(_) => false,
    }
}

/// Index of `node` among siblings with the same tag name, plus that count.
fn type_index(doc: &Document, node: NodeId, name: &str) -> (usize, usize) {
    let Some(parent) = doc.node(node).parent else {
        return (0, 1);
    };
    let same: Vec<NodeId> = doc
        .element_children(parent)
        .filter(|id| doc.element(*id).is_some_and(|el| el.name == name))
        .collect();
    let index = same.iter().position(|id| *id == node).unwrap_or(0);
    (index, same.len())
}

/// Parses a comma-separated selector list. Unparseable selectors are dropped,
/// matching the CSS error-recovery rule.
pub fn parse_selector_list(input: &str) -> Vec<Selector> {
    let tokens = tokenize(input);
    let mut selectors = Vec::new();
    for chunk in split_on_commas(&tokens) {
        if let Some(selector) = parse_selector(&chunk) {
            selectors.push(selector);
        }
    }
    selectors
}

fn split_on_commas(tokens: &[Token]) -> Vec<Vec<Token>> {
    let mut chunks = vec![Vec::new()];
    let mut depth = 0usize;
    for token in tokens {
        match token {
            Token::ParenOpen | Token::BracketOpen | Token::Function(_) => {
                depth += 1;
                chunks.last_mut().unwrap().push(token.clone());
            }
            Token::ParenClose | Token::BracketClose => {
                depth = depth.saturating_sub(1);
                chunks.last_mut().unwrap().push(token.clone());
            }
            Token::Comma if depth == 0 => chunks.push(Vec::new()),
            other => chunks.last_mut().unwrap().push(other.clone()),
        }
    }
    chunks
}

fn parse_selector(tokens: &[Token]) -> Option<Selector> {
    let mut compounds: Vec<Compound> = Vec::new();
    let mut combinators: Vec<Combinator> = Vec::new();
    let mut pseudo_element = None;
    let mut current = Compound::default();
    let mut index = 0;
    // Whether whitespace seen since the last compound implies a descendant
    // combinator (only if a real compound follows).
    let mut pending_space = false;

    while index < tokens.len() {
        match &tokens[index] {
            Token::Whitespace => {
                if !current.is_empty() {
                    pending_space = true;
                }
                index += 1;
            }
            Token::Delim('>') | Token::Delim('+') | Token::Delim('~') => {
                let combinator = match &tokens[index] {
                    Token::Delim('>') => Combinator::Child,
                    Token::Delim('+') => Combinator::NextSibling,
                    _ => Combinator::SubsequentSibling,
                };
                if current.is_empty() {
                    return None; // combinator with nothing before it
                }
                compounds.push(std::mem::take(&mut current));
                combinators.push(combinator);
                pending_space = false;
                index += 1;
            }
            Token::Delim('*') => {
                if pending_space {
                    compounds.push(std::mem::take(&mut current));
                    combinators.push(Combinator::Descendant);
                    pending_space = false;
                }
                current.universal = true;
                index += 1;
            }
            Token::Delim('.') => {
                if pending_space {
                    compounds.push(std::mem::take(&mut current));
                    combinators.push(Combinator::Descendant);
                    pending_space = false;
                }
                index += 1;
                match tokens.get(index) {
                    Some(Token::Ident(name)) => {
                        current.classes.push(name.clone());
                        index += 1;
                    }
                    _ => return None,
                }
            }
            Token::Hash(name) => {
                if pending_space {
                    compounds.push(std::mem::take(&mut current));
                    combinators.push(Combinator::Descendant);
                    pending_space = false;
                }
                current.id = Some(name.clone());
                index += 1;
            }
            Token::Ident(name) => {
                if pending_space {
                    compounds.push(std::mem::take(&mut current));
                    combinators.push(Combinator::Descendant);
                    pending_space = false;
                }
                if current.tag.is_some() {
                    return None;
                }
                current.tag = Some(name.to_ascii_lowercase());
                index += 1;
            }
            Token::BracketOpen => {
                if pending_space {
                    compounds.push(std::mem::take(&mut current));
                    combinators.push(Combinator::Descendant);
                    pending_space = false;
                }
                let (attr, consumed) = parse_attr(&tokens[index..])?;
                current.attrs.push(attr);
                index += consumed;
            }
            Token::Colon => {
                if pending_space {
                    compounds.push(std::mem::take(&mut current));
                    combinators.push(Combinator::Descendant);
                    pending_space = false;
                }
                // `::` introduces a pseudo-element.
                if matches!(tokens.get(index + 1), Some(Token::Colon)) {
                    let name = match tokens.get(index + 2) {
                        Some(Token::Ident(name)) => name.to_ascii_lowercase(),
                        Some(Token::Function(name)) => name.to_ascii_lowercase(),
                        _ => return None,
                    };
                    pseudo_element = Some(match name.as_str() {
                        "before" => PseudoElement::Before,
                        "after" => PseudoElement::After,
                        "marker" => PseudoElement::Marker,
                        "placeholder" => PseudoElement::Placeholder,
                        "selection" => PseudoElement::Selection,
                        _ => PseudoElement::Other,
                    });
                    index += 3;
                    // Skip a function body if present.
                    if matches!(tokens.get(index - 1), Some(Token::Function(_))) {
                        index = skip_balanced(tokens, index);
                    }
                    continue;
                }
                let (pseudo, consumed) = parse_pseudo(&tokens[index..])?;
                // Single-colon `:before` is the legacy pseudo-element syntax.
                match &pseudo {
                    PseudoClass::Unsupported(name) if name == "before" => {
                        pseudo_element = Some(PseudoElement::Before);
                    }
                    PseudoClass::Unsupported(name) if name == "after" => {
                        pseudo_element = Some(PseudoElement::After);
                    }
                    _ => current.pseudos.push(pseudo),
                }
                index += consumed;
            }
            _ => return None,
        }
    }

    if current.is_empty() && compounds.is_empty() {
        return None;
    }
    compounds.push(current);
    if compounds.len() != combinators.len() + 1 {
        return None;
    }

    let specificity = specificity_of(&compounds, pseudo_element);
    Some(Selector {
        compounds,
        combinators,
        pseudo_element,
        specificity,
    })
}

fn specificity_of(compounds: &[Compound], pseudo_element: Option<PseudoElement>) -> Specificity {
    let mut spec = Specificity::default();
    for compound in compounds {
        if compound.id.is_some() {
            spec.ids += 1;
        }
        spec.classes += compound.classes.len() as u16;
        spec.classes += compound.attrs.len() as u16;
        for pseudo in &compound.pseudos {
            match pseudo {
                // `:not()` and `:is()` contribute their argument's specificity.
                PseudoClass::Not(inner) | PseudoClass::Is(inner) => {
                    let inner_spec = specificity_of(inner, None);
                    spec.ids += inner_spec.ids;
                    spec.classes += inner_spec.classes;
                    spec.types += inner_spec.types;
                }
                _ => spec.classes += 1,
            }
        }
        if compound.tag.is_some() {
            spec.types += 1;
        }
    }
    if pseudo_element.is_some() {
        spec.types += 1;
    }
    spec
}

/// Index just past the matching close paren, given `tokens[start - 1]` opened it.
fn skip_balanced(tokens: &[Token], start: usize) -> usize {
    let mut depth = 1usize;
    let mut index = start;
    while index < tokens.len() {
        match &tokens[index] {
            Token::ParenOpen | Token::Function(_) => depth += 1,
            Token::ParenClose => {
                depth -= 1;
                if depth == 0 {
                    return index + 1;
                }
            }
            _ => {}
        }
        index += 1;
    }
    index
}

fn parse_attr(tokens: &[Token]) -> Option<(AttrSelector, usize)> {
    // tokens[0] is `[`
    let mut index = 1;
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

    let op = match tokens.get(index) {
        Some(Token::BracketClose) => {
            return Some((
                AttrSelector {
                    name,
                    op: AttrOp::Exists,
                    value: String::new(),
                    case_insensitive: false,
                },
                index + 1,
            ))
        }
        Some(Token::Delim('=')) => {
            index += 1;
            AttrOp::Equals
        }
        Some(Token::Delim(prefix @ ('~' | '|' | '^' | '$' | '*'))) => {
            let prefix = *prefix;
            if !matches!(tokens.get(index + 1), Some(Token::Delim('='))) {
                return None;
            }
            index += 2;
            match prefix {
                '~' => AttrOp::Includes,
                '|' => AttrOp::DashMatch,
                '^' => AttrOp::Prefix,
                '$' => AttrOp::Suffix,
                _ => AttrOp::Substring,
            }
        }
        _ => return None,
    };

    while matches!(tokens.get(index), Some(Token::Whitespace)) {
        index += 1;
    }
    let value = match tokens.get(index)? {
        Token::Str(s) => s.clone(),
        Token::Ident(s) => s.clone(),
        Token::Number(n) => crate::values::Value::Number(*n).to_css(),
        _ => return None,
    };
    index += 1;

    let mut case_insensitive = false;
    loop {
        match tokens.get(index) {
            Some(Token::Whitespace) => index += 1,
            Some(Token::Ident(flag)) if flag.eq_ignore_ascii_case("i") => {
                case_insensitive = true;
                index += 1;
            }
            Some(Token::Ident(flag)) if flag.eq_ignore_ascii_case("s") => index += 1,
            Some(Token::BracketClose) => {
                index += 1;
                break;
            }
            _ => return None,
        }
    }

    Some((
        AttrSelector {
            name,
            op,
            value,
            case_insensitive,
        },
        index,
    ))
}

fn parse_pseudo(tokens: &[Token]) -> Option<(PseudoClass, usize)> {
    // tokens[0] is `:`
    match tokens.get(1)? {
        Token::Ident(name) => {
            let pseudo = match name.to_ascii_lowercase().as_str() {
                "hover" => PseudoClass::Hover,
                "active" => PseudoClass::Active,
                "focus" => PseudoClass::Focus,
                "focus-visible" => PseudoClass::FocusVisible,
                "focus-within" => PseudoClass::FocusWithin,
                "first-child" => PseudoClass::FirstChild,
                "last-child" => PseudoClass::LastChild,
                "only-child" => PseudoClass::OnlyChild,
                "first-of-type" => PseudoClass::FirstOfType,
                "last-of-type" => PseudoClass::LastOfType,
                "only-of-type" => PseudoClass::OnlyOfType,
                "root" => PseudoClass::Root,
                "empty" => PseudoClass::Empty,
                "checked" => PseudoClass::Checked,
                "disabled" => PseudoClass::Disabled,
                "enabled" => PseudoClass::Enabled,
                "required" => PseudoClass::Required,
                "link" => PseudoClass::Link,
                "any-link" => PseudoClass::AnyLink,
                "visited" => PseudoClass::Visited,
                "target" => PseudoClass::Target,
                other => PseudoClass::Unsupported(other.to_string()),
            };
            Some((pseudo, 2))
        }
        Token::Function(name) => {
            let end = skip_balanced(tokens, 2);
            let args = &tokens[2..end.saturating_sub(1).max(2)];
            let lower = name.to_ascii_lowercase();
            let pseudo = match lower.as_str() {
                "nth-child" | "nth-last-child" | "nth-of-type" | "nth-last-of-type" => {
                    let text = tokens_to_text(args);
                    let nth = Nth::parse(&text)?;
                    match lower.as_str() {
                        "nth-child" => PseudoClass::NthChild(nth),
                        "nth-last-child" => PseudoClass::NthLastChild(nth),
                        "nth-of-type" => PseudoClass::NthOfType(nth),
                        _ => PseudoClass::NthLastOfType(nth),
                    }
                }
                "not" | "is" | "where" => {
                    let mut compounds = Vec::new();
                    for chunk in split_on_commas(args) {
                        let selector = parse_selector(&chunk)?;
                        // Only simple (single-compound) arguments are supported.
                        if selector.compounds.len() != 1 {
                            return Some((PseudoClass::Unsupported(lower), end));
                        }
                        compounds.push(selector.compounds[0].clone());
                    }
                    if lower == "not" {
                        PseudoClass::Not(compounds)
                    } else {
                        PseudoClass::Is(compounds)
                    }
                }
                other => PseudoClass::Unsupported(other.to_string()),
            };
            Some((pseudo, end))
        }
        _ => None,
    }
}

fn tokens_to_text(tokens: &[Token]) -> String {
    let mut out = String::new();
    for token in tokens {
        match token {
            Token::Ident(name) => out.push_str(name),
            Token::Number(n) => out.push_str(&crate::values::Value::Number(*n).to_css()),
            Token::Dimension(n, unit) => {
                out.push_str(&crate::values::Value::Number(*n).to_css());
                out.push_str(unit);
            }
            Token::Delim(ch) => out.push(*ch),
            Token::Whitespace => out.push(' '),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use wat_dom::Document;

    fn parse_one(input: &str) -> Selector {
        let mut list = parse_selector_list(input);
        assert_eq!(list.len(), 1, "expected one selector from {input:?}");
        list.pop().unwrap()
    }

    fn doc_from(html: &str) -> Document {
        // A tiny hand-rolled builder keeps this crate free of an html dependency.
        let mut doc = Document::new();
        let mut stack = vec![doc.root()];
        let mut rest = html;
        while let Some(open) = rest.find('<') {
            let text = &rest[..open];
            if !text.trim().is_empty() {
                let node = doc.create_text(text.to_string());
                let parent = *stack.last().unwrap();
                doc.append(parent, node);
            }
            let close = rest[open..].find('>').unwrap() + open;
            let tag = &rest[open + 1..close];
            rest = &rest[close + 1..];
            if let Some(name) = tag.strip_prefix('/') {
                assert!(!name.is_empty());
                stack.pop();
                continue;
            }
            let (name, attrs) = split_tag(tag);
            let node = doc.create_element(name);
            let parent = *stack.last().unwrap();
            doc.append(parent, node);
            for (key, value) in attrs {
                doc.element_mut(node).unwrap().set_attr(key, value);
            }
            stack.push(node);
        }
        doc
    }

    /// Splits `div class="a b" hidden` into a name and attribute pairs,
    /// respecting double quotes.
    fn split_tag(tag: &str) -> (String, Vec<(String, String)>) {
        let mut parts: Vec<String> = Vec::new();
        let mut current = String::new();
        let mut in_quotes = false;
        for ch in tag.chars() {
            match ch {
                '"' => in_quotes = !in_quotes,
                c if c.is_whitespace() && !in_quotes => {
                    if !current.is_empty() {
                        parts.push(std::mem::take(&mut current));
                    }
                }
                c => current.push(c),
            }
        }
        if !current.is_empty() {
            parts.push(current);
        }
        let mut iter = parts.into_iter();
        let name = iter.next().unwrap_or_default();
        let attrs = iter
            .map(|part| match part.split_once('=') {
                Some((key, value)) => (key.to_string(), value.to_string()),
                None => (part, String::new()),
            })
            .collect();
        (name, attrs)
    }

    #[test]
    fn parses_compound_parts() {
        let sel = parse_one("div#main.a.b[data-x=\"1\"]:hover");
        assert_eq!(sel.compounds.len(), 1);
        let c = &sel.compounds[0];
        assert_eq!(c.tag.as_deref(), Some("div"));
        assert_eq!(c.id.as_deref(), Some("main"));
        assert_eq!(c.classes, vec!["a", "b"]);
        assert_eq!(c.attrs.len(), 1);
        assert_eq!(c.pseudos, vec![PseudoClass::Hover]);
    }

    #[test]
    fn parses_combinators() {
        let sel = parse_one("nav > ul li + a ~ span");
        assert_eq!(sel.compounds.len(), 5);
        assert_eq!(
            sel.combinators,
            vec![
                Combinator::Child,
                Combinator::Descendant,
                Combinator::NextSibling,
                Combinator::SubsequentSibling
            ]
        );
    }

    #[test]
    fn specificity_follows_spec() {
        assert_eq!(
            parse_one("#a").specificity,
            Specificity {
                ids: 1,
                classes: 0,
                types: 0
            }
        );
        assert_eq!(
            parse_one(".a.b").specificity,
            Specificity {
                ids: 0,
                classes: 2,
                types: 0
            }
        );
        assert_eq!(
            parse_one("div p").specificity,
            Specificity {
                ids: 0,
                classes: 0,
                types: 2
            }
        );
        assert!(parse_one("#a").specificity > parse_one(".a.b.c").specificity);
        assert!(parse_one(".a").specificity > parse_one("div span").specificity);
        assert_eq!(parse_one("*").specificity, Specificity::default());
        // :not() takes its argument's specificity.
        assert_eq!(
            parse_one(":not(#x)").specificity,
            Specificity {
                ids: 1,
                classes: 0,
                types: 0
            }
        );
    }

    #[test]
    fn selector_lists_split_on_top_level_commas() {
        let list = parse_selector_list("a, b:not(.x, .y), c");
        assert_eq!(list.len(), 3);
    }

    #[test]
    fn invalid_selectors_are_dropped() {
        assert!(parse_selector_list("> div").is_empty());
        assert!(parse_selector_list("").is_empty());
        assert_eq!(parse_selector_list("a, !!!, b").len(), 2);
    }

    #[test]
    fn matches_descendant_and_child() {
        let doc = doc_from("<div class=\"wrap\"><section><p class=\"lead\">hi</p></section></div>");
        let p = doc.query(".lead").unwrap();
        let ctx = MatchContext::default();
        assert!(parse_one(".wrap p").matches(&doc, p, &ctx));
        assert!(parse_one("section > p").matches(&doc, p, &ctx));
        assert!(!parse_one(".wrap > p").matches(&doc, p, &ctx));
        assert!(parse_one("div section p.lead").matches(&doc, p, &ctx));
    }

    #[test]
    fn matches_sibling_combinators() {
        let doc =
            doc_from("<ul><li class=\"a\"></li><li class=\"b\"></li><li class=\"c\"></li></ul>");
        let ctx = MatchContext::default();
        let c = doc.query(".c").unwrap();
        assert!(parse_one(".b + .c").matches(&doc, c, &ctx));
        assert!(!parse_one(".a + .c").matches(&doc, c, &ctx));
        assert!(parse_one(".a ~ .c").matches(&doc, c, &ctx));
    }

    #[test]
    fn matches_structural_pseudos() {
        let doc = doc_from("<ul><li></li><li></li><li></li><li></li></ul>");
        let items = doc.query_all("li");
        let ctx = MatchContext::default();
        assert!(parse_one("li:first-child").matches(&doc, items[0], &ctx));
        assert!(parse_one("li:last-child").matches(&doc, items[3], &ctx));
        assert!(parse_one("li:nth-child(2)").matches(&doc, items[1], &ctx));
        assert!(parse_one("li:nth-child(odd)").matches(&doc, items[2], &ctx));
        assert!(!parse_one("li:nth-child(odd)").matches(&doc, items[1], &ctx));
        assert!(parse_one("li:nth-child(2n)").matches(&doc, items[1], &ctx));
        assert!(parse_one("li:nth-last-child(1)").matches(&doc, items[3], &ctx));
    }

    #[test]
    fn nth_formula() {
        let nth = Nth::parse("2n+1").unwrap();
        assert_eq!(nth, Nth { a: 2, b: 1 });
        assert!(nth.matches(1) && nth.matches(3) && !nth.matches(2));
        let limited = Nth::parse("-n+3").unwrap();
        assert!(limited.matches(1) && limited.matches(3) && !limited.matches(4));
        assert_eq!(Nth::parse("even").unwrap(), Nth { a: 2, b: 0 });
    }

    #[test]
    fn matches_attribute_operators() {
        let doc =
            doc_from("<a href=\"https://example.com/x\" class=\"btn big\" lang=\"en-GB\"></a>");
        let a = doc.query("a").unwrap();
        let ctx = MatchContext::default();
        assert!(parse_one("[href]").matches(&doc, a, &ctx));
        assert!(parse_one("[href^=\"https://\"]").matches(&doc, a, &ctx));
        assert!(parse_one("[href$=\"/x\"]").matches(&doc, a, &ctx));
        assert!(parse_one("[href*=\"example\"]").matches(&doc, a, &ctx));
        assert!(parse_one("[class~=\"big\"]").matches(&doc, a, &ctx));
        assert!(parse_one("[lang|=\"en\"]").matches(&doc, a, &ctx));
        assert!(!parse_one("[lang=\"EN-GB\"]").matches(&doc, a, &ctx));
        assert!(parse_one("[lang=\"EN-GB\" i]").matches(&doc, a, &ctx));
    }

    #[test]
    fn hover_applies_to_ancestors() {
        let doc = doc_from("<div class=\"card\"><span class=\"label\"></span></div>");
        let card = doc.query(".card").unwrap();
        let label = doc.query(".label").unwrap();
        let ctx = MatchContext::with_hover(Some(label));
        assert!(parse_one(".card:hover").matches(&doc, card, &ctx));
        assert!(parse_one(".label:hover").matches(&doc, label, &ctx));
        let none = MatchContext::default();
        assert!(!parse_one(".card:hover").matches(&doc, card, &none));
    }

    #[test]
    fn not_and_is() {
        let doc = doc_from("<ul><li class=\"a\"></li><li></li></ul>");
        let items = doc.query_all("li");
        let ctx = MatchContext::default();
        assert!(parse_one("li:not(.a)").matches(&doc, items[1], &ctx));
        assert!(!parse_one("li:not(.a)").matches(&doc, items[0], &ctx));
        assert!(parse_one(":is(.a, .b)").matches(&doc, items[0], &ctx));
    }

    #[test]
    fn unknown_pseudo_never_matches() {
        let doc = doc_from("<div></div>");
        let div = doc.query("div").unwrap();
        let ctx = MatchContext::default();
        assert!(!parse_one("div:has(p)").matches(&doc, div, &ctx));
        assert!(!parse_one("div:dir(ltr)").matches(&doc, div, &ctx));
    }

    #[test]
    fn pseudo_elements_are_recorded() {
        assert_eq!(
            parse_one("p::before").pseudo_element,
            Some(PseudoElement::Before)
        );
        assert_eq!(
            parse_one("p:after").pseudo_element,
            Some(PseudoElement::After)
        );
        assert_eq!(parse_one("p").pseudo_element, None);
    }

    #[test]
    fn universal_selector_matches_any_element() {
        let doc = doc_from("<div><span></span></div>");
        let ctx = MatchContext::default();
        let span = doc.query("span").unwrap();
        assert!(parse_one("*").matches(&doc, span, &ctx));
        assert!(parse_one("div *").matches(&doc, span, &ctx));
    }

    #[test]
    fn root_matches_html_element() {
        let doc = doc_from("<html><body></body></html>");
        let html = doc.query("html").unwrap();
        let body = doc.query("body").unwrap();
        let ctx = MatchContext::default();
        assert!(parse_one(":root").matches(&doc, html, &ctx));
        assert!(!parse_one(":root").matches(&doc, body, &ctx));
    }
}
