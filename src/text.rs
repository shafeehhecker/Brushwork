//! Text layout and rasterisation with system fonts.
//!
//! Text is laid out into a coverage buffer ([`TextRaster`]). While the user edits,
//! the UI shows that buffer as an overlay; only on commit is it painted into the image.

use ab_glyph::{Font, FontVec, PxScale, ScaleFont, point};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub family: String,
    /// Em size in image pixels.
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub align: Align,
    pub antialias: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle { family: String::new(), size: 32.0, bold: false, italic: false, align: Align::Left, antialias: true }
    }
}

/// Rendered text coverage. `origin` is the offset of the buffer's top-left from the
/// text anchor (the top-left of the first line box).
#[derive(Clone, Debug, Default)]
pub struct TextRaster {
    pub width: u32,
    pub height: u32,
    pub coverage: Vec<u8>,
    pub origin: (i32, i32),
}

pub struct ResolvedFace {
    pub font: Arc<FontVec>,
    /// Bold/italic requested but not available as a real face; simulated.
    pub fake_bold: bool,
    pub fake_italic: bool,
}

pub struct FontLibrary {
    db: fontdb::Database,
    families: Vec<String>,
    cache: HashMap<(String, bool, bool), Arc<ResolvedFace>>,
    fallback: Option<Arc<FontVec>>,
}

impl FontLibrary {
    /// Scans installed fonts. This can take a moment; call it off the UI thread.
    pub fn load_system() -> FontLibrary {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        FontLibrary::from_db(db)
    }

    pub fn from_db(db: fontdb::Database) -> FontLibrary {
        let mut families: Vec<String> = db.faces().filter_map(|f| f.families.first().map(|(n, _)| n.clone())).collect();
        families.sort_by_key(|f| f.to_lowercase());
        families.dedup();
        FontLibrary { db, families, cache: HashMap::new(), fallback: None }
    }

    /// Font used when no system fonts are available or a family is missing.
    pub fn set_fallback(&mut self, bytes: Vec<u8>) {
        self.fallback = FontVec::try_from_vec(bytes).ok().map(Arc::new);
    }

    pub fn families(&self) -> &[String] {
        &self.families
    }

    /// A sensible default family for the platform.
    pub fn default_family(&self) -> String {
        for f in ["Segoe UI", "Arial", "DejaVu Sans", "Liberation Sans", "Noto Sans", "Helvetica"] {
            if self.families.iter().any(|x| x == f) {
                return f.to_string();
            }
        }
        self.families.first().cloned().unwrap_or_default()
    }

    pub fn resolve(&mut self, family: &str, bold: bool, italic: bool) -> Option<Arc<ResolvedFace>> {
        let key = (family.to_string(), bold, italic);
        if let Some(f) = self.cache.get(&key) {
            return Some(f.clone());
        }
        let query = fontdb::Query {
            families: &[fontdb::Family::Name(family), fontdb::Family::SansSerif],
            weight: if bold { fontdb::Weight::BOLD } else { fontdb::Weight::NORMAL },
            stretch: fontdb::Stretch::Normal,
            style: if italic { fontdb::Style::Italic } else { fontdb::Style::Normal },
        };
        let resolved = self.db.query(&query).and_then(|id| {
            let info = self.db.face(id)?;
            let got_bold = info.weight.0 >= 600;
            let got_italic = info.style != fontdb::Style::Normal;
            let font = self
                .db
                .with_face_data(id, |data, index| FontVec::try_from_vec_and_index(data.to_vec(), index).ok())??;
            Some(ResolvedFace {
                font: Arc::new(font),
                fake_bold: bold && !got_bold,
                fake_italic: italic && !got_italic,
            })
        });
        let resolved = resolved.or_else(|| {
            self.fallback.clone().map(|font| ResolvedFace { font, fake_bold: bold, fake_italic: italic })
        })?;
        let r = Arc::new(resolved);
        // Bound the cache: font files can be several MB each.
        if self.cache.len() > 24 {
            self.cache.clear();
        }
        self.cache.insert(key, r.clone());
        Some(r)
    }
}

/// Lays out and rasterises multi-line text.
pub fn render(face: &ResolvedFace, text: &str, style: &TextStyle) -> TextRaster {
    let font = &*face.font;
    let upe = font.units_per_em().unwrap_or(1000.0);
    let size = style.size.clamp(1.0, 2000.0);
    let scale = PxScale::from(size * font.height_unscaled() / upe);
    let sf = font.as_scaled(scale);
    let line_h = (sf.ascent() - sf.descent() + sf.line_gap()).ceil().max(1.0);
    let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();

    let line_width = |line: &str| -> f32 {
        let mut w = 0.0;
        let mut prev = None;
        for ch in line.chars() {
            let id = font.glyph_id(ch);
            if let Some(p) = prev {
                w += sf.kern(p, id);
            }
            w += sf.h_advance(id);
            prev = Some(id);
        }
        w
    };
    let widths: Vec<f32> = lines.iter().map(|l| line_width(l)).collect();
    let max_w = widths.iter().copied().fold(0.0f32, f32::max);

    let slant = if face.fake_italic { 0.2 } else { 0.0 };
    let embolden = if face.fake_bold { (size / 24.0).ceil().max(1.0) as i32 } else { 0 };
    let pad = 2 + embolden + (slant * sf.ascent()).ceil() as i32;
    let width = (max_w.ceil() as i32 + 2 * pad).max(1) as u32;
    let height = ((line_h * lines.len() as f32).ceil() as i32 + 2 * pad).max(1) as u32;
    if width as u64 * height as u64 > crate::raster::MAX_PIXELS {
        return TextRaster::default();
    }
    let mut cov = vec![0u8; width as usize * height as usize];

    for (li, line) in lines.iter().enumerate() {
        let x0 = match style.align {
            Align::Left => 0.0,
            Align::Center => (max_w - widths[li]) / 2.0,
            Align::Right => max_w - widths[li],
        } + pad as f32;
        let baseline = pad as f32 + li as f32 * line_h + sf.ascent();
        let mut x = x0;
        let mut prev = None;
        for ch in line.chars() {
            let id = font.glyph_id(ch);
            if let Some(p) = prev {
                x += sf.kern(p, id);
            }
            let g = id.with_scale_and_position(scale, point(x.round(), baseline.round()));
            x += sf.h_advance(id);
            prev = Some(id);
            let Some(og) = font.outline_glyph(g) else { continue };
            let b = og.px_bounds();
            og.draw(|gx, gy, c| {
                let py = b.min.y as i32 + gy as i32;
                let shear = (slant * (baseline - py as f32)).round() as i32;
                let px = b.min.x as i32 + gx as i32 + shear;
                let c = if style.antialias {
                    (c * 255.0).round() as u8
                } else if c >= 0.5 {
                    255
                } else {
                    0
                };
                for dx in 0..=embolden {
                    let xx = px + dx;
                    if xx >= 0 && py >= 0 && (xx as u32) < width && (py as u32) < height {
                        let i = py as usize * width as usize + xx as usize;
                        cov[i] = cov[i].max(c);
                    }
                }
            });
        }
    }
    TextRaster { width, height, coverage: cov, origin: (-pad, -pad) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> Option<FontLibrary> {
        let lib = FontLibrary::load_system();
        (!lib.families().is_empty()).then_some(lib)
    }

    #[test]
    fn renders_multiline_and_aligns() {
        let Some(mut lib) = library() else {
            eprintln!("no system fonts; skipping");
            return;
        };
        let fam = lib.default_family();
        let face = lib.resolve(&fam, false, false).unwrap();
        let mut st = TextStyle { family: fam, size: 24.0, ..Default::default() };
        let one = render(&face, "Hello", &st);
        let two = render(&face, "Hello\nWorld!!", &st);
        assert!(one.coverage.iter().any(|&c| c > 200));
        assert!(two.height > one.height * 3 / 2);
        assert!(two.width >= one.width);
        // right alignment moves the short line's ink to the right
        st.align = Align::Right;
        let r = render(&face, "i\nwwwwww", &st);
        st.align = Align::Left;
        let l = render(&face, "i\nwwwwww", &st);
        let first_ink_x = |t: &TextRaster| {
            let rows = t.height as usize / 2;
            (0..t.width as usize).find(|&x| (0..rows).any(|y| t.coverage[y * t.width as usize + x] > 0))
        };
        assert!(first_ink_x(&r) > first_ink_x(&l));
        // bigger size = bigger raster
        st.size = 48.0;
        assert!(render(&face, "Hello", &st).height > one.height);
        assert_eq!(render(&face, "", &st).coverage.iter().filter(|&&c| c > 0).count(), 0);
    }

    #[test]
    fn bold_request_produces_more_ink() {
        let Some(mut lib) = library() else { return };
        let fam = lib.default_family();
        let st = TextStyle { family: fam.clone(), size: 30.0, ..Default::default() };
        let regular = render(&lib.resolve(&fam, false, false).unwrap(), "Bold test", &st);
        let bold = render(&lib.resolve(&fam, true, false).unwrap(), "Bold test", &st);
        let ink = |t: &TextRaster| t.coverage.iter().map(|&c| c as u64).sum::<u64>();
        assert!(ink(&bold) > ink(&regular));
    }
}
