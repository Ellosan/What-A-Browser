//! Rectangles and points, in CSS pixels.

use wat_style::Sides;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const ZERO: Point = Point { x: 0.0, y: 0.0 };

    pub fn new(x: f32, y: f32) -> Self {
        Point { x, y }
    }

    pub fn offset(self, dx: f32, dy: f32) -> Self {
        Point {
            x: self.x + dx,
            y: self.y + dy,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size2D {
    pub width: f32,
    pub height: f32,
}

impl Size2D {
    pub const ZERO: Size2D = Size2D {
        width: 0.0,
        height: 0.0,
    };

    pub fn new(width: f32, height: f32) -> Self {
        Size2D { width, height }
    }

    pub fn is_empty(self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const ZERO: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };

    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    pub fn from_size(origin: Point, size: Size2D) -> Self {
        Rect {
            x: origin.x,
            y: origin.y,
            width: size.width,
            height: size.height,
        }
    }

    pub fn max_x(&self) -> f32 {
        self.x + self.width
    }

    pub fn max_y(&self) -> f32 {
        self.y + self.height
    }

    pub fn origin(&self) -> Point {
        Point {
            x: self.x,
            y: self.y,
        }
    }

    pub fn size(&self) -> Size2D {
        Size2D {
            width: self.width,
            height: self.height,
        }
    }

    pub fn center(&self) -> Point {
        Point {
            x: self.x + self.width / 2.0,
            y: self.y + self.height / 2.0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.x && point.x < self.max_x() && point.y >= self.y && point.y < self.max_y()
    }

    pub fn translate(&self, dx: f32, dy: f32) -> Rect {
        Rect {
            x: self.x + dx,
            y: self.y + dy,
            ..*self
        }
    }

    /// Shrinks the rectangle by `sides`, clamping to zero.
    pub fn inset(&self, sides: Sides<f32>) -> Rect {
        Rect {
            x: self.x + sides.left,
            y: self.y + sides.top,
            width: (self.width - sides.horizontal()).max(0.0),
            height: (self.height - sides.vertical()).max(0.0),
        }
    }

    /// Grows the rectangle by `sides`.
    pub fn outset(&self, sides: Sides<f32>) -> Rect {
        Rect {
            x: self.x - sides.left,
            y: self.y - sides.top,
            width: self.width + sides.horizontal(),
            height: self.height + sides.vertical(),
        }
    }

    /// Grows the rectangle by `amount` on every side.
    pub fn expand(&self, amount: f32) -> Rect {
        self.outset(Sides::all(amount))
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.max_x()
            && other.x < self.max_x()
            && self.y < other.max_y()
            && other.y < self.max_y()
    }

    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let max_x = self.max_x().min(other.max_x());
        let max_y = self.max_y().min(other.max_y());
        (max_x > x && max_y > y).then(|| Rect::new(x, y, max_x - x, max_y - y))
    }

    /// The smallest rectangle containing both.
    pub fn union(&self, other: &Rect) -> Rect {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Rect {
            x,
            y,
            width: self.max_x().max(other.max_x()) - x,
            height: self.max_y().max(other.max_y()) - y,
        }
    }

    /// Rounds outwards to whole pixels, for clipping and damage rectangles.
    pub fn round_out(&self) -> Rect {
        let x = self.x.floor();
        let y = self.y.floor();
        Rect {
            x,
            y,
            width: self.max_x().ceil() - x,
            height: self.max_y().ceil() - y,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_and_containment() {
        let rect = Rect::new(10.0, 20.0, 100.0, 50.0);
        assert_eq!(rect.max_x(), 110.0);
        assert_eq!(rect.max_y(), 70.0);
        assert!(rect.contains(Point::new(10.0, 20.0)));
        assert!(rect.contains(Point::new(109.0, 69.0)));
        assert!(!rect.contains(Point::new(110.0, 70.0)));
        assert!(!rect.contains(Point::new(9.0, 20.0)));
    }

    #[test]
    fn insetting_clamps_to_zero() {
        let rect = Rect::new(0.0, 0.0, 10.0, 10.0);
        let inset = rect.inset(Sides::all(8.0));
        assert_eq!(inset.width, 0.0);
        assert_eq!(inset.height, 0.0);
        assert_eq!(inset.x, 8.0);
    }

    #[test]
    fn inset_and_outset_round_trip() {
        let rect = Rect::new(5.0, 5.0, 100.0, 100.0);
        let sides = Sides {
            top: 1.0,
            right: 2.0,
            bottom: 3.0,
            left: 4.0,
        };
        assert_eq!(rect.inset(sides).outset(sides), rect);
    }

    #[test]
    fn intersection_and_union() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(5.0, 5.0, 10.0, 10.0);
        assert!(a.intersects(&b));
        assert_eq!(a.intersection(&b), Some(Rect::new(5.0, 5.0, 5.0, 5.0)));
        assert_eq!(a.union(&b), Rect::new(0.0, 0.0, 15.0, 15.0));

        let far = Rect::new(100.0, 100.0, 1.0, 1.0);
        assert!(!a.intersects(&far));
        assert_eq!(a.intersection(&far), None);
    }

    #[test]
    fn union_ignores_empty_rects() {
        let a = Rect::new(3.0, 3.0, 4.0, 4.0);
        assert_eq!(a.union(&Rect::ZERO), a);
        assert_eq!(Rect::ZERO.union(&a), a);
    }

    #[test]
    fn round_out_covers_fractions() {
        let rect = Rect::new(1.2, 2.7, 3.1, 4.4);
        let rounded = rect.round_out();
        assert_eq!(rounded.x, 1.0);
        assert_eq!(rounded.y, 2.0);
        assert_eq!(rounded.max_x(), 5.0);
        assert_eq!(rounded.max_y(), 8.0);
    }
}
