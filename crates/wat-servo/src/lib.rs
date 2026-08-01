//! WAT's interface on Servo's engine.
//!
//! This implements [`wat_web::Engine`] with Servo behind it, so the Liquid Glass
//! chrome, the input handling and the window shell stay exactly as they are and
//! only the part that turns a URL into pixels changes.
//!
//! The join is [`SoftwareRenderingContext`]. Servo renders through WebRender,
//! which normally means an OpenGL surface, and WAT composites in software — the
//! chrome's backdrop filters read the pixels underneath them out of a CPU buffer.
//! Servo's software context bridges that: it renders into memory, and
//! `read_to_image` hands the frame over to be blitted under the chrome. It is a
//! copy per frame, which a GPU compositor would not need, but it is the version
//! that does not require rewriting the interface too.
//!
//! This crate is deliberately outside the workspace. Building it builds Servo,
//! which means SpiderMonkey from source: tens of minutes and many gigabytes. The
//! ordinary `cargo build` should not pay that.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use dpi::PhysicalSize;
use euclid::{Point2D, Vector2D};
use servo::{
    DeviceIntRect, LoadStatus, RenderingContext, Scroll, Servo, ServoBuilder,
    SoftwareRenderingContext, WebView, WebViewBuilder, WebViewDelegate, WebViewPoint,
    WebViewVector,
};
use url::Url;

use wat_css::Color;
use wat_layout::geom::{Point, Rect, Size2D};
use wat_paint::{Canvas, Clip, RoundedRect};
use wat_theme::ResolvedTheme;
use wat_web::{Engine, NavigationError, TabView};

/// What a delegate learns about a tab, which is otherwise not readable back.
///
/// Servo reports titles, load progress and failures by calling into the embedder
/// rather than by exposing state, so each tab keeps a cell the delegate writes to
/// and the chrome reads from.
#[derive(Default)]
struct TabState {
    title: RefCell<Option<String>>,
    failed: Cell<bool>,
    loading: Cell<bool>,
    /// Set when Servo has a new frame; cleared when it is painted.
    frame_ready: Cell<bool>,
}

impl WebViewDelegate for TabState {
    fn notify_page_title_changed(&self, _webview: WebView, title: Option<String>) {
        *self.title.borrow_mut() = title;
    }

    fn notify_new_frame_ready(&self, _webview: WebView) {
        self.frame_ready.set(true);
    }

    fn notify_load_status_changed(&self, _webview: WebView, status: LoadStatus) {
        self.loading.set(status != LoadStatus::Complete);
        if status == LoadStatus::Started {
            self.failed.set(false);
        }
    }
}

struct ServoTab {
    id: u64,
    webview: WebView,
    state: Rc<TabState>,
    /// Tracked here because Servo's scroll offset is not queryable from the
    /// embedder side, and the chrome wants it for the scrollbar.
    scroll: Cell<(f32, f32)>,
}

/// The browser's engine, backed by Servo.
pub struct ServoEngine {
    servo: Servo,
    context: Rc<SoftwareRenderingContext>,
    tabs: Vec<ServoTab>,
    active: usize,
    next_id: u64,
    viewport: Size2D,
    scale: f32,
    search_template: String,
    home_url: String,
}

impl ServoEngine {
    /// Starts Servo and prepares a software surface of `viewport` at `scale`.
    ///
    /// `scale` is the device pixel ratio: the surface is that many times larger
    /// than the viewport, which is what Servo needs to lay out for a HiDPI
    /// display and what the chrome already works in.
    pub fn new(viewport: Size2D, scale: f32) -> Result<Self, String> {
        // Servo's networking needs a rustls provider installed before it starts,
        // and installing one twice is an error rather than a no-op.
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

        let context = Rc::new(
            SoftwareRenderingContext::new(surface_size(viewport, scale))
                .map_err(|error| format!("cannot create Servo's software surface: {error:?}"))?,
        );
        context
            .make_current()
            .map_err(|error| format!("cannot make Servo's surface current: {error:?}"))?;

        let servo = ServoBuilder::default().build();
        servo.setup_logging();

        Ok(ServoEngine {
            servo,
            context,
            tabs: Vec::new(),
            active: 0,
            next_id: 1,
            viewport,
            scale,
            search_template: "https://duckduckgo.com/?q={}".to_string(),
            home_url: "https://servo.org/".to_string(),
        })
    }

    pub fn set_search_template(&mut self, template: impl Into<String>) {
        self.search_template = template.into();
    }

    pub fn set_home_url(&mut self, url: impl Into<String>) {
        self.home_url = url.into();
    }

    pub fn home_url(&self) -> &str {
        &self.home_url
    }

    /// Turns what the user typed into a URL, searching if it is not one.
    fn resolve(&self, input: &str) -> Result<Url, NavigationError> {
        resolve_input(input, &self.search_template)
    }

    fn active_tab(&self) -> Option<&ServoTab> {
        self.tabs.get(self.active)
    }

    /// Lets Servo make progress. Everything it does is asynchronous, so this has
    /// to be pumped for a load to advance at all.
    pub fn pump(&self) {
        self.servo.spin_event_loop();
    }

    fn open(&mut self, url: Url, focus: bool) -> u64 {
        let state = Rc::new(TabState::default());
        state.loading.set(true);
        let webview = WebViewBuilder::new(&self.servo, self.context.clone())
            .url(url)
            .hidpi_scale_factor(euclid::Scale::new(self.scale))
            .delegate(state.clone())
            .build();
        webview.resize(surface_size(self.viewport, self.scale));
        if focus {
            webview.focus();
        }
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(ServoTab {
            id,
            webview,
            state,
            scroll: Cell::new((0.0, 0.0)),
        });
        if focus {
            self.active = self.tabs.len() - 1;
        }
        id
    }
}

/// Turns what the user typed into a URL, searching if it is not one.
///
/// A free function so it can be tested without starting Servo, which would spawn
/// a process constellation for the sake of parsing a string.
fn resolve_input(input: &str, search_template: &str) -> Result<Url, NavigationError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(NavigationError::BadAddress(input.to_string()));
    }
    if let Ok(url) = Url::parse(trimmed) {
        return Ok(url);
    }
    // A bare host with a dot is an address; anything else is a search.
    if !trimmed.contains(' ') && trimmed.contains('.') {
        if let Ok(url) = Url::parse(&format!("https://{trimmed}")) {
            return Ok(url);
        }
    }
    let query = urlencoding_encode(trimmed);
    Url::parse(&search_template.replace("{}", &query))
        .map_err(|error| NavigationError::BadAddress(error.to_string()))
}

/// The surface size in device pixels for a CSS-pixel viewport.
fn surface_size(viewport: Size2D, scale: f32) -> PhysicalSize<u32> {
    PhysicalSize::new(
        ((viewport.width * scale).max(1.0)) as u32,
        ((viewport.height * scale).max(1.0)) as u32,
    )
}

/// Percent-encodes a search term. Servo brings no URL encoder to the surface and
/// a search box only needs the reserved characters handled.
fn urlencoding_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

impl Engine for ServoEngine {
    fn tabs(&self) -> Vec<TabView> {
        self.tabs
            .iter()
            .map(|tab| {
                let url = tab
                    .webview
                    .url()
                    .map(|url| url.to_string())
                    .unwrap_or_default();
                let label = tab
                    .state
                    .title
                    .borrow()
                    .clone()
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or_else(|| {
                        if url.is_empty() {
                            "New tab".to_string()
                        } else {
                            url.clone()
                        }
                    });
                TabView {
                    id: tab.id,
                    label,
                    is_secure: url.starts_with("https:"),
                    failed: tab.state.failed.get(),
                    url,
                }
            })
            .collect()
    }

    fn active_index(&self) -> usize {
        self.active
    }

    fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    fn open_tab(&mut self, input: &str) -> u64 {
        match self.resolve(input) {
            Ok(url) => self.open(url, true),
            Err(_) => 0,
        }
    }

    fn open_tab_in_background(&mut self, url: &str) -> u64 {
        match self.resolve(url) {
            Ok(url) => self.open(url, false),
            Err(_) => 0,
        }
    }

    fn close_tab(&mut self, id: u64) {
        if let Some(index) = self.tabs.iter().position(|tab| tab.id == id) {
            self.tabs.remove(index);
            self.active = self.active.min(self.tabs.len().saturating_sub(1));
            if let Some(tab) = self.active_tab() {
                tab.webview.focus();
            }
        }
    }

    fn close_active_tab(&mut self) {
        if let Some(tab) = self.active_tab() {
            let id = tab.id;
            self.close_tab(id);
        }
    }

    fn select_tab(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() {
            return false;
        }
        self.active = index;
        self.tabs[index].webview.focus();
        true
    }

    fn navigate(&mut self, input: &str) -> Result<(), NavigationError> {
        let url = self.resolve(input)?;
        match self.active_tab() {
            Some(tab) => {
                tab.state.loading.set(true);
                tab.webview.load(url);
                Ok(())
            }
            None => {
                self.open(url, true);
                Ok(())
            }
        }
    }

    fn follow_link(&mut self, url: &str) {
        let _ = self.navigate(url);
    }

    fn reload(&mut self) {
        if let Some(tab) = self.active_tab() {
            tab.webview.reload();
        }
    }

    fn go_back(&mut self) -> bool {
        match self.active_tab() {
            Some(tab) => {
                tab.webview.go_back(1);
                true
            }
            None => false,
        }
    }

    fn go_forward(&mut self) -> bool {
        match self.active_tab() {
            Some(tab) => {
                tab.webview.go_forward(1);
                true
            }
            None => false,
        }
    }

    // Servo traverses history in its own process and does not report how deep it
    // is, so the shell is told it can always try; a traversal with nowhere to go
    // is a no-op there.
    fn can_go_back(&self) -> bool {
        !self.tabs.is_empty()
    }

    fn can_go_forward(&self) -> bool {
        !self.tabs.is_empty()
    }

    fn link_at(&self, _point: Point) -> Option<String> {
        // Servo hit-tests inside the engine and reports link targets through the
        // delegate as the cursor moves, rather than answering a query. The shell
        // uses this only to show a target in the status area, so nothing is lost
        // by leaving it unanswered until that is wired to the delegate.
        None
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        let Some(tab) = self.active_tab() else {
            return false;
        };
        let (x, y) = tab.scroll.get();
        tab.scroll.set((x + dx, (y + dy).max(0.0)));
        // Page pixels are CSS pixels, which is what the shell works in. The
        // sign is flipped because the shell reports how far the content moves and
        // Servo wants how far the scroll position advances.
        tab.webview.notify_scroll_event(
            Scroll::Delta(WebViewVector::Page(Vector2D::new(-dx, -dy))),
            WebViewPoint::Page(Point2D::new(0.0, 0.0)),
        );
        true
    }

    fn scroll_offset(&self) -> Point {
        match self.active_tab() {
            Some(tab) => {
                let (x, y) = tab.scroll.get();
                Point::new(x, y)
            }
            None => Point::new(0.0, 0.0),
        }
    }

    fn set_viewport(&mut self, viewport: Size2D, _coarse_pointer: bool) {
        if viewport == self.viewport {
            return;
        }
        self.viewport = viewport;
        let size = surface_size(viewport, self.scale);
        self.context.resize(size);
        for tab in &self.tabs {
            tab.webview.resize(size);
        }
    }

    fn set_theme(&mut self, theme: ResolvedTheme) {
        // Servo reads the preferred colour scheme as a preference, so the
        // interface's own light or dark choice reaches the page's media queries.
        self.servo.set_preference(
            "layout.color_scheme",
            servo::PrefValue::Str(if theme.dark {
                "dark".to_string()
            } else {
                "light".to_string()
            }),
        );
    }

    fn background_color(&self) -> Color {
        // Servo composites the page's own background into the frame it hands
        // back, so nothing needs painting behind it.
        Color::TRANSPARENT
    }

    fn paint(&self, canvas: &mut Canvas, area: Rect, corner_radius: f32, scale: f32) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        if area.is_empty() {
            return;
        }
        tab.webview.paint();
        self.context.present();
        tab.state.frame_ready.set(false);

        let width = (area.width * scale) as i32;
        let height = (area.height * scale) as i32;
        if width <= 0 || height <= 0 {
            return;
        }
        let Some(image) = self
            .context
            .read_to_image(DeviceIntRect::from_origin_and_size(
                servo::DeviceIntPoint::new(0, 0),
                servo::DeviceIntSize::new(width, height),
            ))
        else {
            return;
        };

        // Blit under the chrome, clipped to the same rounded rectangle the
        // interface would have clipped its own page to.
        let device_area = Rect::new(
            area.x * scale,
            area.y * scale,
            area.width * scale,
            area.height * scale,
        );
        let mut clip = Clip::from_rect(canvas.bounds());
        clip.push(RoundedRect::new(
            device_area,
            wat_style::Corners::all(corner_radius * scale),
        ));

        let origin_x = device_area.x as i32;
        let origin_y = device_area.y as i32;
        for (x, y, pixel) in image.enumerate_pixels() {
            let target_x = origin_x + x as i32;
            let target_y = origin_y + y as i32;
            if target_x < 0 || target_y < 0 {
                continue;
            }
            let (target_x, target_y) = (target_x as u32, target_y as u32);
            let coverage = clip.coverage(target_x as f32 + 0.5, target_y as f32 + 0.5);
            if coverage <= 0.0 {
                continue;
            }
            let [r, g, b, a] = pixel.0;
            canvas.blend(target_x, target_y, Color::rgba(r, g, b, a), coverage);
        }
    }

    fn has_pending_work(&self) -> bool {
        self.tabs
            .iter()
            .any(|tab| tab.state.loading.get() || tab.state.frame_ready.get())
    }

    fn run_pending_work(&mut self) -> bool {
        self.pump();
        self.tabs.iter().any(|tab| tab.state.frame_ready.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_host_becomes_an_address_and_a_phrase_becomes_a_search() {
        let search = "https://example.com/?q={}";
        assert_eq!(
            resolve_input("example.com", search).unwrap().as_str(),
            "https://example.com/"
        );
        assert_eq!(
            resolve_input("https://servo.org/", search)
                .unwrap()
                .as_str(),
            "https://servo.org/"
        );
        assert_eq!(
            resolve_input("hello world", search).unwrap().as_str(),
            "https://example.com/?q=hello+world"
        );
        // A single word with no dot is a search, not a host.
        assert_eq!(
            resolve_input("servo", search).unwrap().as_str(),
            "https://example.com/?q=servo"
        );
        assert!(resolve_input("   ", search).is_err());
    }

    #[test]
    fn a_surface_is_scaled_for_the_display() {
        assert_eq!(
            surface_size(Size2D::new(800.0, 600.0), 2.0),
            PhysicalSize::new(1600, 1200)
        );
        // Never zero: Servo refuses a surface smaller than one pixel.
        assert_eq!(
            surface_size(Size2D::new(0.0, 0.0), 1.0),
            PhysicalSize::new(1, 1)
        );
    }

    #[test]
    fn search_terms_are_encoded() {
        assert_eq!(urlencoding_encode("a b&c"), "a+b%26c");
        assert_eq!(urlencoding_encode("plain"), "plain");
    }
}
