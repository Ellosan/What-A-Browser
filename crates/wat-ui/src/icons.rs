//! Icons, drawn from rotated rounded rectangles.
//!
//! Using the same primitive as everything else means icons inherit the
//! rasterizer's antialiasing and need no font, no SVG parser and no asset files.

use wat_css::Color;
use wat_layout::geom::Rect;
use wat_paint::{DisplayItem, DisplayList, RoundedRect};
use wat_style::{Corners, Sides};

/// The icons the chrome needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    ChevronLeft,
    ChevronRight,
    Reload,
    Home,
    Plus,
    Close,
    Menu,
    /// The padlock shown for a secure origin.
    Lock,
    /// A dot shown for an insecure origin.
    Warning,
    Search,
    /// Sun and moon, for the appearance toggle.
    Sun,
    Moon,
    /// A stack of tabs, used on mobile for the tab count.
    Tabs,
}

/// Draws `icon` centred in `bounds`.
pub fn draw(list: &mut DisplayList, icon: Icon, bounds: Rect, color: Color) {
    // Icons are designed on a square, so they stay centred in wide buttons.
    let size = bounds.width.min(bounds.height);
    if size < 4.0 {
        return;
    }
    let center = bounds.center();
    let square = Rect::new(center.x - size / 2.0, center.y - size / 2.0, size, size);
    // A stroke thickness that stays crisp at any icon size.
    let stroke = (size * 0.115).max(1.25);

    match icon {
        Icon::ChevronLeft => chevron(list, square, stroke, color, true),
        Icon::ChevronRight => chevron(list, square, stroke, color, false),
        Icon::Reload => reload(list, square, stroke, color),
        Icon::Home => home(list, square, stroke, color),
        Icon::Plus => plus(list, square, stroke, color),
        Icon::Close => close(list, square, stroke, color),
        Icon::Menu => menu(list, square, stroke, color),
        Icon::Lock => lock(list, square, stroke, color),
        Icon::Warning => warning(list, square, color),
        Icon::Search => search(list, square, stroke, color),
        Icon::Sun => sun(list, square, stroke, color),
        Icon::Moon => moon(list, square, color),
        Icon::Tabs => tabs(list, square, stroke, color),
    }
}

fn bar(list: &mut DisplayList, rect: Rect, radius: f32, rotation: f32, color: Color) {
    let shape = RoundedRect::new(rect, Corners::all(radius)).rotated(rotation);
    list.push(DisplayItem::Fill { shape, color });
}

fn ring(list: &mut DisplayList, rect: Rect, stroke: f32, color: Color) {
    let shape = RoundedRect::new(rect, Corners::all(rect.width.min(rect.height) / 2.0));
    list.push(DisplayItem::Border {
        shape,
        widths: Sides::all(stroke),
        colors: Sides::all(color),
    });
}

/// Two bars meeting at a point, like `<` or `>`.
fn chevron(list: &mut DisplayList, square: Rect, stroke: f32, color: Color, left: bool) {
    let arm = square.width * 0.36;
    let center = square.center();
    // Nudge the vertex so the arms meet visually rather than at their centres.
    let vertex_x = if left {
        center.x - arm * 0.30
    } else {
        center.x + arm * 0.30
    };
    let angle = if left { -45.0 } else { 45.0 };
    let half = arm / 2.0;

    for (rotation, dy) in [(angle, -half * 0.72), (-angle, half * 0.72)] {
        let rect = Rect::new(
            vertex_x - half * 0.72,
            center.y + dy - stroke / 2.0,
            arm,
            stroke,
        );
        bar(list, rect, stroke / 2.0, rotation, color);
    }
}

/// A circle with an arrow head, reading as "reload".
fn reload(list: &mut DisplayList, square: Rect, stroke: f32, color: Color) {
    let circle = square.inset(Sides::all(square.width * 0.18));
    ring(list, circle, stroke, color);

    // An arrow head at the top of the circle, pointing clockwise.
    let head = square.width * 0.22;
    let tip_x = circle.center().x + circle.width * 0.30;
    let tip_y = circle.y + stroke * 0.4;
    for rotation in [-50.0f32, 50.0] {
        let offset = if rotation < 0.0 {
            -head * 0.34
        } else {
            head * 0.34
        };
        bar(
            list,
            Rect::new(
                tip_x - head / 2.0,
                tip_y + offset - stroke / 2.0,
                head,
                stroke,
            ),
            stroke / 2.0,
            rotation,
            color,
        );
    }
}

/// A square with a peaked roof.
fn home(list: &mut DisplayList, square: Rect, stroke: f32, color: Color) {
    let stroke = stroke * 0.85;
    let width = square.width * 0.66;
    let body = Rect::new(
        square.center().x - width / 2.0,
        square.center().y - width * 0.10,
        width,
        width * 0.72,
    );
    let shape = RoundedRect::new(body, Corners::all(stroke));
    list.push(DisplayItem::Border {
        shape,
        widths: Sides::all(stroke),
        colors: Sides::all(color),
    });
    // Roof: two bars meeting above the body.
    let arm = width * 0.62;
    let apex_y = body.y - width * 0.20;
    for rotation in [-45.0f32, 45.0] {
        let offset = if rotation < 0.0 {
            -arm * 0.36
        } else {
            arm * 0.36
        };
        bar(
            list,
            Rect::new(
                square.center().x + offset - arm / 2.0,
                apex_y - stroke / 2.0,
                arm,
                stroke,
            ),
            stroke / 2.0,
            rotation,
            color,
        );
    }
}

fn plus(list: &mut DisplayList, square: Rect, stroke: f32, color: Color) {
    let length = square.width * 0.5;
    let center = square.center();
    bar(
        list,
        Rect::new(
            center.x - length / 2.0,
            center.y - stroke / 2.0,
            length,
            stroke,
        ),
        stroke / 2.0,
        0.0,
        color,
    );
    bar(
        list,
        Rect::new(
            center.x - stroke / 2.0,
            center.y - length / 2.0,
            stroke,
            length,
        ),
        stroke / 2.0,
        0.0,
        color,
    );
}

fn close(list: &mut DisplayList, square: Rect, stroke: f32, color: Color) {
    let length = square.width * 0.72;
    let center = square.center();
    for rotation in [45.0f32, -45.0] {
        bar(
            list,
            Rect::new(
                center.x - length / 2.0,
                center.y - stroke / 2.0,
                length,
                stroke,
            ),
            stroke / 2.0,
            rotation,
            color,
        );
    }
}

fn menu(list: &mut DisplayList, square: Rect, stroke: f32, color: Color) {
    let width = square.width * 0.56;
    let center = square.center();
    let spacing = square.width * 0.19;
    for offset in [-spacing, 0.0, spacing] {
        bar(
            list,
            Rect::new(
                center.x - width / 2.0,
                center.y + offset - stroke / 2.0,
                width,
                stroke,
            ),
            stroke / 2.0,
            0.0,
            color,
        );
    }
}

fn lock(list: &mut DisplayList, square: Rect, stroke: f32, color: Color) {
    let width = square.width * 0.46;
    let center = square.center();
    let body = Rect::new(
        center.x - width / 2.0,
        center.y - width * 0.05,
        width,
        width * 0.62,
    );
    list.push(DisplayItem::Fill {
        shape: RoundedRect::new(body, Corners::all(stroke * 0.9)),
        color,
    });
    // Shackle: a ring whose lower half the body covers.
    let shackle = Rect::new(
        center.x - width * 0.30,
        body.y - width * 0.44,
        width * 0.60,
        width * 0.62,
    );
    ring(list, shackle, stroke * 0.8, color);
    list.push(DisplayItem::Fill {
        shape: RoundedRect::new(body, Corners::all(stroke * 0.9)),
        color,
    });
}

fn warning(list: &mut DisplayList, square: Rect, color: Color) {
    let size = square.width * 0.36;
    let center = square.center();
    let dot = Rect::new(center.x - size / 2.0, center.y - size / 2.0, size, size);
    list.push(DisplayItem::Fill {
        shape: RoundedRect::new(dot, Corners::all(size / 2.0)),
        color,
    });
}

fn search(list: &mut DisplayList, square: Rect, stroke: f32, color: Color) {
    let diameter = square.width * 0.46;
    let lens = Rect::new(
        square.x + square.width * 0.16,
        square.y + square.height * 0.16,
        diameter,
        diameter,
    );
    ring(list, lens, stroke, color);
    let handle = square.width * 0.26;
    bar(
        list,
        Rect::new(
            lens.max_x() - stroke * 0.4,
            lens.max_y() - stroke * 0.4,
            handle,
            stroke,
        ),
        stroke / 2.0,
        45.0,
        color,
    );
}

fn sun(list: &mut DisplayList, square: Rect, stroke: f32, color: Color) {
    let diameter = square.width * 0.36;
    let center = square.center();
    let disc = Rect::new(
        center.x - diameter / 2.0,
        center.y - diameter / 2.0,
        diameter,
        diameter,
    );
    list.push(DisplayItem::Fill {
        shape: RoundedRect::new(disc, Corners::all(diameter / 2.0)),
        color,
    });
    let ray = square.width * 0.17;
    let radius = square.width * 0.36;
    for step in 0..8 {
        let angle = step as f32 * 45.0;
        let radians = angle.to_radians();
        let x = center.x + radians.sin() * radius;
        let y = center.y - radians.cos() * radius;
        bar(
            list,
            Rect::new(x - ray / 2.0, y - stroke / 2.0, ray, stroke),
            stroke / 2.0,
            angle + 90.0,
            color,
        );
    }
}

/// A ring, which reads as a moon at icon sizes. A true crescent would need to
/// subtract a shape, which the painter has no operator for.
fn moon(list: &mut DisplayList, square: Rect, color: Color) {
    let diameter = square.width * 0.58;
    let center = square.center();
    let disc = Rect::new(
        center.x - diameter / 2.0,
        center.y - diameter / 2.0,
        diameter,
        diameter,
    );
    ring(list, disc, (diameter * 0.22).max(1.25), color);
}

fn tabs(list: &mut DisplayList, square: Rect, stroke: f32, color: Color) {
    let size = square.width * 0.46;
    let front = Rect::new(
        square.center().x - size * 0.62,
        square.center().y - size * 0.38,
        size,
        size,
    );
    let back = front.translate(size * 0.34, -size * 0.30);
    for rect in [back, front] {
        list.push(DisplayItem::Border {
            shape: RoundedRect::new(rect, Corners::all(stroke * 1.6)),
            widths: Sides::all(stroke * 0.9),
            colors: Sides::all(color),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &[Icon] = &[
        Icon::ChevronLeft,
        Icon::ChevronRight,
        Icon::Reload,
        Icon::Home,
        Icon::Plus,
        Icon::Close,
        Icon::Menu,
        Icon::Lock,
        Icon::Warning,
        Icon::Search,
        Icon::Sun,
        Icon::Moon,
        Icon::Tabs,
    ];

    fn bounds() -> Rect {
        Rect::new(10.0, 20.0, 24.0, 24.0)
    }

    #[test]
    fn every_icon_draws_something() {
        for icon in ALL {
            let mut list = DisplayList::new();
            draw(&mut list, *icon, bounds(), Color::BLACK);
            assert!(!list.is_empty(), "{icon:?} drew nothing");
            assert!(list.is_balanced());
        }
    }

    #[test]
    fn icons_stay_inside_their_bounds() {
        // A generous allowance for stroke ends and antialiasing.
        let allowance = 3.0;
        for icon in ALL {
            let mut list = DisplayList::new();
            draw(&mut list, *icon, bounds(), Color::BLACK);
            for item in &list.items {
                let rect = match item {
                    DisplayItem::Fill { shape, .. } => shape.bounding_rect(),
                    DisplayItem::Border { shape, .. } => shape.bounding_rect(),
                    _ => continue,
                };
                assert!(
                    rect.x >= bounds().x - allowance
                        && rect.y >= bounds().y - allowance
                        && rect.max_x() <= bounds().max_x() + allowance
                        && rect.max_y() <= bounds().max_y() + allowance,
                    "{icon:?} drew {rect:?} outside {:?}",
                    bounds()
                );
            }
        }
    }

    #[test]
    fn tiny_bounds_are_skipped_rather_than_drawn_badly() {
        let mut list = DisplayList::new();
        draw(
            &mut list,
            Icon::Menu,
            Rect::new(0.0, 0.0, 2.0, 2.0),
            Color::BLACK,
        );
        assert!(list.is_empty());
    }

    #[test]
    fn icons_centre_themselves_in_wide_bounds() {
        let mut narrow = DisplayList::new();
        draw(
            &mut narrow,
            Icon::Plus,
            Rect::new(0.0, 0.0, 20.0, 20.0),
            Color::BLACK,
        );
        let mut wide = DisplayList::new();
        draw(
            &mut wide,
            Icon::Plus,
            Rect::new(0.0, 0.0, 200.0, 20.0),
            Color::BLACK,
        );

        let center_of = |list: &DisplayList| match &list.items[0] {
            DisplayItem::Fill { shape, .. } => shape.rect.center(),
            other => panic!("expected a fill, got {other:?}"),
        };
        // The wide button centres the same glyph, so its x moves but not its size.
        assert!((center_of(&wide).x - 100.0).abs() < 1.0);
        assert!((center_of(&narrow).x - 10.0).abs() < 1.0);
        assert_eq!(center_of(&wide).y, center_of(&narrow).y);
    }

    #[test]
    fn chevrons_are_mirror_images() {
        let mut left = DisplayList::new();
        draw(&mut left, Icon::ChevronLeft, bounds(), Color::BLACK);
        let mut right = DisplayList::new();
        draw(&mut right, Icon::ChevronRight, bounds(), Color::BLACK);
        assert_eq!(left.len(), right.len());

        let rotations = |list: &DisplayList| -> Vec<f32> {
            list.items
                .iter()
                .filter_map(|item| match item {
                    DisplayItem::Fill { shape, .. } => Some(shape.rotation),
                    _ => None,
                })
                .collect()
        };
        let left_rotations = rotations(&left);
        let right_rotations = rotations(&right);
        for (a, b) in left_rotations.iter().zip(&right_rotations) {
            assert!((a + b).abs() < 1e-5, "{a} and {b} should be opposite");
        }
    }

    #[test]
    fn the_menu_icon_is_three_parallel_bars() {
        let mut list = DisplayList::new();
        draw(&mut list, Icon::Menu, bounds(), Color::BLACK);
        assert_eq!(list.len(), 3);
        let ys: Vec<f32> = list
            .items
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Fill { shape, .. } => Some(shape.rect.y),
                _ => None,
            })
            .collect();
        assert!(ys[0] < ys[1] && ys[1] < ys[2], "bars should be stacked");
    }

    #[test]
    fn icon_colour_is_used() {
        let mut list = DisplayList::new();
        let wanted = Color::rgb(1, 2, 3);
        draw(&mut list, Icon::Plus, bounds(), wanted);
        let used = list.items.iter().any(|item| match item {
            DisplayItem::Fill { color, .. } => *color == wanted,
            _ => false,
        });
        assert!(used);
    }
}
