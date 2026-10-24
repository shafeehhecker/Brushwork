//! Integer rectangles in image space.

/// Half-open integer rectangle `[x0, x1) × [y0, y1)` in image pixel coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct IRect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl IRect {
    pub const EMPTY: IRect = IRect { x0: 0, y0: 0, x1: 0, y1: 0 };

    pub const fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        IRect { x0, y0, x1, y1 }
    }

    pub fn from_xywh(x: i32, y: i32, w: i32, h: i32) -> Self {
        IRect::new(x, y, x.saturating_add(w), y.saturating_add(h))
    }

    /// Rectangle spanning two pixel positions, both inclusive, in any order.
    pub fn from_corners(ax: i32, ay: i32, bx: i32, by: i32) -> Self {
        IRect::new(ax.min(bx), ay.min(by), ax.max(bx).saturating_add(1), ay.max(by).saturating_add(1))
    }

    /// Smallest integer rectangle covering the float box `[x0,x1]×[y0,y1]`.
    pub fn covering(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        let c = |v: f32| v.clamp(-1.0e9, 1.0e9);
        IRect::new(
            c(x0.min(x1)).floor() as i32,
            c(y0.min(y1)).floor() as i32,
            c(x0.max(x1)).ceil() as i32,
            c(y0.max(y1)).ceil() as i32,
        )
    }

    pub fn width(&self) -> i32 {
        (self.x1 - self.x0).max(0)
    }
    pub fn height(&self) -> i32 {
        (self.y1 - self.y0).max(0)
    }
    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }
    pub fn area(&self) -> u64 {
        self.width() as u64 * self.height() as u64
    }

    pub fn intersect(&self, o: &IRect) -> IRect {
        let r = IRect::new(self.x0.max(o.x0), self.y0.max(o.y0), self.x1.min(o.x1), self.y1.min(o.y1));
        if r.is_empty() { IRect::EMPTY } else { r }
    }

    pub fn union(&self, o: &IRect) -> IRect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        IRect::new(self.x0.min(o.x0), self.y0.min(o.y0), self.x1.max(o.x1), self.y1.max(o.y1))
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }

    pub fn expand(&self, by: i32) -> IRect {
        IRect::new(
            self.x0.saturating_sub(by),
            self.y0.saturating_sub(by),
            self.x1.saturating_add(by),
            self.y1.saturating_add(by),
        )
    }

    pub fn translate(&self, dx: i32, dy: i32) -> IRect {
        IRect::new(
            self.x0.saturating_add(dx),
            self.y0.saturating_add(dy),
            self.x1.saturating_add(dx),
            self.y1.saturating_add(dy),
        )
    }
}

/// Accumulates a dirty region.
pub fn union_opt(a: Option<IRect>, b: IRect) -> Option<IRect> {
    if b.is_empty() {
        return a;
    }
    Some(match a {
        Some(a) => a.union(&b),
        None => b,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_are_inclusive_and_order_free() {
        let r = IRect::from_corners(5, 7, 2, 3);
        assert_eq!(r, IRect::new(2, 3, 6, 8));
        assert_eq!(r.width(), 4);
        assert_eq!(r.height(), 5);
    }

    #[test]
    fn intersect_and_union() {
        let a = IRect::new(0, 0, 10, 10);
        let b = IRect::new(5, 5, 20, 20);
        assert_eq!(a.intersect(&b), IRect::new(5, 5, 10, 10));
        assert_eq!(a.union(&b), IRect::new(0, 0, 20, 20));
        assert!(a.intersect(&IRect::new(10, 0, 12, 5)).is_empty());
        assert_eq!(IRect::EMPTY.union(&a), a);
    }

    #[test]
    fn covering_handles_extremes() {
        let r = IRect::covering(f32::NAN.max(0.0), 0.0, f32::INFINITY, 1e30);
        assert!(r.x1 > r.x0);
    }
}
