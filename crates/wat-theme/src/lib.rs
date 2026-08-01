//! Theming for What-A-Browser.
//!
//! Everything the browser chrome draws comes from a [`Theme`], and a theme is
//! plain data that round-trips through TOML. The built-in default *is* the
//! Liquid Glass preset, and because every field has a default, a user theme file
//! only needs to list what it wants to change:
//!
//! ```
//! let theme = wat_theme::Theme::from_toml(r##"
//!     name = "Liquid Glass (warm)"
//!     [light.palette]
//!     accent = "#ff7a1a"
//! "##).unwrap();
//! assert_eq!(theme.light.palette.accent, wat_css::Color::rgb(255, 122, 26));
//! // Untouched fields keep the Liquid Glass values.
//! assert!(theme.light.glass.blur > 0.0);
//! ```

mod css;
mod presets;

pub use css::theme_stylesheet;
pub use presets::{preset, preset_names, LIQUID_GLASS, LIQUID_GLASS_DARK};

use serde::{Deserialize, Serialize};
use wat_css::Color;

/// Serialises a [`Color`] as a CSS colour string.
mod color_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use wat_css::Color;

    pub fn serialize<S: Serializer>(color: &Color, serializer: S) -> Result<S::Ok, S::Error> {
        color.to_hex().serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Color, D::Error> {
        let text = String::deserialize(deserializer)?;
        Color::parse(&text)
            .ok_or_else(|| serde::de::Error::custom(format!("`{text}` is not a valid colour")))
    }
}

/// Which variant of a theme to use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    /// Follow the platform preference.
    #[default]
    Auto,
    Light,
    Dark,
}

impl Appearance {
    /// Resolves to a concrete choice given the platform preference.
    pub fn prefers_dark(self, system_prefers_dark: bool) -> bool {
        match self {
            Appearance::Auto => system_prefers_dark,
            Appearance::Light => false,
            Appearance::Dark => true,
        }
    }

    pub fn parse(text: &str) -> Option<Appearance> {
        match text.trim().to_ascii_lowercase().as_str() {
            "auto" | "system" => Some(Appearance::Auto),
            "light" => Some(Appearance::Light),
            "dark" => Some(Appearance::Dark),
            _ => None,
        }
    }
}

/// The colours the chrome draws with.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Palette {
    /// Behind everything, visible through the glass.
    #[serde(with = "color_serde")]
    pub canvas: Color,
    /// Fallback background for web content.
    #[serde(with = "color_serde")]
    pub page: Color,
    /// Solid surface colour, used when glass is disabled.
    #[serde(with = "color_serde")]
    pub surface: Color,
    /// A surface that should read as lifted above its neighbours.
    #[serde(with = "color_serde")]
    pub surface_raised: Color,
    /// Primary text on chrome.
    #[serde(with = "color_serde")]
    pub text: Color,
    /// Secondary text: hints, placeholders, inactive tabs.
    #[serde(with = "color_serde")]
    pub text_muted: Color,
    /// Text drawn on top of the accent colour.
    #[serde(with = "color_serde")]
    pub text_on_accent: Color,
    /// The interactive colour: focus rings, progress, selection.
    #[serde(with = "color_serde")]
    pub accent: Color,
    /// A softened accent for fills behind text.
    #[serde(with = "color_serde")]
    pub accent_soft: Color,
    /// Hairline separators.
    #[serde(with = "color_serde")]
    pub border: Color,
    /// A border that needs to be seen.
    #[serde(with = "color_serde")]
    pub border_strong: Color,
    #[serde(with = "color_serde")]
    pub danger: Color,
    #[serde(with = "color_serde")]
    pub success: Color,
    #[serde(with = "color_serde")]
    pub warning: Color,
}

impl Default for Palette {
    fn default() -> Self {
        // The Liquid Glass light palette.
        Palette {
            canvas: Color::rgb(0xe8, 0xed, 0xf5),
            page: Color::rgb(0xff, 0xff, 0xff),
            surface: Color::rgba(0xff, 0xff, 0xff, 0xd9),
            surface_raised: Color::rgba(0xff, 0xff, 0xff, 0xf2),
            text: Color::rgb(0x10, 0x14, 0x1c),
            text_muted: Color::rgba(0x10, 0x14, 0x1c, 0x99),
            text_on_accent: Color::rgb(0xff, 0xff, 0xff),
            accent: Color::rgb(0x0a, 0x84, 0xff),
            accent_soft: Color::rgba(0x0a, 0x84, 0xff, 0x24),
            border: Color::rgba(0x10, 0x14, 0x1c, 0x1f),
            border_strong: Color::rgba(0x10, 0x14, 0x1c, 0x38),
            danger: Color::rgb(0xff, 0x3b, 0x30),
            success: Color::rgb(0x30, 0xb0, 0x5c),
            warning: Color::rgb(0xff, 0x9f, 0x0a),
        }
    }
}

/// A shadow, in theme terms.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShadowSpec {
    pub offset_y: f32,
    pub blur: f32,
    pub spread: f32,
    #[serde(with = "color_serde")]
    pub color: Color,
}

impl Default for ShadowSpec {
    fn default() -> Self {
        ShadowSpec {
            offset_y: 10.0,
            blur: 28.0,
            spread: -6.0,
            color: Color::rgba(0x08, 0x0c, 0x18, 0x38),
        }
    }
}

/// What makes glass look like glass.
///
/// The chrome composites these in a fixed order: blur and saturate the backdrop,
/// wash it with `tint`, lay a `sheen` highlight down the top, then trace a `rim`
/// light just inside the edge. Every value can be dialled to zero, which turns
/// the same widgets into flat opaque ones.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Glass {
    /// Backdrop blur radius in CSS pixels. Zero disables the glass entirely.
    pub blur: f32,
    /// Backdrop saturation multiplier; above 1 makes colours bloom through.
    pub saturation: f32,
    /// Backdrop brightness multiplier.
    pub brightness: f32,
    /// The wash laid over the blurred backdrop.
    #[serde(with = "color_serde")]
    pub tint: Color,
    /// How much of `tint` to apply, `0.0..=1.0`.
    pub tint_strength: f32,
    /// Strength of the top-down specular highlight.
    pub sheen: f32,
    /// Strength of the inner rim light that gives the edge its thickness.
    pub rim: f32,
    /// Colour of the rim light.
    #[serde(with = "color_serde")]
    pub rim_color: Color,
    /// Outer border colour of a glass surface.
    #[serde(with = "color_serde")]
    pub edge: Color,
    /// Drop shadow cast by glass surfaces.
    pub shadow: ShadowSpec,
}

impl Default for Glass {
    fn default() -> Self {
        Glass {
            blur: 24.0,
            saturation: 1.8,
            brightness: 1.04,
            tint: Color::rgb(0xff, 0xff, 0xff),
            tint_strength: 0.42,
            sheen: 0.34,
            rim: 0.55,
            rim_color: Color::rgb(0xff, 0xff, 0xff),
            edge: Color::rgba(0xff, 0xff, 0xff, 0x59),
            shadow: ShadowSpec::default(),
        }
    }
}

impl Glass {
    /// Is there any glass effect at all?
    pub fn is_enabled(&self) -> bool {
        self.blur > 0.0 || self.tint_strength > 0.0
    }

    /// The wash colour, with `tint_strength` folded into its alpha.
    pub fn wash(&self) -> Color {
        self.tint
            .with_alpha(self.tint_strength * self.tint.alpha_f32())
    }

    /// Gradient stops for the top-down sheen, as `(position, colour)`.
    pub fn sheen_stops(&self) -> Vec<(f32, Color)> {
        if self.sheen <= 0.0 {
            return Vec::new();
        }
        vec![
            (0.0, Color::WHITE.with_alpha(self.sheen)),
            (0.45, Color::WHITE.with_alpha(self.sheen * 0.18)),
            (1.0, Color::WHITE.with_alpha(0.0)),
        ]
    }
}

/// Radii, spacing and the sizes of chrome elements.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Geometry {
    pub radius_small: f32,
    pub radius_medium: f32,
    pub radius_large: f32,
    /// Radius large enough to round a control into a capsule.
    pub radius_pill: f32,
    /// Base spacing unit; most gaps are multiples of it.
    pub spacing: f32,
    pub border_width: f32,
    /// Height of the desktop toolbar.
    pub toolbar_height: f32,
    /// Height of the desktop tab strip.
    pub tab_strip_height: f32,
    /// Height of the mobile top bar.
    pub mobile_top_height: f32,
    /// Height of the mobile bottom bar.
    pub mobile_bottom_height: f32,
    /// Diameter of round toolbar buttons.
    pub control_size: f32,
    /// Thickness of the page load progress bar.
    pub progress_height: f32,
    /// Inset of floating chrome from the window edge.
    pub chrome_inset: f32,
}

impl Default for Geometry {
    fn default() -> Self {
        Geometry {
            radius_small: 8.0,
            radius_medium: 14.0,
            radius_large: 22.0,
            radius_pill: 999.0,
            spacing: 8.0,
            border_width: 1.0,
            toolbar_height: 52.0,
            tab_strip_height: 40.0,
            mobile_top_height: 56.0,
            mobile_bottom_height: 64.0,
            control_size: 34.0,
            progress_height: 3.0,
            chrome_inset: 10.0,
        }
    }
}

/// Fonts and sizes for the chrome.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Typography {
    /// Family stack for interface text.
    pub ui_family: Vec<String>,
    /// Family stack for code and URLs shown verbatim.
    pub mono_family: Vec<String>,
    pub size_small: f32,
    pub size_base: f32,
    pub size_large: f32,
    pub weight_normal: u16,
    pub weight_medium: u16,
    pub weight_bold: u16,
}

impl Default for Typography {
    fn default() -> Self {
        Typography {
            ui_family: vec![
                "Inter".into(),
                "SF Pro Text".into(),
                "Segoe UI".into(),
                "system-ui".into(),
                "sans-serif".into(),
            ],
            mono_family: vec![
                "SF Mono".into(),
                "JetBrains Mono".into(),
                "Consolas".into(),
                "monospace".into(),
            ],
            size_small: 12.0,
            size_base: 14.0,
            size_large: 17.0,
            weight_normal: 400,
            weight_medium: 550,
            weight_bold: 680,
        }
    }
}

/// Animation preferences.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Motion {
    pub enabled: bool,
    /// Seconds for quick feedback such as a hover fade.
    pub duration_fast: f32,
    /// Seconds for a panel opening.
    pub duration_base: f32,
}

impl Default for Motion {
    fn default() -> Self {
        Motion {
            enabled: true,
            duration_fast: 0.12,
            duration_base: 0.24,
        }
    }
}

/// One appearance of a theme.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Variant {
    pub palette: Palette,
    pub glass: Glass,
}

/// A complete theme: shared metrics plus a light and a dark variant.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Theme {
    pub name: String,
    pub appearance: Appearance,
    pub geometry: Geometry,
    pub typography: Typography,
    pub motion: Motion,
    pub light: Variant,
    pub dark: Variant,
}

impl Default for Theme {
    /// The Liquid Glass preset.
    fn default() -> Self {
        Theme {
            name: "Liquid Glass".to_string(),
            appearance: Appearance::Auto,
            geometry: Geometry::default(),
            typography: Typography::default(),
            motion: Motion::default(),
            light: Variant::default(),
            dark: Variant {
                palette: dark_palette(),
                glass: dark_glass(),
            },
        }
    }
}

/// The Liquid Glass dark palette.
fn dark_palette() -> Palette {
    Palette {
        canvas: Color::rgb(0x0b, 0x0e, 0x14),
        page: Color::rgb(0x14, 0x18, 0x20),
        surface: Color::rgba(0x2a, 0x30, 0x3d, 0xcc),
        surface_raised: Color::rgba(0x35, 0x3c, 0x4b, 0xe6),
        text: Color::rgb(0xf2, 0xf5, 0xfa),
        text_muted: Color::rgba(0xf2, 0xf5, 0xfa, 0x8c),
        text_on_accent: Color::rgb(0xff, 0xff, 0xff),
        accent: Color::rgb(0x4c, 0xa6, 0xff),
        accent_soft: Color::rgba(0x4c, 0xa6, 0xff, 0x33),
        border: Color::rgba(0xff, 0xff, 0xff, 0x1a),
        border_strong: Color::rgba(0xff, 0xff, 0xff, 0x33),
        danger: Color::rgb(0xff, 0x6b, 0x60),
        success: Color::rgb(0x4c, 0xd0, 0x7d),
        warning: Color::rgb(0xff, 0xb8, 0x3d),
    }
}

/// The Liquid Glass dark glass: less wash, a cooler rim.
fn dark_glass() -> Glass {
    Glass {
        blur: 28.0,
        saturation: 1.6,
        brightness: 0.92,
        tint: Color::rgb(0x1c, 0x22, 0x2e),
        tint_strength: 0.5,
        sheen: 0.14,
        rim: 0.3,
        rim_color: Color::rgb(0xff, 0xff, 0xff),
        edge: Color::rgba(0xff, 0xff, 0xff, 0x2e),
        shadow: ShadowSpec {
            offset_y: 12.0,
            blur: 34.0,
            spread: -8.0,
            color: Color::rgba(0x00, 0x00, 0x00, 0x8c),
        },
    }
}

/// A theme resolved to one appearance, which is what the chrome draws from.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedTheme {
    pub name: String,
    pub dark: bool,
    pub palette: Palette,
    pub glass: Glass,
    pub geometry: Geometry,
    pub typography: Typography,
    pub motion: Motion,
}

impl Default for ResolvedTheme {
    fn default() -> Self {
        Theme::default().resolve(false)
    }
}

impl ResolvedTheme {
    /// Colour for text at the given emphasis.
    pub fn text_color(&self, muted: bool) -> Color {
        if muted {
            self.palette.text_muted
        } else {
            self.palette.text
        }
    }

    /// The fill for a control, by interaction state.
    pub fn control_fill(&self, hovered: bool, active: bool) -> Color {
        let base = if self.dark {
            Color::WHITE
        } else {
            Color::rgb(0x10, 0x14, 0x1c)
        };
        let alpha = match (hovered, active) {
            (_, true) => 0.18,
            (true, false) => 0.10,
            _ => 0.0,
        };
        base.with_alpha(alpha)
    }

    /// Does this theme want real glass, or flat surfaces?
    pub fn uses_glass(&self) -> bool {
        self.glass.is_enabled()
    }

    /// A surface colour to use when glass is switched off.
    pub fn opaque_surface(&self) -> Color {
        let backdrop = self.palette.canvas;
        self.palette.surface.over(backdrop).with_alpha(1.0)
    }
}

impl Theme {
    /// Parses a theme from TOML. Missing fields keep the Liquid Glass defaults.
    pub fn from_toml(text: &str) -> Result<Theme, String> {
        toml::from_str(text).map_err(|error| error.to_string())
    }

    /// Serialises the theme to TOML.
    pub fn to_toml(&self) -> Result<String, String> {
        toml::to_string_pretty(self).map_err(|error| error.to_string())
    }

    /// Loads a theme file from disk.
    pub fn load(path: &std::path::Path) -> Result<Theme, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        Theme::from_toml(&text)
    }

    /// Loads a named preset, or a theme file if `name` is a path.
    pub fn named(name: &str) -> Result<Theme, String> {
        if let Some(text) = preset(name) {
            return Theme::from_toml(text);
        }
        let path = std::path::Path::new(name);
        if path.exists() {
            return Theme::load(path);
        }
        Err(format!(
            "unknown theme `{name}`; available presets: {}",
            preset_names().join(", ")
        ))
    }

    /// Picks a variant. `system_prefers_dark` only matters for
    /// [`Appearance::Auto`].
    pub fn resolve(&self, system_prefers_dark: bool) -> ResolvedTheme {
        let dark = self.appearance.prefers_dark(system_prefers_dark);
        let variant = if dark { self.dark } else { self.light };
        ResolvedTheme {
            name: self.name.clone(),
            dark,
            palette: variant.palette,
            glass: variant.glass,
            geometry: self.geometry,
            typography: self.typography.clone(),
            motion: self.motion,
        }
    }

    /// Returns a copy with glass effects removed, for slow devices or for users
    /// who find translucency distracting.
    pub fn without_glass(&self) -> Theme {
        let flatten = |mut variant: Variant| {
            variant.glass.blur = 0.0;
            variant.glass.sheen = 0.0;
            variant.glass.rim = 0.0;
            variant.glass.tint_strength = 1.0;
            variant.glass.saturation = 1.0;
            variant.glass.brightness = 1.0;
            variant.palette.surface = variant.palette.surface.with_alpha(1.0);
            variant.palette.surface_raised = variant.palette.surface_raised.with_alpha(1.0);
            variant.glass.tint = variant.palette.surface;
            variant
        };
        Theme {
            name: format!("{} (flat)", self.name),
            light: flatten(self.light),
            dark: flatten(self.dark),
            ..self.clone()
        }
    }

    /// Applies the reduced-motion preference.
    pub fn with_reduced_motion(mut self, reduce: bool) -> Theme {
        if reduce {
            self.motion.enabled = false;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_theme_is_liquid_glass() {
        let theme = Theme::default();
        assert_eq!(theme.name, "Liquid Glass");
        assert!(theme.light.glass.blur > 0.0);
        assert!(theme.light.glass.is_enabled());
        assert!(theme.dark.glass.blur > 0.0);
    }

    #[test]
    fn light_and_dark_variants_differ() {
        let theme = Theme::default();
        let light = theme.resolve(false);
        let dark = theme.resolve(true);
        assert!(!light.dark);
        assert!(dark.dark);
        assert!(light.palette.canvas.luminance() > dark.palette.canvas.luminance());
        assert!(light.palette.text.luminance() < dark.palette.text.luminance());
    }

    #[test]
    fn appearance_overrides_the_system_preference() {
        let mut theme = Theme {
            appearance: Appearance::Light,
            ..Theme::default()
        };
        assert!(!theme.resolve(true).dark);
        theme.appearance = Appearance::Dark;
        assert!(theme.resolve(false).dark);
        theme.appearance = Appearance::Auto;
        assert!(theme.resolve(true).dark);
        assert!(!theme.resolve(false).dark);
    }

    #[test]
    fn appearance_parsing() {
        assert_eq!(Appearance::parse("Dark"), Some(Appearance::Dark));
        assert_eq!(Appearance::parse("system"), Some(Appearance::Auto));
        assert_eq!(Appearance::parse("neon"), None);
    }

    #[test]
    fn partial_toml_only_overrides_what_it_names() {
        let theme = Theme::from_toml(
            r##"
            name = "Custom"
            [light.palette]
            accent = "#00ff88"
            [light.glass]
            blur = 40
            "##,
        )
        .expect("valid theme");

        assert_eq!(theme.name, "Custom");
        assert_eq!(theme.light.palette.accent, Color::rgb(0, 255, 136));
        assert_eq!(theme.light.glass.blur, 40.0);
        // Everything else is still Liquid Glass.
        assert_eq!(theme.light.palette.text, Palette::default().text);
        assert_eq!(theme.light.glass.sheen, Glass::default().sheen);
        assert_eq!(theme.dark, Theme::default().dark);
        assert_eq!(theme.geometry, Geometry::default());
    }

    #[test]
    fn toml_round_trips() {
        let theme = Theme::default();
        let text = theme.to_toml().expect("serialisable");
        let parsed = Theme::from_toml(&text).expect("parseable");
        assert_eq!(parsed, theme);
    }

    #[test]
    fn colours_accept_any_css_syntax() {
        let theme = Theme::from_toml(
            r##"
            [light.palette]
            accent = "rgb(1, 2, 3)"
            text = "rebeccapurple"
            border = "#0f08"
            "##,
        )
        .expect("valid theme");
        assert_eq!(theme.light.palette.accent, Color::rgb(1, 2, 3));
        assert_eq!(theme.light.palette.text, Color::rgb(102, 51, 153));
        assert_eq!(theme.light.palette.border.a, 136);
    }

    #[test]
    fn invalid_input_is_reported_not_ignored() {
        let error = Theme::from_toml("[light.palette]\naccent = \"not-a-colour\"")
            .expect_err("should fail");
        assert!(error.contains("not-a-colour"), "unhelpful error: {error}");

        let error = Theme::from_toml("[light.palette]\nnonsense = 3").expect_err("should fail");
        assert!(error.contains("nonsense"), "unhelpful error: {error}");
    }

    #[test]
    fn glass_can_be_turned_off_entirely() {
        let flat = Theme::default().without_glass();
        let resolved = flat.resolve(false);
        assert!(!resolved.glass.is_enabled() || resolved.glass.blur == 0.0);
        assert_eq!(resolved.glass.blur, 0.0);
        assert_eq!(resolved.glass.sheen, 0.0);
        assert_eq!(
            resolved.palette.surface.a, 255,
            "surfaces become opaque without glass"
        );
    }

    #[test]
    fn wash_folds_strength_into_alpha() {
        let mut glass = Glass {
            tint: Color::WHITE,
            tint_strength: 0.5,
            ..Glass::default()
        };
        assert_eq!(glass.wash().a, 128);
        glass.tint_strength = 0.0;
        assert!(glass.wash().is_transparent());
    }

    #[test]
    fn sheen_stops_fade_downwards() {
        let stops = Glass::default().sheen_stops();
        assert_eq!(stops.len(), 3);
        assert!(stops[0].1.a > stops[1].1.a);
        assert_eq!(stops[2].1.a, 0);

        let flat = Glass {
            sheen: 0.0,
            ..Glass::default()
        };
        assert!(flat.sheen_stops().is_empty());
    }

    #[test]
    fn control_fill_gets_stronger_with_interaction() {
        let theme = ResolvedTheme::default();
        assert!(theme.control_fill(false, false).is_transparent());
        let hovered = theme.control_fill(true, false).a;
        let active = theme.control_fill(true, true).a;
        assert!(hovered > 0 && active > hovered);
    }

    #[test]
    fn opaque_surface_has_no_transparency() {
        let theme = ResolvedTheme::default();
        assert_eq!(theme.opaque_surface().a, 255);
    }

    #[test]
    fn reduced_motion_disables_animation() {
        let theme = Theme::default().with_reduced_motion(true);
        assert!(!theme.motion.enabled);
        assert!(Theme::default().motion.enabled);
    }

    #[test]
    fn unknown_theme_names_list_the_presets() {
        let error = Theme::named("does-not-exist").expect_err("should fail");
        assert!(error.contains("liquid-glass"), "unhelpful error: {error}");
    }
}
