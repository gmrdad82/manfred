# Changelog

Every release of Manfred, newest first. Earlier versions were developed privately; 0.19.0 is the first public one.

## v1.3.0 — 2026-10-16

### Added

- A `mesh` object keeps its glTF morph targets and writes them into the built GLB with their names and weights, so a game can blend them live.
- `morph = { smile = 0.6 }` on a `mesh` object sets its morph weights, also per variant and with `--set object.<name>.morph.<target>=<weight>`; `shot`, `render` and the window draw the mesh posed, and an unknown target name is refused.

### Changed

- Manfred uses pfx-scene 1.0.1, the stable release of the same format.
- Breaking for an app's build binary: `manfred_glb::Node` gained a `targets` field, so a struct literal needs `targets: None`.

## v1.2.0 — 2026-10-16

### Changed

- Manfred runs on pfx 2.0.0, the release of pfx's security and performance audit; renders and the window's frames take the same time as in 1.1.0.
- Changing the hour of a scene whose sky doesn't change with the time of day (a room sky in the window, or an app's runtime with its fixed sky) no longer copies and uploads the sky again.
- The README and docs give 1.1.0's 4K render times from an A/B run against 1.0.1: final 7.9 s, draft 3.8 s.

## v1.1.0 — 2026-10-15

### Added

- The README's Licence section shows Manfred's lockup, the dog mark and the wordmark.

### Changed

- A GPU-denoised render reads back only its colour and writes its file without whole-frame copies.
- Clip frames, window shots and reels encode their PNGs at the fast level, so long clips write several times faster.
- Resuming a clip checks each finished frame from its PNG header instead of decoding it.
- The window draws its floor grid from a memo instead of testing every grid line against every part on each frame.
- Edits no longer re-read the sky HDR or card images that have not changed on disk, and changed meshes hash faster.
- Hull proxies build faster.

### Fixed

- A recipe's mesh counts (segments, rings, ring, grid, bevel and profile segments, the detail UV gutter) above 4096 are refused when it is read, instead of exhausting memory.
- Many large included files no longer slow reading a recipe down to a crawl.
- An imported glTF with out-of-range indices, short UVs or a node cycle no longer crashes or hangs `manfred build` or `manfred summary`.
- The scene runtime no longer crashes, hangs or runs out of memory on a GLB with bad indices or a node cycle, a sky room wider than the engine allows, or a huge motes box.
- A GLB past 4 GiB is refused instead of written corrupt.
- A card image too large for the atlas is refused from its header before it is decoded.
- A window tape stops planning frames as soon as it passes the frame cap.
- `manfred clip` hands pconv absolute paths, so an output name starting with `-` is never read as a flag.

## v1.0.1 — 2026-10-14

### Added

- Each release now carries `docs-vX.Y.Z.tar.gz`, Manfred's section of the PITO MD docs site: an overview, install, seven guides (a first scene, building GLBs, the window, path-traced stills, camera clips, filming the window headless, an app's own generators), the file formats, the command line, the recipe schema, this changelog and the API reference.

### Fixed

- `manfred render --help` now gives the draft's real settings, 12 samples at most and 6 first; it still said 16 and 8 after 0.19.8 lowered them.

## v1.0.0 — 2026-10-14

Manfred 1.0.0: the same scene tool as 0.19.8, now with a prebuilt Linux binary and a README that shows what it does.

### Added

- Each release now carries a ready-to-run Linux x86_64 binary, `manfred-vX.Y.Z-linux-x86_64.tar.gz`, with a `SHA256SUMS` file, so installing is one download instead of a build from source.

### Changed

- The README shows every feature with its own clip or chart: building recipes headless, editing in the window, path-traced renders and camera clips, with the speed and size measured: 39.8 MB against Blender 5.2.2's 1,236.5 MB, a recipe to GLB in 29 ms, the window's first frame in 0.88 s and a 4K path-traced final in 7.5 s.
- The brand reads PITO MD in the README and the trademark notice.
- The version is 1.0.0; scenes render and build exactly as in 0.19.8, and only the version written into each GLB changes.

## v0.19.8 — 2026-10-12

### Changed

- Renders keep the Sobol sampler and spend its gain on speed: a final now adapts to 64 samples a pixel at most (from 128) and a draft to 12 (from 16, at least 6), where Sobol's noise matches what the PCG sampler gave at 128 and 16. A `--spp` or recipe `spp` above 256 sizes the Sobol table to it. Measured on an RX 9060 XT, best of three: the showcase workshop's 4K final 7.7 s, 6.2 s on the GPU (v0.19.6 12.1 s and 10.7 s); its 4K draft 2.6 s, 1.4 s on the GPU (the same); a 640x400 crop of the showcase hero at 4K 1.2 s, 0.9 s on the GPU (2.1 s and 1.8 s). Pixels differ from v0.19.7's.
- The pfx crates move to v0.28.1, whose Sobol sampler shifts its sequence per pixel, so a render's mean matches the PCG sampler's.

### Fixed

- v0.19.7's Sobol sampler cost 10 to 20% more GPU time a sample at the old caps and stopped no sooner (118.3 samples a pixel for the workshop's final under both samplers), so v0.19.7 rendered about 10% slower than v0.19.6; its note that adaptive renders stop sooner did not hold on these scenes.

## v0.19.7 — 2026-10-12

### Changed

- The pfx crates move to v0.28.0, whose tracer samples with Cycles' tabulated Sobol sequence instead of PCG: the same noise in about half the samples, so adaptive renders stop sooner. Pixels differ from v0.19.6's; on scenes lit by small or narrow lights the mean sits up to 0.15% lower.
- Manfred reports to no one: it sends no crash reports, writes no job records and gives the window no heartbeat. `--report-test` is gone, and pfx-report left the dependencies.

## v0.19.6 — 2026-10-12

### Changed

- The pfx crates move to v0.27.0, on wgpu 30, and the window to egui 0.36.2. The tracer takes the GPU's hardware ray queries when the adapter offers them (on Vulkan, `VK_KHR_ray_query`) and its own BVH otherwise. Measured on an RX 9060 XT, best of three, same samples a pixel as v0.19.5: the showcase workshop's 4K final 12.1 s (from 39.2 s), 10.7 s on the GPU (from 37.4 s); its 4K draft 2.6 s (from 6.6 s); a 640x400 crop of the showcase hero at 4K 2.1 s (from 8.1 s). The first render on a machine waits a few seconds while the driver compiles the ray-query shader; its cache keeps it after that.

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
