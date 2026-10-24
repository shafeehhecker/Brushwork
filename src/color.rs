//! Straight (non-premultiplied) 8-bit sRGB colours and compositing.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const TRANSPARENT: Rgba = Rgba::new(0, 0, 0, 0);
    pub const BLACK: Rgba = Rgba::new(0, 0, 0, 255);
    pub const WHITE: Rgba = Rgba::new(255, 255, 255, 255);

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Rgba { r, g, b, a }
    }
    pub const fn opaque(r: u8, g: u8, b: u8) -> Self {
        Rgba { r, g, b, a: 255 }
    }
    pub fn to_array(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }
    pub fn from_slice(p: &[u8]) -> Self {
        Rgba::new(p[0], p[1], p[2], p[3])
    }

    /// `#RRGGBB`, or `#RRGGBBAA` when not fully opaque.
    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, self.a)
        }
    }

    /// Parses `RGB`, `RRGGBB` or `RRGGBBAA`, with or without a leading `#`.
    pub fn from_hex(s: &str) -> Option<Rgba> {
        let s = s.trim();
        let s = s.strip_prefix('#').unwrap_or(s);
        if !s.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
        match s.len() {
            3 => {
                let n = |i: usize| u8::from_str_radix(&s[i..i + 1], 16).ok().map(|v| v * 17);
                Some(Rgba::opaque(n(0)?, n(1)?, n(2)?))
            }
            6 => Some(Rgba::opaque(byte(0)?, byte(2)?, byte(4)?)),
            8 => Some(Rgba::new(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            _ => None,
        }
    }

    /// Largest per-channel difference (alpha included). 0 = identical.
    pub fn distance(self, o: Rgba) -> u8 {
        // Fully transparent pixels are equal regardless of their hidden RGB.
        if self.a == 0 && o.a == 0 {
            return 0;
        }
        let d = |a: u8, b: u8| a.abs_diff(b);
        d(self.r, o.r).max(d(self.g, o.g)).max(d(self.b, o.b)).max(d(self.a, o.a))
    }
}

/// Source-over compositing of `src` onto `dst`, with `src` alpha scaled by `coverage` (0–255).
pub fn over(dst: Rgba, src: Rgba, coverage: u8) -> Rgba {
    if coverage == 0 || src.a == 0 {
        return dst;
    }
    let sa = src.a as u32 * coverage as u32; // 0..=65025
    if sa == 65025 {
        return src;
    }
    let sa = sa as f32 / 65025.0;
    let da = dst.a as f32 / 255.0;
    let oa = sa + da * (1.0 - sa);
    if oa <= 0.0 {
        return Rgba::TRANSPARENT;
    }
    let ch = |s: u8, d: u8| -> u8 {
        let v = (s as f32 * sa + d as f32 * da * (1.0 - sa)) / oa;
        v.round().clamp(0.0, 255.0) as u8
    };
    Rgba::new(ch(src.r, dst.r), ch(src.g, dst.g), ch(src.b, dst.b), (oa * 255.0).round() as u8)
}

/// Reduces alpha by `coverage` (eraser to transparency).
pub fn erase(dst: Rgba, coverage: u8) -> Rgba {
    let a = (dst.a as u32 * (255 - coverage as u32) + 127) / 255;
    if a == 0 { Rgba::TRANSPARENT } else { Rgba { a: a as u8, ..dst } }
}

/// Linear interpolation between `dst` and `src` by coverage (used for "replace" painting).
pub fn replace(dst: Rgba, src: Rgba, coverage: u8) -> Rgba {
    match coverage {
        0 => dst,
        255 => src,
        c => {
            let t = c as u32;
            let l = |a: u8, b: u8| ((a as u32 * (255 - t) + b as u32 * t + 127) / 255) as u8;
            Rgba::new(l(dst.r, src.r), l(dst.g, src.g), l(dst.b, src.b), l(dst.a, src.a))
        }
    }
}

/// HSV helpers for the colour picker (h in degrees 0–360, s/v in 0–1).
pub fn to_hsv(c: Rgba) -> (f32, f32, f32) {
    let r = c.r as f32 / 255.0;
    let g = c.g as f32 / 255.0;
    let b = c.b as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (h, s, max)
}

/// Default palette, in the familiar two-row desktop-paint layout.
pub const PALETTE: [Rgba; 28] = [
    Rgba::opaque(0, 0, 0),
    Rgba::opaque(127, 127, 127),
    Rgba::opaque(136, 0, 21),
    Rgba::opaque(237, 28, 36),
    Rgba::opaque(255, 127, 39),
    Rgba::opaque(255, 242, 0),
    Rgba::opaque(34, 177, 76),
    Rgba::opaque(0, 162, 232),
    Rgba::opaque(63, 72, 204),
    Rgba::opaque(163, 73, 164),
    Rgba::opaque(64, 64, 64),
    Rgba::opaque(0, 128, 128),
    Rgba::opaque(0, 64, 128),
    Rgba::new(0, 0, 0, 0),
    Rgba::opaque(255, 255, 255),
    Rgba::opaque(195, 195, 195),
    Rgba::opaque(185, 122, 87),
    Rgba::opaque(255, 174, 201),
    Rgba::opaque(255, 201, 14),
    Rgba::opaque(239, 228, 176),
    Rgba::opaque(181, 230, 29),
    Rgba::opaque(153, 217, 234),
    Rgba::opaque(112, 146, 190),
    Rgba::opaque(200, 191, 231),
    Rgba::opaque(160, 160, 160),
    Rgba::opaque(128, 255, 255),
    Rgba::opaque(128, 128, 255),
    Rgba::opaque(255, 128, 192),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        assert_eq!(Rgba::from_hex("#FF8000"), Some(Rgba::opaque(255, 128, 0)));
        assert_eq!(Rgba::from_hex("ff8000"), Some(Rgba::opaque(255, 128, 0)));
        assert_eq!(Rgba::from_hex("#f80"), Some(Rgba::opaque(255, 136, 0)));
        assert_eq!(Rgba::from_hex("#11223344"), Some(Rgba::new(0x11, 0x22, 0x33, 0x44)));
        assert_eq!(Rgba::from_hex("#12345"), None);
        assert_eq!(Rgba::from_hex("#GG0000"), None);
        assert_eq!(Rgba::from_hex("#é0000"), None);
        for c in [Rgba::opaque(1, 2, 3), Rgba::new(9, 8, 7, 6)] {
            assert_eq!(Rgba::from_hex(&c.to_hex()), Some(c));
        }
    }

    #[test]
    fn over_basics() {
        let red = Rgba::opaque(255, 0, 0);
        assert_eq!(over(Rgba::WHITE, red, 255), red);
        assert_eq!(over(Rgba::TRANSPARENT, red, 255), red);
        assert_eq!(over(Rgba::WHITE, red, 0), Rgba::WHITE);
        // 50% black over white = mid grey, opaque
        let g = over(Rgba::WHITE, Rgba::new(0, 0, 0, 255), 128);
        assert_eq!(g.a, 255);
        assert!((g.r as i32 - 127).abs() <= 1);
        // semi-transparent over transparent keeps colour, alpha = src alpha
        let s = over(Rgba::TRANSPARENT, Rgba::new(10, 20, 30, 100), 255);
        assert_eq!(s, Rgba::new(10, 20, 30, 100));
    }

    #[test]
    fn erase_and_replace() {
        assert_eq!(erase(Rgba::WHITE, 255), Rgba::TRANSPARENT);
        assert_eq!(erase(Rgba::WHITE, 0), Rgba::WHITE);
        assert_eq!(erase(Rgba::WHITE, 128).a, 127);
        assert_eq!(replace(Rgba::WHITE, Rgba::TRANSPARENT, 255), Rgba::TRANSPARENT);
    }

    #[test]
    fn distance_ignores_hidden_rgb_of_transparent() {
        assert_eq!(Rgba::new(1, 2, 3, 0).distance(Rgba::new(200, 100, 50, 0)), 0);
        assert_eq!(Rgba::BLACK.distance(Rgba::WHITE), 255);
    }
}
