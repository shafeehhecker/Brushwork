# Brushwork architecture

Brushwork is one Cargo package with two targets:

* **`brushwork` library** (`src/lib.rs`) — the image model and every raster operation. No GUI types; fully unit-tested.
* **`brushwork` binary** (`src/main.rs`, `src/app/`) — the egui/eframe desktop UI.

## Foundation decision

Three existing Rust projects were cloned and inspected (October 2026):

| Project | Licence | State | Stack | Why not used as the base |
|---|---|---|---|---|
| lucasscharenbroch/rs-paint | MIT | active, ~12k LOC | GTK4 | GTK4 on Windows needs a bundled MSYS2 GTK runtime; conflicts with "Windows-first, single executable". |
| kylejckson/PaintFE | MIT | very active, ~156k LOC | egui + wgpu, Rhai scripting, rayon, plugin host | Already a large suite with GPU compositing; cutting it down to "lightweight, no unnecessary GPU infrastructure" costs more than building. |
| EmmanuelMess/PaintIt | MIT (README only) | inactive since 2025, ~1.6k LOC | raylib | Prototype; raylib has no native dialogs, clipboard images or text editing. |

Decision: a new, small implementation reusing established libraries. **No code was copied** from these projects.

| Need | Library | Notes |
|---|---|---|
| Window, input, widgets, high-DPI, accessibility | `eframe`/`egui` 0.37, **glow (OpenGL) backend** | wgpu deliberately not used. Reactive repaint ⇒ near-zero idle CPU. AccessKit enabled for screen readers. |
| Decode/encode | `image` (png, jpeg, bmp features only) | |
| Native file dialogs | `rfd` | Win32 `IFileDialog` on Windows, XDG portal on Linux. |
| Clipboard incl. images | `arboard` | Interoperates with other desktop apps. |
| System fonts | `fontdb` (discovery) + `ab_glyph` (rasterising; already used by egui) | |

## Module map

```
src/
  lib.rs            module list
  raster.rs         Raster: authoritative straight-alpha RGBA8 buffer; dimension/size validation
  color.rs          Rgba, hex parsing, src-over / erase / replace compositing, palette
  geom.rs           IRect (half-open integer rectangles, saturating arithmetic)
  history.rs        TileTxn (copy-on-write 64×64 tiles), Patch, History (undo/redo, memory budget)
  paint.rs          PaintOp: coverage layers composited over saved originals
  shapes.rs         capsule (brush/line), Bresenham pencil, rectangle, ellipse, polygon, polyline
  fill.rs           scanline flood fill, colour sampling
  selection.rs      Mask (rectangular or freeform), extraction, outline paths
  transform.rs      flip, rotate, crop, resize (premultiplied), Lossless transforms for history
  text.rs           FontLibrary (system fonts), text layout → coverage
  io.rs             load (format sniffing, limits, EXIF orientation), encode, atomic save
  view.rs           View: screen ↔ image mapping, zoom about a point, fit, pan clamping
  document.rs       Document: image + history + selection + floating selection + file association
src/app/
  mod.rs            BrushworkApp: menus, commands, shortcuts, panels, tabs, status bar, close handling
  canvas.rs         Tab: GPU tile textures, pointer interaction for every tool, overlays, text editor panel
  tools.rs          Tool enum, per-tool settings, vector-drawn tool icons
  dialogs.rs        New image, resize, colour editor, paste-larger, unsaved-changes prompts, help
  platform.rs       rfd file dialogs, arboard clipboard
```

## Key designs

### Display vs. authoritative data
`Document.image` is the only source of truth. The canvas uploads it into ≤512×512 GPU textures and updates only the
dirty sub-rectangles (`TextureHandle::set_partial`) after each edit. Images up to the 32768 px limit therefore never
exceed GPU texture limits, and a brush dab uploads a few KB rather than the whole image.

Magnification uses nearest-neighbour from 200 % up (and at exactly 100 %) for crisp pixels, smooth filtering
otherwise to avoid uneven pixel doubling at fractional zoom.

### Coordinate mapping
`View.zoom` is *physical display pixels per image pixel*, so 100 % is one image pixel per screen pixel at any
Windows display scaling. `to_image`/`to_screen` are pure functions tested at every zoom level and at 100–300 %
display scaling (`view.rs`, `tests/integration.rs`).

### Painting model
Every tool produces per-pixel coverage into a `PaintOp` layer. Pixels are recomputed from the **original** pixels
(saved by the op's tile transaction) and the layers, so:

* a semi-transparent stroke that overlaps itself does not darken;
* shape previews are redrawn by restoring originals (`PaintOp::clear`) — no history entry until mouse-up;
* the finished op becomes an undo patch containing only touched tiles.

Pixels with zero coverage are skipped, so a large ellipse outline only recomputes its ring.

### History
* Local edits: `Patch` of 64×64 tiles. Undo and redo are the same *swap*, so each step stores one copy of the
  touched pixels.
* Flip/rotate: stored as `Lossless` transforms (no pixels) and inverted on undo.
* Crop/resize/expand canvas: the previous full image is kept.
* Bounded by **512 MB and 200 steps per document**; oldest steps are dropped first.
* `History::state_id()` identifies the current state; the document is "unsaved" when it differs from the state at
  the last save, so undoing back to the saved state clears the indicator.

### Selections
`Mask` is a rectangle or a per-pixel freeform mask over its bounds. Moving/scaling lifts the pixels into a
`Floating` selection (the hole becomes transparent); the floating pixels are drawn as an overlay and merged as one
undo step on commit. Undo while floating puts everything back. Painting can be clipped to the selection.

### Text
Text is laid out into a coverage buffer and shown as an overlay texture while editing; it is painted into the image
only on commit (Ctrl+Enter, Apply, clicking elsewhere, or switching tools). Missing bold/italic faces are simulated.

### Files
Format is detected from file contents, not the extension. Decoding enforces the 32768 px / 256 MP limits before
allocating. Saving writes to a temporary file in the same folder, `fsync`s it, then renames it over the target;
on any failure the original file is untouched and the temporary file is removed.
