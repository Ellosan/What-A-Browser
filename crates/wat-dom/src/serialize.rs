//! Turning a [`Document`] back into text, for debugging and the CLI.

use crate::{Document, NodeData, NodeId};
use std::fmt::Write as _;

/// Elements serialised without a closing tag.
const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

fn escape_attr(value: &str) -> String {
    escape_text(value).replace('"', "&quot;")
}

impl Document {
    /// Serialises the subtree rooted at `id` as HTML.
    pub fn to_html(&self, id: NodeId) -> String {
        let mut out = String::new();
        self.write_html(id, &mut out);
        out
    }

    /// Serialises the whole document as HTML.
    pub fn document_html(&self) -> String {
        self.to_html(self.root())
    }

    fn write_html(&self, id: NodeId, out: &mut String) {
        match self.data(id) {
            NodeData::Document => {
                for child in self.children(id) {
                    self.write_html(child, out);
                }
            }
            NodeData::Doctype { name } => {
                let _ = write!(out, "<!DOCTYPE {name}>");
            }
            NodeData::Text(text) => out.push_str(&escape_text(text)),
            NodeData::Comment(text) => {
                let _ = write!(out, "<!--{text}-->");
            }
            NodeData::Element(el) => {
                let _ = write!(out, "<{}", el.name);
                for attr in &el.attributes {
                    if attr.value.is_empty() {
                        let _ = write!(out, " {}", attr.name);
                    } else {
                        let _ = write!(out, " {}=\"{}\"", attr.name, escape_attr(&attr.value));
                    }
                }
                out.push('>');

                // `script` and `style` hold raw text.
                let raw = el.name == "script" || el.name == "style";
                for child in self.children(id) {
                    if raw {
                        if let NodeData::Text(text) = self.data(child) {
                            out.push_str(text);
                            continue;
                        }
                    }
                    self.write_html(child, out);
                }

                if !VOID.contains(&el.name.as_str()) {
                    let _ = write!(out, "</{}>", el.name);
                }
            }
        }
    }

    /// Indented tree dump, one node per line.
    pub fn to_tree_string(&self, id: NodeId) -> String {
        let mut out = String::new();
        self.write_tree(id, 0, &mut out);
        out
    }

    fn write_tree(&self, id: NodeId, depth: usize, out: &mut String) {
        for _ in 0..depth {
            out.push_str("  ");
        }
        match self.data(id) {
            NodeData::Document => out.push_str("#document"),
            NodeData::Doctype { name } => {
                let _ = write!(out, "<!DOCTYPE {name}>");
            }
            NodeData::Element(el) => {
                let _ = write!(out, "<{}>", el.name);
                for attr in &el.attributes {
                    let _ = write!(out, " {}={:?}", attr.name, attr.value);
                }
            }
            NodeData::Text(text) => {
                let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
                let shown = if collapsed.chars().count() > 60 {
                    let head: String = collapsed.chars().take(57).collect();
                    format!("{head}...")
                } else {
                    collapsed
                };
                if shown.is_empty() {
                    out.push_str("#text (whitespace)");
                } else {
                    let _ = write!(out, "#text {shown:?}");
                }
            }
            NodeData::Comment(_) => out.push_str("#comment"),
        }
        out.push('\n');
        for child in self.children(id) {
            self.write_tree(child, depth + 1, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeData;

    #[test]
    fn round_trips_simple_markup() {
        let mut doc = Document::new();
        let div = doc.create_element("div");
        doc.append(doc.root(), div);
        doc.element_mut(div).unwrap().set_attr("class", "a");
        let text = doc.create_text("x < y");
        doc.append(div, text);
        assert_eq!(doc.document_html(), r#"<div class="a">x &lt; y</div>"#);
    }

    #[test]
    fn void_elements_have_no_close_tag() {
        let mut doc = Document::new();
        let br = doc.create_element("br");
        doc.append(doc.root(), br);
        assert_eq!(doc.document_html(), "<br>");
    }

    #[test]
    fn tree_dump_is_indented() {
        let mut doc = Document::new();
        let ul = doc.create_element("ul");
        doc.append(doc.root(), ul);
        let li = doc.create_element("li");
        doc.append(ul, li);
        let dump = doc.to_tree_string(doc.root());
        assert_eq!(dump, "#document\n  <ul>\n    <li>\n");
    }

    #[test]
    fn comments_survive() {
        let mut doc = Document::new();
        let c = doc.create(NodeData::Comment(" hi ".into()));
        doc.append(doc.root(), c);
        assert_eq!(doc.document_html(), "<!-- hi -->");
    }
}
