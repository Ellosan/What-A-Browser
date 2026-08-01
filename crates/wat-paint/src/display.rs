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
}
