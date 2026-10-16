---
title: "The object library"
description: "Keep finished objects as standard glTF models with a sidecar, list and check them, and place them in a recipe by name."
sidebar:
  order: 8
---

An object library is a folder of finished objects, each a standard glTF 2.0 model with a small `object.toml` beside it, which any recipe places by name. A model exported from Blender (File > Export > glTF 2.0) works as well as one Manfred writes: its hierarchy, materials and textures, morph targets and animations come in.

```
library/
  index.json
  car/sports/model.glb
  car/sports/object.toml
  car/sports/thumb.png
```

## Place an object

```toml
[[object]]
name = "hero"
kind = "library"
object = "car/sports"
```

The recipe finds the library through its top-level `library = "<path>"`, or else the nearest `library/` holding an `index.json` from the recipe's folder up. Every node with a mesh is a piece `<object>.<node>`, the root `<object>.body`, so a `[[follow]]` spins `hero.wheel_rl` about its axle as it does a generated car's. The recipe reference's "Library" section has every key.

## List, index and check

```
manfred library list [--kind <kind>] [--library <dir>] [--json]
manfred library index [--library <dir>] [--json]
manfred library check [--library <dir>] [--json]
```

- `list` prints each object from `index.json`: kind and name, triangles, size in metres and tags; `--json` gives `{ library, objects: [...] }` with the index's entries.
- `index` rewrites `index.json` from the folders, sorted by kind and name, with each file's sha256; the same folders give the same bytes. `--json` gives `{ index, objects }`.
- `check` validates every `object.toml` (its tables, its kind and name against its folder, and the licence line `MIT, Catalin Ilinca; original, no third-party models or brand marks`), every `model.glb` (it parses, its first root is named after the object, it carries none of Manfred's own extras), and that `index.json` matches the folders and their sha256s. When `gltf_validator` (the Khronos glTF-Validator) is on the PATH it also validates each model and reports any error; without it, it says so on stderr. It prints each problem as `<path>: <text>` and exits 1 when there is one; `--json` gives `{ library, objects, validator, problems: [{ path, text }] }`.

Without `--library`, each command uses the nearest `library/` with an `index.json` from the current folder up; `index` also takes a `library/` folder without one yet.
