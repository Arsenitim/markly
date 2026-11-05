# Performance

Targets: cold start under 400 ms to a visible window, warm start under
200 ms, zero idle CPU, smooth 60 fps interaction, instant shutdown.
Measured, not assumed.

## Test machine

- Intel Core Ultra 7 255U, integrated Arrow Lake graphics (Mesa 26)
- Ubuntu 26.04, GNOME Shell 50.1, Wayland session, 125 % display scaling
- Release build (`cargo build --release`, thin LTO)

## Results (2026-11-05, warm file cache)

Startup to first painted frame, then to the restored document being shown
(`MARKLY_TRACE=1 MARKLY_BENCH_EXIT=1`, three runs each):

| Restored document | First frame | Load (background) | Doc shown | Whole process incl. exit | Peak RSS |
|---|---|---|---|---|---|
| none (empty state) | 113–120 ms | – | – | 0.19–0.21 s | 90 MB |
| 1600×1000 | 117–147 ms | 21–27 ms | 157–199 ms | 0.25–0.27 s | 102 MB |
| 3840×2160 (4K) | 120–129 ms | 87–88 ms | 212–223 ms | 0.31–0.36 s | 154 MB |
| 7680×4320 (8K) | 125–136 ms | 368–395 ms | 501–517 ms | 0.78–0.81 s | 349 MB |

- The window is shown and interactive before the document finishes
  loading. A large image never delays the first frame.
- Shutdown (flush autosave, save settings, stop the writer): 1–11 ms.
- **Idle CPU:** 0 scheduler ticks over 4 s with a document open (sampled
  from `/proc/<pid>/stat`). No continuous repainting.
- Cold start (empty page cache) was not measured separately.

Off-thread costs (`cargo run --release --example bench_load`):

| Stage | 4K | 8K |
|---|---|---|
| PNG decode | 32 ms | 120 ms |
| Texture data prep | 33 ms (premultiplied path); a plain copy for opaque images | 122 ms |
| Thumbnail base (320 px) | 17 ms | 49 ms |
| Fast PNG encode (storing a pasted image) | 11 ms | 33 ms |
| Flatten for copy/export (no annotations) | 61 ms | 172 ms |
| Flatten with 7 annotations (`demo_document`) | 63 ms | 179 ms |

## Fixed along the way

- egui only learns the GPU's maximum texture size after the first frame, so
  documents loaded during startup were downscaled to 2048 px (slow and blurry).
  The app now queries `GL_MAX_TEXTURE_SIZE` from the context at creation: a
  4K restore dropped from about 430 ms to about 90 ms, now at full resolution.
- Texture preparation and the thumbnail downscale now run in parallel, and
  opaque images skip premultiplication.

## Reproducing

```sh
cargo build --release
cargo run --release --example demo_document -- /tmp/m4k 3840 2160
MARKLY_TRACE=1 MARKLY_BENCH_EXIT=1 MARKLY_DATA_DIR=/tmp/m4k /usr/bin/time -v target/release/markly
```

## Known costs and possible future work

- The release binary is about 11 MB, of which roughly 2 MB is fonts (egui's
  defaults plus Noto Sans SemiBold).
- 8K decode dominates the 8K restore. A downscaled preview texture could be
  shown first and the full-resolution one swapped in later.
- Pixelation previews are recomputed on the UI thread while a pixelation
  rectangle is being resized. This is cheap for typical regions, and the cost
  grows with the region's area.
