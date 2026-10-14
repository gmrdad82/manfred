---
title: Build GLBs from recipes
description: Build a recipe, its variants and one-off overrides into GLB and sidecar files, and compare two builds.
sidebar:
  order: 2
---

`manfred build` turns a recipe into the files a game or an app loads. It is CPU only: no GPU, no window, no clock and no randomness beyond the recipe's `seed`, so the same recipe gives the same bytes on any machine and a changed GLB in a diff is a changed scene.

## What a build writes

The recipe's `[output]` table names the files, relative to the recipe:

```toml
[output]
glb = "scene/desk.glb"
sidecar = "scene/desk.json"
sidecar_dialect = "facts"
bake = "assets/bake.toml"
scene = "scene/desk.scene.toml"
```

- `glb`: the meshes, their materials by name, the node tree. Without it the build writes `<name>.glb`.
- `sidecar` with `sidecar_dialect`: the cameras, sun, sky, look and movers as JSON. `facts` is Manfred's own dialect; an [app's build binary](../app-generators/) can register its own.
- `bake`: a `pfx bake` recipe for the scene, with the same sky, sun and files.
- `scene`: the engine's runtime scene file, which the window and pfx open.

`--out <dir>` writes every file into one folder instead:

```
$ manfred build showcase/workshop.toml --out out
out/workshop.glb
```

Each path written is printed on stdout.

## Variants

A variant is a named change to the recipe, built into its own files:

```toml
[variant.bare]
drop = ["slat*", "wall_*", "vase"]
set = { "sun.irradiance" = 0.0 }
remove = ["light.lamp"]
```

```sh
manfred build desk.toml --variant bare     # writes desk-bare.glb and desk-bare.json
manfred build desk.toml --all-variants     # the recipe, then every variant in name order
```

`drop` removes objects by glob; `set` and `remove` take dotted paths, with an entry of an array of tables addressed by its name (`camera.main.fov_y_deg`, `light.lamp`). A variant's name goes before the last extension of each file name, unless `[variant.<name>.output]` names its files.

## One-off overrides

`--set key=value` replaces a value the recipe already has, for that run only; the file is never written. `build`, `shot`, `render` and `clip` take it, as often as needed:

```
$ manfred build desk.toml --set object.ball.radius=0.08 --out big
big/desk.glb
big/desk.json
```

The value takes the kind of the one it replaces. A key the recipe does not carry is refused in one line, exit 1; a missing `=` is a line Manfred cannot parse, exit 2.

## Problems

`manfred check` and `manfred build` report a problem at its line, naming the key, and exit 1:

```
$ manfred build showcase/workshop.toml --variant nonesuch --out out
manfred build: showcase/workshop.toml: variant.nonesuch: no such variant, the recipe has no variants
```

## Compare two builds

`manfred diff` compares two GLBs by structure, node by node:

```
$ manfred diff out/desk.glb big/desk.glb
~ ball
    bounds [0.1000 -0.0000 -0.0000]..[0.2000 0.1000 0.1000] -> [0.0700 -0.0300 -0.0300]..[0.2300 0.1300 0.1300]
    area 0.031388 -> 0.080353
```

It exits 0 with `no differences beyond tolerance` when they match and 1 when they differ. `--json` prints the same report as JSON ([file formats](../../reference/formats/)). `--render --recipe <recipe>` also draws both GLBs alike through the recipe's camera and compares the pictures by PSNR, failing under `--floor` (35 dB by default):

```sh
manfred diff out/desk.glb big/desk.glb --render --recipe desk.toml --camera main
```
