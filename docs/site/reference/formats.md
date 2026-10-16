---
title: "File formats"
description: "The files and JSON outputs Manfred reads and writes, and the exit codes its commands promise."
sidebar:
  order: 3
---

## The recipe

A recipe is one TOML file, schema 1, starting with `manfred = 1`. [The recipe](../recipe/) is the whole schema. A reader refuses a newer schema; an unknown key is an error.

## The GLB

`manfred build` writes glTF 2.0 binary. Each `[[object]]` is a node named exactly as the object, and each material slot a glTF material named as the recipe's material, so an app finds both by name. `asset.generator` is `manfred <version>`. A mover's parts carry their group in the node's extras. A material with texture maps carries them as glTF textures, their PNG or JPEG images embedded in the binary chunk; [the recipe](../recipe/) says which. An object with `lod` adds its levels of detail as nodes `<name>.lod1`, `<name>.lod2`, … outside the scene, listed in the object's `MSFT_lod` extension, each with `extras.lod` (`of`, `level`, `triangles`, `target`, `error`), and a baked normal map as a PNG in the binary chunk (the recipe guide's "Levels of detail"). The bytes depend on the recipe and its inputs alone: the same recipe gives the same GLB on any machine.

## The sidecar, `facts` dialect

With `sidecar_dialect = "facts"` the sidecar is JSON with sorted keys, in the recipe's own units and axes (metres, Y up, degrees, linear RGB):

```json
{
  "cameras": [
    {
      "fov_y_deg": 35.0,
      "fstop": 0.0,
      "name": "main",
      "orthographic": false,
      "position": [0.0, 0.35, 0.7],
      "shift": [0.0, 0.0],
      "target": [0.0, 0.03, 0.0],
      "up": [0.0, 1.0, 0.0]
    }
  ],
  "sky": { "intensity": 0.9, "rotation_deg": 0.0 },
  "sun": {
    "color": [1.0, 0.92, 0.82],
    "irradiance": 2.2,
    "radius_deg": 0.27,
    "toward": [-0.45, 0.82, 0.35]
  }
}
```

| Key | Holds |
| --- | --- |
| `cameras` | a list in file order: `name`, `orthographic`, `position`, `target`, `up`, `shift`, `fstop`, and whichever of `fov_y_deg`, `focal_mm`, `sensor_mm`, `focus_m` and `ortho_height` the recipe gives |
| `sun` | `toward`, `radius_deg`, `color`, `irradiance` |
| `daylight` | `hour`, `day`, `latitude`, `heading`, for a daylight sun |
| `sky` | `intensity`, `rotation_deg`, and `balance` and `room` when set |
| `plate` | `width`, `height` |
| `movers` | by group: `pivot`, `axis`, `travel`, `clip`, `clip2` |
| `look` | `exposure`, `grade` |

A key the recipe does not set is left out. The recipe's `[sidecar]` table and what generators return are merged over it. `manfred::runtime::facts` reads it back. An app's own dialect writes whatever its writer returns ([your app's own generators](../../guides/app-generators/)).

## The bake recipe

With `[output] bake`, `manfred build` writes a `pfx bake` recipe in the engine's `bake.toml` schema, and `bake-materials.toml` beside it with every material the GLB names. Every path in it is relative to its folder. [The recipe](../recipe/) says what each key becomes, and [pfx](https://docs.pitomd.com/pfx/) bakes it.

## The scene file

With `[output] scene`, `manfred build` writes the engine's runtime scene, a `*.scene.toml` ([pfx-scene](https://docs.pitomd.com/pfx-scene/)). `manfred gui` opens one too, read only, and reloads it as it is saved.

A recipe's shot effects (spot and parented lights, particle systems, flare, motion blur, outdoor haze, wet materials, a LUT and a sound file) pass into the scene under pfx-scene 1.1.0's keys; [the recipe](../recipe/) lists them under "Shot effects".

## `manfred render --progress json`

One JSON object a line on stderr, at most about one a second while tracing. Every line has `stage` and `elapsed_s` (wall seconds since the render began).

| `stage` | Other fields |
| --- | --- |
| `stage` | none: the recipe is read and staged |
| `trace` | adaptive: `cap`, `threshold`, `gpu_s`, `fraction` (0 to 1, only grows) and `eta_s` (null before the first round; an upper bound that falls as pixels stop). Every pixel to `--spp` (`--threshold 0`): `done`, `total`, `fraction`, `eta_s` |
| `write` | none: the picture is being written |
| `done` | `out`, `mean_samples`, `most_samples`, `gpu_s` |

## `manfred diff --json`

```json
{
  "materials_differ": false,
  "nodes": [
    {
      "marks": [
        "bounds [0.1000 -0.0000 -0.0000]..[0.2000 0.1000 0.1000] -> [0.0700 -0.0300 -0.0300]..[0.2300 0.1300 0.1300]",
        "area 0.031388 -> 0.080353"
      ],
      "name": "ball"
    }
  ],
  "only_a": [],
  "only_b": [],
  "order_differs": false
}
```

- `nodes`: each node in both files that differs, with one line a difference in `marks`;
- `only_a`, `only_b`: node names in one file alone;
- `order_differs`: the same nodes in another order;
- `materials_differ`, with `materials_a` and `materials_b` when it is true;
- `render`, with `--render`: `psnr_db` (a number, or `"inf"` when the pictures are identical), `floor_db`, and the paths of `a`, `b` and `difference`, the three PNGs it drew.

## `manfred route --json`

One JSON object on stdout. `list` prints `recipe` (the path as given) and `routes[]`; `set`, `add` and `remove` print `recipe` and the edited `route`, as the file holds it after the edit. A route is `name`, `closed`, `points` (`[x, y, z]` in metres) and `length` (metres along the curve), with `length` null and a `problem` sentence when its points do not make a route.

```json
{"recipe":"street.toml","routes":[{"closed":false,"length":81.87448581919321,"name":"main","points":[[0.0,0.0,0.0],[0.0,0.0,40.0],[12.0,0.0,80.0]]}]}
```

## A clip's folder

`manfred clip --out <dir>` writes:

- `frame-0000.png`, `frame-0001.png`…: 8-bit RGBA PNGs, wider than four digits when the last frame needs it. A frame under its own name is whole;
- `clip.txt`: the line that made the frames (recipe, camera, frames, size, timeline, spin and its turns, renderer and samples, variant, sets). A run with a different line into the folder is refused;
- `frames.ffconcat`: an ffconcat list holding each frame for its share of a second, to the microsecond, for `--video` or any encoder.

## A shoot's folder

`manfred shoot <recipe> --out <dir>` builds the recipe into its `[output]` GLB and scene, then runs `pfx shot` on the scene through `pgpu run --class clip --as manfred` (directly, with a note, when pgpu is not on the `PATH`), with `--from`, `--frames`, `--fps` and `--size` from `[timeline]` and `--range`, `--camera camera.<name>` and `--draft` or `--final`. It writes:

- `frame-00000.png`, `frame-00001.png`…: the timeline's frame numbers, so a `--range` writes only its own. A frame already there and whole is skipped: a shoot stopped part way is run again with the same line and renders only the missing runs of frames (with `--final`, pfx warms up to 8 frames before each run);
- `sound.wav`: the scene's `sound` mixed by pfx, written by a run that covers the whole timeline from frame 0 when the scene names a sound;
- `pfx-<from>.json`: pfx's own `manifest.json` of the run that started at frame `<from>`;
- `shoot.json`: `shoot` (1), `recipe`, `scene`, `glb`, `camera` (the node), `renderer` (`live`), `quality`, `fps`, `size`, `range` (`[a, b]`, both included), `frames` (in the range), `skipped` (already there), `files` (the range's frames on disk), `wav` (`sound.wav` or null), `sound` (whether the scene names one), `pfx` (its version) and `runs[]`: `from`, `frames`, `manifest`, `written`, `quality`, `wall_ms`, `gpu_ms` and `warnings`, from pfx's manifest, for every run into the folder.

With `--json` it prints one line a frame as pfx reads it back, `{"frame", "of", "time", "gpu_ms", "wall_ms"}`, then `shoot.json` as the last line. Manfred encodes nothing: pfx film and pcod make the video.

## A window clip

`manfred gui --clip <dir> --tape <file>` writes three files named `<tape>-<W>x<H>-<fps>fps`:

- `.master.mp4`: x264 High, CRF 14, 8-bit 4:2:0, BT.709 limited range, a keyframe every two seconds;
- `.poster.png`: the poster frame, 8-bit sRGB;
- `.json`, the manifest: `tool` (`name`, `version`), `kind` (`window-clip`), `target` and `tape` as given, `tape_sha256` (of the tape's bytes), `params` (`size`, `fps`, `poster`, `scale`, `framed`, `window` (points, or null), `pixels_per_point` and `drawn`, the window's pixels inside the frame), `frames` (`master`, the frame count; `seconds`; `poster`, the poster's frame), `outputs` (the formats in words), `files[]` (`file`, `role` `master` or `poster`, `bytes`, `fps`, `size`, and the poster's `frame`) and `encodes[]` (`file`, `role`, `fps`, `size`, `args`). Readers ignore keys they don't know.

With `--frames` it writes a folder `<tape>-<W>x<H>-<fps>fps/` of `frame-00000.png`… (8-bit sRGB PNGs of the whole frame, five digits) and the `.json` manifest beside it, whole only once every frame is written: `kind` `window-frames`; `tool`, `target`, `tape`, `tape_sha256` and `params` as above, without `poster`; `frames` (`count`, `seconds`, `folder`, `pattern` `frame-%05d.png`); `outputs`; and `files[]`, one a frame in order: `file` (`<folder>/frame-00000.png`), `role` `frame`, `frame`, `bytes`, and the tape `step` it is in (counting from 0 over the tape's steps and waits) with its `line` in the tape, both null for frame 0, the window before the tape's first step.

## Exit codes

| Code | When |
| --- | --- |
| 0 | done; `manfred diff` found no difference beyond tolerance |
| 1 | a refusal after the line parsed, as one line on stderr, `manfred <command>: …` or `<file>:<line>: <key>: …`; a failed build, render or encode; `manfred diff` found a difference |
| 2 | a line it cannot parse: clap's short error on stderr, then the usage and `For more information, try '--help'.` |
