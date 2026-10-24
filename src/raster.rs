//! The authoritative pixel buffer: straight-alpha RGBA8, row-major, tightly packed.

use crate::color::Rgba;
use crate::geom::IRect;
use std::fmt;

/// Largest permitted width or height.
pub const MAX_DIMENSION: u32 = 32_768;
/// Largest permitted pixel count (256 MP ≈ 1 GiB of RGBA).
pub const MAX_PIXELS: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RasterError {
    ZeroSize,
    TooLarge { width: u64, height: u64 },
    BufferSize { expected: usize, actual: usize },
    OutOfMemory { bytes: usize },
}

impl fmt::Display for RasterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RasterError::ZeroSize => write!(f, "image dimensions must be at least 1 × 1 pixel"),
            RasterError::TooLarge { width, height } => write!(
                f,
                "{width} × {height} is too large (limit {MAX_DIMENSION} px per side and {} megapixels)",
                MAX_PIXELS / (1024 * 1024)
            ),
            RasterError::BufferSize { expected, actual } => {
                write!(f, "pixel buffer has {actual} bytes, expected {expected}")
            }
            RasterError::OutOfMemory { bytes } => {
                write!(f, "not enough memory for a {} MiB image", bytes / (1024 * 1024))
            }
        }
    }
}
impl std::error::Error for RasterError {}

/// Checks requested dimensions and returns the byte length of the buffer.
pub fn validate_dimensions(width: u64, height: u64) -> Result<usize, RasterError> {
    if width == 0 || height == 0 {
        return Err(RasterError::ZeroSize);
    }
    if width > MAX_DIMENSION as u64 || height > MAX_DIMENSION as u64 || width * height > MAX_PIXELS {
        return Err(RasterError::TooLarge { width, height });
    }
    Ok((width * height * 4) as usize)
}

#[derive(Clone, PartialEq, Eq)]
pub struct Raster {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl fmt::Debug for Raster {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Raster({}×{})", self.width, self.height)
    }
}

impl Raster {
    /// New image filled with `fill`. Allocation failure is reported instead of aborting.
    pub fn new(width: u32, height: u32, fill: Rgba) -> Result<Raster, RasterError> {
        let len = validate_dimensions(width as u64, height as u64)?;
        let mut data = Vec::new();
        data.try_reserve_exact(len).map_err(|_| RasterError::OutOfMemory { bytes: len })?;
        if fill == Rgba::TRANSPARENT {
            data.resize(len, 0);
        } else {
            let px = fill.to_array();
            for _ in 0..(len / 4) {
                data.extend_from_slice(&px);
            }
        }
        Ok(Raster { width, height, data })
    }

    pub fn from_rgba(width: u32, height: u32, data: Vec<u8>) -> Result<Raster, RasterError> {
        let len = validate_dimensions(width as u64, height as u64)?;
        if data.len() != len {
            return Err(RasterError::BufferSize { expected: len, actual: data.len() });
        }
        Ok(Raster { width, height, data })
    }

    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn data(&self) -> &[u8] {
        &self.data
    }
    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }
    pub fn into_vec(self) -> Vec<u8> {
        self.data
    }
    pub fn byte_len(&self) -> usize {
        self.data.len()
    }
    pub fn rect(&self) -> IRect {
        IRect::new(0, 0, self.width as i32, self.height as i32)
    }

    pub fn in_bounds(&self, x: i64, y: i64) -> bool {
        x >= 0 && y >= 0 && x < self.width as i64 && y < self.height as i64
    }

    #[inline]
    fn idx(&self, x: u32, y: u32) -> usize {
        (y as usize * self.width as usize + x as usize) * 4
    }

    /// Pixel at (x, y). Panics when out of bounds; use [`Raster::get_checked`] for untrusted input.
    #[inline]
    pub fn get(&self, x: u32, y: u32) -> Rgba {
        let i = self.idx(x, y);
        Rgba::from_slice(&self.data[i..i + 4])
    }

    pub fn get_checked(&self, x: i64, y: i64) -> Option<Rgba> {
        self.in_bounds(x, y).then(|| self.get(x as u32, y as u32))
    }

    #[inline]
    pub fn set(&mut self, x: u32, y: u32, c: Rgba) {
        let i = self.idx(x, y);
        self.data[i..i + 4].copy_from_slice(&c.to_array());
    }

    /// Bytes of one row segment `[x0, x1)` of row `y` (rect must be inside the image).
    pub fn row_span(&self, y: u32, x0: u32, x1: u32) -> &[u8] {
        let a = self.idx(x0, y);
        &self.data[a..a + (x1 - x0) as usize * 4]
    }
    pub fn row_span_mut(&mut self, y: u32, x0: u32, x1: u32) -> &mut [u8] {
        let a = self.idx(x0, y);
        &mut self.data[a..a + (x1 - x0) as usize * 4]
    }

    /// Copies a region (clipped to the image). `None` if the clipped region is empty.
    pub fn copy_region(&self, r: IRect) -> Option<Raster> {
        let r = r.intersect(&self.rect());
        if r.is_empty() {
            return None;
        }
        let (w, h) = (r.width() as u32, r.height() as u32);
        let mut data = Vec::with_capacity(w as usize * h as usize * 4);
        for y in r.y0..r.y1 {
            data.extend_from_slice(self.row_span(y as u32, r.x0 as u32, r.x1 as u32));
        }
        Some(Raster { width: w, height: h, data })
    }

    /// Writes `src` with its top-left at (x, y), replacing pixels, clipped to the image.
    pub fn blit(&mut self, src: &Raster, x: i32, y: i32) -> IRect {
        let dst = IRect::from_xywh(x, y, src.width as i32, src.height as i32).intersect(&self.rect());
        if dst.is_empty() {
            return dst;
        }
        for dy in dst.y0..dst.y1 {
            let sy = (dy - y) as u32;
            let sx0 = (dst.x0 - x) as u32;
            let sx1 = (dst.x1 - x) as u32;
            let row = src.row_span(sy, sx0, sx1).to_vec();
            self.row_span_mut(dy as u32, dst.x0 as u32, dst.x1 as u32).copy_from_slice(&row);
        }
        dst
    }

    /// True if every pixel has alpha 255.
    pub fn is_opaque(&self) -> bool {
        self.data.chunks_exact(4).all(|p| p[3] == 255)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bad_dimensions() {
        assert_eq!(Raster::new(0, 10, Rgba::WHITE).unwrap_err(), RasterError::ZeroSize);
        assert!(matches!(Raster::new(MAX_DIMENSION + 1, 1, Rgba::WHITE), Err(RasterError::TooLarge { .. })));
        assert!(matches!(Raster::new(MAX_DIMENSION, MAX_DIMENSION, Rgba::WHITE), Err(RasterError::TooLarge { .. })));
        assert!(matches!(Raster::from_rgba(2, 2, vec![0; 15]), Err(RasterError::BufferSize { .. })));
    }

    #[test]
    fn fill_get_set() {
        let mut r = Raster::new(3, 2, Rgba::WHITE).unwrap();
        assert_eq!(r.byte_len(), 24);
        assert!(r.data().iter().all(|&b| b == 255));
        r.set(2, 1, Rgba::new(1, 2, 3, 4));
        assert_eq!(r.get(2, 1), Rgba::new(1, 2, 3, 4));
        assert_eq!(&r.data()[20..24], &[1, 2, 3, 4]);
        assert_eq!(r.get_checked(3, 0), None);
        assert_eq!(r.get_checked(-1, 0), None);
        assert!(!r.is_opaque());
        let t = Raster::new(4, 4, Rgba::TRANSPARENT).unwrap();
        assert!(t.data().iter().all(|&b| b == 0));
    }

    #[test]
    fn copy_and_blit_clip() {
        let mut r = Raster::new(4, 4, Rgba::BLACK).unwrap();
        r.set(3, 3, Rgba::WHITE);
        let c = r.copy_region(IRect::new(2, 2, 10, 10)).unwrap();
        assert_eq!((c.width(), c.height()), (2, 2));
        assert_eq!(c.get(1, 1), Rgba::WHITE);
        assert!(r.copy_region(IRect::new(5, 5, 9, 9)).is_none());
        let mut d = Raster::new(4, 4, Rgba::TRANSPARENT).unwrap();
        let w = d.blit(&c, -1, -1);
        assert_eq!(w, IRect::new(0, 0, 1, 1));
        assert_eq!(d.get(0, 0), Rgba::WHITE);
        assert_eq!(d.get(1, 1), Rgba::TRANSPARENT);
    }
}
