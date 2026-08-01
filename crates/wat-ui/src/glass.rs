//! The Liquid Glass surface primitive.
//!
//! Every panel, pill and button in the chrome is this one function. The layers
//! go down in a fixed order, and each is driven by a theme value, so a theme
//! with `blur = 0` and `sheen = 0` turns the very same call into a flat opaque
//! rectangle:
//!
//! 1. a drop shadow, so the surface reads as floating;
//! 2. a backdrop filter — blur, saturate, brighten — over whatever is behind;
//! 3. a wash of the tint colour;
//! 4. a top-down specular sheen;
//! 5. an inner rim light, which is what makes the edge look like thickness;
//! 6. a hairline outer edge.

use wat_css::Color;
use wat_layout::geom::Rect;
use wat_paint::{DisplayItem, DisplayList, LinearGradient, RoundedRect, ShadowItem};
use wat_style::{Filter, Sides};
use wat_theme::{Glass, ResolvedTheme};

/// How prominent a surface should be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Elevation {
    /// Sits in the plane of its neighbours: an inset field, a tab.
    Flush,
    /// The main floating chrome.
    Raised,
    /// A menu or dialog above everything.
    Overlay,
}

impl Elevation {
    fn shadow_scale(self) -> f32 {
        match self {
            Elevation::Flush => 0.0,
            Elevation::Raised => 1.0,
            Elevation::Overlay => 1.5,
        }
    }

    fn tint_scale(self) -> f32 {
        match self {
            Elevation::Flush => 0.7,
            Elevation::Raised => 1.0,
            Elevation::Overlay => 1.15,
        }
    }
}

/// Draws a glass surface into `list`.
pub fn surface(
    list: &mut DisplayList,
    shape: RoundedRect,
    theme: &ResolvedTheme,
    elevation: Elevation,
) {
    surface_tinted(list, shape, theme, elevation, None);
}

/// A glass surface with an extra colour blended into its wash, used for the
/// active tab and for pressed buttons.
pub fn surface_tinted(
    list: &mut DisplayList,
    shape: RoundedRect,
    theme: &ResolvedTheme,
    elevation: Elevation,
    extra_tint: Option<Color>,
) {
    if shape.rect.is_empty() {
        return;
    }
    let glass = &theme.glass;

    // 1. Shadow.
    let shadow_scale = elevation.shadow_scale();
    if shadow_scale > 0.0 && !glass.shadow.color.is_transparent() {
        list.push(DisplayItem::Shadow {
            shape,
            shadow: ShadowItem {
                offset: (0.0, glass.shadow.offset_y * shadow_scale),
                blur: glass.shadow.blur * shadow_scale,
                spread: glass.shadow.spread,
                color: glass.shadow.color,
                inset: false,
            },
        });
    }

    // 2. Backdrop filter.
    if glass.blur > 0.0
        || (glass.saturation - 1.0).abs() > f32::EPSILON
        || (glass.brightness - 1.0).abs() > f32::EPSILON
    {
        list.push(DisplayItem::BackdropFilter {
            shape,
            filter: Filter {
                blur: glass.blur,
                saturate: glass.saturation,
                brightness: glass.brightness,
                contrast: 1.0,
                grayscale: 0.0,
            },
        });
    }

    // 3. Wash.
    let mut wash = glass.wash();
    wash = wash.with_alpha((wash.alpha_f32() * elevation.tint_scale()).min(1.0));
    if let Some(extra) = extra_tint {
        wash = extra.over(wash);
    }
    if !wash.is_transparent() {
        list.push(DisplayItem::Fill { shape, color: wash });
    }

    // 4. Sheen: a highlight down the top third.
    let sheen_stops = glass.sheen_stops();
    if !sheen_stops.is_empty() {
        // The highlight only occupies the upper part of the surface.
        let sheen_rect = Rect::new(
            shape.rect.x,
            shape.rect.y,
            shape.rect.width,
            shape.rect.height,
        );
        list.push(DisplayItem::Gradient {
            shape,
            gradient: LinearGradient::for_angle(sheen_rect, 180.0, sheen_stops),
        });
    }

    // 5. Rim light, just inside the edge.
    if glass.rim > 0.0 {
        list.push(DisplayItem::Shadow {
            shape,
            shadow: ShadowItem {
                offset: (0.0, 1.0),
                blur: 1.5,
                spread: 0.0,
                color: glass.rim_color.with_alpha(glass.rim),
                inset: true,
            },
        });
    }

    // 6. Outer edge.
    if !glass.edge.is_transparent() && theme.geometry.border_width > 0.0 {
        list.push(DisplayItem::Border {
            shape,
            widths: Sides::all(theme.geometry.border_width),
            colors: Sides::all(glass.edge),
        });
    }
}

/// A recessed surface, for the omnibox field and other inputs.
///
/// Instead of a highlight on top it takes a shadow on the inside, which is what
/// makes it read as carved *into* the glass rather than sitting on it.
pub fn well(list: &mut DisplayList, shape: RoundedRect, theme: &ResolvedTheme) {
    if shape.rect.is_empty() {
        return;
    }
    let base = if theme.dark {
        Color::BLACK.with_alpha(0.22)
    } else {
        Color::WHITE.with_alpha(0.55)
    };
    list.push(DisplayItem::Fill { shape, color: base });
    list.push(DisplayItem::Shadow {
        shape,
        shadow: ShadowItem {
            offset: (0.0, 1.0),
            blur: 3.0,
            spread: 0.0,
            color: if theme.dark {
                Color::BLACK.with_alpha(0.45)
            } else {
                Color::rgba(0x10, 0x14, 0x1c, 0x2e)
            },
            inset: true,
        },
    });
    if theme.geometry.border_width > 0.0 {
        list.push(DisplayItem::Border {
            shape,
            widths: Sides::all(theme.geometry.border_width),
            colors: Sides::all(theme.palette.border),
        });
    }
}

/// A plain fill with no glass at all, for solid accents such as the progress bar.
pub fn solid(list: &mut DisplayList, shape: RoundedRect, color: Color) {
    if shape.rect.is_empty() || color.is_transparent() {
        return;
    }
    list.push(DisplayItem::Fill { shape, color });
}

/// A focus ring drawn just outside `shape`.
pub fn focus_ring(list: &mut DisplayList, shape: RoundedRect, theme: &ResolvedTheme) {
    let ring = shape.expand(2.0);
    list.push(DisplayItem::Border {
        shape: ring,
        widths: Sides::all(2.0),
        colors: Sides::all(theme.palette.accent.with_alpha(0.85)),
    });
}

/// How many display items one glass surface costs, for the given theme. Used to
/// keep an eye on frame complexity.
pub fn surface_item_count(glass: &Glass, elevation: Elevation) -> usize {
    let mut count = 0;
    if elevation.shadow_scale() > 0.0 && !glass.shadow.color.is_transparent() {
        count += 1;
    }
    if glass.blur > 0.0
        || (glass.saturation - 1.0).abs() > f32::EPSILON
        || (glass.brightness - 1.0).abs() > f32::EPSILON
    {
        count += 1;
    }
    if !glass.wash().is_transparent() {
        count += 1;
    }
    if !glass.sheen_stops().is_empty() {
        count += 1;
    }
    if glass.rim > 0.0 {
        count += 1;
    }
    if !glass.edge.is_transparent() {
        count += 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use wat_theme::Theme;

    fn shape() -> RoundedRect {
        RoundedRect::new(
            Rect::new(10.0, 10.0, 200.0, 60.0),
            wat_style::Corners::all(18.0),
        )
    }

    #[test]
    fn a_glass_surface_emits_every_layer() {
        let theme = Theme::default().resolve(false);
        let mut list = DisplayList::new();
        surface(&mut list, shape(), &theme, Elevation::Raised);

        // Shadow, backdrop filter, wash, sheen, rim, edge.
        assert_eq!(list.len(), 6, "got {:?}", list.items);
        assert_eq!(list.backdrop_filter_count(), 1);
        assert!(list.is_balanced());
        assert_eq!(
            list.len(),
            surface_item_count(&theme.glass, Elevation::Raised)
        );
    }

    #[test]
    fn the_layers_go_down_in_the_documented_order() {
        let theme = Theme::default().resolve(false);
        let mut list = DisplayList::new();
        surface(&mut list, shape(), &theme, Elevation::Raised);

        let kinds: Vec<&str> = list
            .items
            .iter()
            .map(|item| match item {
                DisplayItem::Shadow { shadow, .. } if shadow.inset => "rim",
                DisplayItem::Shadow { .. } => "shadow",
                DisplayItem::BackdropFilter { .. } => "filter",
                DisplayItem::Fill { .. } => "wash",
                DisplayItem::Gradient { .. } => "sheen",
                DisplayItem::Border { .. } => "edge",
                _ => "other",
            })
            .collect();
        assert_eq!(
            kinds,
            vec!["shadow", "filter", "wash", "sheen", "rim", "edge"]
        );
    }

    #[test]
    fn a_flat_theme_produces_no_blur_and_no_sheen() {
        let theme = Theme::default().without_glass().resolve(false);
        let mut list = DisplayList::new();
        surface(&mut list, shape(), &theme, Elevation::Raised);

        assert_eq!(list.backdrop_filter_count(), 0, "no glass means no blur");
        assert!(
            !list
                .items
                .iter()
                .any(|item| matches!(item, DisplayItem::Gradient { .. })),
            "no sheen either"
        );
        // But it still paints something opaque.
        assert!(list
            .items
            .iter()
            .any(|item| matches!(item, DisplayItem::Fill { .. })));
    }

    #[test]
    fn elevation_scales_the_shadow() {
        let theme = Theme::default().resolve(false);
        let blur_for = |elevation| {
            let mut list = DisplayList::new();
            surface(&mut list, shape(), &theme, elevation);
            list.items.iter().find_map(|item| match item {
                DisplayItem::Shadow { shadow, .. } if !shadow.inset => Some(shadow.blur),
                _ => None,
            })
        };
        assert_eq!(blur_for(Elevation::Flush), None, "flush casts no shadow");
        let raised = blur_for(Elevation::Raised).expect("a shadow");
        let overlay = blur_for(Elevation::Overlay).expect("a shadow");
        assert!(overlay > raised);
    }

    #[test]
    fn an_extra_tint_changes_the_wash() {
        let theme = Theme::default().resolve(false);
        let wash_of = |tint| {
            let mut list = DisplayList::new();
            surface_tinted(&mut list, shape(), &theme, Elevation::Raised, tint);
            list.items.iter().find_map(|item| match item {
                DisplayItem::Fill { color, .. } => Some(*color),
                _ => None,
            })
        };
        let plain = wash_of(None).unwrap();
        let tinted = wash_of(Some(Color::rgba(255, 0, 0, 120))).unwrap();
        assert_ne!(plain, tinted);
        assert_eq!(plain.r, plain.g, "the default wash is neutral");
        assert!(
            tinted.r > tinted.g,
            "the tint should push the wash towards red: {tinted:?}"
        );
    }

    #[test]
    fn an_empty_shape_draws_nothing() {
        let theme = Theme::default().resolve(false);
        let mut list = DisplayList::new();
        surface(
            &mut list,
            RoundedRect::sharp(Rect::new(0.0, 0.0, 0.0, 10.0)),
            &theme,
            Elevation::Raised,
        );
        assert!(list.is_empty());
    }

    #[test]
    fn a_well_is_recessed_rather_than_raised() {
        let theme = Theme::default().resolve(false);
        let mut list = DisplayList::new();
        well(&mut list, shape(), &theme);
        let inset_shadows = list
            .items
            .iter()
            .filter(|item| matches!(item, DisplayItem::Shadow { shadow, .. } if shadow.inset))
            .count();
        let outer_shadows = list
            .items
            .iter()
            .filter(|item| matches!(item, DisplayItem::Shadow { shadow, .. } if !shadow.inset))
            .count();
        assert_eq!(inset_shadows, 1);
        assert_eq!(outer_shadows, 0);
    }

    #[test]
    fn dark_wells_are_darker_than_light_ones() {
        let base = Theme::default();
        let fill_of = |dark| {
            let mut list = DisplayList::new();
            well(&mut list, shape(), &base.resolve(dark));
            list.items.iter().find_map(|item| match item {
                DisplayItem::Fill { color, .. } => Some(*color),
                _ => None,
            })
        };
        assert!(fill_of(true).unwrap().luminance() < fill_of(false).unwrap().luminance());
    }

    #[test]
    fn the_focus_ring_sits_outside_the_shape() {
        let theme = Theme::default().resolve(false);
        let mut list = DisplayList::new();
        focus_ring(&mut list, shape(), &theme);
        match &list.items[0] {
            DisplayItem::Border { shape: ring, .. } => {
                assert!(ring.rect.width > shape().rect.width);
            }
            other => panic!("expected a border, got {other:?}"),
        }
    }
}
