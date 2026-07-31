//! The browser chrome: an adaptive Liquid Glass interface.
//!
//! The chrome is a pure function of the session plus a little interaction state.
//! It produces a [`DisplayList`], answers hit tests, and turns input into
//! [`UiAction`]s for the shell to apply — it never touches the engine itself,
//! which keeps it testable without a window.
//!
//! Two layouts are generated from the same widgets: a desktop layout with a tab
//! strip and a toolbar, and a mobile layout with a floating address pill and a
//! bottom navigation bar. The layout follows the window width, so a resized
//! desktop window becomes the mobile layout — which is also how the mobile build
//! is exercised on a development machine.

pub mod compose;
pub mod glass;
pub mod icons;
pub mod omnibox;

pub use compose::{page_viewport, render_window, render_window_into, window_display_list};
pub use glass::Elevation;
pub use icons::Icon;
pub use omnibox::Omnibox;

use wat_css::Color;
use wat_engine::Session;
use wat_layout::geom::{Point, Rect, Size2D};
use wat_paint::{DisplayItem, DisplayList, RoundedRect, TextItem};
use wat_style::{Corners, Cursor, Sides, TextDecoration};
use wat_text::{FontRequest, FontStore};
use wat_theme::ResolvedTheme;

/// Below this width the chrome switches to the mobile layout.
pub const MOBILE_BREAKPOINT: f32 = 640.0;

/// Which arrangement of the chrome is in use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChromeLayout {
    Desktop,
    Mobile,
}

impl ChromeLayout {
    /// The layout a window of this size should use.
    pub fn for_size(size: Size2D) -> ChromeLayout {
        if size.width < MOBILE_BREAKPOINT {
            ChromeLayout::Mobile
        } else {
            ChromeLayout::Desktop
        }
    }

    pub fn is_mobile(self) -> bool {
        self == ChromeLayout::Mobile
    }
}

/// Everything the chrome can be clicked on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetId {
    Back,
    Forward,
    Reload,
    Home,
    NewTab,
    Menu,
    Omnibox,
    Tab(usize),
    TabClose(usize),
    /// The tab counter on mobile, which opens the menu.
    TabCount,
    MenuItem(MenuItem),
    /// The web page itself.
    Content,
}

/// Entries in the overflow menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItem {
    NewTab,
    CloseTab,
    Reload,
    Home,
    ToggleAppearance,
    Settings,
    About,
    Quit,
}

impl MenuItem {
    pub fn label(self) -> &'static str {
        match self {
            MenuItem::NewTab => "New tab",
            MenuItem::CloseTab => "Close tab",
            MenuItem::Reload => "Reload",
            MenuItem::Home => "Home",
            MenuItem::ToggleAppearance => "Switch appearance",
            MenuItem::Settings => "Settings",
            MenuItem::About => "About What-A-Browser",
            MenuItem::Quit => "Quit",
        }
    }

    /// The menu, in order.
    pub fn all() -> &'static [MenuItem] {
        &[
            MenuItem::NewTab,
            MenuItem::CloseTab,
            MenuItem::Reload,
            MenuItem::Home,
            MenuItem::ToggleAppearance,
            MenuItem::Settings,
            MenuItem::About,
            MenuItem::Quit,
        ]
    }
}

/// What the shell should do in response to input.
#[derive(Clone, Debug, PartialEq)]
pub enum UiAction {
    GoBack,
    GoForward,
    Reload,
    GoHome,
    NewTab,
    CloseActiveTab,
    CloseTab(usize),
    SelectTab(usize),
    /// Navigate the active tab to text the user typed.
    Navigate(String),
    /// Follow a link that has already been resolved.
    OpenUrl(String),
    OpenUrlInNewTab(String),
    ToggleAppearance,
    ScrollContent(f32),
    Quit,
}

/// A keyboard event, in the terms the chrome cares about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Backspace,
    Delete,
    Enter,
    Escape,
    Tab,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    /// Function keys, by number.
    Function(u8),
}

/// Keyboard modifiers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// Command on macOS, Super elsewhere.
    pub meta: bool,
}

impl Modifiers {
    /// The platform's primary accelerator key.
    pub fn accel(self) -> bool {
        self.ctrl || self.meta
    }
}

/// The computed position of every part of the chrome.
#[derive(Clone, Debug, Default)]
pub struct ChromeGeometry {
    pub layout: Option<ChromeLayout>,
    /// The area the web page is drawn in.
    pub content: Rect,
    /// Glass panels, back to front.
    pub panels: Vec<RoundedRect>,
    /// Clickable widgets, innermost last so hit testing can take the last match.
    pub widgets: Vec<(WidgetId, Rect)>,
    pub omnibox: Rect,
    pub menu_panel: Option<RoundedRect>,
    pub progress: Option<Rect>,
}

impl ChromeGeometry {
    /// The widget at `point`, if any.
    pub fn hit(&self, point: Point) -> Option<WidgetId> {
        self.widgets
            .iter()
            .rev()
            .find(|(_, rect)| rect.contains(point))
            .map(|(id, _)| *id)
    }

    pub fn rect_of(&self, id: WidgetId) -> Option<Rect> {
        self.widgets
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .map(|(_, rect)| *rect)
    }
}

/// The browser chrome.
pub struct Chrome {
    theme: ResolvedTheme,
    size: Size2D,
    pub omnibox: Omnibox,
    menu_open: bool,
    hover: Option<WidgetId>,
    pressed: Option<WidgetId>,
    /// Load progress in `0.0..=1.0`; `None` hides the bar.
    pub progress: Option<f32>,
    /// Text shown at the bottom, normally a hovered link's target.
    pub status: Option<String>,
    /// Tab count the geometry was last built for.
    tab_count: usize,
    geometry: ChromeGeometry,
}

impl Chrome {
    pub fn new(theme: ResolvedTheme, size: Size2D) -> Self {
        let mut chrome = Chrome {
            theme,
            size,
            omnibox: Omnibox::new(),
            menu_open: false,
            hover: None,
            pressed: None,
            progress: None,
            status: None,
            tab_count: 1,
            geometry: ChromeGeometry::default(),
        };
        chrome.relayout(1);
        chrome
    }

    pub fn theme(&self) -> &ResolvedTheme {
        &self.theme
    }

    pub fn set_theme(&mut self, theme: ResolvedTheme) {
        self.theme = theme;
        let tabs = self.tab_count;
        self.relayout(tabs);
    }

    pub fn size(&self) -> Size2D {
        self.size
    }

    pub fn layout(&self) -> ChromeLayout {
        ChromeLayout::for_size(self.size)
    }

    pub fn menu_is_open(&self) -> bool {
        self.menu_open
    }

    pub fn geometry(&self) -> &ChromeGeometry {
        &self.geometry
    }

    /// The area the page should be laid out and drawn in.
    pub fn content_rect(&self) -> Rect {
        self.geometry.content
    }

    /// Recomputes geometry. Call after a resize, a tab change or a menu toggle.
    pub fn relayout(&mut self, tab_count: usize) {
        self.tab_count = tab_count;
        self.geometry = match self.layout() {
            ChromeLayout::Desktop => self.desktop_geometry(tab_count),
            ChromeLayout::Mobile => self.mobile_geometry(tab_count),
        };
    }

    /// Resizes the chrome, returning whether the layout kind changed.
    pub fn resize(&mut self, size: Size2D, tab_count: usize) -> bool {
        let before = self.layout();
        self.size = size;
        self.relayout(tab_count);
        before != self.layout()
    }

    fn desktop_geometry(&self, tab_count: usize) -> ChromeGeometry {
        let geometry = &self.theme.geometry;
        let inset = geometry.chrome_inset;
        let spacing = geometry.spacing;
        let panel_height = geometry.tab_strip_height + geometry.toolbar_height;
        let panel = Rect::new(
            inset,
            inset,
            (self.size.width - inset * 2.0).max(0.0),
            panel_height,
        );

        let mut widgets: Vec<(WidgetId, Rect)> = Vec::new();
        let content = Rect::new(
            0.0,
            panel.max_y() + inset,
            self.size.width,
            (self.size.height - panel.max_y() - inset).max(0.0),
        );
        // Content is first so every control hit-tests above it.
        widgets.push((WidgetId::Content, content));

        // ---- tab strip -----------------------------------------------------
        let strip = Rect::new(
            panel.x + spacing,
            panel.y + spacing * 0.5,
            (panel.width - spacing * 2.0).max(0.0),
            (geometry.tab_strip_height - spacing * 0.5).max(0.0),
        );
        let new_tab_size = (geometry.control_size * 0.8).min(strip.height.max(1.0));
        let tabs_area_width = (strip.width - new_tab_size - spacing).max(0.0);
        let tab_count = tab_count.max(1);
        let tab_width = (tabs_area_width / tab_count as f32).clamp(28.0, 220.0);
        for index in 0..tab_count {
            let x = strip.x + index as f32 * (tab_width + 2.0);
            if x + tab_width > strip.x + tabs_area_width + 2.0 {
                break;
            }
            let rect = Rect::new(x, strip.y, tab_width, strip.height);
            widgets.push((WidgetId::Tab(index), rect));
            // The close affordance only fits on a reasonably wide tab.
            if tab_width > 84.0 {
                let button = geometry.control_size * 0.5;
                widgets.push((
                    WidgetId::TabClose(index),
                    Rect::new(
                        rect.max_x() - button - spacing * 0.5,
                        rect.y + (rect.height - button) / 2.0,
                        button,
                        button,
                    ),
                ));
            }
        }
        widgets.push((
            WidgetId::NewTab,
            Rect::new(
                strip.x + tabs_area_width + spacing * 0.5,
                strip.y + (strip.height - new_tab_size) / 2.0,
                new_tab_size,
                new_tab_size,
            ),
        ));

        // ---- toolbar -------------------------------------------------------
        let toolbar = Rect::new(
            panel.x + spacing,
            panel.y + geometry.tab_strip_height,
            (panel.width - spacing * 2.0).max(0.0),
            geometry.toolbar_height,
        );
        let control = geometry.control_size.min(toolbar.height.max(1.0));
        let control_y = toolbar.y + (toolbar.height - control) / 2.0;
        let mut x = toolbar.x;
        for id in [WidgetId::Back, WidgetId::Forward, WidgetId::Reload] {
            widgets.push((id, Rect::new(x, control_y, control, control)));
            x += control + spacing * 0.5;
        }
        let menu_x = toolbar.max_x() - control;
        widgets.push((
            WidgetId::Menu,
            Rect::new(menu_x, control_y, control, control),
        ));
        let home_x = menu_x - control - spacing * 0.5;
        widgets.push((
            WidgetId::Home,
            Rect::new(home_x, control_y, control, control),
        ));

        let omnibox = Rect::new(
            x + spacing * 0.5,
            control_y,
            (home_x - x - spacing * 1.5).max(0.0),
            control,
        );
        widgets.push((WidgetId::Omnibox, omnibox));

        let progress = self.progress.map(|_| {
            Rect::new(
                panel.x,
                panel.max_y() - geometry.progress_height,
                panel.width,
                geometry.progress_height,
            )
        });

        let mut chrome_geometry = ChromeGeometry {
            layout: Some(ChromeLayout::Desktop),
            content,
            panels: vec![RoundedRect::new(panel, Corners::all(geometry.radius_large))],
            widgets,
            omnibox,
            menu_panel: None,
            progress,
        };
        if self.menu_open {
            self.add_menu(&mut chrome_geometry, false);
        }
        chrome_geometry
    }

    fn mobile_geometry(&self, tab_count: usize) -> ChromeGeometry {
        let geometry = &self.theme.geometry;
        let inset = geometry.chrome_inset;
        let spacing = geometry.spacing;
        let _ = tab_count;

        let top = Rect::new(
            inset,
            inset,
            (self.size.width - inset * 2.0).max(0.0),
            geometry.mobile_top_height.min(self.size.height / 3.0),
        );
        let bottom_height = geometry.mobile_bottom_height.min(self.size.height / 3.0);
        let bottom = Rect::new(
            inset,
            (self.size.height - inset - bottom_height).max(top.max_y()),
            (self.size.width - inset * 2.0).max(0.0),
            bottom_height,
        );

        let mut widgets: Vec<(WidgetId, Rect)> = Vec::new();
        let content = Rect::new(
            0.0,
            top.max_y() + inset,
            self.size.width,
            (bottom.y - top.max_y() - inset * 2.0).max(0.0),
        );
        widgets.push((WidgetId::Content, content));

        // The whole top panel is the address field.
        let omnibox = top.inset(Sides::all(spacing));
        widgets.push((WidgetId::Omnibox, omnibox));

        // Bottom bar: four evenly spaced controls. Touch targets are larger than
        // the desktop's, which is what makes them hittable with a thumb.
        let control = (geometry.control_size * 1.3).min(bottom.height.max(1.0));
        let ids = [
            WidgetId::Back,
            WidgetId::Forward,
            WidgetId::TabCount,
            WidgetId::Menu,
        ];
        let slot = bottom.width / ids.len() as f32;
        for (index, id) in ids.iter().enumerate() {
            let center_x = bottom.x + slot * (index as f32 + 0.5);
            widgets.push((
                *id,
                Rect::new(
                    center_x - control / 2.0,
                    bottom.y + (bottom.height - control) / 2.0,
                    control,
                    control,
                ),
            ));
        }

        let progress = self.progress.map(|_| {
            Rect::new(
                top.x,
                top.max_y() - geometry.progress_height,
                top.width,
                geometry.progress_height,
            )
        });

        let mut chrome_geometry = ChromeGeometry {
            layout: Some(ChromeLayout::Mobile),
            content,
            panels: vec![
                RoundedRect::new(top, Corners::all(geometry.radius_large)),
                RoundedRect::new(bottom, Corners::all(geometry.radius_large)),
            ],
            widgets,
            omnibox,
            menu_panel: None,
            progress,
        };
        if self.menu_open {
            self.add_menu(&mut chrome_geometry, true);
        }
        chrome_geometry
    }

    /// Adds the overflow menu's panel and items to `geometry`.
    fn add_menu(&self, geometry: &mut ChromeGeometry, mobile: bool) {
        let metrics = &self.theme.geometry;
        let item_height = metrics.control_size + metrics.spacing * 0.5;
        let padding = metrics.spacing;
        let items = MenuItem::all();
        let width = 232.0f32.min((self.size.width - metrics.chrome_inset * 2.0).max(80.0));
        let height = (items.len() as f32 * item_height + padding * 2.0)
            .min((self.size.height - metrics.chrome_inset * 2.0).max(40.0));

        let anchor = geometry.rect_of(WidgetId::Menu).unwrap_or(Rect::new(
            self.size.width - 48.0,
            48.0,
            32.0,
            32.0,
        ));
        let x = (anchor.max_x() - width).max(metrics.chrome_inset);
        let y = if mobile {
            (anchor.y - height - metrics.spacing).max(metrics.chrome_inset)
        } else {
            anchor.max_y() + metrics.spacing
        };
        let panel = Rect::new(x, y, width, height);

        for (index, item) in items.iter().enumerate() {
            let rect = Rect::new(
                panel.x + padding,
                panel.y + padding + index as f32 * item_height,
                (panel.width - padding * 2.0).max(0.0),
                item_height,
            );
            if rect.max_y() > panel.max_y() - padding * 0.5 {
                break;
            }
            geometry.widgets.push((WidgetId::MenuItem(*item), rect));
        }
        geometry.menu_panel = Some(RoundedRect::new(panel, Corners::all(metrics.radius_medium)));
    }

    // ---- input ------------------------------------------------------------

    /// Updates the hovered widget, returning whether a repaint is needed.
    pub fn pointer_moved(&mut self, point: Option<Point>) -> bool {
        let hover = point.and_then(|point| self.geometry.hit(point));
        if hover == self.hover {
            return false;
        }
        self.hover = hover;
        true
    }

    /// Records a press, returning whether a repaint is needed.
    pub fn pointer_down(&mut self, point: Point) -> bool {
        let pressed = self.geometry.hit(point);
        let changed = pressed != self.pressed;
        self.pressed = pressed;
        changed
    }

    /// Completes a click, producing an action if one is warranted.
    ///
    /// `link` is the link under the pointer in the page, which the shell resolves
    /// through the engine.
    pub fn pointer_up(&mut self, point: Point, link: Option<&str>) -> Option<UiAction> {
        let pressed = self.pressed.take();
        let hit = self.geometry.hit(point);
        // A click only counts if it went down and up on the same widget.
        if pressed.is_some() && pressed != hit {
            return None;
        }
        let widget = hit?;

        // Any click outside the menu closes it without acting.
        if self.menu_open && !matches!(widget, WidgetId::MenuItem(_) | WidgetId::Menu) {
            self.close_menu();
            return None;
        }

        match widget {
            WidgetId::Back => Some(UiAction::GoBack),
            WidgetId::Forward => Some(UiAction::GoForward),
            WidgetId::Reload => Some(UiAction::Reload),
            WidgetId::Home => Some(UiAction::GoHome),
            WidgetId::NewTab => Some(UiAction::NewTab),
            WidgetId::Menu | WidgetId::TabCount => {
                self.toggle_menu();
                None
            }
            WidgetId::Omnibox => {
                self.omnibox.focus();
                None
            }
            WidgetId::Tab(index) => Some(UiAction::SelectTab(index)),
            WidgetId::TabClose(index) => Some(UiAction::CloseTab(index)),
            WidgetId::MenuItem(item) => {
                self.close_menu();
                Some(match item {
                    MenuItem::NewTab => UiAction::NewTab,
                    MenuItem::CloseTab => UiAction::CloseActiveTab,
                    MenuItem::Reload => UiAction::Reload,
                    MenuItem::Home => UiAction::GoHome,
                    MenuItem::ToggleAppearance => UiAction::ToggleAppearance,
                    MenuItem::Settings => UiAction::OpenUrl("about:settings".into()),
                    MenuItem::About => UiAction::OpenUrl("about:version".into()),
                    MenuItem::Quit => UiAction::Quit,
                })
            }
            WidgetId::Content => {
                if self.omnibox.is_focused() {
                    self.omnibox.blur();
                }
                link.map(|url| UiAction::OpenUrl(url.to_string()))
            }
        }
    }

    /// A middle click opens a link in a background tab, or closes a tab.
    pub fn middle_click(&mut self, point: Point, link: Option<&str>) -> Option<UiAction> {
        match self.geometry.hit(point) {
            Some(WidgetId::Content) => link.map(|url| UiAction::OpenUrlInNewTab(url.to_string())),
            Some(WidgetId::Tab(index)) => Some(UiAction::CloseTab(index)),
            _ => None,
        }
    }

    /// Handles a key press.
    pub fn key_pressed(&mut self, key: Key, modifiers: Modifiers) -> Option<UiAction> {
        // Accelerators come first, whether or not the field has focus.
        if modifiers.accel() {
            match &key {
                Key::Char('l') => {
                    self.omnibox.focus();
                    return None;
                }
                Key::Char('t') => return Some(UiAction::NewTab),
                Key::Char('w') => return Some(UiAction::CloseActiveTab),
                Key::Char('r') => return Some(UiAction::Reload),
                Key::Char('q') => return Some(UiAction::Quit),
                Key::Char('d') => return Some(UiAction::ToggleAppearance),
                Key::Left => return Some(UiAction::GoBack),
                Key::Right => return Some(UiAction::GoForward),
                Key::Char('a') if self.omnibox.is_focused() => {
                    self.omnibox.select_all();
                    return None;
                }
                Key::Backspace if self.omnibox.is_focused() => {
                    self.omnibox.delete_word();
                    return None;
                }
                Key::Char(digit @ '1'..='9') => {
                    let index = *digit as usize - '1' as usize;
                    return Some(UiAction::SelectTab(index));
                }
                _ => {}
            }
        }

        if self.omnibox.is_focused() {
            return self.key_in_omnibox(key);
        }

        match key {
            Key::Function(5) => Some(UiAction::Reload),
            Key::Escape if self.menu_open => {
                self.close_menu();
                None
            }
            Key::Down => Some(UiAction::ScrollContent(60.0)),
            Key::Up => Some(UiAction::ScrollContent(-60.0)),
            Key::PageDown | Key::Char(' ') => {
                Some(UiAction::ScrollContent(self.content_rect().height * 0.9))
            }
            Key::PageUp => Some(UiAction::ScrollContent(-self.content_rect().height * 0.9)),
            Key::Home => Some(UiAction::ScrollContent(-1.0e9)),
            Key::End => Some(UiAction::ScrollContent(1.0e9)),
            Key::Char('/') => {
                self.omnibox.focus();
                None
            }
            _ => None,
        }
    }

    fn key_in_omnibox(&mut self, key: Key) -> Option<UiAction> {
        match key {
            Key::Char(ch) if !ch.is_control() => {
                self.omnibox.insert(&ch.to_string());
                None
            }
            Key::Backspace => {
                self.omnibox.backspace();
                None
            }
            Key::Delete => {
                self.omnibox.delete();
                None
            }
            Key::Left => {
                self.omnibox.move_left();
                None
            }
            Key::Right => {
                self.omnibox.move_right();
                None
            }
            Key::Home => {
                self.omnibox.move_home();
                None
            }
            Key::End => {
                self.omnibox.move_end();
                None
            }
            Key::Enter => {
                let text = self.omnibox.commit();
                (!text.is_empty()).then_some(UiAction::Navigate(text))
            }
            Key::Escape => {
                self.omnibox.blur();
                None
            }
            _ => None,
        }
    }

    /// Handles a scroll wheel or trackpad gesture.
    pub fn scroll(&self, point: Point, delta_y: f32) -> Option<UiAction> {
        match self.geometry.hit(point) {
            Some(WidgetId::Content) | None => Some(UiAction::ScrollContent(delta_y)),
            _ => None,
        }
    }

    pub fn toggle_menu(&mut self) {
        self.menu_open = !self.menu_open;
        let tabs = self.tab_count;
        self.relayout(tabs);
    }

    pub fn close_menu(&mut self) {
        if self.menu_open {
            self.menu_open = false;
            let tabs = self.tab_count;
            self.relayout(tabs);
        }
    }

    /// The cursor to show at `point`.
    pub fn cursor_at(&self, point: Point, content_cursor: Cursor) -> Cursor {
        match self.geometry.hit(point) {
            Some(WidgetId::Content) => content_cursor,
            Some(WidgetId::Omnibox) => Cursor::Text,
            Some(_) => Cursor::Pointer,
            None => Cursor::Default,
        }
    }

    // ---- drawing -----------------------------------------------------------

    fn ui_font(&self, size: f32, weight: u16) -> FontRequest {
        FontRequest {
            families: self.theme.typography.ui_family.clone(),
            weight,
            italic: false,
            size,
            letter_spacing: 0.0,
            word_spacing: 0.0,
        }
    }

    /// Pushes a text run, truncating with an ellipsis if it will not fit.
    #[allow(clippy::too_many_arguments)]
    fn text(
        &self,
        list: &mut DisplayList,
        fonts: &FontStore,
        content: &str,
        x: f32,
        center_y: f32,
        font: FontRequest,
        color: Color,
        max_width: f32,
    ) {
        if content.is_empty() || max_width <= 4.0 {
            return;
        }
        let metrics = fonts.line_metrics(&font);
        let baseline = center_y + (metrics.ascent - metrics.descent) / 2.0;

        let mut shown = content.to_string();
        if fonts.measure(&font, &shown) > max_width {
            let ellipsis = "…";
            let ellipsis_width = fonts.measure(&font, ellipsis);
            while !shown.is_empty() && fonts.measure(&font, &shown) + ellipsis_width > max_width {
                shown.pop();
            }
            shown.push_str(ellipsis);
        }

        list.push(DisplayItem::Text(TextItem {
            x,
            baseline,
            text: shown,
            font,
            color,
            extra_word_spacing: 0.0,
            decoration: TextDecoration::default(),
            decoration_color: color,
            shadows: Vec::new(),
        }));
    }

    /// The hover/press wash behind a control.
    fn control_background(&self, list: &mut DisplayList, id: WidgetId, rect: Rect) {
        let hovered = self.hover == Some(id);
        let pressed = self.pressed == Some(id);
        if !hovered && !pressed {
            return;
        }
        let fill = self.theme.control_fill(hovered, pressed);
        if fill.is_transparent() {
            return;
        }
        glass::solid(
            list,
            RoundedRect::new(rect, Corners::all(rect.height / 2.0)),
            fill,
        );
    }

    fn icon_color(&self, enabled: bool) -> Color {
        if enabled {
            self.theme.palette.text
        } else {
            // Dim enough to read as unavailable, not so dim it disappears.
            self.theme.palette.text_muted.scale_alpha(0.62)
        }
    }

    fn control(&self, list: &mut DisplayList, id: WidgetId, icon: Icon, enabled: bool) {
        let Some(rect) = self.geometry.rect_of(id) else {
            return;
        };
        self.control_background(list, id, rect);
        icons::draw(
            list,
            icon,
            rect.inset(Sides::all(rect.width * 0.26)),
            self.icon_color(enabled),
        );
    }

    /// Builds the chrome's display list for the current session state.
    pub fn build(&self, fonts: &FontStore, session: &Session) -> DisplayList {
        let mut list = DisplayList::new();
        let theme = &self.theme;

        // Panels first, so everything else lands on the glass.
        for panel in &self.geometry.panels {
            glass::surface(&mut list, *panel, theme, Elevation::Raised);
        }

        match self.layout() {
            ChromeLayout::Desktop => self.build_desktop(&mut list, fonts, session),
            ChromeLayout::Mobile => self.build_mobile(&mut list, fonts, session),
        }

        // The load progress bar hugs the bottom edge of the top panel.
        if let (Some(rect), Some(progress)) = (self.geometry.progress, self.progress) {
            let filled = Rect::new(
                rect.x,
                rect.y,
                rect.width * progress.clamp(0.0, 1.0),
                rect.height,
            );
            glass::solid(
                &mut list,
                RoundedRect::new(filled, Corners::all(rect.height / 2.0)),
                theme.palette.accent,
            );
        }

        if let Some(status) = &self.status {
            self.build_status(&mut list, fonts, status);
        }

        if let Some(panel) = self.geometry.menu_panel {
            self.build_menu(&mut list, fonts, panel);
        }

        list
    }

    fn build_desktop(&self, list: &mut DisplayList, fonts: &FontStore, session: &Session) {
        let theme = &self.theme;
        let active = session.active_index();

        // ---- tabs ----------------------------------------------------------
        for (id, rect) in &self.geometry.widgets {
            let WidgetId::Tab(index) = id else { continue };
            let tab = session.tabs().get(*index);
            let is_active = *index == active && tab.is_some();
            let shape = RoundedRect::new(*rect, Corners::all(theme.geometry.radius_small));

            if is_active {
                glass::surface_tinted(
                    list,
                    shape,
                    theme,
                    Elevation::Flush,
                    Some(theme.palette.accent_soft),
                );
            } else if self.hover == Some(*id) {
                glass::solid(list, shape, theme.control_fill(true, false));
            }

            let padding = theme.geometry.spacing;
            let close_rect = self.geometry.rect_of(WidgetId::TabClose(*index));
            let label_width = match close_rect {
                Some(close) => (close.x - rect.x - padding * 1.5).max(0.0),
                None => (rect.width - padding * 2.0).max(0.0),
            };
            let label = tab.map(|tab| tab.label()).unwrap_or_default();
            let weight = if is_active {
                theme.typography.weight_medium
            } else {
                theme.typography.weight_normal
            };
            self.text(
                list,
                fonts,
                &label,
                rect.x + padding,
                rect.center().y,
                self.ui_font(theme.typography.size_small, weight),
                if is_active {
                    theme.palette.text
                } else {
                    theme.palette.text_muted
                },
                label_width,
            );

            if let Some(close) = close_rect {
                let close_id = WidgetId::TabClose(*index);
                self.control_background(list, close_id, close);
                icons::draw(
                    list,
                    Icon::Close,
                    close.inset(Sides::all(close.width * 0.28)),
                    if self.hover == Some(close_id) {
                        theme.palette.text
                    } else {
                        theme.palette.text_muted
                    },
                );
            }
        }
        self.control(list, WidgetId::NewTab, Icon::Plus, true);

        // ---- toolbar -------------------------------------------------------
        let can_back = session
            .active()
            .is_some_and(|tab| tab.history.can_go_back());
        let can_forward = session
            .active()
            .is_some_and(|tab| tab.history.can_go_forward());
        self.control(list, WidgetId::Back, Icon::ChevronLeft, can_back);
        self.control(list, WidgetId::Forward, Icon::ChevronRight, can_forward);
        self.control(list, WidgetId::Reload, Icon::Reload, true);
        self.control(list, WidgetId::Home, Icon::Home, true);
        self.control(list, WidgetId::Menu, Icon::Menu, true);

        self.build_omnibox(list, fonts, session);
    }

    fn build_mobile(&self, list: &mut DisplayList, fonts: &FontStore, session: &Session) {
        self.build_omnibox(list, fonts, session);

        let can_back = session
            .active()
            .is_some_and(|tab| tab.history.can_go_back());
        let can_forward = session
            .active()
            .is_some_and(|tab| tab.history.can_go_forward());
        self.control(list, WidgetId::Back, Icon::ChevronLeft, can_back);
        self.control(list, WidgetId::Forward, Icon::ChevronRight, can_forward);
        self.control(list, WidgetId::Menu, Icon::Menu, true);

        // The tab counter: a bordered square with the number of tabs in it, the
        // way phone browsers show it.
        if let Some(rect) = self.geometry.rect_of(WidgetId::TabCount) {
            self.control_background(list, WidgetId::TabCount, rect);
            let box_size = rect.width * 0.62;
            let square = Rect::new(
                rect.center().x - box_size / 2.0,
                rect.center().y - box_size / 2.0,
                box_size,
                box_size,
            );
            let stroke = (box_size * 0.09).max(1.5);
            list.push(DisplayItem::Border {
                shape: RoundedRect::new(square, Corners::all(box_size * 0.26)),
                widths: Sides::all(stroke),
                colors: Sides::all(self.icon_color(true)),
            });

            let count = session.tab_count().to_string();
            let font = self.ui_font(box_size * 0.56, self.theme.typography.weight_bold);
            let width = fonts.measure(&font, &count);
            self.text(
                list,
                fonts,
                &count,
                square.center().x - width / 2.0,
                square.center().y,
                font,
                self.theme.palette.text,
                box_size,
            );
        }
    }

    fn build_omnibox(&self, list: &mut DisplayList, fonts: &FontStore, session: &Session) {
        let theme = &self.theme;
        let rect = self.geometry.omnibox;
        if rect.is_empty() {
            return;
        }
        let shape = RoundedRect::new(rect, Corners::all(rect.height / 2.0));
        glass::well(list, shape, theme);
        if self.omnibox.is_focused() {
            glass::focus_ring(list, shape, theme);
        }

        let padding = theme.geometry.spacing;
        let icon_size = rect.height * 0.55;
        let icon_rect = Rect::new(
            rect.x + padding * 0.75,
            rect.center().y - icon_size / 2.0,
            icon_size,
            icon_size,
        );

        let secure = session.active().is_some_and(|tab| tab.is_secure());
        if self.omnibox.is_focused() {
            icons::draw(list, Icon::Search, icon_rect, theme.palette.text_muted);
        } else if secure {
            icons::draw(list, Icon::Lock, icon_rect, theme.palette.success);
        } else {
            icons::draw(list, Icon::Warning, icon_rect, theme.palette.warning);
        }

        let text_x = icon_rect.max_x() + padding * 0.75;
        let available = (rect.max_x() - text_x - padding).max(0.0);
        let font = self.ui_font(theme.typography.size_base, theme.typography.weight_normal);

        let content = self.omnibox.visible_text();
        let (shown, color) = if content.is_empty() {
            ("Search or enter address", theme.palette.text_muted)
        } else {
            (content, theme.palette.text)
        };

        // A full selection is drawn as a highlight behind the text.
        if self.omnibox.is_focused() && self.omnibox.all_selected() && !content.is_empty() {
            let width = fonts.measure(&font, content).min(available);
            glass::solid(
                list,
                RoundedRect::new(
                    Rect::new(
                        text_x - 2.0,
                        rect.center().y - font.size * 0.72,
                        width + 4.0,
                        font.size * 1.44,
                    ),
                    Corners::all(3.0),
                ),
                theme.palette.accent.with_alpha(0.28),
            );
        }

        self.text(
            list,
            fonts,
            shown,
            text_x,
            rect.center().y,
            font.clone(),
            color,
            available,
        );

        // Caret.
        if self.omnibox.is_focused() && !self.omnibox.all_selected() {
            let before: String = self
                .omnibox
                .text()
                .chars()
                .take(self.omnibox.caret())
                .collect();
            let offset = fonts.measure(&font, &before).min(available);
            glass::solid(
                list,
                RoundedRect::sharp(Rect::new(
                    text_x + offset,
                    rect.center().y - font.size * 0.6,
                    1.5,
                    font.size * 1.2,
                )),
                theme.palette.accent,
            );
        }
    }

    fn build_status(&self, list: &mut DisplayList, fonts: &FontStore, status: &str) {
        let theme = &self.theme;
        let font = self.ui_font(theme.typography.size_small, theme.typography.weight_normal);
        let padding = theme.geometry.spacing;
        let width = (fonts.measure(&font, status) + padding * 2.0)
            .min((self.size.width - theme.geometry.chrome_inset * 2.0).max(0.0));
        let height = theme.typography.size_small * 2.0;
        let rect = Rect::new(
            theme.geometry.chrome_inset,
            self.geometry.content.max_y() - height - theme.geometry.chrome_inset,
            width,
            height,
        );
        let shape = RoundedRect::new(rect, Corners::all(height / 2.0));
        glass::surface(list, shape, theme, Elevation::Raised);
        self.text(
            list,
            fonts,
            status,
            rect.x + padding,
            rect.center().y,
            font,
            theme.palette.text_muted,
            (width - padding * 2.0).max(0.0),
        );
    }

    fn build_menu(&self, list: &mut DisplayList, fonts: &FontStore, panel: RoundedRect) {
        let theme = &self.theme;
        glass::surface(list, panel, theme, Elevation::Overlay);

        for (id, rect) in &self.geometry.widgets {
            let WidgetId::MenuItem(item) = id else {
                continue;
            };
            if self.hover == Some(*id) {
                glass::solid(
                    list,
                    RoundedRect::new(*rect, Corners::all(theme.geometry.radius_small)),
                    theme.palette.accent_soft,
                );
            }
            let icon = match item {
                MenuItem::NewTab => Some(Icon::Plus),
                MenuItem::CloseTab => Some(Icon::Close),
                MenuItem::Reload => Some(Icon::Reload),
                MenuItem::Home => Some(Icon::Home),
                MenuItem::ToggleAppearance => Some(if theme.dark { Icon::Sun } else { Icon::Moon }),
                _ => None,
            };
            let padding = theme.geometry.spacing;
            let mut text_x = rect.x + padding;
            if let Some(icon) = icon {
                let size = rect.height * 0.5;
                icons::draw(
                    list,
                    icon,
                    Rect::new(rect.x + padding, rect.center().y - size / 2.0, size, size),
                    theme.palette.text_muted,
                );
                text_x = rect.x + padding + size + padding * 0.75;
            }
            self.text(
                list,
                fonts,
                item.label(),
                text_x,
                rect.center().y,
                self.ui_font(theme.typography.size_base, theme.typography.weight_normal),
                if *item == MenuItem::Quit {
                    theme.palette.danger
                } else {
                    theme.palette.text
                },
                (rect.max_x() - text_x - padding).max(0.0),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;
    use wat_net::StaticLoader;
    use wat_theme::Theme;

    fn theme() -> ResolvedTheme {
        Theme::default().resolve(false)
    }

    fn session_with(tabs: usize) -> Session {
        let loader = StaticLoader::new()
            .with_html("https://a.example/", "<title>Alpha</title><p>a</p>")
            .with_html("https://b.example/", "<title>Beta</title><p>b</p>")
            .with_html("https://c.example/", "<title>Gamma</title><p>c</p>");
        let mut session = Session::new(
            Rc::new(FontStore::empty()),
            theme(),
            Size2D::new(800.0, 600.0),
            false,
        );
        let urls = [
            "https://a.example/",
            "https://b.example/",
            "https://c.example/",
        ];
        for index in 0..tabs {
            session.open_tab(urls[index % urls.len()], &loader);
        }
        session
    }

    fn desktop_chrome(tabs: usize) -> Chrome {
        let mut chrome = Chrome::new(theme(), Size2D::new(1200.0, 800.0));
        chrome.relayout(tabs);
        chrome
    }

    fn mobile_chrome(tabs: usize) -> Chrome {
        let mut chrome = Chrome::new(theme(), Size2D::new(390.0, 844.0));
        chrome.relayout(tabs);
        chrome
    }

    fn texts(list: &DisplayList) -> Vec<String> {
        list.items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Text(text) => Some(text.text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn layout_follows_the_window_width() {
        assert_eq!(
            ChromeLayout::for_size(Size2D::new(1200.0, 800.0)),
            ChromeLayout::Desktop
        );
        assert_eq!(
            ChromeLayout::for_size(Size2D::new(390.0, 844.0)),
            ChromeLayout::Mobile
        );
        assert_eq!(
            ChromeLayout::for_size(Size2D::new(MOBILE_BREAKPOINT, 800.0)),
            ChromeLayout::Desktop,
            "the breakpoint itself is desktop"
        );
    }

    #[test]
    fn resizing_across_the_breakpoint_is_reported() {
        let mut chrome = desktop_chrome(1);
        assert!(chrome.resize(Size2D::new(400.0, 800.0), 1));
        assert!(chrome.layout().is_mobile());
        assert!(!chrome.resize(Size2D::new(380.0, 800.0), 1), "still mobile");
        assert!(chrome.resize(Size2D::new(900.0, 800.0), 1));
    }

    #[test]
    fn the_content_area_sits_below_the_desktop_chrome() {
        let chrome = desktop_chrome(2);
        let content = chrome.content_rect();
        let panel = chrome.geometry().panels[0].rect;
        assert!(content.y > panel.max_y());
        assert_eq!(content.width, 1200.0);
        assert!(content.height > 600.0);
    }

    #[test]
    fn mobile_content_sits_between_the_two_bars() {
        let chrome = mobile_chrome(1);
        let content = chrome.content_rect();
        let panels = &chrome.geometry().panels;
        assert_eq!(panels.len(), 2, "a top pill and a bottom bar");
        assert!(content.y > panels[0].rect.max_y());
        assert!(content.max_y() < panels[1].rect.y);
    }

    #[test]
    fn desktop_layout_has_a_tab_per_tab() {
        let chrome = desktop_chrome(3);
        let tabs: Vec<usize> = chrome
            .geometry()
            .widgets
            .iter()
            .filter_map(|(id, _)| match id {
                WidgetId::Tab(index) => Some(*index),
                _ => None,
            })
            .collect();
        assert_eq!(tabs, vec![0, 1, 2]);
        assert!(chrome.geometry().rect_of(WidgetId::NewTab).is_some());
    }

    #[test]
    fn tabs_do_not_overflow_the_strip() {
        let chrome = desktop_chrome(40);
        let strip_right = chrome.geometry().panels[0].rect.max_x();
        for (id, rect) in &chrome.geometry().widgets {
            if matches!(id, WidgetId::Tab(_)) {
                assert!(
                    rect.max_x() <= strip_right + 1.0,
                    "tab {rect:?} overflows {strip_right}"
                );
            }
        }
    }

    #[test]
    fn narrow_tabs_drop_their_close_button() {
        let wide = desktop_chrome(2);
        assert!(wide.geometry().rect_of(WidgetId::TabClose(0)).is_some());
        let crowded = desktop_chrome(30);
        assert!(crowded.geometry().rect_of(WidgetId::TabClose(0)).is_none());
    }

    #[test]
    fn mobile_layout_has_no_tab_strip() {
        let chrome = mobile_chrome(3);
        assert!(!chrome
            .geometry()
            .widgets
            .iter()
            .any(|(id, _)| matches!(id, WidgetId::Tab(_))));
        assert!(chrome.geometry().rect_of(WidgetId::TabCount).is_some());
    }

    #[test]
    fn widgets_do_not_overlap_the_omnibox() {
        for chrome in [desktop_chrome(2), mobile_chrome(2)] {
            let omnibox = chrome.geometry().omnibox;
            for (id, rect) in &chrome.geometry().widgets {
                if matches!(id, WidgetId::Omnibox | WidgetId::Content) {
                    continue;
                }
                assert!(
                    !rect.intersects(&omnibox),
                    "{id:?} at {rect:?} overlaps the omnibox at {omnibox:?}"
                );
            }
        }
    }

    #[test]
    fn hit_testing_finds_controls_and_the_page() {
        let chrome = desktop_chrome(2);
        let back = chrome.geometry().rect_of(WidgetId::Back).unwrap();
        assert_eq!(chrome.geometry().hit(back.center()), Some(WidgetId::Back));

        let content = chrome.content_rect();
        assert_eq!(
            chrome.geometry().hit(content.center()),
            Some(WidgetId::Content)
        );
        assert_eq!(chrome.geometry().hit(Point::new(-5.0, -5.0)), None);
    }

    #[test]
    fn clicking_navigation_controls_produces_actions() {
        let mut chrome = desktop_chrome(2);
        let click = |chrome: &mut Chrome, id: WidgetId| {
            let rect = chrome.geometry().rect_of(id).unwrap();
            chrome.pointer_down(rect.center());
            chrome.pointer_up(rect.center(), None)
        };
        assert_eq!(click(&mut chrome, WidgetId::Back), Some(UiAction::GoBack));
        assert_eq!(
            click(&mut chrome, WidgetId::Forward),
            Some(UiAction::GoForward)
        );
        assert_eq!(click(&mut chrome, WidgetId::Reload), Some(UiAction::Reload));
        assert_eq!(click(&mut chrome, WidgetId::Home), Some(UiAction::GoHome));
        assert_eq!(click(&mut chrome, WidgetId::NewTab), Some(UiAction::NewTab));
        assert_eq!(
            click(&mut chrome, WidgetId::Tab(1)),
            Some(UiAction::SelectTab(1))
        );
        assert_eq!(
            click(&mut chrome, WidgetId::TabClose(0)),
            Some(UiAction::CloseTab(0))
        );
    }

    #[test]
    fn a_drag_off_a_control_does_not_click_it() {
        let mut chrome = desktop_chrome(2);
        let back = chrome.geometry().rect_of(WidgetId::Back).unwrap();
        let content = chrome.content_rect();
        chrome.pointer_down(back.center());
        assert_eq!(chrome.pointer_up(content.center(), None), None);
    }

    #[test]
    fn clicking_a_link_in_the_page_opens_it() {
        let mut chrome = desktop_chrome(1);
        let point = chrome.content_rect().center();
        chrome.pointer_down(point);
        assert_eq!(
            chrome.pointer_up(point, Some("https://example.com/x")),
            Some(UiAction::OpenUrl("https://example.com/x".into()))
        );
    }

    #[test]
    fn middle_clicking_a_link_opens_a_background_tab() {
        let mut chrome = desktop_chrome(1);
        let point = chrome.content_rect().center();
        assert_eq!(
            chrome.middle_click(point, Some("https://example.com/x")),
            Some(UiAction::OpenUrlInNewTab("https://example.com/x".into()))
        );
    }

    #[test]
    fn middle_clicking_a_tab_closes_it() {
        let mut chrome = desktop_chrome(3);
        let tab = chrome.geometry().rect_of(WidgetId::Tab(1)).unwrap();
        assert_eq!(
            chrome.middle_click(tab.center(), None),
            Some(UiAction::CloseTab(1))
        );
    }

    #[test]
    fn the_menu_opens_closes_and_acts() {
        let mut chrome = desktop_chrome(1);
        assert!(!chrome.menu_is_open());
        let menu = chrome.geometry().rect_of(WidgetId::Menu).unwrap();
        chrome.pointer_down(menu.center());
        assert_eq!(chrome.pointer_up(menu.center(), None), None);
        assert!(chrome.menu_is_open());
        assert!(chrome.geometry().menu_panel.is_some());

        let item = chrome
            .geometry()
            .rect_of(WidgetId::MenuItem(MenuItem::Reload))
            .expect("the menu lists Reload");
        chrome.pointer_down(item.center());
        assert_eq!(
            chrome.pointer_up(item.center(), None),
            Some(UiAction::Reload)
        );
        assert!(!chrome.menu_is_open(), "acting closes the menu");
    }

    #[test]
    fn clicking_outside_the_menu_dismisses_it_without_acting() {
        let mut chrome = desktop_chrome(1);
        chrome.toggle_menu();
        let point = chrome.content_rect().center();
        chrome.pointer_down(point);
        assert_eq!(chrome.pointer_up(point, Some("https://x/")), None);
        assert!(!chrome.menu_is_open());
    }

    #[test]
    fn the_menu_stays_on_screen_on_mobile() {
        let mut chrome = mobile_chrome(1);
        chrome.toggle_menu();
        let panel = chrome.geometry().menu_panel.expect("a menu").rect;
        assert!(panel.x >= 0.0);
        assert!(panel.y >= 0.0);
        assert!(panel.max_x() <= chrome.size().width + 0.5);
        assert!(
            panel.max_y() <= chrome.size().height + 0.5,
            "the menu opens upwards from the bottom bar"
        );
    }

    #[test]
    fn the_omnibox_focuses_on_click_and_navigates_on_enter() {
        let mut chrome = desktop_chrome(1);
        let rect = chrome.geometry().omnibox;
        chrome.pointer_down(rect.center());
        assert_eq!(chrome.pointer_up(rect.center(), None), None);
        assert!(chrome.omnibox.is_focused());

        for ch in "x.com".chars() {
            chrome.key_pressed(Key::Char(ch), Modifiers::default());
        }
        assert_eq!(
            chrome.key_pressed(Key::Enter, Modifiers::default()),
            Some(UiAction::Navigate("x.com".into()))
        );
        assert!(!chrome.omnibox.is_focused());
    }

    #[test]
    fn escape_abandons_an_edit() {
        let mut chrome = desktop_chrome(1);
        chrome.omnibox.set_url("https://example.com/");
        chrome.omnibox.focus();
        chrome.key_pressed(Key::Char('z'), Modifiers::default());
        assert_eq!(chrome.key_pressed(Key::Escape, Modifiers::default()), None);
        assert!(!chrome.omnibox.is_focused());
        assert_eq!(chrome.omnibox.visible_text(), "https://example.com/");
    }

    #[test]
    fn accelerators_work_whether_or_not_the_field_has_focus() {
        let mut chrome = desktop_chrome(2);
        let accel = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        assert_eq!(
            chrome.key_pressed(Key::Char('t'), accel),
            Some(UiAction::NewTab)
        );
        assert_eq!(
            chrome.key_pressed(Key::Char('w'), accel),
            Some(UiAction::CloseActiveTab)
        );
        assert_eq!(
            chrome.key_pressed(Key::Char('r'), accel),
            Some(UiAction::Reload)
        );
        assert_eq!(chrome.key_pressed(Key::Left, accel), Some(UiAction::GoBack));
        assert_eq!(
            chrome.key_pressed(Key::Right, accel),
            Some(UiAction::GoForward)
        );
        assert_eq!(
            chrome.key_pressed(Key::Char('2'), accel),
            Some(UiAction::SelectTab(1))
        );

        assert_eq!(chrome.key_pressed(Key::Char('l'), accel), None);
        assert!(
            chrome.omnibox.is_focused(),
            "ctrl+L focuses the address bar"
        );
        // Still an accelerator while typing.
        assert_eq!(
            chrome.key_pressed(Key::Char('t'), accel),
            Some(UiAction::NewTab)
        );
    }

    #[test]
    fn typed_characters_go_to_the_field_not_the_shortcuts() {
        let mut chrome = desktop_chrome(1);
        chrome.omnibox.focus();
        chrome.omnibox.clear();
        assert_eq!(
            chrome.key_pressed(Key::Char('t'), Modifiers::default()),
            None
        );
        assert_eq!(chrome.omnibox.text(), "t");
    }

    #[test]
    fn scroll_keys_only_act_when_the_field_is_not_focused() {
        let mut chrome = desktop_chrome(1);
        match chrome.key_pressed(Key::Down, Modifiers::default()) {
            Some(UiAction::ScrollContent(delta)) => assert!(delta > 0.0),
            other => panic!("expected a scroll, got {other:?}"),
        }
        chrome.omnibox.focus();
        assert_eq!(chrome.key_pressed(Key::Down, Modifiers::default()), None);
    }

    #[test]
    fn the_wheel_scrolls_only_over_content() {
        let chrome = desktop_chrome(1);
        let content = chrome.content_rect().center();
        assert_eq!(
            chrome.scroll(content, 40.0),
            Some(UiAction::ScrollContent(40.0))
        );
        let toolbar = chrome.geometry().rect_of(WidgetId::Back).unwrap().center();
        assert_eq!(chrome.scroll(toolbar, 40.0), None);
    }

    #[test]
    fn hover_tracking_reports_changes_once() {
        let mut chrome = desktop_chrome(2);
        let back = chrome.geometry().rect_of(WidgetId::Back).unwrap();
        assert!(chrome.pointer_moved(Some(back.center())));
        assert!(!chrome.pointer_moved(Some(back.center().offset(1.0, 0.0))));
        assert!(chrome.pointer_moved(None));
    }

    #[test]
    fn cursors_reflect_what_is_under_the_pointer() {
        let chrome = desktop_chrome(1);
        let back = chrome.geometry().rect_of(WidgetId::Back).unwrap();
        assert_eq!(
            chrome.cursor_at(back.center(), Cursor::Auto),
            Cursor::Pointer
        );
        assert_eq!(
            chrome.cursor_at(chrome.geometry().omnibox.center(), Cursor::Auto),
            Cursor::Text
        );
        assert_eq!(
            chrome.cursor_at(chrome.content_rect().center(), Cursor::Pointer),
            Cursor::Pointer,
            "the page decides its own cursor"
        );
    }

    // ---- drawing -----------------------------------------------------------

    #[test]
    fn the_desktop_chrome_draws_glass_and_text() {
        let session = session_with(2);
        let mut chrome = desktop_chrome(2);
        chrome.omnibox.set_url(session.active().unwrap().url());
        let fonts = FontStore::empty();
        let list = chrome.build(&fonts, &session);

        assert!(list.is_balanced());
        assert!(
            list.backdrop_filter_count() >= 1,
            "the chrome panel should be glass"
        );
        let labels = texts(&list);
        assert!(
            labels.iter().any(|text| text.contains("Alpha")),
            "{labels:?}"
        );
        assert!(
            labels.iter().any(|text| text.contains("b.example")),
            "the omnibox should show the URL: {labels:?}"
        );
    }

    #[test]
    fn the_mobile_chrome_draws_two_glass_bars() {
        let session = session_with(3);
        let chrome = mobile_chrome(3);
        let fonts = FontStore::empty();
        let list = chrome.build(&fonts, &session);
        assert!(list.is_balanced());
        assert!(list.backdrop_filter_count() >= 2);
        assert!(
            texts(&list).contains(&"3".to_string()),
            "the tab count should be shown: {:?}",
            texts(&list)
        );
    }

    #[test]
    fn a_flat_theme_draws_the_same_chrome_without_blur() {
        let session = session_with(2);
        let mut chrome = desktop_chrome(2);
        chrome.set_theme(Theme::default().without_glass().resolve(false));
        let fonts = FontStore::empty();
        let list = chrome.build(&fonts, &session);
        assert!(list.is_balanced());
        assert_eq!(list.backdrop_filter_count(), 0);
        // The widgets are all still there.
        assert!(chrome.geometry().rect_of(WidgetId::Back).is_some());
        assert!(!list.is_empty());
    }

    #[test]
    fn the_progress_bar_appears_only_while_loading() {
        let session = session_with(1);
        let fonts = FontStore::empty();
        let mut chrome = desktop_chrome(1);
        let without = chrome.build(&fonts, &session).len();

        chrome.progress = Some(0.4);
        chrome.relayout(1);
        let with = chrome.build(&fonts, &session).len();
        assert!(with > without);
        assert!(chrome.geometry().progress.is_some());
    }

    #[test]
    fn the_status_pill_shows_a_hovered_link() {
        let session = session_with(1);
        let fonts = FontStore::empty();
        let mut chrome = desktop_chrome(1);
        chrome.status = Some("https://example.com/deep/link".into());
        let list = chrome.build(&fonts, &session);
        assert!(texts(&list).iter().any(|text| text.contains("example.com")));
    }

    #[test]
    fn long_labels_are_truncated_rather_than_overflowing() {
        let loader = StaticLoader::new().with_html(
            "https://a.example/",
            "<title>An extremely long document title that cannot possibly fit inside a tab</title>",
        );
        let mut session = Session::new(
            Rc::new(FontStore::empty()),
            theme(),
            Size2D::new(800.0, 600.0),
            false,
        );
        session.open_tab("https://a.example/", &loader);

        let chrome = desktop_chrome(1);
        let fonts = FontStore::empty();
        let list = chrome.build(&fonts, &session);
        let tab_rect = chrome.geometry().rect_of(WidgetId::Tab(0)).unwrap();
        let label = list
            .items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Text(text) => Some(text),
                _ => None,
            })
            .find(|text| text.text.starts_with("An extremely"))
            .expect("the tab label");
        assert!(label.text.ends_with('…'), "got {:?}", label.text);
        assert!(fonts.measure(&label.font, &label.text) <= tab_rect.width);
    }

    #[test]
    fn an_empty_session_still_draws() {
        let session = Session::new(
            Rc::new(FontStore::empty()),
            theme(),
            Size2D::new(800.0, 600.0),
            false,
        );
        let chrome = desktop_chrome(0);
        let fonts = FontStore::empty();
        let list = chrome.build(&fonts, &session);
        assert!(list.is_balanced());
        assert!(!list.is_empty());
    }

    #[test]
    fn a_tiny_window_does_not_produce_negative_geometry() {
        let mut chrome = Chrome::new(theme(), Size2D::new(40.0, 40.0));
        chrome.relayout(3);
        for (id, rect) in &chrome.geometry().widgets {
            assert!(
                rect.width >= 0.0 && rect.height >= 0.0,
                "{id:?} is {rect:?}"
            );
        }
        assert!(chrome.content_rect().height >= 0.0);

        let session = session_with(1);
        let fonts = FontStore::empty();
        assert!(chrome.build(&fonts, &session).is_balanced());
    }

    #[test]
    fn the_dark_theme_uses_light_text() {
        let dark = Theme::default().resolve(true);
        let mut chrome = desktop_chrome(1);
        chrome.set_theme(dark);
        let session = session_with(1);
        let fonts = FontStore::empty();
        let list = chrome.build(&fonts, &session);
        let colors: Vec<Color> = list
            .items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Text(text) => Some(text.color),
                _ => None,
            })
            .collect();
        assert!(
            colors.iter().any(|color| color.luminance() > 0.5),
            "dark chrome should draw light text: {colors:?}"
        );
    }
}
