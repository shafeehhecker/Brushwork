# Known limitations and outstanding defects

## Verification gaps (must be checked on Windows before a stable release)

Development happened in a Linux cloud environment where the Rust Windows target could not be installed (network
policy), and no Windows machine was reachable. Everything below is implemented with cross-platform libraries that
support Windows, but has **not yet been observed on Windows**:

1. The Windows build itself, the embedded icon (`build.rs`/`winresource`), portable zip and Inno Setup installer —
   produced by `.github/workflows/ci.yml`, which has not been run yet.
2. Native Windows file dialogs (rfd → `IFileDialog`) for Open / Save As.
3. Clipboard image exchange with Windows applications (arboard: CF_DIB/CF_DIBV5). Transparency in pasted images
   depends on the source application.
4. High-DPI behaviour at 125–200 % scaling and on mixed-DPI multi-monitor setups.
5. Window-manager close button → unsaved-changes prompt (same code path as Ctrl+W, tested only via keyboard).
6. Drag-and-drop of files from Explorer.
7. Screen reader output (AccessKit/UI Automation).
8. Startup time and memory with a real GPU driver.

## Limitations (by design or deferred)

* **Formats:** PNG, JPEG and BMP only. 16-bit images are reduced to 8 bits per channel (a notice is shown).
  Metadata (EXIF, ICC profiles) is not preserved; JPEG orientation is applied on open. No colour management.
* **JPEG quality** is fixed at 92; there is no save-options dialog yet.
* **Single layer.** No layers, adjustments, or filters.
* **Moved / deleted selections leave transparency**, not the secondary colour (Paint.NET behaviour, not MS Paint).
  Saving such an image as JPEG fills those areas with white.
* **Text:** no complex shaping — right-to-left scripts (Arabic, Hebrew) and ligature-dependent scripts render
  unjoined and left-to-right; no font fallback for characters missing from the chosen font. Text cannot be
  re-edited after it is applied (use undo).
* **Selections:** no add/subtract modes, no magic wand, no rotation by arbitrary angles; freeform selections lose their
  shape after being scaled (they become rectangular).
* **Painting** is clipped to the selection by default; this can be turned off in the selection tool options.
* **Fill** is contiguous only (no "fill all similar"), not anti-aliased.
* **Rotation** is limited to 90°/180°.
* **Canvas size** can only grow via paste-larger; there is no general "canvas size" dialog yet.
* **No scrollbars** on the canvas: scroll with the wheel / Shift+wheel, or pan with Space+drag / middle button.
* **History** is limited to 200 steps and 512 MB per document; oldest steps are discarded silently.
* **Large filled shapes** on ≥4K images preview at reduced frame rate (see `CHECKLIST.md` performance).
* The egui colour-picker popup shows one icon button whose glyph is missing from the bundled fonts (cosmetic).
* No crash recovery / autosave.

## Open defects

None known beyond the verification gaps above. Defects found and fixed during development:

* Ctrl+Enter inserted a newline instead of applying text (multiline field consumed Enter first).
* Double-click did not finish a polygon (egui reports double-clicks on release).
* Freeform selection outline drew as disconnected dots (now drawn as continuous paths).
* Uneven pixel doubling at fractional zoom (now smooth below 200 %).
* Dragging a selection handle, or the text box, could be ignored or start a new selection when the press and a
  quick movement arrived in the same frame (presses are now judged by where the button went down and which layer
  is under it).
* Canvas-sized shape previews were slow (compositor rewritten per tile; 5.6× faster at 4K).
