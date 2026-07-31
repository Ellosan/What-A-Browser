# Architecture

What-A-Browser is a browser engine plus a shell around it. This document
describes the pipeline, the seams between the crates, and — at the end — every
place the implementation knowingly departs from the specifications.

## The pipeline

An address becomes pixels in seven stages. Each stage is a separate crate with a
data-only interface, so any of them can be exercised in isolation.

```
              wat-net          wat-html        wat-css + wat-style
address  ──►  Resource  ──►  Document  ──►  StyleTree
                                                 │
                                    wat-layout   ▼
                              LayoutTree ◄── box tree construction
                                    │
                       wat-paint    ▼
                            DisplayList ──►  Canvas ──► window / PNG
```

1. **Load** (`wat-net`). `Address` validates and classifies a URL. `HttpLoader`
   fetches it — over HTTP(S), from the file system, or by decoding a `data:` URL.
   Anything that is not HTML is wrapped in generated HTML: text goes in a `<pre>`,
   an image gets a document built around it, and unsupported types get an
   explanatory page.
2. **Parse** (`wat-html`). A tokenizer produces tags, text, comments and
   doctypes; a reduced tree builder assembles them into a `Document`. Malformed
   markup never fails — it recovers the way browsers do.
3. **Cascade** (`wat-css`, `wat-style`). Author, user and user-agent stylesheets
   are matched against every element. Declarations are sorted by origin,
   importance, inline-ness, specificity and source order, then applied to produce
   one `ComputedStyle` per node. `font-size` is applied first so `em` lengths
   resolve correctly.
4. **Build boxes** (`wat-layout`). The styled tree becomes a box tree: block
   containers, inline formatting contexts, flex and grid containers, replaced
   elements, list markers, and anonymous boxes where block and inline siblings
   mix.
5. **Lay out** (`wat-layout`). Widths flow down, heights come back up. Inline
   content is broken into line boxes using real font metrics. Out-of-flow boxes
   are placed in a second pass against their containing blocks.
6. **Paint** (`wat-paint`). The positioned box tree becomes a flat
   `DisplayList` — a replayable list of fills, borders, shadows, text runs,
   images and backdrop filters, in CSS paint order.
7. **Rasterize** (`wat-paint`). The display list is replayed onto a `Canvas`,
   which is either presented in a window or encoded as a PNG.

## Why the display list exists

Layout and rasterization never talk to each other directly. That buys three
things:

* the chrome and the page produce the same kind of list, so one renderer draws
  both;
* `backdrop-filter` works, because filters are ordered operations on pixels that
  have already been drawn — the chrome's list is replayed *after* the page's, so
  the blur has real page pixels to sample;
* a frame can be inspected in a test without a framebuffer. Most of the painting
  tests assert on display list contents rather than pixels.

## The engine seam

`wat_engine::WebEngine` is the interface between the shell and whatever renders
web content: navigate, resize, scroll, hit-test, produce a frame. `Page` is this
repository's implementation. The shell holds `dyn WebEngine` and reaches past it
for nothing, so an alternative engine only has to implement the same trait.

## The rasterizer

There is no 2D graphics dependency. Shapes are described analytically and
evaluated per pixel:

* **Coverage from distance.** `RoundedRect::distance` is a signed distance field
  for a rectangle with four independent corner radii and an optional rotation
  about its centre. Coverage is `clamp(0.5 - distance, 0, 1)`, which gives one
  pixel of antialiasing for free.
* **Borders** are the difference between the outer shape's coverage and the
  inset shape's, with the edge colour chosen by which side of the box diagonals
  a pixel falls on. That is what makes per-side border colours work.
* **Shadows** rasterize the shape into an 8-bit mask, blur it, then composite it
  with the shape's own coverage subtracted so a box never shadows itself.
* **Blur** is three box passes, sized to approximate a Gaussian of the requested
  sigma, over premultiplied values so transparent pixels cannot bleed colour.
* **Backdrop filters** copy the region under a shape, blur and colour-adjust it,
  and write it back through the shape's coverage. Because it operates on the
  canvas rather than a layer, glass edges stay crisp.
* **Text** is 8-bit coverage masks from `fontdue`, cached per (face, character,
  quantised size) and blitted through the current clip.
* **Icons** are rotated rounded rectangles, which is why the interface needs no
  icon font or SVG parser.

Everything is CPU-side. There is no GPU requirement, which is also why the
headless CLI can produce byte-identical output to the window.

## Theming

`wat-theme` is data with no behaviour. A `Theme` carries shared geometry,
typography and motion plus a light and a dark `Variant`; each variant carries a
`Palette` and a `Glass`. `Theme::resolve` flattens that into the
`ResolvedTheme` the chrome draws from.

Every field has a `Default`, and the defaults *are* the Liquid Glass preset. Two
consequences: a user theme file only has to list what it changes, and a preset is
just a small TOML file. `wat-theme`'s tests assert that the shipped
`liquid-glass.toml` still equals `Theme::default()`, so the file cannot silently
become wrong documentation.

`glass::surface` composites the six glass layers in a fixed order — shadow,
backdrop filter, wash, sheen, rim, edge — each driven by a theme value. Setting
`blur`, `sheen` and `rim` to zero turns the same call into a flat rectangle,
which is all the `flat` preset does.

## The chrome

`wat-ui` computes a `ChromeGeometry` — the content rectangle, the glass panels,
and a list of `(WidgetId, Rect)` pairs — from the window size, the theme and the
tab count. Everything else follows from that structure:

* **Drawing** walks the geometry and emits a display list.
* **Hit testing** searches the widget list in reverse, so later widgets win.
* **Input** turns clicks and keys into `UiAction` values. The chrome never
  touches the engine; the shell applies the actions.

Because the chrome is a pure function of state, the entire interaction model is
tested without a window: 70 tests cover layout, hit testing, keyboard handling
and drawing.

`Browser` in `wat-shell` is the state machine that applies `UiAction`s to a
`Session`. It also has no windowing dependency, so navigation, tab management and
theme switching are tested headlessly. `wat-shell::window` is the only file that
knows about winit, and it does nothing but translate events and copy pixels.

## Deviations from the specifications

These are deliberate simplifications, not bugs to be filed. Each one is a place
where correct behaviour was traded for a smaller, comprehensible implementation.

### Scripting

* No JavaScript engine. `<script>` contents are parsed as raw text and never
  executed. There is no DOM API, no event dispatch beyond hover, and no dynamic
  layout.

### HTML

* The tree builder has no adoption agency algorithm. Mis-nested inline
  formatting (`<b>x<i>y</b>z</i>`) is closed rather than reconstructed; the text
  survives in order but the element nesting differs from a spec parser.
* No foster parenting. Content that a spec parser would move out of a table stays
  where it was written.
* `<template>`, `<frameset>` and namespaced (SVG/MathML) content are parsed as
  ordinary elements. SVG is not rendered.

### CSS

* `@supports` conditions are not evaluated; the block is applied as though every
  condition held.
* `revert` behaves as `unset`, since per-origin values are not retained after the
  cascade.
* `:visited` never matches, and `:has()` and other unmodelled pseudo-classes
  never match rather than matching by accident.
* `calc()` is parsed and preserved but not evaluated.
* Custom properties (`--x`) are parsed but not substituted.
* `ex` and `ch` units are approximated as half the font size instead of being
  read from font metrics.
* Elliptical border radii collapse to their horizontal component.

### Layout

* **Floats** do not affect line boxes. A floated element lays out as a block.
* **Tables** map to flex: a `table-row` becomes a flex line and its cells become
  flex items that share the width. Column sizing from content, `colspan` and
  `rowspan` are not implemented.
* **Grid** resolves `grid-template-columns` (including `repeat()` and `fr`) and
  gaps, and places items row-major. Grid areas, line placement, row templates and
  auto-placement rules are not implemented; `auto` tracks behave as `1fr`.
* Margin collapsing happens between siblings only, not between a parent and its
  first or last child.
* `align-content` on a multi-line flex container is parsed but lines are packed
  at the start.
* Baseline alignment in flex containers falls back to `flex-start`.
* Writing modes and bidirectional text are not implemented; layout is
  left-to-right.

### Painting

* Opacity is applied by scaling each item's alpha rather than compositing a
  group offscreen, so overlapping children inside a translucent subtree show
  through each other.
* `transform` applies translation only.
* Text shadows are drawn as offset translucent copies rather than blurred.
* Images are sampled nearest-neighbour.
* Radial gradients are approximated by a vertical linear gradient.

### Platform

* HiDPI scale factors are ignored; the window renders at one device pixel per
  CSS pixel.
* The system light/dark preference is not read, so `auto` resolves to light
  until the user switches with `Ctrl/Cmd + D` or a theme pins it.
* Inner scrollable elements record their scroll extent but only the page itself
  responds to scroll input.

## Testing strategy

* **Unit tests** live beside the code they cover, one module at a time.
* **Layout tests** use `FontStore::empty()`, whose synthetic metrics make every
  glyph exactly half the font size wide. Line breaking, wrapping and alignment
  can then be asserted to the pixel without depending on which fonts are
  installed.
* **Painting tests** assert on display lists where structure is what matters, and
  on pixels where the visual result is the point — that a backdrop filter really
  smooths what is behind it, that a rounded corner really is cut away.
* **Engine and chrome tests** use `StaticLoader` and `OfflineLoader`, so the
  whole suite runs with no network access.
* The CLI's own tests render `about:` pages to PNGs and decode them again,
  covering the pipeline end to end.
