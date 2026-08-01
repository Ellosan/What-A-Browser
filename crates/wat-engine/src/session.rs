//! Tabs, history and the browsing session.

use std::rc::Rc;

use crate::page::{Page, PageState};
use wat_layout::geom::{Point, Size2D};
use wat_net::{normalize_input, Address, LoadError, Loader};
use wat_text::FontStore;
use wat_theme::ResolvedTheme;

/// Back/forward history for one tab.
#[derive(Clone, Debug, Default)]
pub struct History {
    entries: Vec<String>,
    /// Index of the current entry.
    index: usize,
}

impl History {
    pub fn new(url: impl Into<String>) -> Self {
        History {
            entries: vec![url.into()],
            index: 0,
        }
    }

    pub fn current(&self) -> Option<&str> {
        self.entries.get(self.index).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn can_go_back(&self) -> bool {
        self.index > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.index + 1 < self.entries.len()
    }

    /// Records a navigation, discarding any forward entries.
    pub fn push(&mut self, url: impl Into<String>) {
        let url = url.into();
        if self.current() == Some(url.as_str()) {
            return;
        }
        if !self.entries.is_empty() {
            self.entries.truncate(self.index + 1);
        }
        self.entries.push(url);
        self.index = self.entries.len() - 1;
    }

    /// Replaces the current entry, for a redirect.
    pub fn replace(&mut self, url: impl Into<String>) {
        let url = url.into();
        match self.entries.get_mut(self.index) {
            Some(entry) => *entry = url,
            None => self.entries.push(url),
        }
    }

    pub fn back(&mut self) -> Option<&str> {
        if !self.can_go_back() {
            return None;
        }
        self.index -= 1;
        self.current()
    }

    pub fn forward(&mut self) -> Option<&str> {
        if !self.can_go_forward() {
            return None;
        }
        self.index += 1;
        self.current()
    }

    /// The entries, most recent last.
    pub fn entries(&self) -> &[String] {
        &self.entries
    }
}

/// Identifier for a tab, stable across reordering.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TabId(u64);

impl TabId {
    pub fn value(self) -> u64 {
        self.0
    }

    /// Rebuilds an id from what [`TabId::value`] returned.
    ///
    /// The seam between the interface and an engine passes tab ids as plain
    /// integers, because two engines will not share this type.
    pub fn from_value(value: u64) -> Self {
        TabId(value)
    }
}

/// One tab: a page plus its history.
pub struct Tab {
    pub id: TabId,
    pub page: Page,
    pub history: History,
    /// Set while a load is in flight.
    pub loading: bool,
    /// Progress in `0.0..=1.0`, for the load indicator.
    pub progress: f32,
}

impl Tab {
    /// The tab's label: the document title, or the host.
    pub fn label(&self) -> String {
        match &self.page.title {
            Some(title) if !title.trim().is_empty() => title.clone(),
            _ => {
                let host = self.page.address.display_host();
                if host.is_empty() {
                    "Untitled".to_string()
                } else {
                    host
                }
            }
        }
    }

    pub fn url(&self) -> &str {
        self.page.address.url()
    }

    pub fn is_secure(&self) -> bool {
        self.page.address.is_secure()
    }

    pub fn failed(&self) -> bool {
        self.page.state == PageState::Failed
    }
}

/// A browsing session: the open tabs and which one is in front.
pub struct Session {
    tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
    fonts: Rc<FontStore>,
    theme: ResolvedTheme,
    viewport: Size2D,
    coarse_pointer: bool,
    /// Search engine template; `{}` is replaced with the query.
    pub search_template: String,
    /// Where a new tab starts.
    pub home_url: String,
}

impl Session {
    pub fn new(
        fonts: Rc<FontStore>,
        theme: ResolvedTheme,
        viewport: Size2D,
        coarse_pointer: bool,
    ) -> Self {
        Session {
            tabs: Vec::new(),
            active: 0,
            next_id: 1,
            fonts,
            theme,
            viewport,
            coarse_pointer,
            search_template: "https://duckduckgo.com/?q={}".to_string(),
            home_url: "about:home".to_string(),
        }
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    pub fn active_index(&self) -> usize {
        self.active.min(self.tabs.len().saturating_sub(1))
    }

    pub fn active(&self) -> Option<&Tab> {
        self.tabs.get(self.active_index())
    }

    pub fn active_mut(&mut self) -> Option<&mut Tab> {
        let index = self.active_index();
        self.tabs.get_mut(index)
    }

    pub fn theme(&self) -> &ResolvedTheme {
        &self.theme
    }

    pub fn viewport(&self) -> Size2D {
        self.viewport
    }

    /// Opens a tab and makes it active. `input` is treated as user input, so it
    /// may be a URL or a search.
    pub fn open_tab(&mut self, input: &str, loader: &dyn Loader) -> TabId {
        let id = TabId(self.next_id);
        self.next_id += 1;
        let address = normalize_input(input, &self.search_template)
            .unwrap_or_else(|_| Address::parse("about:home").expect("about:home is valid"));
        let page = Page::load(
            address.clone(),
            loader,
            self.fonts.clone(),
            self.theme.clone(),
            self.viewport,
            self.coarse_pointer,
        );
        let tab = Tab {
            id,
            history: History::new(page.address.url()),
            page,
            loading: false,
            progress: 1.0,
        };
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        if let Some(tab) = self.active_mut() {
            tab.page.scroll_to_fragment();
        }
        id
    }

    /// Closes a tab. The session always keeps at least one tab open.
    pub fn close_tab(&mut self, id: TabId, loader: &dyn Loader) {
        let Some(position) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        self.tabs.remove(position);
        if self.tabs.is_empty() {
            let home = self.home_url.clone();
            self.open_tab(&home, loader);
            return;
        }
        if self.active >= position && self.active > 0 {
            self.active -= 1;
        }
        self.active = self.active.min(self.tabs.len() - 1);
    }

    pub fn close_active_tab(&mut self, loader: &dyn Loader) {
        if let Some(id) = self.active().map(|tab| tab.id) {
            self.close_tab(id, loader);
        }
    }

    pub fn select_tab(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() || index == self.active {
            return false;
        }
        self.active = index;
        true
    }

    pub fn select_tab_id(&mut self, id: TabId) -> bool {
        match self.tabs.iter().position(|tab| tab.id == id) {
            Some(index) => self.select_tab(index),
            None => false,
        }
    }

    /// Moves to the next tab, wrapping around.
    pub fn cycle_tab(&mut self, forward: bool) -> bool {
        if self.tabs.len() < 2 {
            return false;
        }
        let count = self.tabs.len();
        self.active = if forward {
            (self.active + 1) % count
        } else {
            (self.active + count - 1) % count
        };
        true
    }

    /// Navigates the active tab. Returns the error if the address was unusable.
    pub fn navigate(&mut self, input: &str, loader: &dyn Loader) -> Result<(), LoadError> {
        let address = normalize_input(input, &self.search_template)?;
        self.navigate_to(address, loader);
        Ok(())
    }

    /// Navigates the active tab to an address that is already resolved.
    pub fn navigate_to(&mut self, address: Address, loader: &dyn Loader) {
        if self.tabs.is_empty() {
            self.open_tab(address.url(), loader);
            return;
        }
        let index = self.active_index();
        // A same-document jump only moves the scroll position.
        let same_document =
            self.tabs[index].page.address.same_document(&address) && address.fragment().is_some();

        let page = if same_document {
            None
        } else {
            Some(Page::load(
                address.clone(),
                loader,
                self.fonts.clone(),
                self.theme.clone(),
                self.viewport,
                self.coarse_pointer,
            ))
        };

        let tab = &mut self.tabs[index];
        if let Some(page) = page {
            tab.page = page;
        } else {
            tab.page.address = address;
        }
        tab.history.push(tab.page.address.url());
        tab.loading = false;
        tab.progress = 1.0;
        tab.page.scroll_to_fragment();
    }

    /// Reloads the active tab.
    pub fn reload(&mut self, loader: &dyn Loader) {
        let Some(address) = self.active().map(|tab| tab.page.address.clone()) else {
            return;
        };
        let index = self.active_index();
        self.tabs[index].page = Page::load(
            address,
            loader,
            self.fonts.clone(),
            self.theme.clone(),
            self.viewport,
            self.coarse_pointer,
        );
        self.tabs[index].page.scroll_to_fragment();
    }

    /// Goes back in the active tab, returning whether it moved.
    pub fn go_back(&mut self, loader: &dyn Loader) -> bool {
        self.travel(loader, true)
    }

    /// Goes forward in the active tab, returning whether it moved.
    pub fn go_forward(&mut self, loader: &dyn Loader) -> bool {
        self.travel(loader, false)
    }

    fn travel(&mut self, loader: &dyn Loader, back: bool) -> bool {
        let index = self.active_index();
        let Some(tab) = self.tabs.get_mut(index) else {
            return false;
        };
        let url = if back {
            tab.history.back()
        } else {
            tab.history.forward()
        };
        let Some(url) = url.map(str::to_string) else {
            return false;
        };
        let Ok(address) = Address::parse(&url) else {
            return false;
        };
        let page = Page::load(
            address,
            loader,
            self.fonts.clone(),
            self.theme.clone(),
            self.viewport,
            self.coarse_pointer,
        );
        self.tabs[index].page = page;
        self.tabs[index].page.scroll_to_fragment();
        true
    }

    /// Follows a link in the active tab.
    pub fn follow_link(&mut self, url: &str, loader: &dyn Loader) {
        if let Ok(address) = Address::parse(url) {
            self.navigate_to(address, loader);
        }
    }

    /// Opens a link in a new tab without switching to it.
    pub fn open_link_in_background(&mut self, url: &str, loader: &dyn Loader) {
        let previous = self.active;
        self.open_tab(url, loader);
        self.active = previous;
    }

    /// Applies a new viewport to every tab.
    pub fn set_viewport(&mut self, viewport: Size2D, coarse_pointer: bool) {
        self.viewport = viewport;
        self.coarse_pointer = coarse_pointer;
        for tab in &mut self.tabs {
            tab.page.set_viewport(viewport, coarse_pointer);
        }
    }

    /// Applies a new theme to every tab, so internal pages follow it.
    pub fn set_theme(&mut self, theme: ResolvedTheme) {
        self.theme = theme.clone();
        for tab in &mut self.tabs {
            tab.page.set_theme(theme.clone());
        }
    }

    /// Scrolls the active tab.
    pub fn scroll_active(&mut self, dx: f32, dy: f32) -> bool {
        match self.active_mut() {
            Some(tab) => tab.page.scroll_by(dx, dy),
            None => false,
        }
    }

    /// The link under a point in the active tab.
    pub fn link_at(&self, point: Point) -> Option<String> {
        self.active()?.page.link_at(point)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wat_net::StaticLoader;
    use wat_theme::Theme;

    fn session() -> Session {
        Session::new(
            Rc::new(FontStore::empty()),
            Theme::default().resolve(false),
            Size2D::new(800.0, 600.0),
            false,
        )
    }

    fn loader() -> StaticLoader {
        StaticLoader::new()
            .with_html("https://a.example/", "<title>A</title><p>a</p>")
            .with_html("https://b.example/", "<title>B</title><p>b</p>")
            .with_html("https://c.example/", "<title>C</title><p>c</p>")
    }

    #[test]
    fn history_starts_at_its_first_entry() {
        let history = History::new("https://a/");
        assert_eq!(history.current(), Some("https://a/"));
        assert!(!history.can_go_back());
        assert!(!history.can_go_forward());
    }

    #[test]
    fn history_walks_back_and_forward() {
        let mut history = History::new("https://a/");
        history.push("https://b/");
        history.push("https://c/");
        assert_eq!(history.len(), 3);

        assert_eq!(history.back(), Some("https://b/"));
        assert_eq!(history.back(), Some("https://a/"));
        assert_eq!(history.back(), None, "cannot go past the start");
        assert_eq!(history.forward(), Some("https://b/"));
        assert!(history.can_go_forward());
    }

    #[test]
    fn navigating_after_going_back_drops_the_forward_entries() {
        let mut history = History::new("https://a/");
        history.push("https://b/");
        history.push("https://c/");
        history.back();
        history.push("https://d/");
        assert_eq!(
            history.entries(),
            &[
                "https://a/".to_string(),
                "https://b/".to_string(),
                "https://d/".to_string()
            ]
        );
        assert!(!history.can_go_forward());
    }

    #[test]
    fn history_ignores_a_repeat_of_the_current_entry() {
        let mut history = History::new("https://a/");
        history.push("https://a/");
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn history_replace_does_not_add_an_entry() {
        let mut history = History::new("https://a/");
        history.replace("https://a/redirected");
        assert_eq!(history.len(), 1);
        assert_eq!(history.current(), Some("https://a/redirected"));
    }

    #[test]
    fn opening_tabs_activates_the_new_one() {
        let mut session = session();
        let loader = loader();
        let first = session.open_tab("https://a.example/", &loader);
        let second = session.open_tab("https://b.example/", &loader);
        assert_eq!(session.tab_count(), 2);
        assert_eq!(session.active().unwrap().id, second);
        assert_ne!(first, second, "ids must be unique");
        assert_eq!(session.active().unwrap().label(), "B");
    }

    #[test]
    fn tab_labels_fall_back_to_the_host() {
        let loader = StaticLoader::new().with_html("https://a.example/", "<p>no title</p>");
        let mut session = session();
        session.open_tab("https://a.example/", &loader);
        assert_eq!(session.active().unwrap().label(), "a.example");
    }

    #[test]
    fn closing_the_last_tab_opens_a_new_one() {
        let mut session = session();
        let loader = loader();
        let only = session.open_tab("https://a.example/", &loader);
        session.close_tab(only, &loader);
        assert_eq!(session.tab_count(), 1);
        assert_eq!(session.active().unwrap().url(), "about:home");
    }

    #[test]
    fn closing_a_tab_keeps_a_sensible_selection() {
        let mut session = session();
        let loader = loader();
        session.open_tab("https://a.example/", &loader);
        let middle = session.open_tab("https://b.example/", &loader);
        session.open_tab("https://c.example/", &loader);
        assert_eq!(session.active_index(), 2);

        session.close_tab(middle, &loader);
        assert_eq!(session.tab_count(), 2);
        assert_eq!(session.active().unwrap().label(), "C");
    }

    #[test]
    fn selecting_and_cycling_tabs() {
        let mut session = session();
        let loader = loader();
        session.open_tab("https://a.example/", &loader);
        session.open_tab("https://b.example/", &loader);

        assert!(session.select_tab(0));
        assert_eq!(session.active().unwrap().label(), "A");
        assert!(!session.select_tab(0), "already selected");
        assert!(!session.select_tab(99), "out of range");

        assert!(session.cycle_tab(true));
        assert_eq!(session.active().unwrap().label(), "B");
        assert!(session.cycle_tab(true));
        assert_eq!(session.active().unwrap().label(), "A", "cycling wraps");
        assert!(session.cycle_tab(false));
        assert_eq!(session.active().unwrap().label(), "B");
    }

    #[test]
    fn cycling_a_single_tab_does_nothing() {
        let mut session = session();
        let loader = loader();
        session.open_tab("https://a.example/", &loader);
        assert!(!session.cycle_tab(true));
    }

    #[test]
    fn navigation_records_history_and_supports_back() {
        let mut session = session();
        let loader = loader();
        session.open_tab("https://a.example/", &loader);
        session.navigate("https://b.example/", &loader).unwrap();
        assert_eq!(session.active().unwrap().label(), "B");
        assert!(session.active().unwrap().history.can_go_back());

        assert!(session.go_back(&loader));
        assert_eq!(session.active().unwrap().label(), "A");
        assert!(session.go_forward(&loader));
        assert_eq!(session.active().unwrap().label(), "B");
        assert!(!session.go_forward(&loader), "nothing further forward");
    }

    #[test]
    fn typed_searches_become_search_urls() {
        let mut session = session();
        session.search_template = "https://search.example/?q={}".into();
        let loader = StaticLoader::new().with_html(
            "https://search.example/?q=hello+there",
            "<title>Results</title>",
        );
        session.open_tab("about:blank", &loader);
        session.navigate("hello there", &loader).unwrap();
        assert_eq!(
            session.active().unwrap().url(),
            "https://search.example/?q=hello+there"
        );
    }

    #[test]
    fn a_fragment_jump_does_not_reload_the_page() {
        let loader = StaticLoader::new().with_html(
            "https://a.example/",
            "<title>A</title><div style=\"height:2000px\">x</div><p id=\"end\">end</p>",
        );
        let mut session = session();
        session.open_tab("https://a.example/", &loader);
        session.navigate("https://a.example/#end", &loader).unwrap();
        let tab = session.active().unwrap();
        assert_eq!(tab.label(), "A", "the document was kept");
        assert!(
            tab.page.scroll_offset().y > 0.0,
            "and scrolled to the target"
        );
    }

    #[test]
    fn background_tabs_do_not_steal_focus() {
        let mut session = session();
        let loader = loader();
        session.open_tab("https://a.example/", &loader);
        session.open_link_in_background("https://b.example/", &loader);
        assert_eq!(session.tab_count(), 2);
        assert_eq!(session.active().unwrap().label(), "A");
    }

    #[test]
    fn a_failed_load_still_produces_a_usable_tab() {
        let mut session = session();
        let loader = StaticLoader::new();
        session.open_tab("https://nowhere.example/", &loader);
        let tab = session.active().unwrap();
        assert!(tab.failed());
        assert!(!tab.label().is_empty());
        assert!(!tab.page.display_list().is_empty());
    }

    #[test]
    fn resizing_applies_to_every_tab() {
        let mut session = session();
        let loader = loader();
        session.open_tab("https://a.example/", &loader);
        session.open_tab("https://b.example/", &loader);
        session.set_viewport(Size2D::new(400.0, 900.0), true);
        for tab in session.tabs() {
            assert_eq!(tab.page.viewport(), Size2D::new(400.0, 900.0));
        }
    }

    #[test]
    fn a_theme_change_reaches_internal_pages() {
        let mut session = session();
        session.open_tab("about:home", &wat_net::OfflineLoader);
        let dark = Theme::default().resolve(true);
        session.set_theme(dark.clone());
        let tab = session.active().unwrap();
        let html = tab.page.document().query("html").unwrap();
        assert_eq!(
            tab.page.styles().get(html).background_color,
            dark.palette.canvas
        );
    }

    #[test]
    fn reload_rebuilds_the_page() {
        let mut session = session();
        let loader = loader();
        session.open_tab("https://a.example/", &loader);
        session.scroll_active(0.0, 50.0);
        session.reload(&loader);
        assert_eq!(session.active().unwrap().label(), "A");
        assert_eq!(session.active().unwrap().page.scroll_offset().y, 0.0);
    }

    #[test]
    fn an_empty_session_has_no_active_tab() {
        let session = session();
        assert!(session.active().is_none());
        assert_eq!(session.tab_count(), 0);
        assert_eq!(session.link_at(Point::ZERO), None);
    }

    #[test]
    fn navigating_an_empty_session_opens_a_tab() {
        let mut session = session();
        let loader = loader();
        session.navigate("https://a.example/", &loader).unwrap();
        assert_eq!(session.tab_count(), 1);
    }

    #[test]
    fn an_unusable_address_is_reported() {
        let mut session = session();
        let loader = loader();
        session.open_tab("about:blank", &loader);
        assert!(session.navigate("ftp://files.example/", &loader).is_err());
        // The tab is left alone.
        assert_eq!(session.active().unwrap().url(), "about:blank");
    }
}
