#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Brushwork — a lightweight native raster drawing and image editor.

mod app;

use eframe::egui;
use std::path::PathBuf;
use std::time::Instant;

/// Small programmatic icon (a brush stroke on a canvas) so no binary assets are needed.
fn icon() -> egui::IconData {
    let n = 64usize;
    let mut rgba = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let i = (y * n + x) * 4;
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            // rounded canvas square
            let inside = (4.0..60.0).contains(&fx) && (4.0..60.0).contains(&fy);
            if inside {
                rgba[i..i + 4].copy_from_slice(&[250, 250, 248, 255]);
            }
            // blue-to-orange stroke along a curve
            let t = (fx - 10.0) / 44.0;
            if (0.0..=1.0).contains(&t) {
                let cy = 44.0 - 26.0 * (t * std::f32::consts::PI).sin() * 0.9 - t * 6.0;
                let w = 5.0 + 3.0 * (t * std::f32::consts::PI).sin();
                if (fy - cy).abs() < w {
                    let c = [(30.0 + 220.0 * t) as u8, (110.0 + 30.0 * t) as u8, (230.0 - 190.0 * t) as u8, 255];
                    rgba[i..i + 4].copy_from_slice(&c);
                }
            }
            if inside && (!(6.0..=58.0).contains(&fx) || !(6.0..=58.0).contains(&fy)) {
                rgba[i..i + 4].copy_from_slice(&[70, 70, 80, 255]);
            }
        }
    }
    egui::IconData { rgba, width: n as u32, height: n as u32 }
}

fn main() -> eframe::Result {
    let started = Instant::now();
    let mut files = Vec::new();
    let mut probe = false;
    for a in std::env::args_os().skip(1) {
        match a.to_str() {
            Some("--startup-probe") => probe = true,
            Some("--version") => {
                println!("brushwork {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            Some("--help" | "-h") => {
                println!("Usage: brushwork [IMAGE...]\nOpens PNG, JPEG and BMP images.");
                return Ok(());
            }
            _ => files.push(PathBuf::from(a)),
        }
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Brushwork")
            .with_app_id("brushwork")
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([640.0, 420.0])
            .with_drag_and_drop(true)
            .with_icon(icon()),
        // A paint program needs no depth/stencil buffers or MSAA.
        multisampling: 0,
        depth_buffer: 0,
        stencil_buffer: 0,
        ..Default::default()
    };
    eframe::run_native(
        "Brushwork",
        options,
        Box::new(move |cc| Ok(Box::new(app::BrushworkApp::new(cc, files, probe.then_some(started))))),
    )
}
