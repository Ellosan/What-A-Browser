//! Layout for What-A-Browser: box tree construction plus block, inline, flex
//! and grid layout.
//!
//! ```no_run
//! use wat_layout::{layout_document, LayoutContext, NoImages, Size2D};
//! # let document = wat_html::parse("<p>hi</p>");
//! # let engine = wat_style::StyleEngine::new();
//! # let styles = engine.compute(&document, &Default::default(), &Default::default());
//! # let fonts = wat_text::FontStore::new();
//! let ctx = LayoutContext::new(
//!     &document,
//!     &styles,
//!     &fonts,
//!     &NoImages,
//!     Size2D::new(1024.0, 768.0),
//! );
//! let tree = layout_document(&ctx);
//! assert!(tree.root.is_some());
//! ```

mod block;
pub mod boxes;
pub mod builder;
mod flex;
pub mod geom;
mod inline;
pub mod intrinsic;

pub use boxes::{
    font_request, BoxKind, InlineItem, LayoutBox, LayoutTree, Replaced, ReplacedKind, TextFragment,
};
pub use builder::{BoxTreeBuilder, ImageProvider, LayoutContext, NoImages};
pub use geom::{Point, Rect, Size2D};
pub use intrinsic::{shrink_to_fit, IntrinsicWidths};

use block::{layout_in_flow_box, translate_subtree};
use wat_style::Position;

/// Lays out a whole document for the viewport in `ctx`.
pub fn layout_document(ctx: &LayoutContext) -> LayoutTree {
    let mut tree = BoxTreeBuilder::new(ctx).build();
    let Some(root) = tree.root else {
        return tree;
    };

    layout_in_flow_box(
        &mut tree,
        ctx,
        root,
        Point::ZERO,
        ctx.viewport.width,
        ctx.viewport.width,
    );

    // The root box always covers at least the viewport so its background does.
    let root_height = tree.get(root).rect.height;
    if root_height < ctx.viewport.height {
        tree.get_mut(root).rect.height = ctx.viewport.height;
    }

    layout_out_of_flow(&mut tree, ctx);
    apply_relative_offsets(&mut tree);

    let bounds = tree.bounds();
    tree.viewport = ctx.viewport;
    tree.document_size = Size2D::new(
        bounds.max_x().max(ctx.viewport.width),
        bounds.max_y().max(ctx.viewport.height),
    );
    tree
}

/// The containing block rectangle for an out-of-flow box.
fn containing_block(tree: &LayoutTree, ctx: &LayoutContext, index: usize) -> Rect {
    let viewport = Rect::new(0.0, 0.0, ctx.viewport.width, ctx.viewport.height);
    if tree.get(index).style.position == Position::Fixed {
        return viewport;
    }
    let mut current = tree.get(index).parent;
    while let Some(ancestor) = current {
        let layout_box = tree.get(ancestor);
        if layout_box.style.position.is_positioned() {
            return layout_box.padding_box();
        }
        current = layout_box.parent;
    }
    // No positioned ancestor: the initial containing block.
    match tree.root {
        Some(root) => {
            let root_box = tree.get(root);
            Rect::new(
                0.0,
                0.0,
                root_box.rect.width.max(ctx.viewport.width),
                root_box.rect.height.max(ctx.viewport.height),
            )
        }
        None => viewport,
    }
}

/// Lays out and positions absolutely positioned and fixed boxes.
fn layout_out_of_flow(tree: &mut LayoutTree, ctx: &LayoutContext) {
    for index in tree.preorder() {
        if !tree.get(index).out_of_flow {
            continue;
        }
        let block = containing_block(tree, ctx, index);
        let style = tree.get(index).style.clone();

        layout_in_flow_box(
            tree,
            ctx,
            index,
            Point::new(block.x, block.y),
            block.width,
            block.width,
        );

        let rect = tree.get(index).rect;
        let margin = tree.get(index).margin;

        // `left`/`right` and `top`/`bottom` place the box; when both are set the
        // start edge wins unless the width is auto, which we already resolved.
        let target_x = match (style.inset.left, style.inset.right) {
            (Some(left), _) => block.x + left.resolve(block.width) + margin.left,
            (None, Some(right)) => {
                block.max_x() - right.resolve(block.width) - rect.width - margin.right
            }
            (None, None) => rect.x,
        };
        let target_y = match (style.inset.top, style.inset.bottom) {
            (Some(top), _) => block.y + top.resolve(block.height) + margin.top,
            (None, Some(bottom)) => {
                block.max_y() - bottom.resolve(block.height) - rect.height - margin.bottom
            }
            (None, None) => rect.y,
        };

        translate_subtree(tree, index, target_x - rect.x, target_y - rect.y);
    }
}

/// Offsets relatively positioned boxes, which does not affect their siblings.
fn apply_relative_offsets(tree: &mut LayoutTree) {
    for index in tree.preorder() {
        let style = tree.get(index).style.clone();
        if !matches!(style.position, Position::Relative | Position::Sticky) {
            continue;
        }
        let basis = tree
            .get(index)
            .parent
            .map(|parent| tree.get(parent).content_box())
            .unwrap_or(tree.get(index).rect);

        let dx = match (style.inset.left, style.inset.right) {
            (Some(left), _) => left.resolve(basis.width),
            (None, Some(right)) => -right.resolve(basis.width),
            (None, None) => 0.0,
        };
        let dy = match (style.inset.top, style.inset.bottom) {
            (Some(top), _) => top.resolve(basis.height),
            (None, Some(bottom)) => -bottom.resolve(basis.height),
            (None, None) => 0.0,
        };
        translate_subtree(tree, index, dx, dy);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wat_css::{MatchContext, MediaContext, Origin, Stylesheet};
    use wat_dom::Document;
    use wat_style::{StyleEngine, StyleTree};
    use wat_text::FontStore;

    /// Uses a font-less store so metrics are exact: every glyph advances half
    /// the font size, ascent is 0.8em and descent 0.2em.
    struct Harness {
        document: Document,
        styles: StyleTree,
        fonts: FontStore,
    }

    fn harness(html: &str, css: &str) -> Harness {
        let document = wat_html::parse(html);
        let mut engine = StyleEngine::new();
        engine.add_author_sheet(Stylesheet::parse(css, Origin::Author));
        let styles = engine.compute(
            &document,
            &MediaContext::screen(1000.0, 800.0),
            &MatchContext::default(),
        );
        Harness {
            document,
            styles,
            fonts: FontStore::empty(),
        }
    }

    fn lay_out(harness: &Harness, width: f32, height: f32) -> LayoutTree {
        let ctx = LayoutContext::new(
            &harness.document,
            &harness.styles,
            &harness.fonts,
            &NoImages,
            Size2D::new(width, height),
        );
        layout_document(&ctx)
    }

    /// The box generated by the first node matching `selector`.
    fn box_of<'a>(harness: &Harness, tree: &'a LayoutTree, selector: &str) -> &'a LayoutBox {
        let node = harness
            .document
            .query(selector)
            .unwrap_or_else(|| panic!("no node matches {selector}"));
        let index = tree
            .box_for_node(node)
            .unwrap_or_else(|| panic!("no box for {selector}"));
        tree.get(index)
    }

    #[test]
    fn block_boxes_fill_their_container() {
        let harness = harness("<div>x</div>", "body { margin: 0 } div { height: 40px }");
        let tree = lay_out(&harness, 500.0, 300.0);
        let div = box_of(&harness, &tree, "div");
        assert_eq!(div.rect.width, 500.0);
        assert_eq!(div.rect.height, 40.0);
        assert_eq!(div.rect.x, 0.0);
    }

    #[test]
    fn body_margin_indents_content() {
        let harness = harness("<div>x</div>", "div { height: 10px }");
        let tree = lay_out(&harness, 500.0, 300.0);
        let div = box_of(&harness, &tree, "div");
        assert_eq!(div.rect.x, 8.0, "the UA body margin is 8px");
        assert_eq!(div.rect.width, 484.0);
    }

    #[test]
    fn padding_and_border_shrink_the_content_box() {
        let harness = harness(
            "<div>x</div>",
            "body { margin: 0 } div { padding: 10px; border: 5px solid black; height: 20px }",
        );
        let tree = lay_out(&harness, 300.0, 300.0);
        let div = box_of(&harness, &tree, "div");
        assert_eq!(div.rect.width, 300.0);
        assert_eq!(div.content_box().width, 270.0);
        assert_eq!(div.rect.height, 50.0, "20 + 2*10 + 2*5");
    }

    #[test]
    fn border_box_sizing_includes_padding_in_the_width() {
        let harness = harness(
            "<div>x</div>",
            "body{margin:0} div { box-sizing: border-box; width: 200px; padding: 20px; height: 100px }",
        );
        let tree = lay_out(&harness, 400.0, 400.0);
        let div = box_of(&harness, &tree, "div");
        assert_eq!(div.rect.width, 200.0);
        assert_eq!(div.content_box().width, 160.0);
        assert_eq!(div.rect.height, 100.0);
    }

    #[test]
    fn auto_margins_centre_a_fixed_width_box() {
        let harness = harness(
            "<div>x</div>",
            "body{margin:0} div { width: 100px; margin: 0 auto; height: 10px }",
        );
        let tree = lay_out(&harness, 500.0, 300.0);
        let div = box_of(&harness, &tree, "div");
        assert_eq!(div.rect.x, 200.0);
        assert_eq!(div.rect.width, 100.0);
    }

    #[test]
    fn siblings_stack_vertically() {
        let harness = harness(
            "<div class=a>1</div><div class=b>2</div>",
            "body{margin:0} div { height: 30px; margin: 0 }",
        );
        let tree = lay_out(&harness, 300.0, 300.0);
        let a = box_of(&harness, &tree, ".a");
        let b = box_of(&harness, &tree, ".b");
        assert_eq!(a.rect.y, 0.0);
        assert_eq!(b.rect.y, 30.0);
    }

    #[test]
    fn adjacent_margins_collapse_to_the_larger() {
        let harness = harness(
            "<div class=a>1</div><div class=b>2</div>",
            "body{margin:0} .a { height: 10px; margin-bottom: 30px } .b { height: 10px; margin-top: 10px }",
        );
        let tree = lay_out(&harness, 300.0, 300.0);
        let b = box_of(&harness, &tree, ".b");
        assert_eq!(b.rect.y, 40.0, "10 + max(30, 10)");
    }

    #[test]
    fn percentage_width_resolves_against_the_container() {
        let harness = harness(
            "<div class=outer><div class=inner>x</div></div>",
            "body{margin:0} .outer { width: 400px } .inner { width: 50%; height: 10px }",
        );
        let tree = lay_out(&harness, 800.0, 400.0);
        assert_eq!(box_of(&harness, &tree, ".inner").rect.width, 200.0);
    }

    #[test]
    fn text_wraps_at_the_container_edge() {
        // 10px font: each glyph is 5px wide with the synthetic metrics, so each
        // four-letter word is 20px and only one fits on a 40px line.
        let harness = harness(
            "<p>aaaa bbbb cccc</p>",
            "body{margin:0} p { font-size: 10px; width: 40px; margin: 0; line-height: 20px }",
        );
        let tree = lay_out(&harness, 200.0, 400.0);
        let p_node = harness.document.query("p").unwrap();
        let p_index = tree.box_for_node(p_node).unwrap();
        let lines: Vec<usize> = tree
            .children(p_index)
            .iter()
            .copied()
            .filter(|c| matches!(tree.get(*c).kind, BoxKind::Line))
            .collect();
        assert_eq!(lines.len(), 3, "each word needs its own line");
        assert_eq!(tree.get(p_index).rect.height, 60.0);
        // Lines stack downwards.
        assert!(tree.get(lines[1]).rect.y > tree.get(lines[0]).rect.y);
    }

    #[test]
    fn nowrap_keeps_text_on_one_line() {
        let harness = harness(
            "<p>aaaa bbbb cccc</p>",
            "body{margin:0} p { font-size: 10px; width: 50px; white-space: nowrap; margin: 0 }",
        );
        let tree = lay_out(&harness, 200.0, 400.0);
        let p_index = tree
            .box_for_node(harness.document.query("p").unwrap())
            .unwrap();
        let lines = tree
            .children(p_index)
            .iter()
            .filter(|c| matches!(tree.get(**c).kind, BoxKind::Line))
            .count();
        assert_eq!(lines, 1);
    }

    #[test]
    fn br_forces_a_line_break() {
        let harness = harness(
            "<p>a<br>b</p>",
            "body{margin:0} p { font-size: 10px; line-height: 10px; margin: 0 }",
        );
        let tree = lay_out(&harness, 500.0, 400.0);
        let p_index = tree
            .box_for_node(harness.document.query("p").unwrap())
            .unwrap();
        let lines = tree
            .children(p_index)
            .iter()
            .filter(|c| matches!(tree.get(**c).kind, BoxKind::Line))
            .count();
        assert_eq!(lines, 2);
        assert_eq!(tree.get(p_index).rect.height, 20.0);
    }

    #[test]
    fn text_align_moves_the_line() {
        let harness = harness(
            "<p>ab</p>",
            "body{margin:0} p { font-size: 10px; width: 100px; text-align: right; margin: 0 }",
        );
        let tree = lay_out(&harness, 200.0, 400.0);
        let text = tree
            .boxes
            .iter()
            .find(|b| b.kind.is_text())
            .expect("a text fragment");
        // Two glyphs at 5px each, flush to the right edge of a 100px line.
        assert!((text.rect.x - 90.0).abs() < 0.01, "got {}", text.rect.x);
    }

    #[test]
    fn centred_text_is_centred() {
        let harness = harness(
            "<p>ab</p>",
            "body{margin:0} p { font-size: 10px; width: 100px; text-align: center; margin: 0 }",
        );
        let tree = lay_out(&harness, 200.0, 400.0);
        let text = tree.boxes.iter().find(|b| b.kind.is_text()).unwrap();
        assert!((text.rect.x - 45.0).abs() < 0.01, "got {}", text.rect.x);
    }

    #[test]
    fn inline_content_and_block_siblings_get_anonymous_boxes() {
        let harness = harness("<div>text<p>block</p>more</div>", "body{margin:0}");
        let tree = lay_out(&harness, 300.0, 300.0);
        let div_index = tree
            .box_for_node(harness.document.query("div").unwrap())
            .unwrap();
        let kinds: Vec<&BoxKind> = tree
            .children(div_index)
            .iter()
            .map(|c| &tree.get(*c).kind)
            .collect();
        assert_eq!(kinds.len(), 3);
        assert!(matches!(kinds[0], BoxKind::AnonymousBlock));
        assert!(matches!(kinds[1], BoxKind::Block));
        assert!(matches!(kinds[2], BoxKind::AnonymousBlock));
    }

    #[test]
    fn display_none_generates_no_box() {
        let harness = harness(
            "<div class=a>a</div><div class=b>b</div>",
            ".a { display: none }",
        );
        let tree = lay_out(&harness, 300.0, 300.0);
        let hidden = harness.document.query(".a").unwrap();
        assert!(tree.box_for_node(hidden).is_none());
        assert!(tree
            .box_for_node(harness.document.query(".b").unwrap())
            .is_some());
    }

    #[test]
    fn display_contents_splices_children_in() {
        let harness = harness(
            "<div class=wrap><span class=c>x</span></div>",
            ".wrap { display: contents }",
        );
        let tree = lay_out(&harness, 300.0, 300.0);
        assert!(tree
            .box_for_node(harness.document.query(".wrap").unwrap())
            .is_none());
        assert!(tree.boxes.iter().any(|b| b.kind.is_text()));
    }

    #[test]
    fn flex_row_grows_items_to_fill() {
        let harness = harness(
            "<div class=f><div class=a>a</div><div class=b>b</div></div>",
            "body{margin:0} .f { display: flex } .a { flex: 1 } .b { flex: 3 }",
        );
        let tree = lay_out(&harness, 400.0, 300.0);
        let a = box_of(&harness, &tree, ".a");
        let b = box_of(&harness, &tree, ".b");
        assert!((a.rect.width - 100.0).abs() < 0.5, "got {}", a.rect.width);
        assert!((b.rect.width - 300.0).abs() < 0.5, "got {}", b.rect.width);
        assert_eq!(a.rect.y, b.rect.y, "row items share a cross position");
        assert!(b.rect.x > a.rect.x);
    }

    #[test]
    fn flex_gap_separates_items() {
        let harness = harness(
            "<div class=f><div class=a>a</div><div class=b>b</div></div>",
            "body{margin:0} .f { display: flex; gap: 20px } .a, .b { flex: 1 }",
        );
        let tree = lay_out(&harness, 220.0, 300.0);
        let a = box_of(&harness, &tree, ".a");
        let b = box_of(&harness, &tree, ".b");
        assert!((a.rect.width - 100.0).abs() < 0.5, "got {}", a.rect.width);
        assert!((b.rect.x - a.rect.max_x() - 20.0).abs() < 0.5);
    }

    #[test]
    fn flex_justify_content_end_pushes_items_right() {
        let harness = harness(
            "<div class=f><div class=a>a</div></div>",
            "body{margin:0} .f { display: flex; justify-content: flex-end } .a { width: 50px; height: 10px }",
        );
        let tree = lay_out(&harness, 200.0, 300.0);
        let a = box_of(&harness, &tree, ".a");
        assert!((a.rect.x - 150.0).abs() < 0.5, "got {}", a.rect.x);
    }

    #[test]
    fn flex_column_stacks_items() {
        let harness = harness(
            "<div class=f><div class=a>a</div><div class=b>b</div></div>",
            "body{margin:0} .f { display: flex; flex-direction: column } .a, .b { height: 25px }",
        );
        let tree = lay_out(&harness, 200.0, 300.0);
        let a = box_of(&harness, &tree, ".a");
        let b = box_of(&harness, &tree, ".b");
        assert_eq!(a.rect.x, b.rect.x);
        assert!((b.rect.y - a.rect.max_y()).abs() < 0.5);
    }

    #[test]
    fn flex_align_items_center_centres_in_the_cross_axis() {
        let harness = harness(
            "<div class=f><div class=a>a</div></div>",
            "body{margin:0} .f { display: flex; align-items: center; height: 100px } .a { height: 20px }",
        );
        let tree = lay_out(&harness, 200.0, 300.0);
        let a = box_of(&harness, &tree, ".a");
        assert!((a.rect.y - 40.0).abs() < 0.5, "got {}", a.rect.y);
    }

    #[test]
    fn flex_wrap_moves_overflow_to_a_new_line() {
        let harness = harness(
            "<div class=f><div class=i>1</div><div class=i>2</div><div class=i>3</div></div>",
            "body{margin:0} .f { display: flex; flex-wrap: wrap } .i { width: 80px; height: 10px; flex: 0 0 80px }",
        );
        let tree = lay_out(&harness, 200.0, 300.0);
        let items: Vec<&LayoutBox> = tree
            .boxes
            .iter()
            .filter(|b| {
                b.node
                    .and_then(|n| harness.document.element(n))
                    .is_some_and(|el| el.classes().any(|c| c == "i"))
            })
            .collect();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].rect.y, items[1].rect.y);
        assert!(items[2].rect.y > items[0].rect.y, "the third item wraps");
    }

    #[test]
    fn grid_splits_the_row_into_tracks() {
        let harness = harness(
            "<div class=g><div class=c>1</div><div class=c>2</div><div class=c>3</div></div>",
            "body{margin:0} .g { display: grid; grid-template-columns: repeat(2, 1fr); gap: 10px } .c { height: 20px }",
        );
        let tree = lay_out(&harness, 210.0, 400.0);
        let cells: Vec<&LayoutBox> = tree
            .boxes
            .iter()
            .filter(|b| {
                b.node
                    .and_then(|n| harness.document.element(n))
                    .is_some_and(|el| el.classes().any(|c| c == "c"))
            })
            .collect();
        assert_eq!(cells.len(), 3);
        assert!((cells[0].rect.width - 100.0).abs() < 0.5);
        assert!((cells[1].rect.x - 110.0).abs() < 0.5);
        assert!(cells[2].rect.y > cells[0].rect.y, "the third cell wraps");
    }

    #[test]
    fn absolute_positioning_uses_the_nearest_positioned_ancestor() {
        let harness = harness(
            "<div class=outer><div class=abs>x</div></div>",
            "body{margin:0} .outer { position: relative; margin: 20px; height: 200px } \
             .abs { position: absolute; top: 10px; left: 15px; width: 30px; height: 30px }",
        );
        let tree = lay_out(&harness, 400.0, 400.0);
        let abs = box_of(&harness, &tree, ".abs");
        assert_eq!(abs.rect.x, 35.0, "20px outer margin + 15px left");
        assert_eq!(abs.rect.y, 30.0);
        assert_eq!(abs.rect.width, 30.0);
    }

    #[test]
    fn absolute_boxes_do_not_take_space_in_flow() {
        let harness = harness(
            "<div class=abs>x</div><div class=after>y</div>",
            "body{margin:0} .abs { position: absolute; height: 100px } .after { height: 10px }",
        );
        let tree = lay_out(&harness, 400.0, 400.0);
        assert_eq!(box_of(&harness, &tree, ".after").rect.y, 0.0);
    }

    #[test]
    fn right_and_bottom_anchor_to_the_far_edges() {
        let harness = harness(
            "<div class=abs>x</div>",
            "body{margin:0} .abs { position: fixed; right: 10px; bottom: 20px; width: 50px; height: 40px }",
        );
        let tree = lay_out(&harness, 300.0, 200.0);
        let abs = box_of(&harness, &tree, ".abs");
        assert_eq!(abs.rect.max_x(), 290.0);
        assert_eq!(abs.rect.max_y(), 180.0);
    }

    #[test]
    fn relative_offsets_move_the_box_without_moving_siblings() {
        let harness = harness(
            "<div class=a>a</div><div class=b>b</div>",
            "body{margin:0} div { height: 20px } .a { position: relative; top: 5px; left: 7px }",
        );
        let tree = lay_out(&harness, 300.0, 300.0);
        let a = box_of(&harness, &tree, ".a");
        let b = box_of(&harness, &tree, ".b");
        assert_eq!(a.rect.y, 5.0);
        assert_eq!(a.rect.x, 7.0);
        assert_eq!(b.rect.y, 20.0, "the sibling stays where it was");
    }

    #[test]
    fn inline_block_sits_on_the_text_line() {
        let harness = harness(
            "<p>a<span class=b></span>c</p>",
            "body{margin:0} p { font-size: 10px; margin: 0 } \
             .b { display: inline-block; width: 30px; height: 30px }",
        );
        let tree = lay_out(&harness, 300.0, 300.0);
        let b = box_of(&harness, &tree, ".b");
        assert_eq!(b.rect.width, 30.0);
        assert_eq!(b.rect.height, 30.0);
        // It must be offset from the line start by the width of "a".
        assert!((b.rect.x - 5.0).abs() < 0.01, "got {}", b.rect.x);
    }

    #[test]
    fn inline_backgrounds_get_fragment_boxes() {
        let harness = harness(
            "<p>before <span class=s>inside</span> after</p>",
            "body{margin:0} p { font-size: 10px; margin: 0 } .s { background: red }",
        );
        let tree = lay_out(&harness, 500.0, 300.0);
        let fragments: Vec<&LayoutBox> = tree
            .boxes
            .iter()
            .filter(|b| matches!(b.kind, BoxKind::InlineFragment))
            .collect();
        assert_eq!(fragments.len(), 1);
        // "inside" is six glyphs at 5px.
        assert!((fragments[0].rect.width - 30.0).abs() < 0.5);
    }

    #[test]
    fn list_items_get_markers() {
        let harness = harness("<ul><li>one</li><li>two</li></ul>", "");
        let tree = lay_out(&harness, 400.0, 300.0);
        let markers: Vec<&LayoutBox> = tree
            .boxes
            .iter()
            .filter(|b| matches!(b.kind, BoxKind::Marker(_)))
            .collect();
        assert_eq!(markers.len(), 2);
        let items: Vec<&LayoutBox> = tree
            .boxes
            .iter()
            .filter(|b| {
                b.node
                    .and_then(|n| harness.document.element(n))
                    .is_some_and(|el| el.name == "li")
            })
            .collect();
        // Markers sit outside the content box, to its left.
        assert!(markers[0].rect.x < items[0].content_box().x);
    }

    #[test]
    fn ordered_lists_number_their_markers() {
        let harness = harness("<ol><li>a</li><li>b</li></ol>", "");
        let tree = lay_out(&harness, 400.0, 300.0);
        let labels: Vec<String> = tree
            .boxes
            .iter()
            .filter_map(|b| match &b.kind {
                BoxKind::Marker(label) => Some(label.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(labels, vec!["1.", "2."]);
    }

    #[test]
    fn images_use_their_intrinsic_size() {
        struct FixedImage;
        impl ImageProvider for FixedImage {
            fn intrinsic_size(&self, _url: &str) -> Option<Size2D> {
                Some(Size2D::new(200.0, 100.0))
            }
        }

        let harness = harness("<img src=\"a.png\">", "body{margin:0}");
        let ctx = LayoutContext::new(
            &harness.document,
            &harness.styles,
            &harness.fonts,
            &FixedImage,
            Size2D::new(400.0, 300.0),
        );
        let tree = layout_document(&ctx);
        let img = box_of(&harness, &tree, "img");
        assert_eq!(img.rect.width, 200.0);
        assert_eq!(img.rect.height, 100.0);
    }

    #[test]
    fn images_preserve_aspect_ratio_when_only_width_is_given() {
        struct FixedImage;
        impl ImageProvider for FixedImage {
            fn intrinsic_size(&self, _url: &str) -> Option<Size2D> {
                Some(Size2D::new(200.0, 100.0))
            }
        }

        let harness = harness(
            "<img src=\"a.png\" style=\"width:100px\">",
            "body{margin:0}",
        );
        let ctx = LayoutContext::new(
            &harness.document,
            &harness.styles,
            &harness.fonts,
            &FixedImage,
            Size2D::new(400.0, 300.0),
        );
        let tree = layout_document(&ctx);
        let img = box_of(&harness, &tree, "img");
        assert_eq!(img.rect.width, 100.0);
        assert_eq!(img.rect.height, 50.0);
    }

    #[test]
    fn tables_lay_rows_out_as_flex_lines() {
        let harness = harness(
            "<table><tr><td>a</td><td>b</td></tr></table>",
            "body{margin:0} table { width: 200px } td { padding: 0 }",
        );
        let tree = lay_out(&harness, 400.0, 300.0);
        let cells: Vec<&LayoutBox> = tree
            .boxes
            .iter()
            .filter(|b| {
                b.node
                    .and_then(|n| harness.document.element(n))
                    .is_some_and(|el| el.name == "td")
            })
            .collect();
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].rect.y, cells[1].rect.y, "cells share a row");
        assert!(cells[1].rect.x >= cells[0].rect.max_x());
        assert!((cells[0].rect.width + cells[1].rect.width - 200.0).abs() < 1.0);
    }

    #[test]
    fn document_size_grows_with_content() {
        let harness = harness("<div>x</div>", "body{margin:0} div { height: 2000px }");
        let tree = lay_out(&harness, 400.0, 300.0);
        assert!(tree.document_size.height >= 2000.0);
        assert_eq!(tree.document_size.width, 400.0);
    }

    #[test]
    fn document_size_is_at_least_the_viewport() {
        let harness = harness("<div>x</div>", "div { height: 5px }");
        let tree = lay_out(&harness, 400.0, 300.0);
        assert_eq!(tree.document_size.height, 300.0);
    }

    #[test]
    fn empty_document_still_produces_a_root() {
        let harness = harness("", "");
        let tree = lay_out(&harness, 300.0, 200.0);
        assert!(tree.root.is_some());
        assert_eq!(tree.document_size.height, 200.0);
    }

    #[test]
    fn deeply_nested_markup_does_not_blow_up() {
        let mut html = String::new();
        for _ in 0..80 {
            html.push_str("<div>");
        }
        html.push_str("deep");
        for _ in 0..80 {
            html.push_str("</div>");
        }
        let harness = harness(&html, "body{margin:0}");
        let tree = lay_out(&harness, 300.0, 200.0);
        assert!(tree.len() > 80);
    }

    #[test]
    fn min_and_max_width_constrain_the_used_width() {
        let harness = harness(
            "<div class=a>x</div><div class=b>y</div>",
            "body{margin:0} .a { max-width: 100px; height: 5px } .b { min-width: 600px; height: 5px }",
        );
        let tree = lay_out(&harness, 300.0, 300.0);
        assert_eq!(box_of(&harness, &tree, ".a").rect.width, 100.0);
        assert_eq!(box_of(&harness, &tree, ".b").rect.width, 600.0);
    }

    #[test]
    fn overflow_hidden_records_a_scrollable_extent() {
        let harness = harness(
            "<div class=scroll><div class=tall>x</div></div>",
            "body{margin:0} .scroll { height: 50px; overflow-y: auto } .tall { height: 500px }",
        );
        let tree = lay_out(&harness, 300.0, 300.0);
        let scroll = box_of(&harness, &tree, ".scroll");
        assert_eq!(scroll.rect.height, 50.0);
        assert!(scroll.scrollable.height >= 500.0);
        assert!(scroll.scrolls_vertically());
        assert!(scroll.max_scroll_y() >= 450.0);
    }

    #[test]
    fn hit_testing_finds_a_link() {
        let harness = harness(
            "<p><a href=\"/x\">click</a></p>",
            "body{margin:0} p { font-size: 20px; margin: 0 }",
        );
        let tree = lay_out(&harness, 300.0, 300.0);
        let fragment = tree
            .boxes
            .iter()
            .position(|b| matches!(b.kind, BoxKind::InlineFragment))
            .expect("the anchor should produce a fragment");
        let rect = tree.get(fragment).rect;
        let hit = tree.hit_test(rect.center()).expect("something was hit");

        // Links are resolved the way the engine does it: from the hit box's DOM
        // node, walk up the document looking for an anchor.
        let node = tree.get(hit).node.expect("the hit box maps to a node");
        let anchor = std::iter::once(node)
            .chain(harness.document.ancestors(node))
            .find(|candidate| {
                harness
                    .document
                    .element(*candidate)
                    .is_some_and(|el| el.name == "a")
            });
        let anchor = anchor.expect("hit testing should reach the anchor");
        assert_eq!(
            harness.document.element(anchor).unwrap().attr("href"),
            Some("/x")
        );
    }

    #[test]
    fn text_indent_offsets_only_the_first_line() {
        let harness = harness(
            "<p>aaaa bbbb</p>",
            "body{margin:0} p { font-size: 10px; width: 40px; text-indent: 10px; margin: 0 }",
        );
        let tree = lay_out(&harness, 200.0, 300.0);
        let texts: Vec<&LayoutBox> = tree.boxes.iter().filter(|b| b.kind.is_text()).collect();
        assert_eq!(texts.len(), 2);
        assert!((texts[0].rect.x - 10.0).abs() < 0.01);
        assert!((texts[1].rect.x - 0.0).abs() < 0.01);
    }

    #[test]
    fn pre_preserves_newlines() {
        let harness = harness(
            "<pre>one\ntwo\nthree</pre>",
            "body{margin:0} pre { font-size: 10px; line-height: 10px; margin: 0 }",
        );
        let tree = lay_out(&harness, 300.0, 300.0);
        let pre_index = tree
            .box_for_node(harness.document.query("pre").unwrap())
            .unwrap();
        let lines = tree
            .children(pre_index)
            .iter()
            .filter(|c| matches!(tree.get(**c).kind, BoxKind::Line))
            .count();
        assert_eq!(lines, 3);
    }

    #[test]
    fn line_height_sets_the_line_box_height() {
        let harness = harness(
            "<p>one</p>",
            "body{margin:0} p { font-size: 10px; line-height: 40px; margin: 0 }",
        );
        let tree = lay_out(&harness, 300.0, 300.0);
        let p = box_of(&harness, &tree, "p");
        assert_eq!(p.rect.height, 40.0);
    }

    #[test]
    fn input_value_is_rendered_as_text() {
        let harness = harness("<input value=\"hello\">", "body{margin:0}");
        let tree = lay_out(&harness, 300.0, 300.0);
        let text = tree
            .boxes
            .iter()
            .find_map(|b| match &b.kind {
                BoxKind::Text(fragment) => Some(fragment.text.clone()),
                _ => None,
            })
            .expect("the input's value should be laid out");
        assert_eq!(text, "hello");
    }
}
