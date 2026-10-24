//! A document tab: display textures, view, and pointer interaction for every tool.

use super::tools::{Settings, ShapeStyle, Tool};
use brushwork::color::Rgba;
use brushwork::document::Document;
use brushwork::geom::IRect;
use brushwork::history::Dirty;
use brushwork::paint::{Blend, PaintOp};
use brushwork::raster::Raster;
use brushwork::selection::Mask;
use brushwork::text::{self, FontLibrary, TextRaster, TextStyle};
use brushwork::view::View;
use brushwork::{fill, shapes};
use eframe::egui::{
    self, Color32, ColorImage, CursorIcon, Event, PointerButton, Pos2, Rect, Sense, Shape, Stroke, TextureHandle,
    TextureOptions, Ui, Vec2, pos2, vec2,
};

const TEX_TILE: u32 = 512;

fn region_image(img: &Raster, r: IRect) -> ColorImage {
    let mut bytes = Vec::with_capacity(r.area() as usize * 4);
    for y in r.y0..r.y1 {
        bytes.extend_from_slice(img.row_span(y as u32, r.x0 as u32, r.x1 as u32));
    }
    ColorImage::from_rgba_unmultiplied([r.width() as usize, r.height() as usize], &bytes)
}

fn canvas_options(crisp: bool) -> TextureOptions {
    TextureOptions {
        magnification: if crisp { egui::TextureFilter::Nearest } else { egui::TextureFilter::Linear },
        minification: egui::TextureFilter::Linear,
        ..TextureOptions::default()
    }
}

fn pixel_options() -> TextureOptions {
    TextureOptions {
        magnification: egui::TextureFilter::Nearest,
        minification: egui::TextureFilter::Linear,
        ..TextureOptions::default()
    }
}

/// The image split into GPU textures (≤512×512) so updates and size limits stay small.
struct CanvasTextures {
    tiles: Vec<TextureHandle>,
    cols: u32,
    size: (u32, u32),
    /// Nearest-neighbour magnification (zoomed in far enough to see pixels).
    crisp: bool,
}

impl CanvasTextures {
    fn build(ctx: &egui::Context, img: &Raster, crisp: bool) -> Self {
        let (w, h) = (img.width(), img.height());
        let cols = w.div_ceil(TEX_TILE);
        let rows = h.div_ceil(TEX_TILE);
        let mut tiles = Vec::with_capacity((cols * rows) as usize);
        for ty in 0..rows {
            for tx in 0..cols {
                let r = Self::tile_rect((w, h), tx, ty);
                tiles.push(ctx.load_texture(format!("canvas-{tx}-{ty}"), region_image(img, r), canvas_options(crisp)));
            }
        }
        CanvasTextures { tiles, cols, size: (w, h), crisp }
    }

    fn tile_rect(size: (u32, u32), tx: u32, ty: u32) -> IRect {
        let (x0, y0) = (tx * TEX_TILE, ty * TEX_TILE);
        IRect::new(x0 as i32, y0 as i32, (x0 + TEX_TILE).min(size.0) as i32, (y0 + TEX_TILE).min(size.1) as i32)
    }

    fn update(&mut self, img: &Raster, r: IRect) {
        let r = r.intersect(&img.rect());
        if r.is_empty() {
            return;
        }
        let (tx0, ty0) = (r.x0 as u32 / TEX_TILE, r.y0 as u32 / TEX_TILE);
        let (tx1, ty1) = ((r.x1 as u32 - 1) / TEX_TILE, (r.y1 as u32 - 1) / TEX_TILE);
        for ty in ty0..=ty1 {
            for tx in tx0..=tx1 {
                let tr = Self::tile_rect(self.size, tx, ty);
                let sub = tr.intersect(&r);
                let tex = &mut self.tiles[(ty * self.cols + tx) as usize];
                tex.set_partial(
                    [(sub.x0 - tr.x0) as usize, (sub.y0 - tr.y0) as usize],
                    region_image(img, sub),
                    canvas_options(self.crisp),
                );
            }
        }
    }

    fn paint(&self, painter: &egui::Painter, to_screen: impl Fn(f32, f32) -> Pos2, clip: Rect) {
        let rows = self.size.1.div_ceil(TEX_TILE);
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        for ty in 0..rows {
            for tx in 0..self.cols {
                let r = Self::tile_rect(self.size, tx, ty);
                let sr = Rect::from_min_max(to_screen(r.x0 as f32, r.y0 as f32), to_screen(r.x1 as f32, r.y1 as f32));
                if sr.intersects(clip) {
                    painter.image(self.tiles[(ty * self.cols + tx) as usize].id(), sr, uv, Color32::WHITE);
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    N,
    S,
    E,
    W,
    NE,
    NW,
    SE,
    SW,
}

impl Handle {
    const ALL: [Handle; 8] =
        [Handle::NW, Handle::N, Handle::NE, Handle::E, Handle::SE, Handle::S, Handle::SW, Handle::W];
    fn pos(self, r: Rect) -> Pos2 {
        let (x0, xm, x1) = (r.min.x, r.center().x, r.max.x);
        let (y0, ym, y1) = (r.min.y, r.center().y, r.max.y);
        match self {
            Handle::NW => pos2(x0, y0),
            Handle::N => pos2(xm, y0),
            Handle::NE => pos2(x1, y0),
            Handle::E => pos2(x1, ym),
            Handle::SE => pos2(x1, y1),
            Handle::S => pos2(xm, y1),
            Handle::SW => pos2(x0, y1),
            Handle::W => pos2(x0, ym),
        }
    }
    fn cursor(self) -> CursorIcon {
        match self {
            Handle::N | Handle::S => CursorIcon::ResizeVertical,
            Handle::E | Handle::W => CursorIcon::ResizeHorizontal,
            Handle::NE | Handle::SW => CursorIcon::ResizeNeSw,
            Handle::NW | Handle::SE => CursorIcon::ResizeNwSe,
        }
    }
    /// New rectangle after dragging this handle by (dx, dy) image pixels.
    fn apply(self, r: IRect, dx: i32, dy: i32, keep_aspect: bool) -> IRect {
        let (mut x0, mut y0, mut x1, mut y1) = (r.x0, r.y0, r.x1, r.y1);
        match self {
            Handle::N => y0 += dy,
            Handle::S => y1 += dy,
            Handle::E => x1 += dx,
            Handle::W => x0 += dx,
            Handle::NE => {
                x1 += dx;
                y0 += dy
            }
            Handle::NW => {
                x0 += dx;
                y0 += dy
            }
            Handle::SE => {
                x1 += dx;
                y1 += dy
            }
            Handle::SW => {
                x0 += dx;
                y1 += dy
            }
        }
        x0 = x0.min(x1 - 1);
        y0 = y0.min(y1 - 1);
        if keep_aspect
            && matches!(self, Handle::NE | Handle::NW | Handle::SE | Handle::SW)
            && r.width() > 0
            && r.height() > 0
        {
            let aspect = r.width() as f32 / r.height() as f32;
            let w = (x1 - x0) as f32;
            let h = ((w / aspect).round() as i32).max(1);
            match self {
                Handle::NE | Handle::NW => y0 = y1 - h,
                _ => y1 = y0 + h,
            }
        }
        IRect::new(x0, y0, x1.max(x0 + 1), y1.max(y0 + 1))
    }
}

pub enum Interaction {
    Idle,
    Paint { op: PaintOp, last: (f32, f32), label: &'static str },
    Shape { op: PaintOp, start: (f32, f32), tool: Tool },
    Polygon { op: PaintOp, points: Vec<(f32, f32)>, secondary: bool, held: bool },
    Select { start: (f32, f32), cur: (f32, f32) },
    Lasso { points: Vec<(f32, f32)> },
    MoveFloat { start: (f32, f32), orig: IRect },
    ResizeFloat { handle: Handle, start: (f32, f32), orig: IRect },
    MoveText { start: (f32, f32), orig: (i32, i32) },
    Pan,
}

pub struct TextSession {
    pub pos: (i32, i32),
    pub content: String,
    pub request_focus: bool,
    cache: Option<(String, TextStyle, Rgba, TextRaster, Option<TextureHandle>)>,
}

impl TextSession {
    fn screen_rect(&self, to_screen: &impl Fn(f32, f32) -> Pos2) -> Option<Rect> {
        let (_, _, _, r, _) = self.cache.as_ref()?;
        let x0 = (self.pos.0 + r.origin.0) as f32;
        let y0 = (self.pos.1 + r.origin.1) as f32;
        Some(Rect::from_min_max(to_screen(x0, y0), to_screen(x0 + r.width as f32, y0 + r.height as f32)))
    }
}

pub struct Tab {
    pub doc: Document,
    pub view: View,
    pub needs_fit: bool,
    tex: Option<CanvasTextures>,
    pending: Option<Dirty>,
    float_tex: Option<(u64, TextureHandle)>,
    pub interaction: Interaction,
    pub text: Option<TextSession>,
    pub hover_px: Option<(i32, i32)>,
    pub canvas_rect: Rect,
    last_point: Option<(f32, f32)>,
}

/// Shared state the canvas needs from the application.
pub struct CanvasCtx<'a> {
    pub tool: &'a mut Tool,
    pub prev_tool: Tool,
    pub settings: &'a Settings,
    pub fg: &'a mut Rgba,
    pub bg: &'a mut Rgba,
    pub fonts: Option<&'a mut FontLibrary>,
    pub checker: &'a TextureHandle,
    pub space_down: bool,
}

impl Tab {
    pub fn new(doc: Document) -> Tab {
        Tab {
            doc,
            view: View::default(),
            needs_fit: true,
            tex: None,
            pending: Some(Dirty::Full),
            float_tex: None,
            interaction: Interaction::Idle,
            text: None,
            hover_px: None,
            canvas_rect: Rect::NOTHING,
            last_point: None,
        }
    }

    /// Schedules a display refresh.
    pub fn mark(&mut self, d: Dirty) {
        self.pending = match (self.pending, d) {
            (Some(Dirty::Full), _) | (_, Dirty::Full) => Some(Dirty::Full),
            (Some(Dirty::Rect(a)), Dirty::Rect(b)) => Some(Dirty::Rect(a.union(&b))),
            (None, d) => Some(d),
        };
        if d == Dirty::Full {
            self.last_point = None;
        }
    }

    pub fn mark_opt(&mut self, d: Option<Dirty>) {
        if let Some(d) = d {
            self.mark(d);
        }
    }

    pub fn is_busy(&self) -> bool {
        !matches!(self.interaction, Interaction::Idle | Interaction::Pan)
    }

    /// Finishes or abandons any in-progress pointer operation.
    pub fn cancel_interaction(&mut self) {
        match std::mem::replace(&mut self.interaction, Interaction::Idle) {
            Interaction::Paint { op, .. } | Interaction::Shape { op, .. } | Interaction::Polygon { op, .. } => {
                let r = op.cancel(&mut self.doc.image);
                self.mark(Dirty::Rect(r));
            }
            _ => {}
        }
    }

    pub fn fit(&mut self) {
        self.needs_fit = true;
    }

    pub fn zoom_to(&mut self, zoom: f32, ppp: f32) {
        let c = self.canvas_rect.size() / 2.0;
        self.view.zoom_about(zoom, (c.x, c.y), ppp);
    }

    pub fn actual_size(&mut self, ppp: f32) {
        let s = self.canvas_rect.size();
        self.view = View::centered(1.0, self.doc.size(), (s.x, s.y), ppp);
    }

    /// Applies the floating selection to the image.
    pub fn commit_floating(&mut self) {
        let d = self.doc.commit_floating();
        self.mark_opt(d);
    }

    // ---- text ------------------------------------------------------------

    pub fn start_text(&mut self, pos: (i32, i32)) {
        self.text = Some(TextSession { pos, content: String::new(), request_focus: true, cache: None });
    }

    pub fn cancel_text(&mut self) {
        self.text = None;
    }

    /// Paints the text into the image as one undoable step.
    pub fn commit_text(&mut self, fonts: Option<&mut FontLibrary>, style: &TextStyle, color: Rgba) {
        let Some(t) = self.text.take() else { return };
        if t.content.trim().is_empty() {
            return;
        }
        let Some(fonts) = fonts else { return };
        let Some(face) = fonts.resolve(&style.family, style.bold, style.italic) else { return };
        let r = text::render(&face, &t.content, style);
        self.commit_floating();
        let (ox, oy) = (t.pos.0 + r.origin.0, t.pos.1 + r.origin.1);
        let mut op = PaintOp::new(&[Blend::Over(color)]);
        let w = r.width as i32;
        op.add_coverage(&mut self.doc.image, 0, IRect::from_xywh(ox, oy, w, r.height as i32), |y, x0, buf| {
            let ry = (y - oy) as usize;
            let rx = (x0 - ox) as usize;
            buf.copy_from_slice(&r.coverage[ry * w as usize + rx..][..buf.len()]);
        });
        if let Some(d) = op.take_dirty() {
            self.mark(Dirty::Rect(d));
        }
        self.doc.commit_paint(op, "Text");
    }

    fn refresh_text_preview(
        &mut self,
        ctx: &egui::Context,
        fonts: Option<&mut FontLibrary>,
        style: &TextStyle,
        color: Rgba,
    ) {
        let Some(t) = &mut self.text else { return };
        let fresh = matches!(&t.cache, Some((c, s, col, _, _)) if *c == t.content && s == style && *col == color);
        if fresh {
            return;
        }
        let Some(fonts) = fonts else { return };
        let Some(face) = fonts.resolve(&style.family, style.bold, style.italic) else { return };
        // Render a placeholder caret-sized box while empty so the text location stays visible.
        let shown = if t.content.is_empty() { " " } else { t.content.as_str() };
        let r = text::render(&face, shown, style);
        let tex = (r.width > 0 && r.height > 0).then(|| {
            let mut px = Vec::with_capacity(r.coverage.len() * 4);
            for &c in &r.coverage {
                let a = ((color.a as u32 * c as u32 + 127) / 255) as u8;
                px.extend_from_slice(&[color.r, color.g, color.b, a]);
            }
            let img = ColorImage::from_rgba_unmultiplied([r.width as usize, r.height as usize], &px);
            ctx.load_texture("text-preview", img, TextureOptions::LINEAR)
        });
        t.cache = Some((t.content.clone(), style.clone(), color, r, tex));
    }

    // ---- display ---------------------------------------------------------

    fn sync_textures(&mut self, ctx: &egui::Context) {
        let size = self.doc.size();
        // Smooth below 200% (avoids uneven pixel doubling), crisp pixels when zoomed further in.
        let crisp = self.view.zoom >= 2.0 || (self.view.zoom - 1.0).abs() < 1e-3;
        let rebuild =
            self.tex.as_ref().is_none_or(|t| t.size != size || t.crisp != crisp) || self.pending == Some(Dirty::Full);
        if rebuild {
            self.tex = Some(CanvasTextures::build(ctx, &self.doc.image, crisp));
        } else if let (Some(Dirty::Rect(r)), Some(t)) = (self.pending, &mut self.tex) {
            t.update(&self.doc.image, r);
        }
        self.pending = None;
        match &self.doc.floating {
            Some(f) => {
                if self.float_tex.as_ref().is_none_or(|(rev, _)| *rev != f.revision) {
                    let img = region_image(&f.pixels, f.pixels.rect());
                    self.float_tex = Some((f.revision, ctx.load_texture("floating", img, pixel_options())));
                }
            }
            None => self.float_tex = None,
        }
    }

    /// Lays out the canvas, handles input and paints everything.
    pub fn show(&mut self, ui: &mut Ui, cx: &mut CanvasCtx) {
        let rect = ui.available_rect_before_wrap();
        self.canvas_rect = rect;
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        let ctx = ui.ctx().clone();
        let ppp = ctx.pixels_per_point();
        let size = self.doc.size();

        if self.needs_fit && rect.width() > 1.0 {
            self.view = View::fit(size, (rect.width(), rect.height()), ppp, false);
            self.needs_fit = false;
        }

        self.handle_view_input(&ctx, &response, rect, ppp);
        self.handle_tool_input(&ctx, &response, rect, ppp, cx);

        // Live operations report what they changed.
        let live = match &mut self.interaction {
            Interaction::Paint { op, .. } | Interaction::Shape { op, .. } | Interaction::Polygon { op, .. } => {
                op.take_dirty()
            }
            _ => None,
        };
        if let Some(d) = live {
            self.mark(Dirty::Rect(d));
        }

        if *cx.tool == Tool::Text {
            let style = cx.settings.text.clone();
            self.refresh_text_preview(&ctx, cx.fonts.as_deref_mut(), &style, *cx.fg);
        }
        self.sync_textures(&ctx);
        self.view.clamp_pan(self.doc.size(), (rect.width(), rect.height()), ppp);
        self.paint(ui, rect, ppp, cx);
    }

    fn to_screen_fn(&self, rect: Rect, ppp: f32) -> impl Fn(f32, f32) -> Pos2 + use<> {
        let v = self.view;
        let o = rect.min;
        move |x, y| {
            let p = v.to_screen((x, y), ppp);
            pos2(o.x + p.0, o.y + p.1)
        }
    }

    fn to_image(&self, p: Pos2, rect: Rect, ppp: f32) -> (f32, f32) {
        self.view.to_image((p.x - rect.min.x, p.y - rect.min.y), ppp)
    }

    fn handle_view_input(&mut self, ctx: &egui::Context, response: &egui::Response, rect: Rect, ppp: f32) {
        let (zoom_delta, scroll, hover, middle_down, delta) = ctx.input(|i| {
            (i.zoom_delta(), i.smooth_scroll_delta, i.pointer.hover_pos(), i.pointer.middle_down(), i.pointer.delta())
        });
        if response.hovered() {
            if zoom_delta != 1.0 {
                if let Some(p) = hover {
                    let a = p - rect.min;
                    self.view.zoom_about(self.view.zoom * zoom_delta, (a.x, a.y), ppp);
                }
            } else if scroll != Vec2::ZERO {
                self.view.offset.0 += scroll.x;
                self.view.offset.1 += scroll.y;
            }
        }
        if middle_down && (response.hovered() || matches!(self.interaction, Interaction::Pan)) {
            if matches!(self.interaction, Interaction::Idle | Interaction::Pan) {
                self.interaction = Interaction::Pan;
                self.view.offset.0 += delta.x;
                self.view.offset.1 += delta.y;
                ctx.set_cursor_icon(CursorIcon::Grabbing);
            }
        } else if matches!(self.interaction, Interaction::Pan) && !ctx.input(|i| i.pointer.primary_down()) {
            self.interaction = Interaction::Idle;
        }
    }

    fn float_screen_rect(&self, rect: Rect, ppp: f32) -> Option<Rect> {
        let f =
            self.doc.floating.as_ref().map(|f| f.rect).or_else(|| self.doc.selection.as_ref().map(|m| m.bounds()))?;
        let ts = self.to_screen_fn(rect, ppp);
        Some(Rect::from_min_max(ts(f.x0 as f32, f.y0 as f32), ts(f.x1 as f32, f.y1 as f32)))
    }

    fn handle_at(&self, p: Pos2, rect: Rect, ppp: f32) -> Option<Handle> {
        let r = self.float_screen_rect(rect, ppp)?;
        Handle::ALL.into_iter().find(|h| h.pos(r).distance(p) <= 6.0)
    }

    fn inside_selection(&self, img_p: (f32, f32)) -> bool {
        let (x, y) = (img_p.0.floor() as i32, img_p.1.floor() as i32);
        if let Some(f) = &self.doc.floating {
            return f.rect.contains(x, y);
        }
        self.doc.selection.as_ref().is_some_and(|m| m.get(x, y) != 0)
    }

    fn handle_tool_input(
        &mut self,
        ctx: &egui::Context,
        response: &egui::Response,
        rect: Rect,
        ppp: f32,
        cx: &mut CanvasCtx,
    ) {
        let (pointer, current, pressed, released, primary_down, secondary_down, shift, double, moves) =
            ctx.input(|i| {
                let pressed = if i.pointer.button_pressed(PointerButton::Primary) {
                    Some(PointerButton::Primary)
                } else if i.pointer.button_pressed(PointerButton::Secondary) {
                    Some(PointerButton::Secondary)
                } else {
                    None
                };
                let moves: Vec<Pos2> = i
                    .events
                    .iter()
                    .filter_map(|e| if let Event::PointerMoved(p) = e { Some(*p) } else { None })
                    .collect();
                (
                    // On the frame of a press, use where the button went down, not where the
                    // pointer has moved to since (press and motion can arrive in one frame).
                    if pressed.is_some() { i.pointer.press_origin() } else { None }
                        .or(i.pointer.interact_pos())
                        .or(i.pointer.hover_pos()),
                    i.pointer.interact_pos().or(i.pointer.hover_pos()),
                    pressed,
                    i.pointer.any_released(),
                    i.pointer.primary_down(),
                    i.pointer.secondary_down(),
                    i.modifiers.shift,
                    i.pointer.button_double_clicked(PointerButton::Primary),
                    moves,
                )
            });
        let Some(pointer) = pointer else {
            self.hover_px = None;
            return;
        };
        let ip = self.to_image(pointer, rect, ppp);
        let ts = self.to_screen_fn(rect, ppp);
        self.hover_px = response
            .hovered()
            .then(|| (ip.0.floor() as i32, ip.1.floor() as i32))
            .filter(|&(x, y)| self.doc.image.in_bounds(x as i64, y as i64));

        // Space + drag pans with any tool.
        if cx.space_down && response.hovered() {
            ctx.set_cursor_icon(if primary_down { CursorIcon::Grabbing } else { CursorIcon::Grab });
            if primary_down && matches!(self.interaction, Interaction::Idle | Interaction::Pan) {
                self.interaction = Interaction::Pan;
                let d = ctx.input(|i| i.pointer.delta());
                self.view.offset.0 += d.x;
                self.view.offset.1 += d.y;
            }
            return;
        }

        let tool = *cx.tool;
        let s = cx.settings;
        let secondary_pressed = pressed == Some(PointerButton::Secondary);
        let (c1, c2) = if secondary_pressed { (*cx.bg, *cx.fg) } else { (*cx.fg, *cx.bg) };
        let clip = if s.clip_to_selection { self.doc.paint_clip() } else { None };
        // Judge a press by where it happened: the canvas rect, with no window or popup on top there.
        let pressed_here = pressed.is_some()
            && rect.contains(pointer)
            && ctx.layer_id_at(pointer).is_none_or(|l| l == response.layer_id);
        let any_down = primary_down || secondary_down;

        // ---------- presses ----------
        if pressed_here && matches!(self.interaction, Interaction::Idle) {
            if tool.paints() && tool != Tool::Text {
                self.commit_floating();
            }
            match tool {
                Tool::Pencil | Tool::Brush | Tool::Eraser => {
                    let blend = if tool == Tool::Eraser {
                        if s.eraser_to_secondary { Blend::Over(*cx.bg) } else { Blend::Erase }
                    } else {
                        Blend::Over(c1)
                    };
                    let mut op = PaintOp::new(&[blend]).with_clip(clip.clone());
                    let from = if shift { self.last_point.unwrap_or(ip) } else { ip };
                    self.stroke(&mut op, tool, s, from, ip);
                    let label = match tool {
                        Tool::Pencil => "Pencil",
                        Tool::Brush => "Brush",
                        _ => "Eraser",
                    };
                    self.interaction = Interaction::Paint { op, last: ip, label };
                }
                Tool::Line | Tool::Rect | Tool::Ellipse => {
                    let op = shape_op(s.shape_style, tool, c1, c2, clip.clone());
                    self.interaction = Interaction::Shape { op, start: ip, tool };
                }
                Tool::Polygon => {
                    let op = PaintOp::new(&[Blend::Over(c1)]).with_clip(clip.clone());
                    self.interaction =
                        Interaction::Polygon { op, points: vec![ip, ip], secondary: secondary_pressed, held: true };
                }
                Tool::Fill => {
                    let mut op = PaintOp::new(&[Blend::Replace(c1)]).with_clip(clip.clone());
                    let tol = (s.fill_tolerance as u32 * 255 / 100) as u8;
                    if fill::flood_fill(&mut op, &mut self.doc.image, 0, ip.0.floor() as i64, ip.1.floor() as i64, tol)
                    {
                        if let Some(d) = op.take_dirty() {
                            self.mark(Dirty::Rect(d));
                        }
                        self.doc.commit_paint(op, "Fill");
                    }
                }
                Tool::Picker => {
                    if let Some(c) = fill::sample(&self.doc.image, ip.0.floor() as i64, ip.1.floor() as i64) {
                        if secondary_pressed {
                            *cx.bg = c
                        } else {
                            *cx.fg = c
                        }
                        if cx.prev_tool != Tool::Picker {
                            *cx.tool = cx.prev_tool;
                        }
                    }
                }
                Tool::Text => {
                    let on_box = self
                        .text
                        .as_ref()
                        .and_then(|t| t.screen_rect(&ts))
                        .is_some_and(|r| r.expand(4.0).contains(pointer));
                    if on_box {
                        let orig = self.text.as_ref().map(|t| t.pos).unwrap_or_default();
                        self.interaction = Interaction::MoveText { start: ip, orig };
                    } else {
                        let style = s.text.clone();
                        self.commit_text(cx.fonts.as_deref_mut(), &style, *cx.fg);
                        self.start_text((ip.0.round() as i32, ip.1.round() as i32));
                    }
                }
                Tool::Select | Tool::Lasso => {
                    if pressed == Some(PointerButton::Primary) {
                        if let Some(h) = self.handle_at(pointer, rect, ppp) {
                            let d = self.doc.lift_selection();
                            self.mark_opt(d);
                            if let Some(f) = &self.doc.floating {
                                self.interaction = Interaction::ResizeFloat { handle: h, start: ip, orig: f.rect };
                            }
                        } else if self.inside_selection(ip) {
                            let d = self.doc.lift_selection();
                            self.mark_opt(d);
                            if let Some(f) = &self.doc.floating {
                                self.interaction = Interaction::MoveFloat { start: ip, orig: f.rect };
                            }
                        } else {
                            let d = self.doc.set_selection(None);
                            self.mark_opt(d);
                            self.interaction = if tool == Tool::Select {
                                Interaction::Select { start: ip, cur: ip }
                            } else {
                                Interaction::Lasso { points: vec![ip] }
                            };
                        }
                    }
                }
            }
        } else if pressed_here && let Interaction::Polygon { points, held, .. } = &mut self.interaction {
            // Another click: close when on the first point, otherwise add a corner.
            let first = ts(points[0].0, points[0].1);
            if points.len() > 3 && first.distance(pointer) < 8.0 {
                points.pop();
                self.finish_polygon(s, *cx.fg, *cx.bg);
            } else {
                let n = points.len();
                points[n - 1] = ip;
                points.push(ip);
                *held = true;
            }
        }

        if double
            && response.hovered()
            && let Interaction::Polygon { points, .. } = &mut self.interaction
        {
            points.dedup_by(|a, b| (a.0 - b.0).hypot(a.1 - b.1) < 0.5);
            self.finish_polygon(s, *cx.fg, *cx.bg);
            return;
        }

        // ---------- drags / moves ----------
        // From here on, use where the pointer is now.
        let pointer = current.unwrap_or(pointer);
        let ip = self.to_image(pointer, rect, ppp);
        match &mut self.interaction {
            Interaction::Paint { .. } if any_down => {
                let mut pts: Vec<(f32, f32)> = moves.iter().map(|&p| self.to_image(p, rect, ppp)).collect();
                pts.push(ip);
                if let Interaction::Paint { mut op, mut last, label } =
                    std::mem::replace(&mut self.interaction, Interaction::Idle)
                {
                    for p in pts {
                        if p != last {
                            self.stroke(&mut op, tool, s, last, p);
                            last = p;
                        }
                    }
                    self.interaction = Interaction::Paint { op, last, label };
                }
            }
            Interaction::Shape { op, start, tool: st, .. } if any_down => {
                op.clear(&mut self.doc.image);
                draw_shape(op, &mut self.doc.image, *st, s, *start, ip, shift);
            }
            Interaction::Polygon { op, points, held, .. } => {
                let n = points.len();
                if *held && any_down {
                    let prev = ts(points[n - 2].0, points[n - 2].1);
                    if prev.distance(pointer) > 3.0 {
                        // freehand while held
                        points[n - 1] = ip;
                        points.push(ip);
                    }
                } else {
                    points[n - 1] = ip;
                }
                op.clear(&mut self.doc.image);
                shapes::polyline(op, &mut self.doc.image, 0, points, false, s.shape_width, s.shape_aa);
            }
            Interaction::Select { cur, start } if any_down => {
                *cur = ip;
                if shift {
                    let d = (ip.0 - start.0).abs().max((ip.1 - start.1).abs());
                    *cur = (start.0 + d * (ip.0 - start.0).signum(), start.1 + d * (ip.1 - start.1).signum());
                }
            }
            Interaction::Lasso { points } if any_down => {
                if points.last().is_none_or(|l| (l.0 - ip.0).hypot(l.1 - ip.1) > 0.5) {
                    points.push(ip);
                }
            }
            Interaction::MoveFloat { start, orig } if any_down => {
                let (dx, dy) = ((ip.0 - start.0).round() as i32, (ip.1 - start.1).round() as i32);
                let r = orig.translate(dx, dy);
                self.doc.set_floating_rect(r);
                ctx.set_cursor_icon(CursorIcon::Move);
            }
            Interaction::ResizeFloat { handle, start, orig } if any_down => {
                let (dx, dy) = ((ip.0 - start.0).round() as i32, (ip.1 - start.1).round() as i32);
                let r = handle.apply(*orig, dx, dy, shift);
                ctx.set_cursor_icon(handle.cursor());
                self.doc.set_floating_rect(r);
            }
            Interaction::MoveText { start, orig } if any_down => {
                let (dx, dy) = ((ip.0 - start.0).round() as i32, (ip.1 - start.1).round() as i32);
                if let Some(t) = &mut self.text {
                    t.pos = (orig.0 + dx, orig.1 + dy);
                }
            }
            _ => {}
        }

        // ---------- releases ----------
        if released && !any_down {
            match std::mem::replace(&mut self.interaction, Interaction::Idle) {
                Interaction::Paint { mut op, last, label } => {
                    if let Some(d) = op.take_dirty() {
                        self.mark(Dirty::Rect(d));
                    }
                    self.doc.commit_paint(op, label);
                    self.last_point = Some(last);
                }
                Interaction::Shape { mut op, tool, .. } => {
                    if let Some(d) = op.take_dirty() {
                        self.mark(Dirty::Rect(d));
                    }
                    let label = match tool {
                        Tool::Line => "Line",
                        Tool::Rect => "Rectangle",
                        _ => "Ellipse",
                    };
                    self.doc.commit_paint(op, label);
                }
                Interaction::Polygon { op, points, secondary, .. } => {
                    self.interaction = Interaction::Polygon { op, points, secondary, held: false };
                }
                Interaction::Select { start, cur } => {
                    let r = IRect::from_corners(
                        start.0.floor() as i32,
                        start.1.floor() as i32,
                        cur.0.floor() as i32,
                        cur.1.floor() as i32,
                    );
                    let moved = (start.0 - cur.0).abs() >= 0.5 || (start.1 - cur.1).abs() >= 0.5;
                    let m = if moved { Mask::rect(r, self.doc.image.rect()) } else { None };
                    let d = self.doc.set_selection(m);
                    self.mark_opt(d);
                }
                Interaction::Lasso { points } => {
                    let d = self.doc.set_selection(Mask::polygon(&points, self.doc.image.rect()));
                    self.mark_opt(d);
                }
                Interaction::MoveFloat { .. }
                | Interaction::ResizeFloat { .. }
                | Interaction::MoveText { .. }
                | Interaction::Pan
                | Interaction::Idle => {}
            }
        }

        // ---------- hover cursor ----------
        if response.hovered() && matches!(self.interaction, Interaction::Idle) {
            let icon = match tool {
                Tool::Select | Tool::Lasso => match self.handle_at(pointer, rect, ppp) {
                    Some(h) => h.cursor(),
                    None if self.inside_selection(ip) => CursorIcon::Move,
                    None => CursorIcon::Crosshair,
                },
                Tool::Text => CursorIcon::Text,
                _ => CursorIcon::Crosshair,
            };
            ctx.set_cursor_icon(icon);
        }
    }

    fn finish_polygon(&mut self, s: &Settings, fg: Rgba, bg: Rgba) {
        if let Interaction::Polygon { op, points, secondary, .. } =
            std::mem::replace(&mut self.interaction, Interaction::Idle)
        {
            let clip = if s.clip_to_selection { self.doc.paint_clip() } else { None };
            let r = op.cancel(&mut self.doc.image);
            self.mark(Dirty::Rect(r));
            if points.len() < 2 {
                return;
            }
            let (c1, c2) = if secondary { (bg, fg) } else { (fg, bg) };
            let mut op = shape_op(s.shape_style, Tool::Polygon, c1, c2, clip);
            match s.shape_style {
                ShapeStyle::Outline => {
                    shapes::polyline(&mut op, &mut self.doc.image, 0, &points, true, s.shape_width, s.shape_aa)
                }
                ShapeStyle::Filled => shapes::polygon_fill(&mut op, &mut self.doc.image, 0, &points, s.shape_aa),
                ShapeStyle::OutlineAndFill => {
                    shapes::polygon_fill(&mut op, &mut self.doc.image, 0, &points, s.shape_aa);
                    shapes::polyline(&mut op, &mut self.doc.image, 1, &points, true, s.shape_width, s.shape_aa);
                }
            }
            if let Some(d) = op.take_dirty() {
                self.mark(Dirty::Rect(d));
            }
            self.doc.commit_paint(op, "Polygon");
        }
    }

    /// Finishes an in-progress polygon (Enter key).
    pub fn finish_polygon_key(&mut self, s: &Settings, fg: Rgba, bg: Rgba) {
        if let Interaction::Polygon { points, secondary, .. } = &mut self.interaction {
            if points.len() > 1 {
                points.pop();
            }
            let _ = secondary;
            self.finish_polygon(s, fg, bg);
        }
    }

    fn stroke(&mut self, op: &mut PaintOp, tool: Tool, s: &Settings, a: (f32, f32), b: (f32, f32)) {
        let img = &mut self.doc.image;
        match tool {
            Tool::Pencil if s.pencil_size <= 1 => shapes::pixel_line(
                op,
                img,
                0,
                (a.0.floor() as i32, a.1.floor() as i32),
                (b.0.floor() as i32, b.1.floor() as i32),
            ),
            Tool::Pencil => {
                let c = |p: (f32, f32)| (p.0.floor() + 0.5, p.1.floor() + 0.5);
                shapes::capsule(op, img, 0, c(a), c(b), s.pencil_size as f32, false)
            }
            Tool::Brush => shapes::capsule(op, img, 0, a, b, s.brush_size, s.brush_aa),
            _ => shapes::capsule(op, img, 0, a, b, s.eraser_size, true),
        }
    }

    // ---- painting --------------------------------------------------------

    fn paint(&self, ui: &Ui, rect: Rect, ppp: f32, cx: &CanvasCtx) {
        let painter = ui.painter_at(rect);
        let ts = self.to_screen_fn(rect, ppp);
        let (w, h) = self.doc.size();
        let img_rect = Rect::from_min_max(ts(0.0, 0.0), ts(w as f32, h as f32));

        // Transparency checkerboard, constant on screen.
        let vis = img_rect.intersect(rect);
        if vis.is_positive() {
            let cell = 16.0;
            let uv = Rect::from_min_max(
                pos2((vis.min.x - img_rect.min.x) / cell, (vis.min.y - img_rect.min.y) / cell),
                pos2((vis.max.x - img_rect.min.x) / cell, (vis.max.y - img_rect.min.y) / cell),
            );
            painter.image(cx.checker.id(), vis, uv, Color32::WHITE);
        }
        if let Some(t) = &self.tex {
            t.paint(&painter, &ts, rect);
        }
        painter.rect_stroke(
            img_rect.expand(0.5),
            0.0,
            Stroke::new(1.0, Color32::from_gray(90)),
            egui::StrokeKind::Outside,
        );

        // Floating selection.
        if let (Some(f), Some((_, tex))) = (&self.doc.floating, &self.float_tex) {
            let r = Rect::from_min_max(ts(f.rect.x0 as f32, f.rect.y0 as f32), ts(f.rect.x1 as f32, f.rect.y1 as f32));
            painter.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            ants_rect(&painter, r);
            handles(&painter, r);
        } else if let Some(m) = &self.doc.selection {
            if m.is_rectangular() || m.bounds().area() > 16_000_000 {
                let b = m.bounds();
                ants_rect(&painter, Rect::from_min_max(ts(b.x0 as f32, b.y0 as f32), ts(b.x1 as f32, b.y1 as f32)));
            } else {
                for path in m.outline_paths() {
                    let pts: Vec<Pos2> = path.iter().map(|&(x, y)| ts(x as f32, y as f32)).collect();
                    ants_path(&painter, &pts);
                }
            }
            if matches!(*cx.tool, Tool::Select | Tool::Lasso) {
                let b = m.bounds();
                handles(&painter, Rect::from_min_max(ts(b.x0 as f32, b.y0 as f32), ts(b.x1 as f32, b.y1 as f32)));
            }
        }

        // In-progress selections.
        match &self.interaction {
            Interaction::Select { start, cur } => {
                let r = IRect::from_corners(
                    start.0.floor() as i32,
                    start.1.floor() as i32,
                    cur.0.floor() as i32,
                    cur.1.floor() as i32,
                );
                ants_rect(&painter, Rect::from_min_max(ts(r.x0 as f32, r.y0 as f32), ts(r.x1 as f32, r.y1 as f32)));
            }
            Interaction::Lasso { points } => {
                let pts: Vec<Pos2> = points.iter().map(|p| ts(p.0, p.1)).collect();
                ants_path(&painter, &pts);
            }
            Interaction::Polygon { points, .. } => {
                if let Some(p) = points.first() {
                    let c = ts(p.0, p.1);
                    painter.circle_stroke(c, 5.0, Stroke::new(1.0, Color32::WHITE));
                    painter.circle_stroke(c, 6.0, Stroke::new(1.0, Color32::BLACK));
                }
            }
            _ => {}
        }

        // Editable text preview (not yet part of the image).
        if let Some(t) = &self.text {
            if let Some((_, _, _, r, tex)) = &t.cache {
                if let Some(sr) = t.screen_rect(&ts) {
                    if let Some(tex) = tex.as_ref().filter(|_| !t.content.is_empty()) {
                        painter.image(tex.id(), sr, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    let _ = r;
                    ants_rect(&painter, sr.shrink(1.0));
                }
            } else {
                let p = ts(t.pos.0 as f32, t.pos.1 as f32);
                ants_line(&painter, p, p + vec2(0.0, 20.0));
            }
        }

        // Brush footprint.
        if let Some(hp) = ui.ctx().pointer_hover_pos().filter(|p| rect.contains(*p)) {
            let size = match *cx.tool {
                Tool::Brush => Some(cx.settings.brush_size),
                Tool::Eraser => Some(cx.settings.eraser_size),
                Tool::Pencil if cx.settings.pencil_size > 1 => Some(cx.settings.pencil_size as f32),
                _ => None,
            };
            if let Some(sz) = size {
                let r = sz * self.view.scale(ppp) / 2.0;
                if r > 3.0 {
                    painter.circle_stroke(hp, r, Stroke::new(1.0, Color32::from_black_alpha(160)));
                    painter.circle_stroke(hp, r + 1.0, Stroke::new(1.0, Color32::from_white_alpha(160)));
                }
            }
        }
    }

    /// Text editing panel shown next to the text box while the text tool is active.
    pub fn text_editor(&mut self, ctx: &egui::Context) -> TextEditorAction {
        let mut action = TextEditorAction::None;
        let ppp = ctx.pixels_per_point();
        let rect = self.canvas_rect;
        let ts = self.to_screen_fn(rect, ppp);
        let Some(t) = &mut self.text else { return action };
        let anchor = t
            .screen_rect(&ts)
            .map(|r| r.left_bottom() + vec2(0.0, 8.0))
            .unwrap_or_else(|| ts(t.pos.0 as f32, t.pos.1 as f32) + vec2(0.0, 24.0));
        let anchor = pos2(
            anchor.x.clamp(rect.min.x, (rect.max.x - 370.0).max(rect.min.x)),
            anchor.y.clamp(rect.min.y, (rect.max.y - 110.0).max(rect.min.y)),
        );
        let edit_id = egui::Id::unique("text-editor-field");
        // Ctrl+Enter must be taken before the multiline field turns it into a newline.
        if ctx.memory(|m| m.has_focus(edit_id))
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter))
        {
            return TextEditorAction::Commit;
        }
        egui::Window::new("Text")
            .id(egui::Id::unique("text-editor"))
            .current_pos(anchor)
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .show(ctx, |ui| {
                let te = egui::TextEdit::multiline(&mut t.content)
                    .id(edit_id)
                    .desired_rows(3)
                    .desired_width(240.0)
                    .hint_text("Type text…  (Ctrl+Enter to apply, Esc to cancel)");
                let resp = ui.add(te);
                if t.request_focus {
                    resp.request_focus();
                    t.request_focus = false;
                }
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    action = TextEditorAction::Cancel;
                }
                ui.horizontal(|ui| {
                    if ui.button("Apply").on_hover_text("Ctrl+Enter").clicked() {
                        action = TextEditorAction::Commit;
                    }
                    if ui.button("Cancel").on_hover_text("Esc").clicked() {
                        action = TextEditorAction::Cancel;
                    }
                    ui.weak("Drag the box on the canvas to move it");
                });
            });
        action
    }
}

#[derive(PartialEq, Eq)]
pub enum TextEditorAction {
    None,
    Commit,
    Cancel,
}

fn shape_op(style: ShapeStyle, tool: Tool, c1: Rgba, c2: Rgba, clip: Option<Mask>) -> PaintOp {
    let blends: Vec<Blend> = match (tool, style) {
        (Tool::Line, _) | (_, ShapeStyle::Outline) | (_, ShapeStyle::Filled) => vec![Blend::Over(c1)],
        (_, ShapeStyle::OutlineAndFill) => vec![Blend::Over(c2), Blend::Over(c1)],
    };
    PaintOp::new(&blends).with_clip(clip)
}

fn draw_shape(
    op: &mut PaintOp,
    img: &mut Raster,
    tool: Tool,
    s: &Settings,
    start: (f32, f32),
    cur: (f32, f32),
    constrain: bool,
) {
    match tool {
        Tool::Line => {
            let mut end = cur;
            if constrain {
                let (dx, dy) = (cur.0 - start.0, cur.1 - start.1);
                let step = std::f32::consts::PI / 12.0;
                let ang = (dy.atan2(dx) / step).round() * step;
                let len = dx.hypot(dy);
                end = (start.0 + len * ang.cos(), start.1 + len * ang.sin());
            }
            let c = |p: (f32, f32)| (p.0.floor() + 0.5, p.1.floor() + 0.5);
            if s.shape_width <= 1.0 && !s.shape_aa {
                let f = |p: (f32, f32)| (p.0.floor() as i32, p.1.floor() as i32);
                shapes::pixel_line(op, img, 0, f(start), f(end));
            } else {
                shapes::capsule(op, img, 0, c(start), c(end), s.shape_width, s.shape_aa);
            }
        }
        Tool::Rect | Tool::Ellipse => {
            let (sx, sy) = (start.0.floor() as i32, start.1.floor() as i32);
            let (mut ex, mut ey) = (cur.0.floor() as i32, cur.1.floor() as i32);
            if constrain {
                let d = (ex - sx).abs().max((ey - sy).abs());
                ex = sx + d * (ex - sx).signum();
                ey = sy + d * (ey - sy).signum();
            }
            let r = IRect::from_corners(sx, sy, ex, ey);
            let t = s.shape_width.max(1.0);
            let (outline_layer, fill_layer) = match s.shape_style {
                ShapeStyle::Outline => (Some(0), None),
                ShapeStyle::Filled => (None, Some(0)),
                ShapeStyle::OutlineAndFill => (Some(1), Some(0)),
            };
            if tool == Tool::Rect {
                if let Some(l) = fill_layer {
                    shapes::rectangle(op, img, l, r, None);
                }
                if let Some(l) = outline_layer {
                    shapes::rectangle(op, img, l, r, Some(t.round() as i32));
                }
            } else {
                if let Some(l) = fill_layer {
                    shapes::ellipse(op, img, l, r, None, s.shape_aa);
                }
                if let Some(l) = outline_layer {
                    shapes::ellipse(op, img, l, r, Some(t), s.shape_aa);
                }
            }
        }
        _ => {}
    }
}

fn ants_line(p: &egui::Painter, a: Pos2, b: Pos2) {
    p.line_segment([a, b], Stroke::new(1.0, Color32::WHITE));
    p.add(Shape::dashed_line(&[a, b], Stroke::new(1.0, Color32::BLACK), 4.0, 4.0));
}

fn ants_path(p: &egui::Painter, pts: &[Pos2]) {
    if pts.len() < 2 {
        return;
    }
    p.add(Shape::line(pts.to_vec(), Stroke::new(1.0, Color32::WHITE)));
    p.add(Shape::dashed_line(pts, Stroke::new(1.0, Color32::BLACK), 4.0, 4.0));
}

fn ants_rect(p: &egui::Painter, r: Rect) {
    let c = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
    p.add(Shape::line(c.to_vec(), Stroke::new(1.0, Color32::WHITE)));
    p.add(Shape::dashed_line(&c, Stroke::new(1.0, Color32::BLACK), 4.0, 4.0));
}

fn handles(p: &egui::Painter, r: Rect) {
    for h in Handle::ALL {
        let c = h.pos(r);
        let hr = Rect::from_center_size(c, vec2(7.0, 7.0));
        p.rect_filled(hr, 0.0, Color32::WHITE);
        p.rect_stroke(hr, 0.0, Stroke::new(1.0, Color32::from_rgb(40, 90, 200)), egui::StrokeKind::Inside);
    }
}

/// 2×2 checker texture used (with UV repeat) behind transparent pixels.
pub fn checker_texture(ctx: &egui::Context) -> TextureHandle {
    let (a, b) = (Color32::from_gray(255), Color32::from_gray(204));
    let img = ColorImage::new([2, 2], vec![a, b, b, a]);
    ctx.load_texture(
        "checker",
        img,
        TextureOptions { wrap_mode: egui::TextureWrapMode::Repeat, ..TextureOptions::NEAREST },
    )
}
