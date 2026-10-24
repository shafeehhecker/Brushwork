//! Tool definitions, per-tool settings and vector tool icons.

use brushwork::text::TextStyle;
use eframe::egui::{self, Color32, Key, Painter, Pos2, Rect, Stroke, pos2, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Lasso,
    Pencil,
    Brush,
    Eraser,
    Fill,
    Picker,
    Text,
    Line,
    Rect,
    Ellipse,
    Polygon,
}

impl Tool {
    pub const ALL: [Tool; 12] = [
        Tool::Select,
        Tool::Lasso,
        Tool::Pencil,
        Tool::Brush,
        Tool::Eraser,
        Tool::Fill,
        Tool::Picker,
        Tool::Text,
        Tool::Line,
        Tool::Rect,
        Tool::Ellipse,
        Tool::Polygon,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Tool::Select => "Rectangle select",
            Tool::Lasso => "Freeform select",
            Tool::Pencil => "Pencil",
            Tool::Brush => "Brush",
            Tool::Eraser => "Eraser",
            Tool::Fill => "Fill",
            Tool::Picker => "Colour picker",
            Tool::Text => "Text",
            Tool::Line => "Line",
            Tool::Rect => "Rectangle",
            Tool::Ellipse => "Ellipse",
            Tool::Polygon => "Polygon / freeform shape",
        }
    }

    pub fn key(self) -> Key {
        match self {
            Tool::Select => Key::S,
            Tool::Lasso => Key::Q,
            Tool::Pencil => Key::P,
            Tool::Brush => Key::B,
            Tool::Eraser => Key::E,
            Tool::Fill => Key::F,
            Tool::Picker => Key::I,
            Tool::Text => Key::T,
            Tool::Line => Key::L,
            Tool::Rect => Key::R,
            Tool::Ellipse => Key::O,
            Tool::Polygon => Key::G,
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Tool::Select => "Drag to select. Drag inside to move, drag handles to resize. Shift keeps proportions.",
            Tool::Lasso => "Drag around an area to select it. Drag inside a selection to move it.",
            Tool::Pencil => {
                "Hard-edged freehand drawing. Left: primary colour, right: secondary. Shift-click draws a straight line."
            }
            Tool::Brush => {
                "Smooth freehand painting. Left: primary colour, right: secondary. Shift-click draws a straight line."
            }
            Tool::Eraser => "Erase to transparency (or to the secondary colour, see options).",
            Tool::Fill => "Click to fill an area of similar colour.",
            Tool::Picker => "Click to pick the primary colour, right-click for the secondary.",
            Tool::Text => "Click to place text, type in the text box, press Ctrl+Enter or click elsewhere to apply.",
            Tool::Line => "Drag to draw a line. Shift snaps to 15° angles.",
            Tool::Rect => "Drag to draw a rectangle. Shift draws a square.",
            Tool::Ellipse => "Drag to draw an ellipse. Shift draws a circle.",
            Tool::Polygon => {
                "Click to add corners, drag to draw freely; double-click, Enter or click the first point to finish."
            }
        }
    }

    /// Tools that change pixels (and so commit a floating selection when used).
    pub fn paints(self) -> bool {
        !matches!(self, Tool::Select | Tool::Lasso | Tool::Picker)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeStyle {
    Outline,
    Filled,
    OutlineAndFill,
}

pub struct Settings {
    pub pencil_size: u32,
    pub brush_size: f32,
    pub brush_aa: bool,
    pub eraser_size: f32,
    pub eraser_to_secondary: bool,
    pub shape_width: f32,
    pub shape_style: ShapeStyle,
    pub shape_aa: bool,
    /// 0–100 %.
    pub fill_tolerance: u8,
    pub text: TextStyle,
    pub clip_to_selection: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            pencil_size: 1,
            brush_size: 8.0,
            brush_aa: true,
            eraser_size: 16.0,
            eraser_to_secondary: false,
            shape_width: 2.0,
            shape_style: ShapeStyle::Outline,
            shape_aa: true,
            fill_tolerance: 10,
            text: TextStyle::default(),
            clip_to_selection: true,
        }
    }
}

/// Draws a tool icon into `rect`.
pub fn paint_icon(p: &Painter, rect: Rect, tool: Tool, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) / 20.0; // design grid: 20×20
    let at = |x: f32, y: f32| -> Pos2 { pos2(c.x + (x - 10.0) * s, c.y + (y - 10.0) * s) };
    let st = Stroke::new(1.4 * s.max(0.8), color);
    let thin = Stroke::new(1.0 * s.max(0.8), color);
    let dashed = |a: Pos2, b: Pos2| {
        let n = 5;
        for i in 0..n {
            if i % 2 == 0 {
                let t0 = i as f32 / n as f32;
                let t1 = (i + 1) as f32 / n as f32;
                p.line_segment([a.lerp(b, t0), a.lerp(b, t1)], thin);
            }
        }
    };
    match tool {
        Tool::Select => {
            let (a, b, cc, d) = (at(3.0, 4.0), at(17.0, 4.0), at(17.0, 16.0), at(3.0, 16.0));
            dashed(a, b);
            dashed(b, cc);
            dashed(cc, d);
            dashed(d, a);
        }
        Tool::Lasso => {
            let pts: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let t = i as f32 / 24.0 * std::f32::consts::TAU;
                    at(10.0 + 7.0 * t.cos(), 8.0 + 4.5 * t.sin())
                })
                .collect();
            for (i, w) in pts.windows(2).enumerate() {
                if i % 3 != 2 {
                    p.line_segment([w[0], w[1]], thin);
                }
            }
            p.line_segment([at(6.0, 11.5), at(8.0, 17.0)], st);
        }
        Tool::Pencil => {
            p.line_segment([at(4.0, 16.0), at(14.5, 5.5)], Stroke::new(3.0 * s, color));
            p.line_segment([at(3.0, 17.0), at(4.5, 15.5)], st);
            p.line_segment([at(13.0, 4.0), at(16.0, 7.0)], st);
        }
        Tool::Brush => {
            p.line_segment([at(9.0, 11.0), at(16.0, 4.0)], Stroke::new(2.0 * s, color));
            p.circle_filled(at(7.0, 13.0), 2.6 * s, color);
            p.line_segment([at(3.0, 17.5), at(6.0, 14.0)], Stroke::new(2.2 * s, color));
        }
        Tool::Eraser => {
            let pts = vec![at(3.0, 13.0), at(10.0, 6.0), at(17.0, 11.0), at(11.0, 17.0), at(7.0, 17.0)];
            p.add(egui::Shape::closed_line(pts, st));
            p.line_segment([at(6.5, 9.5), at(13.5, 14.5)], st);
        }
        Tool::Fill => {
            let pts = vec![at(4.0, 9.0), at(10.0, 3.5), at(16.0, 9.0), at(10.0, 15.0)];
            p.add(egui::Shape::closed_line(pts, st));
            p.circle_filled(at(16.5, 15.0), 2.0 * s, color);
            p.line_segment([at(16.0, 9.0), at(16.5, 13.0)], st);
        }
        Tool::Picker => {
            p.line_segment([at(4.0, 16.0), at(12.0, 8.0)], Stroke::new(2.0 * s, color));
            p.circle_filled(at(14.0, 6.0), 3.0 * s, color);
            p.line_segment([at(3.0, 17.0), at(4.5, 15.5)], st);
        }
        Tool::Text => {
            p.line_segment([at(4.0, 4.5), at(16.0, 4.5)], Stroke::new(2.0 * s, color));
            p.line_segment([at(10.0, 4.5), at(10.0, 16.0)], Stroke::new(2.0 * s, color));
            p.line_segment([at(7.5, 16.0), at(12.5, 16.0)], st);
        }
        Tool::Line => {
            p.line_segment([at(4.0, 16.0), at(16.0, 4.0)], st);
        }
        Tool::Rect => {
            p.rect_stroke(Rect::from_two_pos(at(3.5, 5.0), at(16.5, 15.0)), 0.0, st, egui::StrokeKind::Middle);
        }
        Tool::Ellipse => {
            p.add(egui::Shape::ellipse_stroke(at(10.0, 10.0), vec2(7.0 * s, 5.5 * s), st));
        }
        Tool::Polygon => {
            let pts = vec![at(4.0, 15.0), at(6.0, 5.0), at(12.0, 9.0), at(16.0, 4.0), at(16.0, 15.0)];
            p.add(egui::Shape::closed_line(pts, st));
        }
    }
}
