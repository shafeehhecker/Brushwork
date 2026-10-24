//! Flood fill and colour sampling.

use crate::color::Rgba;
use crate::geom::IRect;
use crate::paint::PaintOp;
use crate::raster::Raster;

/// Contiguous (4-connected) region similar to the seed pixel.
/// `tolerance` is the maximum per-channel difference (0 = exact match).
/// Returns the region bounds and a mask over the whole image (1 = in region).
pub fn flood_region(img: &Raster, sx: i64, sy: i64, tolerance: u8) -> Option<(IRect, Vec<u8>)> {
    let seed = img.get_checked(sx, sy)?;
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut mask = vec![0u8; w * h];
    let matches = |x: usize, y: usize| img.get(x as u32, y as u32).distance(seed) <= tolerance;
    let mut stack = vec![(sx as usize, sy as usize)];
    let mut b = IRect::EMPTY;
    while let Some((x, y)) = stack.pop() {
        if mask[y * w + x] != 0 || !matches(x, y) {
            continue;
        }
        // scan left and right
        let mut l = x;
        while l > 0 && mask[y * w + l - 1] == 0 && matches(l - 1, y) {
            l -= 1;
        }
        let mut r = x;
        while r + 1 < w && mask[y * w + r + 1] == 0 && matches(r + 1, y) {
            r += 1;
        }
        mask[y * w + l..=y * w + r].fill(1);
        b = b.union(&IRect::new(l as i32, y as i32, r as i32 + 1, y as i32 + 1));
        for ny in [y.wrapping_sub(1), y + 1] {
            if ny >= h {
                continue;
            }
            let mut i = l;
            while i <= r {
                if mask[ny * w + i] == 0 && matches(i, ny) {
                    stack.push((i, ny));
                    // skip the rest of this run
                    while i <= r && mask[ny * w + i] == 0 && matches(i, ny) {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
        }
    }
    Some((b, mask))
}

/// Fills the region at (x, y) into `layer` of `op`. Returns false if the seed is off-canvas.
pub fn flood_fill(op: &mut PaintOp, img: &mut Raster, layer: usize, x: i64, y: i64, tolerance: u8) -> bool {
    let Some((b, mask)) = flood_region(img, x, y, tolerance) else { return false };
    let w = img.width() as usize;
    op.add_coverage(img, layer, b, |yy, x0, buf| {
        let row = &mask[yy as usize * w + x0 as usize..][..buf.len()];
        for (c, &m) in buf.iter_mut().zip(row) {
            *c = m * 255;
        }
    });
    true
}

/// Colour at (x, y), or `None` outside the image.
pub fn sample(img: &Raster, x: i64, y: i64) -> Option<Rgba> {
    img.get_checked(x, y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::Blend;
    use crate::shapes;

    #[test]
    fn fill_stays_inside_outline() {
        let mut img = Raster::new(50, 50, Rgba::WHITE).unwrap();
        let mut o = PaintOp::new(&[Blend::Over(Rgba::BLACK)]);
        shapes::rectangle(&mut o, &mut img, 0, IRect::new(10, 10, 30, 30), Some(1));
        let red = Rgba::opaque(255, 0, 0);
        let mut f = PaintOp::new(&[Blend::Replace(red)]);
        assert!(flood_fill(&mut f, &mut img, 0, 15, 15, 0));
        assert_eq!(img.get(15, 15), red);
        assert_eq!(img.get(11, 28), red);
        assert_eq!(img.get(10, 10), Rgba::BLACK);
        assert_eq!(img.get(5, 5), Rgba::WHITE);
        assert_eq!(img.get(31, 31), Rgba::WHITE);
        assert!(!flood_fill(&mut f, &mut img, 0, -1, 5, 0));
    }

    #[test]
    fn tolerance_and_transparency() {
        let mut img = Raster::new(10, 1, Rgba::TRANSPARENT).unwrap();
        for x in 0..10 {
            img.set(x, 0, Rgba::new(x as u8 * 10, 0, 0, 255));
        }
        let (b, _) = flood_region(&img, 0, 0, 25).unwrap();
        assert_eq!(b, IRect::new(0, 0, 3, 1));
        let t = Raster::new(8, 8, Rgba::TRANSPARENT).unwrap();
        let (b, _) = flood_region(&t, 3, 3, 0).unwrap();
        assert_eq!(b, t.rect());
        assert_eq!(sample(&img, 2, 0), Some(Rgba::new(20, 0, 0, 255)));
        assert_eq!(sample(&img, 2, 1), None);
    }

    #[test]
    fn large_fill_does_not_overflow_stack() {
        let mut img = Raster::new(2000, 2000, Rgba::WHITE).unwrap();
        // checkerboard of single-pixel holes makes many runs
        for y in (0..2000).step_by(2) {
            for x in (0..2000).step_by(2) {
                img.set(x, y, Rgba::BLACK);
            }
        }
        let (b, _) = flood_region(&img, 1, 1, 0).unwrap();
        assert_eq!(b, img.rect());
    }
}
