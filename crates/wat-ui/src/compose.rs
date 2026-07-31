//! Composing a whole window: page underneath, glass chrome on top.
//!
//! Both the desktop shell and the headless CLI go through here, so a screenshot
//! taken from the command line is the same image the window shows.

use crate::Chrome;
use wat_engine::Session;
use wat_layout::geom::Size2D;
use wat_paint::{Canvas, DisplayItem, DisplayList, Renderer, RoundedRect};
use wat_style::Corners;
use wat_text::FontStore;

/// Builds the display list for the whole window.
///
/// The page is drawn first and clipped to the content area, so the chrome's
/// backdrop filters have real page pixels to blur.
pub fn window_display_list(chrome: &Chrome, session: &Session) -> DisplayList {
    let mut list = DisplayList::new();
    let theme = chrome.theme();
    let content = chrome.content_rect();

    // The window background, visible around and through the chrome.
    list.push(DisplayItem::Fill {
        shape: RoundedRect::sharp(wat_layout::geom::Rect::new(
            0.0,
            0.0,
            chrome.size().width,
            chrome.size().height,
        )),
        color: theme.palette.canvas,
    });

    if let Some(tab) = session.active() {
        let page_shape = RoundedRect::new(content, Corners::all(theme.geometry.radius_medium));
        if !content.is_empty() {
            list.push(DisplayItem::Fill {
                shape: page_shape,
                color: tab.page.background_color(),
            });
            list.push(DisplayItem::PushClip(page_shape));
            list.extend(tab.page.display_list_at(content.origin()));
            list.push(DisplayItem::PopClip);
        }
    }

    list
}

/// Renders a whole window into a new canvas.
pub fn render_window(chrome: &Chrome, session: &Session, fonts: &FontStore) -> Canvas {
    let size = chrome.size();
    let mut canvas = Canvas::new(size.width.max(1.0) as u32, size.height.max(1.0) as u32);
    render_window_into(chrome, session, fonts, &mut canvas);
    canvas
}

/// Renders a whole window into an existing canvas.
pub fn render_window_into(
    chrome: &Chrome,
    session: &Session,
    fonts: &FontStore,
    canvas: &mut Canvas,
) {
    let renderer = Renderer::new(fonts);
    renderer.render(&window_display_list(chrome, session), canvas);
    // The chrome is drawn in a second pass so its backdrop filters see the page.
    renderer.render(&chrome.build(fonts, session), canvas);
}

/// The viewport the page should be laid out for, given the chrome's geometry.
pub fn page_viewport(chrome: &Chrome) -> Size2D {
    let content = chrome.content_rect();
    Size2D::new(content.width.max(1.0), content.height.max(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChromeLayout;
    use std::rc::Rc;
    use wat_css::Color;
    use wat_net::StaticLoader;
    use wat_theme::Theme;

    fn session(html: &str) -> Session {
        let loader = StaticLoader::new().with_html("https://example.com/", html);
        let mut session = Session::new(
            Rc::new(FontStore::empty()),
            Theme::default().resolve(false),
            Size2D::new(800.0, 600.0),
            false,
        );
        session.open_tab("https://example.com/", &loader);
        session
    }

    fn chrome_for(session: &Session, size: Size2D) -> Chrome {
        let mut chrome = Chrome::new(Theme::default().resolve(false), size);
        chrome.relayout(session.tab_count());
        chrome
    }

    #[test]
    fn the_window_list_is_balanced_and_ordered() {
        let session = session("<p>hello</p>");
        let chrome = chrome_for(&session, Size2D::new(900.0, 700.0));
        let list = window_display_list(&chrome, &session);
        assert!(list.is_balanced());
        // The first item is the window background.
        assert!(matches!(list.items[0], DisplayItem::Fill { .. }));
    }

    #[test]
    fn the_page_is_clipped_to_the_content_area() {
        let session = session("<div style=\"height:5000px;background:#f00\">tall</div>");
        let chrome = chrome_for(&session, Size2D::new(900.0, 700.0));
        let fonts = FontStore::empty();
        let canvas = render_window(&chrome, &session, &fonts);

        let content = chrome.content_rect();
        // Well inside the content area the page shows.
        let inside = canvas.pixel(
            content.center().x as u32,
            (content.y + content.height / 2.0) as u32,
        );
        assert_eq!(inside, Color::rgb(255, 0, 0));

        // Above the content area the chrome is drawn instead of the page.
        let above = canvas.pixel(content.center().x as u32, 4);
        assert_ne!(above, Color::rgb(255, 0, 0));
    }

    #[test]
    fn the_chrome_blurs_the_page_behind_it() {
        // Hard stripes make blur easy to detect.
        let session = session(
            "<div style=\"height:4000px;background:linear-gradient(180deg,#000,#fff)\"></div>",
        );
        let mut chrome = chrome_for(&session, Size2D::new(900.0, 700.0));
        chrome.relayout(session.tab_count());
        let fonts = FontStore::empty();
        let canvas = render_window(&chrome, &session, &fonts);

        // The glass panel should not be a flat block of the canvas colour: the
        // page underneath must be showing through it.
        let panel = chrome.geometry().panels[0].rect;
        let sample = canvas.pixel(panel.center().x as u32, panel.center().y as u32);
        assert_eq!(sample.a, 255);
        assert_ne!(
            sample,
            chrome.theme().palette.canvas,
            "the panel should be showing the page through the glass"
        );
    }

    #[test]
    fn the_page_viewport_matches_the_content_area() {
        let session = session("<p>x</p>");
        let chrome = chrome_for(&session, Size2D::new(900.0, 700.0));
        let viewport = page_viewport(&chrome);
        assert_eq!(viewport.width, chrome.content_rect().width);
        assert_eq!(viewport.height, chrome.content_rect().height);
    }

    #[test]
    fn a_mobile_window_composes_too() {
        let session = session("<p>hello</p>");
        let mut chrome = Chrome::new(Theme::default().resolve(false), Size2D::new(390.0, 844.0));
        chrome.relayout(session.tab_count());
        assert_eq!(chrome.layout(), ChromeLayout::Mobile);

        let fonts = FontStore::empty();
        let canvas = render_window(&chrome, &session, &fonts);
        assert_eq!(canvas.width(), 390);
        assert_eq!(canvas.height(), 844);
        // Both bars leave visible marks on the canvas.
        assert_eq!(canvas.pixel(195, 20).a, 255);
        assert_eq!(canvas.pixel(195, 820).a, 255);
    }

    #[test]
    fn an_empty_session_renders_the_canvas_colour() {
        let session = Session::new(
            Rc::new(FontStore::empty()),
            Theme::default().resolve(false),
            Size2D::new(400.0, 300.0),
            false,
        );
        let chrome = chrome_for(&session, Size2D::new(400.0, 300.0));
        let fonts = FontStore::empty();
        let canvas = render_window(&chrome, &session, &fonts);
        let content = chrome.content_rect();
        assert_eq!(
            canvas.pixel(content.center().x as u32, content.center().y as u32),
            chrome.theme().palette.canvas
        );
    }
}
