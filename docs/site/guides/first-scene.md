---
title: Your first scene
description: Write a small recipe, check it, build it into a GLB, draw it live and path-trace it.
sidebar:
  order: 1
---

A recipe is one TOML file. This one is a desk with a box and a brass ball, a camera, a sun and a sky.

## Write the recipe

Save this as `desk.toml`:

```toml
manfred = 1
name = "desk"

[output]
glb = "desk.glb"
sidecar = "desk.json"
sidecar_dialect = "facts"

[[object]]
name = "top"
kind = "plane"
size = [1.2, 0.8]
material = "oak"

[[object]]
name = "box"
kind = "box"
at = [-0.15, 0.05, 0.0]
size = [0.2, 0.1, 0.14]
bevel = { width = 0.01, segments = 4 }
material = "blue"

[[object]]
name = "ball"
kind = "sphere"
at = [0.15, 0.05, 0.05]
radius = 0.05
material = "brass"

[[camera]]
name = "main"
position = [0.0, 0.35, 0.7]
target = [0.0, 0.03, 0.0]
fov_y_deg = 35.0

[sun]
model = "authored"
toward = [-0.45, 0.82, 0.35]
color = [1.0, 0.92, 0.82]
irradiance = 2.2

[sky]
kind = "analytic"
intensity = 0.9

[materials.oak]
family = "wood"
base = [0.42, 0.28, 0.16]
roughness = 0.55

[materials.blue]
family = "lacquer"
base = [0.08, 0.2, 0.5]
roughness = 0.3

[materials.brass]
family = "metal"
base = [0.85, 0.66, 0.35]
roughness = 0.3
metalness = 1.0
```

Units are metres, Y is up, angles are degrees and colours are linear RGB from 0 to 1. `manfred = 1` is the schema version; [the recipe](../../reference/recipe/) has every key.

## Check it

```sh
manfred check desk.toml
```

A valid recipe prints nothing and exits 0. A problem is printed at its line, and the command exits 1. With a negative radius on the ball:

```
$ manfred check desk.toml
desk.toml:27: object[ball].radius: expected a number above 0
```

## Build it

```sh
manfred build desk.toml --out out
```

```
out/desk.glb
out/desk.json
```

`manfred build` writes the GLB and the JSON sidecar the `[output]` table names, here into `out/` because of `--out`. It needs no GPU and no window, and takes a few milliseconds. The build is deterministic: the same recipe gives the same bytes on any machine.

![A terminal: manfred check desk.toml prints valid, manfred build writes out/desk.glb and out/desk.json, and two builds give the same sha256.](../images/build.gif)

The sidecar holds what the GLB cannot: the cameras, the sun and the sky, in the `facts` dialect ([file formats](../../reference/formats/)).

## Draw it live

```sh
manfred shot desk.toml --camera main --size 800x500 --out desk.png
```

`manfred shot` draws one still through the `main` camera with the engine's live renderer, headless, and prints `desk.png`.

## Path-trace it

```sh
manfred render desk.toml --camera main --size 800x500 --out desk-traced.png
```

```
manfred render: 800x500, 37.9 samples a pixel on average, 64 at most of 64, threshold 0.01, seed 0: 36 submissions, the longest 10.29 ms, 177 ms on the GPU
desk-traced.png
```

The path written goes to stdout and the summary to stderr. [Path-trace a still](../render/) covers drafts, crops and the other formats.

## Open it in the window

```sh
manfred gui desk.toml
```

Click the ball, drag the gizmo's arrow, and the recipe file changes under you. [Arrange a scene in the window](../window/) says how.
