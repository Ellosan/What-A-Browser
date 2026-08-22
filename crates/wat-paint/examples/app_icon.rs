//! Draws the launcher icon with the browser's own rasterizer.
//!
//! `cargo run --release --example app_icon -p wat-paint -- <res directory>`
//!
//! The icons that shipped before this were hand-made bitmaps that filled every
//! pixel of their square — which is why Android's own lint complained about them,
//! and why they looked like a sticker rather than an app. These are drawn instead
//! of drawn *on*: the same `wat-paint` that draws the browser's glass draws the
//! mark, from the same Liquid Glass palette, so the icon cannot drift away from
//! the interface it belongs to.
//!
//! What comes out:
//!
//! - `mipmap-*/ic_launcher.png` — the legacy icon, a glass tile with a margin
//! - `mipmap-*/ic_launcher_round.png` — the same, circular
//! - `mipmap-*/ic_launcher_foreground.png` — the mark alone, in the safe zone of
//!   an adaptive icon, so the launcher may mask it into whatever shape it likes
//! - `mipmap-*/ic_launcher_background.png` — the gradient behind it, full bleed
//! - `mipmap-*/ic_launcher_monochrome.png` — the silhouette, for themed icons

use std::f32::consts::PI;
use std::path::PathBuf;
use std::rc::Rc;

use wat_css::Color;
use wat_layout::geom::Rect;
use wat_paint::canvas::{LinearGradient, RoundedRect};
use wat_paint::display::{DisplayItem, DisplayList, ShadowItem};
use wat_paint::{Canvas, Renderer};
use wat_style::types::{Corners, Sides};
use wat_text::FontStore;

/// The densities Android asks for, and what one dp is worth in each.
const DENSITIES: [(&str, f32); 5] = [
    ("mdpi", 1.0),
    ("hdpi", 1.5),
    ("xhdpi", 2.0),
    ("xxhdpi", 3.0),
    ("xxxhdpi", 4.0),
];

/// A legacy launcher icon is 48dp; an adaptive one is 108dp with the middle
/// 66dp guaranteed visible.
const LEGACY_DP: f32 = 48.0;
const ADAPTIVE_DP: f32 = 108.0;
const SAFE_DP: f32 = 66.0;

fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color { r, g, b, a }
}

/// From `liquid-glass.toml`: the accent, and the violet it runs into.
fn accent() -> Color {
    rgba(0x0A, 0x84, 0xFF, 255)
}

fn accent_deep() -> Color {
    rgba(0x5B, 0x3F, 0xE8, 255)
}

fn rounded(rect: Rect, radius: f32) -> RoundedRect {
    RoundedRect::new(rect, Corners::all(radius))
}

/// The gradient tile everything else sits on.
fn tile(list: &mut DisplayList, rect: Rect, radius: f32) {
    let shape = rounded(rect, radius);
    list.items.push(DisplayItem::Gradient {
        shape,
        gradient: LinearGradient {
            // Top-left to bottom-right, which is where the light comes from in
            // the rest of the interface.
            start: (rect.x, rect.y),
            end: (rect.x + rect.width, rect.y + rect.height),
            stops: vec![(0.0, accent()), (1.0, accent_deep())],
        },
    });

    // The sheen: a bright wash over the top half, fading out by the middle. The
    // same trick the toolbars use, and what stops the tile reading as flat.
    let sheen = Rect::new(rect.x, rect.y, rect.width, rect.height * 0.55);
    list.items.push(DisplayItem::Gradient {
        shape: RoundedRect::new(
            sheen,
            Corners {
                top_left: radius,
                top_right: radius,
                bottom_right: 0.0,
                bottom_left: 0.0,
            },
        ),
        gradient: LinearGradient {
            start: (rect.x, rect.y),
            end: (rect.x, rect.y + rect.height * 0.55),
            stops: vec![
                (0.0, rgba(255, 255, 255, 66)),
                (1.0, rgba(255, 255, 255, 0)),
            ],
        },
    });

    // The rim: brighter at the top, the way an edge catches light.
    list.items.push(DisplayItem::Border {
        shape,
        widths: Sides::all((rect.width * 0.012).max(1.0)),
        colors: Sides {
            top: rgba(255, 255, 255, 128),
            right: rgba(255, 255, 255, 64),
            bottom: rgba(255, 255, 255, 40),
            left: rgba(255, 255, 255, 64),
        },
    });
}

/// The mark: a W drawn as four bars, the way a pen would draw it.
///
/// Built from rotated rounded rectangles rather than from a font, so the icon
/// does not depend on which fonts the machine building it happens to have — and
/// so the strokes can be given the browser's own corner rounding.
fn mark(list: &mut DisplayList, area: Rect, colour: Color, shadow: bool) {
    let thickness = area.width * 0.155;
    let top = area.y;
    let bottom = area.y + area.height;

    // The five points a W passes through, left to right.
    let points = [
        (area.x, top),
        (area.x + area.width * 0.27, bottom),
        (area.x + area.width * 0.5, top + area.height * 0.42),
        (area.x + area.width * 0.73, bottom),
        (area.x + area.width, top),
    ];

    for pair in points.windows(2) {
        let (x1, y1) = pair[0];
        let (x2, y2) = pair[1];
        let dx = x2 - x1;
        let dy = y2 - y1;
        let length = (dx * dx + dy * dy).sqrt() + thickness * 0.35;
        let angle = dy.atan2(dx) * 180.0 / PI;
        let centre = ((x1 + x2) / 2.0, (y1 + y2) / 2.0);

        let bar = rounded(
            Rect::new(
                centre.0 - length / 2.0,
                centre.1 - thickness / 2.0,
                length,
                thickness,
            ),
            thickness / 2.0,
        )
        .rotated(angle);

        if shadow {
            list.items.push(DisplayItem::Shadow {
                shape: bar,
                shadow: ShadowItem {
                    offset: (0.0, thickness * 0.18),
                    blur: thickness * 0.5,
                    spread: 0.0,
                    color: rgba(0, 0, 0, 70),
                    inset: false,
                },
            });
        }
        list.items.push(DisplayItem::Fill {
            shape: bar,
            color: colour,
        });
    }
}

fn draw(width: u32, height: u32, list: &DisplayList, fonts: &FontStore) -> Canvas {
    let mut canvas = Canvas::new(width, height);
    Renderer::new(fonts).render(list, &mut canvas);
    canvas
}

fn write(canvas: &Canvas, path: PathBuf) {
    let png = canvas.to_png().expect("the canvas encodes");
    std::fs::create_dir_all(path.parent().expect("a parent directory")).expect("the directory");
    std::fs::write(&path, png).expect("the icon writes");
    println!("  {}", path.display());
}

fn main() {
    let res: PathBuf = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "android-webview/app/src/main/res".into())
        .into();

    // The renderer wants a font store even when nothing draws text.
    let fonts = Rc::new(FontStore::new());

    for (density, scale) in DENSITIES {
        let legacy = (LEGACY_DP * scale).round() as u32;
        let adaptive = (ADAPTIVE_DP * scale).round() as u32;
        let directory = res.join(format!("mipmap-{density}"));
        println!("{density} ({scale}x):");

        // --- the legacy icon: a tile with a margin, not edge to edge --------
        let inset = legacy as f32 * 0.06;
        let tile_rect = Rect::new(
            inset,
            inset,
            legacy as f32 - inset * 2.0,
            legacy as f32 - inset * 2.0,
        );
        let mut list = DisplayList::default();
        list.items.push(DisplayItem::Shadow {
            shape: rounded(tile_rect, tile_rect.width * 0.24),
            shadow: ShadowItem {
                offset: (0.0, inset * 0.5),
                blur: inset * 1.4,
                spread: 0.0,
                color: rgba(0, 0, 0, 60),
                inset: false,
            },
        });
        tile(&mut list, tile_rect, tile_rect.width * 0.24);
        let mark_rect = Rect::new(
            tile_rect.x + tile_rect.width * 0.22,
            tile_rect.y + tile_rect.height * 0.28,
            tile_rect.width * 0.56,
            tile_rect.height * 0.44,
        );
        mark(&mut list, mark_rect, rgba(255, 255, 255, 255), true);
        write(
            &draw(legacy, legacy, &list, &fonts),
            directory.join("ic_launcher.png"),
        );

        // --- the round icon: the same, as a circle -------------------------
        let mut round = DisplayList::default();
        tile(&mut round, tile_rect, tile_rect.width / 2.0);
        mark(&mut round, mark_rect, rgba(255, 255, 255, 255), true);
        write(
            &draw(legacy, legacy, &round, &fonts),
            directory.join("ic_launcher_round.png"),
        );

        // --- adaptive: background full bleed, mark inside the safe zone ----
        let full = Rect::new(0.0, 0.0, adaptive as f32, adaptive as f32);
        let mut background = DisplayList::default();
        tile(&mut background, full, 0.0);
        write(
            &draw(adaptive, adaptive, &background, &fonts),
            directory.join("ic_launcher_background.png"),
        );

        let safe = SAFE_DP * scale;
        let margin = (adaptive as f32 - safe) / 2.0;
        let foreground_mark = Rect::new(
            margin + safe * 0.1,
            margin + safe * 0.28,
            safe * 0.8,
            safe * 0.44,
        );
        let mut foreground = DisplayList::default();
        mark(
            &mut foreground,
            foreground_mark,
            rgba(255, 255, 255, 255),
            true,
        );
        write(
            &draw(adaptive, adaptive, &foreground, &fonts),
            directory.join("ic_launcher_foreground.png"),
        );

        // Themed icons are tinted by the launcher, so this one is a silhouette.
        let mut monochrome = DisplayList::default();
        mark(
            &mut monochrome,
            foreground_mark,
            rgba(255, 255, 255, 255),
            false,
        );
        write(
            &draw(adaptive, adaptive, &monochrome, &fonts),
            directory.join("ic_launcher_monochrome.png"),
        );
    }
}
