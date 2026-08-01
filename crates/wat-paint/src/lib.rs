//! Painting for What-A-Browser: display lists plus a software rasterizer.
//!
//! The rasterizer is written from scratch on top of signed distance fields, so
//! rounded rectangles, per-side borders, shadows and — crucially for the Liquid
//! Glass theme — real backdrop blur all come out antialiased without any 2D
//! graphics dependency.
//!
//! ```
//! use wat_css::Color;
//! use wat_layout::geom::Rect;
//! use wat_paint::{Canvas, DisplayItem, DisplayList, Renderer, RoundedRect};
//!
//! let fonts = wat_text::FontStore::empty();
//! let mut canvas = Canvas::new(32, 32);
//! let mut list = DisplayList::new();
//! list.push(DisplayItem::Fill {
//!     shape: RoundedRect::sharp(Rect::new(4.0, 4.0, 24.0, 24.0)),
//!     color: Color::rgb(255, 0, 0),
//! });
//! Renderer::new(&fonts).render(&list, &mut canvas);
//! assert_eq!(canvas.pixel(16, 16), Color::rgb(255, 0, 0));
//! ```

pub mod build;
pub mod canvas;
pub mod display;
pub mod image;
pub mod render;

pub use build::{border_sides, build_display_list, resolve_radii, PaintOptions};
pub use canvas::{blur_alpha, blur_rgba_f32, Canvas, Clip, LinearGradient, RoundedRect};
pub use display::{DisplayItem, DisplayList, ShadowItem, TextItem};
pub use image::{ImageSource, NoImageSource, RasterImage};
pub use render::Renderer;

#[cfg(test)]
mod integration {
    use super::*;
    use wat_css::Color;
    use wat_css::{MatchContext, MediaContext, Origin, Stylesheet};
    use wat_layout::geom::Point;
    use wat_layout::{layout_document, LayoutContext, NoImages, Size2D};
    use wat_style::StyleEngine;
    use wat_text::FontStore;

    /// Renders a small page end to end: HTML in, pixels out.
    fn render(html: &str, css: &str, width: u32, height: u32) -> Canvas {
        render_with_offset(html, css, width, height, Point::ZERO)
    }

    fn render_with_offset(html: &str, css: &str, width: u32, height: u32, offset: Point) -> Canvas {
        let document = wat_html::parse(html);
        let mut engine = StyleEngine::new();
        engine.add_author_sheet(Stylesheet::parse(css, Origin::Author));
        let styles = engine.compute(
            &document,
            &MediaContext::screen(width as f32, height as f32),
            &MatchContext::default(),
        );
        let fonts = FontStore::new();
        let layout_ctx = LayoutContext::new(
            &document,
            &styles,
            &fonts,
            &NoImages,
            Size2D::new(width as f32, height as f32),
        );
        let tree = layout_document(&layout_ctx);

        let options = PaintOptions::new(&NoImageSource).with_offset(offset);
        let list = build_display_list(&tree, &options);
        assert!(list.is_balanced(), "the display list must be balanced");

        let mut canvas = Canvas::filled(width, height, Color::WHITE);
        Renderer::new(&fonts).render(&list, &mut canvas);
        canvas
    }

    #[test]
    fn background_colours_reach_the_canvas() {
        let canvas = render(
            "<div class=box></div>",
            "body { margin: 0 } .box { width: 50px; height: 50px; background: #ff0000 }",
            100,
            100,
        );
        assert_eq!(canvas.pixel(25, 25), Color::rgb(255, 0, 0));
        assert_eq!(canvas.pixel(75, 75), Color::WHITE);
    }

    #[test]
    fn rounded_corners_are_cut_from_backgrounds() {
        let canvas = render(
            "<div class=box></div>",
            "body{margin:0} .box { width: 60px; height: 60px; background: #000; border-radius: 20px }",
            80,
            80,
        );
        assert_eq!(canvas.pixel(30, 30), Color::BLACK);
        assert_eq!(canvas.pixel(1, 1), Color::WHITE, "the corner is cut");
    }

    #[test]
    fn overflow_hidden_clips_children() {
        let canvas = render(
            "<div class=outer><div class=inner></div></div>",
            "body{margin:0} .outer { width: 40px; height: 40px; overflow: hidden } \
             .inner { width: 200px; height: 200px; background: #00f }",
            100,
            100,
        );
        assert_eq!(canvas.pixel(20, 20), Color::rgb(0, 0, 255));
        assert_eq!(canvas.pixel(60, 20), Color::WHITE, "clipped");
    }

    #[test]
    fn a_glass_panel_blurs_the_page_behind_it() {
        let canvas = render(
            "<div class=stripes></div><div class=glass></div>",
            "body{margin:0} \
             .stripes { position: absolute; top: 0; left: 0; width: 200px; height: 200px; \
                        background: linear-gradient(180deg, #000, #fff) } \
             .glass { position: absolute; top: 40px; left: 20px; width: 160px; height: 100px; \
                      backdrop-filter: blur(20px) saturate(180%); \
                      background: rgba(255,255,255,0.18); border-radius: 24px }",
            200,
            200,
        );
        // Inside the panel the page shows through, lightened by the tint.
        assert_eq!(canvas.pixel(100, 90).a, 255);
        // The panel's rounded corner leaves the gradient untouched.
        let corner = canvas.pixel(21, 41);
        let reference = canvas.pixel(5, 41);
        assert!(
            (corner.r as i32 - reference.r as i32).abs() < 12,
            "the corner should still show the raw gradient: {corner:?} vs {reference:?}"
        );
    }

    #[test]
    fn z_index_controls_which_box_wins() {
        let canvas = render(
            "<div class=under></div><div class=over></div>",
            "body{margin:0} \
             .under { position: absolute; top: 0; left: 0; width: 50px; height: 50px; \
                      background: #f00; z-index: 5 } \
             .over { position: absolute; top: 0; left: 0; width: 50px; height: 50px; \
                     background: #0f0; z-index: 1 }",
            60,
            60,
        );
        assert_eq!(
            canvas.pixel(25, 25),
            Color::rgb(255, 0, 0),
            "the higher z-index paints last"
        );
    }

    #[test]
    fn text_renders_dark_pixels_on_a_light_page() {
        let fonts = FontStore::new();
        if !fonts.has_fonts() {
            return;
        }
        let canvas = render(
            "<p>Hello, What-A-Browser</p>",
            "body{margin:0} p { font-size: 24px; color: #000; margin: 0 }",
            400,
            60,
        );
        let dark = (0..60)
            .flat_map(|y| (0..400).map(move |x| (x, y)))
            .filter(|(x, y)| canvas.pixel(*x, *y).luminance() < 0.4)
            .count();
        assert!(
            dark > 50,
            "expected rendered text, found {dark} dark pixels"
        );
    }

    #[test]
    fn borders_are_painted_on_all_sides() {
        let canvas = render(
            "<div class=box></div>",
            "body{margin:0} .box { width: 40px; height: 40px; border: 4px solid #000 }",
            60,
            60,
        );
        assert_eq!(canvas.pixel(20, 1), Color::BLACK);
        assert_eq!(canvas.pixel(1, 20), Color::BLACK);
        assert_eq!(canvas.pixel(46, 20), Color::BLACK);
        assert_eq!(canvas.pixel(20, 46), Color::BLACK);
        assert_eq!(canvas.pixel(24, 24), Color::WHITE, "the middle is clear");
    }

    #[test]
    fn opacity_fades_a_subtree() {
        let canvas = render(
            "<div class=box></div>",
            "body{margin:0} .box { width: 40px; height: 40px; background: #000; opacity: 0.5 }",
            60,
            60,
        );
        let pixel = canvas.pixel(20, 20);
        assert!((120..=136).contains(&pixel.r), "got {pixel:?}");
    }

    #[test]
    fn an_empty_page_paints_nothing_but_the_background() {
        let canvas = render("", "", 40, 40);
        assert_eq!(canvas.pixel(20, 20), Color::WHITE);
    }

    #[test]
    fn scroll_offset_shifts_the_page() {
        let canvas = render_with_offset(
            "<div class=box></div>",
            "body{margin:0} .box { width: 20px; height: 20px; background: #000 }",
            60,
            60,
            Point::new(0.0, -10.0),
        );
        assert_eq!(canvas.pixel(10, 5), Color::BLACK);
        assert_eq!(canvas.pixel(10, 15), Color::WHITE, "scrolled away");
    }
}
