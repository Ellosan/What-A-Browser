//! Tree construction: token stream in, [`Document`] out.
//!
//! The full HTML5 tree construction algorithm has twenty-three insertion modes
//! and the adoption agency algorithm. This is a reduced model that reproduces
//! the behaviour real pages depend on:
//!
//! * `html`, `head` and `body` are implied and reused if written explicitly;
//! * void elements never take children;
//! * implied end tags close `p`, list items, definition items, options, and
//!   table rows/cells before an incompatible sibling opens;
//! * a stray end tag with no matching open element is ignored;
//! * anything still open at end of input is closed.
//!
//! Known deviations are recorded in `docs/ARCHITECTURE.md`: there is no foster
//! parenting for misplaced table content, and mis-nested inline formatting is
//! closed rather than reconstructed.

use crate::tokenizer::{tokenize, Token};
use wat_dom::{Document, Element, NodeData, NodeId};

/// Elements that never have children.
pub const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "basefont", "br", "col", "embed", "frame", "hr", "img", "input", "keygen",
    "link", "meta", "param", "source", "track", "wbr",
];

/// Elements that belong in `<head>` when they appear before body content.
const HEAD_ELEMENTS: &[&str] = &[
    "base", "link", "meta", "title", "style", "script", "noscript",
];

/// Block-level containers whose start tag implicitly closes an open `<p>`.
const CLOSES_PARAGRAPH: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "details",
    "dialog",
    "div",
    "dl",
    "dd",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hgroup",
    "hr",
    "li",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "summary",
    "table",
    "ul",
];

pub struct TreeBuilder {
    document: Document,
    /// Stack of currently open elements, innermost last.
    open: Vec<NodeId>,
    head: NodeId,
    body: NodeId,
    /// Set once body content has been inserted; head-only elements after this
    /// point stay where they are written.
    body_started: bool,
    saw_doctype: bool,
}

impl TreeBuilder {
    pub fn new() -> Self {
        let mut document = Document::new();
        let root = document.root();
        let html = document.create_element("html");
        document.append(root, html);
        let head = document.create_element("head");
        document.append(html, head);
        let body = document.create_element("body");
        document.append(html, body);

        TreeBuilder {
            document,
            open: vec![html, body],
            head,
            body,
            body_started: false,
            saw_doctype: false,
        }
    }

    fn current(&self) -> NodeId {
        *self.open.last().unwrap_or(&self.body)
    }

    fn current_name(&self) -> &str {
        self.document
            .element(self.current())
            .map(|el| el.name.as_str())
            .unwrap_or("")
    }

    /// Is `name` on the open element stack?
    fn is_open(&self, name: &str) -> bool {
        self.open
            .iter()
            .any(|id| self.document.element(*id).is_some_and(|el| el.name == name))
    }

    /// Pops open elements until (and including) the innermost `name`.
    fn close_element(&mut self, name: &str) {
        if !self.is_open(name) {
            return;
        }
        while let Some(id) = self.open.pop() {
            let matched = self.document.element(id).is_some_and(|el| el.name == name);
            if matched {
                break;
            }
        }
    }

    /// Pops any of `names` that are currently the innermost open elements.
    fn close_while_current_in(&mut self, names: &[&str]) {
        while self.open.len() > 1 && names.contains(&self.current_name()) {
            self.open.pop();
        }
    }

    fn imply_end_tags_for(&mut self, name: &str) {
        match name {
            "li" => self.close_while_current_in(&["li"]),
            "dt" | "dd" => self.close_while_current_in(&["dt", "dd"]),
            "option" => self.close_while_current_in(&["option"]),
            "optgroup" => self.close_while_current_in(&["option", "optgroup"]),
            "tr" => self.close_while_current_in(&["td", "th", "tr"]),
            "td" | "th" => self.close_while_current_in(&["td", "th"]),
            "thead" | "tbody" | "tfoot" => {
                self.close_while_current_in(&["td", "th", "tr", "thead", "tbody", "tfoot"])
            }
            // Nested anchors are not allowed; the outer one closes.
            "a" if self.is_open("a") => self.close_element("a"),
            _ => {}
        }
        if CLOSES_PARAGRAPH.contains(&name) && self.is_open("p") {
            self.close_element("p");
        }
    }

    /// Where a new node should be inserted.
    fn insertion_point(&mut self, name: &str) -> NodeId {
        if !self.body_started && HEAD_ELEMENTS.contains(&name) && self.open.len() <= 2 {
            return self.head;
        }
        self.current()
    }

    fn merge_attributes(&mut self, target: NodeId, element: Element) {
        if let Some(existing) = self.document.element_mut(target) {
            for attr in element.attributes {
                if !existing.has_attr(&attr.name) {
                    existing.attributes.push(attr);
                }
            }
        }
    }

    fn start_tag(&mut self, name: String, attributes: Vec<wat_dom::Attribute>, self_closing: bool) {
        let element = Element { name, attributes };
        let name = element.name.clone();

        match name.as_str() {
            // Re-opening the structural elements just merges attributes.
            "html" => {
                let html = self.open[0];
                self.merge_attributes(html, element);
                return;
            }
            "head" => {
                let head = self.head;
                self.merge_attributes(head, element);
                return;
            }
            "body" => {
                let body = self.body;
                self.merge_attributes(body, element);
                self.body_started = true;
                return;
            }
            // `<frameset>` and friends are not supported; treat as generic.
            _ => {}
        }

        self.imply_end_tags_for(&name);

        let parent = self.insertion_point(&name);
        if parent != self.head {
            self.body_started = true;
        }
        let node = self.document.create(NodeData::Element(element));
        self.document.append(parent, node);

        let void = VOID_ELEMENTS.contains(&name.as_str()) || self_closing;
        if !void {
            self.open.push(node);
        }
    }

    fn end_tag(&mut self, name: &str) {
        match name {
            "html" | "body" => {
                // Closing these just unwinds everything inside them.
                while self.open.len() > 2 {
                    self.open.pop();
                }
            }
            "head" => {}
            _ if VOID_ELEMENTS.contains(&name) => {}
            _ => self.close_element(name),
        }
    }

    fn text(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        let parent = self.current();
        // Whitespace between top-level structural elements carries no meaning.
        if !self.body_started && parent == self.body && text.chars().all(char::is_whitespace) {
            return;
        }
        if parent == self.body {
            self.body_started = true;
        }

        // Coalesce with a preceding text node so inline layout sees runs whole.
        if let Some(last) = self.document.node(parent).last_child {
            if let NodeData::Text(existing) = &mut self.document.node_mut(last).data {
                existing.push_str(&text);
                return;
            }
        }
        let node = self.document.create_text(text);
        self.document.append(parent, node);
    }

    pub fn process(&mut self, token: Token) {
        match token {
            Token::Doctype(name) => {
                if !self.saw_doctype {
                    self.saw_doctype = true;
                    let root = self.document.root();
                    let node = self.document.create(NodeData::Doctype { name });
                    // Doctype belongs before <html>.
                    self.document.append(root, node);
                    let html = self.open[0];
                    self.document.detach(html);
                    self.document.append(root, html);
                }
            }
            Token::StartTag {
                name,
                attributes,
                self_closing,
            } => self.start_tag(name, attributes, self_closing),
            Token::EndTag { name } => self.end_tag(&name),
            Token::Text(text) => self.text(text),
            Token::Comment(text) => {
                let parent = self.current();
                let node = self.document.create(NodeData::Comment(text));
                self.document.append(parent, node);
            }
        }
    }

    pub fn finish(mut self) -> Document {
        self.document.quirks = !self.saw_doctype;
        self.document
    }
}

impl Default for TreeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Parses an HTML document.
pub fn parse(input: &str) -> Document {
    let mut builder = TreeBuilder::new();
    for token in tokenize(input) {
        builder.process(token);
    }
    builder.finish()
}

/// Parses an HTML document, recording where it came from.
pub fn parse_with_base(input: &str, base_url: impl Into<String>) -> Document {
    let mut document = parse(input);
    document.base_url = Some(base_url.into());
    document
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(doc: &Document, parent: NodeId) -> Vec<String> {
        doc.element_children(parent)
            .filter_map(|c| doc.element(c).map(|el| el.name.clone()))
            .collect()
    }

    #[test]
    fn implies_html_head_body() {
        let doc = parse("<p>hi</p>");
        let html = doc.find_tag("html").unwrap();
        assert_eq!(tags(&doc, html), vec!["head", "body"]);
        let body = doc.body().unwrap();
        assert_eq!(tags(&doc, body), vec!["p"]);
    }

    #[test]
    fn head_elements_land_in_head() {
        let doc = parse("<title>T</title><link rel=x><p>hi</p>");
        let head = doc.find_tag("head").unwrap();
        assert_eq!(tags(&doc, head), vec!["title", "link"]);
        assert_eq!(doc.title().as_deref(), Some("T"));
    }

    #[test]
    fn explicit_structure_is_reused_not_duplicated() {
        let doc = parse(
            "<html lang=en><head><title>T</title></head><body class=x><p>1</p></body></html>",
        );
        assert_eq!(doc.query_all("html").len(), 1);
        assert_eq!(doc.query_all("body").len(), 1);
        let html = doc.find_tag("html").unwrap();
        assert_eq!(doc.element(html).unwrap().attr("lang"), Some("en"));
        let body = doc.body().unwrap();
        assert_eq!(doc.element(body).unwrap().attr("class"), Some("x"));
    }

    #[test]
    fn unclosed_paragraphs_are_siblings() {
        let doc = parse("<p>one<p>two<p>three");
        let body = doc.body().unwrap();
        assert_eq!(tags(&doc, body), vec!["p", "p", "p"]);
        let first = doc.query("p").unwrap();
        assert_eq!(doc.text_content(first), "one");
    }

    #[test]
    fn list_items_close_each_other() {
        let doc = parse("<ul><li>a<li>b</ul>");
        let ul = doc.query("ul").unwrap();
        assert_eq!(tags(&doc, ul), vec!["li", "li"]);
    }

    #[test]
    fn void_elements_take_no_children() {
        let doc = parse("<div><br>after</div>");
        let div = doc.query("div").unwrap();
        assert_eq!(doc.text_content(div), "after");
        let br = doc.query("br").unwrap();
        assert_eq!(doc.children(br).count(), 0);
    }

    #[test]
    fn stray_end_tag_is_ignored() {
        let doc = parse("<div>a</span>b</div>");
        let div = doc.query("div").unwrap();
        assert_eq!(doc.text_content(div), "ab");
    }

    #[test]
    fn mis_nested_inline_elements_recover() {
        let doc = parse("<b>bold<i>both</b>italic</i>");
        // Not spec-perfect (no adoption agency) but the text survives in order.
        let body = doc.body().unwrap();
        assert_eq!(doc.text_content(body), "boldbothitalic");
    }

    #[test]
    fn table_rows_and_cells_auto_close() {
        let doc = parse("<table><tr><td>a<td>b<tr><td>c</table>");
        let rows = doc.query_all("tr");
        assert_eq!(rows.len(), 2);
        assert_eq!(doc.element_children(rows[0]).count(), 2);
        assert_eq!(doc.element_children(rows[1]).count(), 1);
    }

    #[test]
    fn quirks_flag_tracks_doctype() {
        assert!(parse("<p>x</p>").quirks);
        assert!(!parse("<!doctype html><p>x</p>").quirks);
    }

    #[test]
    fn doctype_precedes_html() {
        let doc = parse("<!doctype html><p>x</p>");
        let children: Vec<_> = doc.children(doc.root()).collect();
        assert!(matches!(doc.data(children[0]), NodeData::Doctype { .. }));
        assert_eq!(doc.element(children[1]).unwrap().name, "html");
    }

    #[test]
    fn adjacent_text_is_coalesced() {
        let doc = parse("<p>a&amp;b</p>");
        let p = doc.query("p").unwrap();
        assert_eq!(doc.children(p).count(), 1);
        assert_eq!(doc.text_content(p), "a&b");
    }

    #[test]
    fn script_body_is_kept_as_text() {
        let doc = parse("<script>let a = 1 < 2;</script>");
        let script = doc.query("script").unwrap();
        assert_eq!(doc.text_content(script), "let a = 1 < 2;");
    }
}
