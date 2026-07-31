//! The cascade: turning stylesheets plus a document into computed styles.

use std::rc::Rc;

use crate::apply::apply_declaration;
use crate::types::{ComputedStyle, Display};
use wat_css::values::LengthContext;
use wat_css::{Declaration, MatchContext, MediaContext, Origin, Specificity, Stylesheet};
use wat_dom::{Document, NodeId};

/// The default stylesheet, applied before user and author rules.
pub const UA_STYLESHEET: &str = include_str!("ua.css");

/// Cascade precedence buckets, lowest first.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Bucket {
    UaNormal,
    UserNormal,
    AuthorNormal,
    AuthorImportant,
    UserImportant,
    UaImportant,
}

fn bucket_for(origin: Origin, important: bool) -> Bucket {
    match (origin, important) {
        (Origin::UserAgent, false) => Bucket::UaNormal,
        (Origin::User, false) => Bucket::UserNormal,
        (Origin::Author, false) => Bucket::AuthorNormal,
        (Origin::Author, true) => Bucket::AuthorImportant,
        (Origin::User, true) => Bucket::UserImportant,
        (Origin::UserAgent, true) => Bucket::UaImportant,
    }
}

/// A declaration that matched, with everything needed to order it.
struct Candidate<'a> {
    bucket: Bucket,
    /// Inline styles beat any selector in the same bucket.
    inline: bool,
    specificity: Specificity,
    order: usize,
    declaration: &'a Declaration,
}

/// Computed styles for every node in a document.
#[derive(Clone, Debug)]
pub struct StyleTree {
    styles: Vec<Rc<ComputedStyle>>,
    /// Style used for nodes that generate no box.
    fallback: Rc<ComputedStyle>,
}

impl Default for StyleTree {
    fn default() -> Self {
        StyleTree::empty()
    }
}

impl StyleTree {
    /// A tree with no styles in it; every lookup returns the initial style.
    pub fn empty() -> Self {
        StyleTree {
            styles: Vec::new(),
            fallback: Rc::new(ComputedStyle::initial()),
        }
    }

    /// The computed style of any node. Text nodes report their parent's style.
    pub fn get(&self, node: NodeId) -> &Rc<ComputedStyle> {
        self.styles.get(node.index()).unwrap_or(&self.fallback)
    }

    pub fn len(&self) -> usize {
        self.styles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.styles.is_empty()
    }
}

/// Holds the stylesheets for one document and computes styles from them.
#[derive(Clone, Debug)]
pub struct StyleEngine {
    pub ua: Stylesheet,
    pub user: Vec<Stylesheet>,
    pub author: Vec<Stylesheet>,
}

impl Default for StyleEngine {
    fn default() -> Self {
        StyleEngine::new()
    }
}

impl StyleEngine {
    /// A style engine with only the default stylesheet loaded.
    pub fn new() -> Self {
        StyleEngine {
            ua: Stylesheet::parse(UA_STYLESHEET, Origin::UserAgent),
            user: Vec::new(),
            author: Vec::new(),
        }
    }

    pub fn add_author_sheet(&mut self, sheet: Stylesheet) {
        self.author.push(sheet);
    }

    pub fn add_user_sheet(&mut self, sheet: Stylesheet) {
        self.user.push(sheet);
    }

    pub fn clear_author_sheets(&mut self) {
        self.author.clear();
    }

    /// Computes styles for the whole document.
    pub fn compute(
        &self,
        document: &Document,
        media: &MediaContext,
        match_ctx: &MatchContext,
    ) -> StyleTree {
        // Flatten the sheets into (origin, rule) pairs in cascade order once,
        // rather than per element.
        let mut rules: Vec<(Origin, &wat_css::StyleRule)> = Vec::new();
        for rule in self.ua.style_rules(media) {
            rules.push((Origin::UserAgent, rule));
        }
        for sheet in &self.user {
            for rule in sheet.style_rules(media) {
                rules.push((Origin::User, rule));
            }
        }
        for sheet in &self.author {
            for rule in sheet.style_rules(media) {
                rules.push((Origin::Author, rule));
            }
        }

        let initial = Rc::new(ComputedStyle::initial());
        let mut styles: Vec<Rc<ComputedStyle>> = vec![initial.clone(); document.len()];

        // The root font size drives `rem`. Resolve the root first, then reuse it.
        let root_font_size = document
            .find_tag("html")
            .map(|html| {
                self.compute_one(document, html, &initial, &rules, media, match_ctx, 16.0)
                    .font_size
            })
            .unwrap_or(16.0);

        let root = document.root();
        self.compute_subtree(
            document,
            root,
            &initial,
            &rules,
            media,
            match_ctx,
            root_font_size,
            &mut styles,
        );

        StyleTree {
            styles,
            fallback: initial,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn compute_subtree(
        &self,
        document: &Document,
        node: NodeId,
        parent_style: &Rc<ComputedStyle>,
        rules: &[(Origin, &wat_css::StyleRule)],
        media: &MediaContext,
        match_ctx: &MatchContext,
        root_font_size: f32,
        styles: &mut Vec<Rc<ComputedStyle>>,
    ) {
        let style = if document.node(node).is_element() {
            Rc::new(self.compute_one(
                document,
                node,
                parent_style,
                rules,
                media,
                match_ctx,
                root_font_size,
            ))
        } else {
            // Text and comments simply take their parent's inherited style.
            parent_style.clone()
        };
        styles[node.index()] = style.clone();

        for child in document.children(node).collect::<Vec<_>>() {
            self.compute_subtree(
                document,
                child,
                &style,
                rules,
                media,
                match_ctx,
                root_font_size,
                styles,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn compute_one(
        &self,
        document: &Document,
        node: NodeId,
        parent_style: &ComputedStyle,
        rules: &[(Origin, &wat_css::StyleRule)],
        media: &MediaContext,
        match_ctx: &MatchContext,
        root_font_size: f32,
    ) -> ComputedStyle {
        let mut candidates: Vec<Candidate> = Vec::new();
        let mut order = 0usize;

        // Presentational attributes act as low-priority UA declarations.
        let hints = presentational_hints(document, node);
        for declaration in &hints {
            candidates.push(Candidate {
                bucket: Bucket::UaNormal,
                inline: false,
                specificity: Specificity::default(),
                order,
                declaration,
            });
            order += 1;
        }

        for (origin, rule) in rules {
            // The most specific matching selector in a rule sets its weight.
            let best = rule
                .selectors
                .iter()
                .filter(|selector| selector.matches(document, node, match_ctx))
                .map(|selector| selector.specificity)
                .max();
            let Some(specificity) = best else {
                order += rule.declarations.len();
                continue;
            };
            for declaration in &rule.declarations {
                candidates.push(Candidate {
                    bucket: bucket_for(*origin, declaration.important),
                    inline: false,
                    specificity,
                    order,
                    declaration,
                });
                order += 1;
            }
        }

        let inline = document
            .element(node)
            .and_then(|el| el.attr("style"))
            .map(wat_css::parse_inline_style)
            .unwrap_or_default();
        for declaration in &inline {
            candidates.push(Candidate {
                bucket: bucket_for(Origin::Author, declaration.important),
                inline: true,
                specificity: Specificity::default(),
                order,
                declaration,
            });
            order += 1;
        }

        candidates.sort_by(|a, b| {
            a.bucket
                .cmp(&b.bucket)
                .then(a.inline.cmp(&b.inline))
                .then(a.specificity.cmp(&b.specificity))
                .then(a.order.cmp(&b.order))
        });

        let mut style = ComputedStyle::inherit_from(parent_style);

        // `font-size` must land first: every `em` length depends on it.
        let mut ctx = LengthContext::new(
            parent_style.font_size,
            root_font_size,
            (media.width, media.height),
        );
        for candidate in candidates
            .iter()
            .filter(|c| matches!(c.declaration.name.as_str(), "font-size" | "font"))
        {
            apply_declaration(&mut style, parent_style, candidate.declaration, &ctx);
        }

        ctx.font_size = style.font_size;
        for candidate in candidates
            .iter()
            .filter(|c| !matches!(c.declaration.name.as_str(), "font-size" | "font"))
        {
            apply_declaration(&mut style, parent_style, candidate.declaration, &ctx);
        }

        // The root element cannot be inline-level, and `display: contents` on it
        // has no meaning.
        if document.node(node).parent == Some(document.root())
            && (style.display.is_inline_level() || style.display == Display::Contents)
        {
            style.display = Display::Block;
        }

        style
    }
}

/// Attribute-driven presentation, e.g. `<img width=100>` or `<td align=center>`.
fn presentational_hints(document: &Document, node: NodeId) -> Vec<Declaration> {
    let Some(element) = document.element(node) else {
        return Vec::new();
    };
    let mut css = String::new();

    // A bare number means pixels; a percentage stays a percentage.
    let mut dimension = |property: &str, raw: &str| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return;
        }
        if trimmed.ends_with('%') {
            css.push_str(&format!("{property}:{trimmed};"));
        } else if trimmed.chars().all(|c| c.is_ascii_digit()) {
            css.push_str(&format!("{property}:{trimmed}px;"));
        }
    };

    match element.name.as_str() {
        "img" | "canvas" | "video" | "iframe" | "embed" | "object" | "table" | "td" | "th"
        | "col" | "colgroup" => {
            if let Some(width) = element.attr("width") {
                dimension("width", width);
            }
            if let Some(height) = element.attr("height") {
                dimension("height", height);
            }
        }
        "hr" => {
            if let Some(width) = element.attr("width") {
                dimension("width", width);
            }
        }
        _ => {}
    }

    if let Some(color) = element.attr("bgcolor") {
        css.push_str(&format!("background-color:{color};"));
    }
    if let Some(color) = element.attr("color") {
        css.push_str(&format!("color:{color};"));
    }
    if let Some(align) = element.attr("align") {
        match align.trim().to_ascii_lowercase().as_str() {
            "center" if element.name == "table" => {
                css.push_str("margin-left:auto;margin-right:auto;")
            }
            align @ ("left" | "right" | "center" | "justify") => {
                css.push_str(&format!("text-align:{align};"))
            }
            _ => {}
        }
    }
    if element.name == "table" {
        if let Some(border) = element.attr("border") {
            if border.trim() != "0" && !border.trim().is_empty() {
                css.push_str("border:1px solid #c9ccd1;");
            }
        }
        if let Some(padding) = element.attr("cellpadding") {
            if padding.chars().all(|c| c.is_ascii_digit()) && !padding.is_empty() {
                css.push_str(&format!("--wat-cellpadding:{padding}px;"));
            }
        }
    }
    // `<td nowrap>`, `<textarea wrap=off>`
    if element.has_attr("nowrap") {
        css.push_str("white-space:nowrap;");
    }

    if css.is_empty() {
        Vec::new()
    } else {
        wat_css::parse_inline_style(&css)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Display, LengthPercentage, Size, TextAlign};
    use wat_css::Color;

    fn styled(html: &str, css: &str) -> (Document, StyleTree) {
        let document = wat_html::parse(html);
        let mut engine = StyleEngine::new();
        engine.add_author_sheet(Stylesheet::parse(css, Origin::Author));
        let tree = engine.compute(
            &document,
            &MediaContext::default(),
            &MatchContext::default(),
        );
        (document, tree)
    }

    fn style_of<'a>(document: &Document, tree: &'a StyleTree, selector: &str) -> &'a ComputedStyle {
        let node = document.query(selector).expect("node not found");
        tree.get(node)
    }

    #[test]
    fn ua_stylesheet_provides_defaults() {
        let (document, tree) = styled("<p>hi</p><span>x</span>", "");
        assert_eq!(style_of(&document, &tree, "p").display, Display::Block);
        assert_eq!(style_of(&document, &tree, "span").display, Display::Inline);
        assert_eq!(style_of(&document, &tree, "body").display, Display::Block);
        // Body has the standard 8px margin.
        assert_eq!(
            style_of(&document, &tree, "body").margin.top,
            crate::types::Margin::Length(LengthPercentage::Px(8.0))
        );
    }

    #[test]
    fn headings_get_relative_sizes() {
        let (document, tree) = styled("<h1>a</h1><h3>b</h3>", "");
        assert_eq!(style_of(&document, &tree, "h1").font_size, 32.0);
        assert!(style_of(&document, &tree, "h1").font_weight >= 700);
        assert!((style_of(&document, &tree, "h3").font_size - 18.72).abs() < 0.01);
    }

    #[test]
    fn author_styles_beat_ua_styles() {
        let (document, tree) = styled("<p>hi</p>", "p { display: inline; color: red }");
        let style = style_of(&document, &tree, "p");
        assert_eq!(style.display, Display::Inline);
        assert_eq!(style.color, Color::rgb(255, 0, 0));
    }

    #[test]
    fn specificity_decides_between_author_rules() {
        let (document, tree) = styled(
            "<p id=\"x\" class=\"c\">hi</p>",
            "p { color: red } .c { color: green } #x { color: blue }",
        );
        assert_eq!(style_of(&document, &tree, "p").color, Color::rgb(0, 0, 255));
    }

    #[test]
    fn source_order_breaks_specificity_ties() {
        let (document, tree) = styled(
            "<p class=\"c\">hi</p>",
            ".c { color: red } .c { color: lime }",
        );
        assert_eq!(style_of(&document, &tree, "p").color, Color::rgb(0, 255, 0));
    }

    #[test]
    fn inline_style_beats_selectors() {
        let (document, tree) = styled(
            "<p id=\"x\" style=\"color: orange\">hi</p>",
            "#x { color: blue }",
        );
        assert_eq!(
            style_of(&document, &tree, "p").color,
            Color::rgb(255, 165, 0)
        );
    }

    #[test]
    fn important_beats_inline() {
        let (document, tree) = styled(
            "<p style=\"color: orange\">hi</p>",
            "p { color: blue !important }",
        );
        assert_eq!(style_of(&document, &tree, "p").color, Color::rgb(0, 0, 255));
    }

    #[test]
    fn inheritance_flows_down() {
        let (document, tree) = styled(
            "<div class=\"outer\"><p><span>deep</span></p></div>",
            ".outer { color: #ff00ff; font-size: 20px }",
        );
        let span = style_of(&document, &tree, "span");
        assert_eq!(span.color, Color::rgb(255, 0, 255));
        assert_eq!(span.font_size, 20.0);
    }

    #[test]
    fn text_nodes_share_their_parents_style() {
        let document = wat_html::parse("<p style=\"color:#00f\">hi</p>");
        let engine = StyleEngine::new();
        let tree = engine.compute(
            &document,
            &MediaContext::default(),
            &MatchContext::default(),
        );
        let p = document.query("p").unwrap();
        let text = document.children(p).next().unwrap();
        assert_eq!(tree.get(text).color, Color::rgb(0, 0, 255));
    }

    #[test]
    fn rem_resolves_against_the_root_font_size() {
        let (document, tree) = styled(
            "<div><p>x</p></div>",
            "html { font-size: 20px } div { font-size: 40px } p { width: 2rem }",
        );
        assert_eq!(
            style_of(&document, &tree, "p").width,
            Size::Definite(LengthPercentage::Px(40.0)),
            "2rem must use the root's 20px, not the parent's 40px"
        );
    }

    #[test]
    fn em_resolves_against_the_parent_font_size() {
        let (document, tree) = styled(
            "<div><p>x</p></div>",
            "div { font-size: 20px } p { font-size: 2em; margin-top: 1em }",
        );
        let style = style_of(&document, &tree, "p");
        assert_eq!(style.font_size, 40.0);
        assert_eq!(
            style.margin.top,
            crate::types::Margin::Length(LengthPercentage::Px(40.0)),
            "margin em uses the element's own font size"
        );
    }

    #[test]
    fn media_queries_gate_rules() {
        let document = wat_html::parse("<p>hi</p>");
        let mut engine = StyleEngine::new();
        engine.add_author_sheet(Stylesheet::parse(
            "p { color: black } @media (max-width: 500px) { p { color: red } }",
            Origin::Author,
        ));

        let wide = engine.compute(
            &document,
            &MediaContext::screen(1000.0, 800.0),
            &MatchContext::default(),
        );
        let p = document.query("p").unwrap();
        assert_eq!(wide.get(p).color, Color::BLACK);

        let narrow = engine.compute(
            &document,
            &MediaContext::screen(400.0, 800.0),
            &MatchContext::default(),
        );
        assert_eq!(narrow.get(p).color, Color::rgb(255, 0, 0));
    }

    #[test]
    fn hover_state_is_applied() {
        let document = wat_html::parse("<a href=\"#\">link</a>");
        let mut engine = StyleEngine::new();
        engine.add_author_sheet(Stylesheet::parse(
            "a:hover { color: #00ff00 }",
            Origin::Author,
        ));
        let a = document.query("a").unwrap();

        let idle = engine.compute(
            &document,
            &MediaContext::default(),
            &MatchContext::default(),
        );
        assert_ne!(idle.get(a).color, Color::rgb(0, 255, 0));

        let hovered = engine.compute(
            &document,
            &MediaContext::default(),
            &MatchContext::with_hover(Some(a)),
        );
        assert_eq!(hovered.get(a).color, Color::rgb(0, 255, 0));
    }

    #[test]
    fn presentational_attributes_are_honoured() {
        let (document, tree) = styled("<img src=\"x.png\" width=\"120\" height=\"60\">", "");
        let img = style_of(&document, &tree, "img");
        assert_eq!(img.width, Size::Definite(LengthPercentage::Px(120.0)));
        assert_eq!(img.height, Size::Definite(LengthPercentage::Px(60.0)));

        let (document, tree) = styled("<td align=\"right\">x</td>", "");
        assert_eq!(
            style_of(&document, &tree, "td").text_align,
            TextAlign::Right
        );
    }

    #[test]
    fn author_css_overrides_presentational_attributes() {
        let (document, tree) = styled("<img src=\"x.png\" width=\"120\">", "img { width: 300px }");
        assert_eq!(
            style_of(&document, &tree, "img").width,
            Size::Definite(LengthPercentage::Px(300.0))
        );
    }

    #[test]
    fn hidden_attribute_hides_the_element() {
        let (document, tree) = styled("<div hidden>x</div>", "");
        assert_eq!(style_of(&document, &tree, "div").display, Display::None);
    }

    #[test]
    fn root_element_is_forced_block() {
        let (document, tree) = styled("<p>x</p>", "html { display: inline }");
        assert_eq!(style_of(&document, &tree, "html").display, Display::Block);
    }

    #[test]
    fn user_sheets_sit_between_ua_and_author() {
        let document = wat_html::parse("<p>hi</p>");
        let mut engine = StyleEngine::new();
        engine.add_user_sheet(Stylesheet::parse("p { color: red }", Origin::User));
        engine.add_author_sheet(Stylesheet::parse("p { color: blue }", Origin::Author));
        let tree = engine.compute(
            &document,
            &MediaContext::default(),
            &MatchContext::default(),
        );
        let p = document.query("p").unwrap();
        assert_eq!(tree.get(p).color, Color::rgb(0, 0, 255));

        // …but an important user declaration wins over the author.
        let mut engine = StyleEngine::new();
        engine.add_user_sheet(Stylesheet::parse(
            "p { color: red !important }",
            Origin::User,
        ));
        engine.add_author_sheet(Stylesheet::parse(
            "p { color: blue !important }",
            Origin::Author,
        ));
        let tree = engine.compute(
            &document,
            &MediaContext::default(),
            &MatchContext::default(),
        );
        assert_eq!(tree.get(p).color, Color::rgb(255, 0, 0));
    }

    #[test]
    fn links_get_the_default_colour_and_underline() {
        let (document, tree) = styled("<a href=\"/x\">link</a>", "");
        let a = style_of(&document, &tree, "a");
        assert!(a.text_decoration.underline);
        assert_ne!(a.color, Color::BLACK);
    }
}
