//! One loaded document, from bytes all the way to a display list.

use std::collections::HashMap;
use std::rc::Rc;

use crate::about;
use wat_css::{MatchContext, MediaContext, Origin, Stylesheet};
use wat_dom::{Document, NodeId};
use wat_layout::geom::{Point, Rect, Size2D};
use wat_layout::{layout_document, ImageProvider, LayoutContext, LayoutTree};
use wat_net::{resolve, Address, AddressKind, LoadError, Loader};
use wat_paint::{build_display_list, DisplayList, ImageSource, PaintOptions, RasterImage};
use wat_script::{Navigation, Rect as ScriptRect, ScriptError, ScriptRuntime};
use wat_style::{StyleEngine, StyleTree};
use wat_text::FontStore;
use wat_theme::ResolvedTheme;

/// Images for one page, keyed by the URL exactly as it appears in the markup —
/// which is what layout and painting look up.
#[derive(Default)]
pub struct PageImages {
    by_key: HashMap<String, Rc<RasterImage>>,
}

impl PageImages {
    pub fn insert(&mut self, key: impl Into<String>, image: Rc<RasterImage>) {
        self.by_key.insert(key.into(), image);
    }

    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }
}

impl ImageProvider for PageImages {
    fn intrinsic_size(&self, url: &str) -> Option<Size2D> {
        self.by_key
            .get(url)
            .map(|image| Size2D::new(image.width as f32, image.height as f32))
    }
}

impl ImageSource for PageImages {
    fn image(&self, url: &str) -> Option<Rc<RasterImage>> {
        self.by_key.get(url).cloned()
    }
}

/// How much of the load pipeline ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageState {
    Loading,
    Loaded,
    Failed,
}

/// A loaded page.
pub struct Page {
    pub address: Address,
    pub title: Option<String>,
    pub status: u16,
    pub state: PageState,
    /// Set when the page shown is an error page rather than the real document.
    pub error: Option<String>,

    document: Document,
    style_engine: StyleEngine,
    styles: StyleTree,
    layout: LayoutTree,
    images: PageImages,

    fonts: Rc<FontStore>,
    theme: ResolvedTheme,
    viewport: Size2D,
    coarse_pointer: bool,
    scroll: Point,
    hover: Option<NodeId>,

    /// The page's JavaScript runtime. `None` when scripting is switched off,
    /// which is what makes a scriptless build a configuration rather than a
    /// different code path.
    scripts: Option<ScriptRuntime>,
    /// Scripts that failed, for the host to report.
    script_errors: Vec<ScriptError>,
    /// A navigation a script asked for, waiting for the browser to act on it.
    pending_navigation: Option<Navigation>,
}

impl Page {
    /// Loads `address`, producing an error page rather than failing.
    pub fn load(
        address: Address,
        loader: &dyn Loader,
        fonts: Rc<FontStore>,
        theme: ResolvedTheme,
        viewport: Size2D,
        coarse_pointer: bool,
    ) -> Page {
        let mut page = Page {
            address: address.clone(),
            title: None,
            status: 0,
            state: PageState::Loading,
            error: None,
            document: Document::new(),
            style_engine: StyleEngine::new(),
            styles: StyleTree::empty(),
            layout: LayoutTree::new(viewport),
            images: PageImages::default(),
            fonts,
            theme,
            viewport,
            coarse_pointer,
            scroll: Point::ZERO,
            hover: None,
            scripts: Some(ScriptRuntime::new(address.url())),
            script_errors: Vec::new(),
            pending_navigation: None,
        };

        match page.fetch_document(&address, loader) {
            Ok((html, base_url)) => {
                page.document = wat_html::parse(&html);
                page.document.base_url = base_url;
                page.state = PageState::Loaded;
            }
            Err(error) => {
                let message = error.to_string();
                page.document =
                    wat_html::parse(&about::error(&page.theme, address.url(), &message));
                page.error = Some(message);
                page.state = PageState::Failed;
            }
        }

        page.title = page.document.title();
        page.collect_stylesheets(loader);
        page.load_images(loader);
        page.restyle();
        page.load_background_images(loader);
        page.relayout();
        page.run_scripts(loader);
        page
    }

    /// A page built from markup that is already in hand, for tests and for
    /// internal pages that need no loading.
    pub fn from_html(
        address: Address,
        html: &str,
        fonts: Rc<FontStore>,
        theme: ResolvedTheme,
        viewport: Size2D,
    ) -> Page {
        let loader = wat_net::OfflineLoader;
        let mut document = wat_html::parse(html);
        let location = address.url().to_string();
        document.base_url = Some(location.clone());
        let mut page = Page {
            address,
            title: document.title(),
            status: 200,
            state: PageState::Loaded,
            error: None,
            document,
            style_engine: StyleEngine::new(),
            styles: StyleTree::empty(),
            layout: LayoutTree::new(viewport),
            images: PageImages::default(),
            fonts,
            theme,
            viewport,
            coarse_pointer: false,
            scroll: Point::ZERO,
            hover: None,
            scripts: Some(ScriptRuntime::new(&location)),
            script_errors: Vec::new(),
            pending_navigation: None,
        };
        page.collect_stylesheets(&loader);
        page.restyle();
        page.relayout();
        page.run_scripts(&loader);
        page
    }

    /// Fetches the document, turning whatever came back into HTML.
    ///
    /// Returns the markup plus the base URL for resolving references.
    fn fetch_document(
        &mut self,
        address: &Address,
        loader: &dyn Loader,
    ) -> Result<(String, Option<String>), LoadError> {
        if address.kind() == AddressKind::About {
            let name = address.about_page().unwrap_or("");
            self.status = 200;
            let html = match name {
                "" | "about" => about::index(&self.theme),
                "home" | "newtab" => about::home(&self.theme, about::DEFAULT_SHORTCUTS),
                "settings" | "preferences" => {
                    about::settings(&self.theme, &wat_theme::preset_names(), None)
                }
                "version" => about::version(&self.theme),
                "blank" => about::blank(&self.theme),
                other => {
                    return Err(LoadError::BadUrl(format!("about:{other}")));
                }
            };
            return Ok((html, Some(address.url().to_string())));
        }

        let resource = loader.load(address)?;
        self.status = resource.status;

        if resource.is_html() {
            return Ok((resource.text(), Some(resource.url.clone())));
        }
        if resource.is_image() {
            // A bare image gets a document built around it.
            if let Some(image) = RasterImage::decode(&resource.body) {
                self.images.insert(resource.url.clone(), Rc::new(image));
            }
            return Ok((
                about::standalone_image(&self.theme, &resource.url),
                Some(resource.url.clone()),
            ));
        }
        if resource.content_type.starts_with("text/")
            || resource.content_type == "application/json"
            || resource.content_type == "application/xml"
        {
            return Ok((
                about::plain_text(&self.theme, &resource.url, &resource.text()),
                Some(resource.url.clone()),
            ));
        }
        if !resource.is_ok() {
            return Err(LoadError::Status {
                url: resource.url.clone(),
                status: resource.status,
            });
        }
        Ok((
            about::unsupported(&self.theme, &resource.url, &resource.content_type),
            Some(resource.url.clone()),
        ))
    }

    /// Gathers `<style>` blocks and `<link rel=stylesheet>` sheets.
    fn collect_stylesheets(&mut self, loader: &dyn Loader) {
        self.style_engine.clear_author_sheets();
        let base = self.document.effective_base();
        let base = match (&base, self.document.base_url.as_deref()) {
            // A relative `<base href>` resolves against the document URL.
            (Some(href), Some(document_url)) => resolve(Some(document_url), href).or(base.clone()),
            _ => base.clone(),
        };

        // Elements are visited in document order so later sheets win ties.
        let nodes: Vec<NodeId> = self.document.descendants(self.document.root()).collect();
        for node in nodes {
            let Some(element) = self.document.element(node) else {
                continue;
            };
            match element.name.as_str() {
                "style" => {
                    let media = element.attr("media").unwrap_or("").to_string();
                    let css = self.document.text_content(node);
                    let css = if media.trim().is_empty() {
                        css
                    } else {
                        format!("@media {media} {{ {css} }}")
                    };
                    self.style_engine
                        .add_author_sheet(Stylesheet::parse(&css, Origin::Author));
                }
                "link" => {
                    let is_stylesheet = element.attr("rel").is_some_and(|rel| {
                        rel.split_ascii_whitespace()
                            .any(|token| token.eq_ignore_ascii_case("stylesheet"))
                    });
                    let href = element.attr("href").unwrap_or("").to_string();
                    let media = element.attr("media").unwrap_or("").to_string();
                    if !is_stylesheet || href.trim().is_empty() {
                        continue;
                    }
                    if let Some(sheet) =
                        self.fetch_stylesheet(loader, base.as_deref(), &href, &media, 0)
                    {
                        self.style_engine.add_author_sheet(sheet);
                    }
                }
                _ => {}
            }
        }
    }

    /// Loads one stylesheet, following `@import` a bounded number of levels.
    fn fetch_stylesheet(
        &self,
        loader: &dyn Loader,
        base: Option<&str>,
        href: &str,
        media: &str,
        depth: u32,
    ) -> Option<Stylesheet> {
        const MAX_IMPORT_DEPTH: u32 = 3;
        if depth > MAX_IMPORT_DEPTH {
            return None;
        }
        let absolute = resolve(base, href)?;
        let address = Address::parse(&absolute).ok()?;
        let resource = match loader.load(&address) {
            Ok(resource) if resource.is_ok() => resource,
            Ok(resource) => {
                log::debug!("stylesheet {absolute} returned HTTP {}", resource.status);
                return None;
            }
            Err(error) => {
                log::debug!("stylesheet {absolute} failed: {error}");
                return None;
            }
        };

        let mut css = resource.text();
        // Pull imports in ahead of the sheet's own rules, as CSS requires.
        let sheet = Stylesheet::parse(&css, Origin::Author);
        let mut imported = String::new();
        for import in sheet.imports(&self.media_context()) {
            if let Some(inner) =
                self.fetch_stylesheet(loader, Some(&resource.url), import, "", depth + 1)
            {
                // Re-serialising is not possible, so the inner sheet is folded in
                // by re-fetching its text.
                if let Some(text) = inner.href.as_deref().and_then(|url| {
                    Address::parse(url)
                        .ok()
                        .and_then(|address| loader.load(&address).ok())
                        .map(|resource| resource.text())
                }) {
                    imported.push_str(&text);
                    imported.push('\n');
                }
            }
        }
        if !imported.is_empty() {
            css = format!("{imported}\n{css}");
        }
        if !media.trim().is_empty() {
            css = format!("@media {media} {{ {css} }}");
        }
        Some(Stylesheet::parse_with_href(
            &css,
            Origin::Author,
            resource.url,
        ))
    }

    /// Fetches every `<img src>` so layout knows its intrinsic size.
    fn load_images(&mut self, loader: &dyn Loader) {
        let base = self.document.effective_base();
        let sources: Vec<String> = self
            .document
            .query_all("img")
            .into_iter()
            .filter_map(|node| self.document.element(node)?.attr("src").map(str::to_string))
            .filter(|src| !src.trim().is_empty())
            .collect();

        for src in sources {
            if self.images.by_key.contains_key(&src) {
                continue;
            }
            if let Some(image) = self.fetch_image(loader, base.as_deref(), &src) {
                self.images.insert(src, image);
            }
        }
    }

    /// Fetches images named by `background-image`, after styling.
    fn load_background_images(&mut self, loader: &dyn Loader) {
        let base = self.document.effective_base();
        let mut wanted: Vec<String> = Vec::new();
        for node in self.document.descendants(self.document.root()) {
            if let wat_style::BackgroundImage::Url(url) = &self.styles.get(node).background_image {
                if !url.trim().is_empty() && !wanted.contains(url) {
                    wanted.push(url.clone());
                }
            }
        }
        for url in wanted {
            if self.images.by_key.contains_key(&url) {
                continue;
            }
            if let Some(image) = self.fetch_image(loader, base.as_deref(), &url) {
                self.images.insert(url, image);
            }
        }
    }

    fn fetch_image(
        &self,
        loader: &dyn Loader,
        base: Option<&str>,
        reference: &str,
    ) -> Option<Rc<RasterImage>> {
        let absolute = resolve(base, reference)?;
        let address = Address::parse(&absolute).ok()?;
        let resource = loader.load(&address).ok()?;
        if !resource.is_ok() {
            return None;
        }
        RasterImage::decode(&resource.body).map(Rc::new)
    }

    fn media_context(&self) -> MediaContext {
        MediaContext {
            width: self.viewport.width,
            height: self.viewport.height,
            device_pixel_ratio: 1.0,
            prefers_dark: self.theme.dark,
            prefers_reduced_motion: !self.theme.motion.enabled,
            coarse_pointer: self.coarse_pointer,
            media_type: wat_css::MediaType::Screen,
        }
    }

    fn match_context(&self) -> MatchContext {
        let target = self
            .address
            .fragment()
            .and_then(|fragment| self.document.query(&format!("#{fragment}")));
        MatchContext {
            hover: self.hover,
            active: None,
            focus: None,
            target,
        }
    }

    // ---- scripting --------------------------------------------------------

    /// Turns scripting off, or back on, for this page.
    ///
    /// Switching it off drops the runtime, so a page that was already running
    /// loses its listeners and timers — which is the point.
    pub fn set_scripting_enabled(&mut self, enabled: bool) {
        match (enabled, self.scripts.is_some()) {
            (true, false) => self.scripts = Some(ScriptRuntime::new(self.address.url())),
            (false, true) => self.scripts = None,
            _ => {}
        }
    }

    pub fn scripting_enabled(&self) -> bool {
        self.scripts.is_some()
    }

    /// Runs the document's scripts and applies whatever they changed.
    ///
    /// External scripts are fetched here and their bodies put in as the
    /// element's text, so the runtime only ever deals with source it can see.
    pub fn run_scripts(&mut self, loader: &dyn Loader) {
        if self.scripts.is_none() {
            return;
        }
        self.fetch_external_scripts(loader);
        self.publish_script_state();

        let Page {
            scripts: Some(scripts),
            document,
            ..
        } = self
        else {
            return;
        };
        self.script_errors = scripts.run_document_scripts(document);
        self.apply_script_effects();

        // The load handlers run against the layout the earlier scripts produced,
        // not the one the page was parsed with, so measuring in one gives the
        // real numbers.
        self.publish_script_state();
        let Page {
            scripts: Some(scripts),
            document,
            ..
        } = self
        else {
            return;
        };
        let load = scripts.dispatch_load(document);
        self.script_errors.extend(load.errors);
        self.apply_script_effects();
    }

    /// Fires an event at a node and applies whatever the handlers changed.
    ///
    /// Returns whether a handler called `preventDefault`, which is how the shell
    /// knows not to follow a link the page has taken over.
    pub fn dispatch_event(&mut self, node: NodeId, kind: &str) -> bool {
        self.publish_script_state();
        let Page {
            scripts: Some(scripts),
            document,
            ..
        } = self
        else {
            return false;
        };
        let outcome = scripts.dispatch(document, node, kind);
        self.script_errors = outcome.errors;
        self.apply_script_effects();
        outcome.default_prevented
    }

    /// Fires a click at whatever is under `point`, if anything is.
    pub fn dispatch_click_at(&mut self, point: Point) -> bool {
        match self.node_at(point) {
            Some(node) => self.dispatch_event(node, "click"),
            None => false,
        }
    }

    /// Runs the callbacks queued by `setTimeout` and applies what they changed.
    pub fn run_timers(&mut self) {
        if !self.has_timers() {
            return;
        }
        self.publish_script_state();
        let Page {
            scripts: Some(scripts),
            document,
            ..
        } = self
        else {
            return;
        };
        self.script_errors = scripts.run_timers(document);
        self.apply_script_effects();
    }

    /// Whether a script is waiting on a timer, so the shell knows to keep
    /// pumping rather than going idle.
    pub fn has_timers(&self) -> bool {
        self.scripts.as_ref().is_some_and(ScriptRuntime::has_timers)
    }

    /// Tells the runtime where things are and how big the window is, so
    /// `getBoundingClientRect` and `window.innerWidth` are not guesses.
    fn publish_script_state(&mut self) {
        let Some(scripts) = self.scripts.as_mut() else {
            return;
        };
        scripts.set_viewport(self.viewport.width, self.viewport.height);
        scripts.set_scroll(self.scroll.x, self.scroll.y);
        scripts.set_location(self.address.url());

        let mut rects = HashMap::new();
        for index in self.layout.preorder() {
            let layout_box = self.layout.get(index);
            let Some(node) = layout_box.node else {
                continue;
            };
            // Viewport coordinates, and the outermost box wins: an inline
            // element can have several fragments, and a page asking for its
            // rectangle means the first one.
            let rect = layout_box.rect;
            rects.entry(node).or_insert(ScriptRect {
                x: rect.x - self.scroll.x,
                y: rect.y - self.scroll.y,
                width: rect.width,
                height: rect.height,
            });
        }
        scripts.set_rects(rects);
    }

    /// Redoes whatever the scripts invalidated.
    fn apply_script_effects(&mut self) {
        let Some(scripts) = self.scripts.as_mut() else {
            return;
        };
        let dirty = scripts.take_dirty();
        let title_changed = scripts.take_title_changed();
        let navigation = scripts.take_navigation();
        let scroll = scripts.take_scroll();

        if dirty {
            // A script can add a <style> element or change a `media` attribute,
            // so the stylesheets are re-collected rather than reused.
            self.collect_stylesheets(&wat_net::OfflineLoader);
            self.restyle();
            self.relayout();
        }
        if title_changed || dirty {
            self.title = self.document.title();
        }
        if let Some((_, y)) = scroll {
            self.scroll_to(y);
        }
        if navigation.is_some() {
            self.pending_navigation = navigation;
        }
    }

    /// A navigation a script asked for, taken so it happens only once.
    pub fn take_script_navigation(&mut self) -> Option<Navigation> {
        self.pending_navigation.take()
    }

    /// Scripts that failed since the last time this was called.
    pub fn take_script_errors(&mut self) -> Vec<ScriptError> {
        std::mem::take(&mut self.script_errors)
    }

    /// Everything the page logged, oldest first.
    pub fn console(&self) -> &[wat_js::ConsoleMessage] {
        match &self.scripts {
            Some(scripts) => scripts.console(),
            None => &[],
        }
    }

    /// Runs one expression against this page, which is what a developer console
    /// would do.
    pub fn eval(&mut self, source: &str) -> Result<String, String> {
        self.publish_script_state();
        let Page {
            scripts: Some(scripts),
            document,
            ..
        } = self
        else {
            return Err("scripting is switched off for this page".to_string());
        };
        let result = scripts
            .eval(document, source)
            .map(|value| wat_js::inspect(&value))
            .map_err(|error| error.message);
        self.apply_script_effects();
        result
    }

    /// Replaces the text of every `<script src>` with the fetched body.
    fn fetch_external_scripts(&mut self, loader: &dyn Loader) {
        let base = self.document.base_url.clone();
        let nodes: Vec<NodeId> = self.document.descendants(self.document.root()).collect();
        for node in nodes {
            let Some(element) = self.document.element(node) else {
                continue;
            };
            if element.name != "script" {
                continue;
            }
            let Some(src) = element.attr("src").map(str::to_string) else {
                continue;
            };
            if src.trim().is_empty() {
                continue;
            }
            let Some(url) = resolve(base.as_deref(), &src) else {
                continue;
            };
            let Ok(address) = Address::parse(&url) else {
                continue;
            };
            match loader.load(&address) {
                Ok(resource) => {
                    let source = resource.text();
                    let text = self.document.create_text(source);
                    // The element's own children are replaced, since a script
                    // with a `src` ignores its inline text anyway.
                    let existing: Vec<NodeId> = self.document.children(node).collect();
                    for child in existing {
                        self.document.detach(child);
                    }
                    self.document.append(node, text);
                }
                Err(error) => {
                    log::warn!("could not load script {url}: {error}");
                }
            }
        }
    }

    /// Recomputes styles. Needed after a viewport, theme or hover change.
    pub fn restyle(&mut self) {
        self.styles =
            self.style_engine
                .compute(&self.document, &self.media_context(), &self.match_context());
    }

    /// Recomputes layout for the current viewport.
    pub fn relayout(&mut self) {
        let ctx = LayoutContext::new(
            &self.document,
            &self.styles,
            &self.fonts,
            &self.images,
            self.viewport,
        );
        self.layout = layout_document(&ctx);
        self.clamp_scroll();
    }

    /// Builds the display list for the current scroll position, with the page's
    /// top-left corner at the canvas origin.
    pub fn display_list(&self) -> DisplayList {
        self.display_list_at(Point::ZERO)
    }

    /// Builds the display list with the page's top-left corner at `origin`,
    /// which is how the shell places the page below the chrome.
    pub fn display_list_at(&self, origin: Point) -> DisplayList {
        let options = PaintOptions::new(&self.images)
            .with_offset(Point::new(
                origin.x - self.scroll.x,
                origin.y - self.scroll.y,
            ))
            .with_cull(Rect::new(
                self.scroll.x,
                self.scroll.y,
                self.viewport.width,
                self.viewport.height,
            ));
        build_display_list(&self.layout, &options)
    }

    /// The page's own background colour, painted before the display list.
    pub fn background_color(&self) -> wat_css::Color {
        // An explicit background on `html` or `body` wins; otherwise the theme's.
        for selector in ["html", "body"] {
            if let Some(node) = self.document.query(selector) {
                let color = self.styles.get(node).background_color;
                if !color.is_transparent() {
                    return color;
                }
            }
        }
        self.theme.palette.page
    }

    pub fn viewport(&self) -> Size2D {
        self.viewport
    }

    pub fn document_size(&self) -> Size2D {
        self.layout.document_size
    }

    pub fn scroll_offset(&self) -> Point {
        self.scroll
    }

    pub fn layout_tree(&self) -> &LayoutTree {
        &self.layout
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn styles(&self) -> &StyleTree {
        &self.styles
    }

    pub fn images(&self) -> &PageImages {
        &self.images
    }

    /// Applies a new viewport, re-styling and re-laying out.
    pub fn set_viewport(&mut self, viewport: Size2D, coarse_pointer: bool) {
        if (self.viewport.width - viewport.width).abs() < 0.5
            && (self.viewport.height - viewport.height).abs() < 0.5
            && self.coarse_pointer == coarse_pointer
        {
            return;
        }
        self.viewport = viewport;
        self.coarse_pointer = coarse_pointer;
        // Media queries can depend on the viewport, so styles are recomputed too.
        self.restyle();
        self.relayout();
    }

    /// Applies a new theme.
    ///
    /// Internal pages carry the theme's stylesheet inside their markup, so they
    /// are regenerated; web pages only need re-styling for the new
    /// `prefers-color-scheme`.
    pub fn set_theme(&mut self, theme: ResolvedTheme) {
        self.theme = theme;
        if self.address.kind() == AddressKind::About {
            let address = self.address.clone();
            let loader = wat_net::OfflineLoader;
            if let Ok((html, base_url)) = self.fetch_document(&address, &loader) {
                self.document = wat_html::parse(&html);
                self.document.base_url = base_url;
                self.title = self.document.title();
                self.collect_stylesheets(&loader);
            }
        }
        self.restyle();
        self.relayout();
    }

    /// Scrolls by `(dx, dy)`, returning whether anything moved.
    pub fn scroll_by(&mut self, dx: f32, dy: f32) -> bool {
        let before = self.scroll;
        self.scroll.x += dx;
        self.scroll.y += dy;
        self.clamp_scroll();
        (self.scroll.x - before.x).abs() > 0.01 || (self.scroll.y - before.y).abs() > 0.01
    }

    /// Jumps to an absolute offset.
    pub fn scroll_to(&mut self, y: f32) -> bool {
        let before = self.scroll.y;
        self.scroll.y = y;
        self.clamp_scroll();
        (self.scroll.y - before).abs() > 0.01
    }

    /// Scrolls so the element named by the URL fragment is at the top.
    pub fn scroll_to_fragment(&mut self) -> bool {
        let Some(fragment) = self.address.fragment() else {
            return false;
        };
        let target = self
            .document
            .query(&format!("#{fragment}"))
            .or_else(|| {
                // Fall back to `<a name="...">`, which older pages use.
                self.document.query_all("a").into_iter().find(|node| {
                    self.document
                        .element(*node)
                        .and_then(|el| el.attr("name"))
                        .is_some_and(|name| name == fragment)
                })
            })
            .and_then(|node| self.layout.box_for_node(node));
        match target {
            Some(index) => {
                let y = self.layout.get(index).rect.y;
                self.scroll_to(y)
            }
            None => false,
        }
    }

    fn clamp_scroll(&mut self) {
        let max_y = (self.layout.document_size.height - self.viewport.height).max(0.0);
        let max_x = (self.layout.document_size.width - self.viewport.width).max(0.0);
        self.scroll.y = self.scroll.y.clamp(0.0, max_y);
        self.scroll.x = self.scroll.x.clamp(0.0, max_x);
    }

    /// The maximum scroll offset, for drawing a scrollbar.
    pub fn max_scroll_y(&self) -> f32 {
        (self.layout.document_size.height - self.viewport.height).max(0.0)
    }

    /// Converts a point in the viewport to document coordinates.
    fn to_document(&self, point: Point) -> Point {
        Point::new(point.x + self.scroll.x, point.y + self.scroll.y)
    }

    /// The DOM node under a viewport point.
    pub fn node_at(&self, point: Point) -> Option<NodeId> {
        let index = self.layout.hit_test(self.to_document(point))?;
        self.layout.get(index).node.or_else(|| {
            // Anonymous boxes carry no node; walk up to one that does.
            let mut current = self.layout.get(index).parent;
            while let Some(parent) = current {
                if let Some(node) = self.layout.get(parent).node {
                    return Some(node);
                }
                current = self.layout.get(parent).parent;
            }
            None
        })
    }

    /// The link target under a viewport point, resolved to an absolute URL.
    pub fn link_at(&self, point: Point) -> Option<String> {
        let node = self.node_at(point)?;
        let anchor = std::iter::once(node)
            .chain(self.document.ancestors(node))
            .find(|candidate| {
                self.document
                    .element(*candidate)
                    .is_some_and(|el| el.name == "a" && el.has_attr("href"))
            })?;
        let href = self.document.element(anchor)?.attr("href")?;
        if href.trim().is_empty() {
            return None;
        }
        resolve(self.document.effective_base().as_deref(), href)
    }

    /// The cursor the content under `point` asks for.
    pub fn cursor_at(&self, point: Point) -> wat_style::Cursor {
        if self.link_at(point).is_some() {
            return wat_style::Cursor::Pointer;
        }
        match self.node_at(point) {
            Some(node) => self.styles.get(node).cursor,
            None => wat_style::Cursor::Auto,
        }
    }

    /// Updates the hovered node, returning whether a repaint is needed.
    pub fn set_hover(&mut self, point: Option<Point>) -> bool {
        let node = point.and_then(|point| self.node_at(point));
        if node == self.hover {
            return false;
        }
        self.hover = node;
        // Hover can change styles and therefore layout, so both are redone.
        self.restyle();
        self.relayout();
        true
    }

    /// Renders the page into a canvas of its own size, for screenshots.
    pub fn render_to_canvas(&self) -> wat_paint::Canvas {
        self.render_to_canvas_scaled(1.0)
    }

    /// Renders at a device pixel ratio: the canvas is `scale` times the
    /// viewport, and the page is drawn that much larger.
    pub fn render_to_canvas_scaled(&self, scale: f32) -> wat_paint::Canvas {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        let mut canvas = wat_paint::Canvas::filled(
            (self.viewport.width * scale).max(1.0) as u32,
            (self.viewport.height * scale).max(1.0) as u32,
            self.background_color(),
        );
        wat_paint::Renderer::new(&self.fonts)
            .render(&self.display_list().scaled(scale), &mut canvas);
        canvas
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wat_net::{Resource, StaticLoader};
    use wat_theme::Theme;

    fn fonts() -> Rc<FontStore> {
        Rc::new(FontStore::empty())
    }

    fn theme() -> ResolvedTheme {
        Theme::default().resolve(false)
    }

    fn page_from(html: &str) -> Page {
        Page::from_html(
            Address::parse("https://example.com/").unwrap(),
            html,
            fonts(),
            theme(),
            Size2D::new(800.0, 600.0),
        )
    }

    #[test]
    fn loads_html_and_finds_the_title() {
        let loader =
            StaticLoader::new().with_html("https://example.com/", "<title>Hi</title><p>body</p>");
        let page = Page::load(
            Address::parse("https://example.com/").unwrap(),
            &loader,
            fonts(),
            theme(),
            Size2D::new(800.0, 600.0),
            false,
        );
        assert_eq!(page.state, PageState::Loaded);
        assert_eq!(page.title.as_deref(), Some("Hi"));
        assert!(page.error.is_none());
        assert!(!page.display_list().is_empty());
    }

    #[test]
    fn a_page_script_runs_and_the_page_is_laid_out_again() {
        let page = page_from(
            "<p id='out'>before</p><script>document.getElementById('out').textContent = 'after'</script>",
        );
        let node = page.document().query("#out").unwrap();
        assert_eq!(page.document().text_content(node), "after");
        // The change went through the whole pipeline, not just the tree: the
        // paragraph has a box, and the display list has something in it.
        assert!(page.layout_tree().box_for_node(node).is_some());
        assert!(!page.display_list().is_empty());
    }

    #[test]
    fn a_script_that_adds_elements_gets_them_laid_out() {
        let page = page_from(
            "<div id='host'></div>
             <script>
               for (let i = 0; i < 3; i++) {
                 const p = document.createElement('p');
                 p.textContent = 'row ' + i;
                 document.getElementById('host').appendChild(p);
               }
             </script>",
        );
        let host = page.document().query("#host").unwrap();
        assert_eq!(page.document().element_children(host).count(), 3);
        for child in page.document().element_children(host) {
            let index = page
                .layout_tree()
                .box_for_node(child)
                .expect("every new element needs a box");
            assert!(
                page.layout_tree().get(index).rect.height > 0.0,
                "and a height"
            );
        }
    }

    #[test]
    fn a_script_that_changes_a_class_is_restyled() {
        let page = page_from(
            "<style>.on { color: rgb(1, 2, 3) }</style>
             <p id='p'>x</p>
             <script>document.getElementById('p').className = 'on'</script>",
        );
        let node = page.document().query("#p").unwrap();
        let color = page.styles().get(node).color;
        assert_eq!((color.r, color.g, color.b), (1, 2, 3));
    }

    #[test]
    fn a_script_that_adds_a_style_element_is_picked_up() {
        let page = page_from(
            "<p id='p'>x</p>
             <script>
               const style = document.createElement('style');
               style.textContent = '#p { color: rgb(4, 5, 6) }';
               document.body.appendChild(style);
             </script>",
        );
        let node = page.document().query("#p").unwrap();
        let color = page.styles().get(node).color;
        assert_eq!((color.r, color.g, color.b), (4, 5, 6));
    }

    #[test]
    fn a_script_can_set_the_title() {
        let page = page_from("<title>old</title><script>document.title = 'new'</script>");
        assert_eq!(page.title.as_deref(), Some("new"));
    }

    #[test]
    fn a_failing_script_leaves_the_page_usable() {
        let mut page = page_from("<p>still here</p><script>oops.missing()</script>");
        let errors = page.take_script_errors();
        assert_eq!(errors.len(), 1);
        assert!(page.document().query("p").is_some());
        assert!(!page.display_list().is_empty());
        assert!(
            page.take_script_errors().is_empty(),
            "the errors are drained"
        );
    }

    #[test]
    fn a_runaway_script_does_not_hang_the_page() {
        let mut page = page_from("<p>fine</p><script>while (true) {}</script>");
        let errors = page.take_script_errors();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].fatal, "{errors:?}");
        assert!(!page.display_list().is_empty(), "the page still renders");
    }

    #[test]
    fn a_click_reaches_a_page_script() {
        let mut page = page_from(
            "<button id='b'>go</button>
             <script>
               document.getElementById('b').addEventListener('click', e => {
                 e.target.textContent = 'clicked';
               });
             </script>",
        );
        let button = page.document().query("#b").unwrap();
        assert!(
            !page.dispatch_event(button, "click"),
            "nothing was prevented"
        );
        assert_eq!(page.document().text_content(button), "clicked");
    }

    #[test]
    fn a_script_can_take_over_a_link_click() {
        let mut page = page_from(
            "<a id='a' href='/next'>go</a>
             <script>document.getElementById('a').addEventListener('click', e => e.preventDefault())</script>",
        );
        let link = page.document().query("#a").unwrap();
        assert!(page.dispatch_event(link, "click"));
    }

    #[test]
    fn a_click_can_be_aimed_at_a_point() {
        let mut page = page_from(
            "<p id='p'>x</p>
             <script>
               window.hits = 0;
               document.getElementById('p').addEventListener('click', () => window.hits++);
             </script>",
        );
        // The click lands on the text inside the paragraph and bubbles up to it,
        // which is what makes a listener on a container work at all.
        assert!(!page.dispatch_click_at(Point::new(20.0, 30.0)));
        assert_eq!(page.eval("window.hits").unwrap(), "1");

        // A click on empty space below the content reaches nothing.
        page.dispatch_click_at(Point::new(700.0, 550.0));
        assert_eq!(page.eval("window.hits").unwrap(), "1");
    }

    #[test]
    fn a_script_can_read_where_things_are() {
        let page = page_from(
            "<div id='d' style='width: 120px; height: 40px'></div>
             <script>
               const rect = document.getElementById('d').getBoundingClientRect();
               document.getElementById('d').setAttribute('data-size', rect.width + 'x' + rect.height);
             </script>",
        );
        let node = page.document().query("#d").unwrap();
        assert_eq!(
            page.document().element(node).unwrap().attr("data-size"),
            Some("120x40"),
            "the script saw the real layout"
        );
    }

    #[test]
    fn timers_run_when_the_page_is_asked_to_run_them() {
        let mut page = page_from(
            "<p id='p'>before</p>
             <script>setTimeout(() => { document.getElementById('p').textContent = 'after' }, 0)</script>",
        );
        let node = page.document().query("#p").unwrap();
        assert!(page.has_timers());
        page.run_timers();
        assert_eq!(page.document().text_content(node), "after");
        assert!(!page.has_timers());
    }

    #[test]
    fn a_load_handler_measures_the_layout_the_scripts_produced() {
        let page = page_from(
            "<div id='host' style='width: 200px'></div>
             <script>
               for (let i = 0; i < 4; i++) {
                 document.getElementById('host').appendChild(document.createElement('p'));
               }
               window.addEventListener('load', () => {
                 const rect = document.getElementById('host').getBoundingClientRect();
                 document.getElementById('host').setAttribute('data-height', String(rect.height));
               });
             </script>",
        );
        let host = page.document().query("#host").unwrap();
        let measured: f32 = page
            .document()
            .element(host)
            .unwrap()
            .attr("data-height")
            .expect("the load handler should have measured")
            .parse()
            .unwrap();
        assert!(
            measured > 0.0,
            "the four paragraphs added by the script have to be in the height: {measured}"
        );
    }

    #[test]
    fn a_script_navigation_is_handed_to_the_browser() {
        let mut page = page_from("<script>location.assign('/elsewhere')</script>");
        let request = page.take_script_navigation().expect("a navigation");
        assert_eq!(request.url, "/elsewhere");
        assert!(page.take_script_navigation().is_none());
    }

    #[test]
    fn an_external_script_is_fetched_and_run() {
        let loader = StaticLoader::new()
            .with_html(
                "https://example.com/",
                "<p id='p'>before</p><script src='/app.js'></script>",
            )
            .with(
                "https://example.com/app.js",
                Resource::new(
                    "https://example.com/app.js",
                    "text/javascript",
                    b"document.getElementById('p').textContent = 'from the file'".to_vec(),
                ),
            );
        let page = Page::load(
            Address::parse("https://example.com/").unwrap(),
            &loader,
            fonts(),
            theme(),
            Size2D::new(800.0, 600.0),
            false,
        );
        let node = page.document().query("#p").unwrap();
        assert_eq!(page.document().text_content(node), "from the file");
    }

    #[test]
    fn the_console_is_kept_for_the_host() {
        let page = page_from("<script>console.log('page said this')</script>");
        assert_eq!(page.console().len(), 1);
        assert_eq!(page.console()[0].text, "page said this");
    }

    #[test]
    fn scripting_can_be_switched_off() {
        let mut page = page_from("<p id='p'>untouched</p>");
        page.set_scripting_enabled(false);
        assert!(!page.scripting_enabled());
        page.run_scripts(&wat_net::OfflineLoader);
        assert!(page.eval("1 + 1").is_err());
        assert!(page.console().is_empty());

        page.set_scripting_enabled(true);
        assert_eq!(page.eval("1 + 1").unwrap(), "2");
    }

    #[test]
    fn a_page_can_be_evaluated_against_like_a_console() {
        let mut page = page_from("<p id='p'>text</p>");
        assert_eq!(
            page.eval("document.getElementById('p').textContent")
                .unwrap(),
            "text"
        );
        page.eval("document.getElementById('p').textContent = 'edited'")
            .unwrap();
        let node = page.document().query("#p").unwrap();
        assert_eq!(page.document().text_content(node), "edited");
        assert!(page.eval("this is not javascript").is_err());
    }

    #[test]
    fn a_failed_load_shows_an_error_page() {
        let loader = StaticLoader::new();
        let page = Page::load(
            Address::parse("https://example.com/missing").unwrap(),
            &loader,
            fonts(),
            theme(),
            Size2D::new(800.0, 600.0),
            false,
        );
        assert_eq!(page.state, PageState::Failed);
        assert!(page.error.is_some());
        // The error page is still a real, rendered document.
        assert!(page.document().query("h1").is_some());
        assert!(!page.display_list().is_empty());
    }

    #[test]
    fn about_pages_load_without_a_network() {
        for name in [
            "about:home",
            "about:settings",
            "about:version",
            "about:blank",
        ] {
            let page = Page::load(
                Address::parse(name).unwrap(),
                &wat_net::OfflineLoader,
                fonts(),
                theme(),
                Size2D::new(800.0, 600.0),
                false,
            );
            assert_eq!(page.state, PageState::Loaded, "{name} should load");
        }
    }

    #[test]
    fn an_unknown_about_page_is_an_error() {
        let page = Page::load(
            Address::parse("about:nonsense").unwrap(),
            &wat_net::OfflineLoader,
            fonts(),
            theme(),
            Size2D::new(800.0, 600.0),
            false,
        );
        assert_eq!(page.state, PageState::Failed);
    }

    #[test]
    fn linked_stylesheets_are_fetched_and_applied() {
        let loader = StaticLoader::new()
            .with_html(
                "https://example.com/",
                "<link rel=stylesheet href=\"style.css\"><p id=x>hi</p>",
            )
            .with(
                "https://example.com/style.css",
                Resource::new(
                    "https://example.com/style.css",
                    "text/css",
                    b"#x { color: #ff0000 }".to_vec(),
                ),
            );
        let page = Page::load(
            Address::parse("https://example.com/").unwrap(),
            &loader,
            fonts(),
            theme(),
            Size2D::new(800.0, 600.0),
            false,
        );
        let node = page.document().query("#x").unwrap();
        assert_eq!(
            page.styles().get(node).color,
            wat_css::Color::rgb(255, 0, 0)
        );
    }

    #[test]
    fn a_missing_stylesheet_does_not_break_the_page() {
        let loader = StaticLoader::new().with_html(
            "https://example.com/",
            "<link rel=stylesheet href=\"gone.css\"><p>still here</p>",
        );
        let page = Page::load(
            Address::parse("https://example.com/").unwrap(),
            &loader,
            fonts(),
            theme(),
            Size2D::new(800.0, 600.0),
            false,
        );
        assert_eq!(page.state, PageState::Loaded);
        assert!(page.document().query("p").is_some());
    }

    #[test]
    fn inline_styles_apply() {
        let page = page_from("<style>p { color: #00ff00 }</style><p>x</p>");
        let node = page.document().query("p").unwrap();
        assert_eq!(
            page.styles().get(node).color,
            wat_css::Color::rgb(0, 255, 0)
        );
    }

    #[test]
    fn a_media_attribute_on_style_is_honoured() {
        let html = "<style media=\"(max-width: 100px)\">p { color: #ff0000 }</style><p>x</p>";
        let page = Page::from_html(
            Address::parse("https://example.com/").unwrap(),
            html,
            fonts(),
            theme(),
            Size2D::new(800.0, 600.0),
        );
        let node = page.document().query("p").unwrap();
        assert_ne!(
            page.styles().get(node).color,
            wat_css::Color::rgb(255, 0, 0)
        );

        let narrow = Page::from_html(
            Address::parse("https://example.com/").unwrap(),
            html,
            fonts(),
            theme(),
            Size2D::new(80.0, 600.0),
        );
        let node = narrow.document().query("p").unwrap();
        assert_eq!(
            narrow.styles().get(node).color,
            wat_css::Color::rgb(255, 0, 0)
        );
    }

    #[test]
    fn plain_text_responses_are_wrapped() {
        let loader = StaticLoader::new().with(
            "https://example.com/a.txt",
            Resource::new(
                "https://example.com/a.txt",
                "text/plain",
                b"hello world".to_vec(),
            ),
        );
        let page = Page::load(
            Address::parse("https://example.com/a.txt").unwrap(),
            &loader,
            fonts(),
            theme(),
            Size2D::new(400.0, 300.0),
            false,
        );
        assert_eq!(page.state, PageState::Loaded);
        let pre = page.document().query("pre").expect("a pre element");
        assert!(page.document().text_content(pre).contains("hello world"));
    }

    #[test]
    fn unsupported_content_gets_an_explanatory_page() {
        let loader = StaticLoader::new().with(
            "https://example.com/a.pdf",
            Resource::new(
                "https://example.com/a.pdf",
                "application/pdf",
                vec![1, 2, 3],
            ),
        );
        let page = Page::load(
            Address::parse("https://example.com/a.pdf").unwrap(),
            &loader,
            fonts(),
            theme(),
            Size2D::new(400.0, 300.0),
            false,
        );
        assert_eq!(page.state, PageState::Loaded);
        assert!(page
            .document()
            .text_content(page.document().body().unwrap())
            .contains("application/pdf"));
    }

    #[test]
    fn scrolling_is_clamped_to_the_document() {
        let mut page = page_from("<div style=\"height:2000px\">tall</div>");
        assert_eq!(page.scroll_offset().y, 0.0);
        assert!(!page.scroll_by(0.0, -100.0), "cannot scroll above the top");
        assert!(page.scroll_by(0.0, 500.0));
        assert_eq!(page.scroll_offset().y, 500.0);
        assert!(page.scroll_by(0.0, 100_000.0));
        assert_eq!(page.scroll_offset().y, page.max_scroll_y());
        assert!(!page.scroll_by(0.0, 10.0), "already at the bottom");
    }

    #[test]
    fn a_short_page_cannot_scroll() {
        let mut page = page_from("<p>short</p>");
        assert_eq!(page.max_scroll_y(), 0.0);
        assert!(!page.scroll_by(0.0, 100.0));
    }

    #[test]
    fn links_resolve_against_the_document_url() {
        let page = page_from(
            "<body style=\"margin:0\"><a href=\"/next\" \
             style=\"display:block;width:100px;height:50px\">go</a></body>",
        );
        let link = page.link_at(Point::new(20.0, 20.0));
        assert_eq!(link.as_deref(), Some("https://example.com/next"));
        assert_eq!(
            page.cursor_at(Point::new(20.0, 20.0)),
            wat_style::Cursor::Pointer
        );
    }

    #[test]
    fn a_base_element_changes_link_resolution() {
        let page = page_from(
            "<head><base href=\"https://cdn.example/dir/\"></head>\
             <body style=\"margin:0\"><a href=\"x.html\" \
             style=\"display:block;width:100px;height:50px\">go</a></body>",
        );
        assert_eq!(
            page.link_at(Point::new(20.0, 20.0)).as_deref(),
            Some("https://cdn.example/dir/x.html")
        );
    }

    #[test]
    fn clicking_empty_space_finds_no_link() {
        let page = page_from("<a href=\"/x\">go</a>");
        assert_eq!(page.link_at(Point::new(700.0, 500.0)), None);
    }

    #[test]
    fn resizing_relayouts_and_reflows_text() {
        let mut page =
            page_from("<p style=\"font-size:10px;margin:0\">aaaa bbbb cccc dddd eeee</p>");
        // The viewport is kept short so the content height, not the viewport
        // minimum, decides the document size.
        let tall_at_narrow = {
            page.set_viewport(Size2D::new(60.0, 40.0), false);
            page.document_size().height
        };
        page.set_viewport(Size2D::new(800.0, 40.0), false);
        let short_at_wide = page.document_size().height;
        assert!(
            tall_at_narrow > short_at_wide,
            "narrow should wrap more: {tall_at_narrow} vs {short_at_wide}"
        );
    }

    #[test]
    fn hover_state_restyles_the_page() {
        let mut page = page_from(
            "<style>a:hover{color:#ff0000}</style>\
             <body style=\"margin:0\"><a href=\"/x\" \
             style=\"display:block;width:100px;height:50px\">go</a></body>",
        );
        let node = page.document().query("a").unwrap();
        assert_ne!(
            page.styles().get(node).color,
            wat_css::Color::rgb(255, 0, 0)
        );

        assert!(page.set_hover(Some(Point::new(20.0, 20.0))));
        assert_eq!(
            page.styles().get(node).color,
            wat_css::Color::rgb(255, 0, 0)
        );
        // Hovering the same node again changes nothing.
        assert!(!page.set_hover(Some(Point::new(30.0, 30.0))));
        assert!(page.set_hover(None));
    }

    #[test]
    fn fragments_scroll_to_their_target() {
        let mut page = Page::from_html(
            Address::parse("https://example.com/#target").unwrap(),
            "<div style=\"height:1500px\">spacer</div><h2 id=\"target\">here</h2>\
             <div style=\"height:1500px\">more</div>",
            fonts(),
            theme(),
            Size2D::new(400.0, 300.0),
        );
        assert!(page.scroll_to_fragment());
        assert!(page.scroll_offset().y > 1000.0);
    }

    #[test]
    fn a_missing_fragment_does_not_scroll() {
        let mut page = Page::from_html(
            Address::parse("https://example.com/#nowhere").unwrap(),
            "<div style=\"height:2000px\">x</div>",
            fonts(),
            theme(),
            Size2D::new(400.0, 300.0),
        );
        assert!(!page.scroll_to_fragment());
        assert_eq!(page.scroll_offset().y, 0.0);
    }

    #[test]
    fn the_page_background_prefers_the_documents_own() {
        let themed = page_from("<p>x</p>");
        assert_eq!(themed.background_color(), theme().palette.page);

        let explicit = page_from("<body style=\"background:#123456\">x</body>");
        assert_eq!(
            explicit.background_color(),
            wat_css::Color::rgb(0x12, 0x34, 0x56)
        );
    }

    #[test]
    fn images_are_fetched_and_sized() {
        let png = {
            let image = RasterImage::solid(40, 20, [255, 0, 0, 255]);
            let mut out = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(
                image::RgbaImage::from_raw(40, 20, image.pixels).unwrap(),
            )
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
            out.into_inner()
        };
        let loader = StaticLoader::new()
            .with_html(
                "https://example.com/",
                "<body style=\"margin:0\"><img src=\"pic.png\"></body>",
            )
            .with(
                "https://example.com/pic.png",
                Resource::new("https://example.com/pic.png", "image/png", png),
            );
        let page = Page::load(
            Address::parse("https://example.com/").unwrap(),
            &loader,
            fonts(),
            theme(),
            Size2D::new(400.0, 300.0),
            false,
        );
        assert_eq!(page.images().len(), 1);
        let img = page.document().query("img").unwrap();
        let index = page.layout_tree().box_for_node(img).unwrap();
        assert_eq!(page.layout_tree().get(index).rect.width, 40.0);
        assert_eq!(page.layout_tree().get(index).rect.height, 20.0);
    }

    #[test]
    fn a_broken_image_still_lays_out() {
        let loader = StaticLoader::new().with_html(
            "https://example.com/",
            "<img src=\"gone.png\" alt=\"missing\">",
        );
        let page = Page::load(
            Address::parse("https://example.com/").unwrap(),
            &loader,
            fonts(),
            theme(),
            Size2D::new(400.0, 300.0),
            false,
        );
        assert!(page.images().is_empty());
        let img = page.document().query("img").unwrap();
        assert!(page.layout_tree().box_for_node(img).is_some());
    }

    #[test]
    fn rendering_produces_a_canvas_of_the_viewport_size() {
        let page = page_from("<p>hello</p>");
        let canvas = page.render_to_canvas();
        assert_eq!(canvas.width(), 800);
        assert_eq!(canvas.height(), 600);
    }

    #[test]
    fn the_dark_theme_reaches_internal_pages() {
        let dark = Theme::default().resolve(true);
        let page = Page::load(
            Address::parse("about:home").unwrap(),
            &wat_net::OfflineLoader,
            fonts(),
            dark.clone(),
            Size2D::new(400.0, 300.0),
            false,
        );
        let html_node = page.document().query("html").unwrap();
        let background = page.styles().get(html_node).background_color;
        assert_eq!(background, dark.palette.canvas);
    }
}
