# Theming

The entire interface is described by a TOML file. There is no separate settings
format and no compiled-in styling to work around: what the browser draws comes
from the values below.

## Using a theme

```sh
wat theme list                       # what ships with the browser
wat theme show liquid-glass > my.toml # start from the default
wat --theme my.toml                  # use it
wat shot about:home --theme my.toml -o preview.png   # see it without a window
```

`--theme` takes a preset name or a path to a `.toml` file. `--dark` and
`--light` override the theme's own `appearance`.

## Only list what you change

Every field has a default, and **the defaults are the Liquid Glass preset**. A
theme file is therefore a patch, not a full document:

```toml
name = "Liquid Glass (warm)"

[light.palette]
accent = "#ff7a1a"
accent_soft = "#ff7a1a26"

[light.glass]
blur = 34.0
saturation = 2.0
sheen = 0.45
```

Everything not mentioned — the dark variant, all the geometry, the typography —
keeps its Liquid Glass value.

Colours accept any CSS syntax: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`,
`rgb()`, `rgba()`, `hsl()`, `hsla()`, named colours and `transparent`.

## Structure

```toml
name = "…"                # shown on the home and settings pages
appearance = "auto"       # "auto" | "light" | "dark"

[geometry]     # radii, spacing and control sizes, shared by both variants
[typography]   # font stacks, sizes and weights, shared by both variants
[motion]       # animation preferences, shared by both variants

[light.palette]      [dark.palette]
[light.glass]        [dark.glass]
[light.glass.shadow] [dark.glass.shadow]
```

## `glass` — what makes glass look like glass

The chrome composites these in a fixed order. Every surface — panels, pills,
buttons, menus — is the same function, so a change here moves the whole
interface at once.

| Field | Default | Effect |
| --- | --- | --- |
| `blur` | `24.0` | Backdrop blur radius in pixels. `0` disables the glass. |
| `saturation` | `1.8` | Backdrop saturation. Above `1` makes colours bloom through. |
| `brightness` | `1.04` | Backdrop brightness. |
| `tint` | `#ffffff` | The wash laid over the blurred backdrop. |
| `tint_strength` | `0.42` | How much of `tint` to apply, `0`–`1`. |
| `sheen` | `0.34` | Strength of the top-down specular highlight. |
| `rim` | `0.55` | Strength of the inner rim light — this is what reads as the *thickness* of the glass. |
| `rim_color` | `#ffffff` | Colour of that rim light. |
| `edge` | `#ffffff59` | The hairline border around a surface. |

The layers go down in this order:

1. `shadow` — so the surface floats
2. `blur` + `saturation` + `brightness` — applied to what is *behind* the surface
3. `tint` × `tint_strength` — the wash
4. `sheen` — the highlight down the top
5. `rim` — the inner edge light
6. `edge` — the outer hairline

### `glass.shadow`

| Field | Default | Effect |
| --- | --- | --- |
| `offset_y` | `10.0` | Vertical offset in pixels. |
| `blur` | `28.0` | Shadow blur radius. |
| `spread` | `-6.0` | Grows (positive) or shrinks (negative) the shadow. |
| `color` | `#080c1838` | Shadow colour, alpha included. |

## `palette`

| Field | Used for |
| --- | --- |
| `canvas` | Behind everything, and what shows through the glass |
| `page` | Background for pages that set none of their own |
| `surface` | Solid surface colour, used when glass is switched off |
| `surface_raised` | A surface that should read as lifted |
| `text` | Primary chrome text |
| `text_muted` | Hints, placeholders, inactive tabs |
| `text_on_accent` | Text drawn on the accent colour |
| `accent` | Focus rings, the caret, the progress bar, links on internal pages |
| `accent_soft` | Fills behind text: the active tab, hovered menu rows |
| `border` | Hairline separators |
| `border_strong` | A border that needs to be noticed |
| `danger` | Errors, and the Quit menu entry |
| `success` | The secure-origin padlock |
| `warning` | The insecure-origin indicator |

## `geometry`

| Field | Default | Effect |
| --- | --- | --- |
| `radius_small` | `8.0` | Tabs, menu rows |
| `radius_medium` | `14.0` | Cards, the menu panel |
| `radius_large` | `22.0` | The chrome panels |
| `radius_pill` | `999.0` | Anything fully rounded |
| `spacing` | `8.0` | Base unit; most gaps are multiples of it |
| `border_width` | `1.0` | Hairline thickness. `0` removes every chrome border |
| `toolbar_height` | `52.0` | Desktop toolbar row |
| `tab_strip_height` | `40.0` | Desktop tab strip |
| `mobile_top_height` | `56.0` | The floating address pill |
| `mobile_bottom_height` | `64.0` | The bottom navigation bar |
| `control_size` | `34.0` | Round button diameter (scaled up for touch) |
| `progress_height` | `3.0` | Load progress bar |
| `chrome_inset` | `10.0` | How far the chrome floats from the window edge. `0` makes it flush |

## `typography`

| Field | Default |
| --- | --- |
| `ui_family` | `["Inter", "SF Pro Text", "Segoe UI", "system-ui", "sans-serif"]` |
| `mono_family` | `["SF Mono", "JetBrains Mono", "Consolas", "monospace"]` |
| `size_small` | `12.0` |
| `size_base` | `14.0` |
| `size_large` | `17.0` |
| `weight_normal` | `400` |
| `weight_medium` | `550` |
| `weight_bold` | `680` |

Family stacks are tried in order and fall back to whatever the system has. The
generic names `sans-serif`, `serif`, `monospace` and `system-ui` map to a list of
concrete candidates, so a stack ending in a generic name always resolves.

## `motion`

| Field | Default | Effect |
| --- | --- | --- |
| `enabled` | `true` | When false, the engine also reports `prefers-reduced-motion: reduce` to pages |
| `duration_fast` | `0.12` | Seconds, for hover feedback |
| `duration_base` | `0.24` | Seconds, for a panel opening |

## Recipes

**Heavier glass.** Push the blur up and the wash down, so more of the page shows
through:

```toml
[light.glass]
blur = 44.0
tint_strength = 0.28
saturation = 2.2
```

**No translucency at all.** This is exactly what the `flat` preset does:

```toml
[light.glass]
blur = 0.0
sheen = 0.0
rim = 0.0
tint_strength = 1.0
tint = "#ffffff"
saturation = 1.0
```

**Flush chrome, no floating panels.**

```toml
[geometry]
chrome_inset = 0.0
radius_large = 0.0
```

**Compact, for small screens.**

```toml
[geometry]
toolbar_height = 42.0
tab_strip_height = 32.0
control_size = 28.0
spacing = 6.0

[typography]
size_base = 13.0
size_small = 11.0
```

## Themed internal pages

`about:home`, `about:settings` and `about:version` are ordinary HTML rendered by
the same engine as the web, and their stylesheet is generated from the theme. A
theme change is reflected on them immediately, with no separate styling to keep
in sync. `about:settings` prints the glass parameters currently in effect, which
makes it a live preview of whatever file you are editing.

## Errors are reported, not ignored

An unparseable colour, an unknown field or a malformed file is an error with the
offending text in the message, rather than a silently dropped value:

```
$ wat --theme broken.toml
wat: `not-a-colour` is not a valid colour
```
