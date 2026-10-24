//! Shape rasterisation into a [`PaintOp`] layer.
//!
//! Pixel `(x, y)` covers the square `[x, x+1) × [y, y+1)`; its centre is `(x+0.5, y+0.5)`.
//! Points passed in are in continuous image coordinates.

use crate::geom::IRect;
use crate::paint::PaintOp;
use crate::raster::Raster;

pub type Pt = (f32, f32);

fn seg_dist2(p: Pt, a: Pt, b: Pt) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    let (qx, qy) = (a.0 + t * dx - p.0, a.1 + t * dy - p.1);
    qx * qx + qy * qy
}

/// Round-capped thick line from `a` to `b` with the given diameter.
/// Anti-aliased edges if `aa`; otherwise a pixel is painted when its centre is within the radius.
pub fn capsule(op: &mut PaintOp, img: &mut Raster, layer: usize, a: Pt, b: Pt, diameter: f32, aa: bool) {
    let r = (diameter.max(1.0)) / 2.0;
    let bbox =
        IRect::covering(a.0.min(b.0) - r - 1.0, a.1.min(b.1) - r - 1.0, a.0.max(b.0) + r + 1.0, a.1.max(b.1) + r + 1.0);
    let (r_in, r_out) = (r - 0.5, r + 0.5);
    op.add_coverage(img, layer, bbox, |y, x0, buf| {
        let py = y as f32 + 0.5;
        for (i, c) in buf.iter_mut().enumerate() {
            let p = ((x0 + i as i32) as f32 + 0.5, py);
            let d2 = seg_dist2(p, a, b);
            *c = if aa {
                if d2 <= r_in.max(0.0) * r_in.max(0.0) {
                    255
                } else if d2 >= r_out * r_out {
                    0
                } else {
                    ((r_out - d2.sqrt()).clamp(0.0, 1.0) * 255.0).round() as u8
                }
            } else if d2 <= r * r || (r <= 0.5 && d2 <= 0.25) {
                255
            } else {
                0
            };
        }
    });
}

/// One-pixel aliased line (Bresenham) between pixel positions, both ends inclusive.
pub fn pixel_line(op: &mut PaintOp, img: &mut Raster, layer: usize, a: (i32, i32), b: (i32, i32)) {
    let (mut x, mut y) = a;
    let dx = (b.0 as i64 - a.0 as i64).abs();
    let dy = -(b.1 as i64 - a.1 as i64).abs();
    let sx = if a.0 < b.0 { 1 } else { -1 };
    let sy = if a.1 < b.1 { 1 } else { -1 };
    let mut err = dx + dy;
    // Guard against absurd input (e.g. coordinates far off-canvas).
    let mut budget = (dx - dy + 1).min(1 << 20);
    loop {
        op.add_coverage(img, layer, IRect::new(x, y, x + 1, y + 1), |_, _, buf| buf.fill(255));
        if (x, y) == b || budget == 0 {
            break;
        }
        budget -= 1;
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// Axis-aligned rectangle over pixel rectangle `r`. `thickness: None` fills it.
pub fn rectangle(op: &mut PaintOp, img: &mut Raster, layer: usize, r: IRect, thickness: Option<i32>) {
    let inner = match thickness {
        Some(t) => {
            let t = t.max(1);
            let i = IRect::new(r.x0 + t, r.y0 + t, r.x1 - t, r.y1 - t);
            if i.is_empty() { IRect::EMPTY } else { i }
        }
        None => IRect::EMPTY,
    };
    op.add_coverage(img, layer, r, |y, x0, buf| {
        for (i, c) in buf.iter_mut().enumerate() {
            *c = if inner.contains(x0 + i as i32, y) { 0 } else { 255 };
        }
    });
}

/// Rasterises a region described by horizontal intervals per scanline (even-odd pairs),
/// with 4× vertical supersampling and exact horizontal coverage when `aa`.
fn scan_fill(
    op: &mut PaintOp,
    img: &mut Raster,
    layer: usize,
    bbox: IRect,
    aa: bool,
    mut intervals: impl FnMut(f32, &mut Vec<f32>),
) {
    let mut xs = Vec::new();
    let mut acc: Vec<f32> = Vec::new();
    let sub = if aa { 4 } else { 1 };
    op.add_coverage(img, layer, bbox, |y, x0, buf| {
        acc.clear();
        acc.resize(buf.len(), 0.0);
        let w = buf.len() as i32;
        for s in 0..sub {
            let yy = y as f32 + (s as f32 + 0.5) / sub as f32;
            intervals(yy, &mut xs);
            for pair in xs.chunks_exact(2) {
                let (l, r) = (pair[0] - x0 as f32, pair[1] - x0 as f32);
                if aa {
                    let a = (l.floor() as i32).max(0);
                    let b = (r.ceil() as i32).min(w);
                    for i in a..b {
                        let ov = (r.min(i as f32 + 1.0) - l.max(i as f32)).max(0.0);
                        acc[i as usize] += ov / sub as f32;
                    }
                } else {
                    let a = ((l - 0.5).ceil() as i32).max(0);
                    let b = ((r - 0.5).ceil() as i32).min(w);
                    for i in a..b {
                        acc[i as usize] = 1.0;
                    }
                }
            }
        }
        for (c, v) in buf.iter_mut().zip(&acc) {
            *c = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    });
}

/// Ellipse inscribed in pixel rectangle `r`. `thickness: None` fills it.
pub fn ellipse(op: &mut PaintOp, img: &mut Raster, layer: usize, r: IRect, thickness: Option<f32>, aa: bool) {
    if r.is_empty() {
        return;
    }
    let (cx, cy) = ((r.x0 + r.x1) as f32 / 2.0, (r.y0 + r.y1) as f32 / 2.0);
    let (rx, ry) = (r.width() as f32 / 2.0, r.height() as f32 / 2.0);
    let inner = thickness.map(|t| (rx - t.max(1.0), ry - t.max(1.0))).filter(|&(a, b)| a > 0.0 && b > 0.0);
    let half = |ra: f32, rb: f32, yy: f32| -> Option<f32> {
        let d = (yy - cy) / rb;
        (d.abs() < 1.0).then(|| ra * (1.0 - d * d).sqrt())
    };
    scan_fill(op, img, layer, r, aa, |yy, xs| {
        xs.clear();
        if let Some(h) = half(rx, ry, yy) {
            match inner.and_then(|(ia, ib)| half(ia, ib, yy)) {
                Some(hi) => xs.extend_from_slice(&[cx - h, cx - hi, cx + hi, cx + h]),
                None => xs.extend_from_slice(&[cx - h, cx + h]),
            }
        }
    });
}

/// Sorted x crossings of a closed polygon with the horizontal line `y` (even-odd pairs).
pub fn polygon_crossings(points: &[Pt], y: f32, xs: &mut Vec<f32>) {
    xs.clear();
    let n = points.len();
    for i in 0..n {
        let (a, b) = (points[i], points[(i + 1) % n]);
        if (a.1 <= y) != (b.1 <= y) {
            xs.push(a.0 + (y - a.1) / (b.1 - a.1) * (b.0 - a.0));
        }
    }
    xs.sort_by(|a, b| a.total_cmp(b));
}

/// Filled closed polygon (even-odd rule).
pub fn polygon_fill(op: &mut PaintOp, img: &mut Raster, layer: usize, points: &[Pt], aa: bool) {
    if points.len() < 3 {
        return;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for &(x, y) in points {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    scan_fill(op, img, layer, IRect::covering(x0, y0, x1, y1), aa, |yy, xs| polygon_crossings(points, yy, xs));
}

/// Polyline through `points` (closed if `closed`).
pub fn polyline(op: &mut PaintOp, img: &mut Raster, layer: usize, points: &[Pt], closed: bool, width: f32, aa: bool) {
    if points.len() == 1 {
        capsule(op, img, layer, points[0], points[0], width, aa);
    }
    for w in points.windows(2) {
        capsule(op, img, layer, w[0], w[1], width, aa);
    }
    if closed && points.len() > 2 {
        capsule(op, img, layer, points[points.len() - 1], points[0], width, aa);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgba;
    use crate::paint::Blend;

    fn op() -> PaintOp {
        PaintOp::new(&[Blend::Over(Rgba::BLACK)])
    }
    fn count_black(img: &Raster) -> usize {
        img.data().chunks_exact(4).filter(|p| p[0] == 0 && p[3] == 255).count()
    }

    #[test]
    fn pixel_line_is_one_pixel_wide_and_inclusive() {
        let mut img = Raster::new(20, 20, Rgba::WHITE).unwrap();
        let mut o = op();
        pixel_line(&mut o, &mut img, 0, (2, 3), (12, 8));
        assert_eq!(count_black(&img), 11);
        assert_eq!(img.get(2, 3), Rgba::BLACK);
        assert_eq!(img.get(12, 8), Rgba::BLACK);
    }

    #[test]
    fn rect_outline_and_fill() {
        let mut img = Raster::new(20, 20, Rgba::WHITE).unwrap();
        let mut o = op();
        rectangle(&mut o, &mut img, 0, IRect::new(2, 2, 12, 8), Some(1));
        assert_eq!(count_black(&img), 2 * 10 + 2 * 4);
        assert_eq!(img.get(5, 5), Rgba::WHITE);
        let mut img = Raster::new(20, 20, Rgba::WHITE).unwrap();
        let mut o = op();
        rectangle(&mut o, &mut img, 0, IRect::new(2, 2, 12, 8), None);
        assert_eq!(count_black(&img), 60);
    }

    #[test]
    fn filled_ellipse_area_and_symmetry() {
        let mut img = Raster::new(120, 120, Rgba::WHITE).unwrap();
        let mut o = op();
        ellipse(&mut o, &mut img, 0, IRect::new(10, 10, 110, 110), None, false);
        let n = count_black(&img) as f32;
        let expected = std::f32::consts::PI * 50.0 * 50.0;
        assert!((n - expected).abs() / expected < 0.01, "{n} vs {expected}");
        for (x, y) in [(10, 60), (109, 60), (60, 10), (60, 109)] {
            assert_eq!(img.get(x, y), Rgba::BLACK, "{x},{y}");
        }
        assert_eq!(img.get(10, 10), Rgba::WHITE);
        // horizontal mirror symmetry
        for y in 10..110 {
            for x in 10..60 {
                assert_eq!(img.get(x, y), img.get(119 - x, y));
            }
        }
    }

    #[test]
    fn ellipse_outline_has_hole() {
        let mut img = Raster::new(60, 60, Rgba::WHITE).unwrap();
        let mut o = op();
        ellipse(&mut o, &mut img, 0, IRect::new(0, 0, 60, 60), Some(3.0), true);
        assert_eq!(img.get(30, 30), Rgba::WHITE);
        assert_eq!(img.get(1, 30), Rgba::BLACK);
    }

    #[test]
    fn polygon_fill_triangle_area() {
        let mut img = Raster::new(100, 100, Rgba::WHITE).unwrap();
        let mut o = op();
        polygon_fill(&mut o, &mut img, 0, &[(0.0, 0.0), (100.0, 0.0), (0.0, 100.0)], false);
        let n = count_black(&img) as i32;
        assert!((n - 5000).abs() < 120, "{n}");
    }

    #[test]
    fn capsule_width() {
        let mut img = Raster::new(40, 40, Rgba::WHITE).unwrap();
        let mut o = op();
        capsule(&mut o, &mut img, 0, (5.0, 20.0), (35.0, 20.0), 6.0, false);
        // column 20 should be covered for rows 17..=22 (centres within radius 3)
        let col: Vec<_> = (0..40).filter(|&y| img.get(20, y) == Rgba::BLACK).collect();
        assert_eq!(col, (17..23).collect::<Vec<_>>());
        // size-1 non-AA capsule produces a dot
        let mut img = Raster::new(5, 5, Rgba::WHITE).unwrap();
        let mut o = op();
        capsule(&mut o, &mut img, 0, (2.5, 2.5), (2.5, 2.5), 1.0, false);
        assert_eq!(count_black(&img), 1);
    }
}
