# Brushwork

A lightweight, native drawing and image editor for Windows, written in Rust.
Draw, paint, add shapes and text, select and move areas, crop, resize, rotate, and save PNG, JPEG and BMP images.
Fully offline: no accounts, no telemetry, no network access.

> "Brushwork" is a working name; check availability before a public release.

## Install (Windows)

* **Installer:** run `Brushwork-<version>-setup.exe`. It installs for the current user (no administrator rights
  needed), adds a Start-menu entry, and optionally adds Brushwork to "Open with" for PNG/JPEG/BMP.
* **Portable:** unzip `Brushwork-<version>-windows-x64-portable.zip` and run `Brushwork.exe`.

No Rust toolchain or runtime is required. Windows 10/11, 64-bit. Images can also be opened from the command line
(`brushwork.exe photo.png`) or by dropping files onto the window.

The Windows downloads are produced by the GitHub Actions workflow in `.github/workflows/ci.yml` (artifacts on every
push; attached to a GitHub release when a `v*` tag is pushed).

## Build from source

Requires Rust 1.88 or newer.

```sh
cargo build --release          # target/release/brushwork(.exe)
cargo test                     # unit + integration tests
cargo run --release --example bench   # performance measurements
```

Windows needs nothing else. Linux needs the usual X11/Wayland and OpenGL libraries (present on desktop systems).
Building the installer requires [Inno Setup 6](https://jrsoftware.org/isinfo.php):
`iscc /DAppVersion=0.1.0 installer\brushwork.iss`.

## Features

**Documents** — multiple tabs; new images (any size up to 32768 px, white or transparent); open/save/save-as PNG, JPEG
and BMP with exact alpha preservation in PNG/BMP; unsaved-change markers in the tab and title; prompts before closing
or quitting with unsaved work; atomic saving that never damages the existing file on failure; clear errors for
damaged, oversized or unsupported files; camera (EXIF) orientation applied on open.

**Tools** — rectangle select, freeform (lasso) select, pencil, brush (anti-aliased), eraser (to transparency or to
the secondary colour), fill (with tolerance), colour picker, text, line, rectangle, ellipse, polygon/freeform shape.
Shapes can be outlined, filled, or both. Left button paints with the primary colour, right button with the secondary.

**Colours** — primary/secondary colours, 28-colour palette (including transparent), colour editor with HSV picker,
RGB(A) fields and hex input, hex fields in the palette bar, swap (X) and reset (D).

**Editing** — unlimited-feeling undo/redo within a memory budget; cut/copy/paste images with other applications;
paste into a new image; move and resize selections with handles; nudge with arrow keys; crop to selection;
resize by percentage or pixels with aspect-ratio lock and choice of resampling; flip and rotate the image or the
floating selection; paint only inside the selection (optional).

**Text** — system font families, size, bold/italic (simulated when a font has no such face), left/centre/right
alignment, multiple lines, primary colour; stays editable and movable until applied.

**View** — zoom 2 %–6400 % (Ctrl+wheel zooms at the cursor), fit to window, actual size (100 % = one image pixel per
screen pixel at any display scaling), scroll with the wheel, pan with Space+drag or the middle button, cursor
position, image and selection size in the status bar, light/dark/system theme.

## Keyboard shortcuts

| Action | Keys | Action | Keys |
|---|---|---|---|
| New / Open / Save | Ctrl+N / Ctrl+O / Ctrl+S | Save as | Ctrl+Shift+S |
| Close tab / Exit | Ctrl+W / Ctrl+Q | Next / previous tab | Ctrl+Tab / Ctrl+Shift+Tab |
| Undo / Redo | Ctrl+Z / Ctrl+Y, Ctrl+Shift+Z | Cut / Copy / Paste | Ctrl+X / C / V |
| Paste into new image | Ctrl+Alt+V | Select all / Deselect | Ctrl+A / Ctrl+D, Esc |
| Erase selection | Delete | Crop to selection | Ctrl+Shift+X |
| Resize image | Ctrl+R | Flip horizontal / vertical | Ctrl+Shift+H / Ctrl+Shift+V |
| Rotate 90° clockwise | Ctrl+Shift+R | Zoom in / out | Ctrl++ / Ctrl+− |
| Fit / actual size | Ctrl+0 / Ctrl+1 | Swap / reset colours | X / D |
| Tool size smaller / larger | [ / ] | Apply paste/move, finish polygon | Enter |
| Apply text | Ctrl+Enter | Help: all shortcuts | F1 |

Tools: S select, Q freeform select, P pencil, B brush, E eraser, F fill, I picker, T text, L line, R rectangle,
O ellipse, G polygon.

## Performance

Measured on the development machine (2-core Linux VM, release build); see `CHECKLIST.md` for the full table and
`examples/bench.rs` to reproduce. Startup to first frame ≈ 0.1 s; idle CPU ≈ 0.4 % of one core; a 400-segment
24 px brush stroke ≈ 30–70 ms total on 1080p–8K images; undo/redo of local edits a few ms.

## Documentation

* `ARCHITECTURE.md` — foundation decision, module layout, key designs.
* `CHECKLIST.md` — feature checklist and verification status, performance results.
* `DEFECTS.md` — known limitations and outstanding defects.
* `THIRD_PARTY.md` — licences of bundled dependencies.

## Licence

MIT — see `LICENSE`. Bundled dependencies are under permissive licences listed in `THIRD_PARTY.md`.
