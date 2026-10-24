//! Undo/redo.
//!
//! Edits record only the 64×64 tiles they touch ([`TileTxn`]): the first time an
//! operation writes into a tile, the tile's original pixels are saved. The finished
//! transaction becomes a [`Patch`]. Undo and redo are the same operation — swapping
//! the stored tiles with the image — so each step stores one copy of the touched
//! pixels, not a before *and* after copy, and never a full-image copy for local edits.
//!
//! Operations that change image dimensions (crop, resize, rotate) store the whole
//! previous image. History is bounded by a byte budget and an entry count; the
//! oldest entries are discarded first.

use crate::geom::IRect;
use crate::raster::Raster;
use crate::transform::Lossless;
use std::collections::HashMap;

pub const TILE: u32 = 64;

fn tile_rect(img_w: u32, img_h: u32, tx: u32, ty: u32) -> IRect {
    let x0 = tx * TILE;
    let y0 = ty * TILE;
    IRect::new(x0 as i32, y0 as i32, (x0 + TILE).min(img_w) as i32, (y0 + TILE).min(img_h) as i32)
}

fn read_rect(img: &Raster, r: IRect) -> Vec<u8> {
    let mut v = Vec::with_capacity(r.area() as usize * 4);
    for y in r.y0..r.y1 {
        v.extend_from_slice(img.row_span(y as u32, r.x0 as u32, r.x1 as u32));
    }
    v
}

fn swap_rect(img: &mut Raster, r: IRect, data: &mut [u8]) {
    let row = r.width() as usize * 4;
    for (i, y) in (r.y0..r.y1).enumerate() {
        img.row_span_mut(y as u32, r.x0 as u32, r.x1 as u32).swap_with_slice(&mut data[i * row..(i + 1) * row]);
    }
}

/// Saved original tiles for an in-progress edit.
#[derive(Default)]
pub struct TileTxn {
    tiles: HashMap<(u32, u32), Vec<u8>>,
    size: (u32, u32),
}

impl TileTxn {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    /// Ensures every tile overlapping `r` has its original pixels saved.
    pub fn save(&mut self, img: &Raster, r: IRect) {
        let r = r.intersect(&img.rect());
        if r.is_empty() {
            return;
        }
        if self.tiles.is_empty() {
            self.size = (img.width(), img.height());
        }
        debug_assert_eq!(self.size, (img.width(), img.height()), "image resized mid-transaction");
        let (tx0, ty0) = (r.x0 as u32 / TILE, r.y0 as u32 / TILE);
        let (tx1, ty1) = ((r.x1 as u32 - 1) / TILE, (r.y1 as u32 - 1) / TILE);
        for ty in ty0..=ty1 {
            for tx in tx0..=tx1 {
                self.tiles
                    .entry((tx, ty))
                    .or_insert_with(|| read_rect(img, tile_rect(img.width(), img.height(), tx, ty)));
            }
        }
    }

    /// Original (pre-transaction) pixel at an in-bounds position.
    #[inline]
    pub fn original(&self, img: &Raster, x: u32, y: u32) -> crate::color::Rgba {
        match self.tiles.get(&(x / TILE, y / TILE)) {
            Some(t) => {
                let tw = (((x / TILE) * TILE + TILE).min(img.width()) - (x / TILE) * TILE) as usize;
                let i = ((y % TILE) as usize * tw + (x % TILE) as usize) * 4;
                crate::color::Rgba::from_slice(&t[i..i + 4])
            }
            None => img.get(x, y),
        }
    }

    /// Saved original pixels of a tile (row-major, tile width may be clipped at the image edge).
    pub fn tile(&self, tx: u32, ty: u32) -> Option<&[u8]> {
        self.tiles.get(&(tx, ty)).map(|v| v.as_slice())
    }

    /// Restores all saved tiles into the image (keeps the saved copies). Returns the affected area.
    pub fn restore(&self, img: &mut Raster) -> IRect {
        let mut dirty = IRect::EMPTY;
        for (&(tx, ty), data) in &self.tiles {
            let r = tile_rect(img.width(), img.height(), tx, ty);
            let row = r.width() as usize * 4;
            for (i, y) in (r.y0..r.y1).enumerate() {
                img.row_span_mut(y as u32, r.x0 as u32, r.x1 as u32).copy_from_slice(&data[i * row..(i + 1) * row]);
            }
            dirty = dirty.union(&r);
        }
        dirty
    }

    /// Bounds of all saved tiles.
    pub fn bounds(&self) -> IRect {
        let (w, h) = self.size;
        self.tiles.keys().fold(IRect::EMPTY, |acc, &(tx, ty)| acc.union(&tile_rect(w, h, tx, ty)))
    }

    /// Converts to a patch, dropping tiles whose pixels did not actually change.
    pub fn into_patch(self, img: &Raster) -> Option<Patch> {
        let (w, h) = (img.width(), img.height());
        let mut tiles: Vec<_> = self
            .tiles
            .into_iter()
            .filter(|((tx, ty), data)| read_rect(img, tile_rect(w, h, *tx, *ty)) != *data)
            .map(|((tx, ty), data)| (tx, ty, data))
            .collect();
        if tiles.is_empty() {
            return None;
        }
        tiles.sort_unstable_by_key(|t| (t.1, t.0));
        Some(Patch { size: (w, h), tiles })
    }
}

/// Swappable set of tiles (see module docs).
pub struct Patch {
    size: (u32, u32),
    tiles: Vec<(u32, u32, Vec<u8>)>,
}

impl Patch {
    pub fn bytes(&self) -> usize {
        self.tiles.iter().map(|t| t.2.len() + 32).sum()
    }
    /// Exchanges stored pixels with the image's current pixels.
    pub fn swap(&mut self, img: &mut Raster) -> IRect {
        assert_eq!(self.size, (img.width(), img.height()), "patch applied to wrong image size");
        let mut dirty = IRect::EMPTY;
        for (tx, ty, data) in &mut self.tiles {
            let r = tile_rect(img.width(), img.height(), *tx, *ty);
            swap_rect(img, r, data);
            dirty = dirty.union(&r);
        }
        dirty
    }
}

enum Change {
    Patch(Patch),
    /// The other version of the whole image (dimension-changing operations).
    Replace(Raster),
    /// An exactly invertible transform; stores no pixels. `forward` is the transform
    /// that takes the image from the "undone" state to the "done" state.
    Lossless {
        forward: Lossless,
        done: bool,
    },
}

struct Entry {
    id: u64,
    label: String,
    change: Change,
}

impl Entry {
    fn bytes(&self) -> usize {
        match &self.change {
            Change::Patch(p) => p.bytes(),
            Change::Replace(r) => r.byte_len(),
            Change::Lossless { .. } => 32,
        }
    }
}

/// What the view must refresh after a history step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dirty {
    Rect(IRect),
    /// Dimensions may have changed: rebuild everything.
    Full,
}

pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    next_id: u64,
    bytes: usize,
    pub max_bytes: usize,
    pub max_entries: usize,
}

impl Default for History {
    fn default() -> Self {
        History {
            undo: Vec::new(),
            redo: Vec::new(),
            next_id: 1,
            bytes: 0,
            max_bytes: 512 * 1024 * 1024,
            max_entries: 200,
        }
    }
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    /// Identifies the current state; used to track the "saved" state for the unsaved-changes flag.
    pub fn state_id(&self) -> u64 {
        self.undo.last().map(|e| e.id).unwrap_or(0)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|e| e.label.as_str())
    }
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|e| e.label.as_str())
    }
    pub fn len(&self) -> usize {
        self.undo.len()
    }
    pub fn is_empty(&self) -> bool {
        self.undo.is_empty()
    }
    pub fn memory_bytes(&self) -> usize {
        self.bytes
    }

    fn push(&mut self, label: &str, change: Change) {
        for e in self.redo.drain(..) {
            self.bytes -= e.bytes();
        }
        let e = Entry { id: self.next_id, label: label.to_string(), change };
        self.next_id += 1;
        self.bytes += e.bytes();
        self.undo.push(e);
        // Keep at least the newest entry even if it alone exceeds the budget.
        while self.undo.len() > 1 && (self.bytes > self.max_bytes || self.undo.len() > self.max_entries) {
            let old = self.undo.remove(0);
            self.bytes -= old.bytes();
        }
    }

    /// Records a local edit. The image must already contain the new pixels.
    pub fn record_patch(&mut self, label: &str, patch: Patch) {
        self.push(label, Change::Patch(patch));
    }

    /// Replaces the whole image, recording the previous one.
    pub fn replace_image(&mut self, label: &str, img: &mut Raster, new_img: Raster) {
        let old = std::mem::replace(img, new_img);
        self.push(label, Change::Replace(old));
    }

    /// Applies an invertible transform to the image and records it.
    pub fn apply_lossless(&mut self, label: &str, img: &mut Raster, t: Lossless) {
        t.apply(img);
        self.push(label, Change::Lossless { forward: t, done: true });
    }

    fn step(img: &mut Raster, e: &mut Entry) -> Dirty {
        match &mut e.change {
            Change::Patch(p) => Dirty::Rect(p.swap(img)),
            Change::Replace(r) => {
                std::mem::swap(img, r);
                Dirty::Full
            }
            Change::Lossless { forward, done } => {
                if *done {
                    forward.inverse().apply(img)
                } else {
                    forward.apply(img)
                }
                *done = !*done;
                Dirty::Full
            }
        }
    }

    pub fn undo(&mut self, img: &mut Raster) -> Option<Dirty> {
        let mut e = self.undo.pop()?;
        let d = Self::step(img, &mut e);
        self.redo.push(e);
        Some(d)
    }

    pub fn redo(&mut self, img: &mut Raster) -> Option<Dirty> {
        let mut e = self.redo.pop()?;
        let d = Self::step(img, &mut e);
        self.undo.push(e);
        Some(d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgba;

    fn paint(img: &mut Raster, txn: &mut TileTxn, r: IRect, c: Rgba) {
        txn.save(img, r);
        for y in r.y0..r.y1 {
            for x in r.x0..r.x1 {
                img.set(x as u32, y as u32, c);
            }
        }
    }

    #[test]
    fn undo_redo_restores_exact_pixels() {
        let mut img = Raster::new(200, 150, Rgba::WHITE).unwrap();
        let original = img.clone();
        let mut h = History::new();

        let mut txn = TileTxn::new();
        paint(&mut img, &mut txn, IRect::new(10, 10, 100, 70), Rgba::BLACK);
        let after1 = img.clone();
        h.record_patch("a", txn.into_patch(&img).unwrap());

        let mut txn = TileTxn::new();
        paint(&mut img, &mut txn, IRect::new(60, 50, 199, 149), Rgba::opaque(255, 0, 0));
        let after2 = img.clone();
        h.record_patch("b", txn.into_patch(&img).unwrap());

        h.undo(&mut img).unwrap();
        assert_eq!(img, after1);
        h.undo(&mut img).unwrap();
        assert_eq!(img, original);
        assert!(h.undo(&mut img).is_none());
        h.redo(&mut img).unwrap();
        assert_eq!(img, after1);
        h.redo(&mut img).unwrap();
        assert_eq!(img, after2);
        assert!(h.redo(&mut img).is_none());
    }

    #[test]
    fn patch_stores_only_touched_tiles() {
        let mut img = Raster::new(1920, 1080, Rgba::WHITE).unwrap();
        let mut txn = TileTxn::new();
        paint(&mut img, &mut txn, IRect::new(100, 100, 110, 110), Rgba::BLACK);
        let p = txn.into_patch(&img).unwrap();
        assert!(p.bytes() < 64 * 64 * 4 + 100, "{}", p.bytes());
    }

    #[test]
    fn unchanged_edit_produces_no_patch() {
        let mut img = Raster::new(100, 100, Rgba::WHITE).unwrap();
        let mut txn = TileTxn::new();
        paint(&mut img, &mut txn, IRect::new(0, 0, 50, 50), Rgba::WHITE);
        assert!(txn.into_patch(&img).is_none());
    }

    #[test]
    fn replace_and_state_ids() {
        let mut img = Raster::new(10, 10, Rgba::WHITE).unwrap();
        let mut h = History::new();
        assert_eq!(h.state_id(), 0);
        h.replace_image("resize", &mut img, Raster::new(20, 5, Rgba::BLACK).unwrap());
        let s1 = h.state_id();
        assert_ne!(s1, 0);
        assert_eq!(h.undo(&mut img), Some(Dirty::Full));
        assert_eq!((img.width(), img.height()), (10, 10));
        assert_eq!(h.state_id(), 0);
        h.redo(&mut img);
        assert_eq!((img.width(), img.height()), (20, 5));
        assert_eq!(h.state_id(), s1);
    }

    #[test]
    fn new_edit_clears_redo_and_budget_trims_oldest() {
        let mut img = Raster::new(64, 64, Rgba::WHITE).unwrap();
        let mut h = History::new();
        h.max_bytes = 3 * (64 * 64 * 4 + 32);
        for i in 0..6u8 {
            let mut txn = TileTxn::new();
            paint(&mut img, &mut txn, IRect::new(0, 0, 64, 64), Rgba::opaque(i, 0, 0));
            h.record_patch("p", txn.into_patch(&img).unwrap());
        }
        assert_eq!(h.len(), 3);
        assert!(h.memory_bytes() <= h.max_bytes);
        h.undo(&mut img);
        assert!(h.can_redo());
        let mut txn = TileTxn::new();
        paint(&mut img, &mut txn, IRect::new(0, 0, 1, 1), Rgba::BLACK);
        h.record_patch("q", txn.into_patch(&img).unwrap());
        assert!(!h.can_redo());
    }

    #[test]
    fn restore_reverts_preview() {
        let mut img = Raster::new(130, 70, Rgba::WHITE).unwrap();
        let orig = img.clone();
        let mut txn = TileTxn::new();
        paint(&mut img, &mut txn, IRect::new(60, 30, 130, 70), Rgba::BLACK);
        assert_eq!(txn.original(&img, 129, 69), Rgba::WHITE);
        txn.restore(&mut img);
        assert_eq!(img, orig);
    }
}
