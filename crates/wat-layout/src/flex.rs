//! Flexbox and a single-axis grid.
//!
//! The flex implementation covers the parts pages actually rely on: base sizes
//! from `flex-basis`/`width`, growing and shrinking, wrapping, `justify-content`
//! and `align-items`, in both row and column directions. Baseline alignment
//! falls back to `flex-start`.
//!
//! `display: grid` is laid out as rows of `grid-template-columns` tracks, which
//! renders the card-grid pattern correctly; grid areas and explicit placement
//! are not implemented.

use std::rc::Rc;

use crate::block::{layout_in_flow_box, resolve_edges, translate_subtree};
use crate::boxes::{BoxKind, LayoutTree};
use crate::builder::LayoutContext;
use crate::geom::{Point, Size2D};
use crate::intrinsic::outer_intrinsic_widths;
use wat_style::{
    AlignItems, AlignSelf, BoxSizing, ComputedStyle, FlexWrap, JustifyContent, LengthPercentage,
    Size, TrackSize,
};

/// A flex item during resolution. Sizes are *outer* sizes: they include the
/// item's own margins, borders and padding.
struct Item {
    index: usize,
    outer_main: f32,
    /// Margins plus borders plus padding along the main axis.
    extra_main: f32,
    min_outer_main: f32,
    grow: f32,
    shrink: f32,
    align: AlignSelf,
}

/// In-flow children of a flex or grid container, in `order` order.
fn flex_children(tree: &LayoutTree, index: usize) -> Vec<usize> {
    let mut children: Vec<usize> = tree
        .children(index)
        .iter()
        .copied()
        .filter(|child| {
            let layout_box = tree.get(*child);
            !layout_box.out_of_flow && !matches!(layout_box.kind, BoxKind::Marker(_))
        })
        .collect();
    children.sort_by_key(|child| tree.get(*child).style.order);
    children
}

/// Replaces an item's style with one that pins the given content size, so the
/// normal box layout path produces the size flexing decided on.
fn pin_size(tree: &mut LayoutTree, index: usize, width: Option<f32>, height: Option<f32>) {
    let mut style = (*tree.get(index).style).clone();
    if let Some(width) = width {
        style.width = Size::Definite(LengthPercentage::Px(width.max(0.0)));
        // The pinned value is a content-box size.
        style.box_sizing = BoxSizing::ContentBox;
    }
    if let Some(height) = height {
        style.height = Size::Definite(LengthPercentage::Px(height.max(0.0)));
        style.box_sizing = BoxSizing::ContentBox;
    }
    tree.get_mut(index).style = Rc::new(style);
}

fn resolved_align(container: &ComputedStyle, item: &ComputedStyle) -> AlignSelf {
    match item.align_self {
        AlignSelf::Auto => match container.align_items {
            AlignItems::Stretch => AlignSelf::Stretch,
            AlignItems::Start => AlignSelf::Start,
            AlignItems::End => AlignSelf::End,
            AlignItems::Center => AlignSelf::Center,
            AlignItems::Baseline => AlignSelf::Baseline,
        },
        explicit => explicit,
    }
}

/// Distributes `free` space along a line according to `justify_content`.
///
/// Returns the offset of the first item and the gap to insert between items.
fn distribute(justify: JustifyContent, free: f32, count: usize) -> (f32, f32) {
    if free <= 0.0 || count == 0 {
        let offset = match justify {
            JustifyContent::End => free.min(0.0).max(free),
            _ => 0.0,
        };
        // Overflowing lines always start at the main-start edge.
        return (if free < 0.0 { 0.0 } else { offset }, 0.0);
    }
    match justify {
        JustifyContent::Start => (0.0, 0.0),
        JustifyContent::End => (free, 0.0),
        JustifyContent::Center => (free / 2.0, 0.0),
        JustifyContent::SpaceBetween => {
            if count > 1 {
                (0.0, free / (count - 1) as f32)
            } else {
                (0.0, 0.0)
            }
        }
        JustifyContent::SpaceAround => {
            let gap = free / count as f32;
            (gap / 2.0, gap)
        }
        JustifyContent::SpaceEvenly => {
            let gap = free / (count + 1) as f32;
            (gap, gap)
        }
    }
}

/// Lays out a flex container's children, returning the content size used.
pub(crate) fn layout_flex(
    tree: &mut LayoutTree,
    ctx: &LayoutContext,
    index: usize,
    content_origin: Point,
    content_width: f32,
    specified_height: Option<f32>,
) -> Size2D {
    let style = tree.get(index).style.clone();
    let horizontal = style.flex_direction.is_row();
    let children = flex_children(tree, index);
    if children.is_empty() {
        return Size2D::ZERO;
    }

    let main_gap = if horizontal {
        style.column_gap.resolve(content_width)
    } else {
        style.row_gap.resolve(content_width)
    };
    let cross_gap = if horizontal {
        style.row_gap.resolve(content_width)
    } else {
        style.column_gap.resolve(content_width)
    };
    let main_available = if horizontal {
        content_width
    } else {
        specified_height.unwrap_or(f32::INFINITY)
    };

    // ---- base sizes --------------------------------------------------------
    let mut items: Vec<Item> = Vec::with_capacity(children.len());
    for child in &children {
        let child_style = tree.get(*child).style.clone();
        let edges = resolve_edges(&child_style, content_width);
        let extra_main = if horizontal {
            edges.margin.horizontal() + edges.inner_horizontal()
        } else {
            edges.margin.vertical() + edges.inner_vertical()
        };

        let outer_main = if horizontal {
            let basis = child_style
                .flex_basis
                .definite(Some(content_width))
                .or_else(|| child_style.width.definite(Some(content_width)));
            match basis {
                Some(content) => content + edges.inner_horizontal() + edges.margin.horizontal(),
                None => outer_intrinsic_widths(tree, ctx, *child).max,
            }
        } else {
            // Column: the base size comes from a trial layout.
            let cross = content_width;
            layout_in_flow_box(tree, ctx, *child, Point::ZERO, cross, cross);
            let trial = tree.get(*child);
            child_style
                .flex_basis
                .definite(None)
                .or_else(|| child_style.height.definite(None))
                .map(|h| h + edges.inner_vertical() + edges.margin.vertical())
                .unwrap_or(trial.rect.height + edges.margin.vertical())
        };

        let min_outer_main = if horizontal {
            let intrinsic_min = outer_intrinsic_widths(tree, ctx, *child).min;
            child_style
                .min_width
                .definite(Some(content_width))
                .map(|min| min + extra_main)
                .unwrap_or(intrinsic_min)
                .min(outer_main.max(intrinsic_min))
        } else {
            child_style
                .min_height
                .definite(None)
                .map(|min| min + extra_main)
                .unwrap_or(0.0)
        };

        items.push(Item {
            index: *child,
            outer_main,
            extra_main,
            min_outer_main,
            grow: child_style.flex_grow,
            shrink: child_style.flex_shrink,
            align: resolved_align(&style, &child_style),
        });
    }

    // ---- wrapping ----------------------------------------------------------
    let wraps = style.flex_wrap != FlexWrap::NoWrap && main_available.is_finite();
    let mut lines: Vec<Vec<usize>> = Vec::new();
    if wraps {
        let mut current: Vec<usize> = Vec::new();
        let mut used = 0.0f32;
        for (position, item) in items.iter().enumerate() {
            let gap = if current.is_empty() { 0.0 } else { main_gap };
            if !current.is_empty() && used + gap + item.outer_main > main_available + 0.01 {
                lines.push(std::mem::take(&mut current));
                used = 0.0;
            }
            let gap = if current.is_empty() { 0.0 } else { main_gap };
            used += gap + item.outer_main;
            current.push(position);
        }
        if !current.is_empty() {
            lines.push(current);
        }
    } else {
        lines.push((0..items.len()).collect());
    }
    if style.flex_wrap == FlexWrap::WrapReverse {
        lines.reverse();
    }

    // ---- flex each line ----------------------------------------------------
    let mut cross_cursor = if horizontal {
        content_origin.y
    } else {
        content_origin.x
    };
    let mut total_main_extent = 0.0f32;
    let mut total_cross_extent = 0.0f32;

    for (line_number, line) in lines.iter().enumerate() {
        let gaps = main_gap * line.len().saturating_sub(1) as f32;
        let base_total: f32 = line.iter().map(|i| items[*i].outer_main).sum();
        let free = if main_available.is_finite() {
            main_available - base_total - gaps
        } else {
            0.0
        };

        if free > 0.0 {
            let total_grow: f32 = line.iter().map(|i| items[*i].grow).sum();
            if total_grow > 0.0 {
                for position in line {
                    let item = &mut items[*position];
                    item.outer_main += free * item.grow / total_grow;
                }
            }
        } else if free < 0.0 {
            // Shrink in proportion to shrink factor times base size.
            let mut remaining = -free;
            let mut weights: Vec<f32> = line
                .iter()
                .map(|i| items[*i].shrink * items[*i].outer_main)
                .collect();
            // Two passes are enough in practice: the second respects minimums.
            for _ in 0..2 {
                let total: f32 = weights.iter().sum();
                if total <= 0.0 || remaining <= 0.01 {
                    break;
                }
                let mut leftover = 0.0f32;
                for (slot, position) in line.iter().enumerate() {
                    if weights[slot] <= 0.0 {
                        continue;
                    }
                    let item = &mut items[*position];
                    let wanted = remaining * weights[slot] / total;
                    let floor = item.min_outer_main.max(item.extra_main);
                    let possible = (item.outer_main - floor).max(0.0);
                    let applied = wanted.min(possible);
                    item.outer_main -= applied;
                    leftover += wanted - applied;
                    if applied < wanted {
                        weights[slot] = 0.0;
                    }
                }
                remaining = leftover;
            }
        }

        // ---- place items along the main axis -------------------------------
        let used: f32 = line.iter().map(|i| items[*i].outer_main).sum::<f32>() + gaps;
        let leftover = if main_available.is_finite() {
            main_available - used
        } else {
            0.0
        };
        let (start_offset, extra_gap) = distribute(style.justify_content, leftover, line.len());

        // Cross sizes need a first pass to learn each item's natural extent.
        let mut line_cross = 0.0f32;
        for position in line {
            let item = &items[*position];
            let content_main = (item.outer_main - item.extra_main).max(0.0);
            if horizontal {
                pin_size(tree, item.index, Some(content_main), None);
                layout_in_flow_box(
                    tree,
                    ctx,
                    item.index,
                    Point::ZERO,
                    item.outer_main,
                    content_width,
                );
            } else {
                let cross_available = content_width;
                let stretch =
                    item.align == AlignSelf::Stretch && tree.get(item.index).style.width.is_auto();
                if !stretch {
                    let widths = outer_intrinsic_widths(tree, ctx, item.index);
                    let width = crate::intrinsic::shrink_to_fit(widths, cross_available);
                    pin_size(tree, item.index, Some(width), None);
                }
                pin_size(tree, item.index, None, Some(content_main));
                layout_in_flow_box(
                    tree,
                    ctx,
                    item.index,
                    Point::ZERO,
                    cross_available,
                    cross_available,
                );
            }
            let laid_out = tree.get(item.index);
            let cross = if horizontal {
                laid_out.rect.height + laid_out.margin.vertical()
            } else {
                laid_out.rect.width + laid_out.margin.horizontal()
            };
            line_cross = line_cross.max(cross);
        }

        // A single-line container with a definite cross size fills it.
        if lines.len() == 1 {
            if horizontal {
                if let Some(height) = specified_height {
                    line_cross = line_cross.max(height);
                }
            } else {
                line_cross = line_cross.max(content_width);
            }
        }

        let mut main_cursor = if horizontal {
            content_origin.x
        } else {
            content_origin.y
        } + start_offset;

        for position in line {
            let item = &items[*position];
            let (cross_extent, margin_before, margin_after) = {
                let laid_out = tree.get(item.index);
                if horizontal {
                    (
                        laid_out.rect.height,
                        laid_out.margin.top,
                        laid_out.margin.bottom,
                    )
                } else {
                    (
                        laid_out.rect.width,
                        laid_out.margin.left,
                        laid_out.margin.right,
                    )
                }
            };
            let free_cross = (line_cross - cross_extent - margin_before - margin_after).max(0.0);
            let cross_offset = match item.align {
                AlignSelf::Start | AlignSelf::Baseline | AlignSelf::Auto => 0.0,
                AlignSelf::End => free_cross,
                AlignSelf::Center => free_cross / 2.0,
                AlignSelf::Stretch => 0.0,
            };

            // Stretch resizes rather than positions.
            if item.align == AlignSelf::Stretch
                && free_cross > 0.0
                && horizontal
                && tree.get(item.index).style.height.is_auto()
            {
                let target = line_cross - margin_before - margin_after;
                let edges = tree.get(item.index).edges();
                pin_size(
                    tree,
                    item.index,
                    None,
                    Some((target - edges.vertical()).max(0.0)),
                );
                layout_in_flow_box(
                    tree,
                    ctx,
                    item.index,
                    Point::ZERO,
                    item.outer_main,
                    content_width,
                );
            }

            let current = tree.get(item.index).rect.origin();
            let (target_x, target_y) = if horizontal {
                (
                    main_cursor + tree.get(item.index).margin.left,
                    cross_cursor + cross_offset + margin_before,
                )
            } else {
                (
                    cross_cursor + cross_offset + margin_before,
                    main_cursor + tree.get(item.index).margin.top,
                )
            };
            translate_subtree(tree, item.index, target_x - current.x, target_y - current.y);

            main_cursor += item.outer_main + main_gap + extra_gap;
        }

        let line_main_extent = used.max(0.0);
        total_main_extent = total_main_extent.max(line_main_extent);
        cross_cursor += line_cross;
        total_cross_extent += line_cross;
        if line_number + 1 < lines.len() {
            cross_cursor += cross_gap;
            total_cross_extent += cross_gap;
        }
    }

    if horizontal {
        Size2D::new(total_main_extent, total_cross_extent)
    } else {
        Size2D::new(total_cross_extent, total_main_extent)
    }
}

/// Resolves `grid-template-columns` into concrete widths.
fn resolve_tracks(tracks: &[TrackSize], available: f32, gap: f32) -> Vec<f32> {
    if tracks.is_empty() {
        return vec![available];
    }
    let gaps = gap * tracks.len().saturating_sub(1) as f32;
    let mut widths = vec![0.0f32; tracks.len()];
    let mut flexible = Vec::new();
    let mut fixed_total = 0.0f32;

    for (slot, track) in tracks.iter().enumerate() {
        match track {
            TrackSize::Length(length) => {
                widths[slot] = length.resolve(available).max(0.0);
                fixed_total += widths[slot];
            }
            TrackSize::Fraction(fraction) => flexible.push((slot, fraction.max(0.0))),
            // `auto`, `min-content` and `max-content` behave as `1fr` here.
            _ => flexible.push((slot, 1.0)),
        }
    }

    let remaining = (available - gaps - fixed_total).max(0.0);
    let total_fraction: f32 = flexible.iter().map(|(_, f)| *f).sum();
    if total_fraction > 0.0 {
        for (slot, fraction) in flexible {
            widths[slot] = remaining * fraction / total_fraction;
        }
    }
    widths
}

/// Lays out a grid container as rows of tracks.
pub(crate) fn layout_grid(
    tree: &mut LayoutTree,
    ctx: &LayoutContext,
    index: usize,
    content_origin: Point,
    content_width: f32,
) -> Size2D {
    let style = tree.get(index).style.clone();
    let children = flex_children(tree, index);
    if children.is_empty() {
        return Size2D::ZERO;
    }

    let column_gap = style.column_gap.resolve(content_width);
    let row_gap = style.row_gap.resolve(content_width);
    let widths = resolve_tracks(&style.grid_template_columns, content_width, column_gap);
    let columns = widths.len().max(1);

    let mut y = content_origin.y;
    let mut widest = 0.0f32;

    for row in children.chunks(columns) {
        let mut row_height = 0.0f32;
        let mut x = content_origin.x;
        for (slot, child) in row.iter().enumerate() {
            let track_width = widths[slot.min(widths.len() - 1)];
            layout_in_flow_box(
                tree,
                ctx,
                *child,
                Point::new(x, y),
                track_width,
                track_width,
            );
            let laid_out = tree.get(*child);
            row_height = row_height.max(laid_out.rect.height + laid_out.margin.vertical());
            widest = widest.max(laid_out.margin_box().max_x() - content_origin.x);
            x += track_width + column_gap;
        }
        y += row_height + row_gap;
    }

    // The trailing gap is not part of the content.
    let height = (y - content_origin.y - row_gap).max(0.0);
    Size2D::new(widest.max(content_width), height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn justify_content_distribution() {
        assert_eq!(distribute(JustifyContent::Start, 100.0, 3), (0.0, 0.0));
        assert_eq!(distribute(JustifyContent::End, 100.0, 3), (100.0, 0.0));
        assert_eq!(distribute(JustifyContent::Center, 100.0, 3), (50.0, 0.0));
        assert_eq!(
            distribute(JustifyContent::SpaceBetween, 90.0, 3),
            (0.0, 45.0)
        );
        assert_eq!(
            distribute(JustifyContent::SpaceAround, 90.0, 3),
            (15.0, 30.0)
        );
        assert_eq!(
            distribute(JustifyContent::SpaceEvenly, 90.0, 2),
            (30.0, 30.0)
        );
        // A single item cannot have space between.
        assert_eq!(
            distribute(JustifyContent::SpaceBetween, 50.0, 1),
            (0.0, 0.0)
        );
        // Overflow starts at the main-start edge.
        assert_eq!(distribute(JustifyContent::Center, -20.0, 2), (0.0, 0.0));
    }

    #[test]
    fn fraction_tracks_share_the_remainder() {
        let tracks = vec![TrackSize::Fraction(1.0), TrackSize::Fraction(1.0)];
        let widths = resolve_tracks(&tracks, 200.0, 0.0);
        assert_eq!(widths, vec![100.0, 100.0]);

        let widths = resolve_tracks(&tracks, 200.0, 20.0);
        assert_eq!(widths, vec![90.0, 90.0], "the gap comes out first");
    }

    #[test]
    fn fixed_tracks_take_priority() {
        let tracks = vec![
            TrackSize::Length(LengthPercentage::Px(50.0)),
            TrackSize::Fraction(1.0),
            TrackSize::Fraction(3.0),
        ];
        let widths = resolve_tracks(&tracks, 250.0, 0.0);
        assert_eq!(widths[0], 50.0);
        assert_eq!(widths[1], 50.0);
        assert_eq!(widths[2], 150.0);
    }

    #[test]
    fn auto_tracks_behave_like_one_fraction() {
        let tracks = vec![TrackSize::Auto, TrackSize::Auto];
        assert_eq!(resolve_tracks(&tracks, 100.0, 0.0), vec![50.0, 50.0]);
    }

    #[test]
    fn no_tracks_means_a_single_column() {
        assert_eq!(resolve_tracks(&[], 300.0, 10.0), vec![300.0]);
    }
}
