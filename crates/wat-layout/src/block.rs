//! Block layout: widths, heights, stacking and margin collapsing.

use crate::boxes::{font_request, BoxKind, LayoutTree};
use crate::builder::LayoutContext;
use crate::flex::{layout_flex, layout_grid};
use crate::geom::{Point, Rect, Size2D};
use crate::inline::layout_inline_content;
use crate::intrinsic::{shrink_to_fit, uses_shrink_to_fit};
use wat_style::{BoxSizing, ComputedStyle, Sides, Size};

/// Resolved edges of a box for one containing block width.
pub(crate) struct ResolvedEdges {
    pub margin: Sides<f32>,
    pub border: Sides<f32>,
    pub padding: Sides<f32>,
}

impl ResolvedEdges {
    pub fn inner_horizontal(&self) -> f32 {
        self.border.horizontal() + self.padding.horizontal()
    }

    pub fn inner_vertical(&self) -> f32 {
        self.border.vertical() + self.padding.vertical()
    }

    pub fn outer_horizontal(&self) -> f32 {
        self.inner_horizontal() + self.margin.horizontal()
    }
}

/// Resolves margins, borders and padding against `containing_width`.
///
/// Vertical percentages resolve against the containing block's *width*, which
/// is what CSS requires.
pub(crate) fn resolve_edges(style: &ComputedStyle, containing_width: f32) -> ResolvedEdges {
    ResolvedEdges {
        margin: Sides {
            top: style.margin.top.resolve(containing_width),
            right: style.margin.right.resolve(containing_width),
            bottom: style.margin.bottom.resolve(containing_width),
            left: style.margin.left.resolve(containing_width),
        },
        border: style.used_border_width(),
        padding: style.padding.map(|p| p.resolve(containing_width)),
    }
}

/// Clamps `value` by the box's min and max in one axis.
fn clamp_size(value: f32, min: Size, max: Size, basis: Option<f32>) -> f32 {
    let mut result = value;
    if let Some(max) = max.definite(basis) {
        result = result.min(max);
    }
    if let Some(min) = min.definite(basis) {
        result = result.max(min);
    }
    result.max(0.0)
}

/// Lays out an in-flow box whose margin box starts at `origin`.
///
/// `available_width` is how much horizontal room the margin box may use;
/// `containing_width` is the containing block's content width, used to resolve
/// percentages.
pub(crate) fn layout_in_flow_box(
    tree: &mut LayoutTree,
    ctx: &LayoutContext,
    index: usize,
    origin: Point,
    available_width: f32,
    containing_width: f32,
) {
    let style = tree.get(index).style.clone();
    let kind = tree.get(index).kind.clone();
    let edges = resolve_edges(&style, containing_width);

    // ---- width -------------------------------------------------------------
    let inner_surround = edges.inner_horizontal();
    let mut content_width = if let Some(specified) = style.width.definite(Some(containing_width)) {
        match style.box_sizing {
            BoxSizing::BorderBox => (specified - inner_surround).max(0.0),
            BoxSizing::ContentBox => specified,
        }
    } else if uses_shrink_to_fit(&style, &kind) {
        let widths = crate::intrinsic::content_intrinsic_widths(tree, ctx, index);
        shrink_to_fit(
            widths,
            (available_width - edges.outer_horizontal()).max(0.0),
        )
    } else {
        (available_width - edges.outer_horizontal()).max(0.0)
    };

    let width_basis = Some(containing_width);
    content_width = clamp_size(
        content_width,
        adjust_for_box_sizing(style.min_width, &style, inner_surround),
        adjust_for_box_sizing(style.max_width, &style, inner_surround),
        width_basis,
    );

    // ---- horizontal placement ---------------------------------------------
    let border_width = content_width + inner_surround;
    let mut margin = edges.margin;
    let free = available_width - border_width - margin.horizontal();
    let auto_left = style.margin.left.is_auto();
    let auto_right = style.margin.right.is_auto();
    if free > 0.0 && !style.width.is_auto() {
        // `margin: 0 auto` centres a fixed-width box; one auto margin pushes.
        match (auto_left, auto_right) {
            (true, true) => {
                margin.left += free / 2.0;
                margin.right += free / 2.0;
            }
            (true, false) => margin.left += free,
            (false, true) => margin.right += free,
            (false, false) => {}
        }
    }

    let border_x = origin.x + margin.left;
    let border_y = origin.y + margin.top;

    {
        let layout_box = tree.get_mut(index);
        layout_box.margin = margin;
        layout_box.border = edges.border;
        layout_box.padding = edges.padding;
        layout_box.rect = Rect::new(border_x, border_y, border_width, 0.0);
    }

    // ---- content -----------------------------------------------------------
    let content_origin = Point::new(
        border_x + edges.border.left + edges.padding.left,
        border_y + edges.border.top + edges.padding.top,
    );

    let specified_height = style.height.definite(None).map(|h| match style.box_sizing {
        BoxSizing::BorderBox => (h - edges.inner_vertical()).max(0.0),
        BoxSizing::ContentBox => h,
    });

    let content_size = match &kind {
        BoxKind::Replaced(_) => layout_replaced(tree, index, content_width, specified_height),
        BoxKind::Flex => layout_flex(
            tree,
            ctx,
            index,
            content_origin,
            content_width,
            specified_height,
        ),
        BoxKind::Grid => layout_grid(tree, ctx, index, content_origin, content_width),
        _ => layout_block_content(tree, ctx, index, content_origin, content_width),
    };

    let content_height = specified_height.unwrap_or(content_size.height);
    let content_height = clamp_size(
        content_height,
        adjust_for_box_sizing(style.min_height, &style, edges.inner_vertical()),
        adjust_for_box_sizing(style.max_height, &style, edges.inner_vertical()),
        None,
    );

    {
        let layout_box = tree.get_mut(index);
        layout_box.rect.height = content_height + edges.inner_vertical();
        layout_box.scrollable = Size2D::new(
            content_size.width.max(content_width),
            content_size.height.max(content_height),
        );
        // The baseline of a block box is the baseline of its last line box,
        // which inline-blocks need in order to sit on a line.
        layout_box.baseline = layout_box.rect.height;
    }
    if let Some(last_line) = tree
        .children(index)
        .iter()
        .rev()
        .find(|child| matches!(tree.get(**child).kind, BoxKind::Line))
        .copied()
    {
        let line = tree.get(last_line);
        let baseline = line.rect.y + line.baseline - border_y;
        tree.get_mut(index).baseline = baseline;
    }

    place_marker(tree, ctx, index, content_origin);
}

/// `min-width`/`max-height` and friends also respect `box-sizing`.
fn adjust_for_box_sizing(size: Size, style: &ComputedStyle, surround: f32) -> Size {
    match (style.box_sizing, size) {
        (BoxSizing::BorderBox, Size::Definite(length)) => {
            // Only a length that is already in pixels can have the padding and
            // borders taken off here; a percentage is adjusted once it resolves.
            match length.resolve_definite() {
                Some(px) => {
                    Size::Definite(wat_style::LengthPercentage::Px((px - surround).max(0.0)))
                }
                None => size,
            }
        }
        _ => size,
    }
}

/// Lays out the children of a block container.
fn layout_block_content(
    tree: &mut LayoutTree,
    ctx: &LayoutContext,
    index: usize,
    content_origin: Point,
    content_width: f32,
) -> Size2D {
    if tree.has_inline_content(index) {
        return layout_inline_content(tree, ctx, index, content_origin, content_width);
    }

    let children: Vec<usize> = tree
        .children(index)
        .iter()
        .copied()
        .filter(|child| {
            let layout_box = tree.get(*child);
            !layout_box.out_of_flow && !matches!(layout_box.kind, BoxKind::Marker(_))
        })
        .collect();

    let mut cursor = content_origin.y;
    let mut previous_bottom_margin = 0.0f32;
    let mut widest = 0.0f32;
    let mut first = true;

    for child in children {
        let child_style = tree.get(child).style.clone();
        let child_edges = resolve_edges(&child_style, content_width);
        // Adjacent sibling margins collapse to the larger of the two.
        let collapsed = if first {
            child_edges.margin.top
        } else {
            previous_bottom_margin.max(child_edges.margin.top)
        };
        let border_y = cursor + collapsed;
        let origin = Point::new(content_origin.x, border_y - child_edges.margin.top);

        layout_in_flow_box(tree, ctx, child, origin, content_width, content_width);

        let child_box = tree.get(child);
        cursor = child_box.rect.max_y();
        previous_bottom_margin = child_box.margin.bottom;
        widest = widest.max(child_box.margin_box().max_x() - content_origin.x);
        first = false;
    }

    let height = (cursor + previous_bottom_margin - content_origin.y).max(0.0);
    Size2D::new(widest, height)
}

/// Sizes a replaced element from its intrinsic size and specified dimensions.
fn layout_replaced(
    tree: &mut LayoutTree,
    index: usize,
    content_width: f32,
    specified_height: Option<f32>,
) -> Size2D {
    let (intrinsic, has_intrinsic) = match &tree.get(index).kind {
        BoxKind::Replaced(replaced) => match replaced.intrinsic {
            Some(size) if size.width > 0.0 && size.height > 0.0 => (size, true),
            // The CSS default size for a replaced element with no intrinsic one.
            _ => (Size2D::new(300.0, 150.0), false),
        },
        _ => (Size2D::new(300.0, 150.0), false),
    };
    let ratio = intrinsic.width / intrinsic.height.max(1.0);
    let style = tree.get(index).style.clone();

    let width_specified = style.width.definite(None).is_some();
    let width = if width_specified {
        content_width
    } else if let Some(height) = specified_height {
        // Only an aspect-ratio-preserving image derives width from height.
        if has_intrinsic {
            height * ratio
        } else {
            content_width.min(intrinsic.width)
        }
    } else {
        content_width.min(intrinsic.width)
    };

    let height = match specified_height {
        Some(height) => height,
        None if width_specified && has_intrinsic => width / ratio.max(0.001),
        None if has_intrinsic => width / ratio.max(0.001),
        None => intrinsic.height,
    };

    tree.get_mut(index).rect.width =
        width + tree.get(index).border.horizontal() + tree.get(index).padding.horizontal();
    Size2D::new(width, height)
}

/// Positions a list item's marker outside its content box.
fn place_marker(tree: &mut LayoutTree, ctx: &LayoutContext, index: usize, content_origin: Point) {
    let marker = tree
        .children(index)
        .iter()
        .copied()
        .find(|child| matches!(tree.get(*child).kind, BoxKind::Marker(_)));
    let Some(marker) = marker else { return };

    let (label, style) = {
        let marker_box = tree.get(marker);
        let label = match &marker_box.kind {
            BoxKind::Marker(label) => label.clone(),
            _ => String::new(),
        };
        (label, marker_box.style.clone())
    };
    let request = font_request(&style);
    let width = ctx.fonts.measure(&request, &label);
    let metrics = ctx.fonts.line_metrics(&request);
    let line_height = style.line_height_px();
    let leading = (line_height - (metrics.ascent + metrics.descent)).max(0.0);
    let baseline = metrics.ascent + leading / 2.0;

    // Markers sit half an em to the left of the content edge.
    let gap = style.font_size * 0.5;
    let x = match style.list_style_position {
        wat_style::ListStylePosition::Inside => content_origin.x,
        wat_style::ListStylePosition::Outside => content_origin.x - width - gap,
    };
    let marker_box = tree.get_mut(marker);
    marker_box.rect = Rect::new(x, content_origin.y, width, line_height);
    marker_box.baseline = baseline;
}

/// Moves a box and everything under it.
pub(crate) fn translate_subtree(tree: &mut LayoutTree, index: usize, dx: f32, dy: f32) {
    if dx == 0.0 && dy == 0.0 {
        return;
    }
    let mut stack = vec![index];
    while let Some(current) = stack.pop() {
        let layout_box = tree.get_mut(current);
        layout_box.rect = layout_box.rect.translate(dx, dy);
        stack.extend(layout_box.children.iter().copied());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wat_style::{LengthPercentage, Margin};

    #[test]
    fn clamping_respects_min_and_max() {
        assert_eq!(
            clamp_size(
                100.0,
                Size::Auto,
                Size::Definite(LengthPercentage::Px(50.0)),
                None
            ),
            50.0
        );
        assert_eq!(
            clamp_size(
                10.0,
                Size::Definite(LengthPercentage::Px(40.0)),
                Size::None,
                None
            ),
            40.0
        );
        // min wins over max when they conflict, as CSS specifies.
        assert_eq!(
            clamp_size(
                10.0,
                Size::Definite(LengthPercentage::Px(80.0)),
                Size::Definite(LengthPercentage::Px(20.0)),
                None
            ),
            80.0
        );
    }

    #[test]
    fn edges_resolve_percentages_against_width() {
        let mut style = ComputedStyle::initial();
        style.padding = Sides::all(LengthPercentage::Percent(10.0));
        style.margin = Sides::all(Margin::Length(LengthPercentage::Px(4.0)));
        let edges = resolve_edges(&style, 200.0);
        assert_eq!(edges.padding.top, 20.0);
        assert_eq!(edges.padding.left, 20.0);
        assert_eq!(edges.margin.left, 4.0);
        assert_eq!(edges.outer_horizontal(), 48.0);
    }
}
