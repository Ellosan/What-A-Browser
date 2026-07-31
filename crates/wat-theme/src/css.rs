//! Turning a theme into CSS, so the browser's own pages follow it.

use crate::ResolvedTheme;
use std::fmt::Write as _;

/// A stylesheet for internal `about:` pages, derived from `theme`.
///
/// Internal pages are ordinary HTML rendered by the same engine as the web, so
/// theming them is a matter of handing the engine the right CSS.
pub fn theme_stylesheet(theme: &ResolvedTheme) -> String {
    let palette = &theme.palette;
    let glass = &theme.glass;
    let geometry = &theme.geometry;
    let typography = &theme.typography;

    let ui_family = typography
        .ui_family
        .iter()
        .map(|family| {
            if family.contains(' ') {
                format!("\"{family}\"")
            } else {
                family.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let mono_family = typography
        .mono_family
        .iter()
        .map(|family| {
            if family.contains(' ') {
                format!("\"{family}\"")
            } else {
                family.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(", ");

    let mut css = String::with_capacity(2048);
    let _ = write!(
        css,
        "html {{ background: {canvas}; color: {text}; \
         font-family: {ui_family}; font-size: {size_base}px; }}\n\
         body {{ margin: 0; padding: {pad}px; background: transparent; }}\n\
         a {{ color: {accent}; }}\n\
         code, pre, .url {{ font-family: {mono_family}; }}\n\
         h1 {{ font-size: {size_h1}px; font-weight: {weight_bold}; margin: 0 0 {gap}px; }}\n\
         h2 {{ font-size: {size_large}px; font-weight: {weight_medium}; \
               margin: {gap2}px 0 {gap}px; }}\n\
         p {{ margin: 0 0 {gap}px; line-height: 1.55; }}\n\
         .muted {{ color: {text_muted}; }}\n\
         .surface {{ background: {surface}; border: {border_width}px solid {edge}; \
              border-radius: {radius_large}px; padding: {pad2}px; \
              backdrop-filter: blur({blur}px) saturate({saturation}); \
              box-shadow: 0 {shadow_y}px {shadow_blur}px {shadow_spread}px {shadow_color}; }}\n\
         .card {{ background: {surface}; border: {border_width}px solid {border}; \
              border-radius: {radius_medium}px; padding: {pad}px; \
              backdrop-filter: blur({blur_soft}px) saturate({saturation}); }}\n\
         .grid {{ display: grid; grid-template-columns: repeat(3, 1fr); gap: {gap}px; }}\n\
         .pill {{ display: inline-block; background: {accent_soft}; color: {accent}; \
              border-radius: {radius_pill}px; padding: 2px {pad}px; \
              font-size: {size_small}px; }}\n\
         .error {{ color: {danger}; }}\n",
        canvas = palette.canvas.to_hex(),
        text = palette.text.to_hex(),
        text_muted = palette.text_muted.to_hex(),
        accent = palette.accent.to_hex(),
        accent_soft = palette.accent_soft.to_hex(),
        surface = palette.surface.to_hex(),
        border = palette.border.to_hex(),
        edge = glass.edge.to_hex(),
        danger = palette.danger.to_hex(),
        ui_family = ui_family,
        mono_family = mono_family,
        size_base = typography.size_base,
        size_small = typography.size_small,
        size_large = typography.size_large,
        size_h1 = typography.size_large * 1.6,
        weight_medium = typography.weight_medium,
        weight_bold = typography.weight_bold,
        pad = geometry.spacing * 2.0,
        pad2 = geometry.spacing * 3.0,
        gap = geometry.spacing * 1.5,
        gap2 = geometry.spacing * 3.0,
        radius_medium = geometry.radius_medium,
        radius_large = geometry.radius_large,
        radius_pill = geometry.radius_pill,
        border_width = geometry.border_width,
        blur = glass.blur,
        blur_soft = glass.blur * 0.6,
        saturation = glass.saturation,
        shadow_y = glass.shadow.offset_y,
        shadow_blur = glass.shadow.blur,
        shadow_spread = glass.shadow.spread,
        shadow_color = glass.shadow.color.to_hex(),
    );
    css
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Theme;
    use wat_css::{MediaContext, Origin, Stylesheet};

    #[test]
    fn the_generated_stylesheet_parses() {
        let theme = Theme::default().resolve(false);
        let css = theme_stylesheet(&theme);
        let sheet = Stylesheet::parse(&css, Origin::UserAgent);
        assert!(sheet.rule_count() >= 10, "got {} rules", sheet.rule_count());
        assert!(!sheet
            .style_rules(&MediaContext::default())
            .iter()
            .any(|rule| rule.declarations.is_empty()));
    }

    #[test]
    fn palette_colours_appear_in_the_output() {
        let mut theme = Theme::default();
        theme.light.palette.accent = wat_css::Color::rgb(0x12, 0x34, 0x56);
        let css = theme_stylesheet(&theme.resolve(false));
        assert!(css.contains("#123456"), "the accent colour must be used");
    }

    #[test]
    fn dark_and_light_produce_different_css() {
        let theme = Theme::default();
        assert_ne!(
            theme_stylesheet(&theme.resolve(false)),
            theme_stylesheet(&theme.resolve(true))
        );
    }

    #[test]
    fn multi_word_font_families_are_quoted() {
        let theme = Theme::default().resolve(false);
        let css = theme_stylesheet(&theme);
        assert!(css.contains("\"SF Pro Text\""), "got: {css}");
    }

    #[test]
    fn a_flat_theme_asks_for_no_blur() {
        let theme = Theme::default().without_glass().resolve(false);
        let css = theme_stylesheet(&theme);
        assert!(css.contains("blur(0px)"), "got: {css}");
    }
}
