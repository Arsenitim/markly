# Contributing

Thanks for helping. The project's guiding rule: **do one thing well**. Paste,
annotate, copy, close. Features are judged by whether they keep the app fast,
small and obvious.

Non-negotiables:

- No networking, telemetry, accounts or cloud features.
- No background services, tray icons, daemons or helper processes (on Linux
  the copied image intentionally disappears when the app quits).
- No web views.
- Never block the UI thread on disk I/O, decoding/encoding or full-resolution
  rendering.
- Store annotation geometry in image coordinates only.
- No placeholder UI. Every visible control must work.

## Workflow

```sh
cargo fmt
cargo clippy --all-targets      # CI treats warnings as errors
cargo test                      # unit + integration tests
cargo build --release
```

CI runs the same steps on Linux, macOS (Apple Silicon) and Windows.

## Code layout

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). Keep GUI types out of
`model`, `geometry`, `render`, `store` and `undo`, because those modules are
what the tests exercise.

## Development aids

| Variable / command | Effect |
|---|---|
| `MARKLY_DATA_DIR=/tmp/x` | Use a throwaway data directory |
| `MARKLY_TRACE=1` | Print first-frame, load and shutdown timings |
| `MARKLY_BENCH_EXIT=1` | Quit as soon as the document is shown (startup benchmark) |
| `MARKLY_DEBUG_SCREENSHOT=out.png` | Save a window screenshot once the document is shown, then quit |
| `MARKLY_DEBUG_SELECT_LAST=1` | Select the last annotation after loading (to see handles) |
| `MARKLY_DEBUG_SCRIPT=steps.txt` | Replay synthetic input (clicks, drags, keys, text, copy/paste, screenshots). Syntax in `src/app/script.rs` |
| `cargo run --release --example demo_document -- DIR [W H]` | Seed DIR with a synthetic screenshot carrying every annotation type, plus `export.png` for preview-vs-export comparison |
| `cargo run --release --example bench_load -- IMAGE` | Time the stages of opening an image |
| `cargo run --example make_icons` | Regenerate `assets/icons/` (SVG, PNG, Windows `.ico`, macOS iconset) after changing `src/icon.rs` |

Script coordinates are logical window points (the top bar is 46 pt tall and
the tool rail 58 pt wide). Example end-to-end check:

```text
wait 10
paste
wait 10
key A
drag 400 250 700 450 15
key T
click 560 560
text Two broken buttons
key Escape
copy
wait 30
screenshot /tmp/annotated.png
quit
```

## Manual testing

Clipboard behaviour, file dialogs, HiDPI and drag & drop depend on the OS
and desktop. Before a release, go through [docs/QA_CHECKLIST.md](docs/QA_CHECKLIST.md)
on every platform you can, and record which ones were actually tested.

## Licensing

By contributing you agree your contribution is dual-licensed under MIT OR
Apache-2.0, like the rest of the project. Do not add assets (icons, fonts,
images) unless their license is compatible and recorded in the repository.
