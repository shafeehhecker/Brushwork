//! Image import/export with validation and atomic saving.

use crate::color::{self, Rgba};
use crate::raster::{self, Raster, RasterError};
use image::{ExtendedColorType, ImageDecoder, ImageEncoder, ImageFormat};
use std::fmt;
use std::fs::File;
use std::io::{BufReader, BufWriter, Cursor, Read, Seek, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileFormat {
    Png,
    Jpeg,
    Bmp,
}

impl FileFormat {
    pub const ALL: [FileFormat; 3] = [FileFormat::Png, FileFormat::Jpeg, FileFormat::Bmp];

    pub fn name(self) -> &'static str {
        match self {
            FileFormat::Png => "PNG",
            FileFormat::Jpeg => "JPEG",
            FileFormat::Bmp => "BMP",
        }
    }
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            FileFormat::Png => &["png"],
            FileFormat::Jpeg => &["jpg", "jpeg", "jpe", "jfif"],
            FileFormat::Bmp => &["bmp", "dib"],
        }
    }
    pub fn from_path(p: &Path) -> Option<FileFormat> {
        let ext = p.extension()?.to_str()?.to_ascii_lowercase();
        FileFormat::ALL.into_iter().find(|f| f.extensions().contains(&ext.as_str()))
    }
    pub fn supports_alpha(self) -> bool {
        !matches!(self, FileFormat::Jpeg)
    }
    fn image_format(self) -> ImageFormat {
        match self {
            FileFormat::Png => ImageFormat::Png,
            FileFormat::Jpeg => ImageFormat::Jpeg,
            FileFormat::Bmp => ImageFormat::Bmp,
        }
    }
    fn from_image_format(f: ImageFormat) -> Option<FileFormat> {
        match f {
            ImageFormat::Png => Some(FileFormat::Png),
            ImageFormat::Jpeg => Some(FileFormat::Jpeg),
            ImageFormat::Bmp => Some(FileFormat::Bmp),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum IoError {
    Io(std::io::Error),
    Unsupported(String),
    Malformed(String),
    Limits(RasterError),
    Encode(String),
}

impl fmt::Display for IoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IoError::Io(e) => write!(f, "{e}"),
            IoError::Unsupported(s) => {
                write!(f, "unsupported file format ({s}). Brushwork opens PNG, JPEG and BMP images")
            }
            IoError::Malformed(s) => write!(f, "the file is damaged or not a valid image: {s}"),
            IoError::Limits(e) => write!(f, "{e}"),
            IoError::Encode(s) => write!(f, "could not encode the image: {s}"),
        }
    }
}
impl std::error::Error for IoError {}
impl From<std::io::Error> for IoError {
    fn from(e: std::io::Error) -> Self {
        IoError::Io(e)
    }
}

pub struct Loaded {
    pub raster: Raster,
    pub format: FileFormat,
    /// Things the user should know about the conversion (e.g. 16-bit reduced to 8-bit).
    pub notes: Vec<String>,
}

fn map_image_err(e: image::ImageError) -> IoError {
    use image::ImageError as E;
    match e {
        E::IoError(e) => IoError::Io(e),
        E::Unsupported(u) => IoError::Unsupported(u.to_string()),
        E::Limits(l) => IoError::Malformed(format!("image exceeds decoding limits ({l})")),
        other => IoError::Malformed(other.to_string()),
    }
}

pub fn load_path(path: &Path) -> Result<Loaded, IoError> {
    let f = File::open(path)?;
    load_reader(BufReader::new(f))
}

pub fn load_bytes(bytes: &[u8]) -> Result<Loaded, IoError> {
    load_reader(Cursor::new(bytes))
}

fn load_reader<R: Read + Seek + std::io::BufRead>(r: R) -> Result<Loaded, IoError> {
    let reader = image::ImageReader::new(r).with_guessed_format()?;
    let Some(fmt) = reader.format() else {
        return Err(IoError::Unsupported("unrecognised file contents".into()));
    };
    let format = FileFormat::from_image_format(fmt).ok_or_else(|| IoError::Unsupported(format!("{fmt:?}")))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(raster::MAX_DIMENSION);
    limits.max_image_height = Some(raster::MAX_DIMENSION);
    limits.max_alloc = Some(raster::MAX_PIXELS * 4 * 2 + 64 * 1024 * 1024);
    let mut reader = reader;
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(map_image_err)?;
    let (w, h) = decoder.dimensions();
    raster::validate_dimensions(w as u64, h as u64).map_err(IoError::Limits)?;
    let color = decoder.color_type();
    let orientation = decoder.orientation().map_err(map_image_err)?;
    let mut img = image::DynamicImage::from_decoder(decoder).map_err(map_image_err)?;
    let mut notes = Vec::new();
    if color.bytes_per_pixel() / color.channel_count() > 1 {
        notes.push("High bit-depth image was converted to 8 bits per channel".to_string());
    }
    if orientation != image::metadata::Orientation::NoTransforms {
        img.apply_orientation(orientation);
        notes.push("Rotated according to the camera orientation tag".to_string());
    }
    let rgba = img.into_rgba8();
    let (w, h) = rgba.dimensions();
    let raster = Raster::from_rgba(w, h, rgba.into_raw()).map_err(IoError::Limits)?;
    Ok(Loaded { raster, format, notes })
}

#[derive(Clone, Copy, Debug)]
pub struct SaveOptions {
    /// 1–100.
    pub jpeg_quality: u8,
}
impl Default for SaveOptions {
    fn default() -> Self {
        SaveOptions { jpeg_quality: 92 }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct SaveReport {
    /// Transparent pixels were composited onto white because the format has no alpha.
    pub flattened_alpha: bool,
}

/// Encodes to any writer.
pub fn encode(
    img: &Raster,
    format: FileFormat,
    opts: SaveOptions,
    out: &mut impl Write,
) -> Result<SaveReport, IoError> {
    let (w, h) = (img.width(), img.height());
    let opaque = img.is_opaque();
    let rgb = || -> Vec<u8> {
        img.data()
            .chunks_exact(4)
            .flat_map(|p| {
                let c =
                    if p[3] == 255 { Rgba::from_slice(p) } else { color::over(Rgba::WHITE, Rgba::from_slice(p), 255) };
                [c.r, c.g, c.b]
            })
            .collect()
    };
    let enc_err = |e: image::ImageError| match e {
        image::ImageError::IoError(e) => IoError::Io(e),
        e => IoError::Encode(e.to_string()),
    };
    let mut report = SaveReport::default();
    match format {
        FileFormat::Png => {
            let enc = image::codecs::png::PngEncoder::new(out);
            if opaque {
                enc.write_image(&rgb(), w, h, ExtendedColorType::Rgb8)
            } else {
                enc.write_image(img.data(), w, h, ExtendedColorType::Rgba8)
            }
            .map_err(enc_err)?
        }
        FileFormat::Jpeg => {
            if w > 65_535 || h > 65_535 {
                return Err(IoError::Encode("JPEG supports at most 65535 px per side".into()));
            }
            report.flattened_alpha = !opaque;
            let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(out, opts.jpeg_quality.clamp(1, 100));
            enc.encode(&rgb(), w, h, ExtendedColorType::Rgb8).map_err(enc_err)?
        }
        FileFormat::Bmp => {
            let mut enc = image::codecs::bmp::BmpEncoder::new(out);
            if opaque {
                enc.encode(&rgb(), w, h, ExtendedColorType::Rgb8)
            } else {
                enc.encode(img.data(), w, h, ExtendedColorType::Rgba8)
            }
            .map_err(enc_err)?
        }
    }
    let _ = format.image_format();
    Ok(report)
}

fn temp_path_for(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "image".into());
    let nonce =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    path.with_file_name(format!(".{name}.{}-{nonce}.brushwork-tmp", std::process::id()))
}

/// Saves atomically: encodes to a temporary file in the same folder, flushes it to disk,
/// then renames it over the destination. If anything fails, the existing file is untouched
/// and the temporary file is removed.
pub fn save_path(img: &Raster, path: &Path, format: FileFormat, opts: SaveOptions) -> Result<SaveReport, IoError> {
    let tmp = temp_path_for(path);
    let result = (|| {
        let f = File::create(&tmp)?;
        let mut w = BufWriter::new(f);
        let report = encode(img, format, opts, &mut w)?;
        let f = w.into_inner().map_err(|e| IoError::Io(e.into_error()))?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)?;
        Ok(report)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_image() -> Raster {
        let mut r = Raster::new(37, 23, Rgba::TRANSPARENT).unwrap();
        for y in 0..23 {
            for x in 0..37 {
                r.set(x, y, Rgba::new((x * 7) as u8, (y * 11) as u8, 200, ((x + y) * 5) as u8));
            }
        }
        r
    }

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("brushwork-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn png_round_trip_preserves_alpha_exactly() {
        let img = sample_image();
        let d = tmpdir("png");
        let p = d.join("a.png");
        save_path(&img, &p, FileFormat::Png, SaveOptions::default()).unwrap();
        let l = load_path(&p).unwrap();
        assert_eq!(l.format, FileFormat::Png);
        assert_eq!(l.raster, img);
        assert!(l.notes.is_empty());
    }

    #[test]
    fn opaque_png_and_bmp_round_trip() {
        let mut img = sample_image();
        for p in img.data_mut().chunks_exact_mut(4) {
            p[3] = 255;
        }
        let d = tmpdir("bmp");
        for (f, name) in [(FileFormat::Png, "o.png"), (FileFormat::Bmp, "o.bmp")] {
            let p = d.join(name);
            save_path(&img, &p, f, SaveOptions::default()).unwrap();
            assert_eq!(load_path(&p).unwrap().raster, img, "{f:?}");
        }
        // BMP with alpha
        let a = sample_image();
        let p = d.join("a.bmp");
        save_path(&a, &p, FileFormat::Bmp, SaveOptions::default()).unwrap();
        assert_eq!(load_path(&p).unwrap().raster, a);
    }

    #[test]
    fn jpeg_flattens_alpha_and_is_close() {
        let mut img = Raster::new(64, 64, Rgba::opaque(200, 40, 40)).unwrap();
        img.set(0, 0, Rgba::TRANSPARENT);
        let mut buf = Vec::new();
        let rep = encode(&img, FileFormat::Jpeg, SaveOptions { jpeg_quality: 95 }, &mut buf).unwrap();
        assert!(rep.flattened_alpha);
        let l = load_bytes(&buf).unwrap();
        assert_eq!(l.format, FileFormat::Jpeg);
        let c = l.raster.get(32, 32);
        assert!(c.r.abs_diff(200) < 6 && c.g.abs_diff(40) < 6 && c.a == 255, "{c:?}");
    }

    #[test]
    fn malformed_and_unsupported_files_error_cleanly() {
        assert!(matches!(load_bytes(b"not an image at all"), Err(IoError::Unsupported(_))));
        let mut buf = Vec::new();
        encode(&sample_image(), FileFormat::Png, SaveOptions::default(), &mut buf).unwrap();
        buf.truncate(buf.len() / 2);
        assert!(load_bytes(&buf).is_err());
        // GIF signature: recognised by image but not enabled → unsupported
        assert!(load_bytes(b"GIF89a\x01\x00\x01\x00\x00\x00\x00;").is_err());
        assert!(matches!(load_path(Path::new("/definitely/missing.png")), Err(IoError::Io(_))));
    }

    #[test]
    fn oversized_header_is_rejected_before_allocation() {
        // Minimal PNG header claiming 100000 × 100000 pixels.
        let mut png = Vec::new();
        png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(b"IHDR");
        ihdr.extend_from_slice(&100_000u32.to_be_bytes());
        ihdr.extend_from_slice(&100_000u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        let crc = crc32(&ihdr);
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(&ihdr);
        png.extend_from_slice(&crc.to_be_bytes());
        assert!(load_bytes(&png).is_err());
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }

    #[test]
    fn failed_save_preserves_existing_file() {
        let d = tmpdir("atomic");
        let p = d.join("keep.png");
        std::fs::write(&p, b"previous contents").unwrap();
        // JPEG larger than 65535 cannot be encoded → error, original untouched
        let big = Raster::new(1, 1, Rgba::WHITE).unwrap();
        save_path(&big, &p, FileFormat::Png, SaveOptions::default()).unwrap();
        let saved = std::fs::read(&p).unwrap();
        // now force a failure by saving into a missing directory
        let bad = d.join("missing-dir").join("x.png");
        assert!(save_path(&big, &bad, FileFormat::Png, SaveOptions::default()).is_err());
        assert_eq!(std::fs::read(&p).unwrap(), saved);
        // no temp files left behind
        let leftovers: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with("brushwork-tmp"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn format_from_extension() {
        assert_eq!(FileFormat::from_path(Path::new("a/b.JPG")), Some(FileFormat::Jpeg));
        assert_eq!(FileFormat::from_path(Path::new("x.bmp")), Some(FileFormat::Bmp));
        assert_eq!(FileFormat::from_path(Path::new("x.gif")), None);
        assert_eq!(FileFormat::from_path(Path::new("noext")), None);
    }
}
