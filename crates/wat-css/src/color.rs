//! Colour type and CSS colour parsing.

/// A straight (non-premultiplied) sRGB colour with 8-bit channels.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl std::fmt::Debug for Color {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.a == 255 {
            write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            write!(
                f,
                "rgba({}, {}, {}, {:.3})",
                self.r,
                self.g,
                self.b,
                self.a as f32 / 255.0
            )
        }
    }
}

impl Color {
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const WHITE: Color = Color::rgb(255, 255, 255);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Color { r, g, b, a }
    }

    /// From floating point channels in `0.0..=1.0`.
    pub fn from_f32(r: f32, g: f32, b: f32, a: f32) -> Self {
        fn channel(v: f32) -> u8 {
            (v.clamp(0.0, 1.0) * 255.0).round() as u8
        }
        Color {
            r: channel(r),
            g: channel(g),
            b: channel(b),
            a: channel(a),
        }
    }

    pub fn alpha_f32(self) -> f32 {
        self.a as f32 / 255.0
    }

    pub fn is_transparent(self) -> bool {
        self.a == 0
    }

    /// Same colour with a new alpha, given as a 0..=1 fraction.
    pub fn with_alpha(self, alpha: f32) -> Self {
        Color {
            a: (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
            ..self
        }
    }

    /// Multiplies the existing alpha by `factor`.
    pub fn scale_alpha(self, factor: f32) -> Self {
        self.with_alpha(self.alpha_f32() * factor)
    }

    /// Linear interpolation in sRGB space; `t == 0` yields `self`.
    pub fn mix(self, other: Color, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Color {
            r: lerp(self.r, other.r),
            g: lerp(self.g, other.g),
            b: lerp(self.b, other.b),
            a: lerp(self.a, other.a),
        }
    }

    /// Composites `self` over `backdrop` using source-over.
    pub fn over(self, backdrop: Color) -> Self {
        let sa = self.alpha_f32();
        if sa >= 1.0 {
            return self;
        }
        if sa <= 0.0 {
            return backdrop;
        }
        let ba = backdrop.alpha_f32();
        let out_a = sa + ba * (1.0 - sa);
        if out_a <= 0.0 {
            return Color::TRANSPARENT;
        }
        let blend = |s: u8, b: u8| {
            let s = s as f32 / 255.0;
            let b = b as f32 / 255.0;
            (s * sa + b * ba * (1.0 - sa)) / out_a
        };
        Color::from_f32(
            blend(self.r, backdrop.r),
            blend(self.g, backdrop.g),
            blend(self.b, backdrop.b),
            out_a,
        )
    }

    /// Perceived luminance in `0.0..=1.0` (Rec. 709 weights).
    pub fn luminance(self) -> f32 {
        (0.2126 * self.r as f32 + 0.7152 * self.g as f32 + 0.0722 * self.b as f32) / 255.0
    }

    /// Multiplies the RGB channels, keeping alpha. `factor > 1` brightens.
    pub fn shade(self, factor: f32) -> Self {
        let ch = |v: u8| ((v as f32 * factor).clamp(0.0, 255.0)).round() as u8;
        Color {
            r: ch(self.r),
            g: ch(self.g),
            b: ch(self.b),
            a: self.a,
        }
    }

    /// `#rrggbb` or `#rrggbbaa`.
    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }

    /// Parses `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()`, `rgba()`,
    /// `hsl()`, `hsla()`, `transparent` or a named colour.
    pub fn parse(input: &str) -> Option<Color> {
        let text = input.trim();
        if let Some(hex) = text.strip_prefix('#') {
            return parse_hex(hex);
        }
        if let Some(open) = text.find('(') {
            let name = text[..open].trim().to_ascii_lowercase();
            let args = text[open + 1..].trim_end().trim_end_matches(')');
            return parse_function(&name, args);
        }
        parse_named(&text.to_ascii_lowercase())
    }
}

pub(crate) fn parse_hex(hex: &str) -> Option<Color> {
    let digits: Vec<u8> = hex
        .chars()
        .map(|c| c.to_digit(16).map(|d| d as u8))
        .collect::<Option<Vec<_>>>()?;
    match digits.len() {
        3 | 4 => {
            let expand = |d: u8| d * 17;
            Some(Color {
                r: expand(digits[0]),
                g: expand(digits[1]),
                b: expand(digits[2]),
                a: digits.get(3).copied().map_or(255, expand),
            })
        }
        6 | 8 => {
            let byte = |i: usize| digits[i] * 16 + digits[i + 1];
            Some(Color {
                r: byte(0),
                g: byte(2),
                b: byte(4),
                a: if digits.len() == 8 { byte(6) } else { 255 },
            })
        }
        _ => None,
    }
}

/// Splits `rgb()`-style arguments on commas or whitespace (and the `/` used for
/// modern alpha syntax).
fn split_args(args: &str) -> Vec<&str> {
    args.split(|c: char| c == ',' || c == '/' || c.is_whitespace())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

/// A channel given as `0..255` or a percentage.
fn channel_value(text: &str) -> Option<f32> {
    if let Some(pct) = text.strip_suffix('%') {
        return Some(pct.trim().parse::<f32>().ok()? / 100.0 * 255.0);
    }
    text.parse::<f32>().ok()
}

fn alpha_value(text: &str) -> f32 {
    if let Some(pct) = text.strip_suffix('%') {
        return pct.trim().parse::<f32>().unwrap_or(100.0) / 100.0;
    }
    text.parse::<f32>().unwrap_or(1.0)
}

pub(crate) fn parse_function(name: &str, args: &str) -> Option<Color> {
    let parts = split_args(args);
    match name {
        "rgb" | "rgba" => {
            if parts.len() < 3 {
                return None;
            }
            let r = channel_value(parts[0])?;
            let g = channel_value(parts[1])?;
            let b = channel_value(parts[2])?;
            let a = parts.get(3).map_or(1.0, |t| alpha_value(t));
            Some(Color::from_f32(r / 255.0, g / 255.0, b / 255.0, a))
        }
        "hsl" | "hsla" => {
            if parts.len() < 3 {
                return None;
            }
            let hue = parse_angle(parts[0])?;
            let saturation = parts[1].trim_end_matches('%').parse::<f32>().ok()? / 100.0;
            let lightness = parts[2].trim_end_matches('%').parse::<f32>().ok()? / 100.0;
            let a = parts.get(3).map_or(1.0, |t| alpha_value(t));
            let (r, g, b) = hsl_to_rgb(hue, saturation.clamp(0.0, 1.0), lightness.clamp(0.0, 1.0));
            Some(Color::from_f32(r, g, b, a))
        }
        _ => None,
    }
}

/// Angle in degrees, accepting `deg`, `rad`, `grad` and `turn`.
fn parse_angle(text: &str) -> Option<f32> {
    let text = text.trim();
    let (number, scale) = if let Some(v) = text.strip_suffix("deg") {
        (v, 1.0)
    } else if let Some(v) = text.strip_suffix("grad") {
        (v, 0.9)
    } else if let Some(v) = text.strip_suffix("turn") {
        (v, 360.0)
    } else if let Some(v) = text.strip_suffix("rad") {
        (v, 180.0 / std::f32::consts::PI)
    } else {
        (text, 1.0)
    };
    Some(number.trim().parse::<f32>().ok()? * scale)
}

pub fn hsl_to_rgb(hue_degrees: f32, saturation: f32, lightness: f32) -> (f32, f32, f32) {
    let hue = hue_degrees.rem_euclid(360.0) / 60.0;
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let secondary = chroma * (1.0 - (hue % 2.0 - 1.0).abs());
    let (r, g, b) = match hue as u32 {
        0 => (chroma, secondary, 0.0),
        1 => (secondary, chroma, 0.0),
        2 => (0.0, chroma, secondary),
        3 => (0.0, secondary, chroma),
        4 => (secondary, 0.0, chroma),
        _ => (chroma, 0.0, secondary),
    };
    let m = lightness - chroma / 2.0;
    (r + m, g + m, b + m)
}

/// The named colours, sorted for binary search.
const NAMED: &[(&str, Color)] = &[
    ("aliceblue", Color::rgb(240, 248, 255)),
    ("antiquewhite", Color::rgb(250, 235, 215)),
    ("aqua", Color::rgb(0, 255, 255)),
    ("aquamarine", Color::rgb(127, 255, 212)),
    ("azure", Color::rgb(240, 255, 255)),
    ("beige", Color::rgb(245, 245, 220)),
    ("bisque", Color::rgb(255, 228, 196)),
    ("black", Color::rgb(0, 0, 0)),
    ("blanchedalmond", Color::rgb(255, 235, 205)),
    ("blue", Color::rgb(0, 0, 255)),
    ("blueviolet", Color::rgb(138, 43, 226)),
    ("brown", Color::rgb(165, 42, 42)),
    ("burlywood", Color::rgb(222, 184, 135)),
    ("cadetblue", Color::rgb(95, 158, 160)),
    ("chartreuse", Color::rgb(127, 255, 0)),
    ("chocolate", Color::rgb(210, 105, 30)),
    ("coral", Color::rgb(255, 127, 80)),
    ("cornflowerblue", Color::rgb(100, 149, 237)),
    ("cornsilk", Color::rgb(255, 248, 220)),
    ("crimson", Color::rgb(220, 20, 60)),
    ("cyan", Color::rgb(0, 255, 255)),
    ("darkblue", Color::rgb(0, 0, 139)),
    ("darkcyan", Color::rgb(0, 139, 139)),
    ("darkgoldenrod", Color::rgb(184, 134, 11)),
    ("darkgray", Color::rgb(169, 169, 169)),
    ("darkgreen", Color::rgb(0, 100, 0)),
    ("darkgrey", Color::rgb(169, 169, 169)),
    ("darkkhaki", Color::rgb(189, 183, 107)),
    ("darkmagenta", Color::rgb(139, 0, 139)),
    ("darkolivegreen", Color::rgb(85, 107, 47)),
    ("darkorange", Color::rgb(255, 140, 0)),
    ("darkorchid", Color::rgb(153, 50, 204)),
    ("darkred", Color::rgb(139, 0, 0)),
    ("darksalmon", Color::rgb(233, 150, 122)),
    ("darkseagreen", Color::rgb(143, 188, 143)),
    ("darkslateblue", Color::rgb(72, 61, 139)),
    ("darkslategray", Color::rgb(47, 79, 79)),
    ("darkslategrey", Color::rgb(47, 79, 79)),
    ("darkturquoise", Color::rgb(0, 206, 209)),
    ("darkviolet", Color::rgb(148, 0, 211)),
    ("deeppink", Color::rgb(255, 20, 147)),
    ("deepskyblue", Color::rgb(0, 191, 255)),
    ("dimgray", Color::rgb(105, 105, 105)),
    ("dimgrey", Color::rgb(105, 105, 105)),
    ("dodgerblue", Color::rgb(30, 144, 255)),
    ("firebrick", Color::rgb(178, 34, 34)),
    ("floralwhite", Color::rgb(255, 250, 240)),
    ("forestgreen", Color::rgb(34, 139, 34)),
    ("fuchsia", Color::rgb(255, 0, 255)),
    ("gainsboro", Color::rgb(220, 220, 220)),
    ("ghostwhite", Color::rgb(248, 248, 255)),
    ("gold", Color::rgb(255, 215, 0)),
    ("goldenrod", Color::rgb(218, 165, 32)),
    ("gray", Color::rgb(128, 128, 128)),
    ("green", Color::rgb(0, 128, 0)),
    ("greenyellow", Color::rgb(173, 255, 47)),
    ("grey", Color::rgb(128, 128, 128)),
    ("honeydew", Color::rgb(240, 255, 240)),
    ("hotpink", Color::rgb(255, 105, 180)),
    ("indianred", Color::rgb(205, 92, 92)),
    ("indigo", Color::rgb(75, 0, 130)),
    ("ivory", Color::rgb(255, 255, 240)),
    ("khaki", Color::rgb(240, 230, 140)),
    ("lavender", Color::rgb(230, 230, 250)),
    ("lavenderblush", Color::rgb(255, 240, 245)),
    ("lawngreen", Color::rgb(124, 252, 0)),
    ("lemonchiffon", Color::rgb(255, 250, 205)),
    ("lightblue", Color::rgb(173, 216, 230)),
    ("lightcoral", Color::rgb(240, 128, 128)),
    ("lightcyan", Color::rgb(224, 255, 255)),
    ("lightgoldenrodyellow", Color::rgb(250, 250, 210)),
    ("lightgray", Color::rgb(211, 211, 211)),
    ("lightgreen", Color::rgb(144, 238, 144)),
    ("lightgrey", Color::rgb(211, 211, 211)),
    ("lightpink", Color::rgb(255, 182, 193)),
    ("lightsalmon", Color::rgb(255, 160, 122)),
    ("lightseagreen", Color::rgb(32, 178, 170)),
    ("lightskyblue", Color::rgb(135, 206, 250)),
    ("lightslategray", Color::rgb(119, 136, 153)),
    ("lightslategrey", Color::rgb(119, 136, 153)),
    ("lightsteelblue", Color::rgb(176, 196, 222)),
    ("lightyellow", Color::rgb(255, 255, 224)),
    ("lime", Color::rgb(0, 255, 0)),
    ("limegreen", Color::rgb(50, 205, 50)),
    ("linen", Color::rgb(250, 240, 230)),
    ("magenta", Color::rgb(255, 0, 255)),
    ("maroon", Color::rgb(128, 0, 0)),
    ("mediumaquamarine", Color::rgb(102, 205, 170)),
    ("mediumblue", Color::rgb(0, 0, 205)),
    ("mediumorchid", Color::rgb(186, 85, 211)),
    ("mediumpurple", Color::rgb(147, 112, 219)),
    ("mediumseagreen", Color::rgb(60, 179, 113)),
    ("mediumslateblue", Color::rgb(123, 104, 238)),
    ("mediumspringgreen", Color::rgb(0, 250, 154)),
    ("mediumturquoise", Color::rgb(72, 209, 204)),
    ("mediumvioletred", Color::rgb(199, 21, 133)),
    ("midnightblue", Color::rgb(25, 25, 112)),
    ("mintcream", Color::rgb(245, 255, 250)),
    ("mistyrose", Color::rgb(255, 228, 225)),
    ("moccasin", Color::rgb(255, 228, 181)),
    ("navajowhite", Color::rgb(255, 222, 173)),
    ("navy", Color::rgb(0, 0, 128)),
    ("oldlace", Color::rgb(253, 245, 230)),
    ("olive", Color::rgb(128, 128, 0)),
    ("olivedrab", Color::rgb(107, 142, 35)),
    ("orange", Color::rgb(255, 165, 0)),
    ("orangered", Color::rgb(255, 69, 0)),
    ("orchid", Color::rgb(218, 112, 214)),
    ("palegoldenrod", Color::rgb(238, 232, 170)),
    ("palegreen", Color::rgb(152, 251, 152)),
    ("paleturquoise", Color::rgb(175, 238, 238)),
    ("palevioletred", Color::rgb(219, 112, 147)),
    ("papayawhip", Color::rgb(255, 239, 213)),
    ("peachpuff", Color::rgb(255, 218, 185)),
    ("peru", Color::rgb(205, 133, 63)),
    ("pink", Color::rgb(255, 192, 203)),
    ("plum", Color::rgb(221, 160, 221)),
    ("powderblue", Color::rgb(176, 224, 230)),
    ("purple", Color::rgb(128, 0, 128)),
    ("rebeccapurple", Color::rgb(102, 51, 153)),
    ("red", Color::rgb(255, 0, 0)),
    ("rosybrown", Color::rgb(188, 143, 143)),
    ("royalblue", Color::rgb(65, 105, 225)),
    ("saddlebrown", Color::rgb(139, 69, 19)),
    ("salmon", Color::rgb(250, 128, 114)),
    ("sandybrown", Color::rgb(244, 164, 96)),
    ("seagreen", Color::rgb(46, 139, 87)),
    ("seashell", Color::rgb(255, 245, 238)),
    ("sienna", Color::rgb(160, 82, 45)),
    ("silver", Color::rgb(192, 192, 192)),
    ("skyblue", Color::rgb(135, 206, 235)),
    ("slateblue", Color::rgb(106, 90, 205)),
    ("slategray", Color::rgb(112, 128, 144)),
    ("slategrey", Color::rgb(112, 128, 144)),
    ("snow", Color::rgb(255, 250, 250)),
    ("springgreen", Color::rgb(0, 255, 127)),
    ("steelblue", Color::rgb(70, 130, 180)),
    ("tan", Color::rgb(210, 180, 140)),
    ("teal", Color::rgb(0, 128, 128)),
    ("thistle", Color::rgb(216, 191, 216)),
    ("tomato", Color::rgb(255, 99, 71)),
    ("transparent", Color::rgba(0, 0, 0, 0)),
    ("turquoise", Color::rgb(64, 224, 208)),
    ("violet", Color::rgb(238, 130, 238)),
    ("wheat", Color::rgb(245, 222, 179)),
    ("white", Color::rgb(255, 255, 255)),
    ("whitesmoke", Color::rgb(245, 245, 245)),
    ("yellow", Color::rgb(255, 255, 0)),
    ("yellowgreen", Color::rgb(154, 205, 50)),
];

pub fn parse_named(name: &str) -> Option<Color> {
    NAMED
        .binary_search_by(|(candidate, _)| (*candidate).cmp(name))
        .ok()
        .map(|index| NAMED[index].1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_table_is_sorted() {
        assert!(NAMED.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn hex_forms() {
        assert_eq!(Color::parse("#f00"), Some(Color::rgb(255, 0, 0)));
        assert_eq!(Color::parse("#ff0000"), Some(Color::rgb(255, 0, 0)));
        assert_eq!(Color::parse("#ff000080"), Some(Color::rgba(255, 0, 0, 128)));
        assert_eq!(Color::parse("#0f08"), Some(Color::rgba(0, 255, 0, 136)));
        assert_eq!(Color::parse("#xyz"), None);
    }

    #[test]
    fn functional_forms() {
        assert_eq!(Color::parse("rgb(1, 2, 3)"), Some(Color::rgb(1, 2, 3)));
        assert_eq!(Color::parse("rgb(1 2 3)"), Some(Color::rgb(1, 2, 3)));
        assert_eq!(
            Color::parse("rgba(0, 0, 0, 0.5)"),
            Some(Color::rgba(0, 0, 0, 128))
        );
        assert_eq!(
            Color::parse("rgb(0 0 0 / 50%)"),
            Some(Color::rgba(0, 0, 0, 128))
        );
        assert_eq!(
            Color::parse("rgb(100%, 0%, 0%)"),
            Some(Color::rgb(255, 0, 0))
        );
    }

    #[test]
    fn hsl_forms() {
        assert_eq!(
            Color::parse("hsl(0, 100%, 50%)"),
            Some(Color::rgb(255, 0, 0))
        );
        assert_eq!(
            Color::parse("hsl(120, 100%, 50%)"),
            Some(Color::rgb(0, 255, 0))
        );
        assert_eq!(
            Color::parse("hsl(240deg 100% 50%)"),
            Some(Color::rgb(0, 0, 255))
        );
        assert_eq!(Color::parse("hsl(0, 0%, 100%)"), Some(Color::WHITE));
    }

    #[test]
    fn named_and_keywords() {
        assert_eq!(
            Color::parse("RebeccaPurple"),
            Some(Color::rgb(102, 51, 153))
        );
        assert_eq!(Color::parse("transparent"), Some(Color::TRANSPARENT));
        assert_eq!(Color::parse("notacolor"), None);
    }

    #[test]
    fn compositing_is_source_over() {
        let half_white = Color::rgba(255, 255, 255, 128);
        let result = half_white.over(Color::BLACK);
        assert_eq!(result.a, 255);
        assert!((127..=129).contains(&result.r), "got {result:?}");
        assert_eq!(Color::TRANSPARENT.over(Color::BLACK), Color::BLACK);
        assert_eq!(Color::WHITE.over(Color::BLACK), Color::WHITE);
    }

    #[test]
    fn luminance_ordering() {
        assert!(Color::WHITE.luminance() > Color::BLACK.luminance());
        assert_eq!(Color::WHITE.luminance(), 1.0);
    }

    #[test]
    fn hex_round_trip() {
        let c = Color::rgba(18, 52, 86, 120);
        assert_eq!(Color::parse(&c.to_hex()), Some(c));
    }
}
