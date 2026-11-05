# Markly

> **Provisional name.** "Markly" is a placeholder until the project picks a
> permanent, trademark-checked name.

A small, fast, local-first image annotation utility for Windows, macOS and
Linux. Built with Rust and egui.

**Paste an image, annotate it, copy the result, close the app.** Your work is
remembered automatically, even if you never save it.

![Markly with an annotated screenshot and the history panel](docs/screenshot.png)

## Features

- **Paste** an image (Ctrl/⌘+V), **open** a file, **drag & drop**, or pass a
  file on the command line. PNG, JPEG and WebP.
- **Annotate** with arrows, rounded rectangles, ellipses, multi-line text,
  smooth freehand strokes and **pixelation** that really destroys the
  underlying pixels.
- Every annotation stays **editable**: select, move, resize with handles,
  recolour, delete, undo/redo.
- **Copy** (Ctrl/⌘+C) puts the flattened image, at the original resolution,
  on the system clipboard, ready to paste into a browser, chat app or editor.
- **Export** to PNG or JPEG.
- **Automatic local history.** Every pasted or opened image becomes a working
  document that is autosaved and restored on the next launch. A history
  panel with thumbnails lets you reopen or delete earlier documents.
- Light and dark themes (follows the system by default).

What it deliberately does *not* do: no screen capture, no cloud, no accounts,
no telemetry, no network access at all, no tray icon or background service,
no web view.

## Keyboard shortcuts

`Cmd` means Ctrl on Windows/Linux and ⌘ on macOS.

| Action | Shortcut |
|---|---|
| Paste image | Cmd+V |
| Copy annotated image | Cmd+C |
| Open image | Cmd+O |
| Export image | Cmd+Shift+S (or Cmd+S) |
| Undo / Redo | Cmd+Z / Cmd+Shift+Z (also Cmd+Y) |
| Delete selected annotation | Delete / Backspace |
| Cancel drag / clear selection / finish text | Escape |
| Fit image / Actual size (1:1) | Cmd+0 / Cmd+1 |
| Zoom in / out | Cmd+= / Cmd+−, or Cmd+mouse wheel, or pinch |
| Pan | Scroll, drag empty space with Select, middle-drag, or Space+drag |
| Nudge selection | Arrow keys (Shift = 10 px) |
| Tools | V select · A arrow · R rectangle · E ellipse · T text · P pen · X pixelate |
| Toggle history | H |
| Constrain (15° arrows, squares, circles) | hold Shift while drawing/resizing |

While you are typing in a text annotation, Cmd+C/V/X/Z and Delete act on the
text, not on the image.

## Install / run

Download an archive for your platform from the
[latest release](https://github.com/Arsenitim/markly/releases/latest)
(Linux x86_64, macOS Apple Silicon, Windows x86_64), unpack it and run
`markly` (`markly.exe` on Windows). The release
is a single portable binary with no installer and no auto-update.

- **macOS:** the binary is not signed or notarised yet. Remove the quarantine
  flag after unpacking: `xattr -d com.apple.quarantine ./markly`.
- **Linux runtime libraries** (present on virtually every desktop; loaded at
  runtime, so building needs no `-dev` packages):
  `libwayland-client`, `libxkbcommon` (Wayland) or `libX11`, `libXcursor`,
  `libXrandr`, `libXi` (X11), plus `libEGL`/`libGL` (Mesa or a vendor driver).
  File dialogs use the **XDG desktop portal** (`xdg-desktop-portal` plus a
  backend such as `-gnome`, `-kde` or `-gtk`).
  Debian/Ubuntu: `sudo apt install libxkbcommon0 libwayland-client0 libegl1 xdg-desktop-portal`.

### Where documents are stored

| OS | Location |
|---|---|
| Linux | `$XDG_DATA_HOME/markly` (usually `~/.local/share/markly`) |
| macOS | `~/Library/Application Support/Markly` |
| Windows | `%APPDATA%\Markly\data` |

Each document is a folder containing the untouched original image,
`document.json` (editable annotations) and `thumbnail.png`. The 300 most
recently modified documents are kept; older ones are pruned automatically.
Nothing ever leaves your computer.

## Clipboard notes and platform limitations

- **Linux (X11 and Wayland):** clipboard content belongs to the process
  that set it and normally vanishes when that process exits. If your copied
  image is still on the clipboard when Markly closes, Markly starts a tiny
  helper (`markly --hold-clipboard`) that keeps serving that one image **until
  something else is copied, then exits**. It is not a daemon and never runs
  otherwise. A clipboard manager, if you use one, makes the helper redundant
  but harmless.
- On Wayland, Markly uses the `wlr/ext-data-control` protocol when the
  compositor offers it, and otherwise the X11 clipboard through XWayland.
  Verified on GNOME 50 (Wayland) only; see [docs/QA_CHECKLIST.md](docs/QA_CHECKLIST.md).
- Images go through the platform's standard image clipboard formats via
  [arboard](https://crates.io/crates/arboard). On Linux that is `image/png`.
  The Windows and macOS formats come from arboard and have not yet been
  checked by hand against other applications (see the QA checklist).
- Copying renders on a background thread. A 4K image takes about 60 ms to
  render and an 8K one about 180 ms, plus the clipboard transfer.
- If the clipboard holds a copied *file* (e.g. from a file manager) or a
  `file://` path to an image, pasting opens that file.

## Building from source

Requires a stable Rust toolchain (1.88 or newer).

```sh
cargo run --release              # run
cargo build --release            # target/release/markly
cargo fmt && cargo clippy --all-targets && cargo test
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for development aids (scripted UI
runs, screenshots, timing traces) and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
for how the code is organised.

## Performance

Measured, not assumed. See [docs/PERFORMANCE.md](docs/PERFORMANCE.md). On a
2025 laptop (Linux, GNOME Wayland) the first frame appears about 110–150 ms
after launch. A 1600×1000 document is restored within about 50 ms after that,
a 4K one within about 100 ms. Idle CPU use is zero, and shutdown takes a few
milliseconds.

## License

Dual-licensed under either of [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE) at your option.

The bundled annotation font, Noto Sans SemiBold, is © Google and licensed
under the [SIL Open Font License 1.1](assets/fonts/OFL.txt). egui's built-in
UI fonts carry their own licenses (Ubuntu Font Licence, OFL, MIT/Apache).
All icons are original vector drawings in `src/app/icons.rs`.
