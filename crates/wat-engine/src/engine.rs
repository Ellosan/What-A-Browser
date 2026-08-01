//! WAT's own engine, behind the [`Engine`] seam.
//!
//! [`Session`] predates the seam and is still the thing the rest of this crate
//! is written against, so this is a wrapper rather than a rewrite. What it adds
//! is ownership of the loader: the shell used to hold one and pass it into every
//! call, which only worked because WAT's networking happens to be a small trait.
//! An engine that brings its own network stack cannot be driven that way, so the
//! loader lives here now.

use std::rc::Rc;

use wat_layout::geom::{Point, Rect, Size2D};
use wat_net::Loader;
use wat_paint::{Canvas, Clip, Renderer, RoundedRect};
use wat_text::FontStore;
use wat_theme::ResolvedTheme;
use wat_web::{Engine, NavigationError, TabView};

use crate::session::{Session, TabId};

/// The WAT engine: `wat-html` through `wat-paint`, driven by [`Session`].
pub struct WatEngine {
    session: Session,
    loader: Box<dyn Loader>,
    fonts: Rc<FontStore>,
}

impl WatEngine {
    pub fn new(
        fonts: Rc<FontStore>,
        loader: Box<dyn Loader>,
        theme: ResolvedTheme,
        viewport: Size2D,
        coarse_pointer: bool,
    ) -> Self {
        WatEngine {
            session: Session::new(fonts.clone(), theme, viewport, coarse_pointer),
            loader,
            fonts,
        }
    }

    /// The session underneath.
    ///
    /// The seam does not cover everything WAT's engine can do — the theme
    /// inspector and the tests reach past it — and hiding it would mean widening
    /// the trait with things only one engine could answer.
    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn session_mut(&mut self) -> &mut Session {
        &mut self.session
    }

    pub fn set_search_template(&mut self, template: impl Into<String>) {
        self.session.search_template = template.into();
    }

    pub fn set_home_url(&mut self, url: impl Into<String>) {
        self.session.home_url = url.into();
    }

    pub fn home_url(&self) -> &str {
        &self.session.home_url
    }
}

impl Engine for WatEngine {
    fn tabs(&self) -> Vec<TabView> {
        self.session
            .tabs()
            .iter()
            .map(|tab| TabView {
                id: tab.id.value(),
                label: tab.label(),
                url: tab.url().to_string(),
                is_secure: tab.is_secure(),
                failed: tab.failed(),
            })
            .collect()
    }

    fn active_index(&self) -> usize {
        self.session.active_index()
    }

    fn tab_count(&self) -> usize {
        self.session.tab_count()
    }

    fn open_tab(&mut self, input: &str) -> u64 {
        self.session.open_tab(input, self.loader.as_ref()).value()
    }

    fn open_tab_in_background(&mut self, url: &str) -> u64 {
        let before = self.session.active_index();
        self.session
            .open_link_in_background(url, self.loader.as_ref());
        debug_assert_eq!(before, self.session.active_index());
        self.session
            .tabs()
            .last()
            .map(|tab| tab.id.value())
            .unwrap_or_default()
    }

    fn close_tab(&mut self, id: u64) {
        self.session
            .close_tab(TabId::from_value(id), self.loader.as_ref());
    }

    fn close_active_tab(&mut self) {
        self.session.close_active_tab(self.loader.as_ref());
    }

    fn select_tab(&mut self, index: usize) -> bool {
        self.session.select_tab(index)
    }

    fn navigate(&mut self, input: &str) -> Result<(), NavigationError> {
        self.session
            .navigate(input, self.loader.as_ref())
            .map_err(|error| NavigationError::Failed(error.to_string()))
    }

    fn follow_link(&mut self, url: &str) {
        self.session.follow_link(url, self.loader.as_ref());
    }

    fn reload(&mut self) {
        self.session.reload(self.loader.as_ref());
    }

    fn go_back(&mut self) -> bool {
        self.session.go_back(self.loader.as_ref())
    }

    fn go_forward(&mut self) -> bool {
        self.session.go_forward(self.loader.as_ref())
    }

    fn can_go_back(&self) -> bool {
        self.session
            .active()
            .is_some_and(|tab| tab.history.can_go_back())
    }

    fn can_go_forward(&self) -> bool {
        self.session
            .active()
            .is_some_and(|tab| tab.history.can_go_forward())
    }

    fn take_requested_navigation(&mut self) -> Option<String> {
        self.session
            .active_mut()
            .and_then(|tab| tab.page.take_script_navigation())
            .map(|request| request.url)
    }

    fn link_at(&self, point: Point) -> Option<String> {
        self.session.link_at(point)
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.session.scroll_active(dx, dy)
    }

    fn scroll_offset(&self) -> Point {
        self.session
            .active()
            .map(|tab| tab.page.scroll_offset())
            .unwrap_or(Point::new(0.0, 0.0))
    }

    fn set_viewport(&mut self, viewport: Size2D, coarse_pointer: bool) {
        self.session.set_viewport(viewport, coarse_pointer);
    }

    fn set_theme(&mut self, theme: ResolvedTheme) {
        self.session.set_theme(theme);
    }

    fn background_color(&self) -> wat_css::Color {
        self.session
            .active()
            .map(|tab| tab.page.background_color())
            .unwrap_or(wat_css::Color::TRANSPARENT)
    }

    fn paint(&self, canvas: &mut Canvas, area: Rect, corner_radius: f32, scale: f32) {
        let Some(tab) = self.session.active() else {
            return;
        };
        if area.is_empty() {
            return;
        }
        // The page lays out in CSS pixels and the canvas is in device pixels, so
        // the finished list is scaled at the last moment — the same treatment the
        // chrome gets, which is what keeps text rasterized at its real size.
        let list = tab.page.display_list_at(area.origin()).scaled(scale);
        let shape = RoundedRect::new(
            Rect::new(
                area.x * scale,
                area.y * scale,
                area.width * scale,
                area.height * scale,
            ),
            wat_style::Corners::all(corner_radius * scale),
        );
        let mut clip = Clip::from_rect(canvas.bounds());
        clip.push(shape);
        Renderer::new(&self.fonts).render_clipped(&list, canvas, &clip);
    }

    fn has_pending_work(&self) -> bool {
        self.session
            .active()
            .is_some_and(|tab| tab.page.has_timers())
    }

    fn run_pending_work(&mut self) -> bool {
        match self.session.active_mut() {
            Some(tab) if tab.page.has_timers() => {
                tab.page.run_timers();
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wat_net::StaticLoader;
    use wat_theme::Theme;

    fn engine(html: &str) -> WatEngine {
        let loader = StaticLoader::new().with_html("https://example.com/", html);
        let mut engine = WatEngine::new(
            Rc::new(FontStore::empty()),
            Box::new(loader),
            Theme::default().resolve(false),
            Size2D::new(800.0, 600.0),
            false,
        );
        engine.open_tab("https://example.com/");
        engine
    }

    #[test]
    fn tabs_are_reported_through_the_seam() {
        let engine = engine("<title>Hello</title><p>hi</p>");
        let tabs = engine.tabs();
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].url, "https://example.com/");
        assert_eq!(tabs[0].label, "Hello");
        assert!(tabs[0].is_secure, "https is secure");
        assert!(!tabs[0].failed);
        assert_eq!(engine.active_index(), 0);
        assert_eq!(engine.tab_count(), 1);
    }

    #[test]
    fn history_is_visible_before_and_after_going_back() {
        let loader = StaticLoader::new()
            .with_html("https://example.com/", "<p>one</p>")
            .with_html("https://example.com/two", "<p>two</p>");
        let mut engine = WatEngine::new(
            Rc::new(FontStore::empty()),
            Box::new(loader),
            Theme::default().resolve(false),
            Size2D::new(800.0, 600.0),
            false,
        );
        engine.open_tab("https://example.com/");
        assert!(!engine.can_go_back());

        engine.follow_link("https://example.com/two");
        assert!(engine.can_go_back());
        assert!(!engine.can_go_forward());

        assert!(engine.go_back());
        assert!(engine.can_go_forward());
        assert_eq!(engine.tabs()[0].url, "https://example.com/");
    }

    #[test]
    fn painting_stays_inside_the_area_it_was_given() {
        let engine = engine("<body style=\"background:#f00\"><p>hi</p></body>");
        let mut canvas = Canvas::new(200, 200);
        // A page painted into the bottom-right quarter must not touch the rest,
        // because the chrome is composited over the top afterwards.
        engine.paint(&mut canvas, Rect::new(100.0, 100.0, 100.0, 100.0), 0.0, 1.0);
        assert_eq!(canvas.pixel(10, 10).a, 0, "outside the area");
        assert_eq!(canvas.pixel(150, 50).a, 0, "above the area");
        assert_ne!(canvas.pixel(150, 150).a, 0, "inside the area");
    }

    #[test]
    fn a_background_tab_does_not_steal_focus() {
        let loader = StaticLoader::new()
            .with_html("https://example.com/", "<p>one</p>")
            .with_html("https://example.com/two", "<p>two</p>");
        let mut engine = WatEngine::new(
            Rc::new(FontStore::empty()),
            Box::new(loader),
            Theme::default().resolve(false),
            Size2D::new(800.0, 600.0),
            false,
        );
        engine.open_tab("https://example.com/");
        engine.open_tab_in_background("https://example.com/two");
        assert_eq!(engine.tab_count(), 2);
        assert_eq!(engine.active_index(), 0, "the first tab is still in front");
        assert_eq!(engine.tabs()[0].url, "https://example.com/");
    }
}
