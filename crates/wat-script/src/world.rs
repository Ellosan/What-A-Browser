//! The state scripts share with the browser.
//!
//! Every DOM handle holds an `Rc<RefCell<World>>`, so the tree a script sees is
//! the same tree the engine lays out. The document itself is swapped in and out
//! around each script run, which keeps the engine's ordinary `&Document`
//! accessors working without wrapping the whole page in a cell.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use wat_css::selector::MatchContext;
use wat_dom::{Document, NodeData, NodeId};
use wat_js::{Scope, Value};

/// A shared handle to the world.
pub type SharedWorld = Rc<RefCell<World>>;

/// A rectangle in viewport coordinates, for `getBoundingClientRect`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// An event listener a script registered.
struct Listener {
    node: NodeId,
    /// The event type, without the `on` prefix.
    kind: String,
    callback: Value,
    /// Registered by assigning to `el.onclick` rather than by
    /// `addEventListener`, so a second assignment replaces it.
    is_property: bool,
    once: bool,
}

/// Where a page's script asked to go.
#[derive(Clone, Debug, PartialEq)]
pub struct Navigation {
    pub url: String,
    /// `location.replace` rather than `location.assign`.
    pub replace: bool,
}

/// A dialog a script opened, for the host to show.
#[derive(Clone, Debug, PartialEq)]
pub struct Dialog {
    pub kind: DialogKind,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogKind {
    Alert,
    Confirm,
    Prompt,
}

/// The state every DOM handle reaches through.
pub struct World {
    /// The live tree. Swapped in by [`crate::ScriptRuntime`] for the duration of
    /// a script run and swapped back out afterwards.
    pub document: Document,
    /// Set whenever a script changed something the engine has to redo work for.
    pub dirty: bool,
    /// Set when a script changed `document.title`.
    pub title_changed: bool,
    /// Layout rectangles, snapshotted before scripts run.
    pub rects: HashMap<NodeId, Rect>,
    /// The document's address, as `location` reports it.
    pub location: String,
    /// A navigation a script asked for.
    pub navigation: Option<Navigation>,
    /// A scroll a script asked for, in CSS pixels.
    pub scroll_to: Option<(f32, f32)>,
    /// The current scroll offset, as `window.scrollY` reports it.
    pub scroll: (f32, f32),
    pub viewport: (f32, f32),
    pub device_pixel_ratio: f32,
    pub user_agent: String,
    /// Dialogs a script opened.
    pub dialogs: Vec<Dialog>,
    /// Events a script asked for with `el.click()`, dispatched once the script
    /// returns.
    pub pending_events: Vec<(NodeId, String)>,

    /// The interpreter's global scope.
    ///
    /// `window` is the global object, so `window.total = 1` has to create the
    /// same binding a bare `total = 1` would, and reading it back has to find
    /// one a script declared with `var`.
    globals: Rc<Scope>,

    listeners: Vec<Listener>,
    /// Identities handed out to handles that are not tied to a node.
    next_identity: usize,
}

impl World {
    pub fn new(document: Document, location: impl Into<String>) -> SharedWorld {
        Rc::new(RefCell::new(World {
            document,
            dirty: false,
            title_changed: false,
            rects: HashMap::new(),
            location: location.into(),
            navigation: None,
            scroll_to: None,
            scroll: (0.0, 0.0),
            viewport: (0.0, 0.0),
            device_pixel_ratio: 1.0,
            user_agent: crate::USER_AGENT.to_string(),
            dialogs: Vec::new(),
            pending_events: Vec::new(),
            globals: Scope::root(),
            listeners: Vec::new(),
            next_identity: 1,
        }))
    }

    /// Points `window` at the interpreter's global scope.
    pub fn attach_globals(&mut self, globals: Rc<Scope>) {
        self.globals = globals;
    }

    /// A global binding, for `window.something`.
    pub fn global(&self, name: &str) -> Option<Value> {
        self.globals.lookup(name)
    }

    /// Creates or replaces a global binding, for `window.something = …`.
    pub fn set_global(&mut self, name: &str, value: Value) {
        self.globals.declare(name, value, true);
    }

    /// Marks the page as needing a restyle and a relayout.
    pub fn touch(&mut self) {
        self.dirty = true;
    }

    /// A fresh identity for a handle with no node of its own, so `===` still
    /// distinguishes two different objects.
    pub fn fresh_identity(&mut self) -> usize {
        self.next_identity += 1;
        self.next_identity
    }

    // ---- attributes -------------------------------------------------------

    pub fn attr(&self, node: NodeId, name: &str) -> Option<String> {
        self.document
            .element(node)
            .and_then(|element| element.attr(name))
            .map(str::to_string)
    }

    pub fn set_attr(&mut self, node: NodeId, name: &str, value: &str) {
        let name = name.to_ascii_lowercase();
        if let Some(element) = self.document.element_mut(node) {
            element.set_attr(name, value);
            self.touch();
        }
    }

    pub fn remove_attr(&mut self, node: NodeId, name: &str) {
        let name = name.to_ascii_lowercase();
        if let Some(element) = self.document.element_mut(node) {
            element.attributes.retain(|attr| attr.name != name);
            self.touch();
        }
    }

    /// Sets or removes an attribute that is present or absent rather than
    /// valued, such as `disabled` and `checked`.
    pub fn set_boolean_attr(&mut self, node: NodeId, name: &str, present: bool) {
        if present {
            self.set_attr(node, name, "");
        } else {
            self.remove_attr(node, name);
        }
    }

    // ---- tree edits -------------------------------------------------------

    /// Appends `child` to `parent`, moving it if it is attached elsewhere.
    ///
    /// Refuses to build a cycle, which the DOM reports as a hierarchy error and
    /// which would otherwise hang every tree walk in the engine.
    pub fn attach_append(&mut self, parent: NodeId, child: NodeId) -> Result<(), String> {
        self.check_hierarchy(parent, child)?;
        self.document.detach(child);
        self.document.append(parent, child);
        self.touch();
        Ok(())
    }

    /// Inserts `child` before `reference`, moving it if it is attached elsewhere.
    pub fn attach_before(&mut self, reference: NodeId, child: NodeId) -> Result<(), String> {
        let Some(parent) = self.document.node(reference).parent else {
            return Err("the reference node has no parent".to_string());
        };
        self.check_hierarchy(parent, child)?;
        if reference == child {
            return Ok(());
        }
        self.document.detach(child);
        self.document.insert_before(reference, child);
        self.touch();
        Ok(())
    }

    fn check_hierarchy(&self, parent: NodeId, child: NodeId) -> Result<(), String> {
        if parent == child || self.document.ancestors(parent).any(|node| node == child) {
            return Err("cannot insert a node into itself or its own descendant".to_string());
        }
        Ok(())
    }

    /// Replaces a node's children with a single text node.
    pub fn set_text_content(&mut self, node: NodeId, text: &str) {
        let children: Vec<NodeId> = self.document.children(node).collect();
        for child in children {
            self.document.detach(child);
        }
        if !text.is_empty() {
            let text = self.document.create_text(text);
            self.document.append(node, text);
        }
        self.touch();
    }

    /// Replaces a node's children with the result of parsing `html`.
    pub fn set_inner_html(&mut self, node: NodeId, html: &str) {
        let children: Vec<NodeId> = self.document.children(node).collect();
        for child in children {
            self.document.detach(child);
        }
        for imported in self.parse_fragment(html) {
            self.document.append(node, imported);
        }
        self.touch();
    }

    /// Parses `html` and copies the result into this document, returning the
    /// top-level nodes.
    ///
    /// The fragment is parsed as a whole document and its body's children are
    /// taken, which is what a real engine's fragment parsing amounts to for
    /// everything a page puts inside an element.
    pub fn parse_fragment(&mut self, html: &str) -> Vec<NodeId> {
        let parsed = wat_html::parse(html);
        let Some(body) = parsed.body() else {
            return Vec::new();
        };
        parsed
            .children(body)
            .collect::<Vec<_>>()
            .into_iter()
            .map(|child| import(&mut self.document, &parsed, child))
            .collect()
    }

    /// Copies a subtree, as `cloneNode` does.
    pub fn clone_node(&mut self, node: NodeId, deep: bool) -> NodeId {
        let data = self.document.data(node).clone();
        let copy = self.document.create(data);
        if deep {
            for child in self.document.children(node).collect::<Vec<_>>() {
                let child_copy = self.clone_node(child, true);
                self.document.append(copy, child_copy);
            }
        }
        copy
    }

    // ---- selectors --------------------------------------------------------

    /// The descendants of `root` matching `selector`, in document order.
    ///
    /// This is the real CSS selector engine, the same one the cascade uses, so
    /// `querySelectorAll` accepts everything a stylesheet can.
    pub fn query_all(&self, root: NodeId, selector: &str) -> Vec<NodeId> {
        let selectors = wat_css::selector::parse_selector_list(selector);
        if selectors.is_empty() {
            return Vec::new();
        }
        let context = MatchContext::default();
        self.document
            .descendants(root)
            .filter(|node| *node != root && self.document.node(*node).is_element())
            .filter(|node| {
                selectors
                    .iter()
                    .any(|candidate| candidate.matches(&self.document, *node, &context))
            })
            .collect()
    }

    pub fn query(&self, root: NodeId, selector: &str) -> Option<NodeId> {
        self.query_all(root, selector).into_iter().next()
    }

    /// Does `node` itself match `selector`?
    pub fn matches_selector(&self, node: NodeId, selector: &str) -> bool {
        let context = MatchContext::default();
        wat_css::selector::parse_selector_list(selector)
            .iter()
            .any(|candidate| candidate.matches(&self.document, node, &context))
    }

    /// `node` or its nearest ancestor matching `selector`.
    pub fn closest(&self, node: NodeId, selector: &str) -> Option<NodeId> {
        std::iter::once(node)
            .chain(self.document.ancestors(node))
            .find(|candidate| {
                self.document.node(*candidate).is_element()
                    && self.matches_selector(*candidate, selector)
            })
    }

    /// Elements under `root` with the given tag name; `*` matches every element.
    pub fn elements_by_tag(&self, root: NodeId, name: &str) -> Vec<NodeId> {
        let wanted = name.to_ascii_lowercase();
        self.document
            .descendants(root)
            .filter(|node| *node != root)
            .filter(|node| match self.document.element(*node) {
                Some(element) => wanted == "*" || element.name == wanted,
                None => false,
            })
            .collect()
    }

    /// Elements under `root` carrying every one of the given classes.
    pub fn elements_by_class(&self, root: NodeId, classes: &str) -> Vec<NodeId> {
        let wanted: Vec<&str> = classes.split_ascii_whitespace().collect();
        if wanted.is_empty() {
            return Vec::new();
        }
        self.document
            .descendants(root)
            .filter(|node| *node != root)
            .filter(|node| match self.document.element(*node) {
                Some(element) => wanted
                    .iter()
                    .all(|class| element.classes().any(|present| present == *class)),
                None => false,
            })
            .collect()
    }

    // ---- listeners --------------------------------------------------------

    pub fn add_listener(&mut self, node: NodeId, kind: &str, callback: Value, once: bool) {
        self.listeners.push(Listener {
            node,
            kind: normalise_event(kind),
            callback,
            is_property: false,
            once,
        });
    }

    pub fn remove_listener(&mut self, node: NodeId, kind: &str, callback: &Value) {
        let kind = normalise_event(kind);
        self.listeners.retain(|listener| {
            !(listener.node == node
                && listener.kind == kind
                && !listener.is_property
                && listener.callback.strict_equals(callback))
        });
    }

    /// Handles `el.onclick = fn`, which replaces any previous assignment.
    pub fn set_property_listener(&mut self, node: NodeId, kind: &str, callback: Option<Value>) {
        let kind = normalise_event(kind);
        self.listeners.retain(|listener| {
            !(listener.node == node && listener.kind == kind && listener.is_property)
        });
        if let Some(callback) = callback {
            self.listeners.push(Listener {
                node,
                kind,
                callback,
                is_property: true,
                once: false,
            });
        }
    }

    pub fn property_listener(&self, node: NodeId, kind: &str) -> Option<Value> {
        let kind = normalise_event(kind);
        self.listeners
            .iter()
            .find(|listener| listener.node == node && listener.kind == kind && listener.is_property)
            .map(|listener| listener.callback.clone())
    }

    /// The callbacks registered for an event on one node, in registration
    /// order, dropping any that were registered with `once`.
    pub fn take_listeners_for(&mut self, node: NodeId, kind: &str) -> Vec<Value> {
        let kind = normalise_event(kind);
        let callbacks: Vec<Value> = self
            .listeners
            .iter()
            .filter(|listener| listener.node == node && listener.kind == kind)
            .map(|listener| listener.callback.clone())
            .collect();
        self.listeners
            .retain(|listener| !(listener.node == node && listener.kind == kind && listener.once));
        callbacks
    }

    /// Whether a node has a listener registered by assignment, which makes the
    /// matching `on…` attribute redundant.
    pub fn has_property_listener(&self, node: NodeId, kind: &str) -> bool {
        self.property_listener(node, kind).is_some()
    }

    pub fn listener_count(&self) -> usize {
        self.listeners.len()
    }

    /// Drops every listener, which is what breaks the reference cycle between
    /// the world and the closures it holds.
    pub fn clear_listeners(&mut self) {
        self.listeners.clear();
    }

    /// Lets go of the global scope, breaking the other cycle: the scope holds
    /// `document`, and `document` holds the world.
    pub fn detach_globals(&mut self) {
        self.globals = Scope::root();
    }
}

/// Strips an `on` prefix and lower-cases, so `onClick`, `onclick` and `click`
/// all name the same event.
pub fn normalise_event(kind: &str) -> String {
    let lower = kind.to_ascii_lowercase();
    match lower.strip_prefix("on") {
        Some(rest) if !rest.is_empty() => rest.to_string(),
        _ => lower,
    }
}

/// Copies `node` and its subtree from `src` into `dest`, returning the copy.
fn import(dest: &mut Document, src: &Document, node: NodeId) -> NodeId {
    let data = match src.data(node) {
        // A nested document node cannot be imported, so it becomes a wrapper
        // that the caller's append flattens away.
        NodeData::Document => NodeData::Element(wat_dom::Element::new("span")),
        other => other.clone(),
    };
    let copy = dest.create(data);
    for child in src.children(node) {
        let child_copy = import(dest, src, child);
        dest.append(copy, child_copy);
    }
    copy
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world(html: &str) -> SharedWorld {
        World::new(wat_html::parse(html), "about:blank")
    }

    #[test]
    fn attributes_round_trip_and_mark_the_page_dirty() {
        let world = world("<p id='a'>text</p>");
        let mut world = world.borrow_mut();
        let node = world.document.query("#a").unwrap();
        assert_eq!(world.attr(node, "id").as_deref(), Some("a"));
        assert!(!world.dirty);

        world.set_attr(node, "class", "big");
        assert!(world.dirty, "a write must ask the engine to redo its work");
        assert_eq!(world.attr(node, "class").as_deref(), Some("big"));

        world.remove_attr(node, "class");
        assert!(world.attr(node, "class").is_none());
    }

    #[test]
    fn boolean_attributes_are_present_or_absent() {
        let world = world("<input>");
        let mut world = world.borrow_mut();
        let node = world.document.query("input").unwrap();
        world.set_boolean_attr(node, "disabled", true);
        assert_eq!(world.attr(node, "disabled").as_deref(), Some(""));
        world.set_boolean_attr(node, "disabled", false);
        assert!(world.attr(node, "disabled").is_none());
    }

    #[test]
    fn setting_text_content_replaces_the_children() {
        let world = world("<div><b>old</b><i>more</i></div>");
        let mut world = world.borrow_mut();
        let node = world.document.query("div").unwrap();
        world.set_text_content(node, "new");
        assert_eq!(world.document.text_content(node), "new");
        assert_eq!(world.document.children(node).count(), 1);
    }

    #[test]
    fn setting_empty_text_content_leaves_no_children() {
        let world = world("<div>old</div>");
        let mut world = world.borrow_mut();
        let node = world.document.query("div").unwrap();
        world.set_text_content(node, "");
        assert_eq!(world.document.children(node).count(), 0);
    }

    #[test]
    fn inner_html_parses_into_the_live_document() {
        let world = world("<div></div>");
        let mut world = world.borrow_mut();
        let node = world.document.query("div").unwrap();
        world.set_inner_html(node, "<span class='x'>hi</span><b>b</b>");
        assert_eq!(world.document.children(node).count(), 2);
        assert_eq!(world.document.text_content(node), "hib");
        // The imported nodes are real nodes in this document, so selectors and
        // the cascade see them.
        let span = world.query(world.document.root(), ".x").unwrap();
        assert_eq!(world.document.element(span).unwrap().name, "span");
    }

    #[test]
    fn a_deep_clone_copies_the_subtree() {
        let world = world("<ul><li>a</li><li>b</li></ul>");
        let mut world = world.borrow_mut();
        let list = world.document.query("ul").unwrap();
        let shallow = world.clone_node(list, false);
        assert_eq!(world.document.children(shallow).count(), 0);
        let deep = world.clone_node(list, true);
        assert_eq!(world.document.children(deep).count(), 2);
        assert_eq!(world.document.text_content(deep), "ab");
    }

    #[test]
    fn selectors_use_the_real_css_engine() {
        let world =
            world("<main><p class='a'>one</p><section><p class='a b'>two</p></section></main>");
        let world = world.borrow();
        let root = world.document.root();
        assert_eq!(world.query_all(root, "p").len(), 2);
        assert_eq!(world.query_all(root, "p.a.b").len(), 1);
        assert_eq!(world.query_all(root, "section > p").len(), 1);
        assert_eq!(world.query_all(root, "main p").len(), 2);
        assert_eq!(world.query_all(root, "p:first-child").len(), 2);
        assert_eq!(world.query_all(root, "nope").len(), 0);

        let inner = world.query(root, "section p").unwrap();
        assert!(world.matches_selector(inner, "p.a"));
        assert!(!world.matches_selector(inner, "p.c"));
        assert_eq!(
            world.closest(inner, "section"),
            world.query(root, "section")
        );
        assert_eq!(
            world.closest(inner, "p"),
            Some(inner),
            "closest includes self"
        );
    }

    #[test]
    fn a_query_is_scoped_to_its_root_and_excludes_it() {
        let world = world("<div id='a'><p>in</p></div><p>out</p>");
        let world = world.borrow();
        let scope = world.document.query("#a").unwrap();
        assert_eq!(world.query_all(scope, "p").len(), 1);
        assert!(
            world.query_all(scope, "#a").is_empty(),
            "the root itself is not a descendant"
        );
    }

    #[test]
    fn lookups_by_tag_and_class() {
        let world = world("<div><b class='x y'>1</b><i class='x'>2</i></div>");
        let world = world.borrow();
        let root = world.document.root();
        assert_eq!(world.elements_by_tag(root, "b").len(), 1);
        assert_eq!(
            world.elements_by_tag(root, "B").len(),
            1,
            "tag names fold case"
        );
        assert_eq!(world.elements_by_class(root, "x").len(), 2);
        assert_eq!(
            world.elements_by_class(root, "x y").len(),
            1,
            "every class must match"
        );
        assert!(world.elements_by_class(root, "").is_empty());
        // `*` is every element, which is what getElementsByTagName('*') means.
        assert!(world.elements_by_tag(root, "*").len() >= 4);
    }

    #[test]
    fn event_names_are_normalised() {
        assert_eq!(normalise_event("onclick"), "click");
        assert_eq!(normalise_event("onClick"), "click");
        assert_eq!(normalise_event("click"), "click");
        assert_eq!(normalise_event("CLICK"), "click");
        // A bare `on` is an event named "on", not an empty name.
        assert_eq!(normalise_event("on"), "on");
    }

    #[test]
    fn listeners_are_kept_per_node_and_type() {
        let world = world("<button></button>");
        let mut world = world.borrow_mut();
        let node = world.document.query("button").unwrap();
        let other = world.document.query("body").unwrap();

        world.add_listener(node, "click", Value::Number(1.0), false);
        world.add_listener(node, "click", Value::Number(2.0), false);
        world.add_listener(node, "input", Value::Number(3.0), false);
        world.add_listener(other, "click", Value::Number(4.0), false);

        let clicks = world.take_listeners_for(node, "click");
        assert_eq!(clicks.len(), 2);
        assert_eq!(clicks[0].to_number(), 1.0, "registration order is kept");
        assert_eq!(
            world.take_listeners_for(node, "onclick").len(),
            2,
            "on-prefix is the same event"
        );
        assert_eq!(world.take_listeners_for(other, "click").len(), 1);
    }

    #[test]
    fn a_once_listener_runs_only_once() {
        let world = world("<button></button>");
        let mut world = world.borrow_mut();
        let node = world.document.query("button").unwrap();
        world.add_listener(node, "click", Value::Number(1.0), true);
        assert_eq!(world.take_listeners_for(node, "click").len(), 1);
        assert_eq!(world.take_listeners_for(node, "click").len(), 0);
    }

    #[test]
    fn removing_a_listener_needs_the_same_function() {
        let world = world("<button></button>");
        let mut world = world.borrow_mut();
        let node = world.document.query("button").unwrap();
        let callback = Value::string("same");
        world.add_listener(node, "click", callback.clone(), false);
        world.remove_listener(node, "click", &Value::string("other"));
        assert_eq!(
            world.listener_count(),
            1,
            "a different function removes nothing"
        );
        world.remove_listener(node, "click", &callback);
        assert_eq!(world.listener_count(), 0);
    }

    #[test]
    fn assigning_a_handler_replaces_the_previous_one() {
        let world = world("<button></button>");
        let mut world = world.borrow_mut();
        let node = world.document.query("button").unwrap();
        world.set_property_listener(node, "onclick", Some(Value::Number(1.0)));
        world.set_property_listener(node, "onclick", Some(Value::Number(2.0)));
        assert_eq!(world.listener_count(), 1);
        assert_eq!(
            world.property_listener(node, "click").unwrap().to_number(),
            2.0
        );
        assert!(world.has_property_listener(node, "click"));

        world.set_property_listener(node, "onclick", None);
        assert_eq!(world.listener_count(), 0);
        assert!(!world.has_property_listener(node, "click"));
    }

    #[test]
    fn an_assigned_handler_and_an_added_one_coexist() {
        let world = world("<button></button>");
        let mut world = world.borrow_mut();
        let node = world.document.query("button").unwrap();
        world.set_property_listener(node, "onclick", Some(Value::Number(1.0)));
        world.add_listener(node, "click", Value::Number(2.0), false);
        assert_eq!(world.take_listeners_for(node, "click").len(), 2);
        // Removing by function must not touch the assigned one.
        world.remove_listener(node, "click", &Value::Number(1.0));
        assert_eq!(world.listener_count(), 2);
    }

    #[test]
    fn identities_are_unique() {
        let world = world("<p></p>");
        let mut world = world.borrow_mut();
        let first = world.fresh_identity();
        assert_ne!(first, world.fresh_identity());
    }
}
