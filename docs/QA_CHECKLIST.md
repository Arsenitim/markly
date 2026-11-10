# Manual QA checklist

Run on each target platform before a release, and record the OS, desktop
and session type (X11/Wayland) next to every result. Use a throwaway data
directory if you prefer: `SKRIB_DATA_DIR=/tmp/skrib-qa skrib`.

Status legend: ✅ verified · ❌ failed · — not yet tested

## Verification status

| Area | Linux: GNOME 50 Wayland (Ubuntu 26.04) | Linux X11 | Windows 10/11 | macOS (universal: Apple Silicon + Intel) |
|---|---|---|---|---|
| Launch, restore last document | ✅ | — | — | — |
| Paste image from the system clipboard | ✅ (`wl-copy`, image/png) | — | — | — |
| Copy flattened image, readable by another app | ✅ (`wl-paste`) | — | — | — |
| Drawing, select, resize, undo/redo, delete | ✅ (scripted input) | — | — | — |
| Text editing on canvas | ✅ (scripted input) | — | — | — |
| History panel and thumbnails | ✅ | — | — | — |
| Light/dark theme | ✅ | — | — | — |
| App icon in launcher / dock / Explorer | ✅ desktop entry installs, validates and launches; icon look not verified on screen | — | — | — |
| File dialogs (Open / Export) | — | — | — | — |
| Drag & drop | — | — | — | — |
| Pasting into browsers, chat apps, image editors | — | — | — | — |

The Linux results were exercised with the developer script runner
(`SKRIB_DEBUG_SCRIPT`), which feeds real egui input events, plus the system
clipboard tools. A human pass with a real mouse and keyboard is still
recommended.

## 1. Startup and restore

- [ ] First launch shows the empty state ("Paste an image to start") with no dialogs.
- [ ] Startup does not paste the clipboard automatically.
- [ ] Paste, annotate, close the window without saving, relaunch: the same
      image and annotations come back, still editable (select one, move it).
- [ ] The window size is restored.
- [ ] `skrib path/to/image.png` opens that file as a new document.
- [ ] `skrib --version` and `skrib --help` print and exit.
- [ ] Launch with a corrupted `settings.json` (write garbage into it): the app starts with defaults.
- [ ] Corrupt the last document's `document.json`: the app starts empty and
      shows a short error; the other history entries are still listed.

## 2. Paste sources (Cmd/Ctrl+V)

- [ ] OS screenshot tool (GNOME Screenshot / Snipping Tool / ⌘⇧⌃4)
- [ ] Browser "Copy image" (Firefox, Chrome/Edge, Safari)
- [ ] Image editor copy (GIMP, Paint, Preview, Krita)
- [ ] Copying an image **file** in the file manager, then pasting opens it
- [ ] Text-only clipboard: shows "The clipboard does not contain an image" and nothing else changes
- [ ] Image with transparency (PNG with alpha): checkerboard shows through; alpha is kept on copy/export
- [ ] Pasting while editing text inserts text, not an image

## 3. Copy targets (Cmd/Ctrl+C)

The pasted result must have the original resolution and show every
annotation (including pixelation and text), with no selection handles.

- [ ] Browser (e.g. a Google Docs / GitHub comment box)
- [ ] Chat apps (Slack, Discord, Teams, Telegram, Signal)
- [ ] Image editors (GIMP, Paint, Preview "New from Clipboard", Krita)
- [ ] Office (LibreOffice, Word, Pages)
- [ ] Windows/macOS: copy, close Skrib, then paste elsewhere. It still works.
      (On Linux the image is expected to disappear on quit; check that no
      `skrib` process remains afterwards: `pgrep -a skrib`.)
- [ ] **Linux:** repeat on X11 and on Wayland (GNOME, KDE Plasma, a wlroots compositor).
- [ ] Cmd+C while editing a text label copies the selected text only.

## 4. Annotation tools

For each tool (A, R, E, T, P, X), with several colours and sizes:

- [ ] Drawing feels immediate and smooth at 60 fps, also on a 4K image.
- [ ] Shift constrains (15° arrows, squares, circles).
- [ ] A click without dragging does not leave an invisible annotation behind.
- [ ] The new annotation is selected; colour/size buttons change it, and undo reverts that.
- [ ] With a drawing tool, hovering an annotation outlines it in pink; pressing
      there selects/moves it, pressing elsewhere draws. Inside a pixelated area
      a drawing tool always draws.
- [ ] Select tool: click selects, hover highlights, drag moves, handles resize,
      Delete/Backspace removes, Escape clears selection, arrow keys nudge.
- [ ] Curved arrow: the middle handle bends it (sideways and lopsided); Shift keeps
      it symmetric; moving the endpoints keeps the bend; export matches.
- [ ] Escape during a drag cancels it.
- [ ] Text: click to place, type several lines (Enter), Escape or click
      elsewhere finishes. Empty text is discarded. Clicking existing text with
      the Text tool (or double-clicking it with Select) edits it.
- [ ] IME input (e.g. Japanese/Chinese) works in text annotations.
- [ ] Pixelation: the exported/copied image contains blocks, not the original pixels.
- [ ] Per-tool style is remembered across restarts.

## 5. Canvas

- [ ] Fit (Cmd+0), 1:1 (Cmd+1), zoom buttons, Cmd+wheel zooms around the cursor, trackpad pinch.
- [ ] Scrolling pans a zoomed image; Space+drag and middle-drag pan.
- [ ] Resizing the window never distorts the image; in Fit mode it refits.
- [ ] On a HiDPI display, 100 % shows the screenshot at true pixel size, crisp.
- [ ] Moving the window between monitors with different scale factors.
- [ ] 8K image: opens without freezing the window, and panning/drawing stay responsive.

## 6. History

- [ ] Each paste or open creates a new entry; the previous one is kept.
- [ ] Thumbnails appear lazily and update after edits.
- [ ] Clicking an entry opens it with its annotations.
- [ ] Deleting an entry (hover → trash) removes it; deleting the open document closes it.
- [ ] "Clear all" asks for inline confirmation, then removes everything.

## 7. Export (Cmd/Ctrl+Shift+S)

- [ ] PNG and JPEG; the default file name is `<source>-annotated.png` next to the source file.
- [ ] The export has the original dimensions regardless of zoom or window size.
- [ ] JPEG: transparent areas become white.
- [ ] The export visually matches the canvas (text position, arrow shapes, pixelation).

## 8. App icon and launcher

- [ ] Linux: `skrib --install-desktop-entry`. Skrib appears in the app
      grid/menu with its icon, launches from there, and the running window
      shows the icon in the dash and Alt+Tab. `--uninstall-desktop-entry`
      removes it.
- [ ] macOS: `Skrib.app` shows the icon in Finder and in the Dock while running.
- [ ] Windows: `skrib.exe` shows the icon in Explorer, the taskbar and Alt+Tab.

## 9. Robustness

- [ ] `kill -9` the app 1 s after an edit, then relaunch: at most the last
      ~0.4 s of edits is lost, and nothing is corrupted.
- [ ] Data directory not writable: the app still works and shows a warning.
- [ ] Idle with a document open: CPU is about 0 % (check with `top` or Activity Monitor).
- [ ] Close is effectively instant.
