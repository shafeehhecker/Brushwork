//! Whole-image transformations.

use crate::geom::IRect;
use crate::raster::{Raster, RasterError};

pub fn flip_horizontal(img: &mut Raster) {
    let w = img.width();
    for y in 0..img.height() {
        let row = img.row_span_mut(y, 0, w);
        let n = w as usize;
        for x in 0..n / 2 {
            let (a, b) = (x * 4, (n - 1 - x) * 4);
            for c in 0..4 {
                row.swap(a + c, b + c);
            }
        }
    }
}

pub fn flip_vertical(img: &mut Raster) {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let stride = w * 4;
    let data = img.data_mut();
    for y in 0..h / 2 {
        let (top, bottom) = data.split_at_mut((h - 1 - y) * stride);
        top[y * stride..(y + 1) * stride].swap_with_slice(&mut bottom[..stride]);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotation {
    Cw90,
    Ccw90,
    Half,
}

pub fn rotate(img: &Raster, rot: Rotation) -> Raster {
    let (w, h) = (img.width(), img.height());
    let (nw, nh) = match rot {
        Rotation::Half => (w, h),
        _ => (h, w),
    };
    let mut out = Raster::from_rgba(nw, nh, vec![0; img.byte_len()]).expect("same pixel count");
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = match rot {
                Rotation::Cw90 => (h - 1 - y, x),
                Rotation::Ccw90 => (y, w - 1 - x),
                Rotation::Half => (w - 1 - x, h - 1 - y),
            };
            out.set(nx, ny, img.get(x, y));
        }
    }
    out
}

/// Exactly invertible whole-image transforms; history records these without pixel copies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lossless {
    FlipH,
    FlipV,
    Rotate(Rotation),
}

impl Lossless {
    pub fn inverse(self) -> Lossless {
        match self {
            Lossless::Rotate(Rotation::Cw90) => Lossless::Rotate(Rotation::Ccw90),
            Lossless::Rotate(Rotation::Ccw90) => Lossless::Rotate(Rotation::Cw90),
            other => other,
        }
    }
    pub fn apply(self, img: &mut Raster) {
        match self {
            Lossless::FlipH => flip_horizontal(img),
            Lossless::FlipV => flip_vertical(img),
            Lossless::Rotate(r) => *img = rotate(img, r),
        }
    }
}

/// Crops to `r` (clipped to the image).
pub fn crop(img: &Raster, r: IRect) -> Option<Raster> {
    img.copy_region(r)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeFilter {
    /// Nearest neighbour — keeps hard pixel edges (pixel art, screenshots).
    Nearest,
    /// Bilinear.
    Bilinear,
    /// Lanczos (sharp, best for photos when shrinking).
    Lanczos,
}

/// Resamples to `w × h`. Works in premultiplied alpha so transparent pixels do not
/// bleed dark fringes into opaque edges.
pub fn resize(img: &Raster, w: u32, h: u32, filter: ResizeFilter) -> Result<Raster, RasterError> {
    crate::raster::validate_dimensions(w as u64, h as u64)?;
    if (w, h) == (img.width(), img.height()) {
        return Ok(img.clone());
    }
    use image::imageops::FilterType;
    use image::{ImageBuffer, Rgba as IRgba};
    if filter == ResizeFilter::Nearest {
        let src: ImageBuffer<IRgba<u8>, &[u8]> =
            ImageBuffer::from_raw(img.width(), img.height(), img.data()).expect("valid buffer");
        let out = image::imageops::resize(&src, w, h, FilterType::Nearest);
        return Raster::from_rgba(w, h, out.into_raw());
    }
    // premultiply into f32
    let pre: Vec<f32> = img
        .data()
        .chunks_exact(4)
        .flat_map(|p| {
            let a = p[3] as f32 / 255.0;
            [p[0] as f32 * a / 255.0, p[1] as f32 * a / 255.0, p[2] as f32 * a / 255.0, a]
        })
        .collect();
    let src: ImageBuffer<IRgba<f32>, Vec<f32>> =
        ImageBuffer::from_raw(img.width(), img.height(), pre).expect("valid buffer");
    let ft = match filter {
        ResizeFilter::Bilinear => FilterType::Triangle,
        _ => FilterType::Lanczos3,
    };
    let out = image::imageops::resize(&src, w, h, ft);
    let data: Vec<u8> = out
        .into_raw()
        .chunks_exact(4)
        .flat_map(|p| {
            let a = p[3].clamp(0.0, 1.0);
            if a <= 0.5 / 255.0 {
                [0, 0, 0, 0]
            } else {
                let c = |v: f32| ((v / a).clamp(0.0, 1.0) * 255.0).round() as u8;
                [c(p[0]), c(p[1]), c(p[2]), (a * 255.0).round() as u8]
            }
        })
        .collect();
    Raster::from_rgba(w, h, data)
}

/// Computes a target size from a percentage, keeping at least 1×1.
pub fn scaled_size(w: u32, h: u32, percent: f32) -> (u32, u32) {
    let s = |v: u32| ((v as f64 * percent as f64 / 100.0).round().max(1.0)).min(u32::MAX as f64) as u32;
    (s(w), s(h))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgba;

    fn numbered(w: u32, h: u32) -> Raster {
        let mut r = Raster::new(w, h, Rgba::TRANSPARENT).unwrap();
        for y in 0..h {
            for x in 0..w {
                r.set(x, y, Rgba::new(x as u8, y as u8, 7, 255));
            }
        }
        r
    }

    #[test]
    fn flips_are_involutions_and_correct() {
        let orig = numbered(5, 3);
        let mut r = orig.clone();
        flip_horizontal(&mut r);
        assert_eq!(r.get(0, 1), orig.get(4, 1));
        assert_eq!(r.get(2, 2), orig.get(2, 2));
        flip_horizontal(&mut r);
        assert_eq!(r, orig);
        flip_vertical(&mut r);
        assert_eq!(r.get(3, 0), orig.get(3, 2));
        flip_vertical(&mut r);
        assert_eq!(r, orig);
    }

    #[test]
    fn rotations() {
        let orig = numbered(4, 3);
        let cw = rotate(&orig, Rotation::Cw90);
        assert_eq!((cw.width(), cw.height()), (3, 4));
        // top-left goes to top-right
        assert_eq!(cw.get(2, 0), orig.get(0, 0));
        assert_eq!(cw.get(0, 3), orig.get(3, 2));
        assert_eq!(rotate(&cw, Rotation::Ccw90), orig);
        let half = rotate(&orig, Rotation::Half);
        assert_eq!(half.get(0, 0), orig.get(3, 2));
        let mut four = orig.clone();
        for _ in 0..4 {
            four = rotate(&four, Rotation::Cw90);
        }
        assert_eq!(four, orig);
        assert_eq!(rotate(&half, Rotation::Half), orig);
    }

    #[test]
    fn crop_and_resize() {
        let orig = numbered(10, 8);
        let c = crop(&orig, IRect::new(2, 3, 6, 8)).unwrap();
        assert_eq!((c.width(), c.height()), (4, 5));
        assert_eq!(c.get(0, 0), orig.get(2, 3));
        let r = resize(&orig, 20, 16, ResizeFilter::Nearest).unwrap();
        assert_eq!(r.get(19, 15), orig.get(9, 7));
        assert_eq!(r.get(0, 0), orig.get(0, 0));
        assert!(resize(&orig, 0, 5, ResizeFilter::Bilinear).is_err());
        assert_eq!(scaled_size(200, 100, 50.0), (100, 50));
        assert_eq!(scaled_size(3, 3, 1.0), (1, 1));
    }

    #[test]
    fn resize_has_no_dark_fringe_at_transparent_edges() {
        let mut img = Raster::new(8, 8, Rgba::TRANSPARENT).unwrap();
        for y in 0..8 {
            for x in 0..4 {
                img.set(x, y, Rgba::opaque(255, 255, 255));
            }
        }
        for f in [ResizeFilter::Bilinear, ResizeFilter::Lanczos] {
            let r = resize(&img, 13, 13, f).unwrap();
            for y in 0..13 {
                for x in 0..13 {
                    let p = r.get(x, y);
                    if p.a > 8 {
                        assert!(p.r > 240, "{f:?} fringe at {x},{y}: {p:?}");
                    }
                }
            }
        }
    }
}
