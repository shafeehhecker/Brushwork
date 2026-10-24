//! Native file dialogs and the system clipboard.

use brushwork::io::FileFormat;
use brushwork::raster::Raster;
use std::path::{Path, PathBuf};

pub fn open_dialog(dir: Option<&Path>) -> Option<Vec<PathBuf>> {
    let mut d = rfd::FileDialog::new()
        .set_title("Open image")
        .add_filter("Images (PNG, JPEG, BMP)", &["png", "jpg", "jpeg", "jpe", "jfif", "bmp", "dib"])
        .add_filter("PNG", FileFormat::Png.extensions())
        .add_filter("JPEG", FileFormat::Jpeg.extensions())
        .add_filter("BMP", FileFormat::Bmp.extensions())
        .add_filter("All files", &["*"]);
    if let Some(dir) = dir {
        d = d.set_directory(dir);
    }
    d.pick_files()
}

/// Returns the chosen path with a supported extension ensured.
pub fn save_dialog(current: Option<&Path>, suggested_name: &str, format: FileFormat) -> Option<(PathBuf, FileFormat)> {
    let order: Vec<FileFormat> =
        std::iter::once(format).chain(FileFormat::ALL.into_iter().filter(|f| *f != format)).collect();
    let mut d = rfd::FileDialog::new().set_title("Save image as");
    for f in &order {
        d = d.add_filter(f.name(), f.extensions());
    }
    let stem = Path::new(suggested_name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".into());
    d = d.set_file_name(format!("{stem}.{}", format.extensions()[0]));
    if let Some(dir) = current.and_then(|p| p.parent()) {
        d = d.set_directory(dir);
    }
    let path = d.save_file()?;
    match FileFormat::from_path(&path) {
        Some(f) => Some((path, f)),
        None => {
            // No (or unknown) extension: append the chosen default's extension.
            let mut p = path.into_os_string();
            p.push(".");
            p.push(format.extensions()[0]);
            Some((PathBuf::from(p), format))
        }
    }
}

pub struct Clipboard {
    inner: Option<arboard::Clipboard>,
}

impl Clipboard {
    pub fn new() -> Self {
        Clipboard { inner: None }
    }

    fn get(&mut self) -> Result<&mut arboard::Clipboard, String> {
        if self.inner.is_none() {
            self.inner = Some(arboard::Clipboard::new().map_err(|e| format!("clipboard unavailable: {e}"))?);
        }
        Ok(self.inner.as_mut().expect("just set"))
    }

    pub fn set_image(&mut self, img: &Raster) -> Result<(), String> {
        let data = arboard::ImageData {
            width: img.width() as usize,
            height: img.height() as usize,
            bytes: std::borrow::Cow::Borrowed(img.data()),
        };
        self.get()?.set_image(data).map_err(|e| format!("could not copy to the clipboard: {e}"))
    }

    pub fn get_image(&mut self) -> Result<Raster, String> {
        let img = self.get()?.get_image().map_err(|e| match e {
            arboard::Error::ContentNotAvailable => "the clipboard does not contain an image".to_string(),
            e => format!("could not read the clipboard: {e}"),
        })?;
        Raster::from_rgba(img.width as u32, img.height as u32, img.bytes.into_owned())
            .map_err(|e| format!("clipboard image: {e}"))
    }
}
