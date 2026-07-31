//! Arena-backed document object model.
//!
//! Nodes live in a flat `Vec` and reference each other by index, which keeps the
//! tree cheap to clone, trivially serialisable and free of reference cycles.

mod query;
mod serialize;

pub use query::Selectorish;

use std::fmt;

/// Handle to a node inside a [`Document`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(u32);

impl NodeId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// A single attribute on an element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attribute {
    /// Lower-cased attribute name.
    pub name: String,
    pub value: String,
}

/// Element payload: tag name plus attributes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Element {
    /// Lower-cased tag name.
    pub name: String,
    pub attributes: Vec<Attribute>,
}

impl Element {
    pub fn new(name: impl Into<String>) -> Self {
        Element {
            name: name.into(),
            attributes: Vec::new(),
        }
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.value.as_str())
    }

    pub fn set_attr(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        let value = value.into();
        match self.attributes.iter_mut().find(|a| a.name == name) {
            Some(existing) => existing.value = value,
            None => self.attributes.push(Attribute { name, value }),
        }
    }

    pub fn has_attr(&self, name: &str) -> bool {
        self.attributes.iter().any(|a| a.name == name)
    }

    /// Whitespace-separated `class` tokens.
    pub fn classes(&self) -> impl Iterator<Item = &str> {
        self.attr("class")
            .unwrap_or_default()
            .split_ascii_whitespace()
    }

    pub fn id(&self) -> Option<&str> {
        self.attr("id").map(str::trim).filter(|s| !s.is_empty())
    }
}

/// The payload of a node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeData {
    /// The root of the tree.
    Document,
    Doctype {
        name: String,
    },
    Element(Element),
    Text(String),
    Comment(String),
}

/// A node plus its position in the tree.
#[derive(Clone, Debug)]
pub struct Node {
    pub data: NodeData,
    pub parent: Option<NodeId>,
    pub first_child: Option<NodeId>,
    pub last_child: Option<NodeId>,
    pub prev_sibling: Option<NodeId>,
    pub next_sibling: Option<NodeId>,
}

impl Node {
    fn new(data: NodeData) -> Self {
        Node {
            data,
            parent: None,
            first_child: None,
            last_child: None,
            prev_sibling: None,
            next_sibling: None,
        }
    }

    pub fn as_element(&self) -> Option<&Element> {
        match &self.data {
            NodeData::Element(el) => Some(el),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match &self.data {
            NodeData::Text(t) => Some(t),
            _ => None,
        }
    }

    pub fn is_element(&self) -> bool {
        matches!(self.data, NodeData::Element(_))
    }
}

/// A parsed document.
#[derive(Clone, Debug)]
pub struct Document {
    nodes: Vec<Node>,
    root: NodeId,
    /// Base URL the document was loaded from, if any.
    pub base_url: Option<String>,
    /// Quirks-ish flag: set when no doctype was present.
    pub quirks: bool,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    pub fn new() -> Self {
        let root = Node::new(NodeData::Document);
        Document {
            nodes: vec![root],
            root: NodeId(0),
            base_url: None,
            quirks: false,
        }
    }

    pub fn root(&self) -> NodeId {
        self.root
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }

    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.index()]
    }

    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id.index())
    }

    pub fn data(&self, id: NodeId) -> &NodeData {
        &self.node(id).data
    }

    pub fn element(&self, id: NodeId) -> Option<&Element> {
        self.node(id).as_element()
    }

    pub fn element_mut(&mut self, id: NodeId) -> Option<&mut Element> {
        match &mut self.node_mut(id).data {
            NodeData::Element(el) => Some(el),
            _ => None,
        }
    }

    /// Creates a detached node.
    pub fn create(&mut self, data: NodeData) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(Node::new(data));
        id
    }

    pub fn create_element(&mut self, name: impl Into<String>) -> NodeId {
        self.create(NodeData::Element(Element::new(name)))
    }

    pub fn create_text(&mut self, text: impl Into<String>) -> NodeId {
        self.create(NodeData::Text(text.into()))
    }

    /// Appends `child` as the last child of `parent`.
    ///
    /// Panics if `child` is already attached; detach it first.
    pub fn append(&mut self, parent: NodeId, child: NodeId) {
        debug_assert!(self.node(child).parent.is_none(), "child already attached");
        debug_assert_ne!(parent, child, "cannot append a node to itself");

        let previous_last = self.node(parent).last_child;
        self.node_mut(child).parent = Some(parent);
        self.node_mut(child).prev_sibling = previous_last;
        self.node_mut(child).next_sibling = None;

        match previous_last {
            Some(last) => self.node_mut(last).next_sibling = Some(child),
            None => self.node_mut(parent).first_child = Some(child),
        }
        self.node_mut(parent).last_child = Some(child);
    }

    /// Removes `id` from its parent, keeping its own subtree intact.
    pub fn detach(&mut self, id: NodeId) {
        let (parent, prev, next) = {
            let node = self.node(id);
            (node.parent, node.prev_sibling, node.next_sibling)
        };
        let Some(parent) = parent else { return };

        match prev {
            Some(prev) => self.node_mut(prev).next_sibling = next,
            None => self.node_mut(parent).first_child = next,
        }
        match next {
            Some(next) => self.node_mut(next).prev_sibling = prev,
            None => self.node_mut(parent).last_child = prev,
        }

        let node = self.node_mut(id);
        node.parent = None;
        node.prev_sibling = None;
        node.next_sibling = None;
    }

    /// Iterates the direct children of `id`.
    pub fn children(&self, id: NodeId) -> Children<'_> {
        Children {
            doc: self,
            next: self.node(id).first_child,
        }
    }

    /// Iterates the ancestors of `id`, closest first.
    pub fn ancestors(&self, id: NodeId) -> Ancestors<'_> {
        Ancestors {
            doc: self,
            next: self.node(id).parent,
        }
    }

    /// Depth-first pre-order traversal of the subtree rooted at `id`.
    pub fn descendants(&self, id: NodeId) -> Descendants<'_> {
        Descendants {
            doc: self,
            stack: vec![id],
        }
    }

    /// Element children only.
    pub fn element_children(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.children(id)
            .filter(move |c| self.node(*c).is_element())
    }

    /// Index of `id` among its element siblings, plus the sibling count.
    pub fn element_index(&self, id: NodeId) -> (usize, usize) {
        let Some(parent) = self.node(id).parent else {
            return (0, 1);
        };
        let siblings: Vec<NodeId> = self.element_children(parent).collect();
        let index = siblings.iter().position(|s| *s == id).unwrap_or(0);
        (index, siblings.len())
    }

    /// First element in document order whose tag name matches.
    pub fn find_tag(&self, name: &str) -> Option<NodeId> {
        self.descendants(self.root)
            .find(|id| self.element(*id).is_some_and(|el| el.name == name))
    }

    pub fn body(&self) -> Option<NodeId> {
        self.find_tag("body")
    }

    pub fn title(&self) -> Option<String> {
        let title = self.find_tag("title")?;
        let text = self.text_content(title);
        let trimmed = text.split_whitespace().collect::<Vec<_>>().join(" ");
        (!trimmed.is_empty()).then_some(trimmed)
    }

    /// Concatenated text of the subtree.
    pub fn text_content(&self, id: NodeId) -> String {
        let mut out = String::new();
        for node in self.descendants(id) {
            if let NodeData::Text(t) = &self.node(node).data {
                out.push_str(t);
            }
        }
        out
    }

    /// Resolves the document's effective base URL, honouring `<base href>`.
    pub fn effective_base(&self) -> Option<String> {
        if let Some(base) = self.find_tag("base") {
            if let Some(href) = self.element(base).and_then(|el| el.attr("href")) {
                if !href.trim().is_empty() {
                    return Some(href.trim().to_string());
                }
            }
        }
        self.base_url.clone()
    }
}

pub struct Children<'a> {
    doc: &'a Document,
    next: Option<NodeId>,
}

impl Iterator for Children<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let current = self.next?;
        self.next = self.doc.node(current).next_sibling;
        Some(current)
    }
}

pub struct Ancestors<'a> {
    doc: &'a Document,
    next: Option<NodeId>,
}

impl Iterator for Ancestors<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let current = self.next?;
        self.next = self.doc.node(current).parent;
        Some(current)
    }
}

pub struct Descendants<'a> {
    doc: &'a Document,
    stack: Vec<NodeId>,
}

impl Iterator for Descendants<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let current = self.stack.pop()?;
        // Push children in reverse so the leftmost is visited first.
        let mut children: Vec<NodeId> = self.doc.children(current).collect();
        children.reverse();
        self.stack.extend(children);
        Some(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> (Document, NodeId, NodeId) {
        let mut doc = Document::new();
        let html = doc.create_element("html");
        doc.append(doc.root(), html);
        let body = doc.create_element("body");
        doc.append(html, body);
        let p = doc.create_element("p");
        doc.append(body, p);
        let text = doc.create_text("hello");
        doc.append(p, text);
        (doc, body, p)
    }

    #[test]
    fn tree_links_are_consistent() {
        let (doc, body, p) = sample();
        assert_eq!(doc.node(p).parent, Some(body));
        assert_eq!(doc.children(body).collect::<Vec<_>>(), vec![p]);
        assert_eq!(doc.ancestors(p).count(), 3); // body, html, document
    }

    #[test]
    fn detach_unlinks_from_siblings() {
        let (mut doc, body, p) = sample();
        let second = doc.create_element("span");
        doc.append(body, second);
        doc.detach(p);
        assert_eq!(doc.children(body).collect::<Vec<_>>(), vec![second]);
        assert_eq!(doc.node(second).prev_sibling, None);
        assert_eq!(doc.node(body).first_child, Some(second));
        assert_eq!(doc.node(body).last_child, Some(second));
    }

    #[test]
    fn text_content_walks_subtree() {
        let (doc, body, _) = sample();
        assert_eq!(doc.text_content(body), "hello");
    }

    #[test]
    fn element_index_counts_only_elements() {
        let mut doc = Document::new();
        let ul = doc.create_element("ul");
        doc.append(doc.root(), ul);
        let ws = doc.create_text("\n  ");
        doc.append(ul, ws);
        let a = doc.create_element("li");
        doc.append(ul, a);
        let b = doc.create_element("li");
        doc.append(ul, b);
        assert_eq!(doc.element_index(b), (1, 2));
    }
}
