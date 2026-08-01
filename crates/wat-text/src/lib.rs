//! Font discovery, metrics, shaping and glyph rasterization.
//!
//! Layout needs advance widths and line metrics; the painter needs coverage
//! bitmaps. Both go through [`FontStore`], which caches faces and rasterized
//! glyphs so a page that repeats the same text does the work once.
//!
//! Everything here is CPU-side and pure Rust: no FreeType, no HarfBuzz, no
//! platform text stack.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// A request for a specific face at a specific size.
#[derive(Clone, Debug, PartialEq)]
pub struct FontRequest {
    /// Family names in preference order; may include generic families.
    pub families: Vec<String>,
    /// 1..=1000, CSS `font-weight`.
    pub weight: u16,
    pub italic: bool,
    /// Size in pixels.
    pub size: f32,
    /// Extra space added after every glyph.
    pub letter_spacing: f32,
    /// Extra space added to space characters.
    pub word_spacing: f32,
}

impl Default for FontRequest {
    fn default() -> Self {
        FontRequest {
            families: Vec::new(),
            weight: 400,
            italic: false,
            size: 16.0,
            letter_spacing: 0.0,
            word_spacing: 0.0,
        }
    }
}

impl FontRequest {
    pub fn new(size: f32) -> Self {
        FontRequest {
            size,
            ..Default::default()
        }
    }

    pub fn with_families(mut self, families: Vec<String>) -> Self {
        self.families = families;
        self
    }

    pub fn with_weight(mut self, weight: u16) -> Self {
        self.weight = weight;
        self
    }

    pub fn italic(mut self, italic: bool) -> Self {
        self.italic = italic;
        self
    }

    /// Key for the face-resolution cache; the size does not affect which face
    /// is chosen.
    fn face_key(&self) -> (Vec<String>, u16, bool) {
        (self.families.clone(), self.weight, self.italic)
    }
}

/// Vertical metrics for one face at one size, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineMetrics {
    /// Distance from the baseline up to the top of the tallest glyph.
    pub ascent: f32,
    /// Distance from the baseline down (positive).
    pub descent: f32,
    pub line_gap: f32,
}

impl LineMetrics {
    /// Natural line height for the face.
    pub fn height(&self) -> f32 {
        self.ascent + self.descent + self.line_gap
    }

    /// A plausible set of metrics for a size, used when no font is available.
    fn synthetic(size: f32) -> Self {
        LineMetrics {
            ascent: size * 0.8,
            descent: size * 0.2,
            line_gap: size * 0.15,
        }
    }
}

/// One rasterized glyph: an 8-bit coverage mask plus placement.
#[derive(Clone, Debug)]
pub struct GlyphBitmap {
    pub width: usize,
    pub height: usize,
    /// Offset from the pen position to the left edge of the bitmap.
    pub left: i32,
    /// Offset from the baseline to the *top* of the bitmap, downwards positive.
    pub top: i32,
    pub coverage: Vec<u8>,
}

impl GlyphBitmap {
    fn empty() -> Self {
        GlyphBitmap {
            width: 0,
            height: 0,
            left: 0,
            top: 0,
            coverage: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// A glyph placed on a line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PositionedGlyph {
    pub ch: char,
    /// Pen x relative to the start of the run.
    pub x: f32,
    pub advance: f32,
}

/// A shaped run of text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapedRun {
    pub glyphs: Vec<PositionedGlyph>,
    pub width: f32,
}

/// Generic CSS families mapped to concrete candidates, most preferred first.
const GENERIC_SANS: &[&str] = &[
    "Inter",
    "Helvetica Neue",
    "Helvetica",
    "Arial",
    "Liberation Sans",
    "DejaVu Sans",
    "Noto Sans",
    "Segoe UI",
    "Roboto",
    "FreeSans",
];
const GENERIC_SERIF: &[&str] = &[
    "Georgia",
    "Times New Roman",
    "Liberation Serif",
    "DejaVu Serif",
    "Noto Serif",
    "FreeSerif",
];
const GENERIC_MONO: &[&str] = &[
    "SF Mono",
    "Menlo",
    "Consolas",
    "Liberation Mono",
    "DejaVu Sans Mono",
    "Noto Sans Mono",
    "Courier New",
    "FreeMono",
];

fn generic_candidates(name: &str) -> Option<&'static [&'static str]> {
    match name.to_ascii_lowercase().as_str() {
        "sans-serif" | "system-ui" | "-apple-system" | "ui-sans-serif" | "blinkmacsystemfont" => {
            Some(GENERIC_SANS)
        }
        "serif" | "ui-serif" => Some(GENERIC_SERIF),
        "monospace" | "ui-monospace" => Some(GENERIC_MONO),
        // `cursive` and `fantasy` have no sensible mapping; fall back to sans.
        "cursive" | "fantasy" => Some(GENERIC_SANS),
        _ => None,
    }
}

/// Key for the face-resolution cache: families, weight and italic flag.
type FaceKey = (Vec<String>, u16, bool);
/// Key for the glyph cache: face, character and quantised size.
type GlyphKey = (fontdb::ID, char, u32);

/// Caches faces, metrics and glyph bitmaps for the whole browser.
pub struct FontStore {
    db: fontdb::Database,
    faces: RefCell<HashMap<fontdb::ID, Option<Rc<fontdue::Font>>>>,
    resolved: RefCell<HashMap<FaceKey, Option<fontdb::ID>>>,
    glyphs: RefCell<HashMap<GlyphKey, Rc<GlyphBitmap>>>,
    /// Faces used to cover characters the primary face lacks.
    fallbacks: Vec<fontdb::ID>,
}

impl Default for FontStore {
    fn default() -> Self {
        FontStore::new()
    }
}

impl FontStore {
    /// Loads the system font set.
    pub fn new() -> Self {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        Self::from_database(db)
    }

    /// An empty store, for tests and headless runs with no fonts installed.
    pub fn empty() -> Self {
        Self::from_database(fontdb::Database::new())
    }

    pub fn from_database(db: fontdb::Database) -> Self {
        let mut store = FontStore {
            db,
            faces: RefCell::new(HashMap::new()),
            resolved: RefCell::new(HashMap::new()),
            glyphs: RefCell::new(HashMap::new()),
            fallbacks: Vec::new(),
        };
        store.fallbacks = store.collect_fallbacks();
        if store.db.is_empty() {
            log::warn!("no system fonts found; text will use synthetic metrics");
        }
        store
    }

    /// Adds a font file, e.g. one shipped with a theme.
    pub fn load_file(&mut self, path: &std::path::Path) -> bool {
        let loaded = self.db.load_font_file(path).is_ok();
        if loaded {
            self.resolved.borrow_mut().clear();
            self.fallbacks = self.collect_fallbacks();
        }
        loaded
    }

    /// Adds a font from memory.
    pub fn load_bytes(&mut self, data: Vec<u8>) {
        self.db.load_font_data(data);
        self.resolved.borrow_mut().clear();
        self.fallbacks = self.collect_fallbacks();
    }

    pub fn face_count(&self) -> usize {
        self.db.len()
    }

    pub fn has_fonts(&self) -> bool {
        !self.db.is_empty()
    }

    /// Families the store knows about, sorted and de-duplicated.
    pub fn families(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .db
            .faces()
            .filter_map(|face| face.families.first().map(|(name, _)| name.clone()))
            .collect();
        names.sort();
        names.dedup();
        names
    }

    fn collect_fallbacks(&self) -> Vec<fontdb::ID> {
        let mut ids = Vec::new();
        for family in GENERIC_SANS.iter().chain(GENERIC_SERIF).chain(GENERIC_MONO) {
            if let Some(id) = self.query_family(family, 400, false) {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        // Last resort: whatever the database lists first.
        if let Some(face) = self.db.faces().next() {
            if !ids.contains(&face.id) {
                ids.push(face.id);
            }
        }
        ids
    }

    fn query_family(&self, family: &str, weight: u16, italic: bool) -> Option<fontdb::ID> {
        let query = fontdb::Query {
            families: &[fontdb::Family::Name(family)],
            weight: fontdb::Weight(weight),
            stretch: fontdb::Stretch::Normal,
            style: if italic {
                fontdb::Style::Italic
            } else {
                fontdb::Style::Normal
            },
        };
        self.db.query(&query)
    }

    /// Picks the face for `request`, consulting the cache first.
    fn resolve(&self, request: &FontRequest) -> Option<fontdb::ID> {
        let key = request.face_key();
        if let Some(cached) = self.resolved.borrow().get(&key) {
            return *cached;
        }

        let mut chosen = None;
        'outer: for family in &request.families {
            let candidates: Vec<&str> = match generic_candidates(family) {
                Some(list) => list.to_vec(),
                None => vec![family.as_str()],
            };
            for candidate in candidates {
                if let Some(id) = self.query_family(candidate, request.weight, request.italic) {
                    chosen = Some(id);
                    break 'outer;
                }
            }
        }
        // No family matched (or none was given): use the default sans-serif.
        if chosen.is_none() {
            for candidate in GENERIC_SANS {
                if let Some(id) = self.query_family(candidate, request.weight, request.italic) {
                    chosen = Some(id);
                    break;
                }
            }
        }
        if chosen.is_none() {
            chosen = self.fallbacks.first().copied();
        }

        self.resolved.borrow_mut().insert(key, chosen);
        chosen
    }

    /// Loads and caches the parsed face for `id`.
    fn face(&self, id: fontdb::ID) -> Option<Rc<fontdue::Font>> {
        if let Some(cached) = self.faces.borrow().get(&id) {
            return cached.clone();
        }
        let parsed = self.db.with_face_data(id, |data, index| {
            let settings = fontdue::FontSettings {
                collection_index: index,
                scale: 40.0,
                ..fontdue::FontSettings::default()
            };
            fontdue::Font::from_bytes(data, settings).ok().map(Rc::new)
        });
        let font = parsed.flatten();
        if font.is_none() {
            log::debug!("failed to parse face {id:?}");
        }
        self.faces.borrow_mut().insert(id, font.clone());
        font
    }

    /// The face that should draw `ch` for this request, falling back when the
    /// primary face has no glyph for it.
    fn face_for_char(
        &self,
        request: &FontRequest,
        ch: char,
    ) -> Option<(fontdb::ID, Rc<fontdue::Font>)> {
        let primary = self.resolve(request);
        if let Some(id) = primary {
            if let Some(font) = self.face(id) {
                if font.lookup_glyph_index(ch) != 0 || ch == '\0' {
                    return Some((id, font));
                }
                // Keep the primary face for whitespace: it has no visible glyph
                // anyway and its advance is the one we want.
                if ch.is_whitespace() {
                    return Some((id, font));
                }
            }
        }
        for id in &self.fallbacks {
            if let Some(font) = self.face(*id) {
                if font.lookup_glyph_index(ch) != 0 {
                    return Some((*id, font));
                }
            }
        }
        primary.and_then(|id| self.face(id).map(|font| (id, font)))
    }

    /// Line metrics for `request`.
    pub fn line_metrics(&self, request: &FontRequest) -> LineMetrics {
        let Some(font) = self.resolve(request).and_then(|id| self.face(id)) else {
            return LineMetrics::synthetic(request.size);
        };
        match font.horizontal_line_metrics(request.size) {
            Some(metrics) => LineMetrics {
                ascent: metrics.ascent,
                descent: -metrics.descent,
                line_gap: metrics.line_gap,
            },
            None => LineMetrics::synthetic(request.size),
        }
    }

    /// Advance width of a single character, spacing included.
    pub fn advance(&self, request: &FontRequest, ch: char) -> f32 {
        let base = match self.face_for_char(request, ch) {
            Some((_, font)) => font.metrics(ch, request.size).advance_width,
            // Synthetic metrics: a monospace-ish half-em box.
            None => {
                if ch == '\t' {
                    request.size * 2.0
                } else {
                    request.size * 0.5
                }
            }
        };
        let word = if ch == ' ' { request.word_spacing } else { 0.0 };
        base + request.letter_spacing + word
    }

    /// Total advance width of `text`.
    pub fn measure(&self, request: &FontRequest, text: &str) -> f32 {
        text.chars().map(|ch| self.advance(request, ch)).sum()
    }

    /// Lays glyphs out left to right, applying kerning where the face has it.
    pub fn shape(&self, request: &FontRequest, text: &str) -> ShapedRun {
        let mut glyphs = Vec::with_capacity(text.len());
        let mut pen = 0.0f32;
        let mut previous: Option<char> = None;
        let face = self.resolve(request).and_then(|id| self.face(id));

        for ch in text.chars() {
            if let (Some(font), Some(prev)) = (face.as_ref(), previous) {
                pen += font.horizontal_kern(prev, ch, request.size).unwrap_or(0.0);
            }
            let advance = self.advance(request, ch);
            glyphs.push(PositionedGlyph {
                ch,
                x: pen,
                advance,
            });
            pen += advance;
            previous = Some(ch);
        }

        ShapedRun { glyphs, width: pen }
    }

    /// Rasterizes `ch`, caching the result.
    pub fn glyph(&self, request: &FontRequest, ch: char) -> Rc<GlyphBitmap> {
        let Some((id, font)) = self.face_for_char(request, ch) else {
            return Rc::new(GlyphBitmap::empty());
        };
        // Quantise the size so near-identical sizes share cache entries.
        let size_key = (request.size * 4.0).round() as u32;
        let key = (id, ch, size_key);
        if let Some(cached) = self.glyphs.borrow().get(&key) {
            return cached.clone();
        }

        let size = size_key as f32 / 4.0;
        let (metrics, coverage) = font.rasterize(ch, size);
        let bitmap = Rc::new(GlyphBitmap {
            width: metrics.width,
            height: metrics.height,
            left: metrics.xmin,
            // fontdue reports ymin from the baseline upwards; the painter wants
            // the distance from the baseline down to the bitmap's top edge.
            top: -(metrics.height as i32 + metrics.ymin),
            coverage,
        });
        self.glyphs.borrow_mut().insert(key, bitmap.clone());
        bitmap
    }

    /// Number of cached glyph bitmaps, exposed for diagnostics.
    pub fn cached_glyphs(&self) -> usize {
        self.glyphs.borrow().len()
    }

    /// Drops cached bitmaps, keeping resolved faces.
    pub fn clear_glyph_cache(&self) {
        self.glyphs.borrow_mut().clear();
    }
}

/// Splits `text` into runs that can each be placed or wrapped as a unit:
/// words, individual whitespace runs and explicit newlines.
///
/// Returns `(text, is_whitespace)` pairs in source order.
pub fn segment_words(text: &str) -> Vec<(&str, bool)> {
    let mut segments = Vec::new();
    let mut start = 0;
    let mut in_whitespace = None;

    for (index, ch) in text.char_indices() {
        let whitespace = ch.is_whitespace();
        match in_whitespace {
            None => in_whitespace = Some(whitespace),
            Some(previous) if previous != whitespace => {
                segments.push((&text[start..index], previous));
                start = index;
                in_whitespace = Some(whitespace);
            }
            _ => {}
        }
        // A newline is always its own segment so `pre` can break on it.
        if ch == '\n' {
            if start < index {
                segments.push((&text[start..index], true));
            }
            segments.push((&text[index..index + 1], true));
            start = index + 1;
            in_whitespace = None;
        }
    }
    if start < text.len() {
        segments.push((&text[start..], in_whitespace.unwrap_or(false)));
    }
    segments
}

/// Collapses each whitespace run to a single space, as CSS
/// `white-space: normal` requires.
///
/// Leading and trailing spaces are preserved: whether they survive depends on
/// where the run lands on a line, which only inline layout knows.
pub fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_whitespace = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !in_whitespace {
                out.push(' ');
                in_whitespace = true;
            }
            continue;
        }
        in_whitespace = false;
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real system fonts, when the machine has any.
    fn store() -> FontStore {
        FontStore::new()
    }

    #[test]
    fn synthetic_metrics_when_no_fonts() {
        let store = FontStore::empty();
        assert!(!store.has_fonts());
        let request = FontRequest::new(20.0);
        assert_eq!(store.advance(&request, 'x'), 10.0);
        assert_eq!(store.measure(&request, "abc"), 30.0);
        let metrics = store.line_metrics(&request);
        assert!(metrics.height() > 20.0);
        assert!(store.glyph(&request, 'x').is_empty());
    }

    #[test]
    fn measures_text_monotonically() {
        let store = store();
        let request = FontRequest::new(16.0);
        let short = store.measure(&request, "hi");
        let long = store.measure(&request, "hi there");
        assert!(long > short, "{long} should exceed {short}");
        assert_eq!(store.measure(&request, ""), 0.0);
    }

    #[test]
    fn larger_sizes_measure_wider() {
        let store = store();
        let small = store.measure(&FontRequest::new(10.0), "measure me");
        let large = store.measure(&FontRequest::new(30.0), "measure me");
        assert!(large > small * 2.0, "{large} vs {small}");
    }

    #[test]
    fn letter_and_word_spacing_add_up() {
        let store = store();
        let plain = FontRequest::new(16.0);
        let mut spaced = plain.clone();
        spaced.letter_spacing = 2.0;
        let text = "abcd";
        let difference = store.measure(&spaced, text) - store.measure(&plain, text);
        assert!((difference - 8.0).abs() < 0.01, "got {difference}");

        let mut word_spaced = plain.clone();
        word_spaced.word_spacing = 5.0;
        let with_spaces = "a b c";
        let difference =
            store.measure(&word_spaced, with_spaces) - store.measure(&plain, with_spaces);
        assert!((difference - 10.0).abs() < 0.01, "got {difference}");
    }

    #[test]
    fn shaping_positions_glyphs_in_order() {
        let store = store();
        let request = FontRequest::new(16.0);
        let run = store.shape(&request, "abc");
        assert_eq!(run.glyphs.len(), 3);
        assert_eq!(run.glyphs[0].x, 0.0);
        assert!(run.glyphs[1].x > 0.0);
        assert!(run.glyphs[2].x > run.glyphs[1].x);
        assert!((run.width - store.measure(&request, "abc")).abs() < 1.0);
    }

    #[test]
    fn line_metrics_are_positive() {
        let store = store();
        let metrics = store.line_metrics(&FontRequest::new(16.0));
        assert!(metrics.ascent > 0.0);
        assert!(metrics.descent >= 0.0);
        assert!(metrics.height() >= metrics.ascent);
    }

    #[test]
    fn glyph_cache_is_reused() {
        let store = store();
        if !store.has_fonts() {
            return;
        }
        let request = FontRequest::new(24.0);
        let first = store.glyph(&request, 'A');
        let cached = store.cached_glyphs();
        let second = store.glyph(&request, 'A');
        assert_eq!(store.cached_glyphs(), cached, "second call must hit cache");
        assert_eq!(first.width, second.width);
        assert!(!first.is_empty(), "'A' should rasterize to a bitmap");
    }

    #[test]
    fn whitespace_glyphs_are_blank_but_advance() {
        let store = store();
        let request = FontRequest::new(16.0);
        assert!(store.glyph(&request, ' ').is_empty());
        assert!(store.advance(&request, ' ') > 0.0);
    }

    #[test]
    fn generic_families_resolve() {
        let store = store();
        if !store.has_fonts() {
            return;
        }
        for family in ["sans-serif", "serif", "monospace", "system-ui"] {
            let request = FontRequest::new(16.0).with_families(vec![family.to_string()]);
            assert!(
                store.measure(&request, "test") > 0.0,
                "{family} produced no advance"
            );
        }
    }

    #[test]
    fn unknown_family_falls_back() {
        let store = store();
        let request =
            FontRequest::new(16.0).with_families(vec!["Definitely Not Installed".to_string()]);
        assert!(store.measure(&request, "test") > 0.0);
    }

    #[test]
    fn word_segmentation() {
        assert_eq!(
            segment_words("hello world"),
            vec![("hello", false), (" ", true), ("world", false)]
        );
        assert_eq!(
            segment_words("a  b"),
            vec![("a", false), ("  ", true), ("b", false)]
        );
        assert_eq!(
            segment_words("a\nb"),
            vec![("a", false), ("\n", true), ("b", false)]
        );
        assert_eq!(segment_words(""), Vec::<(&str, bool)>::new());
        assert_eq!(segment_words(" "), vec![(" ", true)]);
    }

    #[test]
    fn newlines_split_from_surrounding_spaces() {
        let segments = segment_words("a \n b");
        assert!(segments.contains(&("\n", true)));
        assert_eq!(segments.first(), Some(&("a", false)));
        assert_eq!(segments.last(), Some(&("b", false)));
    }

    #[test]
    fn whitespace_collapsing() {
        assert_eq!(collapse_whitespace("a   b"), "a b");
        assert_eq!(collapse_whitespace("a\n\tb"), "a b");
        assert_eq!(collapse_whitespace(""), "");
        // Edge whitespace collapses but survives; trimming is layout's call.
        assert_eq!(collapse_whitespace("  leading"), " leading");
        assert_eq!(collapse_whitespace("trailing  "), "trailing ");
    }
}
