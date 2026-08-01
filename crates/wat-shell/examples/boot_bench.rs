//! How long does the browser take to open?
//!
//! `cargo run --release --example boot_bench -p wat-shell`
//!
//! Cold start is the whole experience on a phone: the app is opened, glanced at
//! and closed again, and a second of nothing before the first frame is the
//! difference between a browser that feels instant and one that does not. This
//! times the phases of a boot separately, because they have completely different
//! fixes and the total on its own says nothing about which to reach for.
//!
//! `WAT_BOOT_FONT_DIR` points the font scan at one directory, which is how the
//! cost of a device's font set can be measured without a device.

use std::time::Instant;

use wat_layout::geom::Size2D;
use wat_paint::Canvas;
use wat_shell::{Browser, ShellConfig};
use wat_text::FontStore;

fn phase<T>(label: &str, body: impl FnOnce() -> T) -> (T, f64) {
    let start = Instant::now();
    let value = body();
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    println!("  {label:<38} {ms:>8.1} ms");
    (value, ms)
}

fn main() {
    // A mid-range phone: 1080x2340 at 3x, which lays out as 360x780 CSS pixels.
    let scale = 3.0f32;
    let logical = Size2D::new(360.0, 780.0);
    let (physical_width, physical_height) = (
        (logical.width * scale) as u32,
        (logical.height * scale) as u32,
    );

    println!(
        "phone: {}x{} device pixels ({}x{} CSS at {scale}x)\n",
        physical_width, physical_height, logical.width, logical.height
    );

    println!("cold start:");
    let (fonts, font_ms) = phase("load the font database", FontStore::new);
    println!("      ({} font faces indexed)", fonts.face_count());
    drop(fonts);

    // The theme is an argument because the glass is most of what a frame costs,
    // and the question of what it costs is worth being able to answer.
    let theme_name = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "liquid-glass".into());
    println!("  theme: {theme_name}");
    let (mut browser, browser_ms) = phase("start the browser (theme, home page)", || {
        Browser::new(&ShellConfig {
            offline: true,
            size: logical,
            theme: theme_name.clone(),
            ..Default::default()
        })
        .expect("the offline browser starts")
    });

    let (mut canvas, canvas_ms) = phase("allocate the canvas", || {
        Canvas::new(physical_width, physical_height)
    });

    let (_, first_frame_ms) = phase("render the first frame", || {
        browser.render_into_scaled(&mut canvas, scale)
    });

    let (_, present_ms) = phase("copy it out for the window", || canvas.to_argb32());

    let total = font_ms + browser_ms + canvas_ms + first_frame_ms + present_ms;
    println!(
        "\n  {:<38} {total:>8.1} ms",
        "total before anything is seen"
    );

    // Where the frame goes, since it is now the whole boot. Aggregated by kind:
    // the chrome is made of many small pieces and the total per kind is what says
    // which primitive to work on.
    println!("\ninside that frame, by primitive:");
    {
        use std::collections::BTreeMap;
        use std::rc::Rc;
        use wat_paint::display::{DisplayItem, DisplayList};
        use wat_paint::Renderer;
        let fonts = Rc::new(FontStore::new());
        let theme = wat_theme::Theme::default().resolve(false);
        let mut session = wat_engine::Session::new(fonts.clone(), theme.clone(), logical, true);
        let loader = wat_net::StaticLoader::new().with_html("https://e/", "<h1>Hi</h1><p>text</p>");
        session.open_tab("https://e/", &loader);
        let mut chrome = wat_ui::Chrome::new(theme, logical);
        chrome.relayout(session.tab_count());
        let renderer = Renderer::new(&fonts);
        let mut totals: BTreeMap<&str, (f64, usize)> = BTreeMap::new();
        for list in [
            wat_ui::window_display_list(&chrome, &session).scaled(scale),
            chrome.build(&fonts, &session).scaled(scale),
        ] {
            for item in &list.items {
                let kind = match item {
                    DisplayItem::Fill { .. } => "Fill",
                    DisplayItem::Gradient { .. } => "Gradient",
                    DisplayItem::Border { .. } => "Border",
                    DisplayItem::Shadow { .. } => "Shadow",
                    DisplayItem::BackdropFilter { .. } => "BackdropFilter",
                    DisplayItem::Text(_) => "Text",
                    DisplayItem::Image { .. } => "Image",
                    _ => "clip/opacity",
                };
                let one = DisplayList {
                    items: vec![item.clone()],
                };
                let start = Instant::now();
                renderer.render(&one, &mut canvas);
                let ms = start.elapsed().as_secs_f64() * 1000.0;
                let entry = totals.entry(kind).or_insert((0.0, 0));
                entry.0 += ms;
                entry.1 += 1;
            }
        }
        let mut rows: Vec<_> = totals.into_iter().collect();
        rows.sort_by(|a, b| b.1 .0.total_cmp(&a.1 .0));
        for (kind, (ms, count)) in rows {
            println!("  {kind:<16} {count:>4} items {ms:>8.1} ms");
        }
    }

    // A second frame, because the first pays for caches the rest do not.
    let (_, second) = phase("a later frame, for comparison", || {
        browser.render_into_scaled(&mut canvas, scale)
    });
    println!(
        "      ({:.0}% of the boot was one-off)",
        100.0 * (1.0 - second / first_frame_ms.max(0.001))
    );
}
