//! An open image: pixels, history, selection and file association.

use crate::color::Rgba;
use crate::geom::IRect;
use crate::history::{Dirty, History, TileTxn};
use crate::io::{FileFormat, Loaded};
use crate::paint::PaintOp;
use crate::raster::{Raster, RasterError};
use crate::selection::Mask;
use crate::transform::{self, Lossless, ResizeFilter, Rotation};
use std::path::PathBuf;

/// Pixels lifted off the canvas (moved selection) or pasted, not yet merged.
pub struct Floating {
    /// Source pixels (transparent outside a freeform selection).
    pub pixels: Raster,
    /// Destination on the canvas; differs in size from `pixels` when scaled.
    pub rect: IRect,
    /// Shape of a freeform selection relative to `pixels`, if any.
    shape: Option<Mask>,
    /// Original tiles of everything touched since the lift (the hole, then the drop).
    txn: TileTxn,
    label: &'static str,
    /// Bumped whenever `pixels` change, so the UI can refresh its texture.
    pub revision: u64,
}

impl Floating {
    pub fn is_scaled(&self) -> bool {
        self.rect.width() as u32 != self.pixels.width() || self.rect.height() as u32 != self.pixels.height()
    }

    /// Pixels at their destination size.
    pub fn resampled(&self) -> Raster {
        if !self.is_scaled() {
            return self.pixels.clone();
        }
        let (w, h) = (self.rect.width().max(1) as u32, self.rect.height().max(1) as u32);
        transform::resize(&self.pixels, w, h, ResizeFilter::Bilinear).unwrap_or_else(|_| self.pixels.clone())
    }
}

pub struct Document {
    pub image: Raster,
    pub history: History,
    pub path: Option<PathBuf>,
    pub format: Option<FileFormat>,
    /// Name shown for documents never saved.
    pub untitled_name: String,
    saved_state: u64,
    pub selection: Option<Mask>,
    pub floating: Option<Floating>,
}

impl Document {
    pub fn new_blank(width: u32, height: u32, background: Rgba, name: String) -> Result<Document, RasterError> {
        Ok(Document::with_image(Raster::new(width, height, background)?, None, None, name))
    }

    pub fn from_loaded(l: Loaded, path: PathBuf) -> Document {
        Document::with_image(l.raster, Some(path), Some(l.format), String::new())
    }

    pub fn with_image(
        image: Raster,
        path: Option<PathBuf>,
        format: Option<FileFormat>,
        untitled_name: String,
    ) -> Document {
        Document {
            image,
            history: History::new(),
            path,
            format,
            untitled_name,
            saved_state: 0,
            selection: None,
            floating: None,
        }
    }

    pub fn display_name(&self) -> String {
        match &self.path {
            Some(p) => {
                p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
            }
            None => self.untitled_name.clone(),
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.history.state_id() != self.saved_state || self.floating.is_some()
    }

    pub fn mark_saved(&mut self) {
        self.saved_state = self.history.state_id();
    }

    pub fn size(&self) -> (u32, u32) {
        (self.image.width(), self.image.height())
    }

    // ---- painting -------------------------------------------------------

    /// Records a finished paint operation.
    pub fn commit_paint(&mut self, op: PaintOp, label: &str) {
        if let Some(p) = op.finish(&self.image) {
            self.history.record_patch(label, p);
        }
    }

    /// Selection mask to clip painting to (none while a floating selection exists).
    pub fn paint_clip(&self) -> Option<Mask> {
        if self.floating.is_some() { None } else { self.selection.clone() }
    }

    // ---- history ---------------------------------------------------------

    pub fn undo(&mut self) -> Option<Dirty> {
        if let Some(f) = self.floating.take() {
            // Undoing while a selection floats puts everything back where it was.
            let r = f.txn.restore(&mut self.image);
            self.selection = None;
            return Some(Dirty::Rect(r));
        }
        let d = self.history.undo(&mut self.image)?;
        self.selection = None;
        Some(d)
    }

    pub fn redo(&mut self) -> Option<Dirty> {
        if self.floating.is_some() {
            return None;
        }
        let d = self.history.redo(&mut self.image)?;
        self.selection = None;
        Some(d)
    }

    // ---- selection -------------------------------------------------------

    pub fn set_selection(&mut self, m: Option<Mask>) -> Option<Dirty> {
        let d = self.commit_floating();
        self.selection = m;
        d
    }

    pub fn select_all(&mut self) -> Option<Dirty> {
        let r = self.image.rect();
        self.set_selection(Mask::rect(r, r))
    }

    pub fn has_selection(&self) -> bool {
        self.selection.is_some() || self.floating.is_some()
    }

    /// Bounds of the current selection or floating pixels.
    pub fn selection_bounds(&self) -> Option<IRect> {
        self.floating.as_ref().map(|f| f.rect).or_else(|| self.selection.as_ref().map(|m| m.bounds()))
    }

    /// Lifts the selected pixels into a floating selection, leaving transparency behind.
    pub fn lift_selection(&mut self) -> Option<Dirty> {
        if self.floating.is_some() {
            return None;
        }
        let mask = self.selection.take()?;
        let pixels = mask.extract(&self.image)?;
        let r = mask.bounds();
        let mut txn = TileTxn::new();
        txn.save(&self.image, r);
        clear_masked(&mut self.image, &mask);
        let shape = (!mask.is_rectangular()).then(|| mask.translated(-r.x0, -r.y0));
        self.floating = Some(Floating { pixels, rect: r, shape, txn, label: "Move selection", revision: next_rev() });
        Some(Dirty::Rect(r))
    }

    /// Places pixels as a floating selection with its top-left at (x, y).
    pub fn paste(&mut self, pixels: Raster, x: i32, y: i32) -> Option<Dirty> {
        let d = self.commit_floating();
        self.selection = None;
        let rect = IRect::from_xywh(x, y, pixels.width() as i32, pixels.height() as i32);
        self.floating =
            Some(Floating { pixels, rect, shape: None, txn: TileTxn::new(), label: "Paste", revision: next_rev() });
        d
    }

    pub fn set_floating_rect(&mut self, r: IRect) {
        if let Some(f) = &mut self.floating
            && r.width() >= 1
            && r.height() >= 1
            && r.width() <= crate::raster::MAX_DIMENSION as i32
            && r.height() <= crate::raster::MAX_DIMENSION as i32
        {
            f.rect = r;
        }
    }

    /// Merges floating pixels into the image as one undoable step.
    pub fn commit_floating(&mut self) -> Option<Dirty> {
        let mut f = self.floating.take()?;
        let px = f.resampled();
        f.txn.save(&self.image, f.rect);
        let mut dirty = f.rect.intersect(&self.image.rect());
        for y in dirty.y0..dirty.y1 {
            for x in dirty.x0..dirty.x1 {
                let s = px.get((x - f.rect.x0) as u32, (y - f.rect.y0) as u32);
                let d = self.image.get(x as u32, y as u32);
                self.image.set(x as u32, y as u32, crate::color::over(d, s, 255));
            }
        }
        dirty = dirty.union(&f.txn.bounds());
        let scaled = f.is_scaled();
        self.selection = match (&f.shape, scaled) {
            (Some(m), false) => Some(m.translated(f.rect.x0, f.rect.y0)),
            _ => Mask::rect(f.rect, self.image.rect()),
        }
        .and_then(|m| {
            let b = m.bounds().intersect(&self.image.rect());
            (!b.is_empty()).then_some(m)
        });
        if let Some(p) = f.txn.into_patch(&self.image) {
            self.history.record_patch(f.label, p);
        }
        Some(Dirty::Rect(dirty))
    }

    /// Erases the selection (or discards floating pixels) as one undoable step.
    pub fn delete_selection(&mut self) -> Option<Dirty> {
        if let Some(f) = self.floating.take() {
            let r = f.txn.bounds();
            if let Some(p) = f.txn.into_patch(&self.image) {
                self.history.record_patch("Delete selection", p);
            }
            return Some(Dirty::Rect(r));
        }
        let mask = self.selection.clone()?;
        let mut txn = TileTxn::new();
        txn.save(&self.image, mask.bounds());
        clear_masked(&mut self.image, &mask);
        if let Some(p) = txn.into_patch(&self.image) {
            self.history.record_patch("Delete selection", p);
        }
        Some(Dirty::Rect(mask.bounds()))
    }

    /// Pixels of the selection for the clipboard.
    pub fn copy_selection(&self) -> Option<Raster> {
        if let Some(f) = &self.floating {
            return Some(f.resampled());
        }
        self.selection.as_ref()?.extract(&self.image)
    }

    // ---- whole-image operations ------------------------------------------

    fn replace(&mut self, label: &str, new_img: Raster) -> Dirty {
        self.history.replace_image(label, &mut self.image, new_img);
        Dirty::Full
    }

    /// Enlarges the canvas to at least `w × h`, keeping existing pixels at the top-left.
    pub fn expand_canvas(&mut self, w: u32, h: u32, fill: Rgba) -> Result<Dirty, RasterError> {
        self.commit_floating();
        let (ow, oh) = self.size();
        let (nw, nh) = (w.max(ow), h.max(oh));
        if (nw, nh) == (ow, oh) {
            return Ok(Dirty::Rect(IRect::EMPTY));
        }
        let mut n = Raster::new(nw, nh, fill)?;
        n.blit(&self.image, 0, 0);
        Ok(self.replace("Expand canvas", n))
    }

    pub fn crop_to_selection(&mut self) -> Option<Dirty> {
        self.commit_floating();
        let mask = self.selection.take()?;
        let cropped = mask.extract(&self.image)?;
        Some(self.replace("Crop to selection", cropped))
    }

    pub fn resize(&mut self, w: u32, h: u32, filter: ResizeFilter) -> Result<Dirty, RasterError> {
        self.commit_floating();
        let r = transform::resize(&self.image, w, h, filter)?;
        self.selection = None;
        Ok(self.replace("Resize image", r))
    }

    /// Flips the floating selection if there is one, otherwise the whole image.
    pub fn flip(&mut self, horizontal: bool) -> Dirty {
        if let Some(f) = &mut self.floating {
            if horizontal {
                transform::flip_horizontal(&mut f.pixels)
            } else {
                transform::flip_vertical(&mut f.pixels)
            }
            f.shape = None;
            f.revision = next_rev();
            return Dirty::Rect(IRect::EMPTY);
        }
        self.selection = None;
        let (label, t) =
            if horizontal { ("Flip horizontal", Lossless::FlipH) } else { ("Flip vertical", Lossless::FlipV) };
        self.history.apply_lossless(label, &mut self.image, t);
        Dirty::Full
    }

    pub fn rotate(&mut self, rot: Rotation) -> Dirty {
        if let Some(f) = &mut self.floating {
            f.pixels = transform::rotate(&f.pixels, rot);
            f.shape = None;
            let (cx, cy) = ((f.rect.x0 + f.rect.x1) / 2, (f.rect.y0 + f.rect.y1) / 2);
            let (w, h) = match rot {
                Rotation::Half => (f.rect.width(), f.rect.height()),
                _ => (f.rect.height(), f.rect.width()),
            };
            f.rect = IRect::from_xywh(cx - w / 2, cy - h / 2, w, h);
            f.revision = next_rev();
            return Dirty::Rect(IRect::EMPTY);
        }
        self.selection = None;
        self.history.apply_lossless("Rotate", &mut self.image, Lossless::Rotate(rot));
        Dirty::Full
    }
}

fn clear_masked(img: &mut Raster, mask: &Mask) {
    let r = mask.bounds().intersect(&img.rect());
    for y in r.y0..r.y1 {
        for x in r.x0..r.x1 {
            if mask.get(x, y) != 0 {
                img.set(x as u32, y as u32, Rgba::TRANSPARENT);
            }
        }
    }
}

fn next_rev() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static REV: AtomicU64 = AtomicU64::new(1);
    REV.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::Blend;
    use crate::shapes;

    fn doc() -> Document {
        let mut d = Document::new_blank(50, 40, Rgba::WHITE, "Untitled".into()).unwrap();
        // black square at 10..20
        let mut op = PaintOp::new(&[Blend::Over(Rgba::BLACK)]);
        shapes::rectangle(&mut op, &mut d.image, 0, IRect::new(10, 10, 20, 20), None);
        d.commit_paint(op, "rect");
        d
    }

    #[test]
    fn dirty_tracking_follows_history() {
        let mut d = Document::new_blank(10, 10, Rgba::WHITE, "U".into()).unwrap();
        assert!(!d.is_dirty());
        let mut op = PaintOp::new(&[Blend::Over(Rgba::BLACK)]);
        shapes::rectangle(&mut op, &mut d.image, 0, IRect::new(0, 0, 2, 2), None);
        d.commit_paint(op, "r");
        assert!(d.is_dirty());
        d.mark_saved();
        assert!(!d.is_dirty());
        d.undo();
        assert!(d.is_dirty());
        d.redo();
        assert!(!d.is_dirty());
    }

    #[test]
    fn move_selection_and_undo() {
        let mut d = doc();
        let before = d.image.clone();
        d.set_selection(Mask::rect(IRect::new(10, 10, 20, 20), d.image.rect()));
        d.lift_selection().unwrap();
        assert_eq!(d.image.get(15, 15), Rgba::TRANSPARENT);
        d.set_floating_rect(IRect::new(30, 5, 40, 15));
        d.commit_floating().unwrap();
        assert_eq!(d.image.get(35, 10), Rgba::BLACK);
        assert_eq!(d.image.get(15, 15), Rgba::TRANSPARENT);
        assert_eq!(d.selection.as_ref().unwrap().bounds(), IRect::new(30, 5, 40, 15));
        d.undo().unwrap();
        assert_eq!(d.image, before);
    }

    #[test]
    fn undo_while_floating_cancels_lift() {
        let mut d = doc();
        let before = d.image.clone();
        let hist = d.history.len();
        d.set_selection(Mask::rect(IRect::new(5, 5, 25, 25), d.image.rect()));
        d.lift_selection();
        d.set_floating_rect(IRect::new(0, 0, 20, 20));
        d.undo();
        assert_eq!(d.image, before);
        assert_eq!(d.history.len(), hist);
        assert!(d.floating.is_none());
    }

    #[test]
    fn paste_scale_and_commit() {
        let mut d = doc();
        let px = Raster::new(2, 2, Rgba::opaque(255, 0, 0)).unwrap();
        d.paste(px, 40, 30);
        d.set_floating_rect(IRect::new(40, 30, 48, 38));
        d.commit_floating();
        assert_eq!(d.image.get(47, 37), Rgba::opaque(255, 0, 0));
        assert_eq!(d.image.get(39, 30), Rgba::WHITE);
        assert_eq!(d.history.undo_label(), Some("Paste"));
        // pasting partially off-canvas is clipped
        d.paste(Raster::new(10, 10, Rgba::BLACK).unwrap(), 45, 35);
        d.commit_floating();
        assert_eq!(d.image.get(49, 39), Rgba::BLACK);
    }

    #[test]
    fn delete_copy_crop() {
        let mut d = doc();
        d.set_selection(Mask::rect(IRect::new(8, 8, 22, 22), d.image.rect()));
        let c = d.copy_selection().unwrap();
        assert_eq!((c.width(), c.height()), (14, 14));
        assert_eq!(c.get(2, 2), Rgba::BLACK);
        assert_eq!(c.get(0, 0), Rgba::WHITE);
        d.delete_selection().unwrap();
        assert_eq!(d.image.get(15, 15), Rgba::TRANSPARENT);
        d.undo();
        assert_eq!(d.image.get(15, 15), Rgba::BLACK);
        d.set_selection(Mask::rect(IRect::new(10, 10, 20, 20), d.image.rect()));
        d.crop_to_selection().unwrap();
        assert_eq!(d.size(), (10, 10));
        assert!(d.image.data().chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
        d.undo();
        assert_eq!(d.size(), (50, 40));
    }

    #[test]
    fn lasso_move_keeps_shape() {
        let mut d = Document::new_blank(20, 20, Rgba::BLACK, "U".into()).unwrap();
        let tri = [(0.0, 0.0), (0.0, 10.0), (10.0, 10.0)];
        d.set_selection(Mask::polygon(&tri, d.image.rect()));
        d.lift_selection();
        assert_eq!(d.image.get(0, 9), Rgba::TRANSPARENT);
        assert_eq!(d.image.get(9, 0), Rgba::BLACK);
        let r = d.floating.as_ref().unwrap().rect;
        d.set_floating_rect(r.translate(10, 10));
        d.commit_floating();
        assert!(!d.selection.as_ref().unwrap().is_rectangular());
    }

    #[test]
    fn transforms_on_floating_and_image() {
        let mut d = doc();
        d.rotate(Rotation::Cw90);
        assert_eq!(d.size(), (40, 50));
        d.flip(true);
        d.undo();
        d.undo();
        assert_eq!(d.size(), (50, 40));
        d.resize(100, 80, ResizeFilter::Nearest).unwrap();
        assert_eq!(d.image.get(30, 30), Rgba::BLACK);
        assert!(d.resize(0, 1, ResizeFilter::Nearest).is_err());
        d.paste(Raster::new(4, 2, Rgba::BLACK).unwrap(), 0, 0);
        d.rotate(Rotation::Cw90);
        let f = d.floating.as_ref().unwrap();
        assert_eq!((f.pixels.width(), f.rect.width(), f.rect.height()), (2, 2, 4));
    }
}
