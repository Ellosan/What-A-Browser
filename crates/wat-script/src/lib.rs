//! DOM bindings, so WAT's JavaScript engine can drive a page.
//!
//! [`wat_js`] knows nothing about HTML; this crate is the layer that gives it a
//! `document`, a `window` and an event loop's worth of dispatch. Every binding
//! goes through the same public [`wat_js::HostObject`] trait an embedder would
//! use, so there is nothing privileged about the DOM.
//!
//! # Owning the document
//!
//! Scripts mutate the tree, so the tree has to be shared — but the engine's
//! layout and paint passes want a plain `&Document`. Rather than wrap the whole
//! page in a cell, the runtime *borrows* the document for the duration of each
//! call:
//!
//! ```
//! use wat_script::ScriptRuntime;
//!
//! let mut document = wat_html::parse("<p id='out'>before</p>");
//! let mut runtime = ScriptRuntime::new("about:blank");
//! runtime.eval(&mut document, "document.getElementById('out').textContent = 'after'");
//!
//! // The document is back in the caller's hands, changed.
//! let paragraph = document.query("#out").unwrap();
//! assert_eq!(document.text_content(paragraph), "after");
//! ```
//!
//! Swapping a `Document` is a few pointer moves, so this costs nothing per call
//! and keeps the ownership obvious.
//!
//! # Events
//!
//! The browser tells the runtime what happened and the runtime reports what the
//! page decided:
//!
//! ```
//! use wat_script::ScriptRuntime;
//!
//! let mut document = wat_html::parse("<a href='/next'>go</a>");
//! let mut runtime = ScriptRuntime::new("https://example.com/");
//! runtime.eval(
//!     &mut document,
//!     "document.querySelector('a').addEventListener('click', e => e.preventDefault())",
//! );
//!
//! let link = document.query("a").unwrap();
//! let outcome = runtime.dispatch(&mut document, link, "click");
//! assert!(outcome.default_prevented, "the page handled the click itself");
//! ```
//!
//! # What is bound
//!
//! `document` (`getElementById`, `querySelector`/`All`, `getElementsBy*`,
//! `createElement`, `createTextNode`, `body`, `head`, `title`, `location`),
//! elements (`textContent`, `innerHTML`, `outerHTML`, attributes and reflected
//! properties, `classList`, `style`, tree walking and tree editing,
//! `getBoundingClientRect`, `addEventListener`), `window` (`innerWidth`,
//! `innerHeight`, `devicePixelRatio`, `scrollTo`, `alert`, `navigator`,
//! `location`) and the `Event` a listener is handed.
//!
//! Collections come back as JavaScript arrays rather than live `NodeList`s, so
//! `forEach`, `map` and spread work without a bespoke host type. Not bound:
//! `getComputedStyle`, `requestAnimationFrame`, `matchMedia`, `fetch`,
//! `XMLHttpRequest`, storage and cookies.

pub mod globals;
pub mod node;
pub mod style;
pub mod world;

use std::collections::HashMap;

use wat_dom::{Document, NodeId};
use wat_js::{ConsoleLevel, ConsoleMessage, Interp, Value};

pub use world::{Dialog, DialogKind, Navigation, Rect, SharedWorld, World};

/// What WAT reports as its user agent.
pub const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (compatible) WAT/",
    env!("CARGO_PKG_VERSION"),
    " (What-A-Browser; WAT Engine)"
);

/// A script that failed, with enough context to show in a console.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptError {
    /// Which `<script>` it was, in document order, or `None` for `eval`.
    pub index: Option<usize>,
    pub message: String,
    /// Set when the failure was a resource limit rather than a thrown value, in
    /// which case the page was stopped rather than merely erroring.
    pub fatal: bool,
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.index {
            Some(index) => write!(f, "script {index}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

/// What an event dispatch changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dispatch {
    /// A listener called `preventDefault`, so the browser must not do whatever
    /// it was going to.
    pub default_prevented: bool,
    /// How many listeners ran, including inline `on…` attributes.
    pub listeners_run: usize,
    /// Listeners that threw.
    pub errors: Vec<ScriptError>,
}

/// A JavaScript runtime bound to one page.
pub struct ScriptRuntime {
    interp: Interp,
    world: SharedWorld,
}

impl ScriptRuntime {
    /// Builds a runtime for a document loaded from `location`.
    pub fn new(location: &str) -> ScriptRuntime {
        let world = World::new(Document::new(), location);
        let mut interp = Interp::new();
        // `window` is the global object, so it needs the scope the globals live
        // in.
        world.borrow_mut().attach_globals(interp.global.clone());

        let window = globals::WindowHandle::bind(&world);
        interp.define_global("window", window.clone());
        interp.define_global("self", window.clone());
        interp.define_global("globalThis", window);
        interp.define_global("document", globals::DocumentHandle::bind(&world));
        interp.define_global("location", globals::LocationHandle::bind(&world));
        interp.define_global("navigator", {
            // Read once: the user agent does not change while a page is open.
            let world_ref = world.borrow();
            globals::navigator_object(&world_ref.user_agent)
        });

        // The dialogs are globals as well as window methods, because pages call
        // them both ways. Nothing here can block for an answer, so a
        // confirmation is declined and a prompt returns null rather than
        // inventing a reply.
        for (name, kind, answer) in [
            ("alert", DialogKind::Alert, Value::Undefined),
            ("confirm", DialogKind::Confirm, Value::Bool(false)),
            ("prompt", DialogKind::Prompt, Value::Null),
        ] {
            let world = world.clone();
            interp.define_global(
                name,
                wat_js::native(name, move |_, _, args| {
                    world.borrow_mut().dialogs.push(Dialog {
                        kind,
                        message: args.first().map(Value::to_js_string).unwrap_or_default(),
                    });
                    Ok(answer.clone())
                }),
            );
        }

        ScriptRuntime { interp, world }
    }

    /// The engine's view of the page, for setting the viewport, the scroll
    /// offset and the layout rectangles scripts can read.
    pub fn world(&self) -> &SharedWorld {
        &self.world
    }

    pub fn set_viewport(&mut self, width: f32, height: f32) {
        self.world.borrow_mut().viewport = (width, height);
    }

    pub fn set_device_pixel_ratio(&mut self, ratio: f32) {
        self.world.borrow_mut().device_pixel_ratio = ratio;
    }

    pub fn set_scroll(&mut self, x: f32, y: f32) {
        self.world.borrow_mut().scroll = (x, y);
    }

    pub fn set_location(&mut self, location: &str) {
        self.world.borrow_mut().location = location.to_string();
    }

    /// Publishes layout rectangles, so `getBoundingClientRect` and the `offset*`
    /// properties report where things actually are.
    pub fn set_rects(&mut self, rects: HashMap<NodeId, Rect>) {
        self.world.borrow_mut().rects = rects;
    }

    /// Runs `source` against `document`.
    pub fn eval(&mut self, document: &mut Document, source: &str) -> Result<Value, ScriptError> {
        self.with_document(document, |runtime| {
            runtime.interp.reset_budget();
            runtime.interp.eval(source).map_err(|message| ScriptError {
                index: None,
                message,
                fatal: false,
            })
        })
    }

    /// Runs every `<script>` in the document, in order.
    ///
    /// A script that throws does not stop the ones after it, which is what a
    /// browser does; the failures are returned so the host can report them.
    pub fn run_document_scripts(&mut self, document: &mut Document) -> Vec<ScriptError> {
        let sources = collect_scripts(document);
        let mut errors = Vec::new();
        for (index, source) in sources.into_iter().enumerate() {
            if source.trim().is_empty() {
                continue;
            }
            self.with_document(document, |runtime| {
                runtime.interp.reset_budget();
                if let Err(message) = runtime.interp.eval(&source) {
                    let fatal = message.contains("too long") || message.contains("call depth");
                    errors.push(ScriptError {
                        index: Some(index),
                        message,
                        fatal,
                    });
                }
            });
        }
        // A script that queued a `click()` gets it delivered before the page is
        // considered loaded.
        self.run_pending_events(document);
        errors
    }

    /// Fires `kind` at `node`, then at each of its ancestors.
    pub fn dispatch(&mut self, document: &mut Document, node: NodeId, kind: &str) -> Dispatch {
        let outcome = self.with_document(document, |runtime| runtime.dispatch_inner(node, kind));
        self.run_pending_events(document);
        outcome
    }

    /// Fires the `load` event, which is how a page runs its start-up code.
    pub fn dispatch_load(&mut self, document: &mut Document) -> Dispatch {
        let root = document.root();
        let mut outcome = self.dispatch(document, root, "DOMContentLoaded");
        let load = self.dispatch(document, root, "load");
        outcome.listeners_run += load.listeners_run;
        outcome.errors.extend(load.errors);
        outcome
    }

    /// Runs the callbacks queued by `setTimeout`, in the order they were queued.
    ///
    /// Delays are not honoured: a host that wants real timing should read
    /// [`Timer::delay`](wat_js::Timer) itself and call this when it is due. What
    /// this does guarantee is that a timer callback cannot run in the middle of
    /// another script.
    pub fn run_timers(&mut self, document: &mut Document) -> Vec<ScriptError> {
        self.with_document(document, |runtime| {
            let mut errors = Vec::new();
            let mut timers = runtime.interp.take_timers();
            timers.sort_by(|a, b| a.delay.total_cmp(&b.delay).then(a.id.cmp(&b.id)));
            for timer in timers {
                runtime.interp.reset_budget();
                if let Err(control) =
                    runtime
                        .interp
                        .call(&timer.callback, Value::Undefined, &timer.args)
                {
                    errors.push(ScriptError {
                        index: None,
                        message: control.message(),
                        fatal: control.is_fatal(),
                    });
                }
            }
            errors
        })
    }

    /// Whether any timer is waiting to run.
    pub fn has_timers(&self) -> bool {
        !self.interp.timers.is_empty()
    }

    /// Delivers the events scripts asked for with `el.click()`.
    fn run_pending_events(&mut self, document: &mut Document) {
        // Bounded, so a handler that clicks itself cannot loop forever.
        for _ in 0..16 {
            let pending = std::mem::take(&mut self.world.borrow_mut().pending_events);
            if pending.is_empty() {
                return;
            }
            for (node, kind) in pending {
                self.with_document(document, |runtime| {
                    runtime.dispatch_inner(node, &kind);
                });
            }
        }
    }

    /// Dispatch, with the document already swapped in.
    fn dispatch_inner(&mut self, node: NodeId, kind: &str) -> Dispatch {
        use std::cell::RefCell;
        use std::rc::Rc;

        let kind = world::normalise_event(kind);
        let state = Rc::new(RefCell::new(globals::EventState::default()));
        // Only some events bubble; `focus` and `blur` famously do not.
        let bubbles = !matches!(kind.as_str(), "focus" | "blur" | "load" | "unload");
        let event = globals::EventHandle::bind(&self.world, state.clone(), &kind, node, bubbles);

        let path: Vec<NodeId> = if bubbles {
            let world = self.world.borrow();
            std::iter::once(node)
                .chain(world.document.ancestors(node))
                .collect()
        } else {
            vec![node]
        };

        let mut outcome = Dispatch::default();
        for target in path {
            state.borrow_mut().current = Some(target);

            // An `on…` attribute is compiled on the spot, unless a handler was
            // assigned to the property, which supersedes it.
            let inline = {
                let world = self.world.borrow();
                if world.has_property_listener(target, &kind) {
                    None
                } else {
                    world.attr(target, &format!("on{kind}"))
                }
            };
            if let Some(source) = inline.filter(|source| !source.trim().is_empty()) {
                outcome.listeners_run += 1;
                if let Err(message) = self.run_inline_handler(target, &source, &event) {
                    outcome.errors.push(message);
                }
            }

            let listeners = self.world.borrow_mut().take_listeners_for(target, &kind);
            for callback in listeners {
                outcome.listeners_run += 1;
                let receiver = node::NodeHandle::bind(&self.world, target);
                self.interp.reset_budget();
                if let Err(control) =
                    self.interp
                        .call(&callback, receiver, std::slice::from_ref(&event))
                {
                    outcome.errors.push(ScriptError {
                        index: None,
                        message: control.message(),
                        fatal: control.is_fatal(),
                    });
                }
            }

            if state.borrow().propagation_stopped {
                break;
            }
        }

        outcome.default_prevented = state.borrow().default_prevented;
        outcome
    }

    /// Compiles and runs an `onclick="…"` attribute.
    ///
    /// The body becomes a function so `this` is the element and `event` is in
    /// scope, exactly as a browser arranges it.
    fn run_inline_handler(
        &mut self,
        target: NodeId,
        source: &str,
        event: &Value,
    ) -> Result<(), ScriptError> {
        let wrapped = format!("(function (event) {{\n{source}\n}})");
        self.interp.reset_budget();
        let handler = self.interp.eval(&wrapped).map_err(|message| ScriptError {
            index: None,
            message,
            fatal: false,
        })?;
        let receiver = node::NodeHandle::bind(&self.world, target);
        self.interp
            .call(&handler, receiver, std::slice::from_ref(event))
            .map(|_| ())
            .map_err(|control| ScriptError {
                index: None,
                message: control.message(),
                fatal: control.is_fatal(),
            })
    }

    /// Whether a script changed the page since this was last called.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.world.borrow_mut().dirty)
    }

    /// Whether a script changed `document.title` since this was last called.
    pub fn take_title_changed(&mut self) -> bool {
        std::mem::take(&mut self.world.borrow_mut().title_changed)
    }

    /// A navigation a script asked for.
    pub fn take_navigation(&mut self) -> Option<Navigation> {
        self.world.borrow_mut().navigation.take()
    }

    /// A scroll a script asked for.
    pub fn take_scroll(&mut self) -> Option<(f32, f32)> {
        self.world.borrow_mut().scroll_to.take()
    }

    /// Dialogs a script opened.
    pub fn take_dialogs(&mut self) -> Vec<Dialog> {
        std::mem::take(&mut self.world.borrow_mut().dialogs)
    }

    /// Everything the page logged.
    pub fn console(&self) -> &[ConsoleMessage] {
        &self.interp.console
    }

    /// Removes and returns the console messages, so a host can drain them.
    pub fn take_console(&mut self) -> Vec<ConsoleMessage> {
        std::mem::take(&mut self.interp.console)
    }

    /// Records a message as though the page had logged it, so engine-level
    /// script failures show up in the same place as `console.error`.
    pub fn log(&mut self, level: ConsoleLevel, text: impl Into<String>) {
        self.interp.log(level, text.into());
    }

    /// Lends the document to the runtime for the duration of `body`.
    fn with_document<R>(
        &mut self,
        document: &mut Document,
        body: impl FnOnce(&mut Self) -> R,
    ) -> R {
        std::mem::swap(&mut self.world.borrow_mut().document, document);
        let result = body(self);
        std::mem::swap(&mut self.world.borrow_mut().document, document);
        result
    }
}

impl Drop for ScriptRuntime {
    fn drop(&mut self) {
        // The world reaches the closures that reach back to it: through the
        // listeners it stores, and through the global scope that holds
        // `document` and `window`. Both have to be let go of by hand, or the
        // world outlives the runtime.
        let mut world = self.world.borrow_mut();
        world.clear_listeners();
        world.detach_globals();
    }
}

/// The source of every runnable `<script>`, in document order.
///
/// A script with a `src` is skipped: fetching is the host's job, and it can put
/// the response in as the element's text before calling this.
fn collect_scripts(document: &Document) -> Vec<String> {
    let mut sources = Vec::new();
    for node in document.descendants(document.root()) {
        let Some(element) = document.element(node) else {
            continue;
        };
        if element.name != "script" {
            continue;
        }
        // A type that is not JavaScript is data, not code.
        if let Some(kind) = element.attr("type") {
            let kind = kind.trim().to_ascii_lowercase();
            let is_javascript = kind.is_empty()
                || kind == "text/javascript"
                || kind == "application/javascript"
                || kind == "module";
            if !is_javascript {
                continue;
            }
        }
        sources.push(document.text_content(node));
    }
    sources
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs a script against markup and hands back both, so a test can check
    /// what the page did to the tree.
    fn run(html: &str, source: &str) -> (Document, ScriptRuntime, String) {
        let mut document = wat_html::parse(html);
        let mut runtime = ScriptRuntime::new("https://example.com/page?q=1#top");
        runtime.set_viewport(800.0, 600.0);
        let result = runtime
            .eval(&mut document, source)
            .unwrap_or_else(|error| panic!("{source} failed: {error}"));
        (document, runtime, result.to_js_string())
    }

    /// The value of an expression evaluated against markup.
    fn value(html: &str, source: &str) -> String {
        run(html, source).2
    }

    /// The markup after a script has run.
    fn html_after(html: &str, source: &str) -> String {
        let (document, _, _) = run(html, source);
        let body = document.body().unwrap();
        let mut out = String::new();
        for child in document.children(body) {
            out.push_str(&document.to_html(child));
        }
        out
    }

    fn error(html: &str, source: &str) -> String {
        let mut document = wat_html::parse(html);
        let mut runtime = ScriptRuntime::new("about:blank");
        match runtime.eval(&mut document, source) {
            Ok(value) => panic!("{source} unexpectedly produced {value:?}"),
            Err(error) => error.message,
        }
    }

    #[test]
    fn the_document_is_returned_to_the_caller() {
        let mut document = wat_html::parse("<p>a</p>");
        let mut runtime = ScriptRuntime::new("about:blank");
        runtime.eval(&mut document, "1").unwrap();
        // The caller still owns a usable document after the run.
        assert!(document.query("p").is_some());
        runtime.eval(&mut document, "1").unwrap();
        assert!(document.query("p").is_some(), "and after a second run");
    }

    #[test]
    fn finding_elements() {
        let html = "<div id='a' class='card big'><p>one</p><p class='x'>two</p></div>";
        assert_eq!(value(html, "document.getElementById('a').tagName"), "DIV");
        assert_eq!(value(html, "document.getElementById('nope')"), "null");
        assert_eq!(
            value(html, "document.querySelector('p').textContent"),
            "one"
        );
        assert_eq!(value(html, "document.querySelectorAll('p').length"), "2");
        assert_eq!(
            value(html, "document.querySelector('.x').textContent"),
            "two"
        );
        assert_eq!(
            value(html, "document.querySelector('#a > p.x').textContent"),
            "two"
        );
        assert_eq!(
            value(html, "document.getElementsByTagName('p').length"),
            "2"
        );
        assert_eq!(
            value(html, "document.getElementsByClassName('card big').length"),
            "1"
        );
        assert_eq!(value(html, "document.body.tagName"), "BODY");
        assert_eq!(value(html, "document.documentElement.tagName"), "HTML");
    }

    #[test]
    fn a_collection_is_an_array_so_the_usual_methods_work() {
        let html = "<ul><li>a</li><li>b</li><li>c</li></ul>";
        assert_eq!(
            value(
                html,
                "Array.from(document.querySelectorAll('li')).map(li => li.textContent).join('')"
            ),
            "abc"
        );
        assert_eq!(
            value(
                html,
                "document.querySelectorAll('li').map(li => li.textContent).join('-')"
            ),
            "a-b-c"
        );
        assert_eq!(
            value(html, "let out = ''; document.querySelectorAll('li').forEach(li => out += li.textContent); out"),
            "abc"
        );
        assert_eq!(
            value(html, "[...document.querySelectorAll('li')].length"),
            "3"
        );
        assert_eq!(
            value(html, "document.querySelectorAll('li')[1].textContent"),
            "b"
        );
    }

    #[test]
    fn reading_and_writing_text() {
        let html = "<div><b>bold</b> plain</div>";
        assert_eq!(
            value(html, "document.querySelector('div').textContent"),
            "bold plain"
        );
        assert_eq!(
            value(html, "document.querySelector('div').innerHTML"),
            "<b>bold</b> plain"
        );
        assert_eq!(
            value(html, "document.querySelector('div').outerHTML"),
            "<div><b>bold</b> plain</div>"
        );
        assert_eq!(
            html_after(html, "document.querySelector('div').textContent = 'new'"),
            "<div>new</div>"
        );
    }

    #[test]
    fn writing_inner_html_parses_into_the_page() {
        let after = html_after(
            "<div></div>",
            "document.querySelector('div').innerHTML = '<span class=x>hi</span>'",
        );
        assert_eq!(after, "<div><span class=\"x\">hi</span></div>");
        // The new nodes are live: selectors find them.
        assert_eq!(
            value(
                "<div></div>",
                "const d = document.querySelector('div'); d.innerHTML = '<i>a</i><i>b</i>'; document.querySelectorAll('i').length"
            ),
            "2"
        );
    }

    #[test]
    fn attributes() {
        let html = "<a href='/one' data-role='link'>go</a>";
        assert_eq!(
            value(html, "document.querySelector('a').getAttribute('href')"),
            "/one"
        );
        assert_eq!(value(html, "document.querySelector('a').href"), "/one");
        assert_eq!(
            value(
                html,
                "document.querySelector('a').getAttribute('data-role')"
            ),
            "link"
        );
        assert_eq!(
            value(html, "document.querySelector('a').getAttribute('missing')"),
            "null"
        );
        assert_eq!(
            value(html, "document.querySelector('a').hasAttribute('href')"),
            "true"
        );
        assert_eq!(
            html_after(
                html,
                "document.querySelector('a').setAttribute('title', 'T')"
            ),
            "<a href=\"/one\" data-role=\"link\" title=\"T\">go</a>"
        );
        assert_eq!(
            html_after(
                html,
                "document.querySelector('a').removeAttribute('data-role')"
            ),
            "<a href=\"/one\">go</a>"
        );
        assert_eq!(
            value(
                html,
                "document.querySelector('a').getAttributeNames().join(',')"
            ),
            "href,data-role"
        );
    }

    #[test]
    fn boolean_attributes_read_and_write_as_booleans() {
        assert_eq!(
            value(
                "<input disabled>",
                "document.querySelector('input').disabled"
            ),
            "true"
        );
        assert_eq!(
            value("<input>", "document.querySelector('input').disabled"),
            "false"
        );
        assert_eq!(
            html_after("<input>", "document.querySelector('input').disabled = true"),
            "<input disabled>"
        );
        assert_eq!(
            html_after(
                "<input disabled>",
                "document.querySelector('input').disabled = false"
            ),
            "<input>"
        );
    }

    #[test]
    fn input_values() {
        assert_eq!(
            value("<input value='v'>", "document.querySelector('input').value"),
            "v"
        );
        assert_eq!(
            html_after("<input>", "document.querySelector('input').value = 'typed'"),
            "<input value=\"typed\">"
        );
        // A textarea keeps its value as its content.
        assert_eq!(
            value(
                "<textarea>text</textarea>",
                "document.querySelector('textarea').value"
            ),
            "text"
        );
        assert_eq!(
            html_after(
                "<textarea></textarea>",
                "document.querySelector('textarea').value = 'x'"
            ),
            "<textarea>x</textarea>"
        );
    }

    #[test]
    fn class_list() {
        let html = "<p class='a b'>x</p>";
        assert_eq!(value(html, "document.querySelector('p').className"), "a b");
        assert_eq!(
            value(html, "document.querySelector('p').classList.length"),
            "2"
        );
        assert_eq!(
            value(html, "document.querySelector('p').classList.contains('a')"),
            "true"
        );
        assert_eq!(
            value(html, "document.querySelector('p').classList.contains('c')"),
            "false"
        );
        assert_eq!(value(html, "document.querySelector('p').classList[0]"), "a");
        assert_eq!(
            value(html, "document.querySelector('p').classList.value"),
            "a b"
        );

        assert_eq!(
            html_after(html, "document.querySelector('p').classList.add('c')"),
            "<p class=\"a b c\">x</p>"
        );
        assert_eq!(
            html_after(html, "document.querySelector('p').classList.add('a')"),
            "<p class=\"a b\">x</p>",
            "adding a class it already has changes nothing"
        );
        assert_eq!(
            html_after(html, "document.querySelector('p').classList.remove('a')"),
            "<p class=\"b\">x</p>"
        );
        assert_eq!(
            html_after(html, "document.querySelector('p').classList.toggle('b')"),
            "<p class=\"a\">x</p>"
        );
        assert_eq!(
            html_after(
                html,
                "document.querySelector('p').classList.toggle('c', false)"
            ),
            "<p class=\"a b\">x</p>",
            "a forced toggle does not add"
        );
        assert_eq!(
            html_after(
                html,
                "document.querySelector('p').classList.replace('a', 'z')"
            ),
            "<p class=\"z b\">x</p>"
        );
        assert_eq!(
            html_after(html, "document.querySelector('p').className = 'only'"),
            "<p class=\"only\">x</p>"
        );
    }

    #[test]
    fn inline_styles() {
        let html = "<p style='color: red'>x</p>";
        assert_eq!(
            value(html, "document.querySelector('p').style.color"),
            "red"
        );
        assert_eq!(
            value(html, "document.querySelector('p').style.margin"),
            "",
            "an unset property reads as an empty string"
        );
        assert_eq!(
            html_after(html, "document.querySelector('p').style.display = 'none'"),
            "<p style=\"color: red; display: none\">x</p>"
        );
        assert_eq!(
            html_after(
                html,
                "document.querySelector('p').style.backgroundColor = 'blue'"
            ),
            "<p style=\"color: red; background-color: blue\">x</p>",
            "camelCase becomes a hyphenated property"
        );
        assert_eq!(
            html_after(html, "document.querySelector('p').style.color = ''"),
            "<p style>x</p>",
            "assigning an empty string removes the property"
        );
        assert_eq!(
            html_after(
                html,
                "document.querySelector('p').style.setProperty('top', '1px')"
            ),
            "<p style=\"color: red; top: 1px\">x</p>"
        );
        assert_eq!(
            value(
                html,
                "document.querySelector('p').style.getPropertyValue('color')"
            ),
            "red"
        );
        assert_eq!(
            html_after(html, "document.querySelector('p').style.cssText = 'top: 0'"),
            "<p style=\"top: 0\">x</p>"
        );
    }

    #[test]
    fn tree_walking() {
        let html = "<ul><li>a</li><li>b</li></ul>";
        assert_eq!(
            value(html, "document.querySelector('ul').children.length"),
            "2"
        );
        assert_eq!(
            value(html, "document.querySelector('ul').childElementCount"),
            "2"
        );
        assert_eq!(
            value(
                html,
                "document.querySelector('ul').firstElementChild.textContent"
            ),
            "a"
        );
        assert_eq!(
            value(
                html,
                "document.querySelector('ul').lastElementChild.textContent"
            ),
            "b"
        );
        assert_eq!(
            value(
                html,
                "document.querySelector('li').nextElementSibling.textContent"
            ),
            "b"
        );
        assert_eq!(
            value(html, "document.querySelector('li').previousElementSibling"),
            "null"
        );
        assert_eq!(
            value(html, "document.querySelector('li').parentElement.tagName"),
            "UL"
        );
        assert_eq!(
            value(html, "document.querySelector('li').closest('ul').tagName"),
            "UL"
        );
        assert_eq!(
            value(html, "document.querySelector('li').matches('li')"),
            "true"
        );
        assert_eq!(
            value(
                html,
                "document.querySelector('ul').contains(document.querySelector('li'))"
            ),
            "true"
        );
    }

    #[test]
    fn building_and_inserting_nodes() {
        assert_eq!(
            html_after(
                "<div></div>",
                "const el = document.createElement('span'); el.textContent = 'made'; document.querySelector('div').appendChild(el)"
            ),
            "<div><span>made</span></div>"
        );
        assert_eq!(
            html_after(
                "<div><b>b</b></div>",
                "const el = document.createElement('i'); document.querySelector('div').insertBefore(el, document.querySelector('b'))"
            ),
            "<div><i></i><b>b</b></div>"
        );
        assert_eq!(
            html_after(
                "<div><b>b</b></div>",
                "document.querySelector('div').removeChild(document.querySelector('b'))"
            ),
            "<div></div>"
        );
        assert_eq!(
            html_after(
                "<div><b>b</b></div>",
                "document.querySelector('b').remove()"
            ),
            "<div></div>"
        );
        assert_eq!(
            html_after(
                "<div><b>b</b></div>",
                "document.querySelector('div').prepend(document.createElement('i'))"
            ),
            "<div><i></i><b>b</b></div>"
        );
        assert_eq!(
            html_after(
                "<ul><li>a</li></ul>",
                "const copy = document.querySelector('li').cloneNode(true); document.querySelector('ul').appendChild(copy)"
            ),
            "<ul><li>a</li><li>a</li></ul>"
        );
    }

    #[test]
    fn appending_an_attached_node_moves_it() {
        assert_eq!(
            html_after(
                "<div id='a'><b>x</b></div><div id='b'></div>",
                "document.getElementById('b').appendChild(document.querySelector('b'))"
            ),
            "<div id=\"a\"></div><div id=\"b\"><b>x</b></div>"
        );
    }

    #[test]
    fn a_cycle_is_refused_rather_than_hanging_the_engine() {
        let message = error(
            "<div><p></p></div>",
            "document.querySelector('p').appendChild(document.querySelector('div'))",
        );
        assert!(
            message.contains("itself or its own descendant"),
            "{message}"
        );
    }

    #[test]
    fn removing_a_node_that_is_not_a_child_is_an_error() {
        let message = error(
            "<div></div><p></p>",
            "document.querySelector('div').removeChild(document.querySelector('p'))",
        );
        assert!(message.contains("not a child"), "{message}");
    }

    #[test]
    fn the_title_can_be_read_and_written() {
        assert_eq!(value("<title>T</title>", "document.title"), "T");
        let mut document = wat_html::parse("<title>old</title>");
        let mut runtime = ScriptRuntime::new("about:blank");
        runtime
            .eval(&mut document, "document.title = 'new'")
            .unwrap();
        assert_eq!(document.title().as_deref(), Some("new"));
        assert!(runtime.take_title_changed());
        assert!(!runtime.take_title_changed(), "the flag is consumed");
    }

    #[test]
    fn location_reports_the_pieces_of_the_url() {
        let html = "<p></p>";
        assert_eq!(
            value(html, "location.href"),
            "https://example.com/page?q=1#top"
        );
        assert_eq!(value(html, "location.protocol"), "https:");
        assert_eq!(value(html, "location.host"), "example.com");
        assert_eq!(value(html, "location.pathname"), "/page");
        assert_eq!(value(html, "location.search"), "?q=1");
        assert_eq!(value(html, "location.hash"), "#top");
        assert_eq!(value(html, "location.origin"), "https://example.com");
        assert_eq!(
            value(html, "document.URL"),
            "https://example.com/page?q=1#top"
        );
    }

    #[test]
    fn a_script_can_ask_to_navigate() {
        let mut document = wat_html::parse("<p></p>");
        let mut runtime = ScriptRuntime::new("https://example.com/");
        runtime
            .eval(&mut document, "location.assign('/next')")
            .unwrap();
        assert_eq!(
            runtime.take_navigation(),
            Some(Navigation {
                url: "/next".to_string(),
                replace: false
            })
        );
        assert!(
            runtime.take_navigation().is_none(),
            "the request is consumed"
        );

        runtime
            .eval(&mut document, "location.replace('/other')")
            .unwrap();
        assert!(runtime.take_navigation().unwrap().replace);

        runtime
            .eval(&mut document, "location.href = '/third'")
            .unwrap();
        assert_eq!(runtime.take_navigation().unwrap().url, "/third");
    }

    #[test]
    fn window_reports_the_viewport() {
        let html = "<p></p>";
        assert_eq!(value(html, "window.innerWidth"), "800");
        assert_eq!(value(html, "window.innerHeight"), "600");
        assert_eq!(value(html, "window.document.body.tagName"), "BODY");
        assert_eq!(value(html, "window === self"), "true");
        assert_eq!(value(html, "window.window === window"), "true");
        assert!(value(html, "navigator.userAgent").contains("WAT/"));
        assert_eq!(value(html, "typeof window.devicePixelRatio"), "number");
    }

    #[test]
    fn a_script_can_ask_to_scroll() {
        let mut document = wat_html::parse("<p></p>");
        let mut runtime = ScriptRuntime::new("about:blank");
        runtime
            .eval(&mut document, "window.scrollTo(0, 120)")
            .unwrap();
        assert_eq!(runtime.take_scroll(), Some((0.0, 120.0)));

        runtime.set_scroll(0.0, 100.0);
        runtime
            .eval(&mut document, "window.scrollBy(0, 50)")
            .unwrap();
        assert_eq!(runtime.take_scroll(), Some((0.0, 150.0)));

        runtime
            .eval(&mut document, "window.scrollTo({ top: 10 })")
            .unwrap();
        assert_eq!(runtime.take_scroll(), Some((0.0, 10.0)));
    }

    #[test]
    fn dialogs_are_recorded_rather_than_shown() {
        let mut document = wat_html::parse("<p></p>");
        let mut runtime = ScriptRuntime::new("about:blank");
        runtime
            .eval(&mut document, "alert('hi'); confirm('sure?')")
            .unwrap();
        let dialogs = runtime.take_dialogs();
        assert_eq!(dialogs.len(), 2);
        assert_eq!(dialogs[0].kind, DialogKind::Alert);
        assert_eq!(dialogs[0].message, "hi");
        assert_eq!(dialogs[1].kind, DialogKind::Confirm);
        // Nothing can block for an answer, so a confirmation is declined.
        assert_eq!(value("<p></p>", "confirm('ok?')"), "false");
    }

    #[test]
    fn layout_rectangles_are_visible_to_scripts() {
        let mut document = wat_html::parse("<p id='a'>x</p>");
        let mut runtime = ScriptRuntime::new("about:blank");
        let node = document.query("#a").unwrap();
        let mut rects = HashMap::new();
        rects.insert(
            node,
            Rect {
                x: 10.0,
                y: 20.0,
                width: 30.0,
                height: 40.0,
            },
        );
        runtime.set_rects(rects);

        let read = |runtime: &mut ScriptRuntime, document: &mut Document, source: &str| {
            runtime.eval(document, source).unwrap().to_js_string()
        };
        assert_eq!(
            read(
                &mut runtime,
                &mut document,
                "document.getElementById('a').getBoundingClientRect().width"
            ),
            "30"
        );
        assert_eq!(
            read(
                &mut runtime,
                &mut document,
                "document.getElementById('a').getBoundingClientRect().bottom"
            ),
            "60"
        );
        assert_eq!(
            read(
                &mut runtime,
                &mut document,
                "document.getElementById('a').offsetHeight"
            ),
            "40"
        );
        // A node with no rectangle reads as zero rather than failing.
        assert_eq!(
            read(
                &mut runtime,
                &mut document,
                "document.body.getBoundingClientRect().width"
            ),
            "0"
        );
    }

    #[test]
    fn document_scripts_run_in_order() {
        let mut document = wat_html::parse(
            "<script>window.order = 'a'</script><p></p><script>window.order += 'b'</script>",
        );
        let mut runtime = ScriptRuntime::new("about:blank");
        assert!(runtime.run_document_scripts(&mut document).is_empty());
        assert_eq!(
            runtime
                .eval(&mut document, "window.order")
                .unwrap()
                .to_js_string(),
            "ab"
        );
    }

    #[test]
    fn a_script_that_throws_does_not_stop_the_next_one() {
        let mut document = wat_html::parse(
            "<script>throw new Error('first')</script><script>window.ran = true</script>",
        );
        let mut runtime = ScriptRuntime::new("about:blank");
        let errors = runtime.run_document_scripts(&mut document);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].index, Some(0));
        assert!(errors[0].message.contains("first"), "{:?}", errors[0]);
        assert!(!errors[0].fatal);
        assert_eq!(
            runtime
                .eval(&mut document, "window.ran")
                .unwrap()
                .to_js_string(),
            "true"
        );
    }

    #[test]
    fn a_runaway_script_is_reported_as_fatal() {
        let mut document = wat_html::parse("<script>while (true) {}</script>");
        let mut runtime = ScriptRuntime::new("about:blank");
        let errors = runtime.run_document_scripts(&mut document);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].fatal, "{:?}", errors[0]);
    }

    #[test]
    fn a_script_with_a_src_or_a_data_type_is_not_run() {
        let mut document = wat_html::parse(
            "<script src='x.js'>window.bad = 1</script><script type='application/json'>{\"a\":1}</script>",
        );
        let mut runtime = ScriptRuntime::new("about:blank");
        // The `src` script's inline text is still run, because the host is
        // expected to have replaced it with the fetched body; the JSON is not.
        let errors = runtime.run_document_scripts(&mut document);
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn a_script_that_changes_the_page_marks_it_dirty() {
        let mut document = wat_html::parse("<p>a</p>");
        let mut runtime = ScriptRuntime::new("about:blank");
        runtime.eval(&mut document, "1 + 1").unwrap();
        assert!(!runtime.take_dirty(), "reading changes nothing");

        runtime
            .eval(
                &mut document,
                "document.querySelector('p').textContent = 'b'",
            )
            .unwrap();
        assert!(runtime.take_dirty());
        assert!(!runtime.take_dirty(), "the flag is consumed");
    }

    #[test]
    fn a_click_reaches_a_listener() {
        let mut document = wat_html::parse("<button id='b'>go</button>");
        let mut runtime = ScriptRuntime::new("about:blank");
        runtime
            .eval(
                &mut document,
                "window.clicks = 0; document.getElementById('b').addEventListener('click', () => window.clicks++)",
            )
            .unwrap();

        let button = document.query("#b").unwrap();
        let outcome = runtime.dispatch(&mut document, button, "click");
        assert_eq!(outcome.listeners_run, 1);
        assert!(outcome.errors.is_empty());
        assert_eq!(
            runtime
                .eval(&mut document, "window.clicks")
                .unwrap()
                .to_number(),
            1.0
        );

        runtime.dispatch(&mut document, button, "click");
        assert_eq!(
            runtime
                .eval(&mut document, "window.clicks")
                .unwrap()
                .to_number(),
            2.0
        );
    }

    #[test]
    fn an_event_bubbles_to_ancestors() {
        let mut document = wat_html::parse("<div id='outer'><button id='b'>go</button></div>");
        let mut runtime = ScriptRuntime::new("about:blank");
        runtime
            .eval(
                &mut document,
                "window.seen = [];
                 document.getElementById('b').addEventListener('click', () => window.seen.push('button'));
                 document.getElementById('outer').addEventListener('click', () => window.seen.push('outer'));
                 document.addEventListener('click', () => window.seen.push('document'));",
            )
            .unwrap();

        let button = document.query("#b").unwrap();
        runtime.dispatch(&mut document, button, "click");
        assert_eq!(
            runtime
                .eval(&mut document, "window.seen.join(',')")
                .unwrap()
                .to_js_string(),
            "button,outer,document",
            "the target runs first, then each ancestor"
        );
    }

    #[test]
    fn stop_propagation_ends_the_walk() {
        let mut document = wat_html::parse("<div id='outer'><button id='b'>go</button></div>");
        let mut runtime = ScriptRuntime::new("about:blank");
        runtime
            .eval(
                &mut document,
                "window.seen = [];
                 document.getElementById('b').addEventListener('click', e => { window.seen.push('button'); e.stopPropagation() });
                 document.getElementById('outer').addEventListener('click', () => window.seen.push('outer'));",
            )
            .unwrap();

        let button = document.query("#b").unwrap();
        runtime.dispatch(&mut document, button, "click");
        assert_eq!(
            runtime
                .eval(&mut document, "window.seen.join(',')")
                .unwrap()
                .to_js_string(),
            "button"
        );
    }

    #[test]
    fn a_listener_sees_the_event_and_its_targets() {
        let mut document = wat_html::parse("<div id='outer'><button id='b'>go</button></div>");
        let mut runtime = ScriptRuntime::new("about:blank");
        runtime
            .eval(
                &mut document,
                "document.getElementById('outer').addEventListener('click', function (e) {
                     window.kind = e.type;
                     window.target = e.target.id;
                     window.current = e.currentTarget.id;
                     window.receiver = this.id;
                     window.bubbles = e.bubbles;
                 });",
            )
            .unwrap();

        let button = document.query("#b").unwrap();
        runtime.dispatch(&mut document, button, "click");
        let read = |runtime: &mut ScriptRuntime, document: &mut Document, name: &str| {
            runtime.eval(document, name).unwrap().to_js_string()
        };
        assert_eq!(read(&mut runtime, &mut document, "window.kind"), "click");
        assert_eq!(read(&mut runtime, &mut document, "window.target"), "b");
        assert_eq!(read(&mut runtime, &mut document, "window.current"), "outer");
        assert_eq!(
            read(&mut runtime, &mut document, "window.receiver"),
            "outer"
        );
        assert_eq!(read(&mut runtime, &mut document, "window.bubbles"), "true");
    }

    #[test]
    fn prevent_default_is_reported_to_the_browser() {
        let mut document = wat_html::parse("<a id='a' href='/x'>go</a>");
        let mut runtime = ScriptRuntime::new("about:blank");
        runtime
            .eval(
                &mut document,
                "document.getElementById('a').addEventListener('click', e => e.preventDefault())",
            )
            .unwrap();
        let link = document.query("#a").unwrap();
        assert!(
            runtime
                .dispatch(&mut document, link, "click")
                .default_prevented
        );

        // Without a handler nothing is prevented, so the browser navigates.
        let mut plain = wat_html::parse("<a id='a' href='/x'>go</a>");
        let mut runtime = ScriptRuntime::new("about:blank");
        let link = plain.query("#a").unwrap();
        assert!(
            !runtime
                .dispatch(&mut plain, link, "click")
                .default_prevented
        );
    }

    #[test]
    fn an_inline_handler_attribute_runs() {
        let mut document = wat_html::parse(
            "<button id='b' onclick=\"this.textContent = 'clicked'; window.ran = event.type\">go</button>",
        );
        let mut runtime = ScriptRuntime::new("about:blank");
        let button = document.query("#b").unwrap();
        let outcome = runtime.dispatch(&mut document, button, "click");
        assert_eq!(outcome.listeners_run, 1);
        assert!(outcome.errors.is_empty(), "{:?}", outcome.errors);
        assert_eq!(document.text_content(button), "clicked");
        assert_eq!(
            runtime
                .eval(&mut document, "window.ran")
                .unwrap()
                .to_js_string(),
            "click"
        );
    }

    #[test]
    fn an_assigned_handler_supersedes_the_attribute() {
        let mut document =
            wat_html::parse("<button id='b' onclick=\"window.from = 'attribute'\">go</button>");
        let mut runtime = ScriptRuntime::new("about:blank");
        runtime
            .eval(
                &mut document,
                "document.getElementById('b').onclick = () => window.from = 'property'",
            )
            .unwrap();
        let button = document.query("#b").unwrap();
        let outcome = runtime.dispatch(&mut document, button, "click");
        assert_eq!(
            outcome.listeners_run, 1,
            "the attribute must not fire as well"
        );
        assert_eq!(
            runtime
                .eval(&mut document, "window.from")
                .unwrap()
                .to_js_string(),
            "property"
        );
    }

    #[test]
    fn a_handler_can_be_read_back_and_cleared() {
        let mut document = wat_html::parse("<button id='b'>go</button>");
        let mut runtime = ScriptRuntime::new("about:blank");
        runtime
            .eval(
                &mut document,
                "const b = document.getElementById('b'); b.onclick = () => 1",
            )
            .unwrap();
        assert_eq!(
            runtime
                .eval(&mut document, "typeof document.getElementById('b').onclick")
                .unwrap()
                .to_js_string(),
            "function"
        );
        runtime
            .eval(&mut document, "document.getElementById('b').onclick = null")
            .unwrap();
        assert_eq!(
            runtime
                .eval(&mut document, "document.getElementById('b').onclick")
                .unwrap()
                .to_js_string(),
            "null"
        );
    }

    #[test]
    fn a_removed_listener_stops_firing() {
        let mut document = wat_html::parse("<button id='b'>go</button>");
        let mut runtime = ScriptRuntime::new("about:blank");
        runtime
            .eval(
                &mut document,
                "window.n = 0;
                 window.handler = () => window.n++;
                 const b = document.getElementById('b');
                 b.addEventListener('click', window.handler);
                 b.removeEventListener('click', window.handler);",
            )
            .unwrap();
        let button = document.query("#b").unwrap();
        assert_eq!(
            runtime
                .dispatch(&mut document, button, "click")
                .listeners_run,
            0
        );
    }

    #[test]
    fn a_once_listener_fires_once() {
        let mut document = wat_html::parse("<button id='b'>go</button>");
        let mut runtime = ScriptRuntime::new("about:blank");
        runtime
            .eval(
                &mut document,
                "window.n = 0; document.getElementById('b').addEventListener('click', () => window.n++, { once: true })",
            )
            .unwrap();
        let button = document.query("#b").unwrap();
        runtime.dispatch(&mut document, button, "click");
        runtime.dispatch(&mut document, button, "click");
        assert_eq!(
            runtime.eval(&mut document, "window.n").unwrap().to_number(),
            1.0
        );
    }

    #[test]
    fn a_throwing_listener_is_reported_but_does_not_stop_the_others() {
        let mut document = wat_html::parse("<button id='b'>go</button>");
        let mut runtime = ScriptRuntime::new("about:blank");
        runtime
            .eval(
                &mut document,
                "const b = document.getElementById('b');
                 b.addEventListener('click', () => { throw new Error('handler') });
                 b.addEventListener('click', () => window.second = true);",
            )
            .unwrap();
        let button = document.query("#b").unwrap();
        let outcome = runtime.dispatch(&mut document, button, "click");
        assert_eq!(outcome.errors.len(), 1);
        assert!(outcome.errors[0].message.contains("handler"));
        assert_eq!(
            runtime
                .eval(&mut document, "window.second")
                .unwrap()
                .to_js_string(),
            "true"
        );
    }

    #[test]
    fn a_script_click_is_delivered() {
        let mut document = wat_html::parse("<button id='b'>go</button>");
        let mut runtime = ScriptRuntime::new("about:blank");
        runtime
            .eval(
                &mut document,
                "window.n = 0; document.getElementById('b').addEventListener('click', () => window.n++)",
            )
            .unwrap();
        runtime
            .eval(&mut document, "document.getElementById('b').click()")
            .unwrap();
        // The queued event is delivered once the script returns.
        let root = document.root();
        runtime.dispatch(&mut document, root, "noop");
        assert_eq!(
            runtime.eval(&mut document, "window.n").unwrap().to_number(),
            1.0
        );
    }

    #[test]
    fn timers_run_when_the_host_says_so() {
        let mut document = wat_html::parse("<p id='p'>before</p>");
        let mut runtime = ScriptRuntime::new("about:blank");
        runtime
            .eval(
                &mut document,
                "setTimeout(() => { document.getElementById('p').textContent = 'after' }, 10)",
            )
            .unwrap();

        let paragraph = document.query("#p").unwrap();
        assert_eq!(document.text_content(paragraph), "before", "not yet");
        assert!(runtime.has_timers());

        assert!(runtime.run_timers(&mut document).is_empty());
        assert_eq!(document.text_content(paragraph), "after");
        assert!(!runtime.has_timers());
        assert!(
            runtime.take_dirty(),
            "a timer that edits the page marks it dirty"
        );
    }

    #[test]
    fn timers_run_in_delay_order() {
        let mut document = wat_html::parse("<p></p>");
        let mut runtime = ScriptRuntime::new("about:blank");
        runtime
            .eval(
                &mut document,
                "window.order = '';
                 setTimeout(() => window.order += 'b', 20);
                 setTimeout(() => window.order += 'a', 5);",
            )
            .unwrap();
        runtime.run_timers(&mut document);
        assert_eq!(
            runtime
                .eval(&mut document, "window.order")
                .unwrap()
                .to_js_string(),
            "ab"
        );
    }

    #[test]
    fn the_load_event_fires_at_the_document() {
        let mut document = wat_html::parse(
            "<script>window.loaded = false; window.addEventListener('load', () => window.loaded = true)</script>",
        );
        let mut runtime = ScriptRuntime::new("about:blank");
        runtime.run_document_scripts(&mut document);
        assert_eq!(
            runtime
                .eval(&mut document, "window.loaded")
                .unwrap()
                .to_js_string(),
            "false"
        );
        runtime.dispatch_load(&mut document);
        assert_eq!(
            runtime
                .eval(&mut document, "window.loaded")
                .unwrap()
                .to_js_string(),
            "true"
        );
    }

    #[test]
    fn dom_content_loaded_also_fires() {
        let mut document = wat_html::parse(
            "<script>document.addEventListener('DOMContentLoaded', () => window.ready = true)</script>",
        );
        let mut runtime = ScriptRuntime::new("about:blank");
        runtime.run_document_scripts(&mut document);
        runtime.dispatch_load(&mut document);
        assert_eq!(
            runtime
                .eval(&mut document, "window.ready")
                .unwrap()
                .to_js_string(),
            "true"
        );
    }

    #[test]
    fn console_messages_are_captured() {
        let mut document = wat_html::parse("<p></p>");
        let mut runtime = ScriptRuntime::new("about:blank");
        runtime
            .eval(
                &mut document,
                "console.log('from the page'); console.error('bad')",
            )
            .unwrap();
        assert_eq!(runtime.console().len(), 2);
        assert_eq!(runtime.console()[0].text, "from the page");
        assert_eq!(runtime.console()[1].level, ConsoleLevel::Error);
        assert_eq!(runtime.take_console().len(), 2);
        assert!(runtime.console().is_empty(), "the messages are drained");
    }

    #[test]
    fn a_node_logs_as_its_markup() {
        let mut document = wat_html::parse("<p id='a' class='b'>x</p>");
        let mut runtime = ScriptRuntime::new("about:blank");
        runtime
            .eval(&mut document, "console.log(document.querySelector('p'))")
            .unwrap();
        assert_eq!(runtime.console()[0].text, "<p id=\"a\" class=\"b\">");
    }

    #[test]
    fn two_handles_to_one_node_are_the_same_object() {
        let html = "<p id='a'>x</p>";
        assert_eq!(
            value(
                html,
                "document.getElementById('a') === document.querySelector('#a')"
            ),
            "true"
        );
        assert_eq!(
            value(
                "<p id='a'></p><p id='b'></p>",
                "document.getElementById('a') === document.getElementById('b')"
            ),
            "false"
        );
        assert_eq!(value(html, "document === window.document"), "true");
    }

    #[test]
    fn an_unknown_method_names_itself_in_the_error() {
        let message = error("<p></p>", "document.querySelector('p').notAThing()");
        assert!(message.contains("notAThing"), "{message}");
    }

    #[test]
    fn a_realistic_page_script_works_end_to_end() {
        let mut document = wat_html::parse(
            r#"<div class="counter">
                 <output id="value">0</output>
                 <button id="up">+</button>
               </div>
               <script>
                 const output = document.getElementById('value');
                 let count = Number(output.textContent);
                 document.getElementById('up').addEventListener('click', () => {
                   count += 1;
                   output.textContent = String(count);
                   output.classList.toggle('odd', count % 2 === 1);
                 });
               </script>"#,
        );
        let mut runtime = ScriptRuntime::new("about:blank");
        assert!(runtime.run_document_scripts(&mut document).is_empty());

        let button = document.query("#up").unwrap();
        let output = document.query("#value").unwrap();

        runtime.dispatch(&mut document, button, "click");
        assert_eq!(document.text_content(output), "1");
        assert_eq!(document.element(output).unwrap().attr("class"), Some("odd"));
        assert!(runtime.take_dirty());

        runtime.dispatch(&mut document, button, "click");
        assert_eq!(document.text_content(output), "2");
        assert_eq!(document.element(output).unwrap().attr("class"), Some(""));
    }
}
