//! The globals a page expects: `document`, `window`, `location`, `navigator`
//! and the `Event` object a listener is handed.

use std::cell::RefCell;
use std::rc::Rc;

use wat_dom::NodeId;
use wat_js::value::HostObject;
use wat_js::{JsObject, Value};

use crate::node::{
    node_of, rect_object, string_arg, NodeHandle, DOCUMENT_SPACE, EVENT_SPACE, LOCATION_SPACE,
    WINDOW_SPACE,
};
use crate::world::{Dialog, DialogKind, Navigation, SharedWorld};

/// `document`.
pub struct DocumentHandle {
    world: SharedWorld,
}

impl DocumentHandle {
    /// Binds this global as a script value.
    pub fn bind(world: &SharedWorld) -> Value {
        Value::Host(Rc::new(DocumentHandle {
            world: world.clone(),
        }))
    }

    fn node(&self, node: Option<NodeId>) -> Value {
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
}

impl HostObject for DocumentHandle {
    fn type_name(&self) -> String {
        "Document".to_string()
    }

    fn describe(&self) -> String {
        "#document".to_string()
    }

    fn identity(&self) -> usize {
        DOCUMENT_SPACE
    }

    fn own_keys(&self) -> Vec<String> {
        vec!["title".to_string(), "readyState".to_string()]
    }

    fn get(&self, key: &str) -> Option<Value> {
        let world = self.world.borrow();
        let value = match key {
            "nodeType" => Value::Number(9.0),
            "nodeName" => Value::string("#document"),
            "documentElement" => self.node(world.document.find_tag("html")),
            "body" => self.node(world.document.body()),
            "head" => self.node(world.document.find_tag("head")),
            "title" => Value::string(world.document.title().unwrap_or_default()),
            // Scripts run after parsing here, so the document is always complete
            // by the time one can ask.
            "readyState" => Value::string("complete"),
            "URL" | "documentURI" => Value::string(world.location.clone()),
            "characterSet" | "charset" => Value::string("UTF-8"),
            "location" => {
                drop(world);
                LocationHandle::bind(&self.world)
            }
            "defaultView" => {
                drop(world);
                WindowHandle::bind(&self.world)
            }
            "children" => self.list(
                world
                    .document
                    .element_children(world.document.root())
                    .collect(),
            ),
            "scrollingElement" => self.node(world.document.find_tag("html")),
            "activeElement" => self.node(world.document.body()),
            _ => {
                // `document.onclick = fn` and friends.
                if let Some(kind) = key.strip_prefix("on") {
                    if !kind.is_empty() {
                        let root = world.document.root();
                        return Some(world.property_listener(root, kind).unwrap_or(Value::Null));
                    }
                }
                return None;
            }
        };
        Some(value)
    }

    fn set(&self, key: &str, value: &Value) -> bool {
        let mut world = self.world.borrow_mut();
        if let Some(kind) = key.strip_prefix("on") {
            if !kind.is_empty() {
                let root = world.document.root();
                let callback = if value.is_callable() {
                    Some(value.clone())
                } else {
                    None
                };
                world.set_property_listener(root, kind, callback);
                return true;
            }
        }
        match key {
            "title" => {
                let text = value.to_js_string();
                match world.document.find_tag("title") {
                    Some(title) => world.set_text_content(title, &text),
                    None => {
                        // A document with no <title> gets one, so a later read
                        // sees what was written.
                        if let Some(head) = world.document.find_tag("head") {
                            let title = world.document.create_element("title");
                            world.document.append(head, title);
                            world.set_text_content(title, &text);
                        }
                    }
                }
                world.title_changed = true;
                true
            }
            _ => false,
        }
    }

    fn invoke(&self, method: &str, args: &[Value]) -> Result<Value, String> {
        let first = args.first();
        match method {
            "getElementById" => {
                let wanted = string_arg(first);
                let world = self.world.borrow();
                let root = world.document.root();
                let found = world.document.descendants(root).find(|node| {
                    world
                        .document
                        .element(*node)
                        .and_then(|element| element.id())
                        == Some(wanted.as_str())
                });
                Ok(self.node(found))
            }
            "querySelector" => {
                let selector = string_arg(first);
                let world = self.world.borrow();
                let root = world.document.root();
                let found = world.query(root, &selector);
                Ok(self.node(found))
            }
            "querySelectorAll" => {
                let selector = string_arg(first);
                let found = {
                    let world = self.world.borrow();
                    let root = world.document.root();
                    world.query_all(root, &selector)
                };
                Ok(self.list(found))
            }
            "getElementsByTagName" => {
                let name = string_arg(first);
                let found = {
                    let world = self.world.borrow();
                    let root = world.document.root();
                    world.elements_by_tag(root, &name)
                };
                Ok(self.list(found))
            }
            "getElementsByClassName" => {
                let classes = string_arg(first);
                let found = {
                    let world = self.world.borrow();
                    let root = world.document.root();
                    world.elements_by_class(root, &classes)
                };
                Ok(self.list(found))
            }
            "createElement" => {
                let name = string_arg(first).to_ascii_lowercase();
                if name.is_empty() {
                    return Err("createElement needs a tag name".to_string());
                }
                let node = self.world.borrow_mut().document.create_element(name);
                Ok(NodeHandle::bind(&self.world, node))
            }
            "createTextNode" => {
                let text = string_arg(first);
                let node = self.world.borrow_mut().document.create_text(text);
                Ok(NodeHandle::bind(&self.world, node))
            }
            "createComment" => {
                let text = string_arg(first);
                let node = self
                    .world
                    .borrow_mut()
                    .document
                    .create(wat_dom::NodeData::Comment(text));
                Ok(NodeHandle::bind(&self.world, node))
            }
            "addEventListener" => {
                let kind = string_arg(first);
                let callback = args.get(1).cloned().unwrap_or(Value::Undefined);
                if !callback.is_callable() {
                    return Err("addEventListener expects a function".to_string());
                }
                let mut world = self.world.borrow_mut();
                let root = world.document.root();
                world.add_listener(root, &kind, callback, false);
                Ok(Value::Undefined)
            }
            "removeEventListener" => {
                let kind = string_arg(first);
                let callback = args.get(1).cloned().unwrap_or(Value::Undefined);
                let mut world = self.world.borrow_mut();
                let root = world.document.root();
                world.remove_listener(root, &kind, &callback);
                Ok(Value::Undefined)
            }
            "contains" => {
                let Some(node) = first.and_then(node_of) else {
                    return Ok(Value::Bool(false));
                };
                let world = self.world.borrow();
                Ok(Value::Bool(world.document.get(node).is_some()))
            }
            other => Err(format!("`{other}` is not a method of Document")),
        }
    }
}

/// `window`.
pub struct WindowHandle {
    world: SharedWorld,
}

impl WindowHandle {
    /// Binds this global as a script value.
    pub fn bind(world: &SharedWorld) -> Value {
        Value::Host(Rc::new(WindowHandle {
            world: world.clone(),
        }))
    }
}

impl HostObject for WindowHandle {
    fn type_name(&self) -> String {
        "Window".to_string()
    }

    fn describe(&self) -> String {
        "[object Window]".to_string()
    }

    fn identity(&self) -> usize {
        WINDOW_SPACE
    }

    fn get(&self, key: &str) -> Option<Value> {
        let world = self.world.borrow();
        let value = match key {
            "document" => {
                drop(world);
                DocumentHandle::bind(&self.world)
            }
            "location" => {
                drop(world);
                LocationHandle::bind(&self.world)
            }
            // `window`, `self` and `globalThis` are all the same object.
            "window" | "self" | "globalThis" | "top" | "parent" => {
                drop(world);
                WindowHandle::bind(&self.world)
            }
            "navigator" => navigator_object(&world.user_agent),
            "innerWidth" => Value::Number(world.viewport.0 as f64),
            "innerHeight" => Value::Number(world.viewport.1 as f64),
            "outerWidth" => Value::Number(world.viewport.0 as f64),
            "outerHeight" => Value::Number(world.viewport.1 as f64),
            "devicePixelRatio" => Value::Number(world.device_pixel_ratio as f64),
            "scrollX" | "pageXOffset" => Value::Number(world.scroll.0 as f64),
            "scrollY" | "pageYOffset" => Value::Number(world.scroll.1 as f64),
            "name" => Value::string(""),
            "closed" => Value::Bool(false),
            "origin" => Value::string(origin_of(&world.location)),
            _ => {
                if let Some(kind) = key.strip_prefix("on") {
                    if !kind.is_empty() {
                        let root = world.document.root();
                        return Some(world.property_listener(root, kind).unwrap_or(Value::Null));
                    }
                }
                // `window` is the global object, so anything else it holds is a
                // global binding.
                return world.global(key);
            }
        };
        Some(value)
    }

    fn set(&self, key: &str, value: &Value) -> bool {
        let mut world = self.world.borrow_mut();
        if let Some(kind) = key.strip_prefix("on") {
            if !kind.is_empty() {
                let root = world.document.root();
                let callback = if value.is_callable() {
                    Some(value.clone())
                } else {
                    None
                };
                world.set_property_listener(root, kind, callback);
                return true;
            }
        }
        if key == "location" {
            world.navigation = Some(Navigation {
                url: value.to_js_string(),
                replace: false,
            });
            return true;
        }
        // Anything else becomes a global, because that is what assigning to a
        // property of the global object means.
        world.set_global(key, value.clone());
        true
    }

    fn invoke(&self, method: &str, args: &[Value]) -> Result<Value, String> {
        let first = args.first();
        match method {
            "addEventListener" => {
                let kind = string_arg(first);
                let callback = args.get(1).cloned().unwrap_or(Value::Undefined);
                if !callback.is_callable() {
                    return Err("addEventListener expects a function".to_string());
                }
                let mut world = self.world.borrow_mut();
                let root = world.document.root();
                world.add_listener(root, &kind, callback, false);
                Ok(Value::Undefined)
            }
            "removeEventListener" => {
                let kind = string_arg(first);
                let callback = args.get(1).cloned().unwrap_or(Value::Undefined);
                let mut world = self.world.borrow_mut();
                let root = world.document.root();
                world.remove_listener(root, &kind, &callback);
                Ok(Value::Undefined)
            }
            "alert" | "confirm" | "prompt" => {
                let kind = match method {
                    "alert" => DialogKind::Alert,
                    "confirm" => DialogKind::Confirm,
                    _ => DialogKind::Prompt,
                };
                self.world.borrow_mut().dialogs.push(Dialog {
                    kind,
                    message: string_arg(first),
                });
                // Nothing here can block for an answer, so a confirmation is
                // declined and a prompt returns null rather than pretending.
                Ok(match kind {
                    DialogKind::Alert => Value::Undefined,
                    DialogKind::Confirm => Value::Bool(false),
                    DialogKind::Prompt => Value::Null,
                })
            }
            "scrollTo" | "scroll" => {
                let (x, y) = scroll_arguments(args);
                self.world.borrow_mut().scroll_to = Some((x, y));
                Ok(Value::Undefined)
            }
            "scrollBy" => {
                let (dx, dy) = scroll_arguments(args);
                let mut world = self.world.borrow_mut();
                let (x, y) = world.scroll;
                world.scroll_to = Some((x + dx, y + dy));
                Ok(Value::Undefined)
            }
            // A frame callback is a timer with no delay, which is what the host
            // treats it as.
            "requestAnimationFrame" => {
                Err("requestAnimationFrame is not supported; use setTimeout".to_string())
            }
            "getComputedStyle" => {
                Err("getComputedStyle is not supported yet; read element.style instead".to_string())
            }
            "matchMedia" => Err("matchMedia is not supported yet".to_string()),
            other => Err(format!("`{other}` is not a method of Window")),
        }
    }
}

/// `window.scrollTo(x, y)` and `window.scrollTo({ top, left })`.
fn scroll_arguments(args: &[Value]) -> (f32, f32) {
    if let Some(Value::Object(options)) = args.first() {
        let left = options
            .get("left")
            .map(|value| value.to_number())
            .unwrap_or(0.0);
        let top = options
            .get("top")
            .map(|value| value.to_number())
            .unwrap_or(0.0);
        return (left as f32, top as f32);
    }
    let x = args.first().map(Value::to_number).unwrap_or(0.0);
    let y = args.get(1).map(Value::to_number).unwrap_or(0.0);
    (finite(x), finite(y))
}

fn finite(value: f64) -> f32 {
    if value.is_finite() {
        value as f32
    } else {
        0.0
    }
}

pub fn navigator_object(user_agent: &str) -> Value {
    let object = JsObject::with_class("Navigator");
    object.set("userAgent", Value::string(user_agent));
    object.set("appName", Value::string("What-A-Browser"));
    object.set("platform", Value::string(std::env::consts::OS));
    object.set("language", Value::string("en"));
    object.set("languages", Value::array(vec![Value::string("en")]));
    object.set("onLine", Value::Bool(true));
    object.set("cookieEnabled", Value::Bool(false));
    // A browser that ships no plugin API says so rather than lying about it.
    object.set("plugins", Value::array(Vec::new()));
    Value::object(object)
}

/// `location`.
pub struct LocationHandle {
    world: SharedWorld,
}

impl LocationHandle {
    /// Binds this global as a script value.
    pub fn bind(world: &SharedWorld) -> Value {
        Value::Host(Rc::new(LocationHandle {
            world: world.clone(),
        }))
    }
}

impl HostObject for LocationHandle {
    fn type_name(&self) -> String {
        "Location".to_string()
    }

    fn describe(&self) -> String {
        self.world.borrow().location.clone()
    }

    fn identity(&self) -> usize {
        LOCATION_SPACE
    }

    fn own_keys(&self) -> Vec<String> {
        [
            "href", "protocol", "host", "hostname", "pathname", "search", "hash", "origin",
        ]
        .iter()
        .map(|name| name.to_string())
        .collect()
    }

    fn get(&self, key: &str) -> Option<Value> {
        let url = self.world.borrow().location.clone();
        let parts = UrlParts::parse(&url);
        let value = match key {
            "href" | "toString" => Value::string(url),
            "protocol" => Value::string(parts.protocol),
            "host" => Value::string(parts.host),
            "hostname" => Value::string(parts.hostname),
            "port" => Value::string(parts.port),
            "pathname" => Value::string(parts.pathname),
            "search" => Value::string(parts.search),
            "hash" => Value::string(parts.hash),
            "origin" => Value::string(origin_of(&self.world.borrow().location)),
            _ => return None,
        };
        Some(value)
    }

    fn set(&self, key: &str, value: &Value) -> bool {
        let mut world = self.world.borrow_mut();
        let url = world.location.clone();
        let target = match key {
            "href" => value.to_js_string(),
            "hash" => {
                let fragment = value.to_js_string();
                let base = url.split('#').next().unwrap_or("").to_string();
                if fragment.starts_with('#') {
                    format!("{base}{fragment}")
                } else {
                    format!("{base}#{fragment}")
                }
            }
            "search" | "pathname" | "host" | "hostname" | "protocol" | "port" => {
                // Rewriting one component means re-resolving the whole URL,
                // which the host does; it is handed the piece that changed.
                return false;
            }
            _ => return false,
        };
        world.navigation = Some(Navigation {
            url: target,
            replace: false,
        });
        true
    }

    fn invoke(&self, method: &str, args: &[Value]) -> Result<Value, String> {
        match method {
            "assign" | "replace" => {
                let url = string_arg(args.first());
                self.world.borrow_mut().navigation = Some(Navigation {
                    url,
                    replace: method == "replace",
                });
                Ok(Value::Undefined)
            }
            "reload" => {
                let mut world = self.world.borrow_mut();
                let url = world.location.clone();
                world.navigation = Some(Navigation { url, replace: true });
                Ok(Value::Undefined)
            }
            "toString" => Ok(Value::string(self.world.borrow().location.clone())),
            other => Err(format!("`{other}` is not a method of Location")),
        }
    }
}

/// The pieces of a URL `location` reports separately.
struct UrlParts {
    protocol: String,
    host: String,
    hostname: String,
    port: String,
    pathname: String,
    search: String,
    hash: String,
}

impl UrlParts {
    fn parse(url: &str) -> UrlParts {
        let mut parts = UrlParts {
            protocol: String::new(),
            host: String::new(),
            hostname: String::new(),
            port: String::new(),
            pathname: "/".to_string(),
            search: String::new(),
            hash: String::new(),
        };

        let (scheme, rest) = match url.split_once(':') {
            Some((scheme, rest)) => (scheme, rest),
            None => ("", url),
        };
        parts.protocol = if scheme.is_empty() {
            String::new()
        } else {
            format!("{scheme}:")
        };

        let rest = rest.strip_prefix("//").unwrap_or(rest);
        let (rest, hash) = match rest.split_once('#') {
            Some((rest, hash)) => (rest, format!("#{hash}")),
            None => (rest, String::new()),
        };
        parts.hash = hash;
        let (rest, search) = match rest.split_once('?') {
            Some((rest, search)) => (rest, format!("?{search}")),
            None => (rest, String::new()),
        };
        parts.search = search;

        // Only a hierarchical URL has an authority; `about:home` is all path.
        if url.contains("//") {
            let (authority, path) = match rest.split_once('/') {
                Some((authority, path)) => (authority, format!("/{path}")),
                None => (rest, "/".to_string()),
            };
            parts.host = authority.to_string();
            match authority.rsplit_once(':') {
                Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) => {
                    parts.hostname = host.to_string();
                    parts.port = port.to_string();
                }
                _ => parts.hostname = authority.to_string(),
            }
            parts.pathname = path;
        } else {
            parts.pathname = rest.to_string();
        }
        parts
    }
}

fn origin_of(url: &str) -> String {
    let parts = UrlParts::parse(url);
    if parts.host.is_empty() {
        return "null".to_string();
    }
    format!("{}//{}", parts.protocol, parts.host)
}

/// What a listener can change about the event it was handed.
#[derive(Default)]
pub struct EventState {
    pub default_prevented: bool,
    pub propagation_stopped: bool,
    /// The node the listener currently running was registered on.
    pub current: Option<NodeId>,
}

/// The `Event` a listener receives.
pub struct EventHandle {
    world: SharedWorld,
    state: Rc<RefCell<EventState>>,
    kind: String,
    target: NodeId,
    bubbles: bool,
    identity: usize,
}

impl EventHandle {
    /// Binds an event as a script value.
    pub fn bind(
        world: &SharedWorld,
        state: Rc<RefCell<EventState>>,
        kind: &str,
        target: NodeId,
        bubbles: bool,
    ) -> Value {
        Value::Host(Rc::new(EventHandle {
            world: world.clone(),
            state,
            kind: kind.to_string(),
            target,
            bubbles,
            identity: EVENT_SPACE + target.index(),
        }))
    }
}

impl HostObject for EventHandle {
    fn type_name(&self) -> String {
        "Event".to_string()
    }

    fn describe(&self) -> String {
        format!("Event {{ type: '{}' }}", self.kind)
    }

    fn identity(&self) -> usize {
        self.identity
    }

    fn own_keys(&self) -> Vec<String> {
        vec!["type".to_string(), "target".to_string()]
    }

    fn get(&self, key: &str) -> Option<Value> {
        let value = match key {
            "type" => Value::string(self.kind.clone()),
            "target" | "srcElement" => NodeHandle::bind(&self.world, self.target),
            "currentTarget" => match self.state.borrow().current {
                Some(node) => NodeHandle::bind(&self.world, node),
                None => Value::Null,
            },
            "bubbles" => Value::Bool(self.bubbles),
            "cancelable" => Value::Bool(true),
            "defaultPrevented" => Value::Bool(self.state.borrow().default_prevented),
            "isTrusted" => Value::Bool(true),
            "eventPhase" => Value::Number(match self.state.borrow().current {
                Some(node) if node == self.target => 2.0,
                _ => 3.0,
            }),
            "timeStamp" => Value::Number(0.0),
            _ => return None,
        };
        Some(value)
    }

    fn set(&self, _key: &str, _value: &Value) -> bool {
        false
    }

    fn invoke(&self, method: &str, _args: &[Value]) -> Result<Value, String> {
        match method {
            "preventDefault" => {
                self.state.borrow_mut().default_prevented = true;
                Ok(Value::Undefined)
            }
            "stopPropagation" | "stopImmediatePropagation" => {
                self.state.borrow_mut().propagation_stopped = true;
                Ok(Value::Undefined)
            }
            "getBoundingClientRect" => Ok(rect_object(Default::default())),
            other => Err(format!("`{other}` is not a method of Event")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_parts_of_an_http_address() {
        let parts = UrlParts::parse("https://example.com:8443/a/b?q=1#top");
        assert_eq!(parts.protocol, "https:");
        assert_eq!(parts.host, "example.com:8443");
        assert_eq!(parts.hostname, "example.com");
        assert_eq!(parts.port, "8443");
        assert_eq!(parts.pathname, "/a/b");
        assert_eq!(parts.search, "?q=1");
        assert_eq!(parts.hash, "#top");
    }

    #[test]
    fn url_parts_of_a_bare_host() {
        let parts = UrlParts::parse("http://example.com");
        assert_eq!(parts.host, "example.com");
        assert_eq!(parts.port, "");
        assert_eq!(parts.pathname, "/");
        assert_eq!(parts.search, "");
    }

    #[test]
    fn url_parts_of_an_internal_page() {
        let parts = UrlParts::parse("about:home");
        assert_eq!(parts.protocol, "about:");
        assert_eq!(parts.host, "", "an about: URL has no authority");
        assert_eq!(parts.pathname, "home");
        assert_eq!(origin_of("about:home"), "null");
    }

    #[test]
    fn origins_come_from_the_scheme_and_authority() {
        assert_eq!(origin_of("https://example.com/a"), "https://example.com");
        assert_eq!(
            origin_of("http://example.com:8080/a"),
            "http://example.com:8080"
        );
    }
}
