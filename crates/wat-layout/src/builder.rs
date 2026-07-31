//! Box tree construction: DOM plus computed styles in, unpositioned box tree out.

use std::rc::Rc;

use crate::boxes::{BoxKind, InlineItem, LayoutBox, LayoutTree, Replaced, ReplacedKind};
use crate::geom::Size2D;
use wat_dom::{Document, NodeData, NodeId};
use wat_style::{ComputedStyle, Display, ListStyleType, Size, StyleTree, WhiteSpace};
use wat_text::{collapse_whitespace, FontStore};

/// Supplies intrinsic sizes for replaced content. The engine implements this
/// against its image cache; layout never decodes anything itself.
pub trait ImageProvider {
    /// Intrinsic size for `url`, which is the raw attribute value.
    fn intrinsic_size(&self, url: &str) -> Option<Size2D>;
}

/// An image provider that knows about nothing.
pub struct NoImages;

impl ImageProvider for NoImages {
    fn intrinsic_size(&self, _url: &str) -> Option<Size2D> {
        None
    }
}

/// Everything layout needs besides the tree itself.
pub struct LayoutContext<'a> {
    pub document: &'a Document,
    pub styles: &'a StyleTree,
    pub fonts: &'a FontStore,
    pub images: &'a dyn ImageProvider,
    pub viewport: Size2D,
}

impl<'a> LayoutContext<'a> {
    pub fn new(
        document: &'a Document,
        styles: &'a StyleTree,
        fonts: &'a FontStore,
        images: &'a dyn ImageProvider,
        viewport: Size2D,
    ) -> Self {
        LayoutContext {
            document,
            styles,
            fonts,
            images,
            viewport,
        }
    }
}

/// Elements rendered as replaced content.
fn replaced_kind(name: &str) -> Option<ReplacedKind> {
    match name {
        "img" => Some(ReplacedKind::Image),
        "video" | "canvas" | "iframe" | "embed" | "object" | "audio" | "svg" => {
            Some(ReplacedKind::Unsupported)
        }
        _ => None,
    }
}

/// Form controls that render their value as text inside an inline-block.
fn is_text_control(name: &str) -> bool {
    matches!(name, "input" | "textarea" | "select" | "button")
}

/// The children of a block container, split into block-level boxes and runs of
/// inline content.
enum ChildSpec {
    Block(usize),
    Inline(Vec<InlineItem>),
}

pub struct BoxTreeBuilder<'a, 'ctx> {
    ctx: &'a LayoutContext<'ctx>,
    tree: LayoutTree,
}

impl<'a, 'ctx> BoxTreeBuilder<'a, 'ctx> {
    pub fn new(ctx: &'a LayoutContext<'ctx>) -> Self {
        BoxTreeBuilder {
            ctx,
            tree: LayoutTree::new(ctx.viewport),
        }
    }

    /// Builds the tree, returning it with geometry still unset.
    pub fn build(mut self) -> LayoutTree {
        let document = self.ctx.document;
        // The root box is the `html` element, or a synthetic block if the
        // document has no elements at all.
        let root_node = document
            .find_tag("html")
            .or_else(|| document.element_children(document.root()).next());

        let root = match root_node {
            Some(node) => match self.build_block_box(node) {
                Some(index) => index,
                None => self.push_anonymous_root(),
            },
            None => self.push_anonymous_root(),
        };
        self.tree.root = Some(root);
        self.tree
    }

    fn push_anonymous_root(&mut self) -> usize {
        let mut style = ComputedStyle::initial();
        style.display = Display::Block;
        self.tree
            .push(LayoutBox::new(None, Rc::new(style), BoxKind::Block))
    }

    fn style_of(&self, node: NodeId) -> Rc<ComputedStyle> {
        self.ctx.styles.get(node).clone()
    }

    /// Builds a block-level (or atomic-inline) box for `node` and its subtree.
    fn build_block_box(&mut self, node: NodeId) -> Option<usize> {
        let style = self.style_of(node);
        if style.display.is_none() {
            return None;
        }

        let element_name = self
            .ctx
            .document
            .element(node)
            .map(|el| el.name.clone())
            .unwrap_or_default();

        // Replaced content never has children of its own.
        if let Some(kind) = replaced_kind(&element_name) {
            return Some(self.build_replaced(node, style, kind, &element_name));
        }

        let box_kind = match style.display {
            Display::Flex | Display::InlineFlex | Display::TableRow => BoxKind::Flex,
            Display::Grid => BoxKind::Grid,
            Display::InlineBlock => BoxKind::InlineBlock,
            _ => BoxKind::Block,
        };

        let index = self
            .tree
            .push(LayoutBox::new(Some(node), style.clone(), box_kind));
        self.tree.get_mut(index).out_of_flow = style.position.is_out_of_flow();

        // `list-item` gets a marker box before its content.
        if style.display == Display::ListItem && style.list_style_type != ListStyleType::None {
            let label = marker_label(self.ctx.document, node, &style);
            let marker =
                self.tree
                    .push(LayoutBox::new(None, style.clone(), BoxKind::Marker(label)));
            self.tree.add_child(index, marker);
        }

        if is_text_control(&element_name) {
            self.build_control_content(node, index, &element_name, &style);
            return Some(index);
        }

        self.build_children(node, index, &style);
        Some(index)
    }

    fn build_replaced(
        &mut self,
        node: NodeId,
        style: Rc<ComputedStyle>,
        kind: ReplacedKind,
        element_name: &str,
    ) -> usize {
        let element = self.ctx.document.element(node);
        let url = element
            .and_then(|el| el.attr("src").or_else(|| el.attr("data")))
            .map(str::to_string)
            .filter(|u| !u.trim().is_empty());
        let label = element
            .and_then(|el| el.attr("alt"))
            .map(str::to_string)
            .filter(|alt| !alt.trim().is_empty())
            .unwrap_or_else(|| element_name.to_string());
        let intrinsic = url
            .as_deref()
            .and_then(|url| self.ctx.images.intrinsic_size(url));

        let replaced = Replaced {
            kind,
            url,
            label,
            intrinsic,
        };
        let index = self.tree.push(LayoutBox::new(
            Some(node),
            style.clone(),
            BoxKind::Replaced(replaced),
        ));
        self.tree.get_mut(index).out_of_flow = style.position.is_out_of_flow();
        index
    }

    /// `<input>`, `<button>` and friends render their value or label.
    fn build_control_content(
        &mut self,
        node: NodeId,
        index: usize,
        element_name: &str,
        style: &Rc<ComputedStyle>,
    ) {
        let element = self.ctx.document.element(node);
        let input_type = element
            .and_then(|el| el.attr("type"))
            .unwrap_or("text")
            .to_ascii_lowercase();

        let text = match element_name {
            "input" => match input_type.as_str() {
                // Checkboxes and radios draw as boxes, with no text.
                "checkbox" | "radio" | "hidden" | "range" | "color" | "file" => String::new(),
                "submit" | "button" | "reset" => element
                    .and_then(|el| el.attr("value"))
                    .unwrap_or(match input_type.as_str() {
                        "submit" => "Submit",
                        "reset" => "Reset",
                        _ => "",
                    })
                    .to_string(),
                "password" => {
                    let value = element.and_then(|el| el.attr("value")).unwrap_or_default();
                    "•".repeat(value.chars().count())
                }
                _ => element
                    .and_then(|el| el.attr("value").or_else(|| el.attr("placeholder")))
                    .unwrap_or_default()
                    .to_string(),
            },
            "select" => {
                // Show the selected option, or the first one.
                let options = self.ctx.document.query_all("option");
                let selected = options
                    .iter()
                    .find(|option| {
                        self.ctx
                            .document
                            .element(**option)
                            .is_some_and(|el| el.has_attr("selected"))
                            && self.ctx.document.ancestors(**option).any(|a| a == node)
                    })
                    .or_else(|| {
                        options
                            .iter()
                            .find(|option| self.ctx.document.ancestors(**option).any(|a| a == node))
                    });
                selected
                    .map(|option| self.ctx.document.text_content(*option).trim().to_string())
                    .unwrap_or_default()
            }
            // `button` and `textarea` use their element content.
            _ => self.ctx.document.text_content(node),
        };

        let processed = process_text(&text, style.white_space);
        if processed.trim().is_empty() {
            return;
        }
        self.tree.get_mut(index).kind = match self.tree.get(index).kind {
            BoxKind::Block => BoxKind::Block,
            ref other => other.clone(),
        };
        let items = vec![InlineItem::Text {
            node,
            style: style.clone(),
            text: processed,
        }];
        self.set_inline_items(index, items);
    }

    /// Builds the children of a block container, inserting anonymous blocks
    /// where block-level and inline-level siblings mix.
    fn build_children(&mut self, node: NodeId, index: usize, parent_style: &Rc<ComputedStyle>) {
        let mut specs: Vec<ChildSpec> = Vec::new();
        self.collect_children(node, parent_style, &mut specs);

        let has_block = specs.iter().any(|spec| matches!(spec, ChildSpec::Block(_)));
        // Flex and grid containers have no inline formatting context: every
        // child becomes an item, wrapped in an anonymous box if it is text.
        let force_items = establishes_item_context(parent_style.display);

        if !has_block && !force_items {
            // A pure inline formatting context: the container owns the items.
            let mut items = Vec::new();
            for spec in specs {
                if let ChildSpec::Inline(mut run) = spec {
                    items.append(&mut run);
                }
            }
            if !items.is_empty() {
                self.set_inline_items(index, items);
            }
            return;
        }

        for spec in specs {
            match spec {
                ChildSpec::Block(child) => self.tree.add_child(index, child),
                ChildSpec::Inline(items) => {
                    if items_are_blank(&items) {
                        continue;
                    }
                    let anonymous = self.tree.push(LayoutBox::new(
                        None,
                        Rc::new(ComputedStyle::inherit_from(parent_style)),
                        BoxKind::AnonymousBlock,
                    ));
                    self.set_inline_items(anonymous, items);
                    self.tree.add_child(index, anonymous);
                }
            }
        }
    }

    /// Tweaks an item's style for the container it lands in.
    fn adjust_item_style(
        &mut self,
        index: usize,
        parent_style: &Rc<ComputedStyle>,
        style: &Rc<ComputedStyle>,
    ) {
        // Table cells share their row's width evenly unless one is sized.
        if parent_style.display == Display::TableRow && style.display == Display::TableCell {
            let mut cell_style = (**style).clone();
            if cell_style.flex_grow == 0.0 {
                cell_style.flex_grow = 1.0;
            }
            if cell_style.width.is_auto() {
                cell_style.flex_basis = Size::Auto;
            }
            self.tree.get_mut(index).style = Rc::new(cell_style);
        }
    }

    fn set_inline_items(&mut self, index: usize, items: Vec<InlineItem>) {
        // Atomic inlines already live in the tree; parent them to their
        // formatting context so the tree stays connected before layout runs.
        for item in &items {
            if let InlineItem::Atomic(child) = item {
                self.tree.get_mut(*child).parent = Some(index);
            }
        }
        self.tree.inline_items.insert(index, items);
    }

    /// Walks the DOM children of `node`, classifying each.
    fn collect_children(
        &mut self,
        node: NodeId,
        parent_style: &Rc<ComputedStyle>,
        specs: &mut Vec<ChildSpec>,
    ) {
        let children: Vec<NodeId> = self.ctx.document.children(node).collect();
        for child in children {
            match self.ctx.document.data(child).clone() {
                NodeData::Text(text) => {
                    let style = self.style_of(child);
                    let processed = process_text(&text, style.white_space);
                    if processed.is_empty() {
                        continue;
                    }
                    let item = InlineItem::Text {
                        node: child,
                        style,
                        text: processed,
                    };
                    push_inline(specs, item);
                }
                NodeData::Element(element) => {
                    let style = self.style_of(child);
                    if style.display.is_none() {
                        continue;
                    }
                    if element.name == "br" {
                        push_inline(specs, InlineItem::Break);
                        continue;
                    }
                    // `display: contents` splices the element's children in.
                    if style.display == Display::Contents {
                        self.collect_children(child, parent_style, specs);
                        continue;
                    }
                    // Out-of-flow boxes are block-level regardless of `display`.
                    if style.position.is_out_of_flow() {
                        if let Some(index) = self.build_block_box(child) {
                            specs.push(ChildSpec::Block(index));
                        }
                        continue;
                    }
                    if let Some(kind) = replaced_kind(&element.name) {
                        let index = self.build_replaced(child, style, kind, &element.name);
                        push_inline(specs, InlineItem::Atomic(index));
                        continue;
                    }
                    // A flex or grid item is blockified, whatever `display` says.
                    if establishes_item_context(parent_style.display) {
                        if let Some(index) = self.build_block_box(child) {
                            self.adjust_item_style(index, parent_style, &style);
                            specs.push(ChildSpec::Block(index));
                        }
                        continue;
                    }
                    if style.display.is_inline_level() {
                        if style.display == Display::Inline && !is_text_control(&element.name) {
                            // A plain inline box contributes open/close markers
                            // around its own inline content.
                            push_inline(
                                specs,
                                InlineItem::Open {
                                    node: Some(child),
                                    style: style.clone(),
                                },
                            );
                            self.collect_children(child, &style, specs);
                            push_inline(specs, InlineItem::Close);
                        } else if let Some(index) = self.build_block_box(child) {
                            push_inline(specs, InlineItem::Atomic(index));
                        }
                        continue;
                    }
                    if let Some(index) = self.build_block_box(child) {
                        specs.push(ChildSpec::Block(index));
                    }
                }
                NodeData::Comment(_) | NodeData::Doctype { .. } | NodeData::Document => {}
            }
        }
    }
}

/// Does a container turn its children into items rather than inline content?
fn establishes_item_context(display: Display) -> bool {
    matches!(
        display,
        Display::Flex | Display::InlineFlex | Display::Grid | Display::TableRow
    )
}

/// Appends `item` to the trailing inline run, starting one if needed.
fn push_inline(specs: &mut Vec<ChildSpec>, item: InlineItem) {
    match specs.last_mut() {
        Some(ChildSpec::Inline(run)) => run.push(item),
        _ => specs.push(ChildSpec::Inline(vec![item])),
    }
}

/// True when a run carries no drawable content.
fn items_are_blank(items: &[InlineItem]) -> bool {
    items.iter().all(|item| match item {
        InlineItem::Text { text, .. } => text.trim().is_empty(),
        InlineItem::Open { .. } | InlineItem::Close => true,
        _ => false,
    })
}

/// Applies white-space processing to a text node's contents.
pub fn process_text(text: &str, white_space: WhiteSpace) -> String {
    match white_space {
        WhiteSpace::Pre | WhiteSpace::PreWrap => text.to_string(),
        WhiteSpace::PreLine => {
            // Each whitespace run collapses to one character: a newline if the
            // run contained one, otherwise a space.
            let mut out = String::with_capacity(text.len());
            let mut run: Option<bool> = None;
            for ch in text.chars() {
                if ch.is_whitespace() {
                    run = Some(run.unwrap_or(false) || ch == '\n');
                    continue;
                }
                if let Some(had_newline) = run.take() {
                    out.push(if had_newline { '\n' } else { ' ' });
                }
                out.push(ch);
            }
            if let Some(had_newline) = run {
                out.push(if had_newline { '\n' } else { ' ' });
            }
            out
        }
        WhiteSpace::Normal | WhiteSpace::Nowrap => collapse_whitespace(text),
    }
}

/// The text of a list item's marker.
fn marker_label(document: &Document, node: NodeId, style: &ComputedStyle) -> String {
    match style.list_style_type {
        ListStyleType::None => String::new(),
        ListStyleType::Disc => "•".to_string(),
        ListStyleType::Circle => "◦".to_string(),
        ListStyleType::Square => "▪".to_string(),
        kind => {
            let index = list_index(document, node);
            match kind {
                ListStyleType::Decimal => format!("{index}."),
                ListStyleType::LowerAlpha => format!("{}.", alphabetic(index, false)),
                ListStyleType::UpperAlpha => format!("{}.", alphabetic(index, true)),
                ListStyleType::LowerRoman => format!("{}.", roman(index).to_lowercase()),
                ListStyleType::UpperRoman => format!("{}.", roman(index)),
                _ => "•".to_string(),
            }
        }
    }
}

/// 1-based position of `node` among its list-item siblings, honouring `start`
/// and `value` attributes.
fn list_index(document: &Document, node: NodeId) -> u32 {
    let Some(parent) = document.node(node).parent else {
        return 1;
    };
    let start = document
        .element(parent)
        .and_then(|el| el.attr("start"))
        .and_then(|s| s.trim().parse::<i64>().ok())
        .unwrap_or(1);

    let mut counter = start;
    for sibling in document.element_children(parent) {
        if let Some(value) = document
            .element(sibling)
            .and_then(|el| el.attr("value"))
            .and_then(|v| v.trim().parse::<i64>().ok())
        {
            counter = value;
        }
        if sibling == node {
            break;
        }
        counter += 1;
    }
    counter.max(1) as u32
}

fn alphabetic(index: u32, upper: bool) -> String {
    let mut remaining = index.max(1);
    let mut out = Vec::new();
    while remaining > 0 {
        let digit = ((remaining - 1) % 26) as u8;
        out.push(if upper { b'A' + digit } else { b'a' + digit });
        remaining = (remaining - 1) / 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

fn roman(index: u32) -> String {
    const TABLE: &[(u32, &str)] = &[
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut remaining = index.max(1);
    let mut out = String::new();
    for (value, numeral) in TABLE {
        while remaining >= *value {
            out.push_str(numeral);
            remaining -= value;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_space_processing_modes() {
        assert_eq!(process_text("a   b", WhiteSpace::Normal), "a b");
        assert_eq!(process_text("a\n b", WhiteSpace::Normal), "a b");
        assert_eq!(process_text("a\n b", WhiteSpace::Pre), "a\n b");
        assert_eq!(process_text("a  \n  b", WhiteSpace::PreLine), "a\nb");
        assert_eq!(process_text("a   b", WhiteSpace::Nowrap), "a b");
    }

    #[test]
    fn alphabetic_markers() {
        assert_eq!(alphabetic(1, false), "a");
        assert_eq!(alphabetic(26, false), "z");
        assert_eq!(alphabetic(27, false), "aa");
        assert_eq!(alphabetic(1, true), "A");
    }

    #[test]
    fn roman_markers() {
        assert_eq!(roman(1), "I");
        assert_eq!(roman(4), "IV");
        assert_eq!(roman(9), "IX");
        assert_eq!(roman(14), "XIV");
        assert_eq!(roman(1987), "MCMLXXXVII");
    }
}
