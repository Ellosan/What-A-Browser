//! The computed style model.
//!
//! Absolute lengths are resolved to CSS pixels during style computation;
//! percentages stay symbolic because their basis is only known during layout.

use wat_css::Color;

/// A value that is either a resolved pixel length or a percentage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LengthPercentage {
    Px(f32),
    Percent(f32),
}

impl LengthPercentage {
    pub const ZERO: LengthPercentage = LengthPercentage::Px(0.0);

    /// Resolves against `basis`. Percentages with an unknown basis resolve to 0.
    pub fn resolve(self, basis: f32) -> f32 {
        match self {
            LengthPercentage::Px(v) => v,
            LengthPercentage::Percent(p) => basis * p / 100.0,
        }
    }

    /// Resolves only if the value does not need a basis.
    pub fn resolve_definite(self) -> Option<f32> {
        match self {
            LengthPercentage::Px(v) => Some(v),
            LengthPercentage::Percent(_) => None,
        }
    }

    pub fn is_zero(self) -> bool {
        matches!(self, LengthPercentage::Px(v) if v == 0.0)
    }
}

/// `width`, `height` and their min/max variants.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Size {
    Auto,
    Definite(LengthPercentage),
    MinContent,
    MaxContent,
    FitContent,
    /// `min-height: none` / `max-width: none`
    None,
}

impl Size {
    pub fn is_auto(self) -> bool {
        matches!(self, Size::Auto)
    }

    pub fn definite(self, basis: Option<f32>) -> Option<f32> {
        match self {
            Size::Definite(LengthPercentage::Px(v)) => Some(v),
            Size::Definite(LengthPercentage::Percent(p)) => basis.map(|b| b * p / 100.0),
            _ => None,
        }
    }
}

/// A margin, which may be `auto`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Margin {
    Auto,
    Length(LengthPercentage),
}

impl Margin {
    pub const ZERO: Margin = Margin::Length(LengthPercentage::ZERO);

    pub fn resolve(self, basis: f32) -> f32 {
        match self {
            Margin::Auto => 0.0,
            Margin::Length(l) => l.resolve(basis),
        }
    }

    pub fn is_auto(self) -> bool {
        matches!(self, Margin::Auto)
    }
}

/// Four edge values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sides<T> {
    pub top: T,
    pub right: T,
    pub bottom: T,
    pub left: T,
}

impl<T: Copy> Sides<T> {
    pub fn all(value: T) -> Self {
        Sides {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    pub fn map<U: Copy>(self, mut f: impl FnMut(T) -> U) -> Sides<U> {
        Sides {
            top: f(self.top),
            right: f(self.right),
            bottom: f(self.bottom),
            left: f(self.left),
        }
    }
}

impl Sides<f32> {
    pub const ZERO: Sides<f32> = Sides {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    };

    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    pub fn vertical(&self) -> f32 {
        self.top + self.bottom
    }
}

/// Four corner values, clockwise from the top-left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Corners<T> {
    pub top_left: T,
    pub top_right: T,
    pub bottom_right: T,
    pub bottom_left: T,
}

impl<T: Copy> Corners<T> {
    pub fn all(value: T) -> Self {
        Corners {
            top_left: value,
            top_right: value,
            bottom_right: value,
            bottom_left: value,
        }
    }

    pub fn map<U: Copy>(self, mut f: impl FnMut(T) -> U) -> Corners<U> {
        Corners {
            top_left: f(self.top_left),
            top_right: f(self.top_right),
            bottom_right: f(self.bottom_right),
            bottom_left: f(self.bottom_left),
        }
    }
}

impl Corners<f32> {
    pub const ZERO: Corners<f32> = Corners {
        top_left: 0.0,
        top_right: 0.0,
        bottom_right: 0.0,
        bottom_left: 0.0,
    };

    pub fn is_zero(&self) -> bool {
        self.top_left == 0.0
            && self.top_right == 0.0
            && self.bottom_right == 0.0
            && self.bottom_left == 0.0
    }

    pub fn max(&self) -> f32 {
        self.top_left
            .max(self.top_right)
            .max(self.bottom_right)
            .max(self.bottom_left)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Display {
    None,
    Block,
    Inline,
    InlineBlock,
    Flex,
    InlineFlex,
    Grid,
    ListItem,
    Table,
    TableRowGroup,
    TableRow,
    TableCell,
    TableCaption,
    /// The element generates no box but its children participate as if they
    /// were children of its parent.
    Contents,
}

impl Display {
    pub fn is_none(self) -> bool {
        self == Display::None
    }

    /// Does this element take part in inline layout?
    pub fn is_inline_level(self) -> bool {
        matches!(
            self,
            Display::Inline | Display::InlineBlock | Display::InlineFlex
        )
    }

    /// Does the element establish a block container for its children?
    pub fn is_block_container(self) -> bool {
        matches!(
            self,
            Display::Block
                | Display::InlineBlock
                | Display::ListItem
                | Display::Table
                | Display::TableRowGroup
                | Display::TableCell
                | Display::TableCaption
        )
    }

    pub fn is_flex_container(self) -> bool {
        matches!(
            self,
            Display::Flex | Display::InlineFlex | Display::TableRow
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Position {
    Static,
    Relative,
    Absolute,
    Fixed,
    Sticky,
}

impl Position {
    /// Is the box taken out of normal flow?
    pub fn is_out_of_flow(self) -> bool {
        matches!(self, Position::Absolute | Position::Fixed)
    }

    /// Does the box establish a containing block for absolute descendants?
    pub fn is_positioned(self) -> bool {
        self != Position::Static
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Float {
    None,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clear {
    None,
    Left,
    Right,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxSizing {
    ContentBox,
    BorderBox,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overflow {
    Visible,
    Hidden,
    Scroll,
    Auto,
    Clip,
}

impl Overflow {
    /// Should content be clipped to the padding box?
    pub fn clips(self) -> bool {
        !matches!(self, Overflow::Visible)
    }

    pub fn scrollable(self) -> bool {
        matches!(self, Overflow::Scroll | Overflow::Auto)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    Visible,
    Hidden,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorderStyle {
    None,
    Hidden,
    Solid,
    Dashed,
    Dotted,
    Double,
    Groove,
    Ridge,
    Inset,
    Outset,
}

impl BorderStyle {
    pub fn is_visible(self) -> bool {
        !matches!(self, BorderStyle::None | BorderStyle::Hidden)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontStyle {
    Normal,
    Italic,
    Oblique,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAlign {
    Start,
    End,
    Left,
    Right,
    Center,
    Justify,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextTransform {
    None,
    Uppercase,
    Lowercase,
    Capitalize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextDecoration {
    pub underline: bool,
    pub overline: bool,
    pub line_through: bool,
}

impl TextDecoration {
    pub fn any(self) -> bool {
        self.underline || self.overline || self.line_through
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WhiteSpace {
    Normal,
    Nowrap,
    Pre,
    PreWrap,
    PreLine,
}

impl WhiteSpace {
    /// Are newlines and runs of spaces preserved?
    pub fn preserves_spaces(self) -> bool {
        matches!(self, WhiteSpace::Pre | WhiteSpace::PreWrap)
    }

    pub fn preserves_newlines(self) -> bool {
        matches!(
            self,
            WhiteSpace::Pre | WhiteSpace::PreWrap | WhiteSpace::PreLine
        )
    }

    pub fn wraps(self) -> bool {
        !matches!(self, WhiteSpace::Nowrap | WhiteSpace::Pre)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineHeight {
    /// `normal` — a font-relative default.
    Normal,
    /// A multiple of the font size.
    Number(f32),
    Px(f32),
}

impl LineHeight {
    pub fn resolve(self, font_size: f32) -> f32 {
        match self {
            LineHeight::Normal => font_size * 1.2,
            LineHeight::Number(n) => font_size * n,
            LineHeight::Px(v) => v,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerticalAlign {
    Baseline,
    Top,
    Middle,
    Bottom,
    TextTop,
    TextBottom,
    Sub,
    Super,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlexDirection {
    Row,
    RowReverse,
    Column,
    ColumnReverse,
}

impl FlexDirection {
    pub fn is_row(self) -> bool {
        matches!(self, FlexDirection::Row | FlexDirection::RowReverse)
    }

    pub fn is_reverse(self) -> bool {
        matches!(
            self,
            FlexDirection::RowReverse | FlexDirection::ColumnReverse
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlexWrap {
    NoWrap,
    Wrap,
    WrapReverse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JustifyContent {
    Start,
    End,
    Center,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignItems {
    Stretch,
    Start,
    End,
    Center,
    Baseline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignSelf {
    Auto,
    Stretch,
    Start,
    End,
    Center,
    Baseline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListStyleType {
    None,
    Disc,
    Circle,
    Square,
    Decimal,
    LowerAlpha,
    UpperAlpha,
    LowerRoman,
    UpperRoman,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListStylePosition {
    Outside,
    Inside,
}

/// A grid or flex track size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TrackSize {
    /// `1fr`, `2fr`
    Fraction(f32),
    Length(LengthPercentage),
    Auto,
    MinContent,
    MaxContent,
}

/// `box-shadow` / `text-shadow` entry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: Color,
    pub inset: bool,
}

impl Default for Shadow {
    fn default() -> Self {
        Shadow {
            offset_x: 0.0,
            offset_y: 0.0,
            blur: 0.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 100),
            inset: false,
        }
    }
}

/// The subset of `filter` / `backdrop-filter` the painter implements.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Filter {
    /// Gaussian blur radius in pixels.
    pub blur: f32,
    /// 1.0 leaves saturation unchanged.
    pub saturate: f32,
    pub brightness: f32,
    pub contrast: f32,
    /// 0.0 leaves colours unchanged.
    pub grayscale: f32,
}

impl Filter {
    pub const NONE: Filter = Filter {
        blur: 0.0,
        saturate: 1.0,
        brightness: 1.0,
        contrast: 1.0,
        grayscale: 0.0,
    };

    pub fn is_none(&self) -> bool {
        *self == Filter::NONE
    }
}

/// A 2D affine transform, built from the `transform` functions we support.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub translate_x: LengthPercentage,
    pub translate_y: LengthPercentage,
    pub scale_x: f32,
    pub scale_y: f32,
    /// Degrees, clockwise.
    pub rotate: f32,
}

impl Transform {
    pub const IDENTITY: Transform = Transform {
        translate_x: LengthPercentage::Px(0.0),
        translate_y: LengthPercentage::Px(0.0),
        scale_x: 1.0,
        scale_y: 1.0,
        rotate: 0.0,
    };

    pub fn is_identity(&self) -> bool {
        *self == Transform::IDENTITY
    }
}

/// A gradient stop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientStop {
    pub color: Color,
    /// Position along the gradient line, `0.0..=1.0`. `None` means "distribute".
    pub position: Option<f32>,
}

/// The background layers we render.
#[derive(Clone, Debug, PartialEq)]
pub enum BackgroundImage {
    None,
    Url(String),
    LinearGradient {
        /// Degrees, `0` pointing up, per CSS.
        angle: f32,
        stops: Vec<GradientStop>,
    },
    RadialGradient {
        stops: Vec<GradientStop>,
    },
}

impl BackgroundImage {
    pub fn is_none(&self) -> bool {
        matches!(self, BackgroundImage::None)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundSize {
    Auto,
    Cover,
    Contain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundRepeat {
    Repeat,
    NoRepeat,
    RepeatX,
    RepeatY,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cursor {
    Auto,
    Default,
    Pointer,
    Text,
    Move,
    NotAllowed,
    Grab,
}

/// The fully resolved style of one element.
#[derive(Clone, Debug, PartialEq)]
pub struct ComputedStyle {
    // Box generation and flow.
    pub display: Display,
    pub position: Position,
    pub float: Float,
    pub clear: Clear,
    pub box_sizing: BoxSizing,
    pub visibility: Visibility,
    pub overflow_x: Overflow,
    pub overflow_y: Overflow,
    pub z_index: Option<i32>,

    // Box model.
    pub width: Size,
    pub height: Size,
    pub min_width: Size,
    pub max_width: Size,
    pub min_height: Size,
    pub max_height: Size,
    pub margin: Sides<Margin>,
    pub padding: Sides<LengthPercentage>,
    pub border_width: Sides<f32>,
    pub border_style: Sides<BorderStyle>,
    pub border_color: Sides<Color>,
    pub border_radius: Corners<LengthPercentage>,
    pub inset: Sides<Option<LengthPercentage>>,

    // Painting.
    pub background_color: Color,
    pub background_image: BackgroundImage,
    pub background_size: BackgroundSize,
    pub background_repeat: BackgroundRepeat,
    pub box_shadow: Vec<Shadow>,
    pub text_shadow: Vec<Shadow>,
    pub opacity: f32,
    pub filter: Filter,
    /// The property that makes the Liquid Glass theme possible.
    pub backdrop_filter: Filter,
    pub transform: Transform,

    // Text (inherited).
    pub color: Color,
    pub font_family: Vec<String>,
    pub font_size: f32,
    pub font_weight: u16,
    pub font_style: FontStyle,
    pub line_height: LineHeight,
    pub letter_spacing: f32,
    pub word_spacing: f32,
    pub text_align: TextAlign,
    pub text_decoration: TextDecoration,
    pub text_decoration_color: Option<Color>,
    pub text_transform: TextTransform,
    pub text_indent: LengthPercentage,
    pub white_space: WhiteSpace,
    pub vertical_align: VerticalAlign,
    pub cursor: Cursor,

    // Flex and grid.
    pub flex_direction: FlexDirection,
    pub flex_wrap: FlexWrap,
    pub justify_content: JustifyContent,
    pub align_items: AlignItems,
    pub align_content: JustifyContent,
    pub align_self: AlignSelf,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub flex_basis: Size,
    pub order: i32,
    pub row_gap: LengthPercentage,
    pub column_gap: LengthPercentage,
    pub grid_template_columns: Vec<TrackSize>,

    // Lists.
    pub list_style_type: ListStyleType,
    pub list_style_position: ListStylePosition,
}

impl Default for ComputedStyle {
    fn default() -> Self {
        ComputedStyle::initial()
    }
}

impl ComputedStyle {
    /// The initial value of every property, per CSS.
    pub fn initial() -> Self {
        ComputedStyle {
            display: Display::Inline,
            position: Position::Static,
            float: Float::None,
            clear: Clear::None,
            box_sizing: BoxSizing::ContentBox,
            visibility: Visibility::Visible,
            overflow_x: Overflow::Visible,
            overflow_y: Overflow::Visible,
            z_index: None,

            width: Size::Auto,
            height: Size::Auto,
            min_width: Size::Auto,
            max_width: Size::None,
            min_height: Size::Auto,
            max_height: Size::None,
            margin: Sides::all(Margin::ZERO),
            padding: Sides::all(LengthPercentage::ZERO),
            border_width: Sides::ZERO,
            border_style: Sides::all(BorderStyle::None),
            border_color: Sides::all(Color::BLACK),
            border_radius: Corners::all(LengthPercentage::ZERO),
            inset: Sides::all(None),

            background_color: Color::TRANSPARENT,
            background_image: BackgroundImage::None,
            background_size: BackgroundSize::Auto,
            background_repeat: BackgroundRepeat::Repeat,
            box_shadow: Vec::new(),
            text_shadow: Vec::new(),
            opacity: 1.0,
            filter: Filter::NONE,
            backdrop_filter: Filter::NONE,
            transform: Transform::IDENTITY,

            color: Color::BLACK,
            font_family: Vec::new(),
            font_size: 16.0,
            font_weight: 400,
            font_style: FontStyle::Normal,
            line_height: LineHeight::Normal,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            text_align: TextAlign::Start,
            text_decoration: TextDecoration::default(),
            text_decoration_color: None,
            text_transform: TextTransform::None,
            text_indent: LengthPercentage::ZERO,
            white_space: WhiteSpace::Normal,
            vertical_align: VerticalAlign::Baseline,
            cursor: Cursor::Auto,

            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::NoWrap,
            justify_content: JustifyContent::Start,
            align_items: AlignItems::Stretch,
            align_content: JustifyContent::Start,
            align_self: AlignSelf::Auto,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: Size::Auto,
            order: 0,
            row_gap: LengthPercentage::ZERO,
            column_gap: LengthPercentage::ZERO,
            grid_template_columns: Vec::new(),

            list_style_type: ListStyleType::Disc,
            list_style_position: ListStylePosition::Outside,
        }
    }

    /// A child's starting style: inherited properties copied, the rest initial.
    pub fn inherit_from(parent: &ComputedStyle) -> Self {
        ComputedStyle {
            color: parent.color,
            font_family: parent.font_family.clone(),
            font_size: parent.font_size,
            font_weight: parent.font_weight,
            font_style: parent.font_style,
            line_height: parent.line_height,
            letter_spacing: parent.letter_spacing,
            word_spacing: parent.word_spacing,
            text_align: parent.text_align,
            text_transform: parent.text_transform,
            text_indent: parent.text_indent,
            white_space: parent.white_space,
            visibility: parent.visibility,
            cursor: parent.cursor,
            list_style_type: parent.list_style_type,
            list_style_position: parent.list_style_position,
            // `text-decoration` is not inherited in CSS, but decorations
            // propagate to descendant inline boxes; the painter relies on this.
            text_decoration: parent.text_decoration,
            text_decoration_color: parent.text_decoration_color,
            text_shadow: parent.text_shadow.clone(),
            ..ComputedStyle::initial()
        }
    }

    /// Resolved line height in pixels.
    pub fn line_height_px(&self) -> f32 {
        self.line_height.resolve(self.font_size)
    }

    /// Does this element paint anything of its own?
    pub fn has_visible_background(&self) -> bool {
        !self.background_color.is_transparent() || !self.background_image.is_none()
    }

    pub fn has_visible_border(&self) -> bool {
        let widths = self.border_width;
        let styles = self.border_style;
        (widths.top > 0.0 && styles.top.is_visible())
            || (widths.right > 0.0 && styles.right.is_visible())
            || (widths.bottom > 0.0 && styles.bottom.is_visible())
            || (widths.left > 0.0 && styles.left.is_visible())
    }

    /// Border widths, zeroed where the style is `none`/`hidden`.
    pub fn used_border_width(&self) -> Sides<f32> {
        Sides {
            top: if self.border_style.top.is_visible() {
                self.border_width.top
            } else {
                0.0
            },
            right: if self.border_style.right.is_visible() {
                self.border_width.right
            } else {
                0.0
            },
            bottom: if self.border_style.bottom.is_visible() {
                self.border_width.bottom
            } else {
                0.0
            },
            left: if self.border_style.left.is_visible() {
                self.border_width.left
            } else {
                0.0
            },
        }
    }

    /// Establishes a new stacking context, so descendants cannot paint outside.
    pub fn creates_stacking_context(&self) -> bool {
        self.opacity < 1.0
            || !self.transform.is_identity()
            || !self.filter.is_none()
            || !self.backdrop_filter.is_none()
            || (self.position.is_positioned() && self.z_index.is_some())
            || self.position == Position::Fixed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_percentage_resolution() {
        assert_eq!(LengthPercentage::Px(10.0).resolve(100.0), 10.0);
        assert_eq!(LengthPercentage::Percent(25.0).resolve(200.0), 50.0);
        assert_eq!(LengthPercentage::Px(4.0).resolve_definite(), Some(4.0));
        assert_eq!(LengthPercentage::Percent(4.0).resolve_definite(), None);
    }

    #[test]
    fn sides_helpers() {
        let sides = Sides {
            top: 1.0,
            right: 2.0,
            bottom: 3.0,
            left: 4.0,
        };
        assert_eq!(sides.horizontal(), 6.0);
        assert_eq!(sides.vertical(), 4.0);
        assert_eq!(Sides::all(2.0).horizontal(), 4.0);
    }

    #[test]
    fn inheritance_copies_text_properties_only() {
        let mut parent = ComputedStyle::initial();
        parent.color = Color::rgb(1, 2, 3);
        parent.font_size = 20.0;
        parent.background_color = Color::WHITE;
        parent.display = Display::Flex;

        let child = ComputedStyle::inherit_from(&parent);
        assert_eq!(child.color, Color::rgb(1, 2, 3));
        assert_eq!(child.font_size, 20.0);
        assert_eq!(
            child.background_color,
            Color::TRANSPARENT,
            "background is not inherited"
        );
        assert_eq!(child.display, Display::Inline, "display is not inherited");
    }

    #[test]
    fn line_height_modes() {
        assert_eq!(LineHeight::Normal.resolve(10.0), 12.0);
        assert_eq!(LineHeight::Number(1.5).resolve(10.0), 15.0);
        assert_eq!(LineHeight::Px(24.0).resolve(10.0), 24.0);
    }

    #[test]
    fn hidden_borders_have_no_used_width() {
        let mut style = ComputedStyle::initial();
        style.border_width = Sides::all(2.0);
        assert_eq!(style.used_border_width(), Sides::ZERO);
        style.border_style = Sides::all(BorderStyle::Solid);
        assert_eq!(style.used_border_width(), Sides::all(2.0));
        assert!(style.has_visible_border());
    }

    #[test]
    fn stacking_context_triggers() {
        let mut style = ComputedStyle::initial();
        assert!(!style.creates_stacking_context());
        style.opacity = 0.5;
        assert!(style.creates_stacking_context());

        let mut glass = ComputedStyle::initial();
        glass.backdrop_filter = Filter {
            blur: 20.0,
            ..Filter::NONE
        };
        assert!(glass.creates_stacking_context());
    }

    #[test]
    fn display_categories() {
        assert!(Display::InlineBlock.is_inline_level());
        assert!(Display::InlineBlock.is_block_container());
        assert!(!Display::Block.is_inline_level());
        assert!(Display::Flex.is_flex_container());
        assert!(
            Display::TableRow.is_flex_container(),
            "rows lay out as flex"
        );
    }

    #[test]
    fn white_space_behaviour() {
        assert!(WhiteSpace::Pre.preserves_newlines());
        assert!(!WhiteSpace::Pre.wraps());
        assert!(WhiteSpace::PreWrap.wraps());
        assert!(!WhiteSpace::Normal.preserves_spaces());
        assert!(WhiteSpace::PreLine.preserves_newlines());
        assert!(!WhiteSpace::PreLine.preserves_spaces());
    }
}
