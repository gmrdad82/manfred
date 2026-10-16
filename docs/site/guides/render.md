---
title: "Path-trace a still"
description: "Render a recipe's camera on the GPU path tracer as a final or a draft, in PNG, EXR or HDR, whole or cropped, with progress as JSON."
sidebar:
  order: 4
---

`manfred render` path-traces a recipe through a camera on the engine's GPU tracer (hardware ray queries, adaptive sampling, a port of Intel Open Image Denoise). It stages the same parts, materials, sky, sun and lights the live renderer draws, so a render and a `manfred shot` of one recipe show one scene.

![Manfred's window on the hero scene: the Render button path-traces the hero camera, and the finished render fills the viewport.](../images/render.gif)

## A final and a draft

```sh
manfred render showcase/hero.toml --camera hero --out hero.png
manfred render showcase/hero.toml --camera hero --draft --out hero-draft.png
```

A final traces each pixel until its noise is under 0.01, at least 16 samples and at most 64, then denoises. `--draft` is a quick look at the same size: at most 12 samples, threshold 0.05, 6 first. On a Radeon RX 9060 XT a 4K final of the workshop's hero camera takes 7.9 s and a 4K draft 3.8 s.

stdout gets the file written; stderr gets a summary:

```
$ manfred render desk.toml --camera main --draft --size 800x500 --out draft.png
manfred render: 800x500, 7.3 samples a pixel on average, 12 at most of 12, threshold 0.05, seed 0: 19 submissions, the longest 5.87 ms, 31 ms on the GPU
draft.png
```

## A moment of the timeline

```sh
manfred render chase.toml --camera chase --time 2.5 --draft --out chase-2.5.png
manfred shot chase.toml --camera chase --time 2.5 --size 540x960 --out chase-2.5-live.png
```

A recipe with a `[timeline]` poses at `--time <seconds>`: every animated object and camera is where its keys put it at that time, interpolated between frames. `--time` without a `[timeline]`, or outside 0 to its length, is refused before the GPU queue.

## Samples and noise

| Flag | Default | Meaning |
| --- | --- | --- |
| `--spp N` | the recipe's `[trace] spp`, else 64 | the most samples any pixel takes |
| `--threshold T` | the recipe's `[trace] threshold`, else 0.01 | a pixel stops once its noise is under this; `0` runs every pixel to `--spp` |
| `--min-samples N` | the recipe's `[trace] min_samples`, else 16 | samples every pixel takes before it may stop |
| `--seed S` | the recipe's `seed` | the tracer's seed |
| `--denoise off\|on` | on for `.png` and a master | off writes the raw trace |

A recipe can set its own defaults:

```toml
[trace]
spp = 128
threshold = 0.05
filter_glossy = 0.25
```

`filter_glossy` and `clamp_indirect` tame fireflies under a small sun; [the recipe](../../reference/recipe/) has the ranges.

## Formats and sizes

- `--out` picks the format by its extension: `.png` (8-bit, dithered, through the recipe's `[look]`), `.exr` or `.hdr` (linear, unfinished).
- `--master <file.png>` also writes a 16-bit PNG of the same finished picture.
- `--size WxH` sets the frame (1586x992 by default); `--equirect` renders the full sphere around the camera (2048x1024 by default) and `--ortho` an orthographic view of the camera's `ortho_height`.

## A crop of a bigger frame

`--crop x,y,w,h` traces only that rectangle of the `--size` frame, with each pixel keeping its place in the full frame's random stream, ray and lens:

```
$ manfred render desk.toml --camera main --size 800x500 --crop 200,100,200,150 --out crop.png
manfred render: 200x150 at 200,100 of 800x500, 41.0 samples a pixel on average, 64 at most of 64, threshold 0.01, seed 0: 23 submissions, the longest 5.83 ms, 33 ms on the GPU
crop.png
```

With `--denoise off`, or as `.exr` or `.hdr`, a crop has the full frame's pixels exactly. Denoised, it is within a level or two: the denoiser sets its exposure from the picture it is given. A `.png` crop is refused when the recipe's finish cannot be worked out on a margin (overlay text, grain, outlines).

## Progress for a program

`--progress json` writes one JSON object a line on stderr, with a `stage` of `stage`, `trace`, `write` and `done`:

```
{"elapsed_s":0.0,"stage":"stage"}
{"cap":64,"elapsed_s":0.1,"eta_s":null,"fraction":0.0,"gpu_s":0.0,"stage":"trace","threshold":0.01}
{"elapsed_s":0.5,"stage":"write"}
{"elapsed_s":0.5,"gpu_s":0.2,"mean_samples":37.9,"most_samples":64,"out":"final.png","stage":"done"}
```

[File formats](../../reference/formats/) lists every field.

## The GPU queue

When [pgpu](https://docs.pitomd.com/pgpu/) is on the `PATH`, `manfred render` queues itself as a pgpu job and waits its turn; without it, it renders at once, paced the same way, so it never holds the desktop's GPU for long. A GPU below the tracer's floor is refused in one line, exit 1.

## Same input, same picture

The tracer reads no clock and no system randomness: the same recipe, flags and seed give the same pixels on the same GPU and driver.
