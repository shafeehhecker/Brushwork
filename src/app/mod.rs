//! The Brushwork desktop application (egui/eframe).

mod canvas;
mod dialogs;
mod platform;
mod tools;

use brushwork::color::{PALETTE, Rgba};
use brushwork::document::Document;
use brushwork::history::Dirty;
use brushwork::io::{self, FileFormat, SaveOptions};
use brushwork::raster::Raster;
use brushwork::text::FontLibrary;
use brushwork::transform::Rotation;
use brushwork::view::View;
use canvas::{CanvasCtx, Interaction, Tab, TextEditorAction};
use dialogs::Dialog;
use eframe::egui::{self, Color32, Key, KeyboardShortcut, Modifiers, Rect, Sense, Stroke, Vec2, pos2, vec2};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tools::{Settings, ShapeStyle, Tool};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    New,
    Open,
    Save,
    SaveAs,
    Close,
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    PasteNew,
    Delete,
    SelectAll,
    Deselect,
    Crop,
    Resize,
    FlipH,
    FlipV,
    RotCw,
    RotCcw,
    Rot180,
    ZoomIn,
    ZoomOut,
    Fit,
    Actual,
    SwapColors,
    ResetColors,
    NextTab,
    PrevTab,
    Shortcuts,
    About,
}

const CTRL: Modifiers = Modifiers::COMMAND;
const CTRL_SHIFT: Modifiers = Modifiers { shift: true, ..Modifiers::COMMAND };
const CTRL_ALT: Modifiers = Modifiers { alt: true, ..Modifiers::COMMAND };

/// Keyboard shortcuts. More specific modifier combinations come first.
const SHORTCUTS: &[(Cmd, KeyboardShortcut)] = &[
    (Cmd::SaveAs, KeyboardShortcut::new(CTRL_SHIFT, Key::S)),
    (Cmd::Redo, KeyboardShortcut::new(CTRL_SHIFT, Key::Z)),
    (Cmd::Crop, KeyboardShortcut::new(CTRL_SHIFT, Key::X)),
    (Cmd::PasteNew, KeyboardShortcut::new(CTRL_ALT, Key::V)),
    (Cmd::PrevTab, KeyboardShortcut::new(CTRL_SHIFT, Key::Tab)),
    (Cmd::FlipH, KeyboardShortcut::new(CTRL_SHIFT, Key::H)),
    (Cmd::FlipV, KeyboardShortcut::new(CTRL_SHIFT, Key::V)),
    (Cmd::RotCw, KeyboardShortcut::new(CTRL_SHIFT, Key::R)),
    (Cmd::New, KeyboardShortcut::new(CTRL, Key::N)),
    (Cmd::Open, KeyboardShortcut::new(CTRL, Key::O)),
    (Cmd::Save, KeyboardShortcut::new(CTRL, Key::S)),
    (Cmd::Close, KeyboardShortcut::new(CTRL, Key::W)),
    (Cmd::Quit, KeyboardShortcut::new(CTRL, Key::Q)),
    (Cmd::Undo, KeyboardShortcut::new(CTRL, Key::Z)),
    (Cmd::Redo, KeyboardShortcut::new(CTRL, Key::Y)),
    (Cmd::SelectAll, KeyboardShortcut::new(CTRL, Key::A)),
    (Cmd::Deselect, KeyboardShortcut::new(CTRL, Key::D)),
    (Cmd::Resize, KeyboardShortcut::new(CTRL, Key::R)),
    (Cmd::ZoomIn, KeyboardShortcut::new(CTRL, Key::Plus)),
    (Cmd::ZoomIn, KeyboardShortcut::new(CTRL, Key::Equals)),
    (Cmd::ZoomOut, KeyboardShortcut::new(CTRL, Key::Minus)),
    (Cmd::Fit, KeyboardShortcut::new(CTRL, Key::Num0)),
    (Cmd::Actual, KeyboardShortcut::new(CTRL, Key::Num1)),
    (Cmd::NextTab, KeyboardShortcut::new(CTRL, Key::Tab)),
    (Cmd::Shortcuts, KeyboardShortcut::new(Modifiers::NONE, Key::F1)),
];

fn shortcut_for(cmd: Cmd) -> Option<&'static KeyboardShortcut> {
    SHORTCUTS.iter().find(|(c, _)| *c == cmd).map(|(_, s)| s)
}

enum Fonts {
    Loading(mpsc::Receiver<FontLibrary>),
    Ready(Box<FontLibrary>),
    Failed,
}

struct Status {
    text: String,
    error: bool,
    at: Instant,
}

pub struct BrushworkApp {
    tabs: Vec<Tab>,
    active: usize,
    tool: Tool,
    prev_tool: Tool,
    settings: Settings,
    fg: Rgba,
    bg: Rgba,
    fonts: Fonts,
    font_filter: String,
    dialog: Option<Dialog>,
    status: Option<Status>,
    untitled_counter: u32,
    clipboard: platform::Clipboard,
    checker: Option<egui::TextureHandle>,
    allow_close: bool,
    title: String,
    fg_hex: String,
    bg_hex: String,
    startup_probe: Option<Instant>,
}

impl BrushworkApp {
    pub fn new(cc: &eframe::CreationContext<'_>, files: Vec<PathBuf>, startup_probe: Option<Instant>) -> Self {
        cc.egui_ctx.set_theme(egui::ThemePreference::System);
        let (tx, rx) = mpsc::channel();
        let ctx = cc.egui_ctx.clone();
        std::thread::Builder::new()
            .name("font-scan".into())
            .spawn(move || {
                let mut lib = FontLibrary::load_system();
                let defs = egui::FontDefinitions::default();
                if let Some(fd) = defs.font_data.get("Ubuntu-Light") {
                    lib.set_fallback((*fd.font).as_ref().to_vec());
                }
                let _ = tx.send(lib);
                ctx.request_repaint();
            })
            .ok();
        let mut app = BrushworkApp {
            tabs: Vec::new(),
            active: 0,
            tool: Tool::Brush,
            prev_tool: Tool::Brush,
            settings: Settings::default(),
            fg: Rgba::BLACK,
            bg: Rgba::WHITE,
            fonts: Fonts::Loading(rx),
            font_filter: String::new(),
            dialog: None,
            status: None,
            untitled_counter: 0,
            clipboard: platform::Clipboard::new(),
            checker: None,
            allow_close: false,
            title: String::new(),
            fg_hex: String::new(),
            bg_hex: String::new(),
            startup_probe,
        };
        if !files.is_empty() {
            app.open_paths(&files);
        }
        if app.tabs.is_empty() {
            app.new_document(1280, 720, Rgba::WHITE);
        }
        app
    }

    // ---- helpers ---------------------------------------------------------

    fn info(&mut self, s: impl Into<String>) {
        self.status = Some(Status { text: s.into(), error: false, at: Instant::now() });
    }
    fn error(&mut self, s: impl Into<String>) {
        self.status = Some(Status { text: s.into(), error: true, at: Instant::now() });
    }
    fn message(&mut self, title: &str, text: impl Into<String>) {
        self.dialog = Some(Dialog::Message { title: title.to_string(), text: text.into() });
    }

    fn tab(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active)
    }

    fn font_library(&mut self) -> Option<&mut FontLibrary> {
        if let Fonts::Loading(rx) = &self.fonts {
            match rx.try_recv() {
                Ok(lib) => {
                    if self.settings.text.family.is_empty() {
                        self.settings.text.family = lib.default_family();
                    }
                    self.fonts = Fonts::Ready(Box::new(lib));
                }
                Err(mpsc::TryRecvError::Disconnected) => self.fonts = Fonts::Failed,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        match &mut self.fonts {
            Fonts::Ready(l) => Some(&mut **l),
            _ => None,
        }
    }

    fn next_untitled(&mut self) -> String {
        self.untitled_counter += 1;
        if self.untitled_counter == 1 { "Untitled".into() } else { format!("Untitled {}", self.untitled_counter) }
    }

    fn add_tab(&mut self, doc: Document) {
        // Replace a pristine, unused blank document (like the one shown at startup).
        if let Some(t) = self.tabs.get(self.active)
            && t.doc.path.is_none()
            && !t.doc.is_dirty()
            && t.doc.history.is_empty()
            && self.tabs.len() == 1
        {
            self.tabs.clear();
        }
        self.tabs.push(Tab::new(doc));
        self.active = self.tabs.len() - 1;
    }

    fn new_document(&mut self, w: u32, h: u32, bg: Rgba) {
        let name = self.next_untitled();
        match Document::new_blank(w, h, bg, name) {
            Ok(d) => {
                self.tabs.push(Tab::new(d));
                self.active = self.tabs.len() - 1;
            }
            Err(e) => self.message("Cannot create image", e.to_string()),
        }
    }

    pub fn open_paths(&mut self, paths: &[PathBuf]) {
        for p in paths {
            let canon = std::fs::canonicalize(p).unwrap_or_else(|_| p.clone());
            if let Some(i) = self
                .tabs
                .iter()
                .position(|t| t.doc.path.as_ref().and_then(|q| std::fs::canonicalize(q).ok()) == Some(canon.clone()))
            {
                self.active = i;
                continue;
            }
            match io::load_path(p) {
                Ok(l) => {
                    let notes = l.notes.join("; ");
                    let doc = Document::from_loaded(l, canon);
                    let name = doc.display_name();
                    self.add_tab(doc);
                    if notes.is_empty() {
                        self.info(format!("Opened {name}"))
                    } else {
                        self.info(format!("Opened {name}: {notes}"))
                    }
                }
                Err(e) => self.message("Cannot open image", format!("{}\n\n{e}", p.display())),
            }
        }
    }

    /// Saves a tab; returns true on success.
    fn save_tab(&mut self, idx: usize, save_as: bool) -> bool {
        let Some(tab) = self.tabs.get_mut(idx) else { return false };
        tab.cancel_interaction();
        let style = self.settings.text.clone();
        let fonts = match &mut self.fonts {
            Fonts::Ready(l) => Some(&mut **l),
            _ => None,
        };
        tab.commit_text(fonts, &style, self.fg);
        tab.commit_floating();
        let doc = &tab.doc;
        let target = match (&doc.path, doc.format, save_as) {
            (Some(p), Some(f), false) => Some((p.clone(), f)),
            _ => platform::save_dialog(doc.path.as_deref(), &doc.display_name(), doc.format.unwrap_or(FileFormat::Png)),
        };
        let Some((path, format)) = target else { return false };
        match io::save_path(&tab.doc.image, &path, format, SaveOptions::default()) {
            Ok(rep) => {
                tab.doc.path = Some(path);
                tab.doc.format = Some(format);
                tab.doc.mark_saved();
                let name = tab.doc.display_name();
                if rep.flattened_alpha {
                    self.info(format!(
                        "Saved {name} — JPEG has no transparency, so transparent areas were saved as white"
                    ));
                } else {
                    self.info(format!("Saved {name}"));
                }
                true
            }
            Err(e) => {
                self.message(
                    "Save failed",
                    format!(
                        "Could not save {}.\nThe previous file (if any) was left unchanged.\n\n{e}",
                        path.display()
                    ),
                );
                false
            }
        }
    }

    fn close_tab(&mut self, idx: usize, force: bool) {
        if idx >= self.tabs.len() {
            return;
        }
        if !force && self.tabs[idx].doc.is_dirty() {
            self.active = idx;
            self.dialog = Some(Dialog::ConfirmClose { tab: idx });
            return;
        }
        self.tabs.remove(idx);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len().saturating_sub(1);
        }
    }

    fn paste_image(&mut self, img: Raster, ctx: &egui::Context) {
        let ppp = ctx.pixels_per_point();
        let Some(t) = self.tabs.get(self.active) else {
            let name = self.next_untitled();
            self.add_tab(Document::with_image(img, None, None, name));
            return;
        };
        let (w, h) = t.doc.size();
        if img.width() > w || img.height() > h {
            self.dialog = Some(Dialog::PasteLarger { img });
            return;
        }
        let p = t.view.to_image((0.0, 0.0), ppp);
        let x = (p.0.ceil() as i32).clamp(0, (w - img.width()) as i32);
        let y = (p.1.ceil() as i32).clamp(0, (h - img.height()) as i32);
        self.finish_paste(img, x, y);
    }

    fn finish_paste(&mut self, img: Raster, x: i32, y: i32) {
        self.set_tool(Tool::Select);
        if let Some(t) = self.tab() {
            t.cancel_interaction();
            let d = t.doc.paste(img, x, y);
            t.mark_opt(d);
            t.mark(Dirty::Rect(brushwork::geom::IRect::EMPTY));
        }
        self.info("Pasted — drag to position, Enter to apply");
    }

    fn set_tool(&mut self, tool: Tool) {
        if tool == self.tool {
            return;
        }
        let style = self.settings.text.clone();
        let fg = self.fg;
        // Leaving the text tool applies the text being edited.
        if self.tool == Tool::Text {
            let fonts = match &mut self.fonts {
                Fonts::Ready(l) => Some(&mut **l),
                _ => None,
            };
            if let Some(t) = self.tabs.get_mut(self.active) {
                t.commit_text(fonts, &style, fg);
            }
        }
        if let Some(t) = self.tabs.get_mut(self.active)
            && matches!(t.interaction, Interaction::Polygon { .. })
        {
            t.cancel_interaction();
        }
        self.prev_tool = self.tool;
        self.tool = tool;
    }

    // ---- commands --------------------------------------------------------

    fn can(&self, cmd: Cmd) -> bool {
        let t = self.tabs.get(self.active);
        let has_tab = t.is_some();
        let sel = t.is_some_and(|t| t.doc.has_selection());
        match cmd {
            Cmd::Save
            | Cmd::SaveAs
            | Cmd::Close
            | Cmd::SelectAll
            | Cmd::Resize
            | Cmd::FlipH
            | Cmd::FlipV
            | Cmd::RotCw
            | Cmd::RotCcw
            | Cmd::Rot180
            | Cmd::ZoomIn
            | Cmd::ZoomOut
            | Cmd::Fit
            | Cmd::Actual => has_tab,
            Cmd::Undo => t.is_some_and(|t| t.doc.history.can_undo() || t.doc.floating.is_some()),
            Cmd::Redo => t.is_some_and(|t| t.doc.history.can_redo() && t.doc.floating.is_none()),
            Cmd::Cut | Cmd::Copy | Cmd::Delete | Cmd::Deselect | Cmd::Crop => sel,
            Cmd::Paste => true,
            Cmd::NextTab | Cmd::PrevTab => self.tabs.len() > 1,
            _ => true,
        }
    }

    fn run(&mut self, ctx: &egui::Context, cmd: Cmd) {
        if !self.can(cmd) {
            return;
        }
        let ppp = ctx.pixels_per_point();
        // Most commands should not happen mid-stroke.
        if !matches!(cmd, Cmd::ZoomIn | Cmd::ZoomOut | Cmd::Fit | Cmd::Actual | Cmd::SwapColors | Cmd::ResetColors)
            && let Some(t) = self.tab()
        {
            if t.is_busy() && !matches!(t.interaction, Interaction::Polygon { .. }) {
                return;
            }
            if matches!(t.interaction, Interaction::Polygon { .. }) {
                t.cancel_interaction();
            }
        }
        // Commands that change pixels apply pending text first.
        if matches!(
            cmd,
            Cmd::Undo
                | Cmd::Redo
                | Cmd::Crop
                | Cmd::Resize
                | Cmd::FlipH
                | Cmd::FlipV
                | Cmd::RotCw
                | Cmd::RotCcw
                | Cmd::Rot180
                | Cmd::Cut
                | Cmd::Copy
                | Cmd::Paste
        ) {
            let style = self.settings.text.clone();
            let fg = self.fg;
            let fonts = match &mut self.fonts {
                Fonts::Ready(l) => Some(&mut **l),
                _ => None,
            };
            if let Some(t) = self.tabs.get_mut(self.active) {
                if cmd == Cmd::Undo && t.text.is_some() {
                    t.cancel_text();
                    return;
                }
                t.commit_text(fonts, &style, fg);
            }
        }
        match cmd {
            Cmd::New => {
                let (w, h) = self.clipboard.get_image().map(|i| (i.width(), i.height())).unwrap_or((1280, 720));
                self.dialog = Some(Dialog::new_image(w, h));
            }
            Cmd::Open => {
                let dir = self
                    .tabs
                    .get(self.active)
                    .and_then(|t| t.doc.path.as_ref())
                    .and_then(|p| p.parent().map(Path::to_path_buf));
                if let Some(paths) = platform::open_dialog(dir.as_deref()) {
                    self.open_paths(&paths);
                }
            }
            Cmd::Save => {
                self.save_tab(self.active, false);
            }
            Cmd::SaveAs => {
                self.save_tab(self.active, true);
            }
            Cmd::Close => self.close_tab(self.active, false),
            Cmd::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Cmd::Undo => {
                if let Some(t) = self.tab() {
                    let d = t.doc.undo();
                    t.mark_opt(d);
                    let l = t.doc.history.redo_label().map(str::to_owned);
                    if let Some(l) = l {
                        self.info(format!("Undid: {l}"));
                    }
                }
            }
            Cmd::Redo => {
                if let Some(t) = self.tab() {
                    let d = t.doc.redo();
                    t.mark_opt(d);
                    let l = t.doc.history.undo_label().map(str::to_owned);
                    if let Some(l) = l {
                        self.info(format!("Redid: {l}"));
                    }
                }
            }
            Cmd::Copy | Cmd::Cut => {
                let Some(img) = self.tabs.get(self.active).and_then(|t| t.doc.copy_selection()) else { return };
                match self.clipboard.set_image(&img) {
                    Ok(()) => {
                        if cmd == Cmd::Cut
                            && let Some(t) = self.tab()
                        {
                            let d = t.doc.delete_selection();
                            t.mark_opt(d);
                        }
                        self.info(format!(
                            "{} {} × {} pixels",
                            if cmd == Cmd::Cut { "Cut" } else { "Copied" },
                            img.width(),
                            img.height()
                        ));
                    }
                    Err(e) => self.error(e),
                }
            }
            Cmd::Paste => match self.clipboard.get_image() {
                Ok(img) => self.paste_image(img, ctx),
                Err(e) => self.error(e),
            },
            Cmd::PasteNew => match self.clipboard.get_image() {
                Ok(img) => {
                    let name = self.next_untitled();
                    self.tabs.push(Tab::new(Document::with_image(img, None, None, name)));
                    self.active = self.tabs.len() - 1;
                }
                Err(e) => self.error(e),
            },
            Cmd::Delete => {
                if let Some(t) = self.tab() {
                    let d = t.doc.delete_selection();
                    t.mark_opt(d);
                }
            }
            Cmd::SelectAll => {
                self.set_tool(Tool::Select);
                if let Some(t) = self.tab() {
                    let d = t.doc.select_all();
                    t.mark_opt(d);
                }
            }
            Cmd::Deselect => {
                if let Some(t) = self.tab() {
                    let d = t.doc.set_selection(None);
                    t.mark_opt(d);
                }
            }
            Cmd::Crop => {
                if let Some(t) = self.tab() {
                    let d = t.doc.crop_to_selection();
                    t.mark_opt(d);
                    t.fit();
                }
            }
            Cmd::Resize => {
                if let Some(t) = self.tab() {
                    t.commit_floating();
                    let (w, h) = t.doc.size();
                    self.dialog = Some(Dialog::resize(w, h));
                }
            }
            Cmd::FlipH | Cmd::FlipV => {
                if let Some(t) = self.tab() {
                    let d = t.doc.flip(cmd == Cmd::FlipH);
                    t.mark(d);
                }
            }
            Cmd::RotCw | Cmd::RotCcw | Cmd::Rot180 => {
                let r = match cmd {
                    Cmd::RotCw => Rotation::Cw90,
                    Cmd::RotCcw => Rotation::Ccw90,
                    _ => Rotation::Half,
                };
                if let Some(t) = self.tab() {
                    let was_float = t.doc.floating.is_some();
                    let d = t.doc.rotate(r);
                    t.mark(d);
                    if !was_float {
                        t.fit();
                    }
                }
            }
            Cmd::ZoomIn => {
                if let Some(t) = self.tab() {
                    let z = t.view.next_zoom_in();
                    t.zoom_to(z, ppp);
                }
            }
            Cmd::ZoomOut => {
                if let Some(t) = self.tab() {
                    let z = t.view.next_zoom_out();
                    t.zoom_to(z, ppp);
                }
            }
            Cmd::Fit => {
                if let Some(t) = self.tab() {
                    let s = t.canvas_rect.size();
                    t.view = View::fit(t.doc.size(), (s.x, s.y), ppp, true);
                }
            }
            Cmd::Actual => {
                if let Some(t) = self.tab() {
                    t.actual_size(ppp);
                }
            }
            Cmd::SwapColors => std::mem::swap(&mut self.fg, &mut self.bg),
            Cmd::ResetColors => {
                self.fg = Rgba::BLACK;
                self.bg = Rgba::WHITE;
            }
            Cmd::NextTab => self.active = (self.active + 1) % self.tabs.len(),
            Cmd::PrevTab => self.active = (self.active + self.tabs.len() - 1) % self.tabs.len(),
            Cmd::Shortcuts => self.dialog = Some(Dialog::Shortcuts),
            Cmd::About => self.dialog = Some(Dialog::About),
        }
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let typing = ctx.egui_wants_keyboard_input();
        let mut cmds = Vec::new();
        if self.dialog.is_none() {
            // Platform clipboard shortcuts arrive as events, not keys.
            ctx.input(|i| {
                for e in &i.events {
                    match e {
                        egui::Event::Copy if !typing => cmds.push(Cmd::Copy),
                        egui::Event::Cut if !typing => cmds.push(Cmd::Cut),
                        egui::Event::Paste(_) | egui::Event::PasteImage(_) if !typing => cmds.push(Cmd::Paste),
                        _ => {}
                    }
                }
            });
            if !typing {
                for (cmd, sc) in SHORTCUTS {
                    if ctx.input_mut(|i| i.consume_shortcut(sc)) {
                        cmds.push(*cmd);
                    }
                }
            } else {
                // Global shortcuts that are safe while a text box has focus.
                for cmd in [Cmd::Save, Cmd::SaveAs, Cmd::New, Cmd::Open] {
                    if let Some(sc) = shortcut_for(cmd)
                        && ctx.input_mut(|i| i.consume_shortcut(sc))
                    {
                        cmds.push(cmd);
                    }
                }
            }
        }
        for c in cmds {
            self.run(ctx, c);
        }
        if typing || self.dialog.is_some() {
            return;
        }
        let (pressed, shift, no_mods): (Vec<Key>, bool, bool) = ctx.input(|i| {
            let keys = i
                .events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, pressed: true, .. } => Some(*key),
                    _ => None,
                })
                .collect();
            (keys, i.modifiers.shift, !i.modifiers.command && !i.modifiers.alt)
        });
        for key in pressed {
            if no_mods
                && !shift
                && let Some(t) = Tool::ALL.into_iter().find(|t| t.key() == key)
            {
                self.set_tool(t);
                continue;
            }
            if !no_mods {
                continue;
            }
            match key {
                Key::X if !shift => self.run(ctx, Cmd::SwapColors),
                Key::D if !shift => self.run(ctx, Cmd::ResetColors),
                Key::Delete | Key::Backspace => self.run(ctx, Cmd::Delete),
                Key::OpenBracket | Key::CloseBracket => {
                    let up = key == Key::CloseBracket;
                    let f = |v: f32| if up { (v * 1.25).max(v + 1.0) } else { (v / 1.25).min(v - 1.0) };
                    match self.tool {
                        Tool::Brush => self.settings.brush_size = f(self.settings.brush_size).clamp(1.0, 500.0),
                        Tool::Eraser => self.settings.eraser_size = f(self.settings.eraser_size).clamp(1.0, 500.0),
                        Tool::Pencil => {
                            self.settings.pencil_size =
                                (f(self.settings.pencil_size as f32).round() as u32).clamp(1, 100)
                        }
                        _ => self.settings.shape_width = f(self.settings.shape_width).clamp(1.0, 100.0),
                    }
                }
                Key::Escape => {
                    if let Some(t) = self.tabs.get_mut(self.active) {
                        if t.text.is_some() {
                            t.cancel_text();
                        } else if t.is_busy() {
                            t.cancel_interaction();
                        } else {
                            let d = t.doc.set_selection(None);
                            t.mark_opt(d);
                        }
                    }
                }
                Key::Enter => {
                    let (s, fg, bg) = (&self.settings, self.fg, self.bg);
                    if let Some(t) = self.tabs.get_mut(self.active) {
                        if matches!(t.interaction, Interaction::Polygon { .. }) {
                            t.finish_polygon_key(s, fg, bg);
                        } else if t.doc.floating.is_some() {
                            t.commit_floating();
                        }
                    }
                }
                Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown => {
                    let step = if shift { 10 } else { 1 };
                    let (dx, dy) = match key {
                        Key::ArrowLeft => (-step, 0),
                        Key::ArrowRight => (step, 0),
                        Key::ArrowUp => (0, -step),
                        _ => (0, step),
                    };
                    if let Some(t) = self.tabs.get_mut(self.active) {
                        if t.is_busy() {
                            continue;
                        }
                        if t.doc.floating.is_none() && t.doc.selection.is_some() {
                            let d = t.doc.lift_selection();
                            t.mark_opt(d);
                        }
                        if let Some(r) = t.doc.floating.as_ref().map(|f| f.rect) {
                            t.doc.set_floating_rect(r.translate(dx, dy));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // ---- UI sections -------------------------------------------------------

    fn menu_item(&mut self, ui: &mut egui::Ui, label: &str, cmd: Cmd) {
        let mut b = egui::Button::new(label);
        if let Some(sc) = shortcut_for(cmd) {
            b = b.shortcut_text(ui.ctx().format_shortcut(sc));
        }
        if ui.add_enabled(self.can(cmd), b).clicked() {
            ui.close();
            let ctx = ui.ctx().clone();
            self.run(&ctx, cmd);
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                self.menu_item(ui, "New…", Cmd::New);
                self.menu_item(ui, "Open…", Cmd::Open);
                ui.separator();
                self.menu_item(ui, "Save", Cmd::Save);
                self.menu_item(ui, "Save As…", Cmd::SaveAs);
                ui.separator();
                self.menu_item(ui, "Close", Cmd::Close);
                self.menu_item(ui, "Exit", Cmd::Quit);
            });
            ui.menu_button("Edit", |ui| {
                let (u, r) = self
                    .tabs
                    .get(self.active)
                    .map(|t| {
                        (t.doc.history.undo_label().map(str::to_owned), t.doc.history.redo_label().map(str::to_owned))
                    })
                    .unwrap_or_default();
                let floating = self.tabs.get(self.active).is_some_and(|t| t.doc.floating.is_some());
                let ul = if floating {
                    "Undo move/paste".to_string()
                } else {
                    u.map(|l| format!("Undo {l}")).unwrap_or_else(|| "Undo".into())
                };
                self.menu_item(ui, &ul, Cmd::Undo);
                self.menu_item(ui, &r.map(|l| format!("Redo {l}")).unwrap_or_else(|| "Redo".into()), Cmd::Redo);
                ui.separator();
                let mut item = |ui: &mut egui::Ui, l: &str, c: Cmd, sc: &str| {
                    if ui.add_enabled(self.can(c), egui::Button::new(l).shortcut_text(sc)).clicked() {
                        ui.close();
                        let ctx = ui.ctx().clone();
                        self.run(&ctx, c);
                    }
                };
                let cmd = if cfg!(target_os = "macos") { "⌘" } else { "Ctrl+" };
                item(ui, "Cut", Cmd::Cut, &format!("{cmd}X"));
                item(ui, "Copy", Cmd::Copy, &format!("{cmd}C"));
                item(ui, "Paste", Cmd::Paste, &format!("{cmd}V"));
                self.menu_item(ui, "Paste into new image", Cmd::PasteNew);
                ui.separator();
                self.menu_item(ui, "Select all", Cmd::SelectAll);
                self.menu_item(ui, "Deselect", Cmd::Deselect);
                if ui
                    .add_enabled(self.can(Cmd::Delete), egui::Button::new("Erase selection").shortcut_text("Del"))
                    .clicked()
                {
                    ui.close();
                    let ctx = ui.ctx().clone();
                    self.run(&ctx, Cmd::Delete);
                }
            });
            ui.menu_button("Image", |ui| {
                self.menu_item(ui, "Crop to selection", Cmd::Crop);
                self.menu_item(ui, "Resize…", Cmd::Resize);
                ui.separator();
                let floating = self.tabs.get(self.active).is_some_and(|t| t.doc.floating.is_some());
                let what = if floating { "selection" } else { "image" };
                self.menu_item(ui, &format!("Flip {what} horizontally"), Cmd::FlipH);
                self.menu_item(ui, &format!("Flip {what} vertically"), Cmd::FlipV);
                ui.separator();
                self.menu_item(ui, "Rotate 90° clockwise", Cmd::RotCw);
                self.menu_item(ui, "Rotate 90° counter-clockwise", Cmd::RotCcw);
                self.menu_item(ui, "Rotate 180°", Cmd::Rot180);
            });
            ui.menu_button("View", |ui| {
                self.menu_item(ui, "Zoom in", Cmd::ZoomIn);
                self.menu_item(ui, "Zoom out", Cmd::ZoomOut);
                self.menu_item(ui, "Fit to window", Cmd::Fit);
                self.menu_item(ui, "Actual size (100%)", Cmd::Actual);
                ui.separator();
                let mut theme = ui.ctx().options(|o| o.theme_preference);
                ui.horizontal(|ui| {
                    ui.label("Theme:");
                    ui.selectable_value(&mut theme, egui::ThemePreference::System, "System");
                    ui.selectable_value(&mut theme, egui::ThemePreference::Light, "Light");
                    ui.selectable_value(&mut theme, egui::ThemePreference::Dark, "Dark");
                });
                ui.ctx().set_theme(theme);
            });
            ui.menu_button("Help", |ui| {
                self.menu_item(ui, "Keyboard shortcuts", Cmd::Shortcuts);
                self.menu_item(ui, "About Brushwork", Cmd::About);
            });
        });
    }

    fn tool_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        egui::Grid::new("tools").spacing(vec2(2.0, 2.0)).show(ui, |ui| {
            for (i, tool) in Tool::ALL.into_iter().enumerate() {
                let selected = self.tool == tool;
                let (rect, resp) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
                let v = ui.style().interact_selectable(&resp, selected);
                if selected || resp.hovered() {
                    ui.painter().rect(rect, 4.0, v.weak_bg_fill, v.bg_stroke, egui::StrokeKind::Inside);
                }
                tools::paint_icon(ui.painter(), rect.shrink(6.0), tool, v.fg_stroke.color);
                let resp = resp.on_hover_ui(|ui| {
                    ui.strong(format!("{} ({})", tool.name(), tool.key().name()));
                    ui.label(tool.hint());
                });
                resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, selected, tool.name()));
                if resp.clicked() {
                    self.set_tool(tool);
                }
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
        ui.add_space(10.0);
        // Primary / secondary swatches.
        let (rect, _) = ui.allocate_exact_size(vec2(66.0, 56.0), Sense::hover());
        let back = Rect::from_min_size(rect.min + vec2(24.0, 22.0), vec2(34.0, 34.0));
        let front = Rect::from_min_size(rect.min + vec2(4.0, 2.0), vec2(34.0, 34.0));
        let bg_resp = ui
            .interact(back, ui.scope_id().with("bg-swatch"), Sense::click())
            .on_hover_text("Secondary colour — click to edit");
        swatch(ui.painter(), back, self.bg);
        let fg_resp = ui
            .interact(front, ui.scope_id().with("fg-swatch"), Sense::click())
            .on_hover_text("Primary colour — click to edit");
        swatch(ui.painter(), front, self.fg);
        if fg_resp.clicked() {
            self.dialog = Some(Dialog::colour(false, self.fg));
        } else if bg_resp.clicked() {
            self.dialog = Some(Dialog::colour(true, self.bg));
        }
        ui.vertical(|ui| {
            if ui.small_button("Swap").on_hover_text("Swap colours (X)").clicked() {
                std::mem::swap(&mut self.fg, &mut self.bg);
            }
            if ui.small_button("Reset").on_hover_text("Reset to black and white (D)").clicked() {
                self.fg = Rgba::BLACK;
                self.bg = Rgba::WHITE;
            }
        });
    }

    fn options_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong(self.tool.name());
            ui.separator();
            let s = &mut self.settings;
            match self.tool {
                Tool::Pencil => {
                    ui.label("Size");
                    ui.add(egui::Slider::new(&mut s.pencil_size, 1..=100).logarithmic(true).suffix(" px"));
                }
                Tool::Brush => {
                    ui.label("Size");
                    ui.add(
                        egui::Slider::new(&mut s.brush_size, 1.0..=500.0)
                            .logarithmic(true)
                            .suffix(" px")
                            .max_decimals(1),
                    );
                    ui.checkbox(&mut s.brush_aa, "Antialiasing");
                    ui.weak("[ ] change size");
                }
                Tool::Eraser => {
                    ui.label("Size");
                    ui.add(
                        egui::Slider::new(&mut s.eraser_size, 1.0..=500.0)
                            .logarithmic(true)
                            .suffix(" px")
                            .max_decimals(1),
                    );
                    ui.checkbox(&mut s.eraser_to_secondary, "Erase to secondary colour")
                        .on_hover_text("Off: erased pixels become transparent.\nOn: paint with the secondary colour.");
                }
                Tool::Line | Tool::Rect | Tool::Ellipse | Tool::Polygon => {
                    if self.tool != Tool::Line {
                        ui.selectable_value(&mut s.shape_style, ShapeStyle::Outline, "Outline");
                        ui.selectable_value(&mut s.shape_style, ShapeStyle::Filled, "Filled");
                        ui.selectable_value(&mut s.shape_style, ShapeStyle::OutlineAndFill, "Outline + fill")
                            .on_hover_text("Outline in the primary colour, filled with the secondary colour");
                        ui.separator();
                    }
                    ui.label("Width");
                    ui.add(
                        egui::Slider::new(&mut s.shape_width, 1.0..=100.0)
                            .logarithmic(true)
                            .suffix(" px")
                            .max_decimals(1),
                    );
                    if self.tool != Tool::Rect {
                        ui.checkbox(&mut s.shape_aa, "Antialiasing");
                    }
                }
                Tool::Fill => {
                    ui.label("Tolerance");
                    ui.add(egui::Slider::new(&mut s.fill_tolerance, 0..=100).suffix(" %"))
                        .on_hover_text("How different a colour may be from the clicked pixel and still be filled");
                }
                Tool::Picker => {
                    ui.weak("Left-click: primary colour · Right-click: secondary colour");
                }
                Tool::Select | Tool::Lasso => {
                    let ctx = ui.ctx().clone();
                    if ui.add_enabled(self.can(Cmd::SelectAll), egui::Button::new("Select all")).clicked() {
                        self.run(&ctx, Cmd::SelectAll);
                    }
                    if ui.add_enabled(self.can(Cmd::Crop), egui::Button::new("Crop to selection")).clicked() {
                        self.run(&ctx, Cmd::Crop);
                    }
                    if ui.add_enabled(self.can(Cmd::Delete), egui::Button::new("Erase")).clicked() {
                        self.run(&ctx, Cmd::Delete);
                    }
                    ui.separator();
                    ui.checkbox(&mut self.settings.clip_to_selection, "Paint only inside selection");
                }
                Tool::Text => self.text_options(ui),
            }
        });
    }

    fn text_options(&mut self, ui: &mut egui::Ui) {
        let loaded = self.font_library().is_some();
        let fam_label = if self.settings.text.family.is_empty() {
            "(default)".to_string()
        } else {
            self.settings.text.family.clone()
        };
        if !loaded {
            ui.spinner();
            ui.weak("Loading fonts…");
        } else {
            let mut chosen = None;
            let filter = &mut self.font_filter;
            let families: Vec<String> = match &self.fonts {
                Fonts::Ready(l) => l.families().to_vec(),
                _ => Vec::new(),
            };
            egui::ComboBox::from_id_salt("font-family").selected_text(fam_label).width(180.0).height(320.0).show_ui(
                ui,
                |ui| {
                    ui.add(egui::TextEdit::singleline(filter).hint_text("Search fonts").desired_width(170.0));
                    let f = filter.to_lowercase();
                    for fam in families.iter().filter(|x| f.is_empty() || x.to_lowercase().contains(&f)) {
                        if ui.selectable_label(*fam == self.settings.text.family, fam).clicked() {
                            chosen = Some(fam.clone());
                        }
                    }
                },
            );
            if let Some(c) = chosen {
                self.settings.text.family = c;
            }
        }
        let t = &mut self.settings.text;
        ui.label("Size");
        ui.add(egui::DragValue::new(&mut t.size).range(4.0..=1000.0).speed(0.5).suffix(" px"));
        ui.toggle_value(&mut t.bold, egui::RichText::new("B").strong()).on_hover_text("Bold");
        ui.toggle_value(&mut t.italic, egui::RichText::new("I").italics()).on_hover_text("Italic");
        ui.separator();
        use brushwork::text::Align;
        ui.selectable_value(&mut t.align, Align::Left, "Left");
        ui.selectable_value(&mut t.align, Align::Center, "Centre");
        ui.selectable_value(&mut t.align, Align::Right, "Right");
        ui.checkbox(&mut t.antialias, "Smooth");
        ui.weak("Colour: primary");
    }

    fn palette_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (i, c) in PALETTE.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                swatch(ui.painter(), rect, *c);
                let resp = resp.on_hover_text(format!(
                    "{}\nLeft-click: primary · Right-click: secondary",
                    if c.a == 0 { "Transparent".to_string() } else { c.to_hex() }
                ));
                if resp.clicked() {
                    self.fg = *c;
                }
                if resp.secondary_clicked() {
                    self.bg = *c;
                }
                if i == 13 {
                    ui.add_space(4.0);
                }
            }
            ui.separator();
            hex_field(ui, "Primary", &mut self.fg, &mut self.fg_hex);
            hex_field(ui, "Secondary", &mut self.bg, &mut self.bg_hex);
            if ui.button("Edit colours…").clicked() {
                self.dialog = Some(Dialog::colour(false, self.fg));
            }
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let ppp = ui.ctx().pixels_per_point();
        ui.horizontal(|ui| {
            let fresh = self.status.as_ref().is_some_and(|s| s.error || s.at.elapsed() < Duration::from_secs(8));
            if fresh {
                let s = self.status.as_ref().expect("checked");
                if s.error {
                    ui.colored_label(ui.visuals().error_fg_color, &s.text);
                } else {
                    ui.label(&s.text);
                }
                if !s.error {
                    ui.ctx().request_repaint_after(Duration::from_secs(8));
                }
            } else {
                ui.weak(self.tool.hint());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let Some(t) = self.tabs.get_mut(self.active) else { return };
                let pct = t.view.zoom * 100.0;
                let mut zoom_cmd = None;
                if ui.small_button("+").on_hover_text("Zoom in (Ctrl++)").clicked() {
                    zoom_cmd = Some(Cmd::ZoomIn);
                }
                ui.label(format!("{:>5}%", if pct < 10.0 { format!("{pct:.1}") } else { format!("{pct:.0}") }));
                if ui.small_button("−").on_hover_text("Zoom out (Ctrl+−)").clicked() {
                    zoom_cmd = Some(Cmd::ZoomOut);
                }
                if ui.small_button("Fit").on_hover_text("Fit to window (Ctrl+0)").clicked() {
                    zoom_cmd = Some(Cmd::Fit);
                }
                ui.separator();
                let (w, h) = t.doc.size();
                ui.label(format!("{w} × {h} px")).on_hover_text("Image size");
                if let Some(r) = t.doc.selection_bounds() {
                    ui.separator();
                    ui.label(format!("Selection {} × {}", r.width(), r.height()));
                }
                ui.separator();
                match t.hover_px {
                    Some((x, y)) => ui.label(format!("{x}, {y}")),
                    None => ui.weak("—"),
                }
                .on_hover_text("Cursor position (pixels)");
                if let Some(c) = zoom_cmd {
                    let ctx = ui.ctx().clone();
                    let _ = ppp;
                    self.run(&ctx, c);
                }
            });
        });
    }

    fn tab_bar(&mut self, ui: &mut egui::Ui) {
        let mut close = None;
        let mut select = None;
        egui::ScrollArea::horizontal().id_salt("tabs").show(ui, |ui| {
            ui.horizontal(|ui| {
                for (i, t) in self.tabs.iter().enumerate() {
                    let name = format!("{}{}", t.doc.display_name(), if t.doc.is_dirty() { " •" } else { "" });
                    let r = ui.selectable_label(i == self.active, name);
                    let r = match &t.doc.path {
                        Some(p) => r.on_hover_text(p.display().to_string()),
                        None => r.on_hover_text("Not saved yet"),
                    };
                    if r.clicked() {
                        select = Some(i);
                    }
                    if r.middle_clicked() {
                        close = Some(i);
                    }
                    if ui.small_button("×").on_hover_text("Close (Ctrl+W)").clicked() {
                        close = Some(i);
                    }
                    ui.add_space(6.0);
                }
                if ui.small_button("+").on_hover_text("New image (Ctrl+N)").clicked() {
                    let ctx = ui.ctx().clone();
                    self.run(&ctx, Cmd::New);
                }
            });
        });
        if let Some(i) = select {
            self.active = i;
        }
        if let Some(i) = close {
            self.close_tab(i, false);
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let title = match self.tabs.get(self.active) {
            Some(t) => format!("{}{} — Brushwork", t.doc.display_name(), if t.doc.is_dirty() { " *" } else { "" }),
            None => "Brushwork".to_string(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_close
            && self.tabs.iter().any(|t| t.doc.is_dirty())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.dialog = Some(Dialog::ConfirmQuit);
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let paths: Vec<PathBuf> = ctx.input(|i| {
            i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect()
        });
        if !paths.is_empty() {
            self.open_paths(&paths);
        }
    }
}

fn swatch(p: &egui::Painter, rect: Rect, c: Rgba) {
    if c.a < 255 {
        let half = rect.width() / 2.0;
        p.rect_filled(rect, 0.0, Color32::WHITE);
        p.rect_filled(Rect::from_min_size(rect.min, Vec2::splat(half)), 0.0, Color32::from_gray(200));
        p.rect_filled(
            Rect::from_min_size(rect.min + Vec2::splat(half), Vec2::splat(half)),
            0.0,
            Color32::from_gray(200),
        );
    }
    p.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a));
    p.rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::from_gray(110)), egui::StrokeKind::Inside);
}

/// Hex entry that applies the colour once it parses.
fn hex_field(ui: &mut egui::Ui, label: &str, c: &mut Rgba, buf: &mut String) {
    let id = ui.scope_id().with(label);
    let focused = ui.memory(|m| m.has_focus(id));
    if !focused {
        *buf = c.to_hex();
    }
    ui.label(label);
    let r = ui
        .add(egui::TextEdit::singleline(buf).id(id).desired_width(80.0).font(egui::TextStyle::Monospace))
        .on_hover_text("Hex colour: #RRGGBB or #RRGGBBAA");
    if r.changed()
        && let Some(n) = Rgba::from_hex(buf)
    {
        *c = n;
    }
}

impl eframe::App for BrushworkApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.checker.is_none() {
            self.checker = Some(canvas::checker_texture(&ctx));
        }
        self.font_library();
        self.handle_close_request(&ctx);
        self.handle_dropped_files(&ctx);
        self.handle_keys(&ctx);

        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::top("options")
            .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(8, 4)))
            .show(ui, |ui| self.options_bar(ui));
        egui::Panel::bottom("status")
            .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(8, 3)))
            .show(ui, |ui| self.status_bar(ui));
        egui::Panel::bottom("palette")
            .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(8, 4)))
            .show(ui, |ui| self.palette_bar(ui));
        egui::Panel::left("tools").resizable(false).exact_size(80.0).show(ui, |ui| self.tool_panel(ui));

        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                self.tab_bar(ui)
            });
            let bg = if ui.visuals().dark_mode { Color32::from_gray(38) } else { Color32::from_gray(160) };
            ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, bg);
            if self.tabs.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() / 3.0);
                    ui.heading("No image open");
                    ui.add_space(8.0);
                    let ctx = ui.ctx().clone();
                    if ui.button("New image…").clicked() {
                        self.run(&ctx, Cmd::New);
                    }
                    if ui.button("Open image…").clicked() {
                        self.run(&ctx, Cmd::Open);
                    }
                    ui.weak("You can also drop image files here.");
                });
                return;
            }
            let space_down = ctx.input(|i| i.key_down(Key::Space)) && !ctx.egui_wants_keyboard_input();
            let checker = self.checker.clone().expect("created above");
            let fonts = match &mut self.fonts {
                Fonts::Ready(l) => Some(&mut **l),
                _ => None,
            };
            let tab = &mut self.tabs[self.active];
            let mut cx = CanvasCtx {
                tool: &mut self.tool,
                prev_tool: self.prev_tool,
                settings: &self.settings,
                fg: &mut self.fg,
                bg: &mut self.bg,
                fonts,
                checker: &checker,
                space_down,
            };
            tab.show(ui, &mut cx);
        });

        // Text editing panel.
        if self.tool == Tool::Text {
            let style = self.settings.text.clone();
            let fg = self.fg;
            if let Some(t) = self.tabs.get_mut(self.active) {
                match t.text_editor(&ctx) {
                    TextEditorAction::Commit => {
                        let fonts = match &mut self.fonts {
                            Fonts::Ready(l) => Some(&mut **l),
                            _ => None,
                        };
                        t.commit_text(fonts, &style, fg);
                    }
                    TextEditorAction::Cancel => t.cancel_text(),
                    TextEditorAction::None => {}
                }
            }
        }

        self.show_dialog(&ctx);
        self.update_title(&ctx);

        if let Some(t0) = self.startup_probe.take() {
            // First frame is ready; report and exit (used for startup measurements).
            eprintln!("startup-probe: first frame after {:.1} ms", t0.elapsed().as_secs_f64() * 1000.0);
            self.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let _ = pos2;
    }
}
