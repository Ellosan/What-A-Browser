//! Applying declarations to a [`ComputedStyle`], including shorthands.

use crate::types::*;
use wat_css::values::{LengthContext, Value};
use wat_css::{Color, Declaration};

/// Properties that inherit by default.
pub fn is_inherited(name: &str) -> bool {
    matches!(
        name,
        "color"
            | "cursor"
            | "font"
            | "font-family"
            | "font-size"
            | "font-style"
            | "font-weight"
            | "letter-spacing"
            | "line-height"
            | "list-style"
            | "list-style-position"
            | "list-style-type"
            | "text-align"
            | "text-indent"
            | "text-shadow"
            | "text-transform"
            | "visibility"
            | "white-space"
            | "word-spacing"
    )
}

/// Copies one property from `from` into `into`, used for `inherit`/`initial`.
fn copy_property(name: &str, from: &ComputedStyle, into: &mut ComputedStyle) {
    match name {
        "color" => into.color = from.color,
        "cursor" => into.cursor = from.cursor,
        "font-family" => into.font_family = from.font_family.clone(),
        "font-size" => into.font_size = from.font_size,
        "font-style" => into.font_style = from.font_style,
        "font-weight" => into.font_weight = from.font_weight,
        "font" => {
            into.font_family = from.font_family.clone();
            into.font_size = from.font_size;
            into.font_style = from.font_style;
            into.font_weight = from.font_weight;
            into.line_height = from.line_height;
        }
        "letter-spacing" => into.letter_spacing = from.letter_spacing,
        "line-height" => into.line_height = from.line_height,
        "list-style-position" => into.list_style_position = from.list_style_position,
        "list-style-type" => into.list_style_type = from.list_style_type,
        "text-align" => into.text_align = from.text_align,
        "text-decoration" | "text-decoration-line" => {
            into.text_decoration = from.text_decoration;
            into.text_decoration_color = from.text_decoration_color;
        }
        "text-indent" => into.text_indent = from.text_indent,
        "text-shadow" => into.text_shadow = from.text_shadow.clone(),
        "text-transform" => into.text_transform = from.text_transform,
        "visibility" => into.visibility = from.visibility,
        "white-space" => into.white_space = from.white_space,
        "word-spacing" => into.word_spacing = from.word_spacing,

        "display" => into.display = from.display,
        "position" => into.position = from.position,
        "float" => into.float = from.float,
        "clear" => into.clear = from.clear,
        "box-sizing" => into.box_sizing = from.box_sizing,
        "opacity" => into.opacity = from.opacity,
        "z-index" => into.z_index = from.z_index,
        "overflow" => {
            into.overflow_x = from.overflow_x;
            into.overflow_y = from.overflow_y;
        }
        "overflow-x" => into.overflow_x = from.overflow_x,
        "overflow-y" => into.overflow_y = from.overflow_y,

        "width" => into.width = from.width,
        "height" => into.height = from.height,
        "min-width" => into.min_width = from.min_width,
        "max-width" => into.max_width = from.max_width,
        "min-height" => into.min_height = from.min_height,
        "max-height" => into.max_height = from.max_height,
        "margin" => into.margin = from.margin,
        "margin-top" => into.margin.top = from.margin.top,
        "margin-right" => into.margin.right = from.margin.right,
        "margin-bottom" => into.margin.bottom = from.margin.bottom,
        "margin-left" => into.margin.left = from.margin.left,
        "padding" => into.padding = from.padding,
        "padding-top" => into.padding.top = from.padding.top,
        "padding-right" => into.padding.right = from.padding.right,
        "padding-bottom" => into.padding.bottom = from.padding.bottom,
        "padding-left" => into.padding.left = from.padding.left,
        "border" => {
            into.border_width = from.border_width;
            into.border_style = from.border_style;
            into.border_color = from.border_color;
        }
        "border-radius" => into.border_radius = from.border_radius,
        "background" => {
            into.background_color = from.background_color;
            into.background_image = from.background_image.clone();
        }
        "background-color" => into.background_color = from.background_color,
        "background-image" => into.background_image = from.background_image.clone(),
        "box-shadow" => into.box_shadow = from.box_shadow.clone(),
        "filter" => into.filter = from.filter,
        "backdrop-filter" | "-webkit-backdrop-filter" => {
            into.backdrop_filter = from.backdrop_filter
        }
        "transform" => into.transform = from.transform,

        "flex-direction" => into.flex_direction = from.flex_direction,
        "flex-wrap" => into.flex_wrap = from.flex_wrap,
        "justify-content" => into.justify_content = from.justify_content,
        "align-items" => into.align_items = from.align_items,
        "align-content" => into.align_content = from.align_content,
        "align-self" => into.align_self = from.align_self,
        "flex-grow" => into.flex_grow = from.flex_grow,
        "flex-shrink" => into.flex_shrink = from.flex_shrink,
        "flex-basis" => into.flex_basis = from.flex_basis,
        "flex" => {
            into.flex_grow = from.flex_grow;
            into.flex_shrink = from.flex_shrink;
            into.flex_basis = from.flex_basis;
        }
        "order" => into.order = from.order,
        "gap" => {
            into.row_gap = from.row_gap;
            into.column_gap = from.column_gap;
        }
        "row-gap" => into.row_gap = from.row_gap,
        "column-gap" => into.column_gap = from.column_gap,
        "grid-template-columns" => into.grid_template_columns = from.grid_template_columns.clone(),
        _ => {}
    }
}

/// Applies a single declaration.
pub fn apply_declaration(
    style: &mut ComputedStyle,
    parent: &ComputedStyle,
    declaration: &Declaration,
    ctx: &LengthContext,
) {
    let name = declaration.name.as_str();
    let value = &declaration.value;

    // CSS-wide keywords come first: they apply to any property.
    if let Some(keyword) = value.as_keyword() {
        match keyword.to_ascii_lowercase().as_str() {
            "inherit" => {
                copy_property(name, parent, style);
                return;
            }
            "initial" => {
                copy_property(name, &ComputedStyle::initial(), style);
                return;
            }
            // `revert` would restore the previous origin's value; treating it as
            // `unset` is the closest approximation without per-origin tracking.
            "unset" | "revert" => {
                if is_inherited(name) {
                    copy_property(name, parent, style);
                } else {
                    copy_property(name, &ComputedStyle::initial(), style);
                }
                return;
            }
            _ => {}
        }
    }

    let items = value.items();
    match name {
        // ---- box generation -------------------------------------------------
        "display" => {
            if let Some(display) = parse_display(value) {
                style.display = display;
            }
        }
        "position" => {
            style.position = match keyword_of(value).as_deref() {
                Some("static") => Position::Static,
                Some("relative") => Position::Relative,
                Some("absolute") => Position::Absolute,
                Some("fixed") => Position::Fixed,
                Some("sticky") => Position::Sticky,
                _ => style.position,
            }
        }
        "float" => {
            style.float = match keyword_of(value).as_deref() {
                Some("left") => Float::Left,
                Some("right") => Float::Right,
                Some("none") => Float::None,
                _ => style.float,
            }
        }
        "clear" => {
            style.clear = match keyword_of(value).as_deref() {
                Some("left") => Clear::Left,
                Some("right") => Clear::Right,
                Some("both") => Clear::Both,
                Some("none") => Clear::None,
                _ => style.clear,
            }
        }
        "box-sizing" => {
            style.box_sizing = match keyword_of(value).as_deref() {
                Some("border-box") => BoxSizing::BorderBox,
                Some("content-box") => BoxSizing::ContentBox,
                _ => style.box_sizing,
            }
        }
        "visibility" => {
            style.visibility = match keyword_of(value).as_deref() {
                Some("hidden") | Some("collapse") => Visibility::Hidden,
                Some("visible") => Visibility::Visible,
                _ => style.visibility,
            }
        }
        "overflow" => {
            if let Some(first) = items.first().and_then(parse_overflow) {
                style.overflow_x = first;
                style.overflow_y = items.get(1).and_then(parse_overflow).unwrap_or(first);
            }
        }
        "overflow-x" => {
            if let Some(overflow) = parse_overflow(value) {
                style.overflow_x = overflow;
            }
        }
        "overflow-y" => {
            if let Some(overflow) = parse_overflow(value) {
                style.overflow_y = overflow;
            }
        }
        "z-index" => {
            style.z_index = match value {
                Value::Number(n) => Some(*n as i32),
                _ if value.is_keyword("auto") => None,
                _ => style.z_index,
            }
        }

        // ---- box model ------------------------------------------------------
        "width" => style.width = parse_size(value, ctx).unwrap_or(style.width),
        "height" => style.height = parse_size(value, ctx).unwrap_or(style.height),
        "min-width" => style.min_width = parse_size(value, ctx).unwrap_or(style.min_width),
        "max-width" => style.max_width = parse_size(value, ctx).unwrap_or(style.max_width),
        "min-height" => style.min_height = parse_size(value, ctx).unwrap_or(style.min_height),
        "max-height" => style.max_height = parse_size(value, ctx).unwrap_or(style.max_height),

        "margin" => {
            if let Some(sides) = parse_sides(items, |v| parse_margin(v, ctx)) {
                style.margin = sides;
            }
        }
        "margin-top" => set_if(&mut style.margin.top, parse_margin(value, ctx)),
        "margin-right" => set_if(&mut style.margin.right, parse_margin(value, ctx)),
        "margin-bottom" => set_if(&mut style.margin.bottom, parse_margin(value, ctx)),
        "margin-left" => set_if(&mut style.margin.left, parse_margin(value, ctx)),
        "margin-inline" => {
            if let Some(sides) = parse_sides(items, |v| parse_margin(v, ctx)) {
                style.margin.left = sides.left;
                style.margin.right = sides.right;
            }
        }
        "margin-block" => {
            if let Some(sides) = parse_sides(items, |v| parse_margin(v, ctx)) {
                style.margin.top = sides.top;
                style.margin.bottom = sides.bottom;
            }
        }

        "padding" => {
            if let Some(sides) = parse_sides(items, |v| length_percentage(v, ctx)) {
                style.padding = sides;
            }
        }
        "padding-top" => set_if(&mut style.padding.top, length_percentage(value, ctx)),
        "padding-right" => set_if(&mut style.padding.right, length_percentage(value, ctx)),
        "padding-bottom" => set_if(&mut style.padding.bottom, length_percentage(value, ctx)),
        "padding-left" => set_if(&mut style.padding.left, length_percentage(value, ctx)),
        "padding-inline" => {
            if let Some(sides) = parse_sides(items, |v| length_percentage(v, ctx)) {
                style.padding.left = sides.left;
                style.padding.right = sides.right;
            }
        }
        "padding-block" => {
            if let Some(sides) = parse_sides(items, |v| length_percentage(v, ctx)) {
                style.padding.top = sides.top;
                style.padding.bottom = sides.bottom;
            }
        }

        "border" => apply_border_shorthand(style, items, ctx, Edge::All),
        "border-top" => apply_border_shorthand(style, items, ctx, Edge::Top),
        "border-right" => apply_border_shorthand(style, items, ctx, Edge::Right),
        "border-bottom" => apply_border_shorthand(style, items, ctx, Edge::Bottom),
        "border-left" => apply_border_shorthand(style, items, ctx, Edge::Left),
        "border-width" => {
            if let Some(sides) = parse_sides(items, |v| border_width(v, ctx)) {
                style.border_width = sides;
            }
        }
        "border-style" => {
            if let Some(sides) = parse_sides(items, parse_border_style) {
                style.border_style = sides;
            }
        }
        "border-color" => {
            if let Some(sides) = parse_sides(items, |v| v.to_color()) {
                style.border_color = sides;
            }
        }
        "border-top-width" => set_if(&mut style.border_width.top, border_width(value, ctx)),
        "border-right-width" => set_if(&mut style.border_width.right, border_width(value, ctx)),
        "border-bottom-width" => set_if(&mut style.border_width.bottom, border_width(value, ctx)),
        "border-left-width" => set_if(&mut style.border_width.left, border_width(value, ctx)),
        "border-top-style" => set_if(&mut style.border_style.top, parse_border_style(value)),
        "border-right-style" => set_if(&mut style.border_style.right, parse_border_style(value)),
        "border-bottom-style" => set_if(&mut style.border_style.bottom, parse_border_style(value)),
        "border-left-style" => set_if(&mut style.border_style.left, parse_border_style(value)),
        "border-top-color" => set_if(&mut style.border_color.top, value.to_color()),
        "border-right-color" => set_if(&mut style.border_color.right, value.to_color()),
        "border-bottom-color" => set_if(&mut style.border_color.bottom, value.to_color()),
        "border-left-color" => set_if(&mut style.border_color.left, value.to_color()),

        "border-radius" => {
            if let Some(corners) = parse_corners(items, |v| length_percentage(v, ctx)) {
                style.border_radius = corners;
            }
        }
        "border-top-left-radius" => {
            set_if(&mut style.border_radius.top_left, radius_of(items, ctx))
        }
        "border-top-right-radius" => {
            set_if(&mut style.border_radius.top_right, radius_of(items, ctx))
        }
        "border-bottom-right-radius" => {
            set_if(&mut style.border_radius.bottom_right, radius_of(items, ctx))
        }
        "border-bottom-left-radius" => {
            set_if(&mut style.border_radius.bottom_left, radius_of(items, ctx))
        }

        "top" => style.inset.top = inset_of(value, ctx),
        "right" => style.inset.right = inset_of(value, ctx),
        "bottom" => style.inset.bottom = inset_of(value, ctx),
        "left" => style.inset.left = inset_of(value, ctx),
        "inset" => {
            if let Some(sides) = parse_sides(items, |v| Some(inset_of(v, ctx))) {
                style.inset = sides;
            }
        }

        // ---- painting -------------------------------------------------------
        "background" => apply_background_shorthand(style, value, ctx),
        "background-color" => set_if(&mut style.background_color, value.to_color()),
        "background-image" => {
            if let Some(image) = parse_background_image(value) {
                style.background_image = image;
            }
        }
        "background-size" => {
            style.background_size = match keyword_of(value).as_deref() {
                Some("cover") => BackgroundSize::Cover,
                Some("contain") => BackgroundSize::Contain,
                _ => BackgroundSize::Auto,
            }
        }
        "background-repeat" => {
            style.background_repeat = match keyword_of(value).as_deref() {
                Some("no-repeat") => BackgroundRepeat::NoRepeat,
                Some("repeat-x") => BackgroundRepeat::RepeatX,
                Some("repeat-y") => BackgroundRepeat::RepeatY,
                _ => BackgroundRepeat::Repeat,
            }
        }
        "box-shadow" => style.box_shadow = parse_shadows(value, ctx, false),
        "text-shadow" => style.text_shadow = parse_shadows(value, ctx, true),
        "opacity" => {
            if let Some(alpha) = alpha_of(value) {
                style.opacity = alpha;
            }
        }
        "filter" => {
            if let Some(filter) = parse_filter(value, ctx) {
                style.filter = filter;
            }
        }
        "backdrop-filter" | "-webkit-backdrop-filter" => {
            if let Some(filter) = parse_filter(value, ctx) {
                style.backdrop_filter = filter;
            }
        }
        "transform" => {
            if let Some(transform) = parse_transform(value, ctx) {
                style.transform = transform;
            }
        }

        // ---- text -----------------------------------------------------------
        "color" => set_if(&mut style.color, value.to_color()),
        "font" => apply_font_shorthand(style, value, ctx),
        "font-family" => {
            let families = parse_font_family(value);
            if !families.is_empty() {
                style.font_family = families;
            }
        }
        "font-size" => {
            if let Some(size) = parse_font_size(value, ctx, parent.font_size) {
                style.font_size = size;
            }
        }
        "font-weight" => {
            if let Some(weight) = parse_font_weight(value, parent.font_weight) {
                style.font_weight = weight;
            }
        }
        "font-style" => {
            style.font_style = match keyword_of(value).as_deref() {
                Some("italic") => FontStyle::Italic,
                Some("oblique") => FontStyle::Oblique,
                Some("normal") => FontStyle::Normal,
                _ => style.font_style,
            }
        }
        "line-height" => {
            if let Some(line_height) = parse_line_height(value, ctx) {
                style.line_height = line_height;
            }
        }
        "letter-spacing" => {
            if value.is_keyword("normal") {
                style.letter_spacing = 0.0;
            } else if let Some(px) = length(value, ctx) {
                style.letter_spacing = px;
            }
        }
        "word-spacing" => {
            if value.is_keyword("normal") {
                style.word_spacing = 0.0;
            } else if let Some(px) = length(value, ctx) {
                style.word_spacing = px;
            }
        }
        "text-align" => {
            style.text_align = match keyword_of(value).as_deref() {
                Some("left") => TextAlign::Left,
                Some("right") => TextAlign::Right,
                Some("center") => TextAlign::Center,
                Some("justify") => TextAlign::Justify,
                Some("start") => TextAlign::Start,
                Some("end") => TextAlign::End,
                _ => style.text_align,
            }
        }
        "text-decoration" | "text-decoration-line" => {
            let mut decoration = TextDecoration::default();
            let mut saw_line_keyword = false;
            for item in items {
                match item.as_keyword().map(str::to_ascii_lowercase).as_deref() {
                    Some("underline") => {
                        decoration.underline = true;
                        saw_line_keyword = true;
                    }
                    Some("overline") => {
                        decoration.overline = true;
                        saw_line_keyword = true;
                    }
                    Some("line-through") => {
                        decoration.line_through = true;
                        saw_line_keyword = true;
                    }
                    Some("none") => saw_line_keyword = true,
                    _ => {
                        if let Some(color) = item.to_color() {
                            style.text_decoration_color = Some(color);
                        }
                    }
                }
            }
            if saw_line_keyword {
                style.text_decoration = decoration;
            }
        }
        "text-decoration-color" => style.text_decoration_color = value.to_color(),
        "text-transform" => {
            style.text_transform = match keyword_of(value).as_deref() {
                Some("uppercase") => TextTransform::Uppercase,
                Some("lowercase") => TextTransform::Lowercase,
                Some("capitalize") => TextTransform::Capitalize,
                Some("none") => TextTransform::None,
                _ => style.text_transform,
            }
        }
        "text-indent" => set_if(&mut style.text_indent, length_percentage(value, ctx)),
        "white-space" => {
            style.white_space = match keyword_of(value).as_deref() {
                Some("nowrap") => WhiteSpace::Nowrap,
                Some("pre") => WhiteSpace::Pre,
                Some("pre-wrap") => WhiteSpace::PreWrap,
                Some("pre-line") => WhiteSpace::PreLine,
                Some("normal") => WhiteSpace::Normal,
                _ => style.white_space,
            }
        }
        "vertical-align" => {
            style.vertical_align = match keyword_of(value).as_deref() {
                Some("top") => VerticalAlign::Top,
                Some("middle") => VerticalAlign::Middle,
                Some("bottom") => VerticalAlign::Bottom,
                Some("text-top") => VerticalAlign::TextTop,
                Some("text-bottom") => VerticalAlign::TextBottom,
                Some("sub") => VerticalAlign::Sub,
                Some("super") => VerticalAlign::Super,
                Some("baseline") => VerticalAlign::Baseline,
                _ => style.vertical_align,
            }
        }
        "cursor" => {
            style.cursor = match keyword_of(value).as_deref() {
                Some("pointer") => Cursor::Pointer,
                Some("text") => Cursor::Text,
                Some("move") => Cursor::Move,
                Some("not-allowed") => Cursor::NotAllowed,
                Some("grab") => Cursor::Grab,
                Some("default") => Cursor::Default,
                _ => Cursor::Auto,
            }
        }

        // ---- flex and grid --------------------------------------------------
        "flex-direction" => {
            style.flex_direction = match keyword_of(value).as_deref() {
                Some("row") => FlexDirection::Row,
                Some("row-reverse") => FlexDirection::RowReverse,
                Some("column") => FlexDirection::Column,
                Some("column-reverse") => FlexDirection::ColumnReverse,
                _ => style.flex_direction,
            }
        }
        "flex-wrap" => {
            style.flex_wrap = match keyword_of(value).as_deref() {
                Some("wrap") => FlexWrap::Wrap,
                Some("wrap-reverse") => FlexWrap::WrapReverse,
                Some("nowrap") => FlexWrap::NoWrap,
                _ => style.flex_wrap,
            }
        }
        "flex-flow" => {
            for item in items {
                match item.as_keyword().map(str::to_ascii_lowercase).as_deref() {
                    Some("row") => style.flex_direction = FlexDirection::Row,
                    Some("row-reverse") => style.flex_direction = FlexDirection::RowReverse,
                    Some("column") => style.flex_direction = FlexDirection::Column,
                    Some("column-reverse") => style.flex_direction = FlexDirection::ColumnReverse,
                    Some("wrap") => style.flex_wrap = FlexWrap::Wrap,
                    Some("wrap-reverse") => style.flex_wrap = FlexWrap::WrapReverse,
                    Some("nowrap") => style.flex_wrap = FlexWrap::NoWrap,
                    _ => {}
                }
            }
        }
        "justify-content" => set_if(&mut style.justify_content, parse_justify(value)),
        "align-content" => set_if(&mut style.align_content, parse_justify(value)),
        "justify-items" | "justify-self" => {}
        "align-items" => {
            style.align_items = match keyword_of(value).as_deref() {
                Some("flex-start") | Some("start") => AlignItems::Start,
                Some("flex-end") | Some("end") => AlignItems::End,
                Some("center") => AlignItems::Center,
                Some("baseline") => AlignItems::Baseline,
                Some("stretch") | Some("normal") => AlignItems::Stretch,
                _ => style.align_items,
            }
        }
        "align-self" => {
            style.align_self = match keyword_of(value).as_deref() {
                Some("auto") => AlignSelf::Auto,
                Some("flex-start") | Some("start") => AlignSelf::Start,
                Some("flex-end") | Some("end") => AlignSelf::End,
                Some("center") => AlignSelf::Center,
                Some("baseline") => AlignSelf::Baseline,
                Some("stretch") => AlignSelf::Stretch,
                _ => style.align_self,
            }
        }
        "place-items" => {
            if let Some(first) = items.first() {
                apply_declaration(
                    style,
                    parent,
                    &Declaration {
                        name: "align-items".into(),
                        value: first.clone(),
                        important: declaration.important,
                    },
                    ctx,
                );
            }
        }
        "place-content" => {
            if let Some(first) = items.first() {
                set_if(&mut style.align_content, parse_justify(first));
                if let Some(second) = items.get(1) {
                    set_if(&mut style.justify_content, parse_justify(second));
                }
            }
        }
        "flex-grow" => {
            if let Some(n) = value.as_number() {
                style.flex_grow = n.max(0.0);
            }
        }
        "flex-shrink" => {
            if let Some(n) = value.as_number() {
                style.flex_shrink = n.max(0.0);
            }
        }
        "flex-basis" => style.flex_basis = parse_size(value, ctx).unwrap_or(style.flex_basis),
        "flex" => apply_flex_shorthand(style, items, ctx),
        "order" => {
            if let Some(n) = value.as_number() {
                style.order = n as i32;
            }
        }
        "gap" => {
            if let Some(row) = items.first().and_then(|v| length_percentage(v, ctx)) {
                style.row_gap = row;
                style.column_gap = items
                    .get(1)
                    .and_then(|v| length_percentage(v, ctx))
                    .unwrap_or(row);
            }
        }
        "row-gap" => set_if(&mut style.row_gap, length_percentage(value, ctx)),
        "column-gap" => set_if(&mut style.column_gap, length_percentage(value, ctx)),
        "grid-template-columns" => {
            let tracks = parse_tracks(value, ctx);
            if !tracks.is_empty() {
                style.grid_template_columns = tracks;
            }
        }
        "grid-gap" => {
            if let Some(row) = items.first().and_then(|v| length_percentage(v, ctx)) {
                style.row_gap = row;
                style.column_gap = items
                    .get(1)
                    .and_then(|v| length_percentage(v, ctx))
                    .unwrap_or(row);
            }
        }

        // ---- lists ----------------------------------------------------------
        "list-style-type" => set_if(&mut style.list_style_type, parse_list_style_type(value)),
        "list-style-position" => {
            style.list_style_position = match keyword_of(value).as_deref() {
                Some("inside") => ListStylePosition::Inside,
                _ => ListStylePosition::Outside,
            }
        }
        "list-style" => {
            for item in items {
                if let Some(kind) = parse_list_style_type(item) {
                    style.list_style_type = kind;
                } else if item.is_keyword("inside") {
                    style.list_style_position = ListStylePosition::Inside;
                } else if item.is_keyword("outside") {
                    style.list_style_position = ListStylePosition::Outside;
                }
            }
        }

        // Properties we parse but do not act on are ignored silently; that is
        // the same outcome as a browser that does not support them.
        _ => {}
    }
}

fn set_if<T>(target: &mut T, value: Option<T>) {
    if let Some(value) = value {
        *target = value;
    }
}

fn keyword_of(value: &Value) -> Option<String> {
    value.as_keyword().map(str::to_ascii_lowercase)
}

fn alpha_of(value: &Value) -> Option<f32> {
    match value {
        Value::Number(n) => Some(n.clamp(0.0, 1.0)),
        Value::Percentage(p) => Some((p / 100.0).clamp(0.0, 1.0)),
        _ => None,
    }
}

fn length(value: &Value, ctx: &LengthContext) -> Option<f32> {
    value.to_px(ctx, None)
}

fn length_percentage(value: &Value, ctx: &LengthContext) -> Option<LengthPercentage> {
    value.to_calc(ctx).map(LengthPercentage::from_calc)
}

fn parse_size(value: &Value, ctx: &LengthContext) -> Option<Size> {
    match keyword_of(value).as_deref() {
        Some("auto") => return Some(Size::Auto),
        Some("none") => return Some(Size::None),
        Some("min-content") => return Some(Size::MinContent),
        Some("max-content") => return Some(Size::MaxContent),
        Some("fit-content") => return Some(Size::FitContent),
        _ => {}
    }
    length_percentage(value, ctx).map(Size::Definite)
}

fn parse_margin(value: &Value, ctx: &LengthContext) -> Option<Margin> {
    if value.is_keyword("auto") {
        return Some(Margin::Auto);
    }
    length_percentage(value, ctx).map(Margin::Length)
}

fn inset_of(value: &Value, ctx: &LengthContext) -> Option<LengthPercentage> {
    if value.is_keyword("auto") {
        return None;
    }
    length_percentage(value, ctx)
}

fn radius_of(items: &[Value], ctx: &LengthContext) -> Option<LengthPercentage> {
    // Elliptical radii (`10px 20px`) collapse to the first value.
    items.first().and_then(|v| length_percentage(v, ctx))
}

fn parse_overflow(value: &Value) -> Option<Overflow> {
    match keyword_of(value).as_deref() {
        Some("visible") => Some(Overflow::Visible),
        Some("hidden") => Some(Overflow::Hidden),
        Some("scroll") => Some(Overflow::Scroll),
        Some("auto") => Some(Overflow::Auto),
        Some("clip") => Some(Overflow::Clip),
        _ => None,
    }
}

fn parse_display(value: &Value) -> Option<Display> {
    // `display: inline flex` (two-value syntax) collapses to the inner value.
    let keywords: Vec<String> = value.items().iter().filter_map(keyword_of).collect();
    let joined = keywords.join(" ");
    let display = match joined.as_str() {
        "none" => Display::None,
        "block" | "flow-root" | "block flow" => Display::Block,
        "inline" | "inline flow" => Display::Inline,
        "inline-block" | "inline flow-root" => Display::InlineBlock,
        "flex" | "block flex" => Display::Flex,
        "inline-flex" | "inline flex" => Display::InlineFlex,
        "grid" | "block grid" | "inline-grid" | "inline grid" => Display::Grid,
        "list-item" => Display::ListItem,
        "table" | "inline-table" => Display::Table,
        "table-row-group" | "table-header-group" | "table-footer-group" => Display::TableRowGroup,
        "table-row" => Display::TableRow,
        "table-cell" => Display::TableCell,
        "table-caption" => Display::TableCaption,
        "contents" => Display::Contents,
        // `table-column` and friends generate no box in this engine.
        "table-column" | "table-column-group" => Display::None,
        _ => return None,
    };
    Some(display)
}

fn parse_justify(value: &Value) -> Option<JustifyContent> {
    match keyword_of(value).as_deref() {
        Some("flex-start") | Some("start") | Some("normal") | Some("left") => {
            Some(JustifyContent::Start)
        }
        Some("flex-end") | Some("end") | Some("right") => Some(JustifyContent::End),
        Some("center") => Some(JustifyContent::Center),
        Some("space-between") => Some(JustifyContent::SpaceBetween),
        Some("space-around") => Some(JustifyContent::SpaceAround),
        Some("space-evenly") => Some(JustifyContent::SpaceEvenly),
        _ => None,
    }
}

fn parse_border_style(value: &Value) -> Option<BorderStyle> {
    match keyword_of(value).as_deref() {
        Some("none") => Some(BorderStyle::None),
        Some("hidden") => Some(BorderStyle::Hidden),
        Some("solid") => Some(BorderStyle::Solid),
        Some("dashed") => Some(BorderStyle::Dashed),
        Some("dotted") => Some(BorderStyle::Dotted),
        Some("double") => Some(BorderStyle::Double),
        Some("groove") => Some(BorderStyle::Groove),
        Some("ridge") => Some(BorderStyle::Ridge),
        Some("inset") => Some(BorderStyle::Inset),
        Some("outset") => Some(BorderStyle::Outset),
        _ => None,
    }
}

fn border_width(value: &Value, ctx: &LengthContext) -> Option<f32> {
    match keyword_of(value).as_deref() {
        Some("thin") => Some(1.0),
        Some("medium") => Some(3.0),
        Some("thick") => Some(5.0),
        _ => length(value, ctx),
    }
}

fn parse_list_style_type(value: &Value) -> Option<ListStyleType> {
    match keyword_of(value).as_deref() {
        Some("none") => Some(ListStyleType::None),
        Some("disc") => Some(ListStyleType::Disc),
        Some("circle") => Some(ListStyleType::Circle),
        Some("square") => Some(ListStyleType::Square),
        Some("decimal") => Some(ListStyleType::Decimal),
        Some("lower-alpha") | Some("lower-latin") => Some(ListStyleType::LowerAlpha),
        Some("upper-alpha") | Some("upper-latin") => Some(ListStyleType::UpperAlpha),
        Some("lower-roman") => Some(ListStyleType::LowerRoman),
        Some("upper-roman") => Some(ListStyleType::UpperRoman),
        _ => None,
    }
}

/// Expands the 1-to-4 value edge shorthand.
fn parse_sides<T: Copy>(
    items: &[Value],
    mut parse: impl FnMut(&Value) -> Option<T>,
) -> Option<Sides<T>> {
    let parsed: Vec<T> = items.iter().filter_map(&mut parse).collect();
    match parsed.len() {
        1 => Some(Sides::all(parsed[0])),
        2 => Some(Sides {
            top: parsed[0],
            bottom: parsed[0],
            left: parsed[1],
            right: parsed[1],
        }),
        3 => Some(Sides {
            top: parsed[0],
            left: parsed[1],
            right: parsed[1],
            bottom: parsed[2],
        }),
        4.. => Some(Sides {
            top: parsed[0],
            right: parsed[1],
            bottom: parsed[2],
            left: parsed[3],
        }),
        _ => None,
    }
}

/// Expands the 1-to-4 value corner shorthand.
fn parse_corners<T: Copy>(
    items: &[Value],
    mut parse: impl FnMut(&Value) -> Option<T>,
) -> Option<Corners<T>> {
    // `border-radius: a b / c d` — the vertical radii after `/` are dropped.
    let horizontal: Vec<&Value> = items.iter().take_while(|v| !v.is_keyword("/")).collect();
    let parsed: Vec<T> = horizontal.iter().filter_map(|v| parse(v)).collect();
    match parsed.len() {
        1 => Some(Corners::all(parsed[0])),
        2 => Some(Corners {
            top_left: parsed[0],
            bottom_right: parsed[0],
            top_right: parsed[1],
            bottom_left: parsed[1],
        }),
        3 => Some(Corners {
            top_left: parsed[0],
            top_right: parsed[1],
            bottom_left: parsed[1],
            bottom_right: parsed[2],
        }),
        4.. => Some(Corners {
            top_left: parsed[0],
            top_right: parsed[1],
            bottom_right: parsed[2],
            bottom_left: parsed[3],
        }),
        _ => None,
    }
}

enum Edge {
    All,
    Top,
    Right,
    Bottom,
    Left,
}

fn apply_border_shorthand(
    style: &mut ComputedStyle,
    items: &[Value],
    ctx: &LengthContext,
    edge: Edge,
) {
    // Defaults for the parts the shorthand omits.
    let mut width = 3.0;
    let mut border_style = BorderStyle::None;
    let mut color = style.color;
    let mut saw_width = false;

    for item in items {
        if let Some(parsed) = parse_border_style(item) {
            border_style = parsed;
        } else if let Some(parsed) = border_width(item, ctx) {
            width = parsed;
            saw_width = true;
        } else if let Some(parsed) = item.to_color() {
            color = parsed;
        }
    }
    // `border: solid red` uses the medium width; `border: none` zeroes it.
    if !saw_width && !border_style.is_visible() {
        width = 0.0;
    }

    match edge {
        Edge::All => {
            style.border_width = Sides::all(width);
            style.border_style = Sides::all(border_style);
            style.border_color = Sides::all(color);
        }
        Edge::Top => {
            style.border_width.top = width;
            style.border_style.top = border_style;
            style.border_color.top = color;
        }
        Edge::Right => {
            style.border_width.right = width;
            style.border_style.right = border_style;
            style.border_color.right = color;
        }
        Edge::Bottom => {
            style.border_width.bottom = width;
            style.border_style.bottom = border_style;
            style.border_color.bottom = color;
        }
        Edge::Left => {
            style.border_width.left = width;
            style.border_style.left = border_style;
            style.border_color.left = color;
        }
    }
}

fn apply_flex_shorthand(style: &mut ComputedStyle, items: &[Value], ctx: &LengthContext) {
    if items.len() == 1 {
        if items[0].is_keyword("none") {
            style.flex_grow = 0.0;
            style.flex_shrink = 0.0;
            style.flex_basis = Size::Auto;
            return;
        }
        if items[0].is_keyword("auto") {
            style.flex_grow = 1.0;
            style.flex_shrink = 1.0;
            style.flex_basis = Size::Auto;
            return;
        }
        if let Value::Number(n) = items[0] {
            style.flex_grow = n.max(0.0);
            style.flex_shrink = 1.0;
            // A unitless `flex: 1` sets the basis to 0, not auto.
            style.flex_basis = Size::Definite(LengthPercentage::Px(0.0));
            return;
        }
    }

    let mut numbers = Vec::new();
    let mut basis = None;
    for item in items {
        match item {
            Value::Number(n) => numbers.push(*n),
            other => {
                if let Some(size) = parse_size(other, ctx) {
                    basis = Some(size);
                }
            }
        }
    }
    if let Some(grow) = numbers.first() {
        style.flex_grow = grow.max(0.0);
    }
    if let Some(shrink) = numbers.get(1) {
        style.flex_shrink = shrink.max(0.0);
    } else {
        style.flex_shrink = 1.0;
    }
    style.flex_basis = basis.unwrap_or(Size::Definite(LengthPercentage::Px(0.0)));
}

fn parse_font_size(value: &Value, ctx: &LengthContext, parent_size: f32) -> Option<f32> {
    // Absolute keywords, scaled from the 16px default.
    let keyword_scale = match keyword_of(value).as_deref() {
        Some("xx-small") => Some(0.5625),
        Some("x-small") => Some(0.625),
        Some("small") => Some(0.8125),
        Some("medium") => Some(1.0),
        Some("large") => Some(1.125),
        Some("x-large") => Some(1.5),
        Some("xx-large") => Some(2.0),
        Some("smaller") => return Some(parent_size / 1.2),
        Some("larger") => return Some(parent_size * 1.2),
        _ => None,
    };
    if let Some(scale) = keyword_scale {
        return Some(16.0 * scale);
    }
    match value {
        // Percentages and `em` resolve against the parent's size.
        Value::Percentage(p) => Some(parent_size * p / 100.0),
        other => other.to_px(ctx, Some(parent_size)).filter(|v| *v >= 0.0),
    }
}

fn parse_font_weight(value: &Value, parent_weight: u16) -> Option<u16> {
    match value {
        Value::Number(n) => Some((*n as u16).clamp(1, 1000)),
        _ => match keyword_of(value).as_deref() {
            Some("normal") => Some(400),
            Some("bold") => Some(700),
            Some("lighter") => Some(if parent_weight >= 600 { 400 } else { 100 }),
            Some("bolder") => Some(if parent_weight <= 500 { 700 } else { 900 }),
            _ => None,
        },
    }
}

fn parse_line_height(value: &Value, ctx: &LengthContext) -> Option<LineHeight> {
    if value.is_keyword("normal") {
        return Some(LineHeight::Normal);
    }
    match value {
        Value::Number(n) => Some(LineHeight::Number(*n)),
        Value::Percentage(p) => Some(LineHeight::Number(p / 100.0)),
        other => other.to_px(ctx, None).map(LineHeight::Px),
    }
}

fn parse_font_family(value: &Value) -> Vec<String> {
    value
        .comma_items()
        .iter()
        .filter_map(|item| {
            // An unquoted multi-word family parses as a space-separated list.
            let name = match item {
                Value::List(parts) => parts
                    .iter()
                    .filter_map(Value::as_str_value)
                    .collect::<Vec<_>>()
                    .join(" "),
                other => other.as_str_value()?.to_string(),
            };
            let trimmed = name.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        })
        .collect()
}

/// `font: [style] [weight] size[/line-height] family…`
fn apply_font_shorthand(style: &mut ComputedStyle, value: &Value, ctx: &LengthContext) {
    // System font keywords are not modelled; leave the inherited font alone.
    if matches!(
        keyword_of(value).as_deref(),
        Some("caption")
            | Some("icon")
            | Some("menu")
            | Some("message-box")
            | Some("small-caption")
            | Some("status-bar")
    ) {
        return;
    }

    let segments = value.comma_items();
    let first = segments.first().copied();
    let leading = first.map(Value::items).unwrap_or_default();

    let mut families: Vec<String> = Vec::new();
    let mut size_seen = false;
    let mut expect_line_height = false;
    let mut weight = None;
    let mut font_style = None;

    for item in leading {
        if expect_line_height {
            if let Some(line_height) = parse_line_height(item, ctx) {
                style.line_height = line_height;
            }
            expect_line_height = false;
            continue;
        }
        if item.is_keyword("/") {
            expect_line_height = true;
            continue;
        }
        if !size_seen {
            match keyword_of(item).as_deref() {
                Some("italic") => {
                    font_style = Some(FontStyle::Italic);
                    continue;
                }
                Some("oblique") => {
                    font_style = Some(FontStyle::Oblique);
                    continue;
                }
                Some("normal") | Some("small-caps") => continue,
                _ => {}
            }
            if let Some(parsed) = parse_font_weight(item, style.font_weight) {
                if matches!(item, Value::Number(_)) || item.as_keyword().is_some() {
                    weight = Some(parsed);
                    continue;
                }
            }
            if let Some(size) = parse_font_size(item, ctx, style.font_size) {
                style.font_size = size;
                size_seen = true;
                continue;
            }
        }
        if let Some(name) = item.as_str_value() {
            families.push(name.to_string());
        }
    }

    for segment in segments.iter().skip(1) {
        families.extend(parse_font_family(segment));
    }

    if let Some(weight) = weight {
        style.font_weight = weight;
    }
    if let Some(font_style) = font_style {
        style.font_style = font_style;
    }
    if !families.is_empty() {
        style.font_family = families;
    }
}

fn apply_background_shorthand(style: &mut ComputedStyle, value: &Value, ctx: &LengthContext) {
    // `background: none` resets both layers.
    if value.is_keyword("none") {
        style.background_color = Color::TRANSPARENT;
        style.background_image = BackgroundImage::None;
        return;
    }

    let mut color = None;
    let mut image = None;
    let mut repeat = None;
    let mut size = None;

    for segment in value.comma_items() {
        for item in segment.items() {
            if let Some(parsed) = parse_background_image(item) {
                if !parsed.is_none() {
                    image = Some(parsed);
                    continue;
                }
            }
            match keyword_of(item).as_deref() {
                Some("no-repeat") => repeat = Some(BackgroundRepeat::NoRepeat),
                Some("repeat") => repeat = Some(BackgroundRepeat::Repeat),
                Some("repeat-x") => repeat = Some(BackgroundRepeat::RepeatX),
                Some("repeat-y") => repeat = Some(BackgroundRepeat::RepeatY),
                Some("cover") => size = Some(BackgroundSize::Cover),
                Some("contain") => size = Some(BackgroundSize::Contain),
                Some("center") | Some("top") | Some("bottom") | Some("left") | Some("right")
                | Some("fixed") | Some("scroll") | Some("local") | Some("border-box")
                | Some("padding-box") | Some("content-box") | Some("/") => {}
                _ => {
                    if let Some(parsed) = item.to_color() {
                        color = Some(parsed);
                    } else if length_percentage(item, ctx).is_some() {
                        // Background positions and sizes are not laid out yet.
                    }
                }
            }
        }
    }

    // The shorthand resets the layers it does not mention.
    style.background_color = color.unwrap_or(Color::TRANSPARENT);
    style.background_image = image.unwrap_or(BackgroundImage::None);
    style.background_repeat = repeat.unwrap_or(BackgroundRepeat::Repeat);
    style.background_size = size.unwrap_or(BackgroundSize::Auto);
}

fn parse_background_image(value: &Value) -> Option<BackgroundImage> {
    if value.is_keyword("none") {
        return Some(BackgroundImage::None);
    }
    if let Some(url) = value.as_url() {
        return Some(BackgroundImage::Url(url.to_string()));
    }
    let Value::Function { name, args } = value else {
        return None;
    };
    match name.as_str() {
        "url" => args
            .first()
            .and_then(Value::as_str_value)
            .map(|u| BackgroundImage::Url(u.to_string())),
        "linear-gradient" | "-webkit-linear-gradient" | "repeating-linear-gradient" => {
            let mut angle = 180.0; // `to bottom` is the CSS default
            let mut stops: Vec<GradientStop> = Vec::new();
            let mut index = 0;
            // A leading angle or `to <side>` sets the direction.
            while index < args.len() {
                match &args[index] {
                    Value::Dimension(_, unit) if unit.is_angle() => {
                        angle = args[index].to_degrees().unwrap_or(angle);
                        index += 1;
                    }
                    Value::Keyword(word) if word.eq_ignore_ascii_case("to") => {
                        index += 1;
                        let mut sides = Vec::new();
                        while let Some(Value::Keyword(side)) = args.get(index) {
                            let lower = side.to_ascii_lowercase();
                            if matches!(lower.as_str(), "top" | "bottom" | "left" | "right") {
                                sides.push(lower);
                                index += 1;
                            } else {
                                break;
                            }
                        }
                        angle = match sides.join(" ").as_str() {
                            "top" => 0.0,
                            "right" => 90.0,
                            "bottom" => 180.0,
                            "left" => 270.0,
                            "top right" | "right top" => 45.0,
                            "bottom right" | "right bottom" => 135.0,
                            "bottom left" | "left bottom" => 225.0,
                            "top left" | "left top" => 315.0,
                            _ => angle,
                        };
                    }
                    _ => break,
                }
            }
            for arg in &args[index..] {
                match arg {
                    Value::Percentage(p) => {
                        if let Some(last) = stops.last_mut() {
                            last.position = Some(p / 100.0);
                        }
                    }
                    other => {
                        if let Some(color) = other.to_color() {
                            stops.push(GradientStop {
                                color,
                                position: None,
                            });
                        }
                    }
                }
            }
            (stops.len() >= 2).then_some(BackgroundImage::LinearGradient { angle, stops })
        }
        "radial-gradient" | "repeating-radial-gradient" => {
            let stops: Vec<GradientStop> = args
                .iter()
                .filter_map(Value::to_color)
                .map(|color| GradientStop {
                    color,
                    position: None,
                })
                .collect();
            (stops.len() >= 2).then_some(BackgroundImage::RadialGradient { stops })
        }
        _ => None,
    }
}

fn parse_shadows(value: &Value, ctx: &LengthContext, text_shadow: bool) -> Vec<Shadow> {
    if value.is_keyword("none") {
        return Vec::new();
    }
    let mut shadows = Vec::new();
    for segment in value.comma_items() {
        let mut shadow = Shadow::default();
        let mut lengths = Vec::new();
        let mut saw_color = false;
        for item in segment.items() {
            if item.is_keyword("inset") {
                shadow.inset = true;
                continue;
            }
            if let Some(px) = length(item, ctx) {
                lengths.push(px);
                continue;
            }
            if let Some(color) = item.to_color() {
                shadow.color = color;
                saw_color = true;
            }
        }
        if lengths.len() < 2 {
            continue;
        }
        shadow.offset_x = lengths[0];
        shadow.offset_y = lengths[1];
        shadow.blur = lengths.get(2).copied().unwrap_or(0.0).max(0.0);
        if !text_shadow {
            shadow.spread = lengths.get(3).copied().unwrap_or(0.0);
        }
        if !saw_color {
            shadow.color = Color::rgba(0, 0, 0, 100);
        }
        shadows.push(shadow);
    }
    shadows
}

fn parse_filter(value: &Value, ctx: &LengthContext) -> Option<Filter> {
    if value.is_keyword("none") {
        return Some(Filter::NONE);
    }
    let mut filter = Filter::NONE;
    let mut matched = false;
    for item in value.items() {
        let Value::Function { name, args } = item else {
            continue;
        };
        let first = args.first();
        match name.as_str() {
            "blur" => {
                if let Some(radius) = first.and_then(|v| length(v, ctx)) {
                    filter.blur = radius.max(0.0);
                    matched = true;
                }
            }
            "saturate" => {
                if let Some(amount) = first.and_then(ratio_of) {
                    filter.saturate = amount.max(0.0);
                    matched = true;
                }
            }
            "brightness" => {
                if let Some(amount) = first.and_then(ratio_of) {
                    filter.brightness = amount.max(0.0);
                    matched = true;
                }
            }
            "contrast" => {
                if let Some(amount) = first.and_then(ratio_of) {
                    filter.contrast = amount.max(0.0);
                    matched = true;
                }
            }
            "grayscale" => {
                if let Some(amount) = first.and_then(ratio_of) {
                    filter.grayscale = amount.clamp(0.0, 1.0);
                    matched = true;
                }
            }
            _ => {}
        }
    }
    matched.then_some(filter)
}

/// A filter amount, given as a number or a percentage.
fn ratio_of(value: &Value) -> Option<f32> {
    match value {
        Value::Number(n) => Some(*n),
        Value::Percentage(p) => Some(p / 100.0),
        _ => None,
    }
}

fn parse_transform(value: &Value, ctx: &LengthContext) -> Option<Transform> {
    if value.is_keyword("none") {
        return Some(Transform::IDENTITY);
    }
    let mut transform = Transform::IDENTITY;
    let mut matched = false;
    for item in value.items() {
        let Value::Function { name, args } = item else {
            continue;
        };
        let arg = |index: usize| args.get(index);
        match name.as_str() {
            "translate" | "translate3d" => {
                if let Some(x) = arg(0).and_then(|v| length_percentage(v, ctx)) {
                    transform.translate_x = x;
                    matched = true;
                }
                if let Some(y) = arg(1).and_then(|v| length_percentage(v, ctx)) {
                    transform.translate_y = y;
                }
            }
            "translatex" => {
                if let Some(x) = arg(0).and_then(|v| length_percentage(v, ctx)) {
                    transform.translate_x = x;
                    matched = true;
                }
            }
            "translatey" => {
                if let Some(y) = arg(0).and_then(|v| length_percentage(v, ctx)) {
                    transform.translate_y = y;
                    matched = true;
                }
            }
            "scale" => {
                if let Some(x) = arg(0).and_then(ratio_of) {
                    transform.scale_x = x;
                    transform.scale_y = arg(1).and_then(ratio_of).unwrap_or(x);
                    matched = true;
                }
            }
            "scalex" => {
                if let Some(x) = arg(0).and_then(ratio_of) {
                    transform.scale_x = x;
                    matched = true;
                }
            }
            "scaley" => {
                if let Some(y) = arg(0).and_then(ratio_of) {
                    transform.scale_y = y;
                    matched = true;
                }
            }
            "rotate" | "rotatez" => {
                if let Some(degrees) = arg(0).and_then(Value::to_degrees) {
                    transform.rotate = degrees;
                    matched = true;
                }
            }
            _ => {}
        }
    }
    matched.then_some(transform)
}

fn parse_tracks(value: &Value, ctx: &LengthContext) -> Vec<TrackSize> {
    let mut tracks = Vec::new();
    for item in value.items() {
        match item {
            Value::Dimension(n, wat_css::Unit::Fr) => tracks.push(TrackSize::Fraction(*n)),
            Value::Function { name, args } if name == "repeat" => {
                let count = args.first().and_then(Value::as_number).unwrap_or(1.0) as usize;
                let inner: Vec<TrackSize> = args[1..]
                    .iter()
                    .flat_map(|v| parse_tracks(v, ctx))
                    .collect();
                for _ in 0..count.min(64) {
                    tracks.extend(inner.iter().copied());
                }
            }
            Value::Function { name, args } if name == "minmax" => {
                // Use the maximum, which is what a spare grid does anyway.
                if let Some(track) = args.get(1).map(|v| single_track(v, ctx)) {
                    tracks.push(track);
                }
            }
            other => tracks.push(single_track(other, ctx)),
        }
    }
    tracks
}

fn single_track(value: &Value, ctx: &LengthContext) -> TrackSize {
    match keyword_of(value).as_deref() {
        Some("auto") => return TrackSize::Auto,
        Some("min-content") => return TrackSize::MinContent,
        Some("max-content") => return TrackSize::MaxContent,
        _ => {}
    }
    if let Value::Dimension(n, wat_css::Unit::Fr) = value {
        return TrackSize::Fraction(*n);
    }
    match length_percentage(value, ctx) {
        Some(length) => TrackSize::Length(length),
        None => TrackSize::Auto,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wat_css::parse_inline_style;

    /// Applies a declaration list to a fresh style.
    fn style_of(css: &str) -> ComputedStyle {
        style_with_parent(css, &ComputedStyle::initial())
    }

    fn style_with_parent(css: &str, parent: &ComputedStyle) -> ComputedStyle {
        let mut style = ComputedStyle::inherit_from(parent);
        let ctx = LengthContext::new(style.font_size, 16.0, (1000.0, 800.0));
        // Font size first, so `em` in later declarations sees the right basis.
        let declarations = parse_inline_style(css);
        for declaration in declarations.iter().filter(|d| d.name == "font-size") {
            apply_declaration(&mut style, parent, declaration, &ctx);
        }
        let ctx = LengthContext::new(style.font_size, 16.0, (1000.0, 800.0));
        for declaration in declarations.iter().filter(|d| d.name != "font-size") {
            apply_declaration(&mut style, parent, declaration, &ctx);
        }
        style
    }

    #[test]
    fn box_model_longhands() {
        let style = style_of("width: 200px; height: 50%; padding-left: 8px");
        assert_eq!(style.width, Size::Definite(LengthPercentage::Px(200.0)));
        assert_eq!(
            style.height,
            Size::Definite(LengthPercentage::Percent(50.0))
        );
        assert_eq!(style.padding.left, LengthPercentage::Px(8.0));
    }

    #[test]
    fn edge_shorthand_expansion() {
        let one = style_of("margin: 5px");
        assert_eq!(one.margin.top, Margin::Length(LengthPercentage::Px(5.0)));
        assert_eq!(one.margin.left, Margin::Length(LengthPercentage::Px(5.0)));

        let two = style_of("padding: 1px 2px");
        assert_eq!(two.padding.top, LengthPercentage::Px(1.0));
        assert_eq!(two.padding.right, LengthPercentage::Px(2.0));
        assert_eq!(two.padding.bottom, LengthPercentage::Px(1.0));
        assert_eq!(two.padding.left, LengthPercentage::Px(2.0));

        let three = style_of("padding: 1px 2px 3px");
        assert_eq!(three.padding.bottom, LengthPercentage::Px(3.0));
        assert_eq!(three.padding.left, LengthPercentage::Px(2.0));

        let four = style_of("padding: 1px 2px 3px 4px");
        assert_eq!(four.padding.left, LengthPercentage::Px(4.0));
    }

    #[test]
    fn auto_margins_survive() {
        let style = style_of("margin: 0 auto");
        assert!(style.margin.left.is_auto());
        assert!(!style.margin.top.is_auto());
    }

    #[test]
    fn border_shorthand() {
        let style = style_of("border: 2px solid #f00");
        assert_eq!(style.border_width, Sides::all(2.0));
        assert_eq!(style.border_style, Sides::all(BorderStyle::Solid));
        assert_eq!(style.border_color.top, Color::rgb(255, 0, 0));

        // Width omitted means medium.
        let medium = style_of("border: solid black");
        assert_eq!(medium.border_width.top, 3.0);

        // `border: none` must not leave a 3px width behind.
        let none = style_of("border: none");
        assert_eq!(none.border_width.top, 0.0);
        assert!(!none.has_visible_border());

        let single = style_of("border-bottom: 1px dashed blue");
        assert_eq!(single.border_width.bottom, 1.0);
        assert_eq!(single.border_width.top, 0.0);
        assert_eq!(single.border_style.bottom, BorderStyle::Dashed);
    }

    #[test]
    fn border_radius_corners() {
        let all = style_of("border-radius: 12px");
        assert_eq!(all.border_radius.top_left, LengthPercentage::Px(12.0));

        let two = style_of("border-radius: 4px 8px");
        assert_eq!(two.border_radius.top_left, LengthPercentage::Px(4.0));
        assert_eq!(two.border_radius.top_right, LengthPercentage::Px(8.0));
        assert_eq!(two.border_radius.bottom_right, LengthPercentage::Px(4.0));

        let elliptical = style_of("border-radius: 10px / 20px");
        assert_eq!(
            elliptical.border_radius.top_left,
            LengthPercentage::Px(10.0)
        );
    }

    #[test]
    fn font_size_units_and_keywords() {
        let parent = {
            let mut p = ComputedStyle::initial();
            p.font_size = 20.0;
            p
        };
        assert_eq!(style_with_parent("font-size: 2em", &parent).font_size, 40.0);
        assert_eq!(
            style_with_parent("font-size: 150%", &parent).font_size,
            30.0
        );
        assert_eq!(
            style_with_parent("font-size: larger", &parent).font_size,
            24.0
        );
        assert_eq!(style_of("font-size: x-large").font_size, 24.0);
        assert_eq!(style_of("font-size: 2rem").font_size, 32.0);
    }

    #[test]
    fn em_lengths_use_the_elements_own_font_size() {
        let style = style_of("font-size: 20px; padding: 1em");
        assert_eq!(style.padding.top, LengthPercentage::Px(20.0));
    }

    #[test]
    fn font_shorthand() {
        let style = style_of("font: italic bold 24px/1.5 \"Segoe UI\", sans-serif");
        assert_eq!(style.font_style, FontStyle::Italic);
        assert_eq!(style.font_weight, 700);
        assert_eq!(style.font_size, 24.0);
        assert_eq!(style.line_height, LineHeight::Number(1.5));
        assert_eq!(style.font_family, vec!["Segoe UI", "sans-serif"]);

        let simple = style_of("font: 12px monospace");
        assert_eq!(simple.font_size, 12.0);
        assert_eq!(simple.font_family, vec!["monospace"]);
    }

    #[test]
    fn font_family_lists() {
        let style = style_of("font-family: Helvetica Neue, Arial, sans-serif");
        assert_eq!(
            style.font_family,
            vec!["Helvetica Neue", "Arial", "sans-serif"]
        );
    }

    #[test]
    fn flex_shorthand_forms() {
        let one = style_of("flex: 1");
        assert_eq!(one.flex_grow, 1.0);
        assert_eq!(one.flex_shrink, 1.0);
        assert_eq!(one.flex_basis, Size::Definite(LengthPercentage::Px(0.0)));

        let none = style_of("flex: none");
        assert_eq!(none.flex_grow, 0.0);
        assert_eq!(none.flex_shrink, 0.0);
        assert_eq!(none.flex_basis, Size::Auto);

        let full = style_of("flex: 2 0 120px");
        assert_eq!(full.flex_grow, 2.0);
        assert_eq!(full.flex_shrink, 0.0);
        assert_eq!(full.flex_basis, Size::Definite(LengthPercentage::Px(120.0)));

        let auto = style_of("flex: auto");
        assert_eq!(auto.flex_basis, Size::Auto);
        assert_eq!(auto.flex_grow, 1.0);
    }

    #[test]
    fn gap_shorthand() {
        let both = style_of("gap: 8px");
        assert_eq!(both.row_gap, LengthPercentage::Px(8.0));
        assert_eq!(both.column_gap, LengthPercentage::Px(8.0));

        let split = style_of("gap: 4px 16px");
        assert_eq!(split.row_gap, LengthPercentage::Px(4.0));
        assert_eq!(split.column_gap, LengthPercentage::Px(16.0));
    }

    #[test]
    fn background_shorthand_resets_layers() {
        let style = style_of("background: #123456 no-repeat");
        assert_eq!(style.background_color, Color::rgb(0x12, 0x34, 0x56));
        assert_eq!(style.background_repeat, BackgroundRepeat::NoRepeat);

        let with_url = style_of("background: url(bg.png) center / cover no-repeat #000");
        assert_eq!(
            with_url.background_image,
            BackgroundImage::Url("bg.png".into())
        );
        assert_eq!(with_url.background_size, BackgroundSize::Cover);
        assert_eq!(with_url.background_color, Color::BLACK);
    }

    #[test]
    fn linear_gradients() {
        let style = style_of("background: linear-gradient(to bottom, #fff, rgba(0,0,0,0.5) 80%)");
        match &style.background_image {
            BackgroundImage::LinearGradient { angle, stops } => {
                assert_eq!(*angle, 180.0);
                assert_eq!(stops.len(), 2);
                assert_eq!(stops[0].color, Color::WHITE);
                assert_eq!(stops[1].position, Some(0.8));
            }
            other => panic!("expected a gradient, got {other:?}"),
        }

        let angled = style_of("background-image: linear-gradient(135deg, red, blue)");
        match &angled.background_image {
            BackgroundImage::LinearGradient { angle, .. } => assert_eq!(*angle, 135.0),
            other => panic!("expected a gradient, got {other:?}"),
        }
    }

    #[test]
    fn shadows() {
        let style = style_of("box-shadow: 0 4px 12px rgba(0,0,0,0.25), inset 0 1px 0 #fff");
        assert_eq!(style.box_shadow.len(), 2);
        assert_eq!(style.box_shadow[0].offset_y, 4.0);
        assert_eq!(style.box_shadow[0].blur, 12.0);
        assert_eq!(style.box_shadow[0].color, Color::rgba(0, 0, 0, 64));
        assert!(style.box_shadow[1].inset);

        let with_spread = style_of("box-shadow: 1px 2px 3px 4px black");
        assert_eq!(with_spread.box_shadow[0].spread, 4.0);

        assert!(style_of("box-shadow: none").box_shadow.is_empty());
    }

    #[test]
    fn backdrop_filter_is_the_glass_primitive() {
        let style = style_of("backdrop-filter: blur(24px) saturate(180%)");
        assert_eq!(style.backdrop_filter.blur, 24.0);
        assert_eq!(style.backdrop_filter.saturate, 1.8);
        assert!(!style.backdrop_filter.is_none());

        let prefixed = style_of("-webkit-backdrop-filter: blur(8px)");
        assert_eq!(prefixed.backdrop_filter.blur, 8.0);
    }

    #[test]
    fn transforms() {
        let style = style_of("transform: translate(10px, 20px) scale(2) rotate(45deg)");
        assert_eq!(style.transform.translate_x, LengthPercentage::Px(10.0));
        assert_eq!(style.transform.translate_y, LengthPercentage::Px(20.0));
        assert_eq!(style.transform.scale_x, 2.0);
        assert_eq!(style.transform.rotate, 45.0);

        let percent = style_of("transform: translateX(-50%)");
        assert_eq!(
            percent.transform.translate_x,
            LengthPercentage::Percent(-50.0)
        );
    }

    #[test]
    fn global_keywords() {
        let mut parent = ComputedStyle::initial();
        parent.color = Color::rgb(9, 9, 9);
        parent.display = Display::Flex;

        let inherited = style_with_parent("display: inherit", &parent);
        assert_eq!(inherited.display, Display::Flex);

        let mut child_parent = ComputedStyle::initial();
        child_parent.color = Color::rgb(1, 2, 3);
        let reset = style_with_parent("color: initial", &child_parent);
        assert_eq!(reset.color, Color::BLACK);

        // `unset` is `inherit` for inherited properties, `initial` otherwise.
        let unset_color = style_with_parent("color: unset", &child_parent);
        assert_eq!(unset_color.color, Color::rgb(1, 2, 3));
        let unset_display = style_with_parent("display: unset", &parent);
        assert_eq!(unset_display.display, Display::Inline);
    }

    #[test]
    fn display_values() {
        assert_eq!(style_of("display: none").display, Display::None);
        assert_eq!(
            style_of("display: inline-block").display,
            Display::InlineBlock
        );
        assert_eq!(style_of("display: flex").display, Display::Flex);
        assert_eq!(style_of("display: grid").display, Display::Grid);
        assert_eq!(style_of("display: table-cell").display, Display::TableCell);
        // Unknown values leave the property alone.
        assert_eq!(style_of("display: ruby").display, Display::Inline);
    }

    #[test]
    fn grid_template_columns() {
        let style = style_of("grid-template-columns: repeat(3, 1fr)");
        assert_eq!(style.grid_template_columns.len(), 3);
        assert!(matches!(
            style.grid_template_columns[0],
            TrackSize::Fraction(f) if f == 1.0
        ));

        let mixed = style_of("grid-template-columns: 200px 1fr auto");
        assert_eq!(mixed.grid_template_columns.len(), 3);
        assert!(matches!(mixed.grid_template_columns[2], TrackSize::Auto));
    }

    #[test]
    fn text_decoration() {
        let style = style_of("text-decoration: underline");
        assert!(style.text_decoration.underline);
        let none = style_of("text-decoration: none");
        assert!(!none.text_decoration.any());
        let colored = style_of("text-decoration: line-through red");
        assert!(colored.text_decoration.line_through);
        assert_eq!(colored.text_decoration_color, Some(Color::rgb(255, 0, 0)));
    }

    #[test]
    fn opacity_clamps() {
        assert_eq!(style_of("opacity: 0.5").opacity, 0.5);
        assert_eq!(style_of("opacity: 50%").opacity, 0.5);
        assert_eq!(style_of("opacity: 5").opacity, 1.0);
        assert_eq!(style_of("opacity: -1").opacity, 0.0);
    }

    #[test]
    fn unknown_properties_are_ignored() {
        let style = style_of("-moz-osx-font-smoothing: grayscale; contain: layout; width: 4px");
        assert_eq!(style.width, Size::Definite(LengthPercentage::Px(4.0)));
    }

    #[test]
    fn inset_properties() {
        let style = style_of("position: absolute; top: 0; right: 10px; bottom: auto");
        assert_eq!(style.position, Position::Absolute);
        assert_eq!(style.inset.top, Some(LengthPercentage::Px(0.0)));
        assert_eq!(style.inset.right, Some(LengthPercentage::Px(10.0)));
        assert_eq!(style.inset.bottom, None, "auto insets stay unset");
    }
}
