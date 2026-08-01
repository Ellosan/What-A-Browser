//! A software canvas: analytic coverage, source-over compositing, separable
//! blur and glyph blitting.
//!
//! Everything is computed from signed distance fields rather than a path
//! rasterizer, which keeps rounded rectangles, borders and glass edges sharp
//! and antialiased without a 2D graphics dependency.

use wat_css::Color;
use wat_layout::geom::Rect;
use wat_style::{Corners, Filter, Sides};

/// A rounded rectangle used for filling and clipping.
///
/// It may be rotated about its own centre, which is what lets the chrome build
/// chevrons and crosses out of the same primitive as everything else.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundedRect {
    pub rect: Rect,
    pub radii: Corners<f32>,
    /// Clockwise rotation about the rectangle's centre, in radians.
    pub rotation: f32,
}

impl RoundedRect {
    pub fn new(rect: Rect, radii: Corners<f32>) -> Self {
        RoundedRect {
            rect,
            radii: clamp_radii(rect, radii),
            rotation: 0.0,
        }
    }

    pub fn sharp(rect: Rect) -> Self {
        RoundedRect {
            rect,
            radii: Corners::ZERO,
            rotation: 0.0,
        }
    }

    /// The same shape, rotated about its centre by `degrees`.
    pub fn rotated(self, degrees: f32) -> Self {
        RoundedRect {
            rotation: degrees.to_radians(),
            ..self
        }
    }

    /// The same shape with every length multiplied by `factor`.
    ///
    /// The rotation is an angle, so it is left alone; scaling is uniform, which
    /// means a rotated shape stays the same shape.
    pub fn scaled(&self, factor: f32) -> Self {
        RoundedRect {
            rect: Rect::new(
                self.rect.x * factor,
                self.rect.y * factor,
                self.rect.width * factor,
                self.rect.height * factor,
            ),
            radii: self.radii.map(|radius| radius * factor),
            rotation: self.rotation,
        }
    }

    /// The axis-aligned box that contains the shape, rotation included.
    pub fn bounding_rect(&self) -> Rect {
        if self.rotation == 0.0 {
            return self.rect;
        }
        let (sin, cos) = self.rotation.sin_cos();
        let half_w = self.rect.width / 2.0;
        let half_h = self.rect.height / 2.0;
        let extent_x = half_w * cos.abs() + half_h * sin.abs();
        let extent_y = half_w * sin.abs() + half_h * cos.abs();
        let center = self.rect.center();
        Rect::new(
            center.x - extent_x,
            center.y - extent_y,
            extent_x * 2.0,
            extent_y * 2.0,
        )
    }

    pub fn is_sharp(&self) -> bool {
        self.radii.is_zero()
    }

    /// Shrinks the shape by `sides`, reducing the radii to match.
    pub fn inset(&self, sides: Sides<f32>) -> RoundedRect {
        let rect = self.rect.inset(sides);
        let shrink = |radius: f32, a: f32, b: f32| (radius - (a + b) / 2.0).max(0.0);
        RoundedRect {
            rotation: self.rotation,
            ..RoundedRect::new(
                rect,
                Corners {
                    top_left: shrink(self.radii.top_left, sides.top, sides.left),
                    top_right: shrink(self.radii.top_right, sides.top, sides.right),
                    bottom_right: shrink(self.radii.bottom_right, sides.bottom, sides.right),
                    bottom_left: shrink(self.radii.bottom_left, sides.bottom, sides.left),
                },
            )
        }
    }

    /// Grows the shape by `amount` on every side.
    pub fn expand(&self, amount: f32) -> RoundedRect {
        RoundedRect {
            rotation: self.rotation,
            ..RoundedRect::new(
                self.rect.expand(amount),
                self.radii.map(|r| (r + amount).max(0.0)),
            )
        }
    }

    /// Signed distance to the shape's edge; negative inside.
    pub fn distance(&self, x: f32, y: f32) -> f32 {
        let center = self.rect.center();
        let half_w = self.rect.width / 2.0;
        let half_h = self.rect.height / 2.0;
        let (mut px, mut py) = (x - center.x, y - center.y);
        if self.rotation != 0.0 {
            // Rotate the sample point into the rectangle's own frame.
            let (sin, cos) = (-self.rotation).sin_cos();
            let rotated_x = px * cos - py * sin;
            let rotated_y = px * sin + py * cos;
            px = rotated_x;
            py = rotated_y;
        }

        let radius = if px < 0.0 {
            if py < 0.0 {
                self.radii.top_left
            } else {
                self.radii.bottom_left
            }
        } else if py < 0.0 {
            self.radii.top_right
        } else {
            self.radii.bottom_right
        }
        .min(half_w.min(half_h))
        .max(0.0);

        let qx = px.abs() - (half_w - radius);
        let qy = py.abs() - (half_h - radius);
        let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
        qx.max(qy).min(0.0) + outside - radius
    }

    /// Antialiased coverage of the pixel centred at `(x, y)`.
    pub fn coverage(&self, x: f32, y: f32) -> f32 {
        if self.rect.is_empty() {
            return 0.0;
        }
        (0.5 - self.distance(x, y)).clamp(0.0, 1.0)
    }

    /// A rectangle lying entirely inside the shape.
    ///
    /// Exact for square corners. For rounded ones it insets every side by the
    /// largest radius, which is conservative — a rounded rectangle contains the
    /// box inset that far, and giving up a band a few pixels wide costs nothing
    /// next to what filling its interior analytically costs. `None` for a
    /// rotated shape, whose interior is not axis-aligned.
    pub fn solid_rect(&self) -> Option<Rect> {
        if self.rotation != 0.0 || self.rect.is_empty() {
            return None;
        }
        let inset = self.radii.max();
        if inset <= 0.0 {
            return Some(self.rect);
        }
        let rect = self.rect.inset(Sides::all(inset));
        (!rect.is_empty()).then_some(rect)
    }
}

/// Radii cannot exceed the box; CSS scales them down proportionally.
fn clamp_radii(rect: Rect, radii: Corners<f32>) -> Corners<f32> {
    let mut radii = radii.map(|r| r.max(0.0));
    let scale = [
        (radii.top_left + radii.top_right, rect.width),
        (radii.bottom_left + radii.bottom_right, rect.width),
        (radii.top_left + radii.bottom_left, rect.height),
        (radii.top_right + radii.bottom_right, rect.height),
    ]
    .iter()
    .filter(|(sum, _)| *sum > 0.0)
    .map(|(sum, extent)| (extent / sum).min(1.0))
    .fold(1.0f32, f32::min);
    if scale < 1.0 {
        radii = radii.map(|r| r * scale);
    }
    radii
}

/// Antialiased coverage of a pixel against a plain axis-aligned rectangle.
///
/// This is `RoundedRect::sharp(rect).coverage(x, y)` with the corner-radius and
/// rotation work stripped out. It is worth having separately because building a
/// `RoundedRect` runs [`clamp_radii`], and the clip evaluated one per pixel per
/// draw — several million times a frame, to reach the same answer this reaches
/// with a handful of instructions.
#[inline]
fn sharp_coverage(rect: Rect, x: f32, y: f32) -> f32 {
    if rect.is_empty() {
        return 0.0;
    }
    let center = rect.center();
    let qx = (x - center.x).abs() - rect.width / 2.0;
    let qy = (y - center.y).abs() - rect.height / 2.0;
    let (ox, oy) = (qx.max(0.0), qy.max(0.0));
    let distance = qx.max(qy).min(0.0) + (ox * ox + oy * oy).sqrt();
    (0.5 - distance).clamp(0.0, 1.0)
}

/// A clip region: the intersection of a stack of rounded rectangles.
#[derive(Clone, Debug, Default)]
pub struct Clip {
    shapes: Vec<RoundedRect>,
    /// Bounding box of the intersection, for cheap rejection.
    bounds: Option<Rect>,
}

impl Clip {
    pub fn unbounded() -> Self {
        Clip {
            shapes: Vec::new(),
            bounds: None,
        }
    }

    pub fn from_rect(rect: Rect) -> Self {
        // The bounding box already describes a sharp rectangle exactly, so
        // keeping one in `shapes` as well would evaluate the same edge twice —
        // which also squared the coverage at the boundary instead of leaving it
        // alone. `push` has always skipped sharp rectangles for this reason.
        Clip {
            shapes: Vec::new(),
            bounds: Some(rect),
        }
    }

    pub fn push(&mut self, shape: RoundedRect) {
        self.bounds = Some(match self.bounds {
            Some(existing) => existing.intersection(&shape.rect).unwrap_or(Rect::ZERO),
            None => shape.rect,
        });
        // A sharp rectangle is already covered by the bounding box.
        if !shape.is_sharp() {
            self.shapes.push(shape);
        }
    }

    pub fn bounds(&self) -> Option<Rect> {
        self.bounds
    }

    pub fn is_empty(&self) -> bool {
        self.bounds.is_some_and(|b| b.is_empty())
    }

    /// A rectangle the clip covers completely, if it has one.
    pub fn solid_rect(&self) -> Option<Rect> {
        let mut area = self.bounds?;
        for shape in &self.shapes {
            area = area.intersection(&shape.solid_rect()?)?;
        }
        (!area.is_empty()).then_some(area)
    }

    /// Coverage of the pixel at `(x, y)`, in `0.0..=1.0`.
    pub fn coverage(&self, x: f32, y: f32) -> f32 {
        if let Some(bounds) = self.bounds {
            if x < bounds.x - 0.5
                || y < bounds.y - 0.5
                || x > bounds.max_x() + 0.5
                || y > bounds.max_y() + 0.5
            {
                return 0.0;
            }
        }
        let mut coverage = match self.bounds {
            Some(bounds) => sharp_coverage(bounds, x, y),
            None => 1.0,
        };
        for shape in &self.shapes {
            if coverage <= 0.0 {
                break;
            }
            coverage *= shape.coverage(x, y);
        }
        coverage
    }
}

/// A linear gradient in canvas space.
#[derive(Clone, Debug, PartialEq)]
pub struct LinearGradient {
    /// Start and end of the gradient line.
    pub start: (f32, f32),
    pub end: (f32, f32),
    /// Stops with positions in `0.0..=1.0`, sorted.
    pub stops: Vec<(f32, Color)>,
}

impl LinearGradient {
    /// The same gradient with its line multiplied by `factor`.
    ///
    /// The stop positions are fractions of that line, so they do not scale.
    pub fn scaled(&self, factor: f32) -> Self {
        LinearGradient {
            start: (self.start.0 * factor, self.start.1 * factor),
            end: (self.end.0 * factor, self.end.1 * factor),
            stops: self.stops.clone(),
        }
    }

    /// Builds the gradient line CSS defines for `angle` over `rect`.
    ///
    /// `angle` is in degrees, 0 pointing up and increasing clockwise.
    pub fn for_angle(rect: Rect, angle: f32, stops: Vec<(f32, Color)>) -> Self {
        let radians = angle.to_radians();
        let (sin, cos) = radians.sin_cos();
        // Length of the gradient line for this angle, per the CSS definition.
        let length = (rect.width * sin.abs() + rect.height * cos.abs()) / 2.0;
        let center = rect.center();
        LinearGradient {
            start: (center.x - sin * length, center.y + cos * length),
            end: (center.x + sin * length, center.y - cos * length),
            stops,
        }
    }

    fn sample(&self, x: f32, y: f32) -> Color {
        if self.stops.is_empty() {
            return Color::TRANSPARENT;
        }
        let dx = self.end.0 - self.start.0;
        let dy = self.end.1 - self.start.1;
        let length_squared = dx * dx + dy * dy;
        let t = if length_squared <= f32::EPSILON {
            0.0
        } else {
            (((x - self.start.0) * dx + (y - self.start.1) * dy) / length_squared).clamp(0.0, 1.0)
        };

        let mut previous = self.stops[0];
        if t <= previous.0 {
            return previous.1;
        }
        for stop in &self.stops[1..] {
            if t <= stop.0 {
                let span = stop.0 - previous.0;
                let local = if span <= f32::EPSILON {
                    0.0
                } else {
                    (t - previous.0) / span
                };
                return previous.1.mix(stop.1, local);
            }
            previous = *stop;
        }
        previous.1
    }
}

/// An RGBA8 canvas with straight (non-premultiplied) alpha.
#[derive(Clone)]
pub struct Canvas {
    width: u32,
    height: u32,
    /// `width * height * 4` bytes, RGBA order.
    pixels: Vec<u8>,
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Self {
        Canvas {
            width,
            height,
            pixels: vec![0; (width as usize) * (height as usize) * 4],
        }
    }

    pub fn filled(width: u32, height: u32, color: Color) -> Self {
        let mut canvas = Canvas::new(width, height);
        canvas.clear(color);
        canvas
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }

    pub fn bounds(&self) -> Rect {
        Rect::new(0.0, 0.0, self.width as f32, self.height as f32)
    }

    pub fn clear(&mut self, color: Color) {
        for chunk in self.pixels.chunks_exact_mut(4) {
            chunk[0] = color.r;
            chunk[1] = color.g;
            chunk[2] = color.b;
            chunk[3] = color.a;
        }
    }

    fn index(&self, x: u32, y: u32) -> usize {
        ((y as usize) * (self.width as usize) + x as usize) * 4
    }

    pub fn pixel(&self, x: u32, y: u32) -> Color {
        if x >= self.width || y >= self.height {
            return Color::TRANSPARENT;
        }
        let i = self.index(x, y);
        Color::rgba(
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        )
    }

    pub fn set_pixel(&mut self, x: u32, y: u32, color: Color) {
        if x >= self.width || y >= self.height {
            return;
        }
        let i = self.index(x, y);
        self.pixels[i] = color.r;
        self.pixels[i + 1] = color.g;
        self.pixels[i + 2] = color.b;
        self.pixels[i + 3] = color.a;
    }

    /// Blends `color` over the pixel using source-over with extra `coverage`.
    pub fn blend(&mut self, x: u32, y: u32, color: Color, coverage: f32) {
        if coverage <= 0.0 || color.a == 0 {
            return;
        }
        let source = if coverage >= 1.0 {
            color
        } else {
            color.scale_alpha(coverage)
        };
        let blended = source.over(self.pixel(x, y));
        self.set_pixel(x, y, blended);
    }

    /// The integer pixel range a shape touches, clipped to the canvas.
    fn pixel_range(&self, rect: Rect, clip: &Clip) -> Option<(u32, u32, u32, u32)> {
        let mut area = rect;
        if let Some(bounds) = clip.bounds() {
            area = area.intersection(&bounds)?;
        }
        let area = area.intersection(&self.bounds())?;
        let x0 = area.x.floor().max(0.0) as u32;
        let y0 = area.y.floor().max(0.0) as u32;
        let x1 = (area.max_x().ceil() as u32).min(self.width);
        let y1 = (area.max_y().ceil() as u32).min(self.height);
        (x1 > x0 && y1 > y0).then_some((x0, y0, x1, y1))
    }

    /// The pixels of `rect` whose centres lie at least half a pixel inside it.
    ///
    /// Given a rectangle known to be covered completely, these are the pixels
    /// covered completely: for a pixel at `x` that means `x >= rect.x` and
    /// `x <= rect.max_x() - 1`.
    fn solid_box(&self, rect: Rect) -> Option<(u32, u32, u32, u32)> {
        let area = rect.intersection(&self.bounds())?;
        let x0 = area.x.ceil().max(0.0) as u32;
        let y0 = area.y.ceil().max(0.0) as u32;
        let x1 = (((area.max_x() - 1.0).floor() + 1.0).max(0.0) as u32).min(self.width);
        let y1 = (((area.max_y() - 1.0).floor() + 1.0).max(0.0) as u32).min(self.height);
        (x1 > x0 && y1 > y0).then_some((x0, y0, x1, y1))
    }

    /// Writes `color` across one row, skipping the blend when it is opaque.
    fn fill_row(&mut self, y: u32, x0: u32, x1: u32, color: Color) {
        if color.a < 255 {
            for x in x0..x1 {
                self.blend(x, y, color, 1.0);
            }
            return;
        }
        let start = self.index(x0, y);
        let end = start + (x1 - x0) as usize * 4;
        for chunk in self.pixels[start..end].chunks_exact_mut(4) {
            chunk[0] = color.r;
            chunk[1] = color.g;
            chunk[2] = color.b;
            chunk[3] = 255;
        }
    }

    /// Fills a rounded rectangle.
    pub fn fill(&mut self, shape: RoundedRect, color: Color, clip: &Clip) {
        if color.a == 0 {
            return;
        }
        let Some((x0, y0, x1, y1)) = self.pixel_range(shape.bounding_rect().expand(1.0), clip)
        else {
            return;
        };

        // Almost every pixel a page draws is in the interior of an unrotated
        // rectangle, where the coverage is exactly one. Evaluating a signed
        // distance for each of them anyway is what made a single full-window
        // fill take tens of milliseconds, so the interior is found once and
        // filled with no geometry at all; only the antialiased fringe around it
        // still needs the general path. Rounded corners narrow the interior but
        // do not remove it, which matters because the page itself is drawn under
        // a rounded clip.
        let interior = shape
            .solid_rect()
            .zip(clip.solid_rect())
            .and_then(|(shape_solid, clip_solid)| shape_solid.intersection(&clip_solid))
            .and_then(|solid| self.solid_box(solid));

        for y in y0..y1 {
            let py = y as f32 + 0.5;
            // The fully-covered columns on this row, empty if it has none.
            let (solid_start, solid_end) = match interior {
                Some((ix0, iy0, ix1, iy1)) if y >= iy0 && y < iy1 => {
                    (ix0.clamp(x0, x1), ix1.clamp(x0, x1))
                }
                _ => (x1, x1),
            };
            let general = |canvas: &mut Self, from: u32, to: u32| {
                for x in from..to {
                    let px = x as f32 + 0.5;
                    let coverage = shape.coverage(px, py) * clip.coverage(px, py);
                    canvas.blend(x, y, color, coverage);
                }
            };
            general(self, x0, solid_start);
            if solid_end > solid_start {
                self.fill_row(y, solid_start, solid_end, color);
            }
            general(self, solid_end.max(solid_start), x1);
        }
    }

    /// Fills a rounded rectangle with a linear gradient.
    pub fn fill_gradient(&mut self, shape: RoundedRect, gradient: &LinearGradient, clip: &Clip) {
        let Some((x0, y0, x1, y1)) = self.pixel_range(shape.bounding_rect().expand(1.0), clip)
        else {
            return;
        };
        for y in y0..y1 {
            let py = y as f32 + 0.5;
            for x in x0..x1 {
                let px = x as f32 + 0.5;
                let coverage = shape.coverage(px, py) * clip.coverage(px, py);
                if coverage <= 0.0 {
                    continue;
                }
                self.blend(x, y, gradient.sample(px, py), coverage);
            }
        }
    }

    /// Draws the ring between `outer` and its inset by `widths`.
    ///
    /// Each edge is painted separately so per-side colours work; the corner
    /// wedges are split along the diagonals.
    pub fn stroke_border(
        &mut self,
        outer: RoundedRect,
        widths: Sides<f32>,
        colors: Sides<Color>,
        clip: &Clip,
    ) {
        if widths.top <= 0.0 && widths.right <= 0.0 && widths.bottom <= 0.0 && widths.left <= 0.0 {
            return;
        }
        let inner = outer.inset(widths);
        let Some((x0, y0, x1, y1)) = self.pixel_range(outer.rect.expand(1.0), clip) else {
            return;
        };
        let center = outer.rect.center();

        for y in y0..y1 {
            let py = y as f32 + 0.5;
            for x in x0..x1 {
                let px = x as f32 + 0.5;
                let ring = (outer.coverage(px, py) - inner.coverage(px, py)).clamp(0.0, 1.0);
                if ring <= 0.0 {
                    continue;
                }
                let coverage = ring * clip.coverage(px, py);
                if coverage <= 0.0 {
                    continue;
                }
                // Choose the edge whose side of the diagonal this pixel is on.
                let dx = (px - center.x) / outer.rect.width.max(1.0);
                let dy = (py - center.y) / outer.rect.height.max(1.0);
                let (color, width) = if dy.abs() >= dx.abs() {
                    if dy < 0.0 {
                        (colors.top, widths.top)
                    } else {
                        (colors.bottom, widths.bottom)
                    }
                } else if dx < 0.0 {
                    (colors.left, widths.left)
                } else {
                    (colors.right, widths.right)
                };
                if width <= 0.0 {
                    continue;
                }
                self.blend(x, y, color, coverage);
            }
        }
    }

    /// Draws an 8-bit coverage mask in a single colour.
    ///
    /// `left`/`top` are the mask's top-left corner in canvas coordinates.
    #[allow(clippy::too_many_arguments)]
    pub fn blit_mask(
        &mut self,
        left: i32,
        top: i32,
        mask_width: usize,
        mask_height: usize,
        mask: &[u8],
        color: Color,
        clip: &Clip,
    ) {
        if mask_width == 0 || mask_height == 0 || color.a == 0 {
            return;
        }
        for row in 0..mask_height {
            let y = top + row as i32;
            if y < 0 || y >= self.height as i32 {
                continue;
            }
            for column in 0..mask_width {
                let x = left + column as i32;
                if x < 0 || x >= self.width as i32 {
                    continue;
                }
                let alpha = mask[row * mask_width + column];
                if alpha == 0 {
                    continue;
                }
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let coverage = (alpha as f32 / 255.0) * clip.coverage(px, py);
                self.blend(x as u32, y as u32, color, coverage);
            }
        }
    }

    /// Draws RGBA8 image pixels scaled into `shape`.
    pub fn draw_image(
        &mut self,
        shape: RoundedRect,
        image_width: u32,
        image_height: u32,
        image: &[u8],
        clip: &Clip,
    ) {
        if image_width == 0 || image_height == 0 || shape.rect.is_empty() {
            return;
        }
        let Some((x0, y0, x1, y1)) = self.pixel_range(shape.bounding_rect().expand(1.0), clip)
        else {
            return;
        };
        let scale_x = image_width as f32 / shape.rect.width;
        let scale_y = image_height as f32 / shape.rect.height;

        for y in y0..y1 {
            let py = y as f32 + 0.5;
            for x in x0..x1 {
                let px = x as f32 + 0.5;
                let coverage = shape.coverage(px, py) * clip.coverage(px, py);
                if coverage <= 0.0 {
                    continue;
                }
                // Nearest-neighbour sampling keeps this dependency-free; the
                // source is usually already close to the drawn size.
                let sx = (((px - shape.rect.x) * scale_x) as u32).min(image_width - 1);
                let sy = (((py - shape.rect.y) * scale_y) as u32).min(image_height - 1);
                let offset = ((sy as usize) * (image_width as usize) + sx as usize) * 4;
                if offset + 3 >= image.len() {
                    continue;
                }
                let color = Color::rgba(
                    image[offset],
                    image[offset + 1],
                    image[offset + 2],
                    image[offset + 3],
                );
                self.blend(x, y, color, coverage);
            }
        }
    }

    /// Draws a drop shadow for `shape`.
    pub fn drop_shadow(
        &mut self,
        shape: RoundedRect,
        offset: (f32, f32),
        spread: f32,
        blur_radius: f32,
        color: Color,
        clip: &Clip,
    ) {
        if color.a == 0 {
            return;
        }
        let shadow_shape = shape.expand(spread).translated(offset.0, offset.1);
        // The blur reaches roughly the blur radius beyond the shape.
        let padding = blur_radius + 2.0;
        let area = shadow_shape.rect.expand(padding);

        let Some((x0, y0, x1, y1)) = self.pixel_range(area, clip) else {
            return;
        };
        let mask_width = (x1 - x0) as usize;
        let mask_height = (y1 - y0) as usize;
        let mut mask = vec![0u8; mask_width * mask_height];
        for row in 0..mask_height {
            let py = (y0 + row as u32) as f32 + 0.5;
            for column in 0..mask_width {
                let px = (x0 + column as u32) as f32 + 0.5;
                mask[row * mask_width + column] =
                    (shadow_shape.coverage(px, py) * 255.0).round() as u8;
            }
        }
        if blur_radius > 0.0 {
            blur_alpha(&mut mask, mask_width, mask_height, blur_radius / 2.0);
        }

        for row in 0..mask_height {
            let y = y0 + row as u32;
            let py = y as f32 + 0.5;
            for column in 0..mask_width {
                let x = x0 + column as u32;
                let alpha = mask[row * mask_width + column];
                if alpha == 0 {
                    continue;
                }
                let px = x as f32 + 0.5;
                // The shadow does not show through the box that casts it.
                let occluded = shape.coverage(px, py);
                let coverage = (alpha as f32 / 255.0) * (1.0 - occluded) * clip.coverage(px, py);
                self.blend(x, y, color, coverage);
            }
        }
    }

    /// Draws an inner shadow, hugging the inside of `shape`'s edge.
    pub fn inner_shadow(
        &mut self,
        shape: RoundedRect,
        offset: (f32, f32),
        spread: f32,
        blur_radius: f32,
        color: Color,
        clip: &Clip,
    ) {
        if color.a == 0 {
            return;
        }
        let hole = shape
            .inset(Sides::all(spread))
            .translated(offset.0, offset.1);
        let Some((x0, y0, x1, y1)) = self.pixel_range(shape.bounding_rect().expand(1.0), clip)
        else {
            return;
        };
        let mask_width = (x1 - x0) as usize;
        let mask_height = (y1 - y0) as usize;
        // The mask is the *complement* of the offset shape, blurred, then
        // limited to the inside of the original.
        let mut mask = vec![0u8; mask_width * mask_height];
        for row in 0..mask_height {
            let py = (y0 + row as u32) as f32 + 0.5;
            for column in 0..mask_width {
                let px = (x0 + column as u32) as f32 + 0.5;
                let inside = hole.coverage(px, py);
                mask[row * mask_width + column] = ((1.0 - inside) * 255.0).round() as u8;
            }
        }
        if blur_radius > 0.0 {
            blur_alpha(&mut mask, mask_width, mask_height, blur_radius / 2.0);
        }

        for row in 0..mask_height {
            let y = y0 + row as u32;
            let py = y as f32 + 0.5;
            for column in 0..mask_width {
                let x = x0 + column as u32;
                let alpha = mask[row * mask_width + column];
                if alpha == 0 {
                    continue;
                }
                let px = x as f32 + 0.5;
                let coverage =
                    (alpha as f32 / 255.0) * shape.coverage(px, py) * clip.coverage(px, py);
                self.blend(x, y, color, coverage);
            }
        }
    }

    /// Applies a filter to what is already on the canvas inside `shape`.
    ///
    /// This is the primitive behind `backdrop-filter`, and so behind the whole
    /// Liquid Glass look: the pixels *under* a surface are blurred and
    /// saturated in place before the surface's own tint is drawn on top.
    pub fn filter_region(&mut self, shape: RoundedRect, filter: Filter, clip: &Clip) {
        if filter.is_none() {
            return;
        }
        let padding = filter.blur * 3.0 + 2.0;
        let sample_area = shape.rect.expand(padding);
        let Some((sx0, sy0, sx1, sy1)) = self.pixel_range(sample_area, &Clip::unbounded()) else {
            return;
        };
        let region_width = (sx1 - sx0) as usize;
        let region_height = (sy1 - sy0) as usize;
        if region_width == 0 || region_height == 0 {
            return;
        }

        // Copy the backdrop out, filter it, then blend it back through the
        // shape's coverage so the edges stay crisp.
        let mut region = vec![0f32; region_width * region_height * 4];
        for row in 0..region_height {
            for column in 0..region_width {
                let color = self.pixel(sx0 + column as u32, sy0 + row as u32);
                let offset = (row * region_width + column) * 4;
                // Premultiply so blurring cannot bleed colour from transparent
                // pixels.
                let alpha = color.alpha_f32();
                region[offset] = color.r as f32 / 255.0 * alpha;
                region[offset + 1] = color.g as f32 / 255.0 * alpha;
                region[offset + 2] = color.b as f32 / 255.0 * alpha;
                region[offset + 3] = alpha;
            }
        }

        if filter.blur > 0.0 {
            blur_rgba_f32(&mut region, region_width, region_height, filter.blur / 2.0);
        }

        for row in 0..region_height {
            let y = sy0 + row as u32;
            let py = y as f32 + 0.5;
            for column in 0..region_width {
                let x = sx0 + column as u32;
                let px = x as f32 + 0.5;
                let coverage = shape.coverage(px, py) * clip.coverage(px, py);
                if coverage <= 0.0 {
                    continue;
                }
                let offset = (row * region_width + column) * 4;
                let alpha = region[offset + 3];
                if alpha <= 0.0 {
                    continue;
                }
                // Un-premultiply, then apply the colour adjustments.
                let mut r = region[offset] / alpha;
                let mut g = region[offset + 1] / alpha;
                let mut b = region[offset + 2] / alpha;
                if filter.grayscale > 0.0 {
                    let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                    r += (luma - r) * filter.grayscale;
                    g += (luma - g) * filter.grayscale;
                    b += (luma - b) * filter.grayscale;
                }
                if (filter.saturate - 1.0).abs() > f32::EPSILON {
                    let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                    r = luma + (r - luma) * filter.saturate;
                    g = luma + (g - luma) * filter.saturate;
                    b = luma + (b - luma) * filter.saturate;
                }
                if (filter.brightness - 1.0).abs() > f32::EPSILON {
                    r *= filter.brightness;
                    g *= filter.brightness;
                    b *= filter.brightness;
                }
                if (filter.contrast - 1.0).abs() > f32::EPSILON {
                    r = (r - 0.5) * filter.contrast + 0.5;
                    g = (g - 0.5) * filter.contrast + 0.5;
                    b = (b - 0.5) * filter.contrast + 0.5;
                }
                let filtered = Color::from_f32(r, g, b, alpha);
                // Replace rather than blend: this *is* the backdrop.
                let existing = self.pixel(x, y);
                self.set_pixel(x, y, existing.mix(filtered, coverage));
            }
        }
    }

    /// Encodes the canvas as a PNG.
    pub fn to_png(&self) -> Result<Vec<u8>, String> {
        let buffer = image::RgbaImage::from_raw(self.width, self.height, self.pixels.clone())
            .ok_or_else(|| "canvas dimensions do not match its buffer".to_string())?;
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(buffer)
            .write_to(&mut out, image::ImageFormat::Png)
            .map_err(|error| error.to_string())?;
        Ok(out.into_inner())
    }

    /// Copies the canvas into a `0xAARRGGBB` buffer, which is what the
    /// platform window surfaces expect.
    pub fn to_argb32(&self) -> Vec<u32> {
        self.pixels
            .chunks_exact(4)
            .map(|p| {
                (u32::from(p[3]) << 24)
                    | (u32::from(p[0]) << 16)
                    | (u32::from(p[1]) << 8)
                    | u32::from(p[2])
            })
            .collect()
    }
}

impl RoundedRect {
    fn translated(&self, dx: f32, dy: f32) -> RoundedRect {
        RoundedRect {
            rect: self.rect.translate(dx, dy),
            ..*self
        }
    }
}

/// Box sizes that approximate a Gaussian of `sigma` in three passes.
fn box_sizes_for_sigma(sigma: f32) -> [usize; 3] {
    if sigma <= 0.0 {
        return [0, 0, 0];
    }
    let ideal = (12.0 * sigma * sigma / 3.0 + 1.0).sqrt();
    let mut lower = ideal.floor() as i32;
    if lower % 2 == 0 {
        lower -= 1;
    }
    let upper = lower + 2;
    let ideal_m = (12.0 * sigma * sigma - (3 * lower * lower) as f32 - (12 * lower) as f32 - 9.0)
        / (-4.0 * lower as f32 - 4.0);
    let m = ideal_m.round() as i32;
    let mut sizes = [0usize; 3];
    for (index, size) in sizes.iter_mut().enumerate() {
        let value = if (index as i32) < m { lower } else { upper };
        *size = value.max(1) as usize;
    }
    sizes
}

/// Three-pass box blur of a single-channel buffer, in place.
pub fn blur_alpha(data: &mut [u8], width: usize, height: usize, sigma: f32) {
    if sigma <= 0.0 || width == 0 || height == 0 {
        return;
    }
    let mut buffer: Vec<f32> = data.iter().map(|v| *v as f32).collect();
    // A Gaussian is approximated by three box passes; they share one scratch
    // buffer rather than each allocating a copy of the whole region.
    let mut scratch = vec![0.0f32; buffer.len()];
    for size in box_sizes_for_sigma(sigma) {
        let radius = size / 2;
        if radius == 0 {
            continue;
        }
        box_blur_f32(&mut buffer, &mut scratch, width, height, radius, 1);
    }
    for (target, value) in data.iter_mut().zip(buffer) {
        *target = value.clamp(0.0, 255.0).round() as u8;
    }
}

/// Three-pass box blur of premultiplied RGBA floats, in place.
pub fn blur_rgba_f32(data: &mut [f32], width: usize, height: usize, sigma: f32) {
    if sigma <= 0.0 || width == 0 || height == 0 {
        return;
    }
    let mut scratch = vec![0.0f32; data.len()];
    for size in box_sizes_for_sigma(sigma) {
        let radius = size / 2;
        if radius == 0 {
            continue;
        }
        box_blur_f32(data, &mut scratch, width, height, radius, 4);
    }
}

/// One separable box blur pass over `channels`-interleaved data.
fn box_blur_f32(
    data: &mut [f32],
    scratch: &mut [f32],
    width: usize,
    height: usize,
    radius: usize,
    channels: usize,
) {
    // Horizontal pass.
    for row in 0..height {
        let row_start = row * width * channels;
        for channel in 0..channels {
            let mut sum = 0.0f32;
            let window = (radius * 2 + 1) as f32;
            // Prime the accumulator with the clamped left edge.
            for offset in 0..=radius {
                let index = offset.min(width - 1);
                sum += data[row_start + index * channels + channel];
            }
            sum += data[row_start + channel] * radius as f32;

            for column in 0..width {
                scratch[row_start + column * channels + channel] = sum / window;
                let leaving = column.saturating_sub(radius);
                let entering = (column + radius + 1).min(width - 1);
                sum -= data[row_start + leaving * channels + channel];
                sum += data[row_start + entering * channels + channel];
            }
        }
    }

    // Vertical pass, walked a row at a time rather than a column at a time.
    //
    // Sliding a window down one column touches a different cache line for every
    // pixel it reads, and for a window-sized region that missed on essentially
    // every access. Keeping one running sum per column instead means both reads
    // and writes go straight along memory. The arithmetic per column is
    // unchanged, and so is the result.
    let stride = width * channels;
    let window = (radius * 2 + 1) as f32;
    let mut sums = vec![0.0f32; stride];
    for offset in 0..=radius {
        let row = offset.min(height - 1);
        for (sum, value) in sums.iter_mut().zip(&scratch[row * stride..][..stride]) {
            *sum += *value;
        }
    }
    for (sum, value) in sums.iter_mut().zip(&scratch[..stride]) {
        *sum += *value * radius as f32;
    }
    for row in 0..height {
        let leaving = row.saturating_sub(radius) * stride;
        let entering = (row + radius + 1).min(height - 1) * stride;
        let out = row * stride;
        for index in 0..stride {
            data[out + index] = sums[index] / window;
            sums[index] -= scratch[leaving + index];
            sums[index] += scratch[entering + index];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(x: f32, y: f32, w: f32, h: f32, r: f32) -> RoundedRect {
        RoundedRect::new(Rect::new(x, y, w, h), Corners::all(r))
    }

    #[test]
    fn sharp_rect_coverage_is_binary_inside_and_out() {
        let s = RoundedRect::sharp(Rect::new(10.0, 10.0, 20.0, 20.0));
        assert_eq!(s.coverage(20.0, 20.0), 1.0);
        assert_eq!(s.coverage(5.0, 20.0), 0.0);
        assert_eq!(s.coverage(35.0, 20.0), 0.0);
    }

    #[test]
    fn rounded_corners_are_cut_away() {
        let s = shape(0.0, 0.0, 40.0, 40.0, 10.0);
        assert_eq!(s.coverage(20.0, 20.0), 1.0, "the middle is solid");
        assert_eq!(s.coverage(0.5, 0.5), 0.0, "the corner is outside");
        assert!(
            s.coverage(20.0, 0.5) > 0.9,
            "the top edge midpoint is inside"
        );
    }

    #[test]
    fn coverage_is_antialiased_at_the_edge() {
        let s = RoundedRect::sharp(Rect::new(10.0, 0.0, 20.0, 20.0));
        // A pixel centre exactly on the edge is half covered.
        let coverage = s.coverage(10.0, 10.0);
        assert!(
            (coverage - 0.5).abs() < 0.01,
            "expected ~0.5, got {coverage}"
        );
    }

    #[test]
    fn radii_are_scaled_down_to_fit() {
        let s = shape(0.0, 0.0, 20.0, 20.0, 100.0);
        assert!(s.radii.top_left <= 10.001, "got {}", s.radii.top_left);
        // A fully rounded square is a circle: its corners are empty.
        assert_eq!(s.coverage(0.5, 0.5), 0.0);
        assert_eq!(s.coverage(10.0, 10.0), 1.0);
    }

    #[test]
    fn inset_shrinks_radii_too() {
        let s = shape(0.0, 0.0, 100.0, 100.0, 20.0);
        let inner = s.inset(Sides::all(10.0));
        assert_eq!(inner.rect, Rect::new(10.0, 10.0, 80.0, 80.0));
        assert!((inner.radii.top_left - 10.0).abs() < 0.01);
    }

    #[test]
    fn fill_writes_only_inside_the_shape() {
        let mut canvas = Canvas::new(20, 20);
        canvas.fill(
            RoundedRect::sharp(Rect::new(5.0, 5.0, 10.0, 10.0)),
            Color::rgb(255, 0, 0),
            &Clip::unbounded(),
        );
        assert_eq!(canvas.pixel(10, 10), Color::rgb(255, 0, 0));
        assert_eq!(canvas.pixel(1, 1), Color::TRANSPARENT);
    }

    #[test]
    fn fill_composites_with_alpha() {
        let mut canvas = Canvas::filled(4, 4, Color::BLACK);
        canvas.fill(
            RoundedRect::sharp(canvas.bounds()),
            Color::rgba(255, 255, 255, 128),
            &Clip::unbounded(),
        );
        let pixel = canvas.pixel(2, 2);
        assert_eq!(pixel.a, 255);
        assert!((120..=136).contains(&pixel.r), "got {pixel:?}");
    }

    #[test]
    fn clip_restricts_drawing() {
        let mut canvas = Canvas::new(20, 20);
        let clip = Clip::from_rect(Rect::new(0.0, 0.0, 10.0, 20.0));
        canvas.fill(
            RoundedRect::sharp(Rect::new(0.0, 0.0, 20.0, 20.0)),
            Color::rgb(0, 0, 255),
            &clip,
        );
        assert_eq!(canvas.pixel(5, 5), Color::rgb(0, 0, 255));
        assert_eq!(canvas.pixel(15, 5), Color::TRANSPARENT);
    }

    #[test]
    fn nested_clips_intersect() {
        let mut clip = Clip::from_rect(Rect::new(0.0, 0.0, 100.0, 100.0));
        clip.push(RoundedRect::sharp(Rect::new(50.0, 0.0, 100.0, 100.0)));
        let bounds = clip.bounds().unwrap();
        assert_eq!(bounds, Rect::new(50.0, 0.0, 50.0, 100.0));
        assert_eq!(clip.coverage(10.0, 10.0), 0.0);
        assert_eq!(clip.coverage(70.0, 10.0), 1.0);
    }

    #[test]
    fn empty_clip_draws_nothing() {
        let mut clip = Clip::from_rect(Rect::new(0.0, 0.0, 10.0, 10.0));
        clip.push(RoundedRect::sharp(Rect::new(50.0, 50.0, 10.0, 10.0)));
        assert!(clip.is_empty());

        let mut canvas = Canvas::new(20, 20);
        canvas.fill(
            RoundedRect::sharp(canvas.bounds()),
            Color::rgb(255, 0, 0),
            &clip,
        );
        assert_eq!(canvas.pixel(5, 5), Color::TRANSPARENT);
    }

    #[test]
    fn gradient_interpolates_between_stops() {
        let rect = Rect::new(0.0, 0.0, 10.0, 100.0);
        // 180deg points down: the first stop is at the top.
        let gradient =
            LinearGradient::for_angle(rect, 180.0, vec![(0.0, Color::BLACK), (1.0, Color::WHITE)]);
        let mut canvas = Canvas::new(10, 100);
        canvas.fill_gradient(RoundedRect::sharp(rect), &gradient, &Clip::unbounded());
        let top = canvas.pixel(5, 2);
        let middle = canvas.pixel(5, 50);
        let bottom = canvas.pixel(5, 97);
        assert!(top.r < middle.r, "{top:?} vs {middle:?}");
        assert!(middle.r < bottom.r, "{middle:?} vs {bottom:?}");
    }

    #[test]
    fn gradient_angle_zero_points_up() {
        let rect = Rect::new(0.0, 0.0, 10.0, 100.0);
        let gradient =
            LinearGradient::for_angle(rect, 0.0, vec![(0.0, Color::BLACK), (1.0, Color::WHITE)]);
        let mut canvas = Canvas::new(10, 100);
        canvas.fill_gradient(RoundedRect::sharp(rect), &gradient, &Clip::unbounded());
        assert!(canvas.pixel(5, 2).r > canvas.pixel(5, 97).r);
    }

    #[test]
    fn borders_paint_a_ring() {
        let mut canvas = Canvas::new(30, 30);
        canvas.stroke_border(
            RoundedRect::sharp(Rect::new(5.0, 5.0, 20.0, 20.0)),
            Sides::all(3.0),
            Sides::all(Color::rgb(0, 255, 0)),
            &Clip::unbounded(),
        );
        assert_eq!(
            canvas.pixel(6, 15),
            Color::rgb(0, 255, 0),
            "inside the ring"
        );
        assert_eq!(
            canvas.pixel(15, 15),
            Color::TRANSPARENT,
            "the middle is untouched"
        );
        assert_eq!(
            canvas.pixel(1, 15),
            Color::TRANSPARENT,
            "outside is untouched"
        );
    }

    #[test]
    fn per_side_border_colours() {
        let mut canvas = Canvas::new(40, 40);
        canvas.stroke_border(
            RoundedRect::sharp(Rect::new(0.0, 0.0, 40.0, 40.0)),
            Sides::all(4.0),
            Sides {
                top: Color::rgb(255, 0, 0),
                right: Color::rgb(0, 255, 0),
                bottom: Color::rgb(0, 0, 255),
                left: Color::rgb(255, 255, 0),
            },
            &Clip::unbounded(),
        );
        assert_eq!(canvas.pixel(20, 1), Color::rgb(255, 0, 0));
        assert_eq!(canvas.pixel(38, 20), Color::rgb(0, 255, 0));
        assert_eq!(canvas.pixel(20, 38), Color::rgb(0, 0, 255));
        assert_eq!(canvas.pixel(1, 20), Color::rgb(255, 255, 0));
    }

    #[test]
    fn glyph_masks_blend_in_their_colour() {
        let mut canvas = Canvas::new(10, 10);
        let mask = vec![255u8, 128, 0, 255];
        canvas.blit_mask(2, 2, 2, 2, &mask, Color::rgb(255, 0, 0), &Clip::unbounded());
        assert_eq!(canvas.pixel(2, 2), Color::rgb(255, 0, 0));
        assert_eq!(canvas.pixel(3, 2).a, 128);
        assert_eq!(canvas.pixel(2, 3), Color::TRANSPARENT);
    }

    #[test]
    fn images_scale_into_their_box() {
        // A 2x2 image: red, green / blue, white.
        let pixels: Vec<u8> = vec![
            255, 0, 0, 255, //
            0, 255, 0, 255, //
            0, 0, 255, 255, //
            255, 255, 255, 255,
        ];
        let mut canvas = Canvas::new(20, 20);
        canvas.draw_image(
            RoundedRect::sharp(Rect::new(0.0, 0.0, 20.0, 20.0)),
            2,
            2,
            &pixels,
            &Clip::unbounded(),
        );
        assert_eq!(canvas.pixel(4, 4), Color::rgb(255, 0, 0));
        assert_eq!(canvas.pixel(15, 4), Color::rgb(0, 255, 0));
        assert_eq!(canvas.pixel(4, 15), Color::rgb(0, 0, 255));
        assert_eq!(canvas.pixel(15, 15), Color::WHITE);
    }

    #[test]
    fn blur_spreads_alpha_outwards() {
        let width = 41;
        let height = 41;
        let mut mask = vec![0u8; width * height];
        // A solid 9x9 block in the middle.
        for y in 16..25 {
            for x in 16..25 {
                mask[y * width + x] = 255;
            }
        }
        blur_alpha(&mut mask, width, height, 4.0);
        assert!(mask[20 * width + 20] > 0, "the centre stays covered");
        assert!(mask[20 * width + 12] > 0, "coverage spreads outwards");
        assert!(
            mask[20 * width + 20] < 255,
            "the centre is softened, got {}",
            mask[20 * width + 20]
        );
    }

    #[test]
    fn blur_conserves_total_coverage() {
        let width = 61;
        let height = 61;
        let mut mask = vec![0u8; width * height];
        for y in 25..36 {
            for x in 25..36 {
                mask[y * width + x] = 255;
            }
        }
        let before: u64 = mask.iter().map(|v| *v as u64).sum();
        blur_alpha(&mut mask, width, height, 3.0);
        let after: u64 = mask.iter().map(|v| *v as u64).sum();
        let ratio = after as f64 / before as f64;
        assert!(
            (0.9..=1.1).contains(&ratio),
            "blur changed total coverage by {ratio}"
        );
    }

    #[test]
    fn box_sizes_grow_with_sigma() {
        assert_eq!(box_sizes_for_sigma(0.0), [0, 0, 0]);
        let small: usize = box_sizes_for_sigma(2.0).iter().sum();
        let large: usize = box_sizes_for_sigma(10.0).iter().sum();
        assert!(large > small);
    }

    #[test]
    fn drop_shadow_lands_outside_the_box() {
        let mut canvas = Canvas::new(60, 60);
        let shape = RoundedRect::sharp(Rect::new(20.0, 20.0, 20.0, 20.0));
        canvas.drop_shadow(
            shape,
            (0.0, 8.0),
            0.0,
            6.0,
            Color::rgba(0, 0, 0, 200),
            &Clip::unbounded(),
        );
        // Below the box there is shadow; above it there is much less.
        let below = canvas.pixel(30, 45).a;
        let above = canvas.pixel(30, 12).a;
        assert!(below > above, "below={below} above={above}");
        assert!(below > 0);
    }

    #[test]
    fn shadow_does_not_paint_under_its_own_box() {
        let mut canvas = Canvas::new(60, 60);
        let shape = RoundedRect::sharp(Rect::new(20.0, 20.0, 20.0, 20.0));
        canvas.drop_shadow(
            shape,
            (0.0, 0.0),
            0.0,
            4.0,
            Color::BLACK,
            &Clip::unbounded(),
        );
        assert_eq!(canvas.pixel(30, 30).a, 0, "the centre is fully occluded");
    }

    #[test]
    fn inner_shadow_hugs_the_inside_edge() {
        let mut canvas = Canvas::new(60, 60);
        let shape = RoundedRect::sharp(Rect::new(10.0, 10.0, 40.0, 40.0));
        canvas.inner_shadow(
            shape,
            (0.0, 0.0),
            4.0,
            3.0,
            Color::rgba(255, 255, 255, 255),
            &Clip::unbounded(),
        );
        let edge = canvas.pixel(12, 30).a;
        let middle = canvas.pixel(30, 30).a;
        let outside = canvas.pixel(2, 30).a;
        assert!(edge > middle, "edge={edge} middle={middle}");
        assert_eq!(outside, 0);
    }

    #[test]
    fn backdrop_filter_blurs_what_is_underneath() {
        let mut canvas = Canvas::filled(60, 60, Color::WHITE);
        // A hard black bar across the middle.
        canvas.fill(
            RoundedRect::sharp(Rect::new(0.0, 28.0, 60.0, 4.0)),
            Color::BLACK,
            &Clip::unbounded(),
        );
        let before_edge = canvas.pixel(30, 24);
        assert_eq!(before_edge, Color::WHITE, "sharp before filtering");

        canvas.filter_region(
            RoundedRect::sharp(Rect::new(10.0, 10.0, 40.0, 40.0)),
            Filter {
                blur: 12.0,
                ..Filter::NONE
            },
            &Clip::unbounded(),
        );
        let after_edge = canvas.pixel(30, 24);
        assert!(
            after_edge.r < 250,
            "the bar should bleed upwards, got {after_edge:?}"
        );
        // Outside the filtered shape nothing changed.
        assert_eq!(canvas.pixel(5, 24), Color::WHITE);
    }

    #[test]
    fn backdrop_filter_respects_rounded_corners() {
        // A fine checkerboard, so blurring changes every pixel it touches.
        let mut canvas = Canvas::new(60, 60);
        for y in 0..60u32 {
            for x in 0..60u32 {
                let dark = ((x / 2) + (y / 2)) % 2 == 0;
                canvas.set_pixel(x, y, if dark { Color::BLACK } else { Color::WHITE });
            }
        }
        let shape = RoundedRect::new(Rect::new(0.0, 0.0, 60.0, 60.0), Corners::all(20.0));
        // (1, 1) sits outside a 20px corner; the centre sits well inside.
        assert_eq!(shape.coverage(1.5, 1.5), 0.0);
        assert_eq!(shape.coverage(30.0, 30.0), 1.0);

        let corner_before = canvas.pixel(1, 1);
        let centre_before = canvas.pixel(30, 30);
        canvas.filter_region(
            shape,
            Filter {
                blur: 10.0,
                ..Filter::NONE
            },
            &Clip::unbounded(),
        );

        assert_eq!(
            canvas.pixel(1, 1),
            corner_before,
            "a pixel outside the rounded corner must be untouched"
        );
        assert_ne!(
            canvas.pixel(30, 30),
            centre_before,
            "a pixel inside the shape must be blurred"
        );
    }

    #[test]
    fn saturation_filter_boosts_colour() {
        let mut canvas = Canvas::filled(10, 10, Color::rgb(200, 100, 100));
        canvas.filter_region(
            RoundedRect::sharp(canvas.bounds()),
            Filter {
                saturate: 2.0,
                ..Filter::NONE
            },
            &Clip::unbounded(),
        );
        let pixel = canvas.pixel(5, 5);
        assert!(pixel.r > 200, "red should intensify, got {pixel:?}");
        assert!(pixel.g < 100, "green should recede, got {pixel:?}");
    }

    #[test]
    fn grayscale_filter_removes_colour() {
        let mut canvas = Canvas::filled(10, 10, Color::rgb(255, 0, 0));
        canvas.filter_region(
            RoundedRect::sharp(canvas.bounds()),
            Filter {
                grayscale: 1.0,
                ..Filter::NONE
            },
            &Clip::unbounded(),
        );
        let pixel = canvas.pixel(5, 5);
        assert!(
            (pixel.r as i32 - pixel.g as i32).abs() <= 1
                && (pixel.g as i32 - pixel.b as i32).abs() <= 1,
            "expected grey, got {pixel:?}"
        );
    }

    #[test]
    fn argb_conversion_packs_channels() {
        let mut canvas = Canvas::new(1, 1);
        canvas.set_pixel(0, 0, Color::rgba(0x12, 0x34, 0x56, 0x78));
        assert_eq!(canvas.to_argb32(), vec![0x78_12_34_56]);
    }

    #[test]
    fn png_encoding_round_trips_dimensions() {
        let canvas = Canvas::filled(8, 4, Color::rgb(1, 2, 3));
        let png = canvas.to_png().expect("encoding should succeed");
        assert_eq!(&png[1..4], b"PNG");
        let decoded = image::load_from_memory(&png).expect("decodable");
        assert_eq!(decoded.width(), 8);
        assert_eq!(decoded.height(), 4);
    }

    #[test]
    fn zero_sized_shapes_are_no_ops() {
        let mut canvas = Canvas::new(10, 10);
        canvas.fill(
            RoundedRect::sharp(Rect::new(5.0, 5.0, 0.0, 10.0)),
            Color::BLACK,
            &Clip::unbounded(),
        );
        assert_eq!(canvas.pixel(5, 5), Color::TRANSPARENT);
    }

    #[test]
    fn drawing_outside_the_canvas_does_not_panic() {
        let mut canvas = Canvas::new(10, 10);
        let clip = Clip::unbounded();
        canvas.fill(
            RoundedRect::sharp(Rect::new(-50.0, -50.0, 20.0, 20.0)),
            Color::BLACK,
            &clip,
        );
        canvas.fill(
            RoundedRect::sharp(Rect::new(100.0, 100.0, 20.0, 20.0)),
            Color::BLACK,
            &clip,
        );
        canvas.blit_mask(-5, -5, 2, 2, &[255, 255, 255, 255], Color::BLACK, &clip);
        canvas.filter_region(
            RoundedRect::sharp(Rect::new(-20.0, -20.0, 5.0, 5.0)),
            Filter {
                blur: 4.0,
                ..Filter::NONE
            },
            &clip,
        );
    }
}
