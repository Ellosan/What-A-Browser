//! Intrinsic sizing: the min-content and max-content widths a box wants.
//!
//! These drive shrink-to-fit sizing for inline-blocks, floats, table cells and
//! flex items with an automatic basis.

use crate::boxes::{font_request, BoxKind, InlineItem, LayoutTree};
use crate::builder::LayoutContext;
use wat_style::{ComputedStyle, Display, LengthPercentage, Size};
use wat_text::segment_words;

/// The pair of intrinsic widths for a box's *content*, excluding its own
/// borders, padding and margins.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct IntrinsicWidths {
    /// Width when every possible break is taken.
    pub min: f32,
    /// Width when no optional break is taken.
    pub max: f32,
}

impl IntrinsicWidths {
    fn zero() -> Self {
        IntrinsicWidths { min: 0.0, max: 0.0 }
    }

    fn combine_max(self, other: IntrinsicWidths) -> Self {
        IntrinsicWidths {
            min: self.min.max(other.min),
            max: self.max.max(other.max),
        }
    }

    fn add(self, other: IntrinsicWidths) -> Self {
        IntrinsicWidths {
            min: self.min + other.min,
            max: self.max + other.max,
        }
    }

    fn grow(self, amount: f32) -> Self {
        IntrinsicWidths {
            min: self.min + amount,
            max: self.max + amount,
        }
    }
}

/// Horizontal border + padding + margin for a box, resolved against `basis`.
fn horizontal_surround(style: &ComputedStyle, basis: f32) -> f32 {
    let border = style.used_border_width();
    border.horizontal()
        + style.padding.left.resolve(basis)
        + style.padding.right.resolve(basis)
        + style.margin.left.resolve(basis)
        + style.margin.right.resolve(basis)
}

/// Intrinsic widths of the box at `index`, including its own surround.
pub fn outer_intrinsic_widths(
    tree: &LayoutTree,
    ctx: &LayoutContext,
    index: usize,
) -> IntrinsicWidths {
    let style = tree.get(index).style.clone();
    let surround = horizontal_surround(&style, 0.0);

    // A definite width short-circuits the walk.
    if let Some(width) = definite_width(&style) {
        let content = if style.box_sizing == wat_style::BoxSizing::BorderBox {
            let inner_surround = style.used_border_width().horizontal()
                + style.padding.left.resolve(0.0)
                + style.padding.right.resolve(0.0);
            (width - inner_surround).max(0.0)
        } else {
            width
        };
        return IntrinsicWidths {
            min: content,
            max: content,
        }
        .grow(surround);
    }

    content_intrinsic_widths(tree, ctx, index).grow(surround)
}

fn definite_width(style: &ComputedStyle) -> Option<f32> {
    match style.width {
        Size::Definite(LengthPercentage::Px(px)) => Some(px),
        _ => None,
    }
}

/// Intrinsic widths of a box's content only.
pub fn content_intrinsic_widths(
    tree: &LayoutTree,
    ctx: &LayoutContext,
    index: usize,
) -> IntrinsicWidths {
    let layout_box = tree.get(index);

    match &layout_box.kind {
        BoxKind::Replaced(replaced) => {
            let width = replaced
                .intrinsic
                .map(|size| size.width)
                // A replaced element with no intrinsic size falls back to the
                // CSS default 300x150 box.
                .unwrap_or(300.0);
            IntrinsicWidths {
                min: width,
                max: width,
            }
        }
        BoxKind::Marker(label) => {
            let request = font_request(&layout_box.style);
            let width = ctx.fonts.measure(&request, label);
            IntrinsicWidths {
                min: width,
                max: width,
            }
        }
        BoxKind::Text(fragment) => {
            let request = font_request(&layout_box.style);
            let width = ctx.fonts.measure(&request, &fragment.text);
            IntrinsicWidths {
                min: width,
                max: width,
            }
        }
        _ => {
            let mut widths = if tree.has_inline_content(index) {
                inline_intrinsic_widths(tree, ctx, index)
            } else {
                IntrinsicWidths::zero()
            };

            let style = layout_box.style.clone();
            let horizontal_flex =
                matches!(layout_box.kind, BoxKind::Flex) && style.flex_direction.is_row();
            let is_grid = matches!(layout_box.kind, BoxKind::Grid);

            let mut children_widths = IntrinsicWidths::zero();
            let mut child_count = 0usize;
            for child in tree.children(index) {
                if tree.get(*child).out_of_flow {
                    continue;
                }
                let child_widths = outer_intrinsic_widths(tree, ctx, *child);
                child_count += 1;
                children_widths = if horizontal_flex || is_grid {
                    // Items sit side by side, so their widths add up.
                    children_widths.add(child_widths)
                } else {
                    children_widths.combine_max(child_widths)
                };
            }

            // Gaps between siblings count towards the intrinsic width.
            if (horizontal_flex || is_grid) && child_count > 1 {
                let gap = style.column_gap.resolve(0.0) * (child_count - 1) as f32;
                children_widths = children_widths.grow(gap);
            }
            if horizontal_flex && style.flex_wrap != wat_style::FlexWrap::NoWrap {
                // Wrapping lets the line break between items.
                children_widths.min = tree
                    .children(index)
                    .iter()
                    .filter(|c| !tree.get(**c).out_of_flow)
                    .map(|c| outer_intrinsic_widths(tree, ctx, *c).min)
                    .fold(0.0, f32::max);
            }

            widths = widths.combine_max(children_widths);
            widths
        }
    }
}

/// Intrinsic widths of an inline formatting context.
fn inline_intrinsic_widths(
    tree: &LayoutTree,
    ctx: &LayoutContext,
    index: usize,
) -> IntrinsicWidths {
    let Some(items) = tree.inline_items.get(&index) else {
        return IntrinsicWidths::zero();
    };

    let mut widths = IntrinsicWidths::zero();
    // Longest unbreakable run and the current line, tracked as we walk.
    let mut current_word = 0.0f32;
    let mut current_line = 0.0f32;

    let flush_word = |word: &mut f32, widths: &mut IntrinsicWidths| {
        widths.min = widths.min.max(*word);
        *word = 0.0;
    };

    for item in items {
        match item {
            InlineItem::Text { style, text, .. } => {
                let request = font_request(style);
                let preserve = style.white_space.preserves_spaces();
                for (segment, is_whitespace) in segment_words(text) {
                    let width = ctx.fonts.measure(&request, segment);
                    if segment == "\n" && style.white_space.preserves_newlines() {
                        flush_word(&mut current_word, &mut widths);
                        widths.max = widths.max.max(current_line);
                        current_line = 0.0;
                        continue;
                    }
                    if is_whitespace && !preserve {
                        // A collapsible space is a break opportunity.
                        flush_word(&mut current_word, &mut widths);
                    } else {
                        current_word += width;
                    }
                    current_line += width;
                }
            }
            InlineItem::Atomic(child) => {
                let child_widths = outer_intrinsic_widths(tree, ctx, *child);
                flush_word(&mut current_word, &mut widths);
                widths.min = widths.min.max(child_widths.min);
                current_line += child_widths.max;
            }
            InlineItem::Break => {
                flush_word(&mut current_word, &mut widths);
                widths.max = widths.max.max(current_line);
                current_line = 0.0;
            }
            InlineItem::Open { style, .. } => {
                current_line += horizontal_surround(style, 0.0);
            }
            InlineItem::Close => {}
        }
    }
    flush_word(&mut current_word, &mut widths);
    widths.max = widths.max.max(current_line);
    // A line can never be narrower than its longest unbreakable piece.
    widths.max = widths.max.max(widths.min);
    widths
}

/// Shrink-to-fit width: `min(max(min-content, available), max-content)`.
pub fn shrink_to_fit(widths: IntrinsicWidths, available: f32) -> f32 {
    widths.max.min(available.max(widths.min)).max(0.0)
}

/// Should this box be sized shrink-to-fit rather than filling its container?
pub fn uses_shrink_to_fit(style: &ComputedStyle, kind: &BoxKind) -> bool {
    if !style.width.is_auto() {
        return false;
    }
    if style.position.is_out_of_flow() {
        return true;
    }
    matches!(kind, BoxKind::InlineBlock)
        || matches!(
            style.display,
            Display::InlineBlock | Display::InlineFlex | Display::TableCell | Display::Table
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrink_to_fit_stays_within_bounds() {
        let widths = IntrinsicWidths {
            min: 50.0,
            max: 200.0,
        };
        assert_eq!(shrink_to_fit(widths, 500.0), 200.0, "never exceed max");
        assert_eq!(shrink_to_fit(widths, 100.0), 100.0, "use what is offered");
        assert_eq!(shrink_to_fit(widths, 10.0), 50.0, "never go below min");
    }

    #[test]
    fn widths_combine_correctly() {
        let a = IntrinsicWidths {
            min: 10.0,
            max: 40.0,
        };
        let b = IntrinsicWidths {
            min: 30.0,
            max: 35.0,
        };
        assert_eq!(
            a.combine_max(b),
            IntrinsicWidths {
                min: 30.0,
                max: 40.0
            }
        );
        assert_eq!(
            a.add(b),
            IntrinsicWidths {
                min: 40.0,
                max: 75.0
            }
        );
        assert_eq!(
            a.grow(5.0),
            IntrinsicWidths {
                min: 15.0,
                max: 45.0
            }
        );
    }
}
