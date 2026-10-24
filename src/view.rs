//! Mapping between screen coordinates and image coordinates.
//!
//! Screen positions are in egui *points* (logical pixels). `zoom` is expressed in
//! physical screen pixels per image pixel, so 100% means one image pixel per
//! physical display pixel regardless of the display scale factor.

pub const ZOOM_LEVELS: [f32; 25] = [
    0.02, 0.03, 0.05, 0.0625, 0.08, 0.10, 0.125, 0.167, 0.25, 0.333, 0.5, 0.667, 1.0, 1.5, 2.0, 3.0, 4.0, 5.0, 6.0,
    8.0, 12.0, 16.0, 24.0, 32.0, 64.0,
];
pub const MIN_ZOOM: f32 = 0.02;
pub const MAX_ZOOM: f32 = 64.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// Physical pixels per image pixel.
    pub zoom: f32,
    /// Position of the image origin relative to the canvas area's top-left, in points.
    pub offset: (f32, f32),
}

impl Default for View {
    fn default() -> Self {
        View { zoom: 1.0, offset: (0.0, 0.0) }
    }
}

impl View {
    /// Points per image pixel at the given display scale.
    pub fn scale(&self, pixels_per_point: f32) -> f32 {
        self.zoom / pixels_per_point
    }

    /// Canvas-relative point → continuous image coordinate.
    pub fn to_image(&self, p: (f32, f32), ppp: f32) -> (f32, f32) {
        let s = self.scale(ppp);
        ((p.0 - self.offset.0) / s, (p.1 - self.offset.1) / s)
    }

    /// Continuous image coordinate → canvas-relative point.
    pub fn to_screen(&self, p: (f32, f32), ppp: f32) -> (f32, f32) {
        let s = self.scale(ppp);
        (p.0 * s + self.offset.0, p.1 * s + self.offset.1)
    }

    /// Changes zoom while keeping the image point under `anchor` (canvas-relative) fixed.
    pub fn zoom_about(&mut self, new_zoom: f32, anchor: (f32, f32), ppp: f32) {
        let new_zoom = new_zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        let img = self.to_image(anchor, ppp);
        self.zoom = new_zoom;
        let s = self.scale(ppp);
        self.offset = (anchor.0 - img.0 * s, anchor.1 - img.1 * s);
    }

    pub fn next_zoom_in(&self) -> f32 {
        ZOOM_LEVELS.iter().copied().find(|&z| z > self.zoom * 1.001).unwrap_or(MAX_ZOOM)
    }
    pub fn next_zoom_out(&self) -> f32 {
        ZOOM_LEVELS.iter().rev().copied().find(|&z| z < self.zoom * 0.999).unwrap_or(MIN_ZOOM)
    }

    /// Fits the image inside the canvas (with a margin) and centres it.
    /// `allow_enlarge = false` caps the zoom at 100%.
    pub fn fit(img: (u32, u32), canvas: (f32, f32), ppp: f32, allow_enlarge: bool) -> View {
        let margin = 16.0;
        let (cw, ch) = ((canvas.0 - 2.0 * margin).max(1.0), (canvas.1 - 2.0 * margin).max(1.0));
        let mut zoom = (cw * ppp / img.0 as f32).min(ch * ppp / img.1 as f32);
        if !allow_enlarge {
            zoom = zoom.min(1.0);
        }
        let zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        View::centered(zoom, img, canvas, ppp)
    }

    pub fn centered(zoom: f32, img: (u32, u32), canvas: (f32, f32), ppp: f32) -> View {
        let s = zoom / ppp;
        let off = ((canvas.0 - img.0 as f32 * s) / 2.0, (canvas.1 - img.1 as f32 * s) / 2.0);
        // snap to physical pixels for crisp rendering
        let snap = |v: f32| (v * ppp).round() / ppp;
        View { zoom, offset: (snap(off.0), snap(off.1)) }
    }

    /// Keeps at least part of the image visible after panning.
    pub fn clamp_pan(&mut self, img: (u32, u32), canvas: (f32, f32), ppp: f32) {
        let s = self.scale(ppp);
        let (iw, ih) = (img.0 as f32 * s, img.1 as f32 * s);
        let keep: f32 = 48.0;
        let cl = |o: f32, size: f32, view: f32| -> f32 {
            let (lo, hi) = (keep.min(view) - size, view - keep.min(view));
            if lo > hi { o } else { o.clamp(lo, hi) }
        };
        self.offset = (cl(self.offset.0, iw, canvas.0), cl(self.offset.1, ih, canvas.1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_at_many_zooms_and_scales() {
        for &ppp in &[1.0, 1.25, 1.5, 2.0, 3.0] {
            for &zoom in &ZOOM_LEVELS {
                let v = View { zoom, offset: (13.5, -27.25) };
                for p in [(0.0, 0.0), (100.0, 50.0), (-3.0, 999.0)] {
                    let i = v.to_image(p, ppp);
                    let back = v.to_screen(i, ppp);
                    assert!((back.0 - p.0).abs() < 1e-2 && (back.1 - p.1).abs() < 1e-2);
                }
            }
        }
    }

    #[test]
    fn hundred_percent_is_one_physical_pixel() {
        // 150% display scaling: one image pixel = 1 physical px = 1/1.5 points
        let v = View { zoom: 1.0, offset: (0.0, 0.0) };
        let i = v.to_image((150.0, 15.0), 1.5);
        assert!((i.0 - 225.0).abs() < 1e-4 && (i.1 - 22.5).abs() < 1e-4);
        // 400% zoom at 100% scaling: points 0..4 map to pixel 0
        let v = View { zoom: 4.0, offset: (10.0, 10.0) };
        assert_eq!(v.to_image((13.9, 10.0), 1.0).0.floor(), 0.0);
        assert_eq!(v.to_image((14.0, 10.0), 1.0).0.floor(), 1.0);
    }

    #[test]
    fn zoom_about_keeps_anchor_fixed() {
        let mut v = View { zoom: 1.0, offset: (5.0, 7.0) };
        let anchor = (321.0, 123.0);
        let before = v.to_image(anchor, 1.25);
        v.zoom_about(8.0, anchor, 1.25);
        let after = v.to_image(anchor, 1.25);
        assert!((before.0 - after.0).abs() < 1e-3 && (before.1 - after.1).abs() < 1e-3);
        v.zoom_about(1000.0, anchor, 1.25);
        assert_eq!(v.zoom, MAX_ZOOM);
    }

    #[test]
    fn fit_contains_and_centres() {
        let v = View::fit((4000, 2000), (800.0, 600.0), 1.0, true);
        let br = v.to_screen((4000.0, 2000.0), 1.0);
        assert!(v.offset.0 >= 15.0 && br.0 <= 785.0 && br.1 <= 600.0);
        let small = View::fit((10, 10), (800.0, 600.0), 2.0, false);
        assert_eq!(small.zoom, 1.0);
        assert!(View::fit((10, 10), (800.0, 600.0), 1.0, true).zoom > 1.0);
    }

    #[test]
    fn zoom_steps() {
        let v = View { zoom: 1.0, offset: (0.0, 0.0) };
        assert_eq!(v.next_zoom_in(), 1.5);
        assert_eq!(v.next_zoom_out(), 0.667);
        let v = View { zoom: 0.7, offset: (0.0, 0.0) };
        assert_eq!(v.next_zoom_in(), 1.0);
    }
}
