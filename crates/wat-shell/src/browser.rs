//! The browser as a state machine, with no windowing system attached.
//!
//! Every user gesture the shell can receive lands here, and everything the
//! window needs to draw comes back out. Keeping the platform out means the whole
//! interaction model is testable without a display.

use std::rc::Rc;

use wat_engine::Session;
use wat_layout::geom::{Point, Size2D};
use wat_net::{HttpLoader, Loader, OfflineLoader, RequestOptions};
use wat_paint::Canvas;
use wat_style::Cursor;
use wat_text::FontStore;
use wat_theme::{Appearance, Theme};
use wat_ui::{page_viewport, render_window_into, Chrome, Key, Modifiers, UiAction};

/// How to start the browser.
#[derive(Clone, Debug)]
pub struct ShellConfig {
    /// Address to open, or `None` for the home page.
    pub url: Option<String>,
    pub size: Size2D,
    /// Theme preset name or path.
    pub theme: String,
    /// Overrides the theme's own appearance setting.
    pub appearance: Option<Appearance>,
    pub offline: bool,
    /// Treat the display as a touch screen.
    pub touch: bool,
    pub search: String,
    pub home: String,
}

impl Default for ShellConfig {
    fn default() -> Self {
        ShellConfig {
            url: None,
            size: Size2D::new(1280.0, 800.0),
            theme: "liquid-glass".to_string(),
            appearance: None,
            offline: false,
            touch: false,
            search: "https://duckduckgo.com/?q={}".to_string(),
            home: "about:home".to_string(),
        }
    }
}

impl ShellConfig {
    /// Defaults suited to a phone.
    pub fn mobile() -> Self {
        ShellConfig {
            size: Size2D::new(390.0, 844.0),
            touch: true,
            ..Default::default()
        }
    }
}

/// The whole browser, minus the window.
pub struct Browser {
    pub session: Session,
    pub chrome: Chrome,
    pub fonts: Rc<FontStore>,
    loader: Box<dyn Loader>,
    theme: Theme,
    dark: bool,
    /// Where the pointer is, in window coordinates.
    pointer: Option<Point>,
    pub needs_redraw: bool,
    pub should_quit: bool,
}

impl Browser {
    /// Builds a browser from `config`, opening its first tab.
    pub fn new(config: &ShellConfig) -> Result<Browser, String> {
        let theme = Theme::named(&config.theme)?;
        let theme = match config.appearance {
            Some(appearance) => Theme {
                appearance,
                ..theme
            },
            None => theme,
        };
        // Without a platform hook for the system preference, `auto` means light.
        let dark = theme.appearance.prefers_dark(false);
        let resolved = theme.resolve(dark);

        let fonts = Rc::new(FontStore::new());
        let chrome = Chrome::new(resolved.clone(), config.size);
        let mut session = Session::new(
            fonts.clone(),
            resolved,
            page_viewport(&chrome),
            config.touch,
        );
        session.search_template = config.search.clone();
        session.home_url = config.home.clone();

        let loader: Box<dyn Loader> = if config.offline {
            Box::new(OfflineLoader)
        } else {
            Box::new(HttpLoader::new(RequestOptions::default()))
        };

        let mut browser = Browser {
            session,
            chrome,
            fonts,
            loader,
            theme,
            dark,
            pointer: None,
            needs_redraw: true,
            should_quit: false,
        };

        let start = config.url.clone().unwrap_or_else(|| config.home.clone());
        browser.session.open_tab(&start, browser.loader.as_ref());
        browser.after_navigation();
        Ok(browser)
    }

    /// The window title.
    pub fn title(&self) -> String {
        match self.session.active() {
            Some(tab) => format!("{} — What-A-Browser", tab.label()),
            None => "What-A-Browser".to_string(),
        }
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn is_dark(&self) -> bool {
        self.dark
    }

    /// Syncs the chrome with the session after anything that can change the URL,
    /// the tab list or the page.
    fn after_navigation(&mut self) {
        self.chrome.relayout(self.session.tab_count());
        self.session.set_viewport(
            page_viewport(&self.chrome),
            self.chrome.layout().is_mobile(),
        );
        if let Some(tab) = self.session.active() {
            let url = tab.url().to_string();
            self.chrome.omnibox.set_url(url);
        }
        self.chrome.status = None;
        self.needs_redraw = true;
    }

    /// Applies a resize, in logical pixels.
    pub fn resize(&mut self, size: Size2D) {
        if (size.width - self.chrome.size().width).abs() < 0.5
            && (size.height - self.chrome.size().height).abs() < 0.5
        {
            return;
        }
        self.chrome.resize(size, self.session.tab_count());
        self.session.set_viewport(
            page_viewport(&self.chrome),
            self.chrome.layout().is_mobile(),
        );
        self.needs_redraw = true;
    }

    /// The last known pointer position, in window coordinates.
    ///
    /// Platforms report button presses without a position, so the shell needs
    /// the position from the most recent move event.
    pub fn pointer_position(&self) -> Option<Point> {
        self.pointer
    }

    /// The link under the pointer, if it is over the page.
    fn link_under_pointer(&self) -> Option<String> {
        let point = self.pointer?;
        let content = self.chrome.content_rect();
        if !content.contains(point) {
            return None;
        }
        let local = Point::new(point.x - content.x, point.y - content.y);
        self.session.link_at(local)
    }

    pub fn pointer_moved(&mut self, point: Point) {
        self.pointer = Some(point);
        let mut redraw = self.chrome.pointer_moved(Some(point));

        let content = self.chrome.content_rect();
        let inside = content.contains(point);
        let local = inside.then(|| Point::new(point.x - content.x, point.y - content.y));
        if let Some(tab) = self.session.active_mut() {
            redraw |= tab.page.set_hover(local);
        }

        // The status pill mirrors the link the pointer is over.
        let status = self.link_under_pointer();
        if status != self.chrome.status {
            self.chrome.status = status;
            redraw = true;
        }
        self.needs_redraw |= redraw;
    }

    pub fn pointer_left(&mut self) {
        self.pointer = None;
        let mut redraw = self.chrome.pointer_moved(None);
        if let Some(tab) = self.session.active_mut() {
            redraw |= tab.page.set_hover(None);
        }
        if self.chrome.status.take().is_some() {
            redraw = true;
        }
        self.needs_redraw |= redraw;
    }

    pub fn pointer_down(&mut self, point: Point) {
        self.pointer = Some(point);
        self.needs_redraw |= self.chrome.pointer_down(point);
    }

    pub fn pointer_up(&mut self, point: Point) {
        self.pointer = Some(point);
        let mut link = self.link_under_pointer();

        // The page sees the click before the browser acts on it. A script may
        // handle it and call `preventDefault`, in which case the link is not
        // followed — which is how a single-page app works at all.
        let content = self.chrome.content_rect();
        if content.contains(point) {
            let local = Point::new(point.x - content.x, point.y - content.y);
            if let Some(tab) = self.session.active_mut() {
                if tab.page.dispatch_click_at(local) {
                    link = None;
                }
            }
            self.needs_redraw = true;
            self.follow_script_navigation();
        }

        let action = self.chrome.pointer_up(point, link.as_deref());
        self.needs_redraw = true;
        if let Some(action) = action {
            self.apply(action);
        }
    }

    /// Goes wherever a script asked to go.
    fn follow_script_navigation(&mut self) {
        let request = self
            .session
            .active_mut()
            .and_then(|tab| tab.page.take_script_navigation());
        if let Some(request) = request {
            self.apply(UiAction::OpenUrl(request.url));
        }
    }

    /// Runs any timer callbacks that scripts have queued.
    ///
    /// The shell calls this from its event loop, which is what keeps a page's
    /// `setTimeout` work on the same thread as everything else.
    pub fn run_script_timers(&mut self) {
        let ran = match self.session.active_mut() {
            Some(tab) if tab.page.has_timers() => {
                tab.page.run_timers();
                true
            }
            _ => false,
        };
        if ran {
            self.needs_redraw = true;
            self.follow_script_navigation();
        }
    }

    /// Whether a page is waiting on a timer, so the shell knows to keep pumping
    /// instead of going idle.
    pub fn has_pending_script_work(&self) -> bool {
        self.session
            .active()
            .is_some_and(|tab| tab.page.has_timers())
    }

    pub fn middle_click(&mut self, point: Point) {
        let link = self.link_under_pointer();
        if let Some(action) = self.chrome.middle_click(point, link.as_deref()) {
            self.apply(action);
        }
    }

    pub fn key_pressed(&mut self, key: Key, modifiers: Modifiers) {
        if let Some(action) = self.chrome.key_pressed(key, modifiers) {
            self.apply(action);
        }
        // Typing changes the omnibox, so a repaint is always warranted.
        self.needs_redraw = true;
    }

    /// A wheel or trackpad scroll at `point`, in logical pixels.
    pub fn scroll(&mut self, point: Point, delta_y: f32) {
        if let Some(action) = self.chrome.scroll(point, delta_y) {
            self.apply(action);
        }
    }

    /// The cursor the window should show.
    pub fn cursor(&self) -> Cursor {
        let Some(point) = self.pointer else {
            return Cursor::Default;
        };
        let content = self.chrome.content_rect();
        let page_cursor = if content.contains(point) {
            self.session
                .active()
                .map(|tab| {
                    tab.page
                        .cursor_at(Point::new(point.x - content.x, point.y - content.y))
                })
                .unwrap_or(Cursor::Auto)
        } else {
            Cursor::Auto
        };
        self.chrome.cursor_at(point, page_cursor)
    }

    /// Applies one UI action.
    pub fn apply(&mut self, action: UiAction) {
        let loader = &self.loader;
        match action {
            UiAction::GoBack => {
                self.session.go_back(loader.as_ref());
                self.after_navigation();
            }
            UiAction::GoForward => {
                self.session.go_forward(loader.as_ref());
                self.after_navigation();
            }
            UiAction::Reload => {
                self.session.reload(loader.as_ref());
                self.after_navigation();
            }
            UiAction::GoHome => {
                let home = self.session.home_url.clone();
                let _ = self.session.navigate(&home, loader.as_ref());
                self.after_navigation();
            }
            UiAction::NewTab => {
                let home = self.session.home_url.clone();
                self.session.open_tab(&home, loader.as_ref());
                self.after_navigation();
                self.chrome.omnibox.focus();
            }
            UiAction::CloseActiveTab => {
                self.session.close_active_tab(loader.as_ref());
                self.after_navigation();
            }
            UiAction::CloseTab(index) => {
                if let Some(id) = self.session.tabs().get(index).map(|tab| tab.id) {
                    self.session.close_tab(id, loader.as_ref());
                }
                self.after_navigation();
            }
            UiAction::SelectTab(index) => {
                if self.session.select_tab(index) {
                    self.after_navigation();
                }
            }
            UiAction::Navigate(input) => {
                if let Err(error) = self.session.navigate(&input, loader.as_ref()) {
                    log::warn!("cannot open {input}: {error}");
                }
                self.after_navigation();
            }
            UiAction::OpenUrl(url) => {
                self.session.follow_link(&url, loader.as_ref());
                self.after_navigation();
            }
            UiAction::OpenUrlInNewTab(url) => {
                self.session.open_link_in_background(&url, loader.as_ref());
                self.chrome.relayout(self.session.tab_count());
                self.needs_redraw = true;
            }
            UiAction::ToggleAppearance => self.toggle_appearance(),
            UiAction::ScrollContent(delta) => {
                if self.session.scroll_active(0.0, delta) {
                    self.needs_redraw = true;
                }
            }
            UiAction::Quit => self.should_quit = true,
        }
    }

    /// Switches between the light and dark variants of the current theme.
    pub fn toggle_appearance(&mut self) {
        self.dark = !self.dark;
        let resolved = self.theme.resolve(self.dark);
        self.chrome.set_theme(resolved.clone());
        self.session.set_theme(resolved);
        self.chrome.relayout(self.session.tab_count());
        self.session.set_viewport(
            page_viewport(&self.chrome),
            self.chrome.layout().is_mobile(),
        );
        self.needs_redraw = true;
    }

    /// Replaces the theme entirely, keeping the current appearance.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
        let resolved = self.theme.resolve(self.dark);
        self.chrome.set_theme(resolved.clone());
        self.session.set_theme(resolved);
        self.after_navigation();
    }

    /// Renders the window into `canvas`, which must be the window's size.
    pub fn render_into(&mut self, canvas: &mut Canvas) {
        render_window_into(&self.chrome, &self.session, &self.fonts, canvas);
        self.needs_redraw = false;
    }

    /// Renders the window into a fresh canvas.
    pub fn render(&mut self) -> Canvas {
        let size = self.chrome.size();
        let mut canvas = Canvas::new(size.width.max(1.0) as u32, size.height.max(1.0) as u32);
        self.render_into(&mut canvas);
        canvas
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wat_ui::WidgetId;

    fn browser() -> Browser {
        Browser::new(&ShellConfig {
            offline: true,
            size: Size2D::new(1000.0, 700.0),
            ..Default::default()
        })
        .expect("the offline browser starts")
    }

    #[test]
    fn a_new_browser_opens_the_home_page() {
        let browser = browser();
        assert_eq!(browser.session.tab_count(), 1);
        assert_eq!(browser.session.active().unwrap().url(), "about:home");
        assert!(browser.title().contains("What-A-Browser"));
        assert!(browser.needs_redraw);
    }

    #[test]
    fn a_start_url_is_honoured() {
        let browser = Browser::new(&ShellConfig {
            offline: true,
            url: Some("about:version".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(browser.session.active().unwrap().url(), "about:version");
    }

    #[test]
    fn an_unknown_theme_fails_to_start_with_a_message() {
        let error = match Browser::new(&ShellConfig {
            theme: "no-such-theme".into(),
            ..Default::default()
        }) {
            Ok(_) => panic!("an unknown theme should not start"),
            Err(error) => error,
        };
        assert!(error.contains("unknown theme"), "got: {error}");
    }

    #[test]
    fn the_page_viewport_matches_the_content_area() {
        let browser = browser();
        let content = browser.chrome.content_rect();
        assert_eq!(
            browser.session.active().unwrap().page.viewport().width,
            content.width
        );
    }

    #[test]
    fn resizing_flows_through_to_the_page() {
        let mut browser = browser();
        browser.resize(Size2D::new(500.0, 900.0));
        assert!(browser.chrome.layout().is_mobile());
        let content = browser.chrome.content_rect();
        assert_eq!(
            browser.session.active().unwrap().page.viewport().height,
            content.height
        );
    }

    #[test]
    fn a_no_op_resize_is_ignored() {
        let mut browser = browser();
        browser.needs_redraw = false;
        browser.resize(browser.chrome.size());
        assert!(!browser.needs_redraw);
    }

    #[test]
    fn new_tab_opens_and_focuses_the_address_bar() {
        let mut browser = browser();
        browser.apply(UiAction::NewTab);
        assert_eq!(browser.session.tab_count(), 2);
        assert!(browser.chrome.omnibox.is_focused());
    }

    #[test]
    fn closing_a_tab_keeps_one_open() {
        let mut browser = browser();
        browser.apply(UiAction::NewTab);
        browser.apply(UiAction::CloseActiveTab);
        assert_eq!(browser.session.tab_count(), 1);
        browser.apply(UiAction::CloseActiveTab);
        assert_eq!(browser.session.tab_count(), 1, "never zero tabs");
    }

    #[test]
    fn navigation_updates_the_omnibox() {
        let mut browser = browser();
        browser.apply(UiAction::Navigate("about:version".into()));
        assert_eq!(browser.session.active().unwrap().url(), "about:version");
        assert_eq!(browser.chrome.omnibox.visible_text(), "about:version");
    }

    #[test]
    fn a_rejected_address_leaves_the_tab_alone() {
        let mut browser = browser();
        browser.apply(UiAction::Navigate("ftp://files.example/".into()));
        assert_eq!(browser.session.active().unwrap().url(), "about:home");
    }

    #[test]
    fn back_and_forward_work_through_actions() {
        let mut browser = browser();
        browser.apply(UiAction::Navigate("about:version".into()));
        browser.apply(UiAction::GoBack);
        assert_eq!(browser.session.active().unwrap().url(), "about:home");
        browser.apply(UiAction::GoForward);
        assert_eq!(browser.session.active().unwrap().url(), "about:version");
    }

    #[test]
    fn toggling_the_appearance_reaches_the_chrome_and_the_page() {
        let mut browser = browser();
        assert!(!browser.is_dark());
        let light_canvas = browser.render();

        browser.toggle_appearance();
        assert!(browser.is_dark());
        assert!(browser.chrome.theme().dark);
        let dark_canvas = browser.render();

        assert_ne!(light_canvas.pixels(), dark_canvas.pixels());
        // The window background follows the theme.
        assert!(dark_canvas.pixel(2, 2).luminance() < light_canvas.pixel(2, 2).luminance());
    }

    #[test]
    fn switching_themes_keeps_the_appearance() {
        let mut browser = browser();
        browser.toggle_appearance();
        browser.set_theme(Theme::from_toml("name = \"Test\"").unwrap());
        assert!(browser.is_dark());
        assert_eq!(browser.chrome.theme().name, "Test");
    }

    #[test]
    fn quitting_sets_the_flag() {
        let mut browser = browser();
        assert!(!browser.should_quit);
        browser.apply(UiAction::Quit);
        assert!(browser.should_quit);
    }

    #[test]
    fn keyboard_accelerators_reach_the_session() {
        let mut browser = browser();
        let accel = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        browser.key_pressed(Key::Char('t'), accel);
        assert_eq!(browser.session.tab_count(), 2);
        browser.key_pressed(Key::Char('w'), accel);
        assert_eq!(browser.session.tab_count(), 1);
    }

    #[test]
    fn typing_an_address_and_pressing_enter_navigates() {
        let mut browser = browser();
        browser.chrome.omnibox.focus();
        browser.chrome.omnibox.clear();
        for ch in "about:version".chars() {
            browser.key_pressed(Key::Char(ch), Modifiers::default());
        }
        browser.key_pressed(Key::Enter, Modifiers::default());
        assert_eq!(browser.session.active().unwrap().url(), "about:version");
    }

    #[test]
    fn clicking_a_link_on_the_home_page_navigates() {
        let mut browser = browser();
        // The home page links to the settings page; find it through layout.
        let content = browser.chrome.content_rect();
        let tab = browser.session.active().unwrap();
        let node = tab
            .page
            .document()
            .query_all("a")
            .into_iter()
            .find(|node| {
                tab.page
                    .document()
                    .element(*node)
                    .and_then(|el| el.attr("href"))
                    == Some("about:settings")
            })
            .expect("the home page links to settings");
        let index = tab
            .page
            .layout_tree()
            .box_for_node(node)
            .expect("the link has a box");
        let rect = tab.page.layout_tree().get(index).rect;
        let point = Point::new(content.x + rect.center().x, content.y + rect.center().y);

        browser.pointer_moved(point);
        assert_eq!(
            browser.chrome.status.as_deref(),
            Some("about:settings"),
            "the status pill should show the link target"
        );
        browser.pointer_down(point);
        browser.pointer_up(point);
        assert_eq!(browser.session.active().unwrap().url(), "about:settings");
    }

    #[test]
    fn clicking_the_toolbar_does_not_navigate_the_page() {
        let mut browser = browser();
        let reload = browser
            .chrome
            .geometry()
            .rect_of(WidgetId::Reload)
            .unwrap()
            .center();
        browser.pointer_down(reload);
        browser.pointer_up(reload);
        assert_eq!(browser.session.active().unwrap().url(), "about:home");
    }

    #[test]
    fn scrolling_over_content_moves_the_page() {
        let mut browser = Browser::new(&ShellConfig {
            offline: true,
            url: Some("about:version".into()),
            size: Size2D::new(400.0, 300.0),
            ..Default::default()
        })
        .unwrap();
        let point = browser.chrome.content_rect().center();
        let before = browser.session.active().unwrap().page.scroll_offset().y;
        browser.scroll(point, 120.0);
        let after = browser.session.active().unwrap().page.scroll_offset().y;
        assert!(after > before, "{before} -> {after}");
    }

    #[test]
    fn scrolling_over_the_toolbar_does_nothing() {
        let mut browser = browser();
        let toolbar = browser
            .chrome
            .geometry()
            .rect_of(WidgetId::Back)
            .unwrap()
            .center();
        let before = browser.session.active().unwrap().page.scroll_offset().y;
        browser.scroll(toolbar, 200.0);
        assert_eq!(
            browser.session.active().unwrap().page.scroll_offset().y,
            before
        );
    }

    #[test]
    fn the_cursor_follows_the_pointer() {
        let mut browser = browser();
        assert_eq!(browser.cursor(), Cursor::Default);
        let omnibox = browser.chrome.geometry().omnibox.center();
        browser.pointer_moved(omnibox);
        assert_eq!(browser.cursor(), Cursor::Text);
        let back = browser
            .chrome
            .geometry()
            .rect_of(WidgetId::Back)
            .unwrap()
            .center();
        browser.pointer_moved(back);
        assert_eq!(browser.cursor(), Cursor::Pointer);
    }

    #[test]
    fn the_pointer_leaving_clears_hover_state() {
        let mut browser = browser();
        let back = browser
            .chrome
            .geometry()
            .rect_of(WidgetId::Back)
            .unwrap()
            .center();
        browser.pointer_moved(back);
        browser.pointer_left();
        assert_eq!(browser.cursor(), Cursor::Default);
        assert!(browser.chrome.status.is_none());
    }

    #[test]
    fn rendering_produces_a_window_sized_canvas_and_clears_the_flag() {
        let mut browser = browser();
        let canvas = browser.render();
        assert_eq!(canvas.width(), 1000);
        assert_eq!(canvas.height(), 700);
        assert!(!browser.needs_redraw);
        // Nothing is left transparent.
        assert_eq!(canvas.pixel(500, 350).a, 255);
    }

    #[test]
    fn a_mobile_config_starts_in_the_touch_layout() {
        let browser = Browser::new(&ShellConfig {
            offline: true,
            ..ShellConfig::mobile()
        })
        .unwrap();
        assert!(browser.chrome.layout().is_mobile());
        assert_eq!(browser.chrome.size(), Size2D::new(390.0, 844.0));
    }

    #[test]
    fn background_tabs_do_not_change_the_active_page() {
        let mut browser = browser();
        browser.apply(UiAction::OpenUrlInNewTab("about:version".into()));
        assert_eq!(browser.session.tab_count(), 2);
        assert_eq!(browser.session.active().unwrap().url(), "about:home");
    }
}
