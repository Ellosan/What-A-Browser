//! The themes shipped with the browser.

/// The default theme, as a TOML document.
pub const LIQUID_GLASS: &str = include_str!("../themes/liquid-glass.toml");
/// Liquid Glass pinned to dark.
pub const LIQUID_GLASS_DARK: &str = include_str!("../themes/liquid-glass-dark.toml");
/// Liquid Glass with the colour removed.
pub const LIQUID_GLASS_GRAPHITE: &str = include_str!("../themes/liquid-glass-graphite.toml");
/// Opaque chrome, for slower devices or a plainer look.
pub const FLAT: &str = include_str!("../themes/flat.toml");

const PRESETS: &[(&str, &str)] = &[
    ("liquid-glass", LIQUID_GLASS),
    ("liquid-glass-dark", LIQUID_GLASS_DARK),
    ("liquid-glass-graphite", LIQUID_GLASS_GRAPHITE),
    ("flat", FLAT),
];

/// The TOML source of a named preset.
pub fn preset(name: &str) -> Option<&'static str> {
    let wanted = name.trim().to_ascii_lowercase();
    PRESETS
        .iter()
        .find(|(preset_name, _)| *preset_name == wanted)
        .map(|(_, source)| *source)
}

/// Every preset name, in menu order.
pub fn preset_names() -> Vec<&'static str> {
    PRESETS.iter().map(|(name, _)| *name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Theme;

    #[test]
    fn every_preset_parses() {
        for name in preset_names() {
            let source = preset(name).expect("the preset exists");
            let theme = Theme::from_toml(source)
                .unwrap_or_else(|error| panic!("preset `{name}` failed to parse: {error}"));
            assert!(
                !theme.name.is_empty(),
                "preset `{name}` has no display name"
            );
        }
    }

    #[test]
    fn the_liquid_glass_file_matches_the_built_in_default() {
        // The shipped file spells out every default; if the two ever drift, the
        // file has become misleading documentation.
        let from_file = Theme::from_toml(LIQUID_GLASS).expect("parses");
        assert_eq!(from_file, Theme::default());
    }

    #[test]
    fn preset_lookup_is_case_insensitive() {
        assert!(preset("Liquid-Glass").is_some());
        assert!(preset("  flat  ").is_some());
        assert!(preset("nope").is_none());
    }

    #[test]
    fn the_dark_preset_pins_dark() {
        let theme = Theme::from_toml(LIQUID_GLASS_DARK).expect("parses");
        assert!(theme.resolve(false).dark);
    }

    #[test]
    fn the_flat_preset_has_no_glass() {
        let theme = Theme::from_toml(FLAT).expect("parses");
        assert_eq!(theme.resolve(false).glass.blur, 0.0);
        assert_eq!(theme.resolve(true).glass.sheen, 0.0);
        assert_eq!(theme.resolve(false).palette.surface.a, 255);
    }

    #[test]
    fn graphite_keeps_the_glass_but_drops_the_colour() {
        let theme = Theme::from_toml(LIQUID_GLASS_GRAPHITE).expect("parses");
        let light = theme.resolve(false);
        assert!(light.glass.blur > 0.0, "still glass");
        assert_eq!(light.glass.saturation, 1.0, "but no colour bloom");
    }

    #[test]
    fn named_lookup_finds_presets() {
        assert_eq!(
            Theme::named("liquid-glass").expect("found").name,
            "Liquid Glass"
        );
        assert!(Theme::named("flat").is_ok());
    }
}
