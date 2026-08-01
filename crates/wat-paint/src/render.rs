//! Replaying a display list onto a [`Canvas`].

use crate::canvas::{Canvas, Clip, RoundedRect};
use crate::display::{DisplayItem, DisplayList, TextItem};
use wat_css::Color;
use wat_layout::geom::Rect;
use wat_style::Sides;
use wat_text::{FontRequest, FontStore};

/// Draws display lists.
pub struct Renderer<'a> {
    fonts: &'a FontStore,
}

impl<'a> Renderer<'a> {
    pub fn new(fonts: &'a FontStore) -> Self {
        Renderer { fonts }
    }

    /// Replays `list` onto `canvas`.
    pub fn render(&self, list: &DisplayList, canvas: &mut Canvas) {
        let mut clips: Vec<Clip> = vec![Clip::from_rect(canvas.bounds())];
        let mut opacities: Vec<f32> = vec![1.0];

        for item in &list.items {
            let clip = clips.last().expect("the clip stack is never empty");
            let opacity = *opacities.last().expect("the opacity stack is never empty");

            match item {
                DisplayItem::PushClip(shape) => {
                    let mut next = clip.clone();
                    next.push(*shape);
                    clips.push(next);
                }
                DisplayItem::PopClip => {
                    if clips.len() > 1 {
                        clips.pop();
                    }
                }
                DisplayItem::PushOpacity(alpha) => {
                    opacities.push(opacity * alpha.clamp(0.0, 1.0));
                }
                DisplayItem::PopOpacity => {
                    if opacities.len() > 1 {
                        opacities.pop();
                    }
                }
                DisplayItem::Fill { shape, color } => {
                    canvas.fill(*shape, fade(*color, opacity), clip);
                }
                DisplayItem::Gradient { shape, gradient } => {
                    if opacity >= 1.0 {
                        canvas.fill_gradient(*shape, gradient, clip);
                    } else {
                        let faded = crate::canvas::LinearGradient {
                            start: gradient.start,
                            end: gradient.end,
                            stops: gradient
                                .stops
                                .iter()
                                .map(|(position, color)| (*position, fade(*color, opacity)))
                                .collect(),
                        };
                        canvas.fill_gradient(*shape, &faded, clip);
                    }
                }
                DisplayItem::Border {
                    shape,
                    widths,
                    colors,
                } => {
                    canvas.stroke_border(
                        *shape,
                        *widths,
                        Sides {
                            top: fade(colors.top, opacity),
                            right: fade(colors.right, opacity),
                            bottom: fade(colors.bottom, opacity),
                            left: fade(colors.left, opacity),
                        },
                        clip,
                    );
                }
                DisplayItem::Shadow { shape, shadow } => {
                    let color = fade(shadow.color, opacity);
                    if shadow.inset {
                        canvas.inner_shadow(
                            *shape,
                            shadow.offset,
                            shadow.spread,
                            shadow.blur,
                            color,
                            clip,
                        );
                    } else {
                        canvas.drop_shadow(
                            *shape,
                            shadow.offset,
                            shadow.spread,
                            shadow.blur,
                            color,
                            clip,
                        );
                    }
                }
                DisplayItem::BackdropFilter { shape, filter } => {
                    canvas.filter_region(*shape, *filter, clip);
                }
                DisplayItem::Text(text) => self.draw_text(canvas, text, opacity, clip),
                DisplayItem::Image { shape, image } => {
                    if opacity >= 1.0 {
                        canvas.draw_image(*shape, image.width, image.height, &image.pixels, clip);
                    } else {
                        // Scale the alpha channel down for the whole image.
                        let mut faded = image.pixels.clone();
                        for pixel in faded.chunks_exact_mut(4) {
                            pixel[3] = (pixel[3] as f32 * opacity).round() as u8;
                        }
                        canvas.draw_image(*shape, image.width, image.height, &faded, clip);
                    }
                }
                DisplayItem::Placeholder {
                    shape,
                    label,
                    color,
                    font,
                } => {
                    self.draw_placeholder(canvas, *shape, label, fade(*color, opacity), font, clip)
                }
            }
        }
    }

    /// Draws one text run, including its shadows and decorations.
    fn draw_text(&self, canvas: &mut Canvas, text: &TextItem, opacity: f32, clip: &Clip) {
        let mut request = text.font.clone();
        // Justification widens spaces, which the shaper models as word spacing.
        request.word_spacing += text.extra_word_spacing;

        let run = self.fonts.shape(&request, &text.text);

        for shadow in &text.shadows {
            let color = fade(shadow.color, opacity);
            for glyph in &run.glyphs {
                if glyph.ch.is_whitespace() {
                    continue;
                }
                let bitmap = self.fonts.glyph(&request, glyph.ch);
                if bitmap.is_empty() {
                    continue;
                }
                // Shadow blur is approximated by drawing the glyph offset and
                // translucent; a per-glyph blur would cost far more.
                let softness = if shadow.blur > 0.0 { 0.6 } else { 1.0 };
                canvas.blit_mask(
                    (text.x + glyph.x + shadow.offset.0).round() as i32 + bitmap.left,
                    (text.baseline + shadow.offset.1).round() as i32 + bitmap.top,
                    bitmap.width,
                    bitmap.height,
                    &bitmap.coverage,
                    color.scale_alpha(softness),
                    clip,
                );
            }
        }

        let color = fade(text.color, opacity);
        for glyph in &run.glyphs {
            if glyph.ch.is_whitespace() {
                continue;
            }
            let bitmap = self.fonts.glyph(&request, glyph.ch);
            if bitmap.is_empty() {
                continue;
            }
            canvas.blit_mask(
                (text.x + glyph.x).round() as i32 + bitmap.left,
                text.baseline.round() as i32 + bitmap.top,
                bitmap.width,
                bitmap.height,
                &bitmap.coverage,
                color,
                clip,
            );
        }

        if text.decoration.any() {
            let metrics = self.fonts.line_metrics(&request);
            let thickness = (request.size / 14.0).max(1.0);
            let decoration_color = fade(text.decoration_color, opacity);
            let mut line = |y: f32| {
                canvas.fill(
                    RoundedRect::sharp(Rect::new(text.x, y, run.width, thickness)),
                    decoration_color,
                    clip,
                );
            };
            if text.decoration.underline {
                line(text.baseline + (metrics.descent * 0.35).max(1.0));
            }
            if text.decoration.overline {
                line(text.baseline - metrics.ascent);
            }
            if text.decoration.line_through {
                line(text.baseline - metrics.ascent * 0.32);
            }
        }
    }

    /// Draws the frame browsers show for content they cannot render.
    fn draw_placeholder(
        &self,
        canvas: &mut Canvas,
        shape: RoundedRect,
        label: &str,
        color: Color,
        font: &FontRequest,
        clip: &Clip,
    ) {
        canvas.fill(shape, color.scale_alpha(0.06), clip);
        canvas.stroke_border(
            shape,
            Sides::all(1.0),
            Sides::all(color.scale_alpha(0.35)),
            clip,
        );
        if label.is_empty() || shape.rect.width < 24.0 || shape.rect.height < 12.0 {
            return;
        }

        let mut request = font.clone();
        request.size = font.size.min(shape.rect.height * 0.5).max(8.0);
        let metrics = self.fonts.line_metrics(&request);
        // Trim the label until it fits the frame.
        let mut shown = label.to_string();
        let padding = 6.0;
        while shown.chars().count() > 1
            && self.fonts.measure(&request, &shown) > shape.rect.width - padding * 2.0
        {
            shown.pop();
        }
        let width = self.fonts.measure(&request, &shown);
        let mut inner_clip = clip.clone();
        inner_clip.push(shape);
        self.draw_text(
            canvas,
            &TextItem {
                x: shape.rect.x + (shape.rect.width - width) / 2.0,
                baseline: shape.rect.center().y + metrics.ascent / 2.0,
                text: shown,
                font: request,
                color: color.scale_alpha(0.75),
                extra_word_spacing: 0.0,
                decoration: Default::default(),
                decoration_color: color,
                shadows: Vec::new(),
            },
            1.0,
            &inner_clip,
        );
    }
}

fn fade(color: Color, opacity: f32) -> Color {
    if opacity >= 1.0 {
        color
    } else {
        color.scale_alpha(opacity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::LinearGradient;
    use crate::display::ShadowItem;
    use crate::image::RasterImage;
    use std::rc::Rc;
    use wat_style::{Corners, Filter};

    fn renderer_with_fonts() -> FontStore {
        FontStore::new()
    }

    #[test]
    fn fills_are_drawn() {
        let fonts = FontStore::empty();
        let renderer = Renderer::new(&fonts);
        let mut canvas = Canvas::new(20, 20);
        let mut list = DisplayList::new();
        list.push(DisplayItem::Fill {
            shape: RoundedRect::sharp(Rect::new(5.0, 5.0, 10.0, 10.0)),
            color: Color::rgb(255, 0, 0),
        });
        renderer.render(&list, &mut canvas);
        assert_eq!(canvas.pixel(10, 10), Color::rgb(255, 0, 0));
        assert_eq!(canvas.pixel(1, 1), Color::TRANSPARENT);
    }

    #[test]
    fn clip_scopes_are_honoured_and_restored() {
        let fonts = FontStore::empty();
        let renderer = Renderer::new(&fonts);
        let mut canvas = Canvas::new(20, 20);
        let mut list = DisplayList::new();
        list.push(DisplayItem::PushClip(RoundedRect::sharp(Rect::new(
            0.0, 0.0, 10.0, 20.0,
        ))));
        list.push(DisplayItem::Fill {
            shape: RoundedRect::sharp(Rect::new(0.0, 0.0, 20.0, 5.0)),
            color: Color::rgb(255, 0, 0),
        });
        list.push(DisplayItem::PopClip);
        list.push(DisplayItem::Fill {
            shape: RoundedRect::sharp(Rect::new(0.0, 10.0, 20.0, 5.0)),
            color: Color::rgb(0, 0, 255),
        });
        renderer.render(&list, &mut canvas);

        assert_eq!(canvas.pixel(5, 2), Color::rgb(255, 0, 0));
        assert_eq!(canvas.pixel(15, 2), Color::TRANSPARENT, "clipped away");
        assert_eq!(canvas.pixel(15, 12), Color::rgb(0, 0, 255), "clip restored");
    }

    #[test]
    fn opacity_scopes_multiply() {
        let fonts = FontStore::empty();
        let renderer = Renderer::new(&fonts);
        let mut canvas = Canvas::new(10, 10);
        let mut list = DisplayList::new();
        list.push(DisplayItem::PushOpacity(0.5));
        list.push(DisplayItem::Fill {
            shape: RoundedRect::sharp(canvas.bounds()),
            color: Color::BLACK,
        });
        list.push(DisplayItem::PopOpacity);
        renderer.render(&list, &mut canvas);
        assert_eq!(canvas.pixel(5, 5).a, 128);
    }

    #[test]
    fn nested_opacity_compounds() {
        let fonts = FontStore::empty();
        let renderer = Renderer::new(&fonts);
        let mut canvas = Canvas::new(10, 10);
        let mut list = DisplayList::new();
        list.push(DisplayItem::PushOpacity(0.5));
        list.push(DisplayItem::PushOpacity(0.5));
        list.push(DisplayItem::Fill {
            shape: RoundedRect::sharp(canvas.bounds()),
            color: Color::BLACK,
        });
        list.push(DisplayItem::PopOpacity);
        list.push(DisplayItem::PopOpacity);
        renderer.render(&list, &mut canvas);
        assert_eq!(canvas.pixel(5, 5).a, 64);
    }

    #[test]
    fn unbalanced_pops_do_not_panic() {
        let fonts = FontStore::empty();
        let renderer = Renderer::new(&fonts);
        let mut canvas = Canvas::new(10, 10);
        let mut list = DisplayList::new();
        list.push(DisplayItem::PopClip);
        list.push(DisplayItem::PopOpacity);
        list.push(DisplayItem::Fill {
            shape: RoundedRect::sharp(canvas.bounds()),
            color: Color::BLACK,
        });
        renderer.render(&list, &mut canvas);
        assert_eq!(canvas.pixel(5, 5), Color::BLACK);
    }

    #[test]
    fn gradients_and_borders_render() {
        let fonts = FontStore::empty();
        let renderer = Renderer::new(&fonts);
        let mut canvas = Canvas::new(40, 40);
        let rect = Rect::new(0.0, 0.0, 40.0, 40.0);
        let mut list = DisplayList::new();
        list.push(DisplayItem::Gradient {
            shape: RoundedRect::sharp(rect),
            gradient: LinearGradient::for_angle(
                rect,
                180.0,
                vec![(0.0, Color::BLACK), (1.0, Color::WHITE)],
            ),
        });
        list.push(DisplayItem::Border {
            shape: RoundedRect::new(rect, Corners::all(8.0)),
            widths: Sides::all(2.0),
            colors: Sides::all(Color::rgb(255, 0, 0)),
        });
        renderer.render(&list, &mut canvas);
        assert!(canvas.pixel(20, 35).r > canvas.pixel(20, 5).r);
        assert_eq!(canvas.pixel(20, 0), Color::rgb(255, 0, 0));
    }

    #[test]
    fn images_render_through_the_list() {
        let fonts = FontStore::empty();
        let renderer = Renderer::new(&fonts);
        let mut canvas = Canvas::new(10, 10);
        let image = Rc::new(RasterImage::solid(2, 2, [0, 128, 255, 255]));
        let mut list = DisplayList::new();
        list.push(DisplayItem::Image {
            shape: RoundedRect::sharp(canvas.bounds()),
            image,
        });
        renderer.render(&list, &mut canvas);
        assert_eq!(canvas.pixel(5, 5), Color::rgb(0, 128, 255));
    }

    #[test]
    fn image_opacity_is_applied() {
        let fonts = FontStore::empty();
        let renderer = Renderer::new(&fonts);
        let mut canvas = Canvas::new(10, 10);
        let image = Rc::new(RasterImage::solid(1, 1, [255, 255, 255, 255]));
        let mut list = DisplayList::new();
        list.push(DisplayItem::PushOpacity(0.25));
        list.push(DisplayItem::Image {
            shape: RoundedRect::sharp(canvas.bounds()),
            image,
        });
        list.push(DisplayItem::PopOpacity);
        renderer.render(&list, &mut canvas);
        assert_eq!(canvas.pixel(5, 5).a, 64);
    }

    #[test]
    fn text_puts_ink_on_the_canvas() {
        let fonts = renderer_with_fonts();
        if !fonts.has_fonts() {
            return;
        }
        let renderer = Renderer::new(&fonts);
        let mut canvas = Canvas::filled(120, 40, Color::WHITE);
        let mut list = DisplayList::new();
        list.push(DisplayItem::Text(TextItem {
            x: 4.0,
            baseline: 28.0,
            text: "Hello".into(),
            font: FontRequest::new(24.0),
            color: Color::BLACK,
            extra_word_spacing: 0.0,
            decoration: Default::default(),
            decoration_color: Color::BLACK,
            shadows: Vec::new(),
        }));
        renderer.render(&list, &mut canvas);

        let dark_pixels = (0..40)
            .flat_map(|y| (0..120).map(move |x| (x, y)))
            .filter(|(x, y)| canvas.pixel(*x, *y).luminance() < 0.5)
            .count();
        assert!(
            dark_pixels > 20,
            "expected glyph coverage, got {dark_pixels}"
        );
    }

    #[test]
    fn underline_is_drawn_below_the_baseline() {
        let fonts = FontStore::empty();
        let renderer = Renderer::new(&fonts);
        let mut canvas = Canvas::new(60, 40);
        let mut list = DisplayList::new();
        list.push(DisplayItem::Text(TextItem {
            x: 0.0,
            baseline: 20.0,
            text: "link".into(),
            font: FontRequest::new(16.0),
            color: Color::BLACK,
            extra_word_spacing: 0.0,
            decoration: wat_style::TextDecoration {
                underline: true,
                ..Default::default()
            },
            decoration_color: Color::rgb(0, 0, 255),
            shadows: Vec::new(),
        }));
        renderer.render(&list, &mut canvas);
        // The synthetic font draws no glyphs, so only the underline appears.
        let underline_row = (20..26)
            .find(|y| canvas.pixel(2, *y).a > 0)
            .expect("an underline row");
        assert!(underline_row > 20);
        // The 1.14px-thick line only partly covers its last row, so check the
        // colour rather than exact alpha.
        let pixel = canvas.pixel(2, underline_row);
        assert_eq!((pixel.r, pixel.g, pixel.b), (0, 0, 255));
        assert!(pixel.a > 200, "got {pixel:?}");
    }

    #[test]
    fn placeholders_draw_a_frame() {
        let fonts = FontStore::empty();
        let renderer = Renderer::new(&fonts);
        let mut canvas = Canvas::new(80, 40);
        let mut list = DisplayList::new();
        list.push(DisplayItem::Placeholder {
            shape: RoundedRect::sharp(Rect::new(5.0, 5.0, 70.0, 30.0)),
            label: "alt text".into(),
            color: Color::BLACK,
            font: FontRequest::new(14.0),
        });
        renderer.render(&list, &mut canvas);
        assert!(canvas.pixel(5, 20).a > 0, "the frame edge is drawn");
        assert!(canvas.pixel(40, 20).a > 0, "the fill is drawn");
        assert_eq!(canvas.pixel(1, 20).a, 0, "nothing outside the frame");
    }

    #[test]
    fn glass_surface_blurs_then_tints() {
        let fonts = FontStore::empty();
        let renderer = Renderer::new(&fonts);
        // A striped backdrop, so blurring is measurable.
        let mut canvas = Canvas::new(80, 80);
        for y in 0..80u32 {
            let dark = (y / 4) % 2 == 0;
            for x in 0..80u32 {
                canvas.set_pixel(x, y, if dark { Color::BLACK } else { Color::WHITE });
            }
        }
        let variance_before = row_variance(&canvas, 20, 60);

        let shape = RoundedRect::new(Rect::new(10.0, 10.0, 60.0, 60.0), Corners::all(16.0));
        let mut list = DisplayList::new();
        list.push(DisplayItem::BackdropFilter {
            shape,
            filter: Filter {
                blur: 16.0,
                saturate: 1.6,
                ..Filter::NONE
            },
        });
        list.push(DisplayItem::Fill {
            shape,
            color: Color::rgba(255, 255, 255, 40),
        });
        renderer.render(&list, &mut canvas);

        let variance_after = row_variance(&canvas, 20, 60);
        assert!(
            variance_after < variance_before / 2.0,
            "the backdrop should be much smoother: {variance_before} -> {variance_after}"
        );
        // Outside the glass the stripes are untouched.
        assert_eq!(canvas.pixel(2, 2), Color::BLACK);
    }

    /// Vertical variance of luminance down a column, as a smoothness measure.
    fn row_variance(canvas: &Canvas, x: u32, height: u32) -> f32 {
        let values: Vec<f32> = (10..height)
            .map(|y| canvas.pixel(x, y).luminance())
            .collect();
        let mean = values.iter().sum::<f32>() / values.len() as f32;
        values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / values.len() as f32
    }

    #[test]
    fn shadows_render_outside_and_inside() {
        let fonts = FontStore::empty();
        let renderer = Renderer::new(&fonts);
        let mut canvas = Canvas::new(80, 80);
        let shape = RoundedRect::sharp(Rect::new(20.0, 20.0, 40.0, 40.0));
        let mut list = DisplayList::new();
        list.push(DisplayItem::Shadow {
            shape,
            shadow: ShadowItem {
                offset: (0.0, 6.0),
                blur: 8.0,
                spread: 0.0,
                color: Color::rgba(0, 0, 0, 180),
                inset: false,
            },
        });
        list.push(DisplayItem::Fill {
            shape,
            color: Color::WHITE,
        });
        list.push(DisplayItem::Shadow {
            shape,
            shadow: ShadowItem {
                offset: (0.0, 0.0),
                blur: 6.0,
                spread: 3.0,
                color: Color::rgba(0, 0, 0, 200),
                inset: true,
            },
        });
        renderer.render(&list, &mut canvas);

        assert!(canvas.pixel(40, 66).a > 0, "an outer shadow below the box");
        let edge = canvas.pixel(40, 22).luminance();
        let middle = canvas.pixel(40, 40).luminance();
        assert!(edge < middle, "the inner shadow darkens the edge");
    }
}
