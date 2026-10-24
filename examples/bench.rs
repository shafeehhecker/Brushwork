//! Measures core operations at representative image sizes.
//! Run with: cargo run --release --example bench

use brushwork::color::Rgba;
use brushwork::document::Document;
use brushwork::geom::IRect;
use brushwork::io::{self, FileFormat, SaveOptions};
use brushwork::paint::{Blend, PaintOp};
use brushwork::selection::Mask;
use brushwork::transform::{ResizeFilter, Rotation};
use brushwork::{fill, shapes};
use std::time::Instant;

fn ms<T>(label: &str, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let r = f();
    println!("  {label:<42} {:>9.2} ms", t.elapsed().as_secs_f64() * 1000.0);
    r
}

fn main() {
    for (w, h) in [(1920u32, 1080u32), (3840, 2160), (7680, 4320)] {
        println!("{w} × {h} ({:.1} MB of pixels)", w as f64 * h as f64 * 4.0 / 1e6);
        let mut doc = ms("new document", || Document::new_blank(w, h, Rgba::WHITE, "b".into()).unwrap());

        // A long brush stroke drawn as many short segments, like mouse input.
        let mut op = PaintOp::new(&[Blend::Over(Rgba::new(20, 40, 200, 200))]);
        let n = 400;
        ms(&format!("brush stroke, {n} segments, 24 px"), || {
            let mut last = (10.0f32, 10.0f32);
            for i in 1..=n {
                let t = i as f32 / n as f32;
                let p = (10.0 + t * (w as f32 - 20.0), h as f32 / 2.0 + (t * 40.0).sin() * h as f32 / 3.0);
                shapes::capsule(&mut op, &mut doc.image, 0, last, p, 24.0, true);
                last = p;
            }
        });
        ms("  -> commit to history", || doc.commit_paint(op, "brush"));
        println!("     history size {:.1} MB", doc.history.memory_bytes() as f64 / 1e6);

        let mut op = PaintOp::new(&[Blend::Over(Rgba::BLACK)]);
        ms("ellipse preview redraw (full canvas, AA)", || {
            shapes::ellipse(&mut op, &mut doc.image, 0, IRect::new(0, 0, w as i32, h as i32), Some(4.0), true)
        });
        ms("ellipse preview clear", || op.clear(&mut doc.image));
        ms("filled ellipse (full canvas, AA)", || {
            shapes::ellipse(&mut op, &mut doc.image, 0, IRect::new(0, 0, w as i32, h as i32), None, true)
        });
        doc.commit_paint(op, "ellipse");

        let mut op = PaintOp::new(&[Blend::Replace(Rgba::opaque(255, 0, 0))]);
        ms("flood fill (large region)", || fill::flood_fill(&mut op, &mut doc.image, 0, 1, 1, 25));
        doc.commit_paint(op, "fill");

        ms("undo fill", || doc.undo());
        ms("redo fill", || doc.redo());
        ms("undo ×3", || {
            doc.undo();
            doc.undo();
            doc.undo();
        });
        ms("redo ×3", || {
            doc.redo();
            doc.redo();
            doc.redo();
        });

        doc.set_selection(Mask::rect(IRect::new(100, 100, w as i32 / 2, h as i32 / 2), doc.image.rect()));
        ms("lift selection (quarter image)", || doc.lift_selection());
        doc.set_floating_rect(IRect::new(200, 200, 200 + w as i32 / 2 - 100, 200 + h as i32 / 2 - 100));
        ms("commit moved selection", || doc.commit_floating());

        ms("flip horizontal", || doc.flip(true));
        ms("rotate 90°", || doc.rotate(Rotation::Cw90));
        ms("undo rotate", || doc.undo());
        ms("resize to 50% (Lanczos)", || doc.resize(w / 2, h / 2, ResizeFilter::Lanczos).unwrap());
        ms("undo resize", || doc.undo());

        let dir = std::env::temp_dir();
        let png = dir.join("brushwork-bench.png");
        let jpg = dir.join("brushwork-bench.jpg");
        ms("save PNG (atomic)", || io::save_path(&doc.image, &png, FileFormat::Png, SaveOptions::default()).unwrap());
        ms("load PNG", || io::load_path(&png).unwrap());
        ms("save JPEG (atomic)", || io::save_path(&doc.image, &jpg, FileFormat::Jpeg, SaveOptions::default()).unwrap());
        ms("load JPEG", || io::load_path(&jpg).unwrap());
        println!(
            "     files: PNG {:.1} MB, JPEG {:.1} MB; history {:.1} MB in {} steps\n",
            std::fs::metadata(&png).unwrap().len() as f64 / 1e6,
            std::fs::metadata(&jpg).unwrap().len() as f64 / 1e6,
            doc.history.memory_bytes() as f64 / 1e6,
            doc.history.len()
        );
        let _ = std::fs::remove_file(png);
        let _ = std::fs::remove_file(jpg);
    }
}
