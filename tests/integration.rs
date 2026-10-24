//! End-to-end checks of the core workflows, independent of the GUI.

use brushwork::color::Rgba;
use brushwork::document::Document;
use brushwork::geom::IRect;
use brushwork::io::{self, FileFormat, SaveOptions};
use brushwork::paint::{Blend, PaintOp};
use brushwork::selection::Mask;
use brushwork::shapes;
use brushwork::transform::{ResizeFilter, Rotation};
use brushwork::view::{View, ZOOM_LEVELS};

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("brushwork-it-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}

/// Clicking a screen point with the pencil must colour exactly the pixel under it,
/// at every zoom level, display scale and pan offset.
#[test]
fn pencil_hits_pixel_under_cursor_at_all_zooms() {
    for &ppp in &[1.0f32, 1.25, 1.5, 2.0] {
        for &zoom in &ZOOM_LEVELS {
            let mut doc = Document::new_blank(64, 48, Rgba::WHITE, "t".into()).unwrap();
            let view = View { zoom, offset: (37.25, -11.5) };
            // centre of pixel (13, 29) on screen
            let target = (13.0f32, 29.0f32);
            let scr = view.to_screen((target.0 + 0.5, target.1 + 0.5), ppp);
            let ip = view.to_image(scr, ppp);
            let px = (ip.0.floor() as i32, ip.1.floor() as i32);
            let mut op = PaintOp::new(&[Blend::Over(Rgba::BLACK)]);
            shapes::pixel_line(&mut op, &mut doc.image, 0, px, px);
            doc.commit_paint(op, "dot");
            assert_eq!(doc.image.get(13, 29), Rgba::BLACK, "zoom {zoom} ppp {ppp}");
            let count = doc.image.data().chunks_exact(4).filter(|p| p[0] == 0).count();
            assert_eq!(count, 1, "zoom {zoom} ppp {ppp}");
        }
    }
}

#[test]
fn draw_save_reload_edit_undo_round_trip() {
    let mut doc = Document::new_blank(320, 200, Rgba::TRANSPARENT, "t".into()).unwrap();
    let mut op = PaintOp::new(&[Blend::Over(Rgba::new(255, 0, 0, 128))]);
    shapes::capsule(&mut op, &mut doc.image, 0, (10.0, 10.0), (300.0, 190.0), 12.0, true);
    doc.commit_paint(op, "stroke");
    let path = tmp("roundtrip.png");
    io::save_path(&doc.image, &path, FileFormat::Png, SaveOptions::default()).unwrap();
    doc.mark_saved();
    assert!(!doc.is_dirty());

    let loaded = io::load_path(&path).unwrap();
    assert_eq!(loaded.raster, doc.image, "PNG must preserve straight alpha exactly");
    let mut doc2 = Document::from_loaded(loaded, path.clone());
    let snapshot = doc2.image.clone();
    // semi-transparent stroke kept its alpha
    let p = doc2.image.get(155, 100);
    assert!(p.a > 100 && p.a < 160 && p.r == 255, "{p:?}");

    doc2.rotate(Rotation::Cw90);
    doc2.resize(100, 160, ResizeFilter::Bilinear).unwrap();
    doc2.set_selection(Mask::rect(IRect::new(0, 0, 50, 50), doc2.image.rect()));
    doc2.delete_selection();
    assert!(doc2.is_dirty());
    while doc2.undo().is_some() {}
    assert_eq!(doc2.image, snapshot);
    assert!(!doc2.is_dirty());
}

#[test]
fn undo_redo_consistency_over_mixed_operations() {
    let mut doc = Document::new_blank(97, 61, Rgba::WHITE, "t".into()).unwrap();
    let mut states = vec![doc.image.clone()];
    let colors = [Rgba::BLACK, Rgba::opaque(200, 10, 10), Rgba::new(0, 0, 255, 90)];
    for i in 0..12 {
        match i % 6 {
            0 => {
                let mut op = PaintOp::new(&[Blend::Over(colors[i % 3])]);
                shapes::capsule(&mut op, &mut doc.image, 0, (i as f32 * 7.0, 3.0), (90.0, 55.0 - i as f32), 5.0, true);
                doc.commit_paint(op, "brush");
            }
            1 => {
                let mut op = PaintOp::new(&[Blend::Erase]);
                shapes::capsule(&mut op, &mut doc.image, 0, (0.0, 30.0), (96.0, 30.0), 9.0, true);
                doc.commit_paint(op, "erase");
            }
            2 => {
                doc.flip(i % 4 == 2);
            }
            3 => {
                let mut op = PaintOp::new(&[Blend::Replace(colors[(i + 1) % 3])]);
                brushwork::fill::flood_fill(&mut op, &mut doc.image, 0, 1, 1, 10);
                doc.commit_paint(op, "fill");
            }
            4 => {
                doc.set_selection(Mask::rect(IRect::new(5, 5, 40, 30), doc.image.rect()));
                doc.lift_selection();
                doc.set_floating_rect(IRect::new(30, 20, 70, 50));
                doc.commit_floating();
                doc.set_selection(None);
            }
            _ => {
                doc.rotate(Rotation::Half);
            }
        }
        states.push(doc.image.clone());
    }
    // undo all the way back, checking every intermediate state
    for expected in states.iter().rev().skip(1) {
        doc.undo().unwrap();
        assert_eq!(&doc.image, expected);
    }
    assert!(doc.undo().is_none());
    for expected in states.iter().skip(1) {
        doc.redo().unwrap();
        assert_eq!(&doc.image, expected);
    }
}

#[test]
fn transparent_document_and_eraser() {
    let mut doc = Document::new_blank(10, 10, Rgba::TRANSPARENT, "t".into()).unwrap();
    assert!(doc.image.data().iter().all(|&b| b == 0));
    let mut op = PaintOp::new(&[Blend::Over(Rgba::opaque(9, 9, 9))]);
    let all = doc.image.rect();
    shapes::rectangle(&mut op, &mut doc.image, 0, all, None);
    doc.commit_paint(op, "fill");
    let mut er = PaintOp::new(&[Blend::Erase]);
    shapes::rectangle(&mut er, &mut doc.image, 0, IRect::new(0, 0, 5, 10), None);
    doc.commit_paint(er, "erase");
    assert_eq!(doc.image.get(2, 2), Rgba::TRANSPARENT);
    assert_eq!(doc.image.get(7, 2), Rgba::opaque(9, 9, 9));
    // JPEG cannot store alpha: flattened onto white
    let p = tmp("flat.jpg");
    let rep = io::save_path(&doc.image, &p, FileFormat::Jpeg, SaveOptions { jpeg_quality: 100 }).unwrap();
    assert!(rep.flattened_alpha);
    let l = io::load_path(&p).unwrap();
    assert!(l.raster.get(1, 1).r > 240 && l.raster.get(1, 1).a == 255);
}

#[test]
fn colour_sampling_matches_pixels() {
    let mut doc = Document::new_blank(5, 5, Rgba::TRANSPARENT, "t".into()).unwrap();
    doc.image.set(3, 4, Rgba::new(1, 2, 3, 4));
    assert_eq!(brushwork::fill::sample(&doc.image, 3, 4), Some(Rgba::new(1, 2, 3, 4)));
    assert_eq!(brushwork::fill::sample(&doc.image, 5, 4), None);
    assert_eq!(brushwork::fill::sample(&doc.image, -1, 0), None);
}

#[test]
fn off_canvas_and_extreme_coordinates_are_safe() {
    let mut doc = Document::new_blank(50, 50, Rgba::WHITE, "t".into()).unwrap();
    let mut op = PaintOp::new(&[Blend::Over(Rgba::BLACK)]);
    shapes::capsule(&mut op, &mut doc.image, 0, (-1e7, -1e7), (1e7, 1e7), 3.0, true);
    shapes::pixel_line(&mut op, &mut doc.image, 0, (i32::MIN / 2, 0), (i32::MAX / 2, 0));
    shapes::rectangle(&mut op, &mut doc.image, 0, IRect::new(i32::MIN, i32::MIN, i32::MAX, i32::MAX), Some(2));
    shapes::ellipse(&mut op, &mut doc.image, 0, IRect::new(-100, -100, 1000, 20), None, true);
    shapes::polygon_fill(&mut op, &mut doc.image, 0, &[(f32::NAN, 0.0), (10.0, 10.0), (0.0, 10.0)], true);
    doc.commit_paint(op, "x");
    doc.paste(brushwork::raster::Raster::new(4, 4, Rgba::BLACK).unwrap(), i32::MAX - 2, i32::MIN + 2);
    doc.commit_floating();
    assert!(doc.set_selection(Mask::rect(IRect::new(100, 100, 200, 200), doc.image.rect())).is_none());
}
