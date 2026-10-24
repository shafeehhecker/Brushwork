//! Modal dialogs.

use super::{BrushworkApp, SHORTCUTS};
use brushwork::color::Rgba;
use brushwork::raster::{self, Raster};
use brushwork::transform::{self, ResizeFilter};
use eframe::egui::{self, Color32};

pub enum Dialog {
    NewImage { w: u32, h: u32, transparent: bool },
    Resize { orig: (u32, u32), w: u32, h: u32, percent: f32, by_percent: bool, keep_aspect: bool, filter: ResizeFilter },
    Colour { secondary: bool, color: Color32, hex: String },
    PasteLarger { img: Raster },
    ConfirmClose { tab: usize },
    ConfirmQuit,
    Message { title: String, text: String },
    Shortcuts,
    About,
}

impl Dialog {
    pub fn new_image(w: u32, h: u32) -> Dialog {
        Dialog::NewImage { w, h, transparent: false }
    }
    pub fn resize(w: u32, h: u32) -> Dialog {
        Dialog::Resize {
            orig: (w, h),
            w,
            h,
            percent: 100.0,
            by_percent: true,
            keep_aspect: true,
            filter: ResizeFilter::Lanczos,
        }
    }
    pub fn colour(secondary: bool, c: Rgba) -> Dialog {
        Dialog::Colour { secondary, color: Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a), hex: c.to_hex() }
    }
}

/// Work to run on the app after the dialog UI has been drawn.
type Deferred = Box<dyn FnOnce(&mut BrushworkApp)>;

enum Outcome {
    Keep,
    Close,
}

fn modal(ctx: &egui::Context, id: &str, title: &str, add: impl FnOnce(&mut egui::Ui) -> Outcome) -> Outcome {
    let mut out = Outcome::Keep;
    let resp = egui::Modal::new(egui::Id::unique(id)).show(ctx, |ui| {
        ui.set_max_width(460.0);
        ui.heading(title);
        ui.add_space(6.0);
        out = add(ui);
    });
    if resp.should_close() {
        // Esc or click outside
        return Outcome::Close;
    }
    out
}

fn size_error(w: u32, h: u32) -> Option<String> {
    raster::validate_dimensions(w as u64, h as u64).err().map(|e| e.to_string())
}

fn to_rgba(c: Color32) -> Rgba {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    Rgba::new(r, g, b, a)
}

impl BrushworkApp {
    pub(super) fn show_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.take() else { return };
        let mut after: Vec<Deferred> = Vec::new();
        let outcome = match &mut dialog {
            Dialog::NewImage { w, h, transparent } => modal(ctx, "dlg-new", "New image", |ui| {
                egui::Grid::new("new-grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    ui.label("Width");
                    ui.add(egui::DragValue::new(w).range(1..=raster::MAX_DIMENSION).suffix(" px"));
                    ui.end_row();
                    ui.label("Height");
                    ui.add(egui::DragValue::new(h).range(1..=raster::MAX_DIMENSION).suffix(" px"));
                    ui.end_row();
                    ui.label("Background");
                    ui.horizontal(|ui| {
                        ui.radio_value(transparent, false, "White");
                        ui.radio_value(transparent, true, "Transparent");
                    });
                    ui.end_row();
                });
                ui.horizontal(|ui| {
                    for (pw, ph) in [(800, 600), (1280, 720), (1920, 1080), (3840, 2160)] {
                        if ui.small_button(format!("{pw}×{ph}")).clicked() {
                            *w = pw;
                            *h = ph;
                        }
                    }
                });
                let err = size_error(*w, *h);
                if let Some(e) = &err {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                ui.add_space(6.0);
                let mut out = Outcome::Keep;
                ui.horizontal(|ui| {
                    if ui.add_enabled(err.is_none(), egui::Button::new("Create")).clicked()
                        || (err.is_none() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                    {
                        let (w, h, t) = (*w, *h, *transparent);
                        after.push(Box::new(move |app| {
                            app.new_document(w, h, if t { Rgba::TRANSPARENT } else { Rgba::WHITE })
                        }));
                        out = Outcome::Close;
                    }
                    if ui.button("Cancel").clicked() {
                        out = Outcome::Close;
                    }
                });
                out
            }),
            Dialog::Resize { orig, w, h, percent, by_percent, keep_aspect, filter } => {
                modal(ctx, "dlg-resize", "Resize image", |ui| {
                    ui.label(format!("Current size: {} × {} px", orig.0, orig.1));
                    ui.horizontal(|ui| {
                        ui.radio_value(by_percent, true, "By percentage");
                        ui.radio_value(by_percent, false, "By pixels");
                    });
                    let aspect = orig.0 as f64 / orig.1 as f64;
                    egui::Grid::new("resize-grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                        if *by_percent {
                            ui.label("Percentage");
                            ui.add(egui::DragValue::new(percent).range(1.0..=1000.0).speed(1.0).suffix(" %"));
                            {
                                let (nw, nh) = transform::scaled_size(orig.0, orig.1, *percent);
                                *w = nw;
                                *h = nh;
                            }
                            ui.end_row();
                        } else {
                            ui.label("Width");
                            if ui.add(egui::DragValue::new(w).range(1..=raster::MAX_DIMENSION).suffix(" px")).changed()
                                && *keep_aspect
                            {
                                *h = ((*w as f64 / aspect).round() as u32).max(1);
                            }
                            ui.end_row();
                            ui.label("Height");
                            if ui.add(egui::DragValue::new(h).range(1..=raster::MAX_DIMENSION).suffix(" px")).changed()
                                && *keep_aspect
                            {
                                *w = ((*h as f64 * aspect).round() as u32).max(1);
                            }
                            ui.end_row();
                            ui.label("");
                            ui.checkbox(keep_aspect, "Keep aspect ratio");
                            ui.end_row();
                        }
                        ui.label("Resampling");
                        egui::ComboBox::from_id_salt("filter")
                            .selected_text(match filter {
                                ResizeFilter::Lanczos => "Best quality (Lanczos)",
                                ResizeFilter::Bilinear => "Bilinear",
                                ResizeFilter::Nearest => "Nearest neighbour (pixel art)",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(filter, ResizeFilter::Lanczos, "Best quality (Lanczos)");
                                ui.selectable_value(filter, ResizeFilter::Bilinear, "Bilinear");
                                ui.selectable_value(filter, ResizeFilter::Nearest, "Nearest neighbour (pixel art)");
                            });
                        ui.end_row();
                    });
                    ui.label(format!("New size: {} × {} px", w, h));
                    let err = size_error(*w, *h);
                    if let Some(e) = &err {
                        ui.colored_label(ui.visuals().error_fg_color, e);
                    }
                    let mut out = Outcome::Keep;
                    ui.horizontal(|ui| {
                        if ui.add_enabled(err.is_none(), egui::Button::new("Resize")).clicked() {
                            let (w, h, f) = (*w, *h, *filter);
                            after.push(Box::new(move |app| {
                                if let Some(t) = app.tab() {
                                    match t.doc.resize(w, h, f) {
                                        Ok(d) => {
                                            t.mark(d);
                                            t.fit();
                                        }
                                        Err(e) => {
                                            let msg = e.to_string();
                                            app.message("Cannot resize", msg);
                                        }
                                    }
                                }
                            }));
                            out = Outcome::Close;
                        }
                        if ui.button("Cancel").clicked() {
                            out = Outcome::Close;
                        }
                    });
                    out
                })
            }
            Dialog::Colour { secondary, color, hex } => modal(ctx, "dlg-colour", "Edit colour", |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(secondary, false, "Primary");
                    ui.selectable_value(secondary, true, "Secondary");
                });
                let mut changed = false;
                if egui::color_picker::color_picker_color32(ui, color, egui::color_picker::Alpha::OnlyBlend) {
                    changed = true;
                }
                let mut c = to_rgba(*color);
                ui.horizontal(|ui| {
                    for (label, v) in [("R", &mut c.r), ("G", &mut c.g), ("B", &mut c.b), ("A", &mut c.a)] {
                        ui.label(label);
                        if ui.add(egui::DragValue::new(v).range(0..=255)).changed() {
                            changed = true;
                        }
                    }
                });
                if changed {
                    *color = Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a);
                    *hex = c.to_hex();
                }
                ui.horizontal(|ui| {
                    ui.label("Hex");
                    if ui
                        .add(egui::TextEdit::singleline(hex).desired_width(100.0).font(egui::TextStyle::Monospace))
                        .changed()
                        && let Some(n) = Rgba::from_hex(hex)
                    {
                        *color = Color32::from_rgba_unmultiplied(n.r, n.g, n.b, n.a);
                    }
                });
                let mut out = Outcome::Keep;
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        let (s, c) = (*secondary, to_rgba(*color));
                        after.push(Box::new(move |app| if s { app.bg = c } else { app.fg = c }));
                        out = Outcome::Close;
                    }
                    if ui.button("Cancel").clicked() {
                        out = Outcome::Close;
                    }
                });
                out
            }),
            Dialog::PasteLarger { img } => {
                let (iw, ih) = (img.width(), img.height());
                let mut choice = 0;
                let o = modal(ctx, "dlg-paste", "Pasted image is larger than the canvas", |ui| {
                    ui.label(format!("The clipboard image is {iw} × {ih} px. Enlarge the canvas to fit it?"));
                    ui.add_space(6.0);
                    let mut out = Outcome::Keep;
                    ui.horizontal(|ui| {
                        if ui.button("Enlarge canvas").clicked() {
                            choice = 1;
                            out = Outcome::Close;
                        }
                        if ui.button("Keep canvas size").clicked() {
                            choice = 2;
                            out = Outcome::Close;
                        }
                        if ui.button("Cancel").clicked() {
                            out = Outcome::Close;
                        }
                    });
                    out
                });
                if choice != 0 {
                    let img = std::mem::replace(img, Raster::new(1, 1, Rgba::TRANSPARENT).expect("1×1"));
                    after.push(Box::new(move |app| {
                        if choice == 1
                            && let Some(t) = app.tab()
                        {
                            let fill = if t.doc.image.is_opaque() { Rgba::WHITE } else { Rgba::TRANSPARENT };
                            match t.doc.expand_canvas(iw, ih, fill) {
                                Ok(d) => {
                                    t.mark(d);
                                    t.fit();
                                }
                                Err(e) => {
                                    let m = e.to_string();
                                    app.message("Cannot enlarge canvas", m);
                                    return;
                                }
                            }
                        }
                        app.finish_paste(img, 0, 0);
                    }));
                }
                o
            }
            Dialog::ConfirmClose { tab } => {
                let idx = *tab;
                let name = self.tabs.get(idx).map(|t| t.doc.display_name()).unwrap_or_default();
                modal(ctx, "dlg-close", "Save changes?", |ui| {
                    ui.label(format!("Do you want to save changes to “{name}” before closing?"));
                    ui.add_space(6.0);
                    let mut out = Outcome::Keep;
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            after.push(Box::new(move |app| {
                                if app.save_tab(idx, false) {
                                    app.close_tab(idx, true);
                                }
                            }));
                            out = Outcome::Close;
                        }
                        if ui.button("Don't save").clicked() {
                            after.push(Box::new(move |app| app.close_tab(idx, true)));
                            out = Outcome::Close;
                        }
                        if ui.button("Cancel").clicked() {
                            out = Outcome::Close;
                        }
                    });
                    out
                })
            }
            Dialog::ConfirmQuit => {
                let names: Vec<String> =
                    self.tabs.iter().filter(|t| t.doc.is_dirty()).map(|t| t.doc.display_name()).collect();
                let ctx2 = ctx.clone();
                modal(ctx, "dlg-quit", "Unsaved changes", |ui| {
                    ui.label("These images have unsaved changes:");
                    for n in &names {
                        ui.label(format!("  • {n}"));
                    }
                    ui.add_space(6.0);
                    let mut out = Outcome::Keep;
                    ui.horizontal(|ui| {
                        if ui.button("Save all").clicked() {
                            let c = ctx2.clone();
                            after.push(Box::new(move |app| {
                                for i in 0..app.tabs.len() {
                                    if app.tabs[i].doc.is_dirty() && !app.save_tab(i, false) {
                                        return; // cancelled or failed: stay open
                                    }
                                }
                                app.allow_close = true;
                                c.send_viewport_cmd(egui::ViewportCommand::Close);
                            }));
                            out = Outcome::Close;
                        }
                        if ui.button("Quit without saving").clicked() {
                            let c = ctx2.clone();
                            after.push(Box::new(move |app| {
                                app.allow_close = true;
                                c.send_viewport_cmd(egui::ViewportCommand::Close);
                            }));
                            out = Outcome::Close;
                        }
                        if ui.button("Cancel").clicked() {
                            out = Outcome::Close;
                        }
                    });
                    out
                })
            }
            Dialog::Message { title, text } => {
                let title = title.clone();
                modal(ctx, "dlg-msg", &title, |ui| {
                    ui.label(text.as_str());
                    ui.add_space(6.0);
                    if ui.button("OK").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        Outcome::Close
                    } else {
                        Outcome::Keep
                    }
                })
            }
            Dialog::Shortcuts => modal(ctx, "dlg-keys", "Keyboard shortcuts", |ui| {
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    egui::Grid::new("keys").num_columns(2).striped(true).show(ui, |ui| {
                        let mut seen = std::collections::HashSet::new();
                        for (cmd, sc) in SHORTCUTS {
                            if seen.insert(format!("{cmd:?}")) {
                                ui.label(format!("{cmd:?}"));
                                ui.monospace(ctx.format_shortcut(sc));
                                ui.end_row();
                            }
                        }
                        for (k, d) in [
                            ("Ctrl+X / C / V", "Cut / copy / paste"),
                            ("Delete", "Erase selection"),
                            ("Esc", "Cancel / deselect"),
                            ("Enter", "Apply pasted or moved pixels, finish polygon"),
                            ("Arrows (Shift)", "Nudge selection 1 (10) px"),
                            ("X / D", "Swap / reset colours"),
                            ("[ / ]", "Smaller / larger tool size"),
                            ("Space + drag, middle drag", "Pan"),
                            ("Wheel / Shift+wheel", "Scroll"),
                            ("Ctrl+wheel", "Zoom at cursor"),
                        ] {
                            ui.label(d);
                            ui.monospace(k);
                            ui.end_row();
                        }
                        for t in super::tools::Tool::ALL {
                            ui.label(t.name());
                            ui.monospace(t.key().name());
                            ui.end_row();
                        }
                    });
                });
                if ui.button("Close").clicked() { Outcome::Close } else { Outcome::Keep }
            }),
            Dialog::About => modal(ctx, "dlg-about", "Brushwork", |ui| {
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.label("A lightweight drawing and image editor.");
                ui.label("Free and open source (MIT licence). Works fully offline; no accounts, no telemetry.");
                ui.add_space(6.0);
                if ui.button("Close").clicked() { Outcome::Close } else { Outcome::Keep }
            }),
        };
        if matches!(outcome, Outcome::Keep) {
            self.dialog = Some(dialog);
        }
        for f in after {
            f(self);
        }
    }
}
