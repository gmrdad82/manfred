---
title: "glTF in and out"
description: "Import a glTF 2.0 binary into a .manf document with detected facts, and export a document or a recipe as a standalone GLB."
sidebar:
  order: 8
---

Manfred reads any standard glTF 2.0 binary, a Blender export included, and writes GLBs other tools open: Blender, a game engine, a viewer. Both commands are CPU only, except `--thumb`.

## Import

```
manfred import <file.glb> (--as <kind>/<name> | --out <x.manf>) [--tags <tag,tag>] [--thumb] [--json]
```

```
$ manfred import coupe.glb --as vehicle/coupe --tags car,red
library/vehicle/coupe.manf: vehicle/coupe, 4 nodes, 1 wheels, 1 light anchors, 9480 bytes
```

It writes a `.manf` document:

- **GLTF and GBIN** hold the file's JSON and BIN chunks as they are, so the node hierarchy, the materials and their KHR extensions, the textures, the morph targets and the animations all stay.
- **RCPE** is a recipe with one object, `kind = "mesh"`, `path = "@gltf"`, the document's own glTF.
- **FACT** holds what Manfred detects: `size`, `min` and `max` from the bounds (metres, Y up), `nodes`, `triangles`, `wheels` from nodes named `wheel*` (`wheel_fl` is `fl`: its `centre`, its `radius` and its `axle`, the thinnest axis of its bounds), `anchors` from nodes named for a light, lamp, head or tail (`kind` `head`, `tail` or `lamp`, a `position` and the node's `-Z` as `direction`), and `kind`, `name`, `tags` and the `licence` line.
- **NODE** maps each node, as `<name>.<node>`, to its glTF index.
- **THMB**, with `--thumb`, a 256x256 PNG drawn headless on the live renderer.

`--as <kind>/<name>` writes `library/<kind>/<name>.manf` under the current folder, `--out` writes where it says and takes the file's name. It refuses a file that is not a standalone glTF 2.0 binary (a buffer or image in an outside file), and, when `gltf_validator` (the Khronos glTF Validator) is on the PATH, a file it reports errors in. `--json` prints one object: `out`, `kind`, `name`, `bytes`, `nodes`, `thumb`, `validator` (`passed` or `absent`) and `facts`.

## Export

```
manfred export <file.manf|recipe> --glb <out.glb> [--only <object>] [--json]
```

A `.manf` exports its own glTF. A recipe is built first: one object (the recipe's only one, or `--only`) exports as a node tree with real pivots, the library's form, and a scene of many keeps its objects' parents as the node hierarchy. Either way the GLB is standalone and carries no Manfred extras: every `extras` is dropped but a mesh's `extras.targetNames`, the morph target names glTF tools read, and so is every extension not named `KHR_`, `EXT_` or `MSFT_`. `--json` prints `source`, `glb`, `bytes` and `nodes`.

```
$ manfred export library/vehicle/coupe.manf --glb coupe.glb
coupe.glb: 4 nodes, 9480 bytes
```
