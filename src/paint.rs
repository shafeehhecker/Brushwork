//! Coverage-based painting.
//!
//! Every drawing tool produces *coverage* (0–255 per pixel) into one or more
//! layers of a [`PaintOp`]. Each layer has a blend (paint a colour over, erase,
//! or replace). Pixels are recomposed from the *original* pixels saved by the
//! op's [`TileTxn`], so:
//!
//! * overlapping parts of one stroke never darken (coverage takes the maximum);
//! * shape previews are redrawn by [`PaintOp::clear`] without touching history;
//! * the finished op becomes an undo patch of only the touched tiles.

use crate::color::{self, Rgba};
use crate::geom::{IRect, union_opt};
use crate::history::{Patch, TILE, TileTxn};
use crate::raster::Raster;
use crate::selection::Mask;
use std::collections::HashMap;

const TILE_PX: usize = (TILE * TILE) as usize;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Blend {
    Over(Rgba),
    Erase,
    Replace(Rgba),
}

impl Blend {
    #[inline]
    fn apply(self, dst: Rgba, cov: u8) -> Rgba {
        match self {
            Blend::Over(c) => color::over(dst, c, cov),
            Blend::Erase => color::erase(dst, cov),
            Blend::Replace(c) => color::replace(dst, c, cov),
        }
    }
}

struct Layer {
    blend: Blend,
    cov: HashMap<(u32, u32), Box<[u8; TILE_PX]>>,
}

pub struct PaintOp {
    txn: TileTxn,
    layers: Vec<Layer>,
    clip: Option<Mask>,
    dirty: Option<IRect>,
    touched: IRect,
    row: Vec<u8>,
}

impl PaintOp {
    /// One coverage layer per blend; later layers composite on top of earlier ones.
    pub fn new(blends: &[Blend]) -> Self {
        PaintOp {
            txn: TileTxn::new(),
            layers: blends.iter().map(|&blend| Layer { blend, cov: HashMap::new() }).collect(),
            clip: None,
            dirty: None,
            touched: IRect::EMPTY,
            row: Vec::new(),
        }
    }

    /// Restricts painting to a selection.
    pub fn with_clip(mut self, clip: Option<Mask>) -> Self {
        self.clip = clip;
        self
    }

    /// Region changed since the last call (for incremental display updates).
    pub fn take_dirty(&mut self) -> Option<IRect> {
        self.dirty.take()
    }

    pub fn is_empty(&self) -> bool {
        self.touched.is_empty()
    }

    /// Adds coverage to `layer` over `rect`. `fill_row(y, x0, buf)` writes coverage for
    /// pixels `x0..x0+buf.len()` of row `y` into a zeroed buffer. Coverage combines by maximum.
    pub fn add_coverage(
        &mut self,
        img: &mut Raster,
        layer: usize,
        rect: IRect,
        mut fill_row: impl FnMut(i32, i32, &mut [u8]),
    ) {
        let r = rect.intersect(&img.rect());
        if r.is_empty() || layer >= self.layers.len() {
            return;
        }
        self.txn.save(img, r);
        let w = r.width() as usize;
        let mut row = std::mem::take(&mut self.row);
        row.resize(w, 0);
        let mut any = false;
        let cov = &mut self.layers[layer].cov;
        for y in r.y0..r.y1 {
            row.fill(0);
            fill_row(y, r.x0, &mut row);
            let ty = y as u32 / TILE;
            let ly = (y as u32 % TILE) as usize;
            let mut x = r.x0;
            while x < r.x1 {
                let tx = x as u32 / TILE;
                let tile_end = ((tx + 1) * TILE).min(r.x1 as u32) as i32;
                let seg = &row[(x - r.x0) as usize..(tile_end - r.x0) as usize];
                if seg.iter().any(|&c| c != 0) {
                    any = true;
                    let t = cov.entry((tx, ty)).or_insert_with(|| Box::new([0u8; TILE_PX]));
                    let base = ly * TILE as usize + (x as u32 % TILE) as usize;
                    for (i, &c) in seg.iter().enumerate() {
                        let d = &mut t[base + i];
                        *d = (*d).max(c);
                    }
                }
                x = tile_end;
            }
        }
        self.row = row;
        if any {
            self.recompose(img, r);
        }
    }

    /// Recomputes pixels in `r` from the saved originals and the coverage layers,
    /// one tile at a time so lookups happen per tile rather than per pixel.
    fn recompose(&mut self, img: &mut Raster, r: IRect) {
        let (iw, _) = (img.width(), img.height());
        let (tx0, ty0) = (r.x0 as u32 / TILE, r.y0 as u32 / TILE);
        let (tx1, ty1) = ((r.x1 as u32 - 1) / TILE, (r.y1 as u32 - 1) / TILE);
        let mut covs: Vec<Option<(&[u8; TILE_PX], Blend)>> = Vec::with_capacity(self.layers.len());
        for ty in ty0..=ty1 {
            for tx in tx0..=tx1 {
                let tile = IRect::new(
                    (tx * TILE) as i32,
                    (ty * TILE) as i32,
                    ((tx + 1) * TILE).min(iw) as i32,
                    ((ty + 1) * TILE).min(img.height()) as i32,
                );
                let sub = tile.intersect(&r);
                if sub.is_empty() {
                    continue;
                }
                let Some(orig) = self.txn.tile(tx, ty) else { continue };
                covs.clear();
                covs.extend(self.layers.iter().map(|l| l.cov.get(&(tx, ty)).map(|c| (&**c, l.blend))));
                let tw = tile.width() as usize;
                for y in sub.y0..sub.y1 {
                    let ly = (y - tile.y0) as usize;
                    let row = img.row_span_mut(y as u32, sub.x0 as u32, sub.x1 as u32);
                    for (i, x) in (sub.x0..sub.x1).enumerate() {
                        let lx = (x - tile.x0) as usize;
                        let ci = ly * TILE as usize + lx;
                        // Coverage only grows and the image starts from the originals, so a
                        // pixel without coverage already holds its original value.
                        if covs.iter().flatten().all(|(cov, _)| cov[ci] == 0) {
                            continue;
                        }
                        let oi = (ly * tw + lx) * 4;
                        let mut c = Rgba::from_slice(&orig[oi..oi + 4]);
                        let clip = match &self.clip {
                            Some(m) => m.get(x, y),
                            None => 255,
                        };
                        for (cov, blend) in covs.iter().flatten() {
                            let mut cv = cov[ly * TILE as usize + lx];
                            if clip != 255 {
                                cv = ((cv as u32 * clip as u32 + 127) / 255) as u8;
                            }
                            if cv != 0 {
                                c = blend.apply(c, cv);
                            }
                        }
                        row[i * 4..i * 4 + 4].copy_from_slice(&c.to_array());
                    }
                }
            }
        }
        self.touched = self.touched.union(&r);
        self.dirty = union_opt(self.dirty, r);
    }

    /// Removes all coverage and restores the original pixels (used to redraw shape previews).
    pub fn clear(&mut self, img: &mut Raster) {
        let r = self.txn.restore(img);
        for l in &mut self.layers {
            l.cov.clear();
        }
        self.dirty = union_opt(self.dirty, r);
        self.touched = IRect::EMPTY;
    }

    /// Changes a layer's blend and recomposes what has been drawn so far.
    pub fn set_blend(&mut self, img: &mut Raster, layer: usize, blend: Blend) {
        if let Some(l) = self.layers.get_mut(layer) {
            l.blend = blend;
            let t = self.touched;
            if !t.is_empty() {
                self.recompose(img, t);
            }
        }
    }

    /// Ends the operation, returning the undo patch (if anything changed).
    pub fn finish(self, img: &Raster) -> Option<Patch> {
        self.txn.into_patch(img)
    }

    /// Cancels the operation, restoring the original pixels. Returns the affected area.
    pub fn cancel(self, img: &mut Raster) -> IRect {
        self.txn.restore(img)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_coverage_does_not_accumulate() {
        let mut img = Raster::new(100, 100, Rgba::WHITE).unwrap();
        let mut op = PaintOp::new(&[Blend::Over(Rgba::new(0, 0, 0, 128))]);
        let full = |_: i32, _: i32, b: &mut [u8]| b.fill(255);
        op.add_coverage(&mut img, 0, IRect::new(10, 10, 30, 30), full);
        let once = img.get(15, 15);
        op.add_coverage(&mut img, 0, IRect::new(10, 10, 30, 30), full);
        assert_eq!(img.get(15, 15), once);
        assert!(once.r > 120 && once.r < 135, "{once:?}");
    }

    #[test]
    fn clear_and_cancel_restore() {
        let mut img = Raster::new(80, 80, Rgba::WHITE).unwrap();
        let orig = img.clone();
        let mut op = PaintOp::new(&[Blend::Over(Rgba::BLACK)]);
        op.add_coverage(&mut img, 0, IRect::new(0, 0, 80, 80), |_, _, b| b.fill(255));
        assert_eq!(img.get(70, 70), Rgba::BLACK);
        op.clear(&mut img);
        assert_eq!(img, orig);
        op.add_coverage(&mut img, 0, IRect::new(5, 5, 6, 6), |_, _, b| b.fill(255));
        assert_eq!(img.get(5, 5), Rgba::BLACK);
        assert_eq!(img.get(70, 70), Rgba::WHITE);
        op.cancel(&mut img);
        assert_eq!(img, orig);
    }

    #[test]
    fn clip_mask_limits_painting() {
        let mut img = Raster::new(20, 20, Rgba::WHITE).unwrap();
        let clip = Mask::rect(IRect::new(5, 5, 10, 10), img.rect());
        let mut op = PaintOp::new(&[Blend::Over(Rgba::BLACK)]).with_clip(clip);
        let all = img.rect();
        op.add_coverage(&mut img, 0, all, |_, _, b| b.fill(255));
        assert_eq!(img.get(5, 5), Rgba::BLACK);
        assert_eq!(img.get(4, 5), Rgba::WHITE);
        assert_eq!(img.get(10, 9), Rgba::WHITE);
        let p = op.finish(&img).unwrap();
        assert!(p.bytes() > 0);
    }

    #[test]
    fn layers_composite_in_order_and_erase() {
        let mut img = Raster::new(4, 4, Rgba::WHITE).unwrap();
        let red = Rgba::opaque(255, 0, 0);
        let mut op = PaintOp::new(&[Blend::Over(red), Blend::Over(Rgba::BLACK)]);
        op.add_coverage(&mut img, 0, IRect::new(0, 0, 4, 4), |_, _, b| b.fill(255));
        op.add_coverage(&mut img, 1, IRect::new(0, 0, 2, 4), |_, _, b| b.fill(255));
        assert_eq!(img.get(0, 0), Rgba::BLACK);
        assert_eq!(img.get(3, 0), red);
        let mut er = PaintOp::new(&[Blend::Erase]);
        er.add_coverage(&mut img, 0, IRect::new(0, 0, 1, 1), |_, _, b| b.fill(255));
        assert_eq!(img.get(0, 0), Rgba::TRANSPARENT);
    }
}
