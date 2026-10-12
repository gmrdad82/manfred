# Changelog

Every release of Manfred, newest first. Earlier versions were developed privately; 0.19.0 is the first public one.

## v0.19.5 — 2026-10-12

### Added

- `manfred render --crop` writes a `.png` or `--master` through a finish with bloom and vignette: it traces and finishes a margin around the rectangle (the bloom's reach, aligned to its 2×2 blocks), lays the vignette by the full frame's coordinates, then cuts the rectangle out. A frame whose sides are odd, overlay text, grain, a film dither and outlines are still refused for a `.png` or master crop.
- `--denoise on` denoises an `.exr` or `.hdr` too, a crop included; without it they stay the raw trace, as before.

### Changed

- A denoised crop's traced window is aligned to 16 pixels of the full frame, the denoiser's pooling grid. Measured on the showcase hero at 3840x2160, two 640x400 crops against the full frame's `.png`: mean 0.04 and 0.03 levels, p99 1 level, at most 2 at the defaults; mean 0.07 at most with `--draft`. With `--denoise off` a finished crop equals the full frame. Full frames render exactly as in v0.19.4.

## v0.19.4 — 2026-10-12

### Added

- A recipe's `[trace]` takes `spp`, `threshold` and `min_samples`, the adaptive sampling `manfred render` uses for it; `--spp`, `--threshold` and `--min-samples` still override them, and `--draft` ignores them.

### Changed

- The pfx crates move to a revision whose tracer splits a dense band of the frame into column pieces instead of dropping to one lane, so 4K frames with large busy areas trace several times faster. The traced pixels are unchanged.

## v0.19.3 — 2026-10-12

### Changed

- `manfred render` denoises on the GPU with pfx's port of Intel Open Image Denoise, read from the tracer's textures: about 2 s at 4K instead of about 5 s on the CPU. The pixels of a denoised `.png` change, so a plate and its crops come from one version.
- A denoised crop grows by 96 pixels around the rectangle, the new denoiser's reach; it matches the full frame closely rather than bit for bit, and `--denoise off`, `.exr` and `.hdr` crops stay exact.
- `manfred clip --trace` keeps one denoiser across its frames.
- The pfx crates move to pfx 0.26.0's revision.
- Measured on an RX 9060 XT, release build, the showcase workshop's hero camera at 3840x2160 with the GPU to itself: a final at the defaults in 39.5 s end to end (37.4 s on the GPU, 118.3 samples a pixel on average), from 57.4 s in v0.19.2; a draft in 6.5 s (5.1 s on the GPU), from about 12 s.

## v0.19.2 — 2026-10-12

### Added

- `manfred render --crop x,y,w,h` renders one rectangle of the frame with the same pixels the full frame has there, for variants of one part of a still.

### Changed

- `manfred render --progress json` gives a fraction and an ETA with adaptive sampling too, from the tracer's rounds.
- The pfx crates move to a newer revision whose tracer starts every trace at full width, several times faster on short renders, with the same pixels.

## v0.19.1 — 2026-10-12

### Added

- `manfred render` samples adaptively: each pixel stops once its noise is under `--threshold` (0.01), after `--min-samples` (16), with `--spp` as the most any pixel takes; `--threshold 0` runs every pixel to `--spp`.
- `manfred render --draft` for a quick look: 16 samples at most, threshold 0.05.
- `manfred render --master <file.png>` writes a 16-bit PNG master of the finished picture.
- `manfred render --progress json` prints progress as JSON lines on stderr, with the fraction and an ETA when every pixel runs to `--spp`, and the samples a pixel took when it ends.

### Changed

- `manfred render` defaults to 128 samples at most, adaptive and denoised.
- `manfred render` dithers its 8-bit PNG and writes its PNGs with fast compression.
- The window's render keeps a fixed sample count, so its progress still counts samples.
- GPU tests run in pgpu's clip class.

## v0.19.0 — 2026-10-08

### Added

- The code is open source under MIT, with NOTICE, TRADEMARKS.md, a code of conduct and a security policy.
- `manfred gui --clip` films the offscreen window playing a tape into an x264 master, a poster and a manifest.
- `bin/gate --public` and CI run the gate that needs no GPU, a licence check included.

### Changed

- Command-line errors follow clap's form: a short error on stderr and exit 2, and a bare `manfred` prints that error.
- Every dependency comes from a public source over https, the pfx crates from pfx `v0.25.0`.
- Scenes are written and opened through pfx-scene, and crash reports and job progress go through pfx-report.
- The root package's `jobs` and `report` features are gone.

### Fixed

- A recipe opened from a folder's picker saves its edits.
