# Changelog

Every release of Manfred, newest first. Earlier versions were developed privately; 0.19.0 is the first public one.

## v0.19.3 — 2026-10-12

### Changed

- `manfred render` denoises on the GPU with pfx's port of Intel Open Image Denoise, read from the tracer's textures: about 2 s at 4K instead of about 5 s on the CPU. The pixels of a denoised `.png` change, so a plate and its crops come from one version.
- A denoised crop grows by 96 pixels around the rectangle, the new denoiser's reach; it matches the full frame closely rather than bit for bit, and `--denoise off`, `.exr` and `.hdr` crops stay exact.
- `manfred clip --trace` keeps one denoiser across its frames.
- The pfx crates move to pfx 0.26.0's revision.

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
