//! The display list: a flat, replayable description of one frame.

use std::rc::Rc;

use crate::canvas::{LinearGradient, RoundedRect};
use crate::image::RasterImage;
use wat_css::Color;
use wat_style::{Filter, Sides, TextDecoration};
use wat_text::FontRequest;

/// A shadow, resolved to canvas coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowItem {
    pub offset: (f32, f32),
    pub blur: f32,
    pub spread: f32,
    pub color: Color,
    pub inset: bool,
}

/// A run of text to draw, positioned by its baseline.
#[derive(Clone, Debug, PartialEq)]
pub struct TextItem {
    /// Left edge of the run.
    pub x: f32,
    /// Baseline position.
    pub baseline: f32,
    pub text: String,
    pub font: FontRequest,
    pub color: Color,
    /// Extra advance added after each space, from justification.
    pub extra_word_spacing: f32,
    pub decoration: TextDecoration,
    pub decoration_color: Color,
    pub shadows: Vec<ShadowItem>,
}

/// One drawing operation.
#[derive(Clone, Debug)]
pub enum DisplayItem {
    /// Intersects the clip region until the matching [`DisplayItem::PopClip`].
    PushClip(RoundedRect),
    PopClip,
    /// Multiplies the alpha of everything drawn until the matching pop.
    PushOpacity(f32),
    PopOpacity,
    Fill {
        shape: RoundedRect,
        color: Color,
    },
    Gradient {
        shape: RoundedRect,
        gradient: LinearGradient,
    },
    Border {
        shape: RoundedRect,
        widths: Sides<f32>,
        colors: Sides<Color>,
    },
    Shadow {
        shape: RoundedRect,
        shadow: ShadowItem,
    },
    /// Filters the pixels already drawn inside `shape`: the glass primitive.
    BackdropFilter {
        shape: RoundedRect,
        filter: Filter,
    },
    Text(TextItem),
    Image {
        shape: RoundedRect,
        image: Rc<RasterImage>,
    },
    /// Stands in for content we cannot render, drawn as a labelled frame.
    Placeholder {
        shape: RoundedRect,
        label: String,
        color: Color,
        font: FontRequest,
    },
}

impl ShadowItem {
    fn scaled(&self, factor: f32) -> ShadowItem {
        ShadowItem {
            offset: (self.offset.0 * factor, self.offset.1 * factor),
            blur: self.blur * factor,
            spread: self.spread * factor,
            ..*self
        }
    }
}

impl TextItem {
    fn scaled(&self, factor: f32) -> TextItem {
        let mut font = self.font.clone();
        font.size *= factor;
        font.letter_spacing *= factor;
        font.word_spacing *= factor;
        TextItem {
            x: self.x * factor,
            baseline: self.baseline * factor,
            font,
            extra_word_spacing: self.extra_word_spacing * factor,
            shadows: self.shadows.iter().map(|s| s.scaled(factor)).collect(),
            text: self.text.clone(),
            ..*self
        }
    }
}

impl DisplayItem {
    /// This item with every length multiplied by `factor`.
    fn scaled(&self, factor: f32) -> DisplayItem {
        let shape = |shape: &RoundedRect| shape.scaled(factor);
        match self {
            DisplayItem::PushClip(clip) => DisplayItem::PushClip(shape(clip)),
            // Nothing to scale: these only move the stacks.
            DisplayItem::PopClip => DisplayItem::PopClip,
            DisplayItem::PushOpacity(alpha) => DisplayItem::PushOpacity(*alpha),
            DisplayItem::PopOpacity => DisplayItem::PopOpacity,
            DisplayItem::Fill { shape: rect, color } => DisplayItem::Fill {
                shape: shape(rect),
                color: *color,
            },
            DisplayItem::Gradient {
                shape: rect,
                gradient,
            } => DisplayItem::Gradient {
                shape: shape(rect),
                gradient: gradient.scaled(factor),
            },
            DisplayItem::Border {
                shape: rect,
                widths,
                colors,
            } => DisplayItem::Border {
                shape: shape(rect),
                widths: widths.map(|width| width * factor),
                colors: *colors,
            },
            DisplayItem::Shadow {
                shape: rect,
                shadow,
            } => DisplayItem::Shadow {
                shape: shape(rect),
                shadow: shadow.scaled(factor),
            },
            DisplayItem::BackdropFilter {
                shape: rect,
                filter,
            } => DisplayItem::BackdropFilter {
                shape: shape(rect),
                filter: Filter {
                    // Only the blur is a length; the colour adjustments are
                    // ratios and mean the same thing at any scale.
                    blur: filter.blur * factor,
                    ..*filter
                },
            },
            DisplayItem::Text(text) => DisplayItem::Text(text.scaled(factor)),
            DisplayItem::Image { shape: rect, image } => DisplayItem::Image {
                shape: shape(rect),
                image: image.clone(),
            },
            DisplayItem::Placeholder {
                shape: rect,
                label,
                color,
                font,
            } => {
                let mut font = font.clone();
                font.size *= factor;
                DisplayItem::Placeholder {
                    shape: shape(rect),
                    label: label.clone(),
                    color: *color,
                    font,
                }
            }
        }
    }
}

/// An ordered list of drawing operations for one frame.
#[derive(Clone, Debug, Default)]
pub struct DisplayList {
    pub items: Vec<DisplayItem>,
}

impl DisplayList {
    pub fn new() -> Self {
        DisplayList { items: Vec::new() }
    }

    pub fn push(&mut self, item: DisplayItem) {
        self.items.push(item);
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn extend(&mut self, other: DisplayList) {
        self.items.extend(other.items);
    }

    /// Appends a clip scope around `body`'s output.
    pub fn with_clip(&mut self, shape: RoundedRect, body: impl FnOnce(&mut DisplayList)) {
        self.push(DisplayItem::PushClip(shape));
        body(self);
        self.push(DisplayItem::PopClip);
    }

    /// The same list at a different device pixel ratio.
    ///
    /// Everything above this point works in CSS pixels; a display with more
    /// device pixels than that is handled here, at the very last moment, by
    /// multiplying the geometry. Font sizes are scaled rather than the glyph
    /// bitmaps, so text is rasterized at the size it will actually be drawn and
    /// stays sharp instead of being blown up.
    pub fn scaled(&self, factor: f32) -> DisplayList {
        if (factor - 1.0).abs() < f32::EPSILON {
            return self.clone();
        }
        DisplayList {
            items: self.items.iter().map(|item| item.scaled(factor)).collect(),
        }
    }

    /// The same frame with the expensive effects left out.
    ///
    /// For the first frame after launch, where putting something on screen now
    /// beats putting everything on screen later. Backdrop filters and shadows go
    /// — between them they are most of what a glass frame costs, and at a phone's
    /// device resolution that is the difference between a browser that opens and
    /// one that hesitates. Gradients collapse to their midpoint colour.
    ///
    /// The surfaces themselves stay: a glass panel is a filter followed by a
    /// translucent fill, so without the filter it is still a tinted panel in the
    /// right place, just not blurred. The layout is identical, which is what
    /// makes the real frame arriving a moment later a sharpening rather than a
    /// jump.
    pub fn preview(&self) -> DisplayList {
        DisplayList {
            items: self
                .items
                .iter()
                .filter(|item| {
                    !matches!(
                        item,
                        DisplayItem::BackdropFilter { .. } | DisplayItem::Shadow { .. }
                    )
                })
                .map(|item| match item {
                    DisplayItem::Gradient { shape, gradient } => DisplayItem::Fill {
                        shape: *shape,
                        color: gradient.midpoint_color(),
                    },
                    other => other.clone(),
                })
                .collect(),
        }
    }

    /// Number of glass surfaces in the list, which is a useful signal in tests
    /// and in the theme inspector.
    pub fn backdrop_filter_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| matches!(item, DisplayItem::BackdropFilter { .. }))
            .count()
    }

    /// Are the clip and opacity scopes balanced?
    pub fn is_balanced(&self) -> bool {
        let mut clips = 0i32;
        let mut opacities = 0i32;
        for item in &self.items {
            match item {
                DisplayItem::PushClip(_) => clips += 1,
                DisplayItem::PopClip => clips -= 1,
                DisplayItem::PushOpacity(_) => opacities += 1,
                DisplayItem::PopOpacity => opacities -= 1,
                _ => {}
            }
            if clips < 0 || opacities < 0 {
                return false;
            }
        }
        clips == 0 && opacities == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wat_layout::geom::Rect;

    fn shape() -> RoundedRect {
        RoundedRect::sharp(Rect::new(0.0, 0.0, 10.0, 10.0))
    }

    #[test]
    fn clip_scopes_balance() {
        let mut list = DisplayList::new();
        list.with_clip(shape(), |inner| {
            inner.push(DisplayItem::Fill {
                shape: shape(),
                color: Color::BLACK,
            });
        });
        assert_eq!(list.len(), 3);
        assert!(list.is_balanced());
    }

    #[test]
    fn unbalanced_lists_are_detected() {
        let mut list = DisplayList::new();
        list.push(DisplayItem::PushClip(shape()));
        assert!(!list.is_balanced());

        let mut list = DisplayList::new();
        list.push(DisplayItem::PopClip);
        assert!(!list.is_balanced());
    }

    #[test]
    fn backdrop_filters_are_counted() {
        let mut list = DisplayList::new();
        assert_eq!(list.backdrop_filter_count(), 0);
        list.push(DisplayItem::BackdropFilter {
            shape: shape(),
            filter: Filter {
                blur: 10.0,
                ..Filter::NONE
            },
        });
        assert_eq!(list.backdrop_filter_count(), 1);
    }
    #[test]
    fn scaling_multiplies_every_length() {
        let mut list = DisplayList::new();
        list.push(DisplayItem::Fill {
            shape: RoundedRect::new(
                Rect::new(10.0, 20.0, 30.0, 40.0),
                wat_style::Corners::all(4.0),
            ),
            color: Color::BLACK,
        });
        list.push(DisplayItem::Text(TextItem {
            x: 8.0,
            baseline: 16.0,
            text: "hi".to_string(),
            font: FontRequest::new(12.0),
            color: Color::BLACK,
            extra_word_spacing: 2.0,
            decoration: Default::default(),
            decoration_color: Color::BLACK,
            shadows: vec![ShadowItem {
                offset: (1.0, 2.0),
                blur: 3.0,
                spread: 4.0,
                color: Color::BLACK,
                inset: false,
            }],
        }));
        list.push(DisplayItem::BackdropFilter {
            shape: shape(),
            filter: Filter {
                blur: 10.0,
                saturate: 1.8,
                ..Filter::NONE
            },
        });

        let scaled = list.scaled(2.0);
        match &scaled.items[0] {
            DisplayItem::Fill { shape, .. } => {
                assert_eq!(shape.rect, Rect::new(20.0, 40.0, 60.0, 80.0));
                assert_eq!(shape.radii.top_left, 8.0);
            }
            other => panic!("got {other:?}"),
        }
        match &scaled.items[1] {
            DisplayItem::Text(text) => {
                assert_eq!(text.x, 16.0);
                assert_eq!(text.baseline, 32.0);
                assert_eq!(text.extra_word_spacing, 4.0);
                // The font is scaled, not the glyphs: text is rasterized at the
                // size it is drawn, so it stays sharp.
                assert_eq!(text.font.size, 24.0);
                assert_eq!(text.shadows[0].blur, 6.0);
                assert_eq!(text.shadows[0].offset, (2.0, 4.0));
            }
            other => panic!("got {other:?}"),
        }
        match &scaled.items[2] {
            DisplayItem::BackdropFilter { filter, .. } => {
                assert_eq!(filter.blur, 20.0);
                assert_eq!(filter.saturate, 1.8, "a ratio is not a length");
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn a_preview_drops_what_costs_the_most_and_keeps_the_layout() {
        let mut list = DisplayList::new();
        list.push(DisplayItem::PushClip(shape()));
        list.push(DisplayItem::BackdropFilter {
            shape: shape(),
            filter: Filter {
                blur: 20.0,
                ..Filter::NONE
            },
        });
        list.push(DisplayItem::Shadow {
            shape: shape(),
            shadow: ShadowItem {
                offset: (0.0, 2.0),
                blur: 8.0,
                spread: 0.0,
                color: Color::BLACK,
                inset: false,
            },
        });
        list.push(DisplayItem::Gradient {
            shape: shape(),
            gradient: LinearGradient {
                start: (0.0, 0.0),
                end: (0.0, 10.0),
                stops: vec![(0.0, Color::BLACK), (1.0, Color::WHITE)],
            },
        });
        list.push(DisplayItem::Fill {
            shape: shape(),
            color: Color::BLACK,
        });
        list.push(DisplayItem::PopClip);

        let preview = list.preview();
        assert_eq!(preview.backdrop_filter_count(), 0, "the glass is dropped");
        assert!(
            !preview
                .items
                .iter()
                .any(|item| matches!(item, DisplayItem::Shadow { .. })),
            "the shadows are dropped"
        );
        // The gradient becomes a flat fill rather than disappearing, so the
        // surface is still drawn and the frame still looks like the frame.
        assert!(matches!(
            preview.items[1],
            DisplayItem::Fill { color, .. } if color != Color::BLACK
        ));
        assert!(preview.is_balanced(), "the clip scopes must survive");
        assert_eq!(preview.len(), 4, "clip, gradient-as-fill, fill, pop");
    }

    #[test]
    fn a_preview_of_a_plain_frame_is_the_same_frame() {
        let mut list = DisplayList::new();
        list.push(DisplayItem::Fill {
            shape: shape(),
            color: Color::BLACK,
        });
        assert_eq!(list.preview().len(), list.len());
    }

    #[test]
    fn scaling_by_one_changes_nothing() {
        let mut list = DisplayList::new();
        list.push(DisplayItem::Fill {
            shape: shape(),
            color: Color::BLACK,
        });
        let scaled = list.scaled(1.0);
        match (&list.items[0], &scaled.items[0]) {
            (DisplayItem::Fill { shape: a, .. }, DisplayItem::Fill { shape: b, .. }) => {
                assert_eq!(a.rect, b.rect)
            }
            _ => panic!("the item kind changed"),
        }
    }

    #[test]
    fn scaling_keeps_the_structure_intact() {
        let mut list = DisplayList::new();
        list.with_clip(shape(), |inner| {
            inner.push(DisplayItem::PushOpacity(0.5));
            inner.push(DisplayItem::Fill {
                shape: shape(),
                color: Color::BLACK,
            });
            inner.push(DisplayItem::PopOpacity);
        });
        let scaled = list.scaled(3.0);
        assert_eq!(scaled.len(), list.len());
        assert!(scaled.is_balanced(), "clip and opacity scopes must survive");
        assert!(matches!(scaled.items[1], DisplayItem::PushOpacity(alpha) if alpha == 0.5));
    }
}
