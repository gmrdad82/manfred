# Path-traced renders

`manfred render` traces a recipe through pfx's offline path tracer (`pfx_trace`, pfx rev `075fb1b`). It stages the same parts, materials, sky, sun and lights the live renderer draws, so a render and a `manfred shot` of one recipe show one scene.

```
manfred render <recipe> --camera <name> [--spp N] [--threshold T] [--min-samples N] [--draft] [--seed S] [--size WxH] [--equirect|--ortho] [--crop x,y,w,h] [--variant V] [--set key=value]... [--denoise off|on] [--master <file.png>] [--progress plain|json] --out <file>
```

| Option | Default | Meaning |
| --- | --- | --- |
| `--camera` | required | a `[[camera]]` of the recipe |
| `--spp` | the recipe's `[trace] spp`, else 64 | samples per pixel, at least 1: the most any pixel takes (`--samples` is the same flag) |
| `--threshold` | the recipe's `[trace] threshold`, else 0.01 | adaptive sampling: a pixel stops once its noise estimate is under this; `0` runs every pixel to `--spp` |
| `--min-samples` | the recipe's `[trace] min_samples`, else 16 | the samples every pixel takes before it may stop, at least 2 (never more than `--spp`) |
| `--draft` | | a quick look: `--spp 12 --threshold 0.05 --min-samples 6`, whatever the recipe sets; not with those three |
| `--seed` | the recipe's `seed`, cut to 32 bits | the tracer's seed |
| `--size` | 1586x992, or 2048x1024 with `--equirect` | the picture, each side 1 to 16384 |
| `--ortho` | | an orthographic view of the camera's `ortho_height` |
| `--equirect` | | the full sphere around the camera |
| `--crop` | | `x,y,w,h` in the frame's pixels: renders only that rectangle, the same pixels the full frame has there (below) |
| `--variant` | | a `[variants.<name>]` of the recipe |
| `--denoise` | `on` for a `.png` or master, `off` for `.exr` and `.hdr` | `off` writes a `.png` or master from the raw trace, without the denoiser; `on` also denoises an `.exr` or `.hdr` |
| `--set` | | `key=value`, replaces a value the recipe has, after the variant; as often as needed (`docs/recipe.md`, overrides) |
| `--out` | required | `.png`, `.exr` or `.hdr`, by its extension |
| `--master` | | also writes a 16-bit `.png` master of the same finished picture |
| `--progress` | `plain` | `json` gives progress on stderr as JSON lines (below) |

It prints the out file on stdout (and the master after it) and, on stderr, the size, the samples (with adaptive sampling the mean and the most a pixel took), the seed, submissions, the longest submission and the GPU time. A line it cannot parse exits 2; a recipe or camera it refuses, or a failed render, exits 1.

## Adaptive sampling

By default each pixel traces only until it is clean: the tracer (`pfx_trace::Trace::sample_adaptive`, as `pfx render` samples) runs every pixel to `--min-samples` in two rounds, then grows the count by half each round up to `--spp`, and after each round stops every pixel whose noise estimate, and its eight neighbours', is under `--threshold`. Flat, matte and evenly lit surfaces stop early; glossy highlights, soft shadows and fine detail run longer. `--threshold 0`, or `--spp 1`, traces every pixel to `--spp`, as before 0.19.1. C3 in the window renders that way, so its progress counts samples.

A final still is the defaults: adaptive to 64 samples at threshold 0.01 unless the recipe's `[trace]` sets its own (docs/recipe.md), denoised, with the recipe's `[trace] filter_glossy` and `clamp_indirect` against fireflies. `--draft` is a quick look at the same size. `--denoise off --threshold 0 --spp 1024` (or more) is only for a ground-truth reference to compare a denoised final against.

## Progress

Plain progress (the default) prints `progress <done>/<total>` about twenty times when every pixel runs to `--spp`, and, with adaptive sampling, a line every 30 seconds with the fraction and the GPU time so far. `--progress json` prints one JSON object a line on stderr instead, at most one a second while tracing:

| `stage` | Fields beside `stage` and `elapsed_s` (wall seconds since the render began) |
| --- | --- |
| `stage` | none: the recipe is read and staged |
| `trace` | to `--spp` for every pixel: `done`, `total`, `fraction` (0 to 1) and `eta_s` (null before the first sample). With adaptive sampling: `cap`, `threshold`, `gpu_s`, `fraction` and `eta_s`, counted in pixel-samples: the rounds done, the current round by its GPU time, and the rounds left as if every pixel still running ran them, so the fraction only grows and the ETA is an upper bound that falls as pixels stop |
| `write` | none: the picture is being written |
| `done` | `out`, `mean_samples`, `most_samples`, `gpu_s` |

## Crops

`--crop x,y,w,h` traces only that rectangle of the `--size` frame, with the tracer's window (`Trace::set_window`): every pixel keeps the random stream, the ray, the lens shift and the depth of field of its place in the full frame, so a crop is the full frame's pixels there, for a key-down or lit variant of one part of a plate. The same recipe, size, camera, seed, samples and threshold give the same traced pixels as the full frame on one GPU and driver. Around the rectangle the trace runs on margins it then cuts away:

- **The finish** (a `.png` or master): the bloom reads its neighbours, so the rectangle grows by the bloom's reach (its downsample times its radius plus 3: 30 pixels for the graded look), aligned to the bloom's 2×2 blocks, and the finish runs on that; the vignette and the `.png` dither are laid by the full frame's coordinates. The finished crop is the full frame's finish there (with the denoiser off, to the float).
- **Adaptive sampling**: one pixel a round on each side, since convergence spreads through its 3×3 neighbourhood one pixel a round.
- **The denoiser**: at least 96 pixels, Open Image Denoise's tile overlap, with the traced window aligned to 16 pixels of the full frame so the network's pooling grid is the full frame's.

The denoiser sets its exposure from the image it is given, so a denoised crop is close to the full frame there but not bit for bit. Measured on the showcase hero at 3840x2160, two 640x400 crops against the full frame's `.png`: mean 0.04 and 0.03 levels, p99 1 level (the dither), at most 2 at the defaults; mean 0.07, at most 4, with `--draft`. `--denoise off` is exact.

A `.png` or `--master` crop is refused (exit 1) when its finish cannot be worked out on a margin: overlay text, which is placed by the whole frame; grain, a film dither, outlines or any other pass that reads past a margin; or a bloom whose 2×2 blocks do not tile the frame (an odd `--size`). An `.exr` or `.hdr` crop always works: the raw trace, or with `--denoise on` the denoised one.

## The scene it traces

The staging lives in `crates/render/src/staging.rs` and reuses `crates/scene`'s own mapping, the one `manfred shot` and the window draw with (`manfred_scene::{pieces, material, Lighting, Eye}`):

- **Meshes:** every visible part becomes pieces exactly as the live stage cuts them, one per material slot, in world space with degenerate triangles dropped. Each piece is one `stage::Mesh` placed once with the identity transform, as live places it.
- **Materials:** interned by name and by whether a card wears them, in first-seen order, from the app's catalogue and the recipe's `[materials]`, the same `Material` values live binds; a card's copy carries live's content layer (`cards::layered`, blend `Over`). An unknown name is refused with the object's name.
- **Shadows:** `shadow = "only"` pieces are the tracer's `shadow_only`: they cast and are not seen. `shadow = "none"` pieces are placed with `casts_shadow: false`: no shadow ray (sun, sky, emitters, local lights) sees them, while camera and bounce rays still hit them, as live draws them.
- **Sides:** a placement's `two_sided` is its piece's, so a wall without `two_sided = true` is seen through from behind, traced as live culls it.
- **Sun and sky:** `Lighting::of(recipe)` at the recipe's own hour, as the window opens: the `[sun]`, authored or daylight by its `model`, with its `radius_deg`, and the analytic sky or the prepared HDR, room or mix, rotated, exposed and tinted as live gets it.
- **Lights:** each `[[light]]` is a tracer `LocalLight::Point` with live's position, colour, intensity, radius, range (the same reach when `range` is 0) and shadow switch. Emissive materials need nothing: the tracer finds and samples them with MIS (`docs/trace-lights.md` in the engine).
- **Glass shadows:** by default the tracer does not tint a shadow through glass: the sun's and a local light's shadow of a pane are as dark as an opaque occluder's, where live tints them (transmission × base), which is Cycles' rule. A recipe's `[trace] transmissive_shadows = true` (false by default) sets `Detail::transmissive_shadows` on the staged scene, so shadow rays toward the sun, the sky and local lights pass glass and liquid tinted by live's `cast_tint`, and a `manfred render` and a `manfred shot` of the recipe shadow glass the same way. The same key is written as `[trace]` in `scene.toml` when true, where the engine's `stage::scene` sets the same flag. `tmp/tracesides-glass-sun-*.png` showed the difference in the checks.
- **Fireflies:** the recipe's `[trace] clamp_indirect` and `filter_glossy` (both 0, off, by default) set `Detail::clamp_indirect` and `Detail::filter_glossy` on the staged scene, for `manfred render` and a traced `manfred clip` alike, and are written into the scene's `[trace]`, where the engine's `stage::scene` sets the same. The clamp caps each light contribution after the first bounce at that luminance, keeping its hue; the filter makes every vertex after a diffuse bounce at least that rough, so a sharp lobe reached through a diffuse bounce under a small sun stops throwing fireflies while the chrome and glass the camera sees stay crisp. Off keeps every render's bytes (pfx's `docs/trace.md`, "Fireflies").
- **Not traced:** haze. Text objects, cards, cutouts, `face_camera` and mover clips are traced; see below.

## Cards and clips in the tracer

Since v0.18.15 the tracer takes content images, cutouts and the no-shadow flag on each `Placement`. Since v0.18.19 the placement also carries its clip planes, its shadow-only flag and its own alpha cutoff, and `Stage::build` copies them onto the instance, so staging no longer patches the built instances. Staging reads the cards through the live side's own decoding (`manfred_scene::Cards::of`, in `crates/scene/src/cards.rs`, which shares `dress`'s loop), so a card's image is read and decoded once per file and the slot cards share one placeholder:

- **Images:** each card's placement carries its picture as `PlacementContent`, premultiplied in linear light and stored as sRGB bytes exactly as live's atlas holds it, with `uv_offset` 0, `uv_scale` 1 and the crop `[0, 0, 1, 1]`, so the whole image lies on the quad's 0..1 UVs; the tracer gives each distinct image its own slot, so nothing is packed. A slot card carries live's 64×64 grey placeholder. The blend is the card material's content layer, `Over` as live.
- **Cutouts:** a card that live cuts, one whose `alpha_cutoff` (0.5 unless set) is above 0 and whose image has a texel below it, sets `cutout` on its plain quad and `alpha_cutoff` to the authored cutoff; every other card, opaque photos and the placeholder among them, stays a plain content quad, as live keeps it one. With `cutout`, camera, bounce and shadow rays pass where the image's bilinear alpha is below that cutoff, so the card and its shadow keep the image's shape to the texel, the same rule live uses on the one quad. A cutoff of 0, or an image whose alpha is only 0 or 1, gives the same edge in both.
- **`face_camera`:** the card's placement model is `cards::facing` for the render's eye, the turn live gives the same eye: toward the camera's position in perspective and in an equirect, along the view direction in orthographic (`--ortho` turns it as an orthographic eye too).
- **Mover clips:** every piece of a mover carries live's `clips` planes on its `Placement::clip`, which `Stage::build` copies onto the instance. A point past a plane is no hit for camera, bounce and shadow rays, and an emitter's sample past it is dropped, so a part travelling into the shell is cut in the render as in the window.
- **`shadow = "only"`** is the tracer's `Placement::shadow_only`, copied onto the instance by `Stage::build`: its triangles are skipped by camera and bounce rays and seen by shadow rays only, so the part casts and is not seen, as live's shadow-only instance. No gap and no request: "only" and "none" are both traced.

What still differs from live: the tracer samples a card's content at level 0 where live samples the atlas's mips, so a card seen small is sharper in a render. The cut itself agrees: both discard on the content's bilinear alpha at the authored cutoff.

## Text in the tracer

Since v0.18.20 the engine's `pfx_trace::text` draws laid-out text from the MSDF at every hit (the engine's `docs/trace.md`, "Text"). Staging lays the recipe's text through the live side's own `lay` (`manfred_scene::Laid::of`, kept in `Staging::text`), so a render and a `manfred shot` place the same glyphs, and `crates/render/src/text.rs` stages each object one of three ways:

- **A carrier** for free-standing text, for unlit text on a part, and for lit text on a part the next two ways cannot take: `text::lettering`, its `carrier()` quad placed at live's model (with `on`, live's `world(on) · face(on) · LIFT · local · flip`, so it sits 0.5 mm off the face where live draws its quads), a lift kept at 0.5 mm: with v0.19.0's scale-aware ray start the traced text barely moves with it (its mean colours stay within a few levels from 0.5 mm down to 0.1 mm), but live's depth buffer does, and below 0.4 mm the live label and mark flicker between the text and the face under it (IoU against the trace 0.61 at 0.3 mm, 0.02 at 0.2 mm, 0.08 at 0.005 mm); 0.5 mm gives 0.944 and 0.4 mm gave 0.951, too near the 0.94 floor to move, `lettering.content()` as cutout content, and `text::material(lit)`: `Emit` for `surface` text, which shows its colour whatever the light, `Over` a black Lambert for `lit` text, which is its colour as a Lambert base. The cutout is the glyphs' own coverage, discarded below the placement's `alpha_cutoff` (0.5), so an edge stays on the median's 0.5 line at any zoom. Carriers cast no shadow, as live's text casts none.
- **Fitted to the part** for `lit` text on a part whose face `text::fit` maps and whose other triangles don't share the face's UVs (`Fit::elsewhere` is empty), such as a `plane`: the part's own placement carries `fit.content(&lettering)`, and its material becomes a copy with the default content layer (`Over`, strength 1), interned once. A `box` is never fitted: its UVs repeat on every face, so its text goes on a carrier.
- **Over a card's picture** for `lit` text on a card that doesn't face the camera, when the face's other triangles don't share its UVs: `fit.over` keeps the picture and draws the glyphs over it at every hit, sharp at any picture resolution. A card that faces the camera takes a carrier.

Unlit text on a part is a carrier because the content layer has no single blend for it (the part's base under `1 − a` and the text as emission). A part takes one text's content; a second lit text on it goes on a carrier.

**Overlay text** is painted on the CPU at the finish chain's point, as live draws it: the PNG is the denoised film through the finish chain without `Encode`, then `text::overlay` with the overlay quads moved to their screen anchor for the render's size, then `Encode` (`picture::looked_with` given `(&finish, &Overlay::of(&staging.text))`). The EXR and HDR carry the raw trace, so they have no overlay.

## Cameras

From the recipe camera through `Eye::from_camera`, the same eye `manfred shot` uses:

- **Perspective:** `right` and `up` carry the half-angle tangents of the eye's field of view (`fov_y_deg`, or `focal_mm` fitted to the frame), and the lens carries `shift` in the same units the live projection uses. A camera with `fstop` above 0 traces a thin lens: its aperture is `focal_mm / 1000 / fstop` metres across and it focuses on the plane `focus_m` ahead, or at the target's distance when `focus_m` is unset. `focal_mm` is the camera's, or the one its sensor (`sensor_mm`, 36 by default) and field of view give; `Eye::lens` works it out for both drawers. `fstop = 0` is a pinhole, and orthographic and equirect views never blur. Live draws the same lens through the engine's depth of field (`Renderer::set_lens`) for the camera a caller looks through (`Stage::look_through`), with the sensor `dof::Lens::sensor_for_view` gives so its circles of confusion are the tracer's size. It blurs the finished frame in screen space after TAA, so edges and a blurred foreground over a sharp background differ from the traced thin lens, which samples the aperture per ray.
- **Orthographic:** a camera of `kind = "orthographic"`, or any camera with `--ortho`, traces a view `ortho_height` tall and `ortho_height × W/H` wide, with the shift. `--ortho` on a camera without `ortho_height` is refused.
- **Equirect:** `--equirect` traces the full sphere from the camera's position with its basis, as the engine's `docs/trace.md` maps it: the centre looks along the camera's forward, a quarter to the right along its right, the top row along its up. Render it 2:1. The `.hdr` loads back through the engine's `Sky` as it stands.

## Pictures

- **`.png`:** 8-bit sRGB through the recipe's look: pfx's GPU denoiser (Intel Open Image Denoise's RT filter, ported to wgpu compute) over the colour, albedo and normal the tracer averages, read from the tracer's textures on the GPU (`--denoise off` skips it, so the PNG is the raw trace), then the recipe's finish chain (`[look]`, or the graded chain), the overlay text, and sRGB encoding, as the live renderer finishes a frame. It is quantised to 8 bits with an 8×8 ordered dither (pfx-post's `bayer`, ±half a level), so smooth gradients do not band; the same pixels without the dither are what the window and clips write. Under `[look] view = "standard"` the chain is exposure alone, so the PNG is linear radiance times exposure, clamped to [0, 1] and put through the sRGB curve, as Cycles writes its Standard view (`docs/recipe.md`, look).
- **`--master`:** the same finished pixels as the `.png`, before the dither, at 16 bits a channel (RGBA, big-endian, alpha 65535, sRGB tagged), the master to grade or scale from. It works beside any `--out`.
- **`.exr`:** the traced colour, linear, as half floats (RGBA, alpha 1, lossless compression), written on one thread.
- **`.hdr`:** the traced colour, linear, as Radiance RGBE with run-length scanlines (flat below 8 or above 32767 pixels across), the encoding the engine's `Sky::parse` reads.

The EXR and HDR carry the raw trace, neither denoised nor graded.

## Determinism

The same recipe, camera, size, samples, threshold and seed give the same bytes on one GPU and driver. The tracer seeds every pixel by the seed, its absolute sample and its index, and any split of the samples into submissions gives the bytes one dispatch would; adaptive sampling's rounds are fixed by `--min-samples` and `--spp`, and which pixels stop is decided on the GPU from the samples alone. Nothing that shapes the picture reads the clock: only the progress lines' `elapsed_s` and `eta_s` do. A different GPU or driver differs in the low bits; renders are compared by PSNR or region means, never byte for byte across machines.

## Pacing

`Trace::sample_paced` sends one sample at a time through Manfred's own `Pacer`, which cuts each sample into bands of whole 8-row workgroups sized to 3.5 ms with one in flight; since engine v0.18.18 the Pacer fits a fixed cost per submission plus a cost per row, so a latency-bound shader grows out of narrow bands. After each submission the loop adds its GPU time to the engine's `TurnState`, which turns through `turn_reporting` (so only under pgpu; without it the turn is a no-op and the bands stay the same) once 50 ms of work has added up or 400 ms of wall time have passed since the last turn, inside the 0.5 s between turns that pgpu asks for (the engine reads `PFX_TURN_MS` and `PFX_TURN_WALL_MS` to change them). Manfred keeps no rule or clock of its own: the engine's clock is the only one the loop reads, for the cadence and never for the output. `render` also turns after the build, after staging, after the tracer's setup, after the last sample and after the readback, so CPU time between GPU work is never charged to one long gap. `trace` takes the turn function as a parameter, and the loop's tests count its turns on the CPU, the wall rule among them. No submission comes near the 300 ms limit; the run reports its longest.

The run's figures (`Paced`) sum the slice times `sample_paced`'s `Pacer::run` returns, which on this GPU are timestamp pairs around each band (`Timing::Gpu`): GPU time, with no freeze and no turn's wait inside. `Pacer::run_measured` is for a caller that times its own slices, such as an app's live frames, so Manfred has no use for it. `Paced::wall_ms` alone is the Pacer's wall clock, and it holds freezes and turns.

Measured on an RX 9060 XT: an emissive room traced as a 1024×512 equirect gaps between turns at p50 ~55 ms in debug and release alike, the largest one 0.18 s in debug, before the first sample. A heavy scene in a debug build still has phases over 0.5 s that no turn can split: a 122-part desk scene took 1.6 s to build, 0.65 s to stage and 3.3 s in the tracer's setup, against 0.2, 0.05 and 0.17 s in release, because the engine and Manfred's own crates run unoptimised. The root `Cargo.toml` gives the engine crates, naga, wgpu-core and wgpu-hal `opt-level = 2` in debug, which takes the tracer's setup from 3.3 s to about 0.3 s and the staging to about 0.2 s; they rebuild only when a pin moves. Manfred's own crates stay unoptimised so incremental builds stay quick, and that scene's build phase, CPU only and before the first GPU work, is the one left over 0.5 s. A sample costs about 44.5 ms of GPU time at 40-row bands, flat over 1024 samples with the GPU otherwise idle; when pgpu gives queued jobs only a share of the GPU because someone is using the machine, each band is slower, the Pacer narrows the bands to 8 or 16 rows, and the same sample measures 120–165 ms.

Measured again at engine v0.18.18, release, seed 1, under `pgpu run --class clip` with the GPU otherwise idle for every run, the same emissive room at 1024×512 equirect:

| run | submissions | submissions a sample | longest | GPU | GPU a sample |
|---|---|---|---|---|---|
| v0.18.12, 256 spp | 3,459 | 13.5 | 4.25 ms | 11.4 s | 44.5 ms |
| v0.18.18, 256 spp | 3,685 | 14.4 | 7.33 ms | 13.5 s | 52.8 ms |
| v0.18.18, 1024 spp | 12,629 | 12.3 | 7.69 ms | 46.7 s | 45.6 ms |

With the GPU idle the cost is where it was: 512 rows in about 12 submissions is about 42 rows a band, v0.18.12's 40, and the first samples of a run, while the Pacer learns, raise the 256-sample mean. The longest band is now a little over the 6 ms ceiling, the outlier the Pacer measures before it cuts; every turn's `--longest-ms` stays far under the 50 ms pgpu allows. The shared-GPU figure, where v0.18.12's Pacer stayed at 8 to 16 rows, is not re-measured.

## The process line

`pgpu` is an optional GPU queue command. Started outside a queued job (`GPU_QUEUE` unset) with `pgpu` on the PATH, `manfred render` replaces itself, by `exec`, with a line that ends in

```
pgpu run --class clip --as manfred -- <manfred> render <the same arguments>
```

- When `systemd-run` is on the PATH, the line starts it in a user scope with a memory cap, at a lower priority (`nice`), and, when `MANFRED_CPUS` is set and `taskset` is there, on those CPUs (`taskset -c $MANFRED_CPUS`); without `systemd-run` it is `pgpu run` alone.
- Because it is an `exec`, the pid the caller holds is the same process all the way to `pgpu run`, so TERM to it reaches pgpu, which stops the job.
- Inside the job (`GPU_QUEUE=1`) it renders. Without pgpu it says so and renders directly, paced the same way.

## The tracer's floor

`Trace::new` checks the adapter against the engine's tracer floor (15 storage buffers per compute stage, filterable f32, compute). A GPU below it is refused in plain words:

```
manfred render: this GPU is below the tracer's floor, so manfred render cannot trace on it: <what is short>
```

## C3, the window's render

C3 holds the camera, the size and a samples field (default 256, at least 1). Render runs, on a thread, with the window's own executable:

```
<manfred> render <recipe> --camera <name> --size WxH --spp N --threshold 0 --out tmp/manfred/renders/<recipe>-<camera>-<W>x<H>.png
```

`manfred render` runs under `pgpu run --class clip` by its own process line when pgpu is there, so the window wraps nothing. Cancel (or `esc` in C3) sends TERM to that pid; the queued, waiting, running, done and failed states, the result view and the full-size overlay are as before (`docs/cameras.md`).

## Clips

`manfred clip` renders a camera moving along its path (`docs/recipe.md`, cameras), or an object turning under a still camera, to numbered frames, and optionally a video:

```
manfred clip <recipe> --camera <name> --frames N --fps F [--spin <object> [--turns N]] --size WxH --out <dir> [--trace [--spp N]] [--variant V] [--set key=value]... [--video <file.mp4|.webm>]
```

| Option | Default | Meaning |
| --- | --- | --- |
| `--camera` | required | a `[[camera]]` with a `[camera.path]`; a camera without one is refused, except with `--spin`, which refuses a camera with one |
| `--spin` | | the object to turn, with everything parented to it; not with `--timeline` |
| `--turns` | 1 | the turns `--spin` makes over the clip, fractional or negative (the other way); only with `--spin` |
| `--frames` | required | how many frames, at least 1 |
| `--fps` | required | frames a second, 1 to 240; it times the video only |
| `--size` | required | each frame, each side 1 to 16384 |
| `--out` | required | the folder for `frame-0000.png`…, wider than four digits when the last frame needs it |
| `--trace` | | the tracer instead of the live renderer |
| `--spp` | 64 | the tracer's samples a frame; only with `--trace` |
| `--variant`, `--set` | | as `manfred shot` and `manfred render` take them |
| `--video` | | encode the frames to this `.mp4` (H.264) or `.webm` (VP9) with `pcod`, an optional encoder command |

Frame `i` of `N` shows the camera at the path's time for it: `i / N` on an orbit, so a whole number of turns loops without a repeated frame, and `i / (N − 1)` on keys, so the last frame lands on the last key. It prints each finished frame on stdout, and on stderr its GPU time (traced: also the submissions and the longest), then the video. A line it cannot parse exits 2; a camera without a path or a failure exits 1.

- **Spin** (`--spin <object> [--turns N]`, a turntable): the camera from `--camera` and every light stay where the recipe puts them, and frame `i` of `N` turns the object, and every object parented to it at any depth, by `turns × 360° × i / N` about the world Y axis through the object's origin (its `at`, through its parents). Frame `N` would be frame 0 again for a whole number of turns, so the clip loops without a repeated frame; a positive turn is counter-clockwise seen from above, as `rotate = [0, +a, 0]` is. The object may be hidden, such as an empty root that only parents the figure. An unknown object exits 1 naming the recipe's objects; a camera with a `[camera.path]` exits 1 rather than move (spin a still camera, or orbit without `--spin`), and `--spin` with `--timeline` exits 2. The object turns as a whole: no rebuild, the same stage and the same meshes for every frame. Live, each frame only replaces the turned objects' instance matrices (`Stage::preview`, as the window's gizmo moves an object), so an 8-million-triangle master is uploaded once for the clip. Traced, each frame places the turned pieces with that matrix (`Staging::posed`) and hands the clip's one `Trace` the new instance matrices (`Trace::set_instance_transforms`, pfx 2.4's two-level tracer): the bottom levels are built once for the clip and each frame rebuilds only the top level over the moved instances. Text, lights and emitters parented to the object stay put. `clip.txt` names the spin and its turns. Measured (RX 9060 XT, RADV, release, under pgpu), 8 frames at 256×256 of a three-part figure (torso, head, arm) on a hidden root: live, 30 ms a frame after 1.8 s of loading and staging, 6 ms of it on the GPU; traced at 16 samples, 24 to 26 ms a frame with the one trace (4 bottom builds for the clip, one top build a frame), against 63 to 335 ms a frame when every frame staged a new trace, 10 to 16 ms of it on the GPU either way, and the frames the same picture (81 dB PSNR between the two paths).

- **Live** (the default): one stage for the whole clip, built once. Each frame is `manfred shot`'s still of that frame's pose: the same 48 settling frames, scene time from 0, which the renderer takes as a cut, so no history crosses from one clip frame to the next, then the readback. Its pacing is `manfred shot`'s: each settling frame's GPU time goes to the engine's `TurnState`, which turns under pgpu, when there is one, every 50 ms of work or 400 ms of wall time. Frame 0 of an orbit about the camera's own target, or of keys without a key at 0, is `manfred shot` of the camera, byte for byte.
- **Traced** (`--trace`): with pgpu on the PATH, `manfred clip` re-execs itself under `pgpu run --class clip --as manfred` exactly as `manfred render` does (the process line above, with `clip` for `render`), and with the `jobs` feature it publishes a record (`clip <dir>`, stages `frames`, progress in frames, and `video`). It builds and reads the recipe once (`manfred_render::Reel`) and stages one `Trace` for the whole clip at frame 0. Each later frame sets that trace's camera, lens and projection and, through `Trace::set_instance_transforms`, the instance matrices of that frame's pose: the objects a `--timeline` animates, a `--spin`'s turned objects, and `face_camera` cards, which turn toward the eye; nothing is rebuilt when only the camera moves, and an instance whose matrix did not change is not touched. A new trace is staged only when the size or the samples change; lights, emitters and text stay where frame 0 put them, as they did before the trace was kept, since no clip moves them in Manfred's tracer. Each frame then traces through `manfred render`'s own loop, turns included, at the recipe's seed for every frame, as Blender keeps one seed unless asked to animate it. Each PNG is denoised and finished as `manfred render` writes it, without its dither; frame 0's film is `manfred render`'s of the camera at the same samples with `--threshold 0`, byte for byte.
- **Resuming:** a frame is written as `.frame-0000.partial.png` and renamed into place once the PNG is whole, so a frame under its own name was finished. A run skips every frame whose PNG decodes in full (every chunk's CRC through `IEND`) as 8-bit RGB or RGBA of the asked size, and makes the others; a frame cut short by a crash, a stray partial, or a frame of another size is made again. `<dir>/clip.txt` keeps the line that made the frames (recipe, camera, frames, size, timeline, spin and its turns, renderer and samples, variant, sets); a run with a different line into the same folder is refused rather than mixing frames. `--fps` and `--video` are not in it: changing them re-encodes the same frames.
- **Colour:** a frame whose every pixel is opaque, as every frame of a recipe drawn over its sky is, is written as an 8-bit RGB PNG, live and traced alike, so H.264 takes it; a frame with any transparent pixel keeps its alpha as 8-bit RGBA, and then only `.webm` keeps it.
- **Deterministic:** each frame depends on its pose, the recipe and the flags only, never on the frames made before it, so a resumed clip has the bytes of an unbroken one.
- **Video:** after the frames, `manfred clip` hands pcod the frames folder, whose PNG frames it reads in name order at `--fps`:

  ```
  pcod <dir> <video> --fps F --profile best --force
  ```

  so the video has exactly N frames over N/F seconds. Without pcod on the PATH it says in one line that `--video` needs pcod (pito-codecs), keeps the frames, and exits 1. Under `--trace` the encode runs inside the same pgpu job; `--profile best` encodes on the CPU.

**Measured** (RX 9060 XT, RADV, release, under pgpu), a 24-frame orbit at 1920×1080 of the twelve-object table scene Manfred then tested on, the one `tests/golden/table/table.toml` replaces (`[camera.path] kind = "orbit"` about the camera's target, one turn):

- **Live:** 7.2 s for the 24 frames, about 0.28 s a frame after 0.4 s of loading and staging (one frame alone takes 0.67 s, as `manfred shot` does); 48 to 92 ms of measured GPU time a frame. The WebM adds under a second. Frame 0 is `manfred shot`'s PNG byte for byte, and a second run gives the same bytes.
- **Traced** at 64 samples: 2 min 34 s with the WebM, about 6.4 s a frame; 1.5 to 6.6 s of GPU time a frame, 1,500 to 1,830 submissions for a frame of the desk (591 behind the wall), the longest 4.2 to 11.7 ms. At 960×540 and 32 samples a frame is 1.2 to 1.6 s of GPU time; one band there measured 19.5 ms, the Pacer's outlier before it narrows, far under the 50 ms pgpu allows.
- **The back wall** is a one-sided plane at z = −0.42, and the half-turn camera (z = −0.66) stands behind it. Live culls its back face and sees the desk through it; the tracer has no one-sided surfaces, so frames 9 to 15 of the traced orbit see the wall's unlit back, nearly black and cheap (1.5 to 1.8 s). An orbit that stays in front of the wall (`radius_scale` 0.6 or less here) matches in both.

## Tests

- **Spin:** `src/cli/clip.rs` checks the angle a frame turns (0 at frame 0, `turns × 360°` at frame `N`, whole turns back to the identity), that the turn of a hidden root about its origin equals adding to its `rotate` y for the root, its child and its grandchild, that a child's spin turns its own children about the child's origin, the family a spin moves, the still camera it needs, and the spin in `clip.txt`.
- **Clips:** the recipe crate reads and refuses camera paths and places the camera at a time (`crates/recipe/src/path.rs`, `crates/recipe/tests/path.rs`): frame 0, a quarter turn, the last frame of a 24-frame orbit, rising, scaling and easing, keys with and without a key at 0. `src/cli/clip.rs` covers the flags, frame names, the concat list and pcod's line, the process line, the folder's line, and resuming past finished, missing, cut, partial and wrong-size frames. On the GPU (`#[ignore]`): three live frames of an orbit of the table at 320×200, frame 0 byte for byte `manfred shot` of the camera, and frames 1 and 2 made again after deleting them come out byte for byte as before; three traced frames at 160×100 and 4 samples, frame 0 byte for byte `manfred render` of the camera.
- **CPU:** argument parsing and the process line (`src/cli/render.rs`); the staging of three boxes, two materials, a shadow-only part, a point light and a sun against hand-built expectations, and the perspective, orthographic and equirect cameras (`crates/render/src/staging.rs`); RGBE against the engine's own HDR reader, flat and run-length, and EXR round trips of known pixels (`crates/render/src/picture.rs`).
- **GPU** (`#[ignore]`d; run with `cargo test -- --ignored` on a machine with a Vulkan GPU, optionally under `pgpu run`, one at a time with a turn after each), in `crates/render/tests/trace.rs`:
  - a box under a sun renders, two renders with one seed are byte-identical and another seed differs;
  - an emitter-only furnace room (albedo 0.5, no specular, emission `[0.25, 0.5, 0.75]`) keeps its mean at `Le/(1 − albedo)` within 1%;
  - a 1 m square in a 2 m orthographic view over 64 pixels covers 32×32 of them; an equirect from inside a room of coloured walls sees each wall where `docs/trace.md` puts it, and its `.hdr` reads back through `Sky`;
  - the table as an equirect, written as `tmp/render-equirect.hdr` and previewed through the graded look as `tmp/render-equirect.png`.
- **Cards** (`crates/render/tests/tracecards.rs`): on the CPU, the placements of a recipe with an image card, a cutout card, a slot card, a `face_camera` card, a mover-clipped box and a `shadow = "none"` box carry the right image, crop, cutout, model, `casts_shadow`, clip planes, shadow-only flag and alpha cutoff, and a perspective eye turns the card toward its position; the table through its lens and as an equirect places exactly as `Placement::new(mesh, IDENTITY, material)` with no content slots and no planes. On the GPU, both trace to the same bytes as that plain staging at one seed, and the card scene, traced orthographic at 256 samples (`tmp/tracecards-traced.png`) beside its `manfred shot` (`tmp/tracecards-live.png`), shows the photo, the backdrop through the cut, the placeholder, the clipped slider, the turned sign, no shadow under the no-shadow box and a shadow under its twin, each region's mean (7×7 pixels, 8-bit, after the finish) within 24 levels of live. Measured on an RX 9060 XT: the photo, the sprite, the backdrop through its cut, the placeholder, the clipped slider and the background past its plane, the sign and its turned edge, and the backdrop under the no-shadow box all within 1 level of live (the backdrop there reads 96.6 in luma against 97.3 in the open); the one region apart is the solid box's shadow, 21.8 levels darker in blue traced (49 against 71), live's shadow fill being lighter than the traced sky and bounce.
- **Text** (`crates/render/tests/tracetext.rs`): on the CPU, a recipe with unlit and lit free-standing text, lit text on a box, lit text on a plane and an overlay title stages the free-standing words and the box's word as carriers (cutout `ContentKind::Text`, `text::material(lit)`, live's model, no shadow, the placement's `alpha_cutoff`; the box's other ten triangles share its UVs), fits the plane's word to the plane (`fit.content(&lettering)`, the plane's material with the default content layer), and letters lit text over a card's picture at both 1024² and 16² (`fit.over`); the same words stage the same way for a near and a far camera; the overlay paints only inside its box and an empty overlay leaves the PNG's bytes alone; the table (`tests/golden/table/table.toml`), seen through its lens and as an equirect, lays no text and stages the same meshes, placements and materials, with no text slots. On the GPU, the scene traced at 800×500, 128 samples (`tmp/tracetext-traced.png`) against its `manfred shot` (`tmp/tracetext-live.png`), each object's text being the pixels that change by more than 24 levels when the text is added, in both: IoU at least 0.94 per object and the mean colour where both cover within 12 levels. A close-up of the unlit word at 8× its shot size, without the denoiser, is `tmp/lettering-zoom.png`. Measured on the printed path, before this change, on the RX 9060 XT: the unlit word IoU 0.967 (7.3 levels apart), the lit word 0.984 (2.7), the box's word 0.926 (3.7), the plane's word 0.953 (5.8), the overlay 1.000 (0), and the frame 2.834 ms a sample against 2.292 ms with no text. Measured again on the lettering path, same pin and GPU: the unlit word IoU 0.984 (1.9), the lit word 0.992 (4.4), the box's word 0.944 (5.4), the plane's word 0.958 (0.8), the overlay 1.000 (0), and the frame 2.849 ms a sample against 2.282 ms with no text. The close-up is 1400×408, the word 38 px tall in the shot, and its edges stay hard.
  - `src/cli/render.rs` traces the table through `manfred render` at 64 samples beside `manfred shot` (`tmp/render-table.png`, `tmp/render-table-live.png`) and prints the mean-luminance ratio, live over traced, over the frame and per region of a 7×5 grid.

## Measured (RX 9060 XT, RADV, under pgpu)

- **The table scene of the time** (camera `main`, 1586×992, 64 samples, seed 7): 2,018 submissions, the longest 6.09 ms, 6.85 s on the GPU. Mean luminance, live over traced, of the two PNGs as they are written: 0.878 over the frame; per region of a 7×5 grid, median 0.869, from 0.374 to 0.968. The trace is brighter almost everywhere by the bounce light only it carries. The two outliers are the slab at the bottom right, whose screen live draws as blue content and the tracer as its plain material (0.374), and the region under the ball (0.514).
- **The furnace room** at 64×64 and 64 samples: mean `[0.49989, 0.99978, 1.49968]` against `[0.5, 1.0, 1.5]`.
- **Orthographic:** the 1 m square lights exactly 1,024 of 4,096 pixels; in perspective at 30° from 5 m, 576 against 23.89² = 571.
- **That scene as an equirect** at 1024×512 and 32 samples: 59 submissions, the longest 5.55 ms, 174 ms on the GPU.
- **The neutral scenes** (`tests/scenes.rs`, `#[ignore]`): the cards, toybox and table goldens and both showcase scenes are drawn live and traced at 8 samples, and a generator's materials trace to the same bytes as the same materials written by hand. Parity against an app (its live renderer, Blender or Cycles) is the app's to prove now, with its own scenes.
