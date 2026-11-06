# Architecture

Markly is a small document editor, not a bitmap painter: the original image
is never modified, and every annotation is an editable, serialisable object
stored in **original-image pixel coordinates**.

```text
src/
  main.rs        CLI entry: flags, desktop-entry install, window options
  lib.rs         library root (everything below is testable without a GUI)
  geometry.rs    Pt/Rect, ViewTransform (image ↔ canvas ↔ screen), shape math
  model.rs       Document, Annotation{id, Shape}, styles, hit testing, handles
  text.rs        bundled font, text layout/measurement (ab_glyph)
  render.rs      tiny-skia CPU renderer: flatten/export/thumbnails, pixelation
  export.rs      PNG/JPEG export
  store.rs       on-disk layout, atomic writes, history listing, writer thread
  settings.rs    persisted preferences (tool, per-tool style, theme, window)
  undo.rs        snapshot undo/redo
  clipboard.rs   clipboard worker thread
  app/           egui front-end (GUI state only)
    mod.rs       app state, document lifecycle, shortcuts, background results
    canvas.rs    view transform use, pointer interaction, preview painting
    toolbar.rs   top bar and tool/style rail
    history.rs   history panel (lazy listing + lazy thumbnails)
    icons.rs     original vector icons, window icon
    theme.rs     light/dark palettes
    script.rs    developer aid: scripted input for end-to-end checks
```

## Document model

`Document` (`model.rs`) holds id, timestamps, image size, the stored
original's file name, the optional source path and `Vec<Annotation>`.
`Annotation { id: u64, shape: Shape }`, where `Shape` is an internally tagged
enum (`"kind": "arrow" | "rectangle" | …`). Ids are stable for the life of
the document: they survive undo/redo and restarts.

Pixelation is a regular annotation (`Shape::Pixelate { rect, block }`). It
always obscures the **image layer** and is rendered beneath all other
annotations, in both the preview and the export, so the two can never
disagree about which pixels are hidden.

## Coordinate spaces

`ViewTransform` (`geometry.rs`) maps between three spaces:

- **image space:** pixels of the original image. This is the only space
  that is ever stored.
- **canvas space:** logical points relative to the canvas widget, where
  pan and zoom live.
- **screen space:** logical points relative to the window (egui's space).
  Physical pixels = points × `pixels_per_point`.

"100 %" means one image pixel per *physical* pixel
(`zoom = 1 / pixels_per_point`), so HiDPI displays show screenshots at their
true size. The pan is snapped to whole physical pixels to keep 1:1 views
crisp. Pointer positions are converted to image space before they touch the
model, and hit-test tolerances are converted from screen points to image
pixels.

## Rendering: preview vs. export

There are two renderers, fed by the same geometry:

| | Interactive preview | Export / copy / thumbnails |
|---|---|---|
| Where | `app/canvas.rs` | `render.rs` |
| How | egui shapes (GPU, antialiased) | tiny-skia (CPU, antialiased) |
| Arrows | convex pieces from `arrow_geometry` | path union of the same pieces |
| Text | egui galley using the **same font bytes** | glyph outlines via ab_glyph |
| Pixelation | `pixel_grid()` → tiny nearest-filtered texture | `pixel_grid()` → filled blocks |

The export never reads back pixels from the window. It renders at the
original resolution (or any scale, for thumbnails) independently of zoom.
Text layout differs only in sub-pixel rounding between egui and ab_glyph.

## Threads

The UI thread never does disk I/O, image decoding/encoding or full-size
rendering:

- **loader** (one per open): reads and decodes the original, prepares the
  GPU texture data and the 320 px thumbnail base, all in parallel.
- **writer** (`store::Writer`): a single queue for originals, `document.json`,
  thumbnails, settings and deletions. Document saves are coalesced so only
  the latest state per document is written. Writes are atomic.
- **clipboard** (`clipboard::ClipboardWorker`): owns the arboard handle,
  reads pastes, flattens and sets copies.
- **history listing / thumbnail loading:** short-lived threads, started only
  while the history panel is visible.

Results come back through channels; the worker calls
`ctx.request_repaint()`. egui repaints only on input or on these
notifications, which is why idle CPU is zero.

## Persistence

```text
<data dir>/settings.json
<data dir>/documents/<id>/original.{png,jpg,webp}   (opened files: verbatim bytes)
<data dir>/documents/<id>/document.json
<data dir>/documents/<id>/thumbnail.png
```

- Pasted images are PNG-encoded (fast compression) on the writer thread.
  Opened files are stored byte-for-byte (no recompression).
- Edits mark the document dirty, and it is saved 400 ms after the last change.
  Saving waits while a drag is in progress, so a drag is written once.
- On exit, any pending save is queued and the writer is flushed (bounded
  wait).
- `atomic_write` writes a temp file in the same directory, fsyncs, then
  renames. Leftover temp files are cleaned up at startup.
- Corrupt or incomplete documents are skipped in history (never deleted
  automatically). Annotations with non-finite geometry are dropped on load.
- No database: a directory per document is simple, robust, inspectable and
  needs no migrations. History listing parses only `document.json` files,
  and only when the panel is opened.

## Undo

`UndoStack` stores snapshots of the annotation list (geometry only, never
pixels; capped at 200). Interactions snapshot when they start and commit
once when they end, so a drag of hundreds of pointer events is one undo step.
A whole text-editing session is also one step. Undo history is per process,
by design. The annotations themselves are persisted.

## Clipboard

Paste: egui-winit turns Cmd+V into `Event::Paste(text)` or
`Event::PasteImage`. An opaque `PasteImage` is used directly (lossless).
Anything else asks the clipboard worker, which tries an image, then a copied
file list, then a `file://`/path text. While a text annotation is being
edited, clipboard events are left to the text field.

Copy: the worker flattens and sets an RGBA image. On Linux the image is served
by this process, so it leaves the clipboard when Markly quits. That is
intentional and documented ("paste before you quit").

## Switching to wgpu

Nothing outside `main.rs` (`Renderer::Glow`) and
`app::query_max_texture_side` (a GL query with a fallback) depends on glow.
Switching means changing the eframe feature from `glow` to `wgpu`, setting
`Renderer::Wgpu`, and using `on_exit(&mut self)` without the GL argument.
