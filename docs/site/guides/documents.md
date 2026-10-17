---
title: "Keep a scene in one .manf"
description: "Pack a recipe, its files, its build and its pfx scene into one .manf document, work on it with every command, and fold back the edits pfx makes."
sidebar:
  order: 3
---

A `.manf` is Manfred's document, as a `.blend` is Blender's: one file with the recipe, its includes and every file it reads (LUTs, HDRs, PNG maps, mesh GLBs), the built glTF, the pfx scene built from it, the facts of every object, a map of the glTF nodes and an optional thumbnail.

It is a private container that only Manfred and pfx open, built for speed. A chunk table sits at the front, so a reader takes the thumbnail or the facts without touching the meshes, and an unchanged document opens without a rebuild. It is not a zip, which would make every open inflate and scan it, and not a GLB, which has no room for the recipe that made it. Other tools such as Blender exchange GLB files with Manfred, never `.manf`. The layout is in [the formats reference](../../reference/formats/#the-manf-file).

## Save, unpack and build

```
manfred save showcase/chase.toml --manf chase.manf [--thumb] [--json]
manfred unpack chase.manf chase/ [--json]
manfred build chase.manf
```

`save` loads and builds the recipe, records every file the load, the build and the scene read, and writes them into the document with the recipe, named by their paths from the nearest folder that holds them all. `--thumb` draws a 320 × 200 thumbnail through the recipe's first camera. `unpack` writes the recipe and its files back under a folder at the same paths, so `manfred build` there gives the same GLB bytes.

`manfred build chase.manf` writes the document's GLB without building while the recipe and its files are the ones it was built from, and says so on stderr; after a change it builds once and rewrites the document. `--variant`, `--all-variants` and `--set` build the document's recipe like any other recipe and leave the document as it is.

## Every command takes a .manf

`check`, `route`, `timeline`, `shot`, `render`, `clip`, `shoot` and `gui` take a `.manf` wherever they take a recipe. Manfred reads the recipe from the document and resolves its paths from the document's files in memory, with no temporary files, as if the files sat in the `.manf`'s folder: outputs land there, as they would beside the unpacked recipe.

A command that edits the recipe, such as `manfred route chase.manf set --route main --at 2 --point 4,0,30`, writes the edit and a fresh build back into the `.manf` in one atomic replace. The window writes its edits back into the document when it closes.

## When pfx edits the scene

The document carries two sources of truth: the recipe, Manfred's, and the scene, pfx's. pfx may edit the scene, never the recipe. Manfred records a hash of the scene it built; when the scene inside a `.manf` no longer matches it, every command refuses to rebuild over the edits in one line naming `manfred fold`:

```
manfred fold chase.manf            # list the edits by scene path
manfred fold chase.manf --apply    # fold them into the recipe
manfred fold chase.manf --keep     # keep them as overrides
```

`--apply` writes each edit whose scene path has a recipe key (a light's intensity, colour, position, radius, range, direction, cone or shadow) into the recipe, in the file that holds it, with the same patch code as every recipe edit; an edit with no recipe key, such as a generated object's material, stays as an override, and the list says which. `--keep` keeps every edit as an override. Overrides apply on top of every later build's scene, so the edit survives the next rebuild. `--json` prints the edits, each with its scene file, path, old and new value and recipe key.
