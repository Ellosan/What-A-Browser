//! The WAT engine: the pipeline that turns an address into pixels, plus the
//! tab and history model built on top of it.
//!
//! ```
//! use std::rc::Rc;
//! use wat_engine::{Session, WebEngine};
//! use wat_layout::Size2D;
//!
//! let loader = wat_net::StaticLoader::new()
//!     .with_html("https://example.com/", "<title>Hi</title><h1>Hello</h1>");
//! let mut session = Session::new(
//!     Rc::new(wat_text::FontStore::empty()),
//!     wat_theme::Theme::default().resolve(false),
//!     Size2D::new(800.0, 600.0),
//!     false,
//! );
//! session.open_tab("https://example.com/", &loader);
//! assert_eq!(session.active().unwrap().label(), "Hi");
//! ```
//!
//! # Engine independence
//!
//! [`WebEngine`] is the seam between the browser shell and whatever renders web
//! content. [`Page`] is this repository's implementation — an engine written from
//! scratch, sharing no code with Chromium, WebKit or Gecko. A different backend
//! only has to implement the same trait for the shell to drive it.

pub mod about;
pub mod engine;
pub mod page;
pub mod session;

pub use about::{ENGINE_NAME, VERSION};
pub use engine::WatEngine;
pub use page::{Page, PageImages, PageState};
pub use session::{History, Session, Tab, TabId};

use wat_layout::geom::{Point, Size2D};
use wat_net::Address;
use wat_paint::{Canvas, DisplayList};

/// What the browser shell needs from a web engine.
///
/// Implemented here by [`Page`]. The shell never reaches past this trait for
/// content rendering, so a different engine can be substituted wholesale.
pub trait WebEngine {
    /// The address currently displayed.
    fn address(&self) -> &Address;

    /// The document title, if it has one.
    fn title(&self) -> Option<&str>;

    /// Size of the rendered document, which may exceed the viewport.
    fn content_size(&self) -> Size2D;

    /// Current scroll offset.
    fn scroll(&self) -> Point;

    /// Scrolls by a delta; returns whether anything moved.
    fn scroll_by(&mut self, dx: f32, dy: f32) -> bool;

    /// Tells the engine how much room it has.
    fn resize(&mut self, viewport: Size2D, coarse_pointer: bool);

    /// The colour to clear the content area with.
    fn background(&self) -> wat_css::Color;

    /// Builds a display list for the current state.
    fn frame(&self) -> DisplayList;

    /// Reports the pointer position, for `:hover`. Returns whether to repaint.
    fn pointer_moved(&mut self, position: Option<Point>) -> bool;

    /// The link at a viewport point, if any.
    fn link_at(&self, point: Point) -> Option<String>;

    /// The cursor the content wants at a viewport point.
    fn cursor_at(&self, point: Point) -> wat_style::Cursor;

    /// Renders into a canvas of the viewport's size.
    fn render(&self) -> Canvas;
}

impl WebEngine for Page {
    fn address(&self) -> &Address {
        &self.address
    }

    fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    fn content_size(&self) -> Size2D {
        self.document_size()
    }

    fn scroll(&self) -> Point {
        self.scroll_offset()
    }

    fn scroll_by(&mut self, dx: f32, dy: f32) -> bool {
        Page::scroll_by(self, dx, dy)
    }

    fn resize(&mut self, viewport: Size2D, coarse_pointer: bool) {
        self.set_viewport(viewport, coarse_pointer);
    }

    fn background(&self) -> wat_css::Color {
        self.background_color()
    }

    fn frame(&self) -> DisplayList {
        self.display_list()
    }

    fn pointer_moved(&mut self, position: Option<Point>) -> bool {
        self.set_hover(position)
    }

    fn link_at(&self, point: Point) -> Option<String> {
        Page::link_at(self, point)
    }

    fn cursor_at(&self, point: Point) -> wat_style::Cursor {
        Page::cursor_at(self, point)
    }

    fn render(&self) -> Canvas {
        self.render_to_canvas()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;
    use wat_net::StaticLoader;
    use wat_text::FontStore;
    use wat_theme::Theme;

    fn page(html: &str) -> Page {
        Page::from_html(
            Address::parse("https://example.com/").unwrap(),
            html,
            Rc::new(FontStore::empty()),
            Theme::default().resolve(false),
            Size2D::new(400.0, 300.0),
        )
    }

    #[test]
    fn the_engine_trait_covers_the_shell_surface() {
        // Exercised through the trait so the seam is actually used.
        let mut engine: Box<dyn WebEngine> = Box::new(page(
            "<title>T</title><div style=\"height:2000px;background:#eee\">x</div>",
        ));

        assert_eq!(engine.title(), Some("T"));
        assert_eq!(engine.address().url(), "https://example.com/");
        assert!(engine.content_size().height >= 2000.0);
        assert_eq!(engine.scroll(), Point::ZERO);
        assert!(!engine.frame().is_empty());
        assert!(engine.frame().is_balanced());

        assert!(engine.scroll_by(0.0, 100.0));
        assert_eq!(engine.scroll().y, 100.0);
        assert!(
            !engine.frame().is_empty(),
            "the tall div is still on screen after scrolling"
        );

        engine.resize(Size2D::new(200.0, 200.0), true);
        let canvas = engine.render();
        assert_eq!(canvas.width(), 200);
        assert_eq!(canvas.height(), 200);
    }

    #[test]
    fn the_trait_exposes_links_and_cursors() {
        let engine = page(
            "<body style=\"margin:0\"><a href=\"/next\" \
             style=\"display:block;width:100px;height:40px\">go</a></body>",
        );
        let point = Point::new(10.0, 10.0);
        assert_eq!(
            WebEngine::link_at(&engine, point).as_deref(),
            Some("https://example.com/next")
        );
        assert_eq!(
            WebEngine::cursor_at(&engine, point),
            wat_style::Cursor::Pointer
        );
    }

    #[test]
    fn a_session_drives_the_engine_end_to_end() {
        let loader = StaticLoader::new()
            .with_html(
                "https://example.com/",
                "<title>Home</title><a href=\"/two\">two</a>",
            )
            .with_html("https://example.com/two", "<title>Two</title><p>second</p>");

        let mut session = Session::new(
            Rc::new(FontStore::empty()),
            Theme::default().resolve(false),
            Size2D::new(800.0, 600.0),
            false,
        );
        session.open_tab("https://example.com/", &loader);
        assert_eq!(session.active().unwrap().label(), "Home");

        session.follow_link("https://example.com/two", &loader);
        assert_eq!(session.active().unwrap().label(), "Two");
        assert!(session.go_back(&loader));
        assert_eq!(session.active().unwrap().label(), "Home");
    }

    #[test]
    fn version_metadata_is_populated() {
        assert!(!VERSION.is_empty());
        assert_eq!(ENGINE_NAME, "WAT Engine");
    }
}
