//! The browser's own pages, generated as HTML and rendered by the same engine
//! that renders the web.

use wat_theme::{theme_stylesheet, ResolvedTheme};

/// Browser version, taken from the crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Name of the rendering engine.
pub const ENGINE_NAME: &str = "WAT Engine";

/// Wraps `body` in a document that already carries the theme's stylesheet.
fn page(theme: &ResolvedTheme, title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html><head><title>{title}</title><style>{css}</style></head>\
         <body>{body}</body></html>",
        css = theme_stylesheet(theme),
    )
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The new tab page.
pub fn home(theme: &ResolvedTheme, shortcuts: &[(&str, &str)]) -> String {
    let mut tiles = String::new();
    for (label, url) in shortcuts {
        tiles.push_str(&format!(
            "<a class=\"card\" href=\"{}\"><strong>{}</strong><br>\
             <span class=\"muted\">{}</span></a>",
            escape(url),
            escape(label),
            escape(&short_host(url)),
        ));
    }

    let body = format!(
        "<div class=\"surface\">\
           <h1>What-A-Browser</h1>\
           <p class=\"muted\">An independent browser engine. \
            Theme: <span class=\"pill\">{theme_name}</span></p>\
           <p>Type an address or a search in the bar above.</p>\
         </div>\
         <h2>Shortcuts</h2>\
         <div class=\"grid\">{tiles}</div>",
        theme_name = escape(&theme.name),
    );
    page(theme, "New tab", &body)
}

/// The settings page: what the browser is, and what it can be told to do.
pub fn settings(theme: &ResolvedTheme, presets: &[&str], theme_path: Option<&str>) -> String {
    let preset_list = presets
        .iter()
        .map(|name| format!("<span class=\"pill\">{}</span>", escape(name)))
        .collect::<Vec<_>>()
        .join(" ");
    let source = match theme_path {
        Some(path) => format!("<code>{}</code>", escape(path)),
        None => "the built-in preset".to_string(),
    };

    let body = format!(
        "<div class=\"surface\">\
           <h1>Settings</h1>\
           <p class=\"muted\">Everything here is a plain TOML file. \
              Edit it and reload — there is nothing else to configure.</p>\
         </div>\
         <h2>Theme</h2>\
         <div class=\"card\">\
           <p><strong>{name}</strong> — {appearance}, loaded from {source}</p>\
           <p class=\"muted\">Backdrop blur {blur}px · saturation {saturation} · \
              tint strength {tint} · sheen {sheen} · rim {rim}</p>\
         </div>\
         <h2>Presets</h2>\
         <div class=\"card\"><p>{preset_list}</p>\
           <p class=\"muted\">Pass one with <code>--theme &lt;name&gt;</code>, \
              or give a path to your own file.</p></div>\
         <h2>Engine</h2>\
         <div class=\"card\">\
           <p>{engine} {version}</p>\
           <p class=\"muted\">Not based on Chromium, WebKit or Gecko. \
              HTML, CSS, layout and rasterization are implemented in this repository.</p>\
         </div>",
        name = escape(&theme.name),
        appearance = if theme.dark { "dark" } else { "light" },
        blur = theme.glass.blur,
        saturation = theme.glass.saturation,
        tint = theme.glass.tint_strength,
        sheen = theme.glass.sheen,
        rim = theme.glass.rim,
        engine = ENGINE_NAME,
        version = VERSION,
    );
    page(theme, "Settings", &body)
}

/// The version page.
pub fn version(theme: &ResolvedTheme) -> String {
    let body = format!(
        "<div class=\"surface\">\
           <h1>What-A-Browser {version}</h1>\
           <p class=\"muted\">{engine} {version}</p>\
         </div>\
         <h2>What is implemented here</h2>\
         <div class=\"card\">\
           <p>HTML tokenizer and tree builder · CSS parser, selector engine and cascade, \
              with custom properties and <code>calc()</code> · \
              block, inline, flex and grid layout · a software rasterizer with \
              signed-distance antialiasing and real backdrop blur · \
              font discovery, shaping metrics and glyph rasterization · \
              a JavaScript engine and DOM bindings, also written from scratch.</p>\
         </div>\
         <h2>What is not</h2>\
         <div class=\"card\">\
           <p>JavaScript has no regular expressions, promises or generators. \
              Floats, tables and grid placement are approximations. \
              There is no networking from scripts. \
              See <code>docs/ARCHITECTURE.md</code> for the full list.</p>\
         </div>",
        version = VERSION,
        engine = ENGINE_NAME,
    );
    page(theme, "About What-A-Browser", &body)
}

/// A page listing the recognised `about:` addresses.
pub fn index(theme: &ResolvedTheme) -> String {
    let mut list = String::new();
    for name in PAGES {
        list.push_str(&format!(
            "<li><a href=\"about:{0}\">about:{0}</a></li>",
            name
        ));
    }
    let body = format!("<div class=\"surface\"><h1>Internal pages</h1><ul>{list}</ul></div>");
    page(theme, "about:", &body)
}

/// The blank page.
pub fn blank(theme: &ResolvedTheme) -> String {
    page(theme, "", "")
}

/// An error page for a load that failed.
pub fn error(theme: &ResolvedTheme, url: &str, message: &str) -> String {
    let body = format!(
        "<div class=\"surface\">\
           <h1>This page did not load</h1>\
           <p class=\"error\">{message}</p>\
           <p class=\"muted url\">{url}</p>\
           <p class=\"muted\">Check the address, or try again.</p>\
         </div>",
        message = escape(message),
        url = escape(url),
    );
    page(theme, "Problem loading page", &body)
}

/// A page wrapping content the browser cannot render, such as a PDF.
pub fn unsupported(theme: &ResolvedTheme, url: &str, content_type: &str) -> String {
    let body = format!(
        "<div class=\"surface\">\
           <h1>Nothing to show</h1>\
           <p>This address returned <code>{content_type}</code>, \
              which this browser does not render.</p>\
           <p class=\"muted url\">{url}</p>\
         </div>",
        content_type = escape(content_type),
        url = escape(url),
    );
    page(theme, "Unsupported content", &body)
}

/// Wraps plain text so it can be shown in the page area.
pub fn plain_text(theme: &ResolvedTheme, url: &str, text: &str) -> String {
    let body = format!("<pre>{}</pre>", escape(text));
    page(theme, &short_host(url), &body)
}

/// Wraps a bare image so it can be shown centred on the page.
pub fn standalone_image(theme: &ResolvedTheme, url: &str) -> String {
    let body = format!(
        "<div style=\"text-align:center;padding:16px\">\
           <img src=\"{}\" alt=\"image\">\
         </div>",
        escape(url)
    );
    page(theme, &short_host(url), &body)
}

/// The `about:` pages this browser answers.
pub const PAGES: &[&str] = &["home", "settings", "version", "blank"];

/// Is `name` a page we generate?
pub fn is_known_page(name: &str) -> bool {
    name.is_empty() || PAGES.contains(&name)
}

/// A short label for a URL, used as a fallback document title.
fn short_host(url: &str) -> String {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url)
        .trim_start_matches("www.")
        .to_string()
}

/// The default shortcuts on the home page.
pub const DEFAULT_SHORTCUTS: &[(&str, &str)] = &[
    ("Settings", "about:settings"),
    ("About this browser", "about:version"),
    ("Example page", "https://example.com/"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use wat_theme::Theme;

    fn theme() -> ResolvedTheme {
        Theme::default().resolve(false)
    }

    /// Every generated page must be parseable and produce a body with content.
    fn assert_renderable(html: &str, expect_title: bool) {
        let document = wat_html::parse(html);
        assert!(document.find_tag("html").is_some());
        let body = document.body().expect("a body element");
        assert!(
            document.element_children(body).count() > 0 || !expect_title,
            "the body should not be empty"
        );
        if expect_title {
            assert!(document.title().is_some(), "the page needs a title");
        }
        // The theme stylesheet must have made it in and must parse.
        let style = document.find_tag("style").expect("a style element");
        let css = document.text_content(style);
        let sheet = wat_css::Stylesheet::parse(&css, wat_css::Origin::Author);
        assert!(
            sheet.rule_count() > 5,
            "the theme CSS should be substantial"
        );
    }

    #[test]
    fn the_home_page_renders() {
        let html = home(&theme(), DEFAULT_SHORTCUTS);
        assert_renderable(&html, true);
        assert!(html.contains("What-A-Browser"));
        assert!(html.contains("about:settings"));
    }

    #[test]
    fn the_home_page_shows_the_theme_name() {
        let base = Theme {
            name: "My Theme".into(),
            ..Theme::default()
        };
        let html = home(&base.resolve(false), &[]);
        assert!(html.contains("My Theme"));
    }

    #[test]
    fn the_settings_page_reports_the_glass_values() {
        let html = settings(&theme(), &["liquid-glass", "flat"], Some("/etc/wat.toml"));
        assert_renderable(&html, true);
        assert!(html.contains("24"), "the blur radius should be shown");
        assert!(html.contains("liquid-glass"));
        assert!(html.contains("/etc/wat.toml"));
    }

    #[test]
    fn the_version_page_is_honest_about_limits() {
        let html = version(&theme());
        assert_renderable(&html, true);
        assert!(html.contains("also written from scratch"));
        assert!(html.contains(VERSION));
    }

    #[test]
    fn the_error_page_shows_the_message_and_url() {
        let html = error(&theme(), "https://example.com/x", "connection refused");
        assert_renderable(&html, true);
        assert!(html.contains("connection refused"));
        assert!(html.contains("https://example.com/x"));
    }

    #[test]
    fn markup_in_inputs_is_escaped() {
        let html = error(&theme(), "https://x/?a=<script>", "<b>bad</b>");
        assert!(!html.contains("<script>"), "the URL must be escaped");
        assert!(html.contains("&lt;b&gt;bad&lt;/b&gt;"));

        let document = wat_html::parse(&html);
        assert!(
            document.query("script").is_none(),
            "no script element may be created from page inputs"
        );
    }

    #[test]
    fn plain_text_is_wrapped_in_pre() {
        let html = plain_text(&theme(), "https://x/a.txt", "line 1\nline 2 <b>");
        let document = wat_html::parse(&html);
        let pre = document.query("pre").expect("a pre element");
        let text = document.text_content(pre);
        assert!(text.contains("line 1\nline 2 <b>"));
    }

    #[test]
    fn blank_is_valid_but_empty() {
        let html = blank(&theme());
        let document = wat_html::parse(&html);
        let body = document.body().unwrap();
        assert_eq!(document.element_children(body).count(), 0);
    }

    #[test]
    fn the_index_lists_every_page() {
        let html = index(&theme());
        for name in PAGES {
            assert!(html.contains(&format!("about:{name}")), "missing {name}");
        }
    }

    #[test]
    fn known_page_names() {
        assert!(is_known_page("home"));
        assert!(is_known_page(""));
        assert!(!is_known_page("nonsense"));
    }

    #[test]
    fn dark_and_light_pages_differ() {
        let base = Theme::default();
        assert_ne!(
            home(&base.resolve(false), &[]),
            home(&base.resolve(true), &[])
        );
    }
}
