//! Inline layout: turning a stream of inline items into line boxes.

use std::rc::Rc;

use crate::block::layout_in_flow_box;
use crate::boxes::{font_request, BoxKind, InlineItem, LayoutBox, LayoutTree, TextFragment};
use crate::builder::LayoutContext;
use crate::geom::{Point, Rect, Size2D};
use wat_style::{ComputedStyle, Sides, TextAlign, TextTransform, VerticalAlign};
use wat_text::segment_words;

/// An inline element that is open while pieces are placed.
struct InlineInstance {
    node: Option<wat_dom::NodeId>,
    style: Rc<ComputedStyle>,
}

enum PieceContent {
    Text {
        node: wat_dom::NodeId,
        text: String,
        /// Number of collapsible spaces, for justification.
        spaces: usize,
    },
    /// An atomic inline that already exists in the tree.
    Atomic(usize),
}

/// One placed item on the current line.
struct Piece {
    content: PieceContent,
    style: Rc<ComputedStyle>,
    x: f32,
    width: f32,
    /// Height above the baseline.
    ascent: f32,
    /// Depth below the baseline.
    descent: f32,
    /// Positive moves the piece up.
    baseline_shift: f32,
    /// Inline elements enclosing this piece, outermost first.
    open: Vec<usize>,
}

/// A collapsible space held back until we know whether content follows it.
struct PendingSpace {
    style: Rc<ComputedStyle>,
    node: wat_dom::NodeId,
    width: f32,
    open: Vec<usize>,
}

/// State carried across lines while an inline context is laid out.
struct InlineLayout<'a, 'ctx> {
    ctx: &'a LayoutContext<'ctx>,
    /// Content-box origin of the containing block.
    origin: Point,
    available: f32,
    /// Style of the block container, used for the line strut and alignment.
    container: Rc<ComputedStyle>,

    instances: Vec<InlineInstance>,
    open: Vec<usize>,

    pieces: Vec<Piece>,
    pen: f32,
    /// Trailing collapsible space that only materialises if more text follows.
    /// It keeps the inline nesting it was written in, so a space before
    /// `<span>` does not end up inside the span's fragment.
    pending_space: Option<PendingSpace>,
    /// Lines produced so far, as (line box index).
    lines: Vec<usize>,
    y: f32,
    max_line_width: f32,
    /// `text-indent` applies to the first line only.
    indent: f32,
}

/// Lays out the inline content of `index`, returning the content size.
pub(crate) fn layout_inline_content(
    tree: &mut LayoutTree,
    ctx: &LayoutContext,
    index: usize,
    origin: Point,
    available: f32,
) -> Size2D {
    let Some(items) = tree.inline_items.get(&index).cloned() else {
        return Size2D::ZERO;
    };
    let container = tree.get(index).style.clone();
    let indent = container.text_indent.resolve(available);

    let mut state = InlineLayout {
        ctx,
        origin,
        available: available.max(0.0),
        container,
        instances: Vec::new(),
        open: Vec::new(),
        pieces: Vec::new(),
        pen: 0.0,
        pending_space: None,
        lines: Vec::new(),
        y: origin.y,
        max_line_width: 0.0,
        indent,
    };
    state.pen = indent;

    for item in &items {
        match item {
            InlineItem::Open { node, style } => {
                state.instances.push(InlineInstance {
                    node: *node,
                    style: style.clone(),
                });
                let instance = state.instances.len() - 1;
                state.open.push(instance);
                // Opening edge: left border and padding push the pen along.
                let edge = style.used_border_width().left + style.padding.left.resolve(available);
                state.pen += edge;
            }
            InlineItem::Close => {
                if let Some(instance) = state.open.pop() {
                    let style = state.instances[instance].style.clone();
                    let edge =
                        style.used_border_width().right + style.padding.right.resolve(available);
                    state.pen += edge;
                }
            }
            InlineItem::Break => {
                state.pending_space = None;
                state.finish_line(tree, true);
            }
            InlineItem::Atomic(child) => state.place_atomic(tree, *child),
            InlineItem::Text { node, style, text } => state.place_text(tree, *node, style, text),
        }
    }
    state.pending_space = None;
    state.finish_line(tree, false);

    let lines = std::mem::take(&mut state.lines);
    let height = state.y - origin.y;
    let width = state.max_line_width;
    for line in lines {
        tree.add_child(index, line);
    }

    Size2D::new(width, height)
}

impl InlineLayout<'_, '_> {
    /// Width already used on the current line.
    fn used(&self) -> f32 {
        self.pen
    }

    fn line_is_empty(&self) -> bool {
        self.pieces.is_empty()
    }

    fn text_metrics(&self, style: &ComputedStyle) -> (f32, f32) {
        let request = font_request(style);
        let metrics = self.ctx.fonts.line_metrics(&request);
        let line_height = style.line_height_px();
        let content_height = metrics.ascent + metrics.descent;
        let leading = line_height - content_height;
        (
            metrics.ascent + leading / 2.0,
            metrics.descent + leading / 2.0,
        )
    }

    fn baseline_shift(&self, style: &ComputedStyle) -> f32 {
        match style.vertical_align {
            VerticalAlign::Sub => -style.font_size * 0.2,
            VerticalAlign::Super => style.font_size * 0.33,
            _ => 0.0,
        }
    }

    fn place_text(
        &mut self,
        tree: &mut LayoutTree,
        node: wat_dom::NodeId,
        style: &Rc<ComputedStyle>,
        text: &str,
    ) {
        let transformed = apply_text_transform(text, style.text_transform);
        let request = font_request(style);
        let (ascent, descent) = self.text_metrics(style);
        let shift = self.baseline_shift(style);
        let preserve_spaces = style.white_space.preserves_spaces();
        let wraps = style.white_space.wraps();

        for (segment, is_whitespace) in segment_words(&transformed) {
            // A preserved newline breaks the line; a collapsed one is already
            // a space by this point.
            if segment == "\n" && style.white_space.preserves_newlines() {
                self.pending_space = None;
                self.finish_line(tree, true);
                continue;
            }
            let width = self.ctx.fonts.measure(&request, segment);

            if is_whitespace && !preserve_spaces {
                // Collapsible whitespace: hold it until we know more follows.
                if !self.line_is_empty() {
                    self.pending_space = Some(PendingSpace {
                        style: style.clone(),
                        node,
                        width,
                        open: self.open.clone(),
                    });
                }
                continue;
            }

            let pending_width = self.pending_space.as_ref().map_or(0.0, |s| s.width);
            if wraps
                && !self.line_is_empty()
                && self.used() + pending_width + width > self.available + 0.01
            {
                // Break before this word; the pending space disappears.
                self.pending_space = None;
                self.finish_line(tree, false);
            } else {
                self.flush_pending_space();
            }

            if is_whitespace && preserve_spaces {
                let spaces = segment.chars().filter(|c| *c == ' ').count();
                let open = self.open.clone();
                self.append_text(node, style, segment, width, spaces, open);
                continue;
            }
            let open = self.open.clone();
            self.append_text_with_metrics(
                node, style, segment, width, 0, ascent, descent, shift, open,
            );
        }
    }

    /// Emits the held-back space, if there is one, in its own nesting.
    fn flush_pending_space(&mut self) {
        let Some(space) = self.pending_space.take() else {
            return;
        };
        self.append_text(
            space.node,
            &space.style.clone(),
            " ",
            space.width,
            1,
            space.open,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn append_text_with_metrics(
        &mut self,
        node: wat_dom::NodeId,
        style: &Rc<ComputedStyle>,
        text: &str,
        width: f32,
        spaces: usize,
        ascent: f32,
        descent: f32,
        shift: f32,
        open: Vec<usize>,
    ) {
        // Merge into the previous piece when nothing changed between them.
        if let Some(last) = self.pieces.last_mut() {
            if last.open == open
                && Rc::ptr_eq(&last.style, style)
                && (last.x + last.width - self.pen).abs() < 0.01
            {
                if let PieceContent::Text {
                    text: existing,
                    spaces: existing_spaces,
                    ..
                } = &mut last.content
                {
                    existing.push_str(text);
                    *existing_spaces += spaces;
                    last.width += width;
                    self.pen += width;
                    return;
                }
            }
        }
        self.pieces.push(Piece {
            content: PieceContent::Text {
                node,
                text: text.to_string(),
                spaces,
            },
            style: style.clone(),
            x: self.pen,
            width,
            ascent,
            descent,
            baseline_shift: shift,
            open,
        });
        self.pen += width;
    }

    fn append_text(
        &mut self,
        node: wat_dom::NodeId,
        style: &Rc<ComputedStyle>,
        text: &str,
        width: f32,
        spaces: usize,
        open: Vec<usize>,
    ) {
        let (ascent, descent) = self.text_metrics(style);
        let shift = self.baseline_shift(style);
        self.append_text_with_metrics(
            node, style, text, width, spaces, ascent, descent, shift, open,
        );
    }

    fn place_atomic(&mut self, tree: &mut LayoutTree, child: usize) {
        // Lay the atomic box out at a provisional position; it moves once the
        // line's baseline is known.
        let remaining = (self.available - self.used()).max(0.0);
        layout_in_flow_box(
            tree,
            self.ctx,
            child,
            Point::ZERO,
            remaining,
            self.available,
        );

        let margin_box = tree.get(child).margin_box();
        let width = margin_box.width;
        let pending_width = self.pending_space.as_ref().map_or(0.0, |s| s.width);
        if tree.get(child).style.white_space.wraps()
            && !self.line_is_empty()
            && self.used() + pending_width + width > self.available + 0.01
        {
            self.pending_space = None;
            self.finish_line(tree, false);
        } else {
            self.flush_pending_space();
        }

        let style = tree.get(child).style.clone();
        // The baseline of an atomic inline sits on its bottom margin edge,
        // unless `vertical-align` says otherwise.
        let height = margin_box.height;
        let (ascent, descent) = match style.vertical_align {
            VerticalAlign::Middle => (height / 2.0, height / 2.0),
            VerticalAlign::Top | VerticalAlign::TextTop => (height, 0.0),
            VerticalAlign::Bottom | VerticalAlign::TextBottom => (height, 0.0),
            _ => {
                let baseline = tree.get(child).baseline;
                if baseline > 0.0 && baseline <= height {
                    (baseline, height - baseline)
                } else {
                    (height, 0.0)
                }
            }
        };

        self.pieces.push(Piece {
            content: PieceContent::Atomic(child),
            style,
            x: self.pen,
            width,
            ascent,
            descent,
            baseline_shift: 0.0,
            open: self.open.clone(),
        });
        self.pen += width;
    }

    /// Closes the current line, positioning every piece on it.
    fn finish_line(&mut self, tree: &mut LayoutTree, forced: bool) {
        if self.pieces.is_empty() && !forced {
            self.pen = 0.0;
            return;
        }

        // The strut guarantees an empty line still occupies its line height.
        let (strut_ascent, strut_descent) = self.text_metrics(&self.container);
        let mut ascent = strut_ascent;
        let mut descent = strut_descent;
        for piece in &self.pieces {
            ascent = ascent.max(piece.ascent + piece.baseline_shift);
            descent = descent.max(piece.descent - piece.baseline_shift);
        }
        let line_height = ascent + descent;
        let line_width = self.pieces.last().map_or(0.0, |p| p.x + p.width);

        // Horizontal alignment.
        let leftover = (self.available - line_width).max(0.0);
        let is_last_line = !forced;
        let (offset, justify_extra) = match self.container.text_align {
            TextAlign::Center => (leftover / 2.0, 0.0),
            TextAlign::Right | TextAlign::End => (leftover, 0.0),
            TextAlign::Justify if !is_last_line => {
                let spaces: usize = self
                    .pieces
                    .iter()
                    .map(|p| match &p.content {
                        PieceContent::Text { spaces, .. } => *spaces,
                        _ => 0,
                    })
                    .sum();
                if spaces > 0 {
                    (0.0, leftover / spaces as f32)
                } else {
                    (0.0, 0.0)
                }
            }
            _ => (0.0, 0.0),
        };

        let line_index = {
            let mut line = LayoutBox::new(None, self.container.clone(), BoxKind::Line);
            line.rect = Rect::new(self.origin.x, self.y, self.available, line_height);
            line.baseline = ascent;
            tree.push(line)
        };

        let pieces = std::mem::take(&mut self.pieces);

        // Inline element fragments paint below the text on the line, so they are
        // added as children first.
        let mut fragment_rects: Vec<(usize, Rect)> = Vec::new();
        let mut running_extra = 0.0f32;
        let mut piece_rects: Vec<(usize, Rect)> = Vec::with_capacity(pieces.len());
        for (piece_index, piece) in pieces.iter().enumerate() {
            let extra_before = running_extra;
            let spaces = match &piece.content {
                PieceContent::Text { spaces, .. } => *spaces,
                _ => 0,
            };
            running_extra += justify_extra * spaces as f32;

            let x = self.origin.x + offset + piece.x + extra_before;
            let width = piece.width + justify_extra * spaces as f32;
            let top = self.y + ascent - piece.ascent - piece.baseline_shift;
            let rect = Rect::new(x, top, width, piece.ascent + piece.descent);
            piece_rects.push((piece_index, rect));

            for instance in &piece.open {
                match fragment_rects.iter_mut().find(|(i, _)| i == instance) {
                    Some((_, existing)) => *existing = existing.union(&rect),
                    None => fragment_rects.push((*instance, rect)),
                }
            }
        }

        for (instance, rect) in fragment_rects {
            let style = self.instances[instance].style.clone();
            let node = self.instances[instance].node;
            let border = style.used_border_width();
            let padding = Sides {
                top: style.padding.top.resolve(self.available),
                right: style.padding.right.resolve(self.available),
                bottom: style.padding.bottom.resolve(self.available),
                left: style.padding.left.resolve(self.available),
            };
            let mut fragment = LayoutBox::new(node, style, BoxKind::InlineFragment);
            // Padding and border grow the fragment outwards from the text.
            fragment.rect = Rect::new(
                rect.x - padding.left - border.left,
                rect.y - padding.top - border.top,
                rect.width + padding.horizontal() + border.horizontal(),
                rect.height + padding.vertical() + border.vertical(),
            );
            fragment.padding = padding;
            fragment.border = border;
            fragment.baseline = rect.y + rect.height - fragment.rect.y;
            let fragment_index = tree.push(fragment);
            tree.add_child(line_index, fragment_index);
        }

        for (piece_index, rect) in piece_rects {
            let piece = &pieces[piece_index];
            match &piece.content {
                PieceContent::Text { node, text, spaces } => {
                    let extra = if *spaces > 0 { justify_extra } else { 0.0 };
                    let mut fragment_box = LayoutBox::new(
                        Some(*node),
                        piece.style.clone(),
                        BoxKind::Text(TextFragment {
                            text: text.clone(),
                            baseline: piece.ascent,
                            extra_word_spacing: extra,
                        }),
                    );
                    fragment_box.rect = rect;
                    fragment_box.baseline = piece.ascent;
                    let index = tree.push(fragment_box);
                    tree.add_child(line_index, index);
                }
                PieceContent::Atomic(child) => {
                    let margin = tree.get(*child).margin;
                    let delta_x = rect.x + margin.left - tree.get(*child).rect.x;
                    let delta_y = rect.y + margin.top - tree.get(*child).rect.y;
                    crate::block::translate_subtree(tree, *child, delta_x, delta_y);
                    tree.add_child(line_index, *child);
                }
            }
        }

        self.max_line_width = self.max_line_width.max(line_width + running_extra);
        self.y += line_height;
        self.lines.push(line_index);
        self.pen = 0.0;
        // `text-indent` only affects the first line.
        self.indent = 0.0;
    }
}

fn apply_text_transform(text: &str, transform: TextTransform) -> String {
    match transform {
        TextTransform::None => text.to_string(),
        TextTransform::Uppercase => text.to_uppercase(),
        TextTransform::Lowercase => text.to_lowercase(),
        TextTransform::Capitalize => {
            let mut out = String::with_capacity(text.len());
            let mut at_word_start = true;
            for ch in text.chars() {
                if at_word_start && ch.is_alphabetic() {
                    out.extend(ch.to_uppercase());
                    at_word_start = false;
                } else {
                    out.push(ch);
                    if ch.is_whitespace() {
                        at_word_start = true;
                    }
                }
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_transforms() {
        assert_eq!(apply_text_transform("abc", TextTransform::Uppercase), "ABC");
        assert_eq!(apply_text_transform("ABC", TextTransform::Lowercase), "abc");
        assert_eq!(
            apply_text_transform("hello wide world", TextTransform::Capitalize),
            "Hello Wide World"
        );
        assert_eq!(apply_text_transform("keep", TextTransform::None), "keep");
    }
}
