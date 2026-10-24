//! Selection masks.

use crate::color::Rgba;
use crate::geom::IRect;
use crate::raster::Raster;

/// A selection shape rasterised to a per-pixel mask over its bounding rectangle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mask {
    rect: IRect,
    /// `None` means every pixel of `rect` is selected (rectangular selection).
    bits: Option<Vec<u8>>,
}

impl Mask {
    /// Rectangular selection, clipped to `bounds`. `None` if empty.
    pub fn rect(r: IRect, bounds: IRect) -> Option<Mask> {
        let r = r.intersect(&bounds);
        (!r.is_empty()).then_some(Mask { rect: r, bits: None })
    }

    /// Freeform (lasso) selection from a closed polygon in image coordinates, clipped to `bounds`.
    /// A pixel is selected when its centre is inside the polygon (even-odd rule).
    pub fn polygon(points: &[(f32, f32)], bounds: IRect) -> Option<Mask> {
        if points.len() < 3 {
            return None;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for &(x, y) in points {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        let r = IRect::covering(x0, y0, x1, y1).intersect(&bounds);
        if r.is_empty() {
            return None;
        }
        let w = r.width() as usize;
        let mut bits = vec![0u8; w * r.height() as usize];
        let mut xs = Vec::new();
        let mut any = false;
        for y in r.y0..r.y1 {
            let yy = y as f32 + 0.5;
            crate::shapes::polygon_crossings(points, yy, &mut xs);
            for pair in xs.chunks_exact(2) {
                // pixel x selected when x+0.5 in [l, r)
                let a = ((pair[0] - 0.5).ceil() as i32).max(r.x0);
                let b = ((pair[1] - 0.5).ceil() as i32).min(r.x1);
                for x in a..b {
                    bits[(y - r.y0) as usize * w + (x - r.x0) as usize] = 255;
                    any = true;
                }
            }
        }
        if !any {
            return None;
        }
        Some(Mask { rect: r, bits: Some(bits) }.trimmed())
    }

    fn trimmed(self) -> Mask {
        let Some(bits) = &self.bits else { return self };
        let w = self.rect.width() as usize;
        let mut b = IRect::EMPTY;
        for (i, &v) in bits.iter().enumerate() {
            if v != 0 {
                let x = self.rect.x0 + (i % w) as i32;
                let y = self.rect.y0 + (i / w) as i32;
                b = b.union(&IRect::new(x, y, x + 1, y + 1));
            }
        }
        if b == self.rect {
            return self;
        }
        let mut nb = vec![0u8; b.area() as usize];
        let nw = b.width() as usize;
        for y in b.y0..b.y1 {
            for x in b.x0..b.x1 {
                nb[(y - b.y0) as usize * nw + (x - b.x0) as usize] = self.get(x, y);
            }
        }
        Mask { rect: b, bits: Some(nb) }
    }

    pub fn bounds(&self) -> IRect {
        self.rect
    }
    pub fn is_rectangular(&self) -> bool {
        self.bits.is_none()
    }

    /// 255 if selected, 0 otherwise.
    #[inline]
    pub fn get(&self, x: i32, y: i32) -> u8 {
        if !self.rect.contains(x, y) {
            return 0;
        }
        match &self.bits {
            None => 255,
            Some(b) => b[(y - self.rect.y0) as usize * self.rect.width() as usize + (x - self.rect.x0) as usize],
        }
    }

    /// Moves the mask by an offset.
    pub fn translated(&self, dx: i32, dy: i32) -> Mask {
        Mask { rect: self.rect.translate(dx, dy), bits: self.bits.clone() }
    }

    /// Outline segments between selected and unselected pixels, for drawing "marching ants".
    /// Each segment is ((x0,y0),(x1,y1)) along pixel edges, horizontal runs merged.
    pub fn outline(&self) -> Vec<((i32, i32), (i32, i32))> {
        let r = self.rect;
        if self.bits.is_none() {
            return vec![
                ((r.x0, r.y0), (r.x1, r.y0)),
                ((r.x1, r.y0), (r.x1, r.y1)),
                ((r.x1, r.y1), (r.x0, r.y1)),
                ((r.x0, r.y1), (r.x0, r.y0)),
            ];
        }
        let mut segs = Vec::new();
        // horizontal edges
        for y in r.y0..=r.y1 {
            let mut start: Option<i32> = None;
            for x in r.x0..=r.x1 {
                let edge = x < r.x1 && (self.get(x, y - 1) != 0) != (self.get(x, y) != 0);
                match (edge, start) {
                    (true, None) => start = Some(x),
                    (false, Some(s)) => {
                        segs.push(((s, y), (x, y)));
                        start = None;
                    }
                    _ => {}
                }
            }
        }
        // vertical edges
        for x in r.x0..=r.x1 {
            let mut start: Option<i32> = None;
            for y in r.y0..=r.y1 {
                let edge = y < r.y1 && (self.get(x - 1, y) != 0) != (self.get(x, y) != 0);
                match (edge, start) {
                    (true, None) => start = Some(y),
                    (false, Some(s)) => {
                        segs.push(((x, s), (x, y)));
                        start = None;
                    }
                    _ => {}
                }
            }
        }
        segs
    }

    /// Outline as closed paths (vertex lists along pixel edges), for continuous drawing.
    pub fn outline_paths(&self) -> Vec<Vec<(i32, i32)>> {
        use std::collections::HashMap;
        let segs = self.outline();
        let mut at: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (i, (a, b)) in segs.iter().enumerate() {
            at.entry(*a).or_default().push(i);
            at.entry(*b).or_default().push(i);
        }
        let mut used = vec![false; segs.len()];
        let mut paths = Vec::new();
        for start in 0..segs.len() {
            if used[start] {
                continue;
            }
            used[start] = true;
            let (a, mut cur) = segs[start];
            let mut path = vec![a, cur];
            while cur != a {
                let Some(&next) = at.get(&cur).and_then(|v| v.iter().find(|&&i| !used[i])) else { break };
                used[next] = true;
                let (p, q) = segs[next];
                cur = if p == cur { q } else { p };
                path.push(cur);
            }
            paths.push(path);
        }
        paths
    }

    /// Copies selected pixels into a new raster the size of the bounds; unselected pixels are transparent.
    pub fn extract(&self, img: &Raster) -> Option<Raster> {
        let mut out = img.copy_region(self.rect)?;
        if self.bits.is_some() {
            let r = self.rect.intersect(&img.rect());
            for y in 0..out.height() {
                for x in 0..out.width() {
                    if self.get(r.x0 + x as i32, r.y0 + y as i32) == 0 {
                        out.set(x, y, Rgba::TRANSPARENT);
                    }
                }
            }
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_mask_clips_to_image() {
        let m = Mask::rect(IRect::new(-5, -5, 10, 3), IRect::new(0, 0, 8, 8)).unwrap();
        assert_eq!(m.bounds(), IRect::new(0, 0, 8, 3));
        assert_eq!(m.get(0, 0), 255);
        assert_eq!(m.get(7, 2), 255);
        assert_eq!(m.get(8, 2), 0);
        assert_eq!(m.get(0, 3), 0);
        assert!(Mask::rect(IRect::new(10, 10, 20, 20), IRect::new(0, 0, 8, 8)).is_none());
    }

    #[test]
    fn polygon_mask_boundaries() {
        // square from (2,2) to (6,6): pixel centres 2.5..5.5 inside → pixels 2..=5
        let pts = [(2.0, 2.0), (6.0, 2.0), (6.0, 6.0), (2.0, 6.0)];
        let m = Mask::polygon(&pts, IRect::new(0, 0, 10, 10)).unwrap();
        assert_eq!(m.bounds(), IRect::new(2, 2, 6, 6));
        assert_eq!(m.get(2, 2), 255);
        assert_eq!(m.get(5, 5), 255);
        assert_eq!(m.get(6, 5), 0);
        assert_eq!(m.get(1, 3), 0);
        // triangle: bottom-left half
        let tri = [(0.0, 0.0), (0.0, 10.0), (10.0, 10.0)];
        let m = Mask::polygon(&tri, IRect::new(0, 0, 10, 10)).unwrap();
        assert_eq!(m.get(0, 9), 255);
        assert_eq!(m.get(9, 0), 0);
        assert_eq!(m.get(2, 7), 255);
        assert!(Mask::polygon(&[(0.0, 0.0), (1.0, 1.0)], IRect::new(0, 0, 4, 4)).is_none());
    }

    #[test]
    fn extract_masks_out_unselected() {
        let img = Raster::new(10, 10, Rgba::BLACK).unwrap();
        let tri = [(0.0, 0.0), (0.0, 10.0), (10.0, 10.0)];
        let m = Mask::polygon(&tri, img.rect()).unwrap();
        // pixel centre inside when x < y → bounds x 0..9, y 1..10
        assert_eq!(m.bounds(), IRect::new(0, 1, 9, 10));
        let e = m.extract(&img).unwrap();
        assert_eq!((e.width(), e.height()), (9, 9));
        assert_eq!(e.get(0, 8), Rgba::BLACK); // image (0, 9)
        assert_eq!(e.get(8, 0), Rgba::TRANSPARENT); // image (8, 1)
    }

    #[test]
    fn outline_of_rect_has_four_edges() {
        let m = Mask::rect(IRect::new(1, 1, 4, 4), IRect::new(0, 0, 9, 9)).unwrap();
        assert_eq!(m.outline().len(), 4);
        let pts = [(1.0, 1.0), (4.0, 1.0), (4.0, 4.0), (1.0, 4.0)];
        let p = Mask::polygon(&pts, IRect::new(0, 0, 9, 9)).unwrap();
        assert_eq!(p.outline().len(), 4);
        let paths = p.outline_paths();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].first(), paths[0].last());
        // a triangle mask yields one closed staircase path
        let tri = [(0.0, 0.0), (0.0, 20.0), (20.0, 20.0)];
        let t = Mask::polygon(&tri, IRect::new(0, 0, 30, 30)).unwrap();
        let paths = t.outline_paths();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].first(), paths[0].last());
    }
}
