# Feature checklist

Status key: **T** = covered by automated tests · **G** = exercised in the running GUI (X11/Mesa, scripted input with
screenshots) · **W** = needs confirmation on real Windows hardware (see `DEFECTS.md` §Verification gaps).

## A. Document and canvas
| Item | Status |
|---|---|
| New document, configurable size, white or transparent | T G |
| Open / Save / Save As (PNG, JPEG, BMP) | T G (Save via Ctrl+S; Save As dialog W) |
| Correct alpha handling (PNG/BMP exact; JPEG flattened onto white with notice) | T |
| Multiple documents in tabs | G |
| Zoom in/out, Ctrl+wheel at cursor, fit to window, actual size | T (mapping) G |
| Crop and resize | T G |
| Canvas size and cursor position in status bar | G |
| Unsaved-change indicator (tab "•", title "*"), cleared when undoing back to saved state | T G |
| Confirmation before closing a tab / quitting with unsaved work | G (tab close); quit path W |
| Dimension and memory limits (32768 px/side, 256 MP), allocation failure reported | T |
| Clear errors for unsupported / damaged / oversized files | T |
| Drag-and-drop and command-line opening | command line G; drag-and-drop W |

## B. Drawing tools
| Item | Status |
|---|---|
| Pencil (1 px Bresenham; larger sizes hard-edged) | T G |
| Brush, adjustable size, anti-aliasing, no self-overlap darkening | T G |
| Eraser (to transparency or secondary colour) | T G |
| Line (Shift: 15° steps) | G |
| Rectangle / filled / outline+fill | T G |
| Ellipse / filled / outline+fill (Shift: circle/square) | T G |
| Polygon & freeform shape (click corners, drag freely, double-click / Enter / first point closes) | T G |
| Fill bucket with tolerance | T G |
| Eyedropper (left primary, right secondary; returns to previous tool) | T (sampling) G |
| Text insertion | T G |
| Rectangular selection | T G |
| Freeform selection | T G |
| Cut, copy, paste, move, resize, delete on selections | T G |
| Context-sensitive tool options | G |

## C. Colour
| Item | Status |
|---|---|
| Primary/secondary colours with swatches | G |
| Preset palette (incl. transparent) | G |
| Custom colour picker (HSV) | G |
| RGB(A) fields and hex input | T (hex) G |
| Eyedropper sampling | T G |
| Swap (X) / reset (D) | G |
| Transparent pixels (checkerboard display) | T G |

## D. Editing and transformations
| Item | Status |
|---|---|
| Undo / redo for drawing, erasing, fills, text, selection moves, paste, delete, crop, resize, flips, rotations | T G |
| Flip horizontal/vertical; rotate 90° CW/CCW, 180° (image or floating selection) | T |
| Resize by pixels or percent, keep aspect ratio, filter choice | T G (dialog) |
| Selection move (drag, arrow keys) and resize (8 handles, Shift keeps aspect) | T G |
| Clipboard image interchange with other applications | G (within X11 session); W on Windows |
| Keyboard shortcuts | G |

## E. Text
| Item | Status |
|---|---|
| Editable until applied (overlay, not rasterised into the image) | G |
| Font family (system fonts, searchable list), size, bold, italic, alignment, multiline, colour | T G |
| Move text box by dragging before applying | G |

## F. Interface
| Item | Status |
|---|---|
| Title bar with document name and unsaved marker | G |
| File, Edit, Image, View, Help menus with shortcut hints | G |
| Tool panel with tooltips, options bar, palette bar, status bar | G |
| High-DPI: zoom expressed in physical pixels; egui scales UI | T (mapping) W |
| Screen-reader support (AccessKit) | enabled; W |
| Light / dark / system theme | G |

## Reliability (§6)
| Item | Status |
|---|---|
| Atomic save; failed save keeps previous file and leaves no temp files | T |
| Off-canvas, extreme and NaN coordinates handled without panics | T |
| Decoder limits applied before allocation | T |

## Release (§9)
| Item | Status |
|---|---|
| Automated tests pass (58 unit + 6 integration) | T |
| Clean checkout builds | verified (fresh clone, `cargo test`) |
| Windows executable without Rust toolchain, portable zip + installer | CI workflow written; W (not yet run) |
| Known limitations documented | `DEFECTS.md` |

## Performance (§7)

Release build, 2-core cloud VM (Linux), Mesa software OpenGL. Windows numbers will differ; GPU-accelerated drivers
should be faster for display, identical for the CPU-side operations below.

| Measure | Target | Result |
|---|---|---|
| Startup to first frame | < 1 s | ≈ 0.09–0.10 s |
| Idle CPU | ≈ 0 | 4 ticks / 10 s ≈ 0.4 % of one core |
| Resident memory, empty 1280×720 document | proportional | ≈ 125 MB (mostly the software OpenGL driver; to re-measure on Windows) |

`cargo run --release --example bench` (ms):

| Operation | 1920×1080 | 3840×2160 | 7680×4320 |
|---|---|---|---|
| New document | 5 | 10 | 88 |
| Brush stroke, 400 segments, 24 px (whole stroke) | 27–41 | 38–63 | 69–73 |
| Commit stroke to history | 1.2 | 3.2 | 7.6 |
| Ellipse outline preview redraw, canvas-sized (per mouse move) | 30 | 51 | 217 |
| Filled ellipse, canvas-sized | 41–65 | 147–227 | 552–972 |
| Flood fill | < 1 | < 1 | 1 |
| Undo / redo of local edit | < 0.1–5 | 7–14 | 26–56 |
| Flip / rotate 90° | 4 / 11 | 15 / 70 | 60 / 408 |
| Resize to 50 % (Lanczos) | 78 | 274 | 1363 |
| Save / load PNG | 14 / 6 | 28 / 21 | 107 / 124 |
| Save / load JPEG | 58 / 8 | 254 / 33 | 1150 / 240 |
| History after 5 large edits | 20 MB | 71 MB | 262 MB |

Optimisation done because of these measurements: the compositor originally used per-pixel hash lookups
(canvas-sized ellipse preview 287 ms at 4K); it now works per tile and skips pixels without coverage (51 ms).
Remaining bottleneck: *filled* shapes covering most of a ≥4K canvas recompute every pixel on each mouse move,
so such previews update at 4–7 fps at 4K. Ordinary-sized shapes and all brush work are unaffected.
