//! The seam between the browser interface and a web engine.
//!
//! The chrome, the input handling and the window shell are one body of code; the
//! engine that turns a URL into pixels is another. This crate is the boundary
//! between them, so that more than one engine can sit behind the same browser.
//!
//! Two things shaped the trait. The first is that an engine owns its own
//! networking: WAT's does its loading through `wat-net`, and an engine like
//! Servo brings a whole network stack of its own, so no loader is threaded
//! through these calls. The second is that painting cannot be a display list.
//! WAT's engine produces one, but an engine built on WebRender produces a
//! rendered surface instead, and the only thing both can agree to do is put
//! their pixels into a region of the target canvas — which is what [`Engine`]
//! asks for.
//!
//! Everything here is described in CSS pixels. The device pixel ratio arrives as
//! the `scale` argument to [`Engine::paint`] and applies to that call only.

use wat_css::Color;
use wat_layout::geom::{Point, Rect, Size2D};
use wat_paint::Canvas;
use wat_theme::ResolvedTheme;

/// A tab, as the interface needs to describe one.
///
/// A flattened snapshot rather than a borrow of the engine's own structure,
/// because two engines will not agree on what a tab is made of, and the chrome
/// only ever needs to label it.
#[derive(Clone, Debug, PartialEq)]
pub struct TabView {
    pub id: u64,
    /// What to show on the tab. Never empty; falls back to the address.
    pub label: String,
    pub url: String,
    /// Whether the transport was authenticated, for the padlock.
    pub is_secure: bool,
    /// Whether the load failed, so the chrome can say so.
    pub failed: bool,
}

/// Why a navigation did not happen.
#[derive(Clone, Debug, PartialEq)]
pub enum NavigationError {
    /// The address could not be understood as one.
    BadAddress(String),
    /// The engine could not fetch it.
    Failed(String),
}

impl std::fmt::Display for NavigationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NavigationError::BadAddress(what) => write!(formatter, "not an address: {what}"),
            NavigationError::Failed(why) => write!(formatter, "{why}"),
        }
    }
}

impl std::error::Error for NavigationError {}

/// A web engine the browser can drive.
///
/// Implementations are expected to be cheap to query — the chrome asks for tab
/// labels and scroll offsets every time it builds a frame.
pub trait Engine {
    // ---- tabs ----

    fn tabs(&self) -> Vec<TabView>;

    fn active_index(&self) -> usize;

    fn tab_count(&self) -> usize {
        self.tabs().len()
    }

    /// Opens `input` in a new tab and makes it active, returning its id.
    fn open_tab(&mut self, input: &str) -> u64;

    /// Opens `url` in a new tab without leaving the current one.
    fn open_tab_in_background(&mut self, url: &str) -> u64;

    fn close_tab(&mut self, id: u64);

    fn close_active_tab(&mut self);

    /// Selects by position in [`Engine::tabs`]; false if there is no such tab.
    fn select_tab(&mut self, index: usize) -> bool;

    // ---- navigation ----

    /// Navigates the active tab. `input` may be an address or a search term.
    fn navigate(&mut self, input: &str) -> Result<(), NavigationError>;

    /// Follows a link the user activated, which is already a resolved URL.
    fn follow_link(&mut self, url: &str);

    fn reload(&mut self);

    fn go_back(&mut self) -> bool;

    fn go_forward(&mut self) -> bool;

    fn can_go_back(&self) -> bool;

    fn can_go_forward(&self) -> bool;

    /// A navigation the page asked for itself, taken once.
    ///
    /// Scripts can set `location`, and the engine cannot act on that alone
    /// because the shell owns history and the address bar.
    fn take_requested_navigation(&mut self) -> Option<String> {
        None
    }

    // ---- input and geometry ----

    /// The link at a point in the page's own coordinates, if there is one.
    fn link_at(&self, point: Point) -> Option<String>;

    /// Scrolls the active page. False if it was already at the end.
    fn scroll(&mut self, dx: f32, dy: f32) -> bool;

    fn scroll_offset(&self) -> Point;

    /// Resizes the area the page lays out for. `coarse_pointer` tells the engine
    /// it is being driven by a finger, which media queries can see.
    fn set_viewport(&mut self, viewport: Size2D, coarse_pointer: bool);

    /// Hands the engine the resolved theme, which pages can read as the
    /// preferred colour scheme and accent.
    fn set_theme(&mut self, theme: ResolvedTheme);

    // ---- painting ----

    /// The active page's own background, painted behind its content.
    fn background_color(&self) -> Color;

    /// Paints the active page into `area` of `canvas`.
    ///
    /// `area` is in CSS pixels and `corner_radius` is the rounding the interface
    /// wants the page clipped to; the engine must not draw outside that shape,
    /// because the chrome is composited on top afterwards and expects the page's
    /// pixels to be underneath it. `scale` is the device pixel ratio: the canvas
    /// is that many times larger than `area` describes.
    fn paint(&self, canvas: &mut Canvas, area: Rect, corner_radius: f32, scale: f32);

    // ---- work in flight ----

    /// Whether the engine has timers or animations still to run.
    fn has_pending_work(&self) -> bool {
        false
    }

    /// Runs whatever is due. True if anything changed and a repaint is needed.
    fn run_pending_work(&mut self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_navigation_error_reads_as_a_sentence() {
        assert_eq!(
            NavigationError::BadAddress("::".into()).to_string(),
            "not an address: ::"
        );
        assert_eq!(
            NavigationError::Failed("connection refused".into()).to_string(),
            "connection refused"
        );
    }

    #[test]
    fn tab_count_follows_the_tab_list() {
        struct Two;
        impl Engine for Two {
            fn tabs(&self) -> Vec<TabView> {
                vec![
                    TabView {
                        id: 1,
                        label: "a".into(),
                        url: "about:blank".into(),
                        is_secure: false,
                        failed: false,
                    },
                    TabView {
                        id: 2,
                        label: "b".into(),
                        url: "about:blank".into(),
                        is_secure: false,
                        failed: false,
                    },
                ]
            }
            fn active_index(&self) -> usize {
                0
            }
            fn open_tab(&mut self, _: &str) -> u64 {
                0
            }
            fn open_tab_in_background(&mut self, _: &str) -> u64 {
                0
            }
            fn close_tab(&mut self, _: u64) {}
            fn close_active_tab(&mut self) {}
            fn select_tab(&mut self, _: usize) -> bool {
                false
            }
            fn navigate(&mut self, _: &str) -> Result<(), NavigationError> {
                Ok(())
            }
            fn follow_link(&mut self, _: &str) {}
            fn reload(&mut self) {}
            fn go_back(&mut self) -> bool {
                false
            }
            fn go_forward(&mut self) -> bool {
                false
            }
            fn can_go_back(&self) -> bool {
                false
            }
            fn can_go_forward(&self) -> bool {
                false
            }
            fn link_at(&self, _: Point) -> Option<String> {
                None
            }
            fn scroll(&mut self, _: f32, _: f32) -> bool {
                false
            }
            fn scroll_offset(&self) -> Point {
                Point::new(0.0, 0.0)
            }
            fn set_viewport(&mut self, _: Size2D, _: bool) {}
            fn set_theme(&mut self, _: ResolvedTheme) {}
            fn background_color(&self) -> Color {
                Color::WHITE
            }
            fn paint(&self, _: &mut Canvas, _: Rect, _: f32, _: f32) {}
        }
        assert_eq!(Two.tab_count(), 2);
        // The default is derived, so an engine cannot report a count that
        // disagrees with the tabs it lists.
        assert_eq!(Two.tabs().len(), Two.tab_count());
    }
}
