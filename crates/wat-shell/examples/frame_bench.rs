//! Where does a frame go?
//!
//! `cargo run --release --example frame_bench -p wat-shell`
//!
//! The renderer is software, so frame time is the whole performance story, and
//! it is easy to lose an order of magnitude in one inner loop without any test
//! noticing. This prices a frame by phase and then by display item, so a
//! regression points at the thing that caused it.

use std::hint::black_box;
use std::rc::Rc;
use std::time::Instant;

use wat_engine::Session;
use wat_layout::geom::Size2D;
use wat_paint::display::{DisplayItem, DisplayList};
use wat_paint::{Canvas, Renderer};
use wat_text::FontStore;
use wat_theme::Theme;
use wat_ui::{window_display_list, Chrome};

/// Milliseconds per iteration, after one warm-up.
fn time<T>(label: &str, iterations: u32, mut body: impl FnMut() -> T) -> f64 {
    body();
    let start = Instant::now();
    for _ in 0..iterations {
        black_box(body());
    }
    let each = start.elapsed().as_secs_f64() * 1000.0 / f64::from(iterations);
    println!("  {label:<34} {each:>8.2} ms");
    each
}

/// The same list with every backdrop filter dropped, to price the glass.
fn without_glass(list: &DisplayList) -> DisplayList {
    DisplayList {
        items: list
            .items
            .iter()
            .filter(|item| !matches!(item, DisplayItem::BackdropFilter { .. }))
            .cloned()
            .collect(),
    }
}

fn kind_of(item: &DisplayItem) -> &'static str {
    match item {
        DisplayItem::PushClip(_) => "PushClip",
        DisplayItem::PopClip => "PopClip",
        DisplayItem::PushOpacity(_) => "PushOpacity",
        DisplayItem::PopOpacity => "PopOpacity",
        DisplayItem::Fill { .. } => "Fill",
        DisplayItem::Gradient { .. } => "Gradient",
        DisplayItem::Border { .. } => "Border",
        DisplayItem::Shadow { .. } => "Shadow",
        DisplayItem::BackdropFilter { .. } => "BackdropFilter",
        DisplayItem::Text(_) => "Text",
        DisplayItem::Image { .. } => "Image",
        DisplayItem::Placeholder { .. } => "Placeholder",
    }
}

fn main() {
    let (width, height) = (1280.0f32, 800.0f32);
    let size = Size2D::new(width, height);

    let fonts = Rc::new(FontStore::new());
    let theme = Theme::default().resolve(false);
    let mut session = Session::new(fonts.clone(), theme.clone(), size, false);
    let loader = wat_net::StaticLoader::new().with_html(
        "https://example.com/",
        "<h1>What A Browser</h1><p>Some body text to lay out and paint.</p>",
    );
    session.open_tab("https://example.com/", &loader);

    let mut chrome = Chrome::new(theme, size);
    chrome.relayout(session.tab_count());

    let mut canvas = Canvas::new(width as u32, height as u32);
    let renderer = Renderer::new(&fonts);
    let page = window_display_list(&chrome, &session);
    let ui = chrome.build(&fonts, &session);

    println!("== {width} x {height} ==");
    println!(
        "\nitems: page {} ({} glass), chrome {} ({} glass)",
        page.len(),
        page.backdrop_filter_count(),
        ui.len(),
        ui.backdrop_filter_count()
    );

    // Layout and style are cached in the session, so this should be ~free; if it
    // is not, something is invalidating the cache every frame.
    println!("\nbuilding the display lists:");
    time("page", 50, || window_display_list(&chrome, &session));
    time("chrome", 50, || chrome.build(&fonts, &session));

    println!("\nrasterizing:");
    let page_ms = time("page", 20, || renderer.render(&page, &mut canvas));
    let ui_ms = time("chrome", 20, || renderer.render(&ui, &mut canvas));

    let (page_flat, ui_flat) = (without_glass(&page), without_glass(&ui));
    println!("\nrasterizing without the glass:");
    let page_flat_ms = time("page", 20, || renderer.render(&page_flat, &mut canvas));
    let ui_flat_ms = time("chrome", 20, || renderer.render(&ui_flat, &mut canvas));

    let frame = page_ms + ui_ms;
    let glass = (page_ms - page_flat_ms) + (ui_ms - ui_flat_ms);
    println!("\nframe {frame:.1} ms ({:.1} fps)", 1000.0 / frame);
    println!(
        "glass {glass:.1} ms — {:.0}% of it",
        100.0 * glass / frame.max(f64::EPSILON)
    );

    // Per item, so an expensive one is named rather than guessed at.
    println!("\nitems costing more than 1 ms:");
    for (label, list) in [("page", &page), ("chrome", &ui)] {
        for (index, item) in list.items.iter().enumerate() {
            let one = DisplayList {
                items: vec![item.clone()],
            };
            let start = Instant::now();
            for _ in 0..10 {
                renderer.render(&one, &mut canvas);
            }
            let ms = start.elapsed().as_secs_f64() * 100.0;
            if ms > 1.0 {
                println!("  {label} #{index:<3} {:<16} {ms:>8.2} ms", kind_of(item));
            }
        }
    }

    println!();
    time("to_argb32 (present copy)", 50, || canvas.to_argb32());
}
