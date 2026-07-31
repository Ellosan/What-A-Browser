//! Element and node bindings: `Node`, `Element`, `classList` and `style`.
//!
//! Collections are returned as real JavaScript arrays rather than live
//! `NodeList` objects. That is a deliberate simplification: it costs liveness,
//! and in exchange `forEach`, `map`, spread and indexing all work through the
//! ordinary array built-ins instead of needing a bespoke host type for each.

use std::rc::Rc;

use wat_dom::{NodeData, NodeId};
use wat_js::value::HostObject;
use wat_js::{JsObject, Value};

use crate::style;
use crate::world::SharedWorld;

/// Identity spaces, so `===` tells two kinds of handle apart while two handles
/// to the same node compare equal.
pub(crate) const NODE_SPACE: usize = 1 << 28;
pub(crate) const CLASS_LIST_SPACE: usize = 2 << 28;
pub(crate) const STYLE_SPACE: usize = 3 << 28;
pub(crate) const DOCUMENT_SPACE: usize = 4 << 28;
pub(crate) const WINDOW_SPACE: usize = 5 << 28;
pub(crate) const LOCATION_SPACE: usize = 6 << 28;
pub(crate) const EVENT_SPACE: usize = 7 << 28;

/// Attributes an element reflects as a property, so `el.href` works as well as
/// `el.getAttribute('href')`.
const REFLECTED: &[&str] = &[
    "href",
    "src",
    "alt",
    "title",
    "type",
    "name",
    "placeholder",
    "target",
    "rel",
    "action",
    "method",
    "lang",
    "role",
];

/// Attributes that are present or absent rather than valued.
const BOOLEAN: &[&str] = &[
    "disabled", "checked", "hidden", "readonly", "required", "selected", "multiple", "open",
];

/// A handle on one DOM node.
pub struct NodeHandle {
    pub(crate) world: SharedWorld,
    pub(crate) node: NodeId,
}

impl NodeHandle {
    /// Binds a node as a script value.
    pub fn bind(world: &SharedWorld, node: NodeId) -> Value {
        Value::Host(Rc::new(NodeHandle {
            world: world.clone(),
            node,
        }))
    }

    /// Wraps an optional node, so a missing relative reads as `null` the way the
    /// DOM reports it.
    fn maybe(&self, node: Option<NodeId>) -> Value {
        match node {
            Some(node) => NodeHandle::bind(&self.world, node),
            None => Value::Null,
        }
    }

    fn list(&self, nodes: Vec<NodeId>) -> Value {
        Value::array(
            nodes
                .into_iter()
                .map(|node| NodeHandle::bind(&self.world, node))
                .collect(),
        )
    }

    /// The node named by an argument, if it is a handle on this document.
    fn argument_node(&self, value: Option<&Value>) -> Option<NodeId> {
        node_of(value?)
    }

    /// Whether this handle still points at a node in the live tree.
    ///
    /// A handle can outlive the document it came from, because the runtime swaps
    /// the tree out between runs. Every entry point checks first and behaves
    /// like a node that has been detached rather than panicking.
    fn is_live(&self) -> bool {
        self.world.borrow().document.get(self.node).is_some()
    }
}

/// The node a value refers to, if it is a node handle.
pub(crate) fn node_of(value: &Value) -> Option<NodeId> {
    match value {
        Value::Host(host) => Some(host.as_any()?.downcast_ref::<NodeHandle>()?.node),
        _ => None,
    }
}

impl HostObject for NodeHandle {
    fn type_name(&self) -> String {
        if !self.is_live() {
            return "Node".to_string();
        }
        let world = self.world.borrow();
        match world.document.data(self.node) {
            NodeData::Element(element) => format!("HTML{}Element", element.name),
            NodeData::Text(_) => "Text".to_string(),
            NodeData::Comment(_) => "Comment".to_string(),
            NodeData::Doctype { .. } => "DocumentType".to_string(),
            NodeData::Document => "Document".to_string(),
        }
    }

    fn describe(&self) -> String {
        if !self.is_live() {
            return "<detached>".to_string();
        }
        let world = self.world.borrow();
        match world.document.data(self.node) {
            NodeData::Element(element) => {
                let mut out = format!("<{}", element.name);
                if let Some(id) = element.id() {
                    out.push_str(&format!(" id=\"{id}\""));
                }
                if let Some(class) = element.attr("class") {
                    out.push_str(&format!(" class=\"{class}\""));
                }
                out.push('>');
                out
            }
            NodeData::Text(text) => format!("#text {text:?}"),
            NodeData::Comment(_) => "#comment".to_string(),
            other => format!("{other:?}"),
        }
    }

    fn identity(&self) -> usize {
        NODE_SPACE + self.node.index()
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn get(&self, key: &str) -> Option<Value> {
        if !self.is_live() {
            return None;
        }
        let world = self.world.borrow();
        let document = &world.document;
        let element = document.element(self.node);

        // `el.onclick` reads back the handler that was assigned to it.
        if let Some(kind) = key.strip_prefix("on") {
            if !kind.is_empty() {
                return Some(
                    world
                        .property_listener(self.node, kind)
                        .unwrap_or(Value::Null),
                );
            }
        }

        let value = match key {
            "nodeType" => Value::Number(match document.data(self.node) {
                NodeData::Element(_) => 1.0,
                NodeData::Text(_) => 3.0,
                NodeData::Comment(_) => 8.0,
                NodeData::Doctype { .. } => 10.0,
                NodeData::Document => 9.0,
            }),
            "nodeName" | "tagName" => match element {
                Some(element) => Value::string(element.name.to_ascii_uppercase()),
                None => match document.data(self.node) {
                    NodeData::Text(_) => Value::string("#text"),
                    NodeData::Comment(_) => Value::string("#comment"),
                    _ => Value::string("#document"),
                },
            },
            "localName" => Value::string(element.map(|el| el.name.clone()).unwrap_or_default()),
            "id" => Value::string(
                element
                    .and_then(|el| el.attr("id"))
                    .unwrap_or_default()
                    .to_string(),
            ),
            "className" => Value::string(
                element
                    .and_then(|el| el.attr("class"))
                    .unwrap_or_default()
                    .to_string(),
            ),
            "classList" => Value::Host(Rc::new(ClassList {
                world: self.world.clone(),
                node: self.node,
            })),
            "style" => Value::Host(Rc::new(StyleDeclaration {
                world: self.world.clone(),
                node: self.node,
            })),
            "textContent" | "innerText" => Value::string(document.text_content(self.node)),
            "innerHTML" => {
                let mut out = String::new();
                for child in document.children(self.node) {
                    out.push_str(&document.to_html(child));
                }
                Value::string(out)
            }
            "outerHTML" => Value::string(document.to_html(self.node)),
            "nodeValue" | "data" => match document.data(self.node) {
                NodeData::Text(text) => Value::string(text.clone()),
                NodeData::Comment(text) => Value::string(text.clone()),
                _ => Value::Null,
            },
            "parentNode" | "parentElement" => {
                let parent = document
                    .node(self.node)
                    .parent
                    .filter(|parent| key == "parentNode" || document.node(*parent).is_element());
                self.maybe(parent)
            }
            "ownerDocument" => crate::globals::DocumentHandle::bind(&self.world),
            "children" => self.list(document.element_children(self.node).collect()),
            "childNodes" => self.list(document.children(self.node).collect()),
            "childElementCount" => {
                Value::Number(document.element_children(self.node).count() as f64)
            }
            "firstChild" => self.maybe(document.node(self.node).first_child),
            "lastChild" => self.maybe(document.node(self.node).last_child),
            "firstElementChild" => self.maybe(document.element_children(self.node).next()),
            "lastElementChild" => self.maybe(document.element_children(self.node).last()),
            "nextSibling" => self.maybe(document.node(self.node).next_sibling),
            "previousSibling" => self.maybe(document.node(self.node).prev_sibling),
            "nextElementSibling" => {
                let mut current = document.node(self.node).next_sibling;
                while let Some(node) = current {
                    if document.node(node).is_element() {
                        break;
                    }
                    current = document.node(node).next_sibling;
                }
                self.maybe(current)
            }
            "previousElementSibling" => {
                let mut current = document.node(self.node).prev_sibling;
                while let Some(node) = current {
                    if document.node(node).is_element() {
                        break;
                    }
                    current = document.node(node).prev_sibling;
                }
                self.maybe(current)
            }
            "value" => match element {
                // A textarea keeps its value as its content, not an attribute.
                Some(element) if element.name == "textarea" => {
                    Value::string(document.text_content(self.node))
                }
                Some(element) => {
                    Value::string(element.attr("value").unwrap_or_default().to_string())
                }
                None => Value::Undefined,
            },
            "offsetWidth" | "clientWidth" => {
                Value::Number(world.rects.get(&self.node).map(|r| r.width).unwrap_or(0.0) as f64)
            }
            "offsetHeight" | "clientHeight" => {
                Value::Number(world.rects.get(&self.node).map(|r| r.height).unwrap_or(0.0) as f64)
            }
            "offsetLeft" => {
                Value::Number(world.rects.get(&self.node).map(|r| r.x).unwrap_or(0.0) as f64)
            }
            "offsetTop" => {
                Value::Number(world.rects.get(&self.node).map(|r| r.y).unwrap_or(0.0) as f64)
            }
            name if BOOLEAN.contains(&name) => {
                Value::Bool(element.map(|el| el.has_attr(name)).unwrap_or(false))
            }
            name if REFLECTED.contains(&name) => Value::string(
                element
                    .and_then(|el| el.attr(name))
                    .unwrap_or_default()
                    .to_string(),
            ),
            _ => return None,
        };
        Some(value)
    }

    fn set(&self, key: &str, value: &Value) -> bool {
        if !self.is_live() {
            return false;
        }
        let mut world = self.world.borrow_mut();

        // `el.onclick = fn` registers a handler; assigning anything else clears
        // it, which is how a page removes one.
        if let Some(kind) = key.strip_prefix("on") {
            if !kind.is_empty() {
                let callback = if value.is_callable() {
                    Some(value.clone())
                } else {
                    None
                };
                world.set_property_listener(self.node, kind, callback);
                return true;
            }
        }

        match key {
            "id" => world.set_attr(self.node, "id", &value.to_js_string()),
            "className" => world.set_attr(self.node, "class", &value.to_js_string()),
            "textContent" | "innerText" => world.set_text_content(self.node, &value.to_js_string()),
            "innerHTML" => world.set_inner_html(self.node, &value.to_js_string()),
            "nodeValue" | "data" => {
                let text = value.to_js_string();
                match world.document.node_mut(self.node).data {
                    NodeData::Text(ref mut existing) => *existing = text,
                    NodeData::Comment(ref mut existing) => *existing = text,
                    _ => return false,
                }
                world.touch();
            }
            "value" => {
                let is_textarea = world
                    .document
                    .element(self.node)
                    .is_some_and(|element| element.name == "textarea");
                if is_textarea {
                    world.set_text_content(self.node, &value.to_js_string());
                } else {
                    world.set_attr(self.node, "value", &value.to_js_string());
                }
            }
            "style" => {
                // `el.style = 'color: red'` replaces the whole inline style.
                world.set_attr(self.node, "style", &value.to_js_string());
            }
            name if BOOLEAN.contains(&name) => {
                world.set_boolean_attr(self.node, name, value.truthy())
            }
            name if REFLECTED.contains(&name) => {
                world.set_attr(self.node, name, &value.to_js_string())
            }
            _ => return false,
        }
        true
    }

    fn invoke(&self, method: &str, args: &[Value]) -> Result<Value, String> {
        if !self.is_live() {
            return Err(format!("`{method}` was called on a node that is gone"));
        }
        let first = args.first();
        match method {
            "getAttribute" => {
                let name = string_arg(first).to_ascii_lowercase();
                let world = self.world.borrow();
                Ok(match world.attr(self.node, &name) {
                    Some(value) => Value::string(value),
                    None => Value::Null,
                })
            }
            "setAttribute" => {
                let name = string_arg(first);
                let value = string_arg(args.get(1));
                self.world.borrow_mut().set_attr(self.node, &name, &value);
                Ok(Value::Undefined)
            }
            "removeAttribute" => {
                let name = string_arg(first);
                self.world.borrow_mut().remove_attr(self.node, &name);
                Ok(Value::Undefined)
            }
            "hasAttribute" => {
                let name = string_arg(first).to_ascii_lowercase();
                let world = self.world.borrow();
                Ok(Value::Bool(world.attr(self.node, &name).is_some()))
            }
            "toggleAttribute" => {
                let name = string_arg(first).to_ascii_lowercase();
                let mut world = self.world.borrow_mut();
                let present = world.attr(self.node, &name).is_some();
                let wanted = match args.get(1) {
                    Some(force) => force.truthy(),
                    None => !present,
                };
                world.set_boolean_attr(self.node, &name, wanted);
                Ok(Value::Bool(wanted))
            }
            "getAttributeNames" => {
                let world = self.world.borrow();
                let names = world
                    .document
                    .element(self.node)
                    .map(|element| {
                        element
                            .attributes
                            .iter()
                            .map(|attr| Value::string(attr.name.clone()))
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(Value::array(names))
            }
            "appendChild" | "append" => {
                let mut world = self.world.borrow_mut();
                let mut last = Value::Undefined;
                for argument in args {
                    match node_of(argument) {
                        Some(child) => {
                            world.attach_append(self.node, child)?;
                            last = argument.clone();
                        }
                        // Appending a string appends a text node, as `append`
                        // does.
                        None => {
                            let text = world.document.create_text(argument.to_js_string());
                            world.attach_append(self.node, text)?;
                        }
                    }
                }
                Ok(last)
            }
            "removeChild" => {
                let Some(child) = self.argument_node(first) else {
                    return Err("removeChild expects a node".to_string());
                };
                let mut world = self.world.borrow_mut();
                if world.document.node(child).parent != Some(self.node) {
                    return Err("the node to remove is not a child of this node".to_string());
                }
                world.document.detach(child);
                world.touch();
                Ok(first.cloned().unwrap_or(Value::Undefined))
            }
            "remove" => {
                let mut world = self.world.borrow_mut();
                world.document.detach(self.node);
                world.touch();
                Ok(Value::Undefined)
            }
            "insertBefore" => {
                let Some(child) = self.argument_node(first) else {
                    return Err("insertBefore expects a node".to_string());
                };
                let reference = self.argument_node(args.get(1));
                let mut world = self.world.borrow_mut();
                match reference {
                    Some(reference) => world.attach_before(reference, child)?,
                    // A null reference appends, as the DOM specifies.
                    None => world.attach_append(self.node, child)?,
                }
                Ok(first.cloned().unwrap_or(Value::Undefined))
            }
            "replaceChild" => {
                let Some(fresh) = self.argument_node(first) else {
                    return Err("replaceChild expects a node".to_string());
                };
                let Some(stale) = self.argument_node(args.get(1)) else {
                    return Err("replaceChild expects a node to replace".to_string());
                };
                let mut world = self.world.borrow_mut();
                world.attach_before(stale, fresh)?;
                world.document.detach(stale);
                world.touch();
                Ok(args.get(1).cloned().unwrap_or(Value::Undefined))
            }
            "prepend" => {
                let mut world = self.world.borrow_mut();
                let reference = world.document.node(self.node).first_child;
                for argument in args {
                    let child = match node_of(argument) {
                        Some(child) => child,
                        None => world.document.create_text(argument.to_js_string()),
                    };
                    match reference {
                        Some(reference) => world.attach_before(reference, child)?,
                        None => world.attach_append(self.node, child)?,
                    }
                }
                Ok(Value::Undefined)
            }
            "cloneNode" => {
                let deep = first.map(Value::truthy).unwrap_or(false);
                let copy = self.world.borrow_mut().clone_node(self.node, deep);
                Ok(NodeHandle::bind(&self.world, copy))
            }
            "contains" => {
                let Some(other) = self.argument_node(first) else {
                    return Ok(Value::Bool(false));
                };
                let world = self.world.borrow();
                Ok(Value::Bool(
                    other == self.node
                        || world
                            .document
                            .ancestors(other)
                            .any(|node| node == self.node),
                ))
            }
            "querySelector" => {
                let selector = string_arg(first);
                let found = self.world.borrow().query(self.node, &selector);
                Ok(match found {
                    Some(node) => NodeHandle::bind(&self.world, node),
                    None => Value::Null,
                })
            }
            "querySelectorAll" => {
                let selector = string_arg(first);
                let found = self.world.borrow().query_all(self.node, &selector);
                Ok(self.list(found))
            }
            "getElementsByTagName" => {
                let name = string_arg(first);
                let found = self.world.borrow().elements_by_tag(self.node, &name);
                Ok(self.list(found))
            }
            "getElementsByClassName" => {
                let classes = string_arg(first);
                let found = self.world.borrow().elements_by_class(self.node, &classes);
                Ok(self.list(found))
            }
            "matches" => {
                let selector = string_arg(first);
                let matched = self.world.borrow().matches_selector(self.node, &selector);
                Ok(Value::Bool(matched))
            }
            "closest" => {
                let selector = string_arg(first);
                let found = self.world.borrow().closest(self.node, &selector);
                Ok(match found {
                    Some(node) => NodeHandle::bind(&self.world, node),
                    None => Value::Null,
                })
            }
            "getBoundingClientRect" => {
                let rect = self.world.borrow().rects.get(&self.node).copied();
                Ok(rect_object(rect.unwrap_or_default()))
            }
            "addEventListener" => {
                let kind = string_arg(first);
                let callback = args.get(1).cloned().unwrap_or(Value::Undefined);
                if !callback.is_callable() {
                    return Err("addEventListener expects a function".to_string());
                }
                let once = matches!(args.get(2), Some(Value::Object(options)) if options.get("once").is_some_and(|value| value.truthy()));
                self.world
                    .borrow_mut()
                    .add_listener(self.node, &kind, callback, once);
                Ok(Value::Undefined)
            }
            "removeEventListener" => {
                let kind = string_arg(first);
                let callback = args.get(1).cloned().unwrap_or(Value::Undefined);
                self.world
                    .borrow_mut()
                    .remove_listener(self.node, &kind, &callback);
                Ok(Value::Undefined)
            }
            // A synthetic event is queued rather than dispatched here, because a
            // host object has no way to call back into the interpreter. The
            // runtime drains the queue as soon as the script returns.
            "click" | "focus" | "blur" => {
                self.world
                    .borrow_mut()
                    .pending_events
                    .push((self.node, method.to_string()));
                Ok(Value::Undefined)
            }
            "scrollIntoView" => {
                let mut world = self.world.borrow_mut();
                if let Some(rect) = world.rects.get(&self.node).copied() {
                    world.scroll_to = Some((0.0, rect.y));
                }
                Ok(Value::Undefined)
            }
            other => Err(format!("`{other}` is not a method of {}", self.type_name())),
        }
    }
}

/// `element.classList`.
pub struct ClassList {
    world: SharedWorld,
    node: NodeId,
}

impl ClassList {
    fn classes(&self) -> Vec<String> {
        self.world
            .borrow()
            .attr(self.node, "class")
            .map(|value| value.split_ascii_whitespace().map(str::to_string).collect())
            .unwrap_or_default()
    }

    fn write(&self, classes: &[String]) {
        self.world
            .borrow_mut()
            .set_attr(self.node, "class", &classes.join(" "));
    }
}

impl HostObject for ClassList {
    fn type_name(&self) -> String {
        "DOMTokenList".to_string()
    }

    fn describe(&self) -> String {
        format!("DOMTokenList [{}]", self.classes().join(", "))
    }

    fn identity(&self) -> usize {
        CLASS_LIST_SPACE + self.node.index()
    }

    fn own_keys(&self) -> Vec<String> {
        (0..self.classes().len())
            .map(|index| index.to_string())
            .collect()
    }

    fn get(&self, key: &str) -> Option<Value> {
        let classes = self.classes();
        match key {
            "length" => Some(Value::Number(classes.len() as f64)),
            "value" => Some(Value::string(classes.join(" "))),
            other => other
                .parse::<usize>()
                .ok()
                .and_then(|index| classes.get(index).cloned())
                .map(Value::string),
        }
    }

    fn set(&self, key: &str, value: &Value) -> bool {
        if key == "value" {
            self.world
                .borrow_mut()
                .set_attr(self.node, "class", &value.to_js_string());
            return true;
        }
        false
    }

    fn invoke(&self, method: &str, args: &[Value]) -> Result<Value, String> {
        let mut classes = self.classes();
        match method {
            "contains" => {
                let wanted = string_arg(args.first());
                Ok(Value::Bool(classes.contains(&wanted)))
            }
            "add" => {
                for argument in args {
                    let class = argument.to_js_string();
                    if !class.is_empty() && !classes.contains(&class) {
                        classes.push(class);
                    }
                }
                self.write(&classes);
                Ok(Value::Undefined)
            }
            "remove" => {
                for argument in args {
                    let class = argument.to_js_string();
                    classes.retain(|present| *present != class);
                }
                self.write(&classes);
                Ok(Value::Undefined)
            }
            "toggle" => {
                let class = string_arg(args.first());
                if class.is_empty() {
                    return Ok(Value::Bool(false));
                }
                let present = classes.contains(&class);
                // A second argument forces the outcome either way.
                let wanted = match args.get(1) {
                    Some(force) => force.truthy(),
                    None => !present,
                };
                if wanted && !present {
                    classes.push(class);
                } else if !wanted && present {
                    classes.retain(|existing| *existing != class);
                }
                self.write(&classes);
                Ok(Value::Bool(wanted))
            }
            "replace" => {
                let stale = string_arg(args.first());
                let fresh = string_arg(args.get(1));
                let mut replaced = false;
                for class in classes.iter_mut() {
                    if *class == stale {
                        *class = fresh.clone();
                        replaced = true;
                    }
                }
                if replaced {
                    self.write(&classes);
                }
                Ok(Value::Bool(replaced))
            }
            "item" => {
                let index = args.first().map(Value::to_number).unwrap_or(0.0);
                Ok(match classes.get(index.max(0.0) as usize) {
                    Some(class) => Value::string(class.clone()),
                    None => Value::Null,
                })
            }
            other => Err(format!("`{other}` is not a method of DOMTokenList")),
        }
    }
}

/// `element.style`: the inline style attribute, seen as properties.
pub struct StyleDeclaration {
    world: SharedWorld,
    node: NodeId,
}

impl StyleDeclaration {
    fn text(&self) -> String {
        self.world
            .borrow()
            .attr(self.node, "style")
            .unwrap_or_default()
    }
}

impl HostObject for StyleDeclaration {
    fn type_name(&self) -> String {
        "CSSStyleDeclaration".to_string()
    }

    fn describe(&self) -> String {
        format!("CSSStyleDeclaration {{ {} }}", self.text())
    }

    fn identity(&self) -> usize {
        STYLE_SPACE + self.node.index()
    }

    fn own_keys(&self) -> Vec<String> {
        style::declarations(&self.text())
            .into_iter()
            .map(|(name, _)| style::to_camel_case(&name))
            .collect()
    }

    fn get(&self, key: &str) -> Option<Value> {
        // Every other name is treated as a property, so the methods have to be
        // excluded explicitly or `style.setProperty` would read as the empty
        // value of a property called `setProperty`.
        const METHODS: &[&str] = &[
            "getPropertyValue",
            "getPropertyPriority",
            "setProperty",
            "removeProperty",
            "item",
        ];
        if METHODS.contains(&key) {
            return None;
        }
        let text = self.text();
        if key == "cssText" {
            return Some(Value::string(text));
        }
        if key == "length" {
            return Some(Value::Number(style::declarations(&text).len() as f64));
        }
        let property = style::to_kebab_case(key);
        // A property that is not set reads as an empty string, not undefined,
        // which is what a page tests against.
        Some(Value::string(
            style::property(&text, &property).unwrap_or_default(),
        ))
    }

    fn set(&self, key: &str, value: &Value) -> bool {
        let text = self.text();
        let updated = if key == "cssText" {
            value.to_js_string()
        } else {
            let property = style::to_kebab_case(key);
            let value = value.to_js_string();
            if value.is_empty() {
                style::remove_property(&text, &property)
            } else {
                style::set_property(&text, &property, &value)
            }
        };
        self.world
            .borrow_mut()
            .set_attr(self.node, "style", &updated);
        true
    }

    fn invoke(&self, method: &str, args: &[Value]) -> Result<Value, String> {
        let text = self.text();
        match method {
            "getPropertyValue" => {
                let property = style::to_kebab_case(&string_arg(args.first()));
                Ok(Value::string(
                    style::property(&text, &property).unwrap_or_default(),
                ))
            }
            "setProperty" => {
                let property = style::to_kebab_case(&string_arg(args.first()));
                let value = string_arg(args.get(1));
                let updated = if value.is_empty() {
                    style::remove_property(&text, &property)
                } else {
                    style::set_property(&text, &property, &value)
                };
                self.world
                    .borrow_mut()
                    .set_attr(self.node, "style", &updated);
                Ok(Value::Undefined)
            }
            "removeProperty" => {
                let property = style::to_kebab_case(&string_arg(args.first()));
                let previous = style::property(&text, &property).unwrap_or_default();
                let updated = style::remove_property(&text, &property);
                self.world
                    .borrow_mut()
                    .set_attr(self.node, "style", &updated);
                Ok(Value::string(previous))
            }
            "item" => {
                let index = args.first().map(Value::to_number).unwrap_or(0.0);
                let declarations = style::declarations(&text);
                Ok(match declarations.get(index.max(0.0) as usize) {
                    Some((name, _)) => Value::string(name.clone()),
                    None => Value::string(""),
                })
            }
            other => Err(format!("`{other}` is not a method of CSSStyleDeclaration")),
        }
    }
}

/// The object `getBoundingClientRect` returns.
pub(crate) fn rect_object(rect: crate::world::Rect) -> Value {
    let object = JsObject::with_class("DOMRect");
    object.set("x", Value::Number(rect.x as f64));
    object.set("y", Value::Number(rect.y as f64));
    object.set("width", Value::Number(rect.width as f64));
    object.set("height", Value::Number(rect.height as f64));
    object.set("top", Value::Number(rect.y as f64));
    object.set("left", Value::Number(rect.x as f64));
    object.set("right", Value::Number((rect.x + rect.width) as f64));
    object.set("bottom", Value::Number((rect.y + rect.height) as f64));
    Value::object(object)
}

/// An argument as a string, treating a missing one as empty rather than
/// `"undefined"`.
pub(crate) fn string_arg(value: Option<&Value>) -> String {
    match value {
        Some(Value::Undefined) | None => String::new(),
        Some(other) => other.to_js_string(),
    }
}
