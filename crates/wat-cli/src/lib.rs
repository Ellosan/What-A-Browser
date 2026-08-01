//! The command line side of What-A-Browser.
//!
//! Everything the window does is available headlessly: rendering a page to a
//! PNG, taking a screenshot of the whole browser, and dumping the intermediate
//! trees the engine builds. That makes the engine inspectable and testable
//! without a display server.

use std::path::PathBuf;
use std::rc::Rc;

use wat_engine::{Page, Session};
use wat_layout::geom::{Point, Size2D};
use wat_net::{normalize_input, Address, HttpLoader, Loader, OfflineLoader};
use wat_text::FontStore;
use wat_theme::{preset_names, Theme};
use wat_ui::{page_viewport, render_window_scaled, Chrome};

/// Options shared by every command.
#[derive(Clone, Debug)]
pub struct CommonOptions {
    pub width: f32,
    pub height: f32,
    /// Theme preset name or path to a theme file.
    pub theme: String,
    /// Force the dark or light variant.
    pub appearance: Option<bool>,
    /// Refuse all network access.
    pub offline: bool,
    /// Treat the viewport as a touch screen.
    pub mobile: bool,
    /// Device pixel ratio: the output is this many pixels per CSS pixel.
    pub scale: f32,
    pub search: String,
}

impl Default for CommonOptions {
    fn default() -> Self {
        CommonOptions {
            width: 1280.0,
            height: 800.0,
            theme: "liquid-glass".to_string(),
            appearance: None,
            offline: false,
            mobile: false,
            scale: 1.0,
            search: "https://duckduckgo.com/?q={}".to_string(),
        }
    }
}

impl CommonOptions {
    pub fn size(&self) -> Size2D {
        Size2D::new(self.width.max(64.0), self.height.max(64.0))
    }

    /// The device pixel ratio, clamped to something renderable.
    pub fn scale(&self) -> f32 {
        if self.scale.is_finite() && self.scale > 0.0 {
            self.scale.clamp(0.25, 8.0)
        } else {
            1.0
        }
    }

    /// The canvas size in device pixels.
    pub fn pixel_size(&self) -> (u32, u32) {
        let size = self.size();
        let scale = self.scale();
        (
            (size.width * scale).round().max(1.0) as u32,
            (size.height * scale).round().max(1.0) as u32,
        )
    }

    /// Loads and resolves the theme.
    pub fn resolve_theme(&self) -> Result<(Theme, wat_theme::ResolvedTheme), String> {
        let theme = Theme::named(&self.theme)?;
        let dark = self
            .appearance
            .unwrap_or_else(|| theme.appearance.prefers_dark(false));
        let resolved = theme.resolve(dark);
        Ok((theme, resolved))
    }

    pub fn loader(&self) -> Box<dyn Loader> {
        if self.offline {
            Box::new(OfflineLoader)
        } else {
            Box::new(HttpLoader::default())
        }
    }
}

/// The command to run.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Render just the page.
    Render {
        url: String,
        out: PathBuf,
    },
    /// Render the whole browser window, chrome included.
    Shot {
        url: String,
        out: PathBuf,
    },
    /// Print the parsed DOM.
    Dom {
        url: String,
    },
    /// Print the box tree.
    Boxes {
        url: String,
    },
    /// Print the page's text content.
    Text {
        url: String,
    },
    /// Print the computed style of the first match for a selector.
    Style {
        url: String,
        selector: String,
    },
    /// List the available themes.
    ThemeList,
    /// Print a theme as TOML.
    ThemeShow {
        name: String,
    },
    /// Launch the graphical browser.
    Gui {
        url: Option<String>,
    },
    Version,
    Help,
}

/// A parsed command line.
#[derive(Clone, Debug)]
pub struct Invocation {
    pub command: Command,
    pub options: CommonOptions,
}

/// Usage text.
pub const USAGE: &str = "\
What-A-Browser (WAT) — an independent browser engine

USAGE:
    wat [URL]                       Open the browser window
    wat <COMMAND> [ARGS] [OPTIONS]

COMMANDS:
    render <URL>       Render the page to a PNG (default: page.png)
    shot <URL>         Render the whole window, chrome included (default: shot.png)
    dom <URL>          Print the parsed document tree
    boxes <URL>        Print the layout box tree
    text <URL>         Print the page's text content
    style <URL> <SEL>  Print the computed style of the first match for SEL
    theme list         List the bundled themes
    theme show <NAME>  Print a theme as TOML
    version            Print version information
    help               Show this message

OPTIONS:
    -o, --out <PATH>   Where to write an image
    --width <PX>       Viewport width (default 1280)
    --height <PX>      Viewport height (default 800)
    --theme <NAME>     Theme preset name or path to a .toml file
    --dark             Force the dark variant
    --light            Force the light variant
    --mobile           Use the touch layout and a phone-sized viewport
    --offline          Refuse all network access
    --search <URL>     Search template, with {} for the query

EXAMPLES:
    wat shot https://example.com -o example.png
    wat render about:home --dark -o home.png
    wat boxes https://example.com --width 480
    wat theme show flat > my-theme.toml
";

/// Parses a command line, excluding the program name.
pub fn parse_args(args: &[String]) -> Result<Invocation, String> {
    let mut options = CommonOptions::default();
    let mut positional: Vec<String> = Vec::new();
    let mut out: Option<PathBuf> = None;

    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        let mut value = |name: &str| -> Result<String, String> {
            index += 1;
            args.get(index)
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match arg {
            "-o" | "--out" => out = Some(PathBuf::from(value("--out")?)),
            "--width" => {
                options.width = value("--width")?
                    .parse()
                    .map_err(|_| "--width needs a number".to_string())?
            }
            "--height" => {
                options.height = value("--height")?
                    .parse()
                    .map_err(|_| "--height needs a number".to_string())?
            }
            "--theme" => options.theme = value("--theme")?,
            "--search" => options.search = value("--search")?,
            "--dark" => options.appearance = Some(true),
            "--light" => options.appearance = Some(false),
            "--offline" => options.offline = true,
            "--mobile" => options.mobile = true,
            "--scale" => {
                options.scale = value("--scale")?
                    .parse()
                    .map_err(|_| "--scale needs a number".to_string())?
            }
            "-h" | "--help" => {
                return Ok(Invocation {
                    command: Command::Help,
                    options,
                })
            }
            "-V" | "--version" => {
                return Ok(Invocation {
                    command: Command::Version,
                    options,
                })
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown option `{other}`"));
            }
            other => positional.push(other.to_string()),
        }
        index += 1;
    }

    // The mobile flag implies a phone-shaped viewport unless one was given.
    if options.mobile
        && options.width == CommonOptions::default().width
        && options.height == CommonOptions::default().height
    {
        options.width = 390.0;
        options.height = 844.0;
    }

    let command = match positional.split_first() {
        None => Command::Gui { url: None },
        Some((first, rest)) => match first.as_str() {
            "render" => Command::Render {
                url: require_url(rest, "render")?,
                out: out.unwrap_or_else(|| PathBuf::from("page.png")),
            },
            "shot" => Command::Shot {
                url: require_url(rest, "shot")?,
                out: out.unwrap_or_else(|| PathBuf::from("shot.png")),
            },
            "dom" => Command::Dom {
                url: require_url(rest, "dom")?,
            },
            "boxes" => Command::Boxes {
                url: require_url(rest, "boxes")?,
            },
            "text" => Command::Text {
                url: require_url(rest, "text")?,
            },
            "style" => {
                let url = require_url(rest, "style")?;
                let selector = rest
                    .get(1)
                    .cloned()
                    .ok_or_else(|| "style needs a selector".to_string())?;
                Command::Style { url, selector }
            }
            "theme" => match rest.first().map(String::as_str) {
                None | Some("list") => Command::ThemeList,
                Some("show") | Some("dump") => Command::ThemeShow {
                    name: rest
                        .get(1)
                        .cloned()
                        .unwrap_or_else(|| "liquid-glass".to_string()),
                },
                Some(other) => return Err(format!("unknown theme command `{other}`")),
            },
            "version" => Command::Version,
            "help" => Command::Help,
            // Anything else is an address to open in the window.
            _ => Command::Gui {
                url: Some(positional.join(" ")),
            },
        },
    };

    Ok(Invocation { command, options })
}

fn require_url(rest: &[String], command: &str) -> Result<String, String> {
    rest.first()
        .cloned()
        .ok_or_else(|| format!("{command} needs a URL"))
}

/// Loads a page for the given options.
fn load_page(url: &str, options: &CommonOptions, viewport: Size2D) -> Result<Page, String> {
    let (_, theme) = options.resolve_theme()?;
    let address = normalize_input(url, &options.search).map_err(|error| error.to_string())?;
    let loader = options.loader();
    let fonts = Rc::new(FontStore::new());
    let mut page = Page::load(
        address,
        loader.as_ref(),
        fonts,
        theme,
        viewport,
        options.mobile,
    );
    page.scroll_to_fragment();
    Ok(page)
}

/// `wat render`
pub fn render(url: &str, out: &std::path::Path, options: &CommonOptions) -> Result<String, String> {
    let page = load_page(url, options, options.size())?;
    // The page is laid out in CSS pixels and drawn at the device pixel ratio,
    // exactly as the window does it.
    let canvas = page.render_to_canvas_scaled(options.scale());
    let png = canvas.to_png()?;
    std::fs::write(out, png).map_err(|error| format!("{}: {error}", out.display()))?;
    Ok(format!(
        "wrote {} ({}x{}) for {}{}",
        out.display(),
        canvas.width(),
        canvas.height(),
        page.address,
        match &page.error {
            Some(error) => format!(" — load failed: {error}"),
            None => String::new(),
        }
    ))
}

/// `wat shot`
pub fn shot(url: &str, out: &std::path::Path, options: &CommonOptions) -> Result<String, String> {
    let (_, theme) = options.resolve_theme()?;
    let fonts = Rc::new(FontStore::new());
    let size = options.size();

    let mut chrome = Chrome::new(theme.clone(), size);
    let mut session = Session::new(fonts.clone(), theme, page_viewport(&chrome), options.mobile);
    session.search_template = options.search.clone();

    let loader = options.loader();
    session.open_tab(url, loader.as_ref());
    chrome.relayout(session.tab_count());
    session.set_viewport(page_viewport(&chrome), options.mobile);
    if let Some(tab) = session.active() {
        chrome.omnibox.set_url(tab.url());
    }

    let (width, height) = options.pixel_size();
    let mut canvas = wat_paint::Canvas::new(width, height);
    render_window_scaled(&chrome, &session, &fonts, &mut canvas, options.scale());
    let png = canvas.to_png()?;
    std::fs::write(out, png).map_err(|error| format!("{}: {error}", out.display()))?;
    Ok(format!(
        "wrote {} ({}x{}) — {} layout, {} theme",
        out.display(),
        canvas.width(),
        canvas.height(),
        if chrome.layout().is_mobile() {
            "mobile"
        } else {
            "desktop"
        },
        chrome.theme().name,
    ))
}

/// `wat dom`
pub fn dom(url: &str, options: &CommonOptions) -> Result<String, String> {
    let page = load_page(url, options, options.size())?;
    let document = page.document();
    Ok(document.to_tree_string(document.root()))
}

/// `wat boxes`
pub fn boxes(url: &str, options: &CommonOptions) -> Result<String, String> {
    let page = load_page(url, options, options.size())?;
    Ok(describe_boxes(page.layout_tree()))
}

/// A readable dump of the box tree.
pub fn describe_boxes(tree: &wat_layout::LayoutTree) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "document {:.0}x{:.0}, {} boxes",
        tree.document_size.width,
        tree.document_size.height,
        tree.len()
    );

    fn walk(tree: &wat_layout::LayoutTree, index: usize, depth: usize, out: &mut String) {
        use std::fmt::Write as _;
        let layout_box = tree.get(index);
        for _ in 0..depth {
            out.push_str("  ");
        }
        let name = match &layout_box.kind {
            wat_layout::BoxKind::Text(fragment) => {
                let text: String = fragment.text.chars().take(40).collect();
                format!("text {text:?}")
            }
            wat_layout::BoxKind::Replaced(replaced) => format!("replaced <{}>", replaced.label),
            wat_layout::BoxKind::Marker(label) => format!("marker {label:?}"),
            other => {
                let tag = layout_box.node.map(|_| String::new()).unwrap_or_default();
                format!("{other:?}{tag}")
            }
        };
        let rect = layout_box.rect;
        let _ = writeln!(
            out,
            "{name} at ({:.1}, {:.1}) {:.1}x{:.1}",
            rect.x, rect.y, rect.width, rect.height
        );
        for child in tree.children(index) {
            walk(tree, *child, depth + 1, out);
        }
    }

    if let Some(root) = tree.root {
        walk(tree, root, 0, &mut out);
    }
    out
}

/// `wat text`
pub fn text(url: &str, options: &CommonOptions) -> Result<String, String> {
    let page = load_page(url, options, options.size())?;
    let document = page.document();
    let body = document.body().unwrap_or(document.root());
    let raw = document.text_content(body);
    // Collapse runs of blank lines so the output stays readable.
    let mut out = String::new();
    let mut blank = 0;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(trimmed);
        out.push('\n');
    }
    Ok(out)
}

/// `wat style`
pub fn style(url: &str, selector: &str, options: &CommonOptions) -> Result<String, String> {
    let page = load_page(url, options, options.size())?;
    let document = page.document();
    let node = document
        .query(selector)
        .ok_or_else(|| format!("nothing matches `{selector}`"))?;
    let style = page.styles().get(node);
    let element = document
        .element(node)
        .map(|el| el.name.clone())
        .unwrap_or_else(|| "#text".to_string());

    let rect = page
        .layout_tree()
        .box_for_node(node)
        .map(|index| page.layout_tree().get(index).rect);

    Ok(format!(
        "<{element}>\n\
         display: {:?}\n\
         position: {:?}\n\
         color: {:?}\n\
         background-color: {:?}\n\
         font: {:?} {}px weight {}\n\
         line-height: {:.1}px\n\
         margin: {:?}\n\
         padding: {:?}\n\
         border-radius: {:?}\n\
         backdrop-filter: {:?}\n\
         box: {}\n",
        style.display,
        style.position,
        style.color,
        style.background_color,
        style.font_family,
        style.font_size,
        style.font_weight,
        style.line_height_px(),
        style.margin,
        style.padding,
        style.border_radius,
        style.backdrop_filter,
        match rect {
            Some(rect) => format!(
                "({:.1}, {:.1}) {:.1}x{:.1}",
                rect.x, rect.y, rect.width, rect.height
            ),
            None => "no box generated".to_string(),
        }
    ))
}

/// `wat theme list`
pub fn theme_list() -> String {
    use std::fmt::Write as _;
    let mut out = String::from("bundled themes:\n");
    for name in preset_names() {
        match Theme::named(name) {
            Ok(theme) => {
                let light = theme.resolve(false);
                let _ = writeln!(
                    out,
                    "  {name:<22} {} — blur {}px, saturation {}, sheen {}",
                    theme.name, light.glass.blur, light.glass.saturation, light.glass.sheen
                );
            }
            Err(error) => {
                let _ = writeln!(out, "  {name:<22} <failed to load: {error}>");
            }
        }
    }
    out.push_str("\nPass any of these to --theme, or a path to your own .toml file.\n");
    out
}

/// `wat theme show`
pub fn theme_show(name: &str) -> Result<String, String> {
    Theme::named(name)?.to_toml()
}

/// `wat version`
pub fn version() -> String {
    format!(
        "What-A-Browser {version} (WAT)\n\
         engine: {engine} {version}\n\
         renderer: software, signed-distance antialiasing, real backdrop blur\n\
         not based on Chromium, WebKit or Gecko\n\
         JavaScript: WAT JS, written from scratch\n",
        version = wat_engine::VERSION,
        engine = wat_engine::ENGINE_NAME,
    )
}

/// Runs a parsed invocation, returning text to print.
///
/// [`Command::Gui`] is not handled here: the caller launches the window, since
/// that needs the platform event loop.
pub fn run(invocation: &Invocation) -> Result<String, String> {
    let options = &invocation.options;
    match &invocation.command {
        Command::Render { url, out } => render(url, out, options),
        Command::Shot { url, out } => shot(url, out, options),
        Command::Dom { url } => dom(url, options),
        Command::Boxes { url } => boxes(url, options),
        Command::Text { url } => text(url, options),
        Command::Style { url, selector } => style(url, selector, options),
        Command::ThemeList => Ok(theme_list()),
        Command::ThemeShow { name } => theme_show(name),
        Command::Version => Ok(version()),
        Command::Help => Ok(USAGE.to_string()),
        Command::Gui { .. } => Err("the GUI is launched by the caller".to_string()),
    }
}

/// Resolves a URL the way the CLI does, for callers that need it up front.
pub fn resolve_url(input: &str, options: &CommonOptions) -> Result<Address, String> {
    normalize_input(input, &options.search).map_err(|error| error.to_string())
}

/// Scrolls a page as `--scroll` would, kept public for the shell's tests.
pub fn scroll_page(page: &mut Page, amount: f32) -> bool {
    page.scroll_by(0.0, amount)
}

/// The origin the CLI renders a page at, which is the canvas origin.
pub const PAGE_ORIGIN: Point = Point::ZERO;

#[cfg(test)]
mod tests {
    use super::*;

    fn args(input: &str) -> Vec<String> {
        input.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn no_arguments_opens_the_window() {
        let invocation = parse_args(&[]).unwrap();
        assert_eq!(invocation.command, Command::Gui { url: None });
    }

    #[test]
    fn a_bare_url_opens_the_window_there() {
        let invocation = parse_args(&args("https://example.com")).unwrap();
        assert_eq!(
            invocation.command,
            Command::Gui {
                url: Some("https://example.com".into())
            }
        );
    }

    #[test]
    fn commands_and_their_defaults() {
        let invocation = parse_args(&args("render https://x/")).unwrap();
        assert_eq!(
            invocation.command,
            Command::Render {
                url: "https://x/".into(),
                out: PathBuf::from("page.png")
            }
        );

        let invocation = parse_args(&args("shot https://x/ -o a.png")).unwrap();
        assert_eq!(
            invocation.command,
            Command::Shot {
                url: "https://x/".into(),
                out: PathBuf::from("a.png")
            }
        );

        assert_eq!(
            parse_args(&args("theme list")).unwrap().command,
            Command::ThemeList
        );
        assert_eq!(
            parse_args(&args("theme")).unwrap().command,
            Command::ThemeList
        );
        assert_eq!(
            parse_args(&args("theme show flat")).unwrap().command,
            Command::ThemeShow {
                name: "flat".into()
            }
        );
    }

    #[test]
    fn options_are_parsed() {
        let invocation = parse_args(&args(
            "render https://x/ --width 640 --height 480 --dark --theme flat",
        ))
        .unwrap();
        assert_eq!(invocation.options.width, 640.0);
        assert_eq!(invocation.options.height, 480.0);
        assert_eq!(invocation.options.appearance, Some(true));
        assert_eq!(invocation.options.theme, "flat");

        let light = parse_args(&args("render https://x/ --light")).unwrap();
        assert_eq!(light.options.appearance, Some(false));
    }

    #[test]
    fn the_mobile_flag_implies_a_phone_viewport() {
        let invocation = parse_args(&args("shot https://x/ --mobile")).unwrap();
        assert!(invocation.options.mobile);
        assert_eq!(invocation.options.width, 390.0);

        // …unless a size was given explicitly.
        let explicit = parse_args(&args("shot https://x/ --mobile --width 500")).unwrap();
        assert_eq!(explicit.options.width, 500.0);
    }

    #[test]
    fn help_and_version_flags() {
        assert_eq!(parse_args(&args("--help")).unwrap().command, Command::Help);
        assert_eq!(parse_args(&args("help")).unwrap().command, Command::Help);
        assert_eq!(
            parse_args(&args("--version")).unwrap().command,
            Command::Version
        );
    }

    #[test]
    fn missing_arguments_are_reported() {
        assert!(parse_args(&args("render")).is_err());
        assert!(parse_args(&args("style https://x/")).is_err());
        assert!(parse_args(&args("--width")).is_err());
        assert!(parse_args(&args("--width abc")).is_err());
        assert!(parse_args(&args("--bogus")).is_err());
    }

    #[test]
    fn the_usage_text_mentions_every_command() {
        for command in [
            "render", "shot", "dom", "boxes", "text", "style", "theme", "version", "help",
        ] {
            assert!(USAGE.contains(command), "usage does not mention {command}");
        }
    }

    #[test]
    fn theme_listing_names_the_presets() {
        let listing = theme_list();
        for name in preset_names() {
            assert!(listing.contains(name), "{name} missing from {listing}");
        }
    }

    #[test]
    fn theme_show_round_trips() {
        let toml = theme_show("liquid-glass").expect("serialises");
        let parsed = Theme::from_toml(&toml).expect("parses");
        assert_eq!(parsed, Theme::default());
        assert!(theme_show("nonexistent").is_err());
    }

    #[test]
    fn version_text_is_honest() {
        let text = version();
        assert!(text.contains("not based on Chromium"));
        assert!(text.contains("WAT JS"));
    }

    // The commands below run against `about:` pages, so they need no network.

    #[test]
    fn dom_dump_works_offline() {
        let options = CommonOptions {
            offline: true,
            ..Default::default()
        };
        let dump = dom("about:home", &options).expect("dumped");
        assert!(dump.contains("#document"));
        assert!(dump.contains("<html>"));
        assert!(dump.contains("<body>"));
    }

    #[test]
    fn box_dump_reports_geometry() {
        let options = CommonOptions {
            offline: true,
            width: 900.0,
            height: 700.0,
            ..Default::default()
        };
        let dump = boxes("about:version", &options).expect("dumped");
        assert!(dump.contains("document 900x"), "got: {dump}");
        assert!(dump.contains("boxes"));
        assert!(dump.contains(" at ("));
    }

    #[test]
    fn text_extraction_reads_the_page() {
        let options = CommonOptions {
            offline: true,
            ..Default::default()
        };
        let extracted = text("about:version", &options).expect("extracted");
        assert!(extracted.contains("What-A-Browser"));
    }

    #[test]
    fn style_inspection_reports_computed_values() {
        let options = CommonOptions {
            offline: true,
            ..Default::default()
        };
        let report = style("about:home", ".surface", &options).expect("inspected");
        assert!(report.contains("backdrop-filter"));
        assert!(
            report.contains("blur"),
            "the glass surface should be blurred"
        );
        assert!(style("about:home", ".nothing-here", &options).is_err());
    }

    #[test]
    fn render_writes_a_png() {
        let out = std::env::temp_dir().join("wat-cli-render-test.png");
        let options = CommonOptions {
            offline: true,
            width: 320.0,
            height: 240.0,
            ..Default::default()
        };
        let message = render("about:home", &out, &options).expect("rendered");
        assert!(message.contains("320x240"), "got: {message}");

        let bytes = std::fs::read(&out).expect("the file exists");
        let decoded = wat_paint::RasterImage::decode(&bytes).expect("a valid PNG");
        assert_eq!(decoded.width, 320);
        assert_eq!(decoded.height, 240);
        std::fs::remove_file(&out).ok();
    }

    #[test]
    fn shot_writes_a_png_of_the_whole_window() {
        let out = std::env::temp_dir().join("wat-cli-shot-test.png");
        let options = CommonOptions {
            offline: true,
            width: 900.0,
            height: 600.0,
            ..Default::default()
        };
        let message = shot("about:home", &out, &options).expect("rendered");
        assert!(message.contains("desktop"), "got: {message}");

        let bytes = std::fs::read(&out).expect("the file exists");
        let decoded = wat_paint::RasterImage::decode(&bytes).expect("a valid PNG");
        assert_eq!(decoded.width, 900);
        std::fs::remove_file(&out).ok();
    }

    #[test]
    fn a_mobile_shot_uses_the_touch_layout() {
        let out = std::env::temp_dir().join("wat-cli-mobile-test.png");
        let options = CommonOptions {
            offline: true,
            mobile: true,
            width: 390.0,
            height: 844.0,
            ..Default::default()
        };
        let message = shot("about:home", &out, &options).expect("rendered");
        assert!(message.contains("mobile"), "got: {message}");
        std::fs::remove_file(&out).ok();
    }

    #[test]
    fn an_unreachable_page_still_renders_an_error_page() {
        let out = std::env::temp_dir().join("wat-cli-error-test.png");
        let options = CommonOptions {
            offline: true,
            ..Default::default()
        };
        let message = render("https://nowhere.invalid/", &out, &options).expect("rendered");
        assert!(message.contains("load failed"), "got: {message}");
        assert!(std::fs::metadata(&out).is_ok());
        std::fs::remove_file(&out).ok();
    }

    #[test]
    fn an_unknown_theme_is_an_error_not_a_panic() {
        let options = CommonOptions {
            theme: "not-a-theme".into(),
            offline: true,
            ..Default::default()
        };
        let error = dom("about:home", &options).expect_err("should fail");
        assert!(error.contains("unknown theme"), "got: {error}");
    }

    #[test]
    fn the_dark_and_light_variants_render_differently() {
        let dark_options = CommonOptions {
            offline: true,
            appearance: Some(true),
            width: 200.0,
            height: 150.0,
            ..Default::default()
        };
        let light_options = CommonOptions {
            appearance: Some(false),
            ..dark_options.clone()
        };
        let dark = load_page("about:home", &dark_options, dark_options.size())
            .unwrap()
            .render_to_canvas();
        let light = load_page("about:home", &light_options, light_options.size())
            .unwrap()
            .render_to_canvas();
        assert_ne!(dark.pixels(), light.pixels());
        assert!(dark.pixel(100, 75).luminance() < light.pixel(100, 75).luminance());
    }
}
