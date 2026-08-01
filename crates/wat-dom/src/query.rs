//! Tiny convenience matcher used by the engine and the UA layer.
//!
//! This is intentionally *not* the CSS selector engine — that lives in
//! `glass-css`. It only covers `tag`, `.class` and `#id` lookups so crates that
//! do not depend on the CSS crate can still find nodes.

use crate::{Document, NodeId};

/// A one-component match target: `div`, `.card` or `#main`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selectorish {
    Tag(String),
    Class(String),
    Id(String),
}

impl Selectorish {
    pub fn parse(input: &str) -> Option<Self> {
        let input = input.trim();
        let mut chars = input.chars();
        match chars.next()? {
            '.' => Some(Selectorish::Class(chars.as_str().to_string())),
            '#' => Some(Selectorish::Id(chars.as_str().to_string())),
            _ if input.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') => {
                Some(Selectorish::Tag(input.to_ascii_lowercase()))
            }
            _ => None,
        }
    }

    pub fn matches(&self, doc: &Document, id: NodeId) -> bool {
        let Some(el) = doc.element(id) else {
            return false;
        };
        match self {
            Selectorish::Tag(name) => el.name == *name,
            Selectorish::Class(class) => el.classes().any(|c| c == class),
            Selectorish::Id(wanted) => el.id() == Some(wanted.as_str()),
        }
    }
}

impl Document {
    /// First node in document order matching `selector`.
    pub fn query(&self, selector: &str) -> Option<NodeId> {
        let sel = Selectorish::parse(selector)?;
        self.descendants(self.root())
            .find(|id| sel.matches(self, *id))
    }

    /// All nodes matching `selector`, in document order.
    pub fn query_all(&self, selector: &str) -> Vec<NodeId> {
        match Selectorish::parse(selector) {
            Some(sel) => self
                .descendants(self.root())
                .filter(|id| sel.matches(self, *id))
                .collect(),
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeData;

    #[test]
    fn finds_by_class_and_id() {
        let mut doc = Document::new();
        let div = doc.create_element("div");
        doc.append(doc.root(), div);
        doc.element_mut(div).unwrap().set_attr("class", "card wide");
        let span = doc.create_element("span");
        doc.append(div, span);
        doc.element_mut(span).unwrap().set_attr("id", "label");

        assert_eq!(doc.query(".card"), Some(div));
        assert_eq!(doc.query("#label"), Some(span));
        assert_eq!(doc.query("span"), Some(span));
        assert!(doc.query(".missing").is_none());
    }

    #[test]
    fn ignores_non_elements() {
        let mut doc = Document::new();
        let comment = doc.create(NodeData::Comment("x".into()));
        doc.append(doc.root(), comment);
        assert!(doc.query("div").is_none());
    }
}
