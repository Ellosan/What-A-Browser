//! Turning a laid-out box tree into a display list.
//!
//! Paint order per box follows CSS 2.1 Appendix E, reduced to what this engine
//! generates: outer shadows, then the backdrop filter, then backgrounds, inner
//! shadows, borders, and finally content and children. Children are ordered by
//! `z-index` within their stacking context.

use crate::canvas::{LinearGradient, RoundedRect};
use crate::display::{DisplayItem, DisplayList, ShadowItem, TextItem};
use crate::image::ImageSource;
use wat_css::Color;
use wat_layout::geom::{Point, Rect};
use wat_layout::{font_request, BoxKind, LayoutBox, LayoutTree, ReplacedKind};
use wat_style::{
    BackgroundImage, ComputedStyle, Corners, GradientStop, Shadow, Sides, TextDecoration,
    Visibility,
};

/// Options for one paint pass.
pub struct PaintOptions<'a> {
    /// Where images come from.
    pub images: &'a dyn ImageSource,
    /// Translation applied to every item, used for page scrolling.
    pub offset: Point,
    /// Only boxes intersecting this rectangle are painted.
    pub cull: Option<Rect>,
}

impl<'a> PaintOptions<'a> {
    pub fn new(images: &'a dyn ImageSource) -> Self {
        PaintOptions {
            images,
            offset: Point::ZERO,
            cull: None,
        }
    }

    pub fn with_offset(mut self, offset: Point) -> Self {
        self.offset = offset;
        self
    }

    pub fn with_cull(mut self, cull: Rect) -> Self {
        self.cull = Some(cull);
        self
    }
}

/// Builds the display list for a laid-out document.
pub fn build_display_list(tree: &LayoutTree, options: &PaintOptions) -> DisplayList {
    let mut list = DisplayList::new();
    if let Some(root) = tree.root {
        let mut builder = Builder {
            tree,
            options,
            list: &mut list,
        };
        builder.paint_box(root, options.offset);
    }
    list
}

struct Builder<'a, 'opts> {
    tree: &'a LayoutTree,
    options: &'a PaintOptions<'opts>,
    list: &'a mut DisplayList,
}

impl Builder<'_, '_> {
    fn shape_for(&self, layout_box: &LayoutBox, rect: Rect) -> RoundedRect {
        RoundedRect::new(rect, resolve_radii(&layout_box.style, rect))
    }

    /// Would anything this box paints be visible?
    fn culled(&self, rect: Rect) -> bool {
        match self.options.cull {
            Some(cull) => !rect.intersects(&cull),
            None => false,
        }
    }

    fn paint_box(&mut self, index: usize, offset: Point) {
        let layout_box = self.tree.get(index);
        let style = layout_box.style.clone();
        if style.visibility == Visibility::Hidden {
            return;
        }

        // `transform` is honoured for translation, which is what UI overlays and
        // centred dialogs rely on; scale and rotation are not applied.
        let transform = Point::new(
            style.transform.translate_x.resolve(layout_box.rect.width),
            style.transform.translate_y.resolve(layout_box.rect.height),
        );
        let offset = Point::new(offset.x + transform.x, offset.y + transform.y);
        let rect = layout_box.rect.translate(offset.x, offset.y);

        let opacity_pushed = style.opacity < 1.0;
        if opacity_pushed {
            self.list.push(DisplayItem::PushOpacity(style.opacity));
        }

        let border_shape = self.shape_for(layout_box, rect);
        let padding_shape = border_shape.inset(layout_box.border);

        let paints_itself = !self.culled(rect.expand(shadow_reach(&style)));
        if paints_itself {
            self.paint_decorations(index, border_shape, padding_shape, &style);
        }

        // Content that must be clipped to this box.
        let clips = style.overflow_x.clips() || style.overflow_y.clips();
        if clips {
            self.list.push(DisplayItem::PushClip(padding_shape));
        }

        let scroll = layout_box.scroll_offset;
        let content_offset = Point::new(offset.x - scroll.x, offset.y - scroll.y);

        if paints_itself {
            self.paint_content(index, rect, &style, offset);
        }
        for child in self.paint_order(index) {
            self.paint_box(child, content_offset);
        }

        if clips {
            self.list.push(DisplayItem::PopClip);
        }
        if opacity_pushed {
            self.list.push(DisplayItem::PopOpacity);
        }
    }

    /// Shadows, backdrop filter, backgrounds and borders.
    fn paint_decorations(
        &mut self,
        index: usize,
        border_shape: RoundedRect,
        padding_shape: RoundedRect,
        style: &ComputedStyle,
    ) {
        for shadow in style.box_shadow.iter().filter(|s| !s.inset) {
            self.list.push(DisplayItem::Shadow {
                shape: border_shape,
                shadow: shadow_item(*shadow),
            });
        }

        // The backdrop filter runs before this box paints anything opaque, so it
        // sees only what is behind the surface.
        if !style.backdrop_filter.is_none() {
            self.list.push(DisplayItem::BackdropFilter {
                shape: border_shape,
                filter: style.backdrop_filter,
            });
        }

        if !style.background_color.is_transparent() {
            self.list.push(DisplayItem::Fill {
                shape: border_shape,
                color: style.background_color,
            });
        }

        match &style.background_image {
            BackgroundImage::None => {}
            BackgroundImage::LinearGradient { angle, stops } => {
                self.list.push(DisplayItem::Gradient {
                    shape: padding_shape,
                    gradient: LinearGradient::for_angle(
                        padding_shape.rect,
                        *angle,
                        gradient_stops(stops),
                    ),
                });
            }
            BackgroundImage::RadialGradient { stops } => {
                // Approximated by a vertical linear gradient.
                self.list.push(DisplayItem::Gradient {
                    shape: padding_shape,
                    gradient: LinearGradient::for_angle(
                        padding_shape.rect,
                        180.0,
                        gradient_stops(stops),
                    ),
                });
            }
            BackgroundImage::Url(url) => {
                if let Some(image) = self.options.images.image(url) {
                    self.list.push(DisplayItem::Image {
                        shape: padding_shape,
                        image,
                    });
                }
            }
        }

        for shadow in style.box_shadow.iter().filter(|s| s.inset) {
            self.list.push(DisplayItem::Shadow {
                shape: padding_shape,
                shadow: shadow_item(*shadow),
            });
        }

        if style.has_visible_border() {
            let layout_box = self.tree.get(index);
            self.list.push(DisplayItem::Border {
                shape: border_shape,
                widths: layout_box.border,
                colors: style.border_color,
            });
        }
    }

    /// Text, replaced content and markers.
    fn paint_content(&mut self, index: usize, rect: Rect, style: &ComputedStyle, offset: Point) {
        let layout_box = self.tree.get(index);
        match &layout_box.kind {
            BoxKind::Text(fragment) => {
                if fragment.text.trim().is_empty() && !style.text_decoration.any() {
                    return;
                }
                self.list.push(DisplayItem::Text(TextItem {
                    x: rect.x,
                    baseline: rect.y + fragment.baseline,
                    text: fragment.text.clone(),
                    font: font_request(style),
                    color: style.color,
                    extra_word_spacing: fragment.extra_word_spacing,
                    decoration: style.text_decoration,
                    decoration_color: style.text_decoration_color.unwrap_or(style.color),
                    shadows: style.text_shadow.iter().copied().map(shadow_item).collect(),
                }));
            }
            BoxKind::Marker(label) => {
                if label.is_empty() {
                    return;
                }
                self.list.push(DisplayItem::Text(TextItem {
                    x: rect.x,
                    baseline: rect.y + layout_box.baseline,
                    text: label.clone(),
                    font: font_request(style),
                    color: style.color,
                    extra_word_spacing: 0.0,
                    decoration: TextDecoration::default(),
                    decoration_color: style.color,
                    shadows: Vec::new(),
                }));
            }
            BoxKind::Replaced(replaced) => {
                let content = layout_box
                    .content_box()
                    .translate(rect.x - layout_box.rect.x, rect.y - layout_box.rect.y);
                let shape = RoundedRect::new(content, resolve_radii(style, content));
                let image = replaced
                    .url
                    .as_deref()
                    .and_then(|url| self.options.images.image(url));
                match image {
                    Some(image) if replaced.kind == ReplacedKind::Image => {
                        self.list.push(DisplayItem::Image { shape, image });
                    }
                    _ => {
                        // A frame with the alt text, as browsers do for a broken
                        // image or unsupported embed.
                        self.list.push(DisplayItem::Placeholder {
                            shape,
                            label: replaced.label.clone(),
                            color: style.color,
                            font: font_request(style),
                        });
                    }
                }
            }
            _ => {
                let _ = offset;
            }
        }
    }

    /// Children in paint order: negative z-index, then in-flow, then positive.
    fn paint_order(&self, index: usize) -> Vec<usize> {
        let children = self.tree.children(index);
        let mut ordered: Vec<(i32, usize, usize)> = children
            .iter()
            .enumerate()
            .map(|(position, child)| {
                let style = &self.tree.get(*child).style;
                let layer = match style.z_index {
                    Some(z) if style.position.is_positioned() => z,
                    // Positioned boxes without a z-index paint above in-flow
                    // content but below any positive layer.
                    None if style.position.is_positioned() => 0,
                    _ => -1,
                };
                (layer, position, *child)
            })
            .collect();
        ordered.sort_by_key(|(layer, position, _)| (*layer, *position));
        ordered.into_iter().map(|(_, _, child)| child).collect()
    }
}

/// How far outside its border box a box's shadows can reach.
fn shadow_reach(style: &ComputedStyle) -> f32 {
    style
        .box_shadow
        .iter()
        .filter(|shadow| !shadow.inset)
        .map(|shadow| {
            shadow.blur + shadow.spread + shadow.offset_x.abs().max(shadow.offset_y.abs())
        })
        .fold(0.0, f32::max)
}

fn shadow_item(shadow: Shadow) -> ShadowItem {
    ShadowItem {
        offset: (shadow.offset_x, shadow.offset_y),
        blur: shadow.blur,
        spread: shadow.spread,
        color: shadow.color,
        inset: shadow.inset,
    }
}

/// Distributes stops that have no explicit position.
fn gradient_stops(stops: &[GradientStop]) -> Vec<(f32, Color)> {
    let count = stops.len();
    let mut out: Vec<(f32, Color)> = Vec::with_capacity(count);
    for (index, stop) in stops.iter().enumerate() {
        let position = stop.position.unwrap_or_else(|| {
            if count <= 1 {
                0.0
            } else {
                index as f32 / (count - 1) as f32
            }
        });
        out.push((position.clamp(0.0, 1.0), stop.color));
    }
    // Positions must not decrease.
    let mut highest = 0.0f32;
    for stop in &mut out {
        highest = highest.max(stop.0);
        stop.0 = highest;
    }
    out
}

/// Resolves percentage corner radii against the box.
pub fn resolve_radii(style: &ComputedStyle, rect: Rect) -> Corners<f32> {
    let basis = rect.width.min(rect.height);
    Corners {
        top_left: style.border_radius.top_left.resolve(basis),
        top_right: style.border_radius.top_right.resolve(basis),
        bottom_right: style.border_radius.bottom_right.resolve(basis),
        bottom_left: style.border_radius.bottom_left.resolve(basis),
    }
}

/// Border widths as a [`Sides`], for callers building shapes by hand.
pub fn border_sides(layout_box: &LayoutBox) -> Sides<f32> {
    layout_box.border
}

#[cfg(test)]
mod tests {
    use super::*;
    use wat_style::LengthPercentage;

    #[test]
    fn stops_without_positions_are_distributed() {
        let stops = vec![
            GradientStop {
                color: Color::BLACK,
                position: None,
            },
            GradientStop {
                color: Color::WHITE,
                position: None,
            },
            GradientStop {
                color: Color::BLACK,
                position: None,
            },
        ];
        let resolved = gradient_stops(&stops);
        assert_eq!(resolved[0].0, 0.0);
        assert_eq!(resolved[1].0, 0.5);
        assert_eq!(resolved[2].0, 1.0);
    }

    #[test]
    fn explicit_stop_positions_are_kept_monotonic() {
        let stops = vec![
            GradientStop {
                color: Color::BLACK,
                position: Some(0.6),
            },
            GradientStop {
                color: Color::WHITE,
                position: Some(0.2),
            },
        ];
        let resolved = gradient_stops(&stops);
        assert_eq!(resolved[0].0, 0.6);
        assert_eq!(resolved[1].0, 0.6, "a decreasing stop is pinned");
    }

    #[test]
    fn percentage_radii_resolve_against_the_shorter_side() {
        let mut style = ComputedStyle::initial();
        style.border_radius = Corners::all(LengthPercentage::Percent(50.0));
        let radii = resolve_radii(&style, Rect::new(0.0, 0.0, 100.0, 40.0));
        assert_eq!(radii.top_left, 20.0);
    }

    #[test]
    fn shadow_reach_covers_offset_blur_and_spread() {
        let mut style = ComputedStyle::initial();
        assert_eq!(shadow_reach(&style), 0.0);
        style.box_shadow = vec![Shadow {
            offset_x: 0.0,
            offset_y: 10.0,
            blur: 20.0,
            spread: 5.0,
            color: Color::BLACK,
            inset: false,
        }];
        assert_eq!(shadow_reach(&style), 35.0);

        // Inset shadows never leave the box.
        style.box_shadow[0].inset = true;
        assert_eq!(shadow_reach(&style), 0.0);
    }
}
