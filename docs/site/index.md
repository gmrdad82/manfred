---
title: "Manfred"
description: "A lighter Blender, part of PITO MD. 3D scenes written as TOML recipes, built into GLB headless, arranged in a live window and path-traced on the GPU."
sidebar:
  order: 0
---

![Manfred's window playing a tour: the picker opens the workshop scene, a click picks the vase and makes it brass, the hour slider brings the sun down to evening, the lamp slides along x, and Render path-traces the hero camera.](images/tour.gif)

Manfred is a lighter Blender, part of [PITO MD](https://pitomd.com): 3D scenes written as TOML recipes, built into GLB headless, arranged and lit in a live window, and path-traced into stills and clips on the GPU. No Blender anywhere: one 39.8 MB binary does it all.

- **31× smaller than Blender:** 39.8 MB, one file, against Blender 5.2.2's 1,236.5 MB in 5,593 files.
- **Recipe to GLB in 29 ms:** the 19-object workshop scene, 13 MB of memory at most, the same bytes on every run.
- **Window to first frame in 0.88 s,** the workshop scene loaded, built and drawn.
- **A 4K path-traced final in 7.9 s,** adaptive to 64 samples and denoised; a 4K draft in 3.8 s.

Every number is measured on a Ryzen 9 9950X with a Radeon RX 9060 XT on Linux.

![A path-traced still life on a blueprint-blue backdrop under a low warm sun: a blue lacquer case with a glowing display, a chrome ball, an amber capsule, a porcelain vase on an oak plinth, a wine glass and a paper note.](images/hero.png)

## What it does

You describe a scene in a TOML recipe: its objects and how they are built, where they sit, the cameras, lights, sky, text and materials. Manfred builds it, shows it, lets you arrange it, and renders it, and the recipe stays the one source of truth. It is for people and AI agents alike, made with agentic development in mind: building 3D scenes as text (a game's set dressing, product shots, app icons and cards). Recipes diff and review like code, so a scene can live in a repository next to the program that uses it.

It is not a modeller: there is no sculpting and no hand mesh editing. Meshes come from Manfred's primitives (bevelled boxes, spheres, cylinders, tori, capsules, lathes, sweeps, pockets, cards and 3D text), from 2.5D outlines, or from generators an app writes in Rust.

| Command | What it does |
| --- | --- |
| `manfred check` | lists a recipe's problems at their line |
| `manfred build` | a recipe to a GLB and a JSON sidecar, headless, no GPU |
| `manfred diff` | compares two GLBs by structure, or draws both and compares them by PSNR |
| `manfred shot` | one still on the live renderer, headless |
| `manfred render` | a still path-traced on the GPU: PNG, EXR or HDR |
| `manfred clip` | a camera moving along a path, as frames and a video |
| `manfred gui` | the window: pick, move, re-material, set cameras, render |
| `manfred route` | lists a recipe's routes, or moves, adds and removes their points in the file |
| `manfred set` | writes values into a recipe's file by dotted key, keeping its comments, and checks it |
| `manfred get` | prints a recipe's values by dotted key |
| `manfred new street` | writes a starter chase-shot recipe from a seed, with its lilac grade in lilac.cube beside it |
| `manfred timeline` | a recipe's timeline: frames, keyed nodes, the camera node, what travels how far and how fast |
| `manfred library` | lists, indexes and checks the object library: standard glTF models with a sidecar, which a recipe places by name |
| `manfred shoot` | a timeline's frames rendered by `pfx shot` on the live renderer, with its WAV and a manifest, resumable |

## Where to start

- [Install](install/) Manfred and run the quick start.
- [Your first scene](guides/first-scene/): write a recipe, check it, build it, draw it and trace it.
- [The recipe](reference/recipe/) is the whole schema, and [the command line](reference/cli/) every command and flag.
- [Your app's own generators](guides/app-generators/): link the `manfred` library and add meshes only your app knows. Its [API reference](api/manfred/index.html) is generated from the code.

Manfred is built on [pfx](https://docs.pitomd.com/pfx/), PITO MD's engine: its live renderer draws the window, its tracer renders the stills, and a game loads the GLBs Manfred builds.
